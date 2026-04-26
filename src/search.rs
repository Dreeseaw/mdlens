use anyhow::Result;
use regex::Regex;
use std::collections::BTreeMap;
use std::path::Path;

use crate::model::Section;
use crate::parse::load_markdown;
use crate::render::{SearchResult, SearchSnippet};

/// Search markdown files for a query and return section-level results.
/// Results are sorted with canonical/source-of-truth docs ranked first.
pub fn search_files(
    root: &str,
    query: &str,
    case_sensitive: bool,
    use_regex: bool,
    max_results: usize,
    context_lines: usize,
) -> Result<Vec<SearchResult>> {
    let files = discover_markdown_files(root)?;
    let mut all_results: Vec<SearchResult> = Vec::new();

    for file_path in &files {
        let parsed = load_markdown(file_path)?;
        let results = search_document(
            &parsed.doc,
            &parsed.lines,
            query,
            case_sensitive,
            use_regex,
            context_lines,
        )?;
        all_results.extend(results);
    }

    // Agent queries are often natural phrases ("headline metric", "rank experiments")
    // while docs may use nearby but non-identical wording. If an exact plain-text
    // phrase has no hits, retry with query terms so the model gets a useful map
    // instead of falling back to expensive whole-directory tree reads.
    if all_results.is_empty() && !use_regex {
        if let Some(token_pattern) = token_fallback_pattern(query) {
            for file_path in &files {
                let parsed = load_markdown(file_path)?;
                let results = search_document(
                    &parsed.doc,
                    &parsed.lines,
                    &token_pattern,
                    case_sensitive,
                    true,
                    context_lines,
                )?;
                all_results.extend(results);
            }
        }
    }

    all_results.sort_by(|lhs, rhs| {
        source_priority(&rhs.path)
            .cmp(&source_priority(&lhs.path))
            .then(section_priority(&rhs.section_path).cmp(&section_priority(&lhs.section_path)))
            .then(rhs.match_count.cmp(&lhs.match_count))
            .then(path_depth(&lhs.path).cmp(&path_depth(&rhs.path)))
            .then(lhs.token_estimate.cmp(&rhs.token_estimate))
            .then(lhs.path.cmp(&rhs.path))
            .then(lhs.line_start.cmp(&rhs.line_start))
            .then(lhs.section_id.cmp(&rhs.section_id))
    });

    Ok(all_results.into_iter().take(max_results).collect())
}

fn token_fallback_pattern(query: &str) -> Option<String> {
    let tokens: Vec<String> = query
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .map(str::trim)
        .filter(|token| token.len() >= 3)
        .map(regex::escape)
        .collect();

    if tokens.len() < 2 {
        return None;
    }

    Some(tokens.join("|"))
}

/// Search within a single document.
fn search_document(
    doc: &crate::model::Document,
    lines: &[String],
    query: &str,
    case_sensitive: bool,
    use_regex: bool,
    context_lines: usize,
) -> Result<Vec<SearchResult>> {
    let pattern = if use_regex {
        query.to_string()
    } else {
        regex::escape(query)
    };

    let regex = if case_sensitive {
        Regex::new(&pattern)?
    } else {
        Regex::new(&format!("(?i){pattern}"))?
    };

    let ordered_sections = flatten_sections(&doc.sections);
    let mut active_sections: Vec<&Section> = Vec::new();
    let mut next_section_idx = 0usize;
    let mut section_matches: BTreeMap<String, Vec<MatchLine>> = BTreeMap::new();

    for (line_idx, line) in lines.iter().enumerate() {
        let line_num = line_idx + 1;

        while active_sections
            .last()
            .is_some_and(|section| section.line_end < line_num)
        {
            active_sections.pop();
        }

        while next_section_idx < ordered_sections.len()
            && ordered_sections[next_section_idx].line_start == line_num
        {
            active_sections.push(ordered_sections[next_section_idx]);
            next_section_idx += 1;
        }

        if regex.is_match(line) {
            if let Some(section) = active_sections.last() {
                section_matches
                    .entry(section.id.clone())
                    .or_default()
                    .push(MatchLine { line_num });
            }
        }
    }

    let mut results = Vec::new();
    for (section_id, matches) in section_matches {
        if let Some(section) = doc.find_section_by_id(&section_id) {
            results.push(SearchResult {
                path: doc.path.clone(),
                section_id: section.id.clone(),
                section_title: section.title.clone(),
                section_path: section.path.clone(),
                line_start: section.line_start,
                line_end: section.line_end,
                token_estimate: section.token_estimate,
                match_count: matches.len(),
                snippets: build_snippets(&matches, context_lines, lines),
                body: None,
                preview: None,
            });
        }
    }

    Ok(results)
}

fn flatten_sections(sections: &[Section]) -> Vec<&Section> {
    let mut ordered = Vec::new();
    collect_sections(sections, &mut ordered);
    ordered.sort_by(|lhs, rhs| {
        lhs.line_start
            .cmp(&rhs.line_start)
            .then(lhs.level.cmp(&rhs.level))
            .then(lhs.id.cmp(&rhs.id))
    });
    ordered
}

fn collect_sections<'a>(sections: &'a [Section], ordered: &mut Vec<&'a Section>) {
    for section in sections {
        ordered.push(section);
        collect_sections(&section.children, ordered);
    }
}

fn build_snippets(
    matches: &[MatchLine],
    context_lines: usize,
    lines: &[String],
) -> Vec<SearchSnippet> {
    let mut snippets = Vec::new();
    for match_line in matches {
        let start = if match_line.line_num > context_lines + 1 {
            match_line.line_num - context_lines - 1
        } else {
            0
        };
        let end = std::cmp::min(match_line.line_num + context_lines, lines.len());

        snippets.push(SearchSnippet {
            line_start: start + 1,
            line_end: end,
            text: lines[start..end].join("\n"),
        });
    }
    snippets
}

struct MatchLine {
    line_num: usize,
}

fn source_priority(path: &str) -> i32 {
    let file = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase();
    let stem_lower = Path::new(path)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase();
    let stem_orig = Path::new(path)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(path);

    let mut score = 0i32;
    // Files named like source docs should beat scratchpads with many repeated
    // keyword hits. This is intentionally convention-based, not eval-specific:
    // real markdown trees often have policies/specs/runbooks plus noisy notes.
    for marker in [
        "policy",
        "runbook",
        "guide",
        "manual",
        "spec",
        "reference",
        "card",
        "schema",
        "protocol",
    ] {
        if stem_lower.contains(marker) {
            score += 45;
        }
    }
    // State/status tracking docs are often the current authoritative record.
    if stem_lower.ends_with("_state")
        || stem_lower.ends_with("_status")
        || stem_lower.ends_with("_current")
        || file == "state.md"
        || file == "status.md"
        || file == "current.md"
    {
        score += 110;
    }
    if stem_lower.contains("canonical")
        || stem_lower.contains("source_of_truth")
        || stem_lower.contains("source-of-truth")
        || stem_lower.contains("authoritative")
    {
        score += 105;
    }
    // Numbered intro docs: 00_ is typically the orientation/index
    if file.starts_with("00_") {
        score += 95;
    }
    // 01_ is typically the first protocol or spec doc
    if file.starts_with("01_") {
        score += 90;
    }
    // Named overview/orientation/readme files are canonical entry points
    if stem_lower.contains("orientation")
        || stem_lower.contains("overview")
        || stem_lower.contains("readme")
        || stem_lower.contains("getting_started")
        || stem_lower.contains("roadmap")
    {
        score += 85;
    }
    // Chat logs and informal discussion files are useful color, not source of truth.
    if stem_lower.contains("chat")
        || stem_lower.contains("discussion")
        || stem_lower.contains("conversation")
        || stem_lower.contains("scratch")
        || stem_lower.contains("tmp")
        || stem_lower.contains("temp")
        || stem_lower.contains("draft")
        || stem_lower.contains("random")
        || stem_lower.contains("copied")
        || stem_lower.contains("copy")
    {
        score -= 180;
    }
    // All-uppercase stems (e.g. README, TRACKER, SPEC) are convention for important docs
    if is_all_caps_stem(stem_orig) {
        score += 20;
    }
    if is_dated_doc(&file) {
        score -= 25;
    }
    score
}

fn is_canonical_doc_path(path: &str) -> bool {
    let file = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase();
    let stem_lower = Path::new(path)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase();
    stem_lower.ends_with("_state")
        || stem_lower.ends_with("_status")
        || stem_lower.ends_with("_current")
        || file == "state.md"
        || file == "status.md"
        || file == "current.md"
        || stem_lower.contains("canonical")
        || stem_lower.contains("source_of_truth")
        || stem_lower.contains("source-of-truth")
        || stem_lower.contains("authoritative")
        || file.starts_with("00_")
        || file.starts_with("01_")
        || stem_lower.contains("orientation")
        || stem_lower.contains("overview")
        || file == "readme.md"
}

fn is_all_caps_stem(stem: &str) -> bool {
    !stem.is_empty()
        && stem
            .chars()
            .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
}

fn section_priority(path: &[String]) -> i32 {
    let joined = path.join(" > ").to_ascii_lowercase();
    let mut score = 0i32;
    for marker in [
        "source of truth",
        "current state",
        "current status",
        "authoritative",
        "canonical",
        "protocol",
        "metric",
        "formula",
        "current loader",
        "do not use",
        "policy",
        "privacy policy",
        "annotation policy",
        "counting rule",
        "safety read",
        "known risk",
        "risk",
        "rule",
        "overview",
        "summary",
        "key",
        // Synthesis / conclusion sections that often contain the decisive finding.
        "interpretation",
        "what this means",
        "what we learned",
        "conclusion",
        "findings",
        "key results",
        "key finding",
        "direction",
        "roadmap",
        "recommendation",
    ] {
        if joined.contains(marker) {
            score += 12;
        }
    }
    score
}

fn path_depth(path: &str) -> usize {
    Path::new(path).components().count()
}

fn is_dated_doc(file_name: &str) -> bool {
    // Detect experiment-counter prefixes (10+) like 14_notes.md, 124_analysis.md.
    // Exempt single-digit and 00/01 prefixes which are conventional ordering.
    let mut parts = file_name.split('_');
    if let Some(first) = parts.next() {
        if !first.is_empty() && first.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(n) = first.parse::<usize>() {
                if n >= 10 {
                    return true;
                }
            }
        }
    }
    file_name.contains("2026-")
        || file_name.contains("2025-")
        || file_name.contains("2024-")
        || file_name.contains("2023-")
}

/// Returns high-value section summaries for sidebar navigation.
///
/// Sidebars are most useful when they expose exact read targets like "Formula",
/// "Run-Level Quality Score", or "Current Champion", even if those sections are
/// nested later in a long document. Ranking all sections beats listing only the
/// first few top-level headings for agent navigation.
pub fn get_doc_section_summaries(path: &str) -> Result<Vec<(String, String)>> {
    let parsed = load_markdown(path)?;
    let mut summaries = Vec::new();
    collect_summary_sections(&parsed.doc.sections, &mut summaries);
    summaries.sort_by(|lhs, rhs| {
        section_priority(&rhs.2)
            .cmp(&section_priority(&lhs.2))
            .then(lhs.2.len().cmp(&rhs.2.len()))
            .then(compare_section_ids(&lhs.0, &rhs.0))
    });
    let summaries = summaries
        .into_iter()
        .map(|(id, title, path)| {
            let label = if path.len() > 2 {
                path[(path.len() - 2)..].join(" > ")
            } else if path.len() > 1 {
                path.last().cloned().unwrap_or(title)
            } else {
                title
            };
            (id, label)
        })
        .collect();
    Ok(summaries)
}

fn collect_summary_sections(
    sections: &[Section],
    summaries: &mut Vec<(String, String, Vec<String>)>,
) {
    for section in sections {
        if section.title != "<preamble>" {
            summaries.push((
                section.id.clone(),
                section.title.clone(),
                section.path.clone(),
            ));
        }
        collect_summary_sections(&section.children, summaries);
    }
}

fn compare_section_ids(lhs: &str, rhs: &str) -> std::cmp::Ordering {
    let lhs_parts = parse_section_id(lhs);
    let rhs_parts = parse_section_id(rhs);
    lhs_parts.cmp(&rhs_parts).then(lhs.cmp(rhs))
}

fn parse_section_id(id: &str) -> Vec<usize> {
    id.split('.')
        .filter_map(|part| part.parse::<usize>().ok())
        .collect()
}

/// Discover markdown files in a directory or return a single file.
pub fn discover_markdown_files(root: &str) -> Result<Vec<String>> {
    discover_markdown_files_with_mode(root, false)
}

pub fn discover_markdown_files_with_mode(root: &str, canonical_only: bool) -> Result<Vec<String>> {
    let path = Path::new(root);
    if path.is_file() {
        return Ok(vec![root.to_string()]);
    }

    let mut files = Vec::new();
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .build();

    for entry in walker {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy().to_string();
        if file_name.ends_with(".md") || file_name.ends_with(".markdown") {
            files.push(entry.path().to_string_lossy().to_string());
        }
    }

    files.sort();
    if canonical_only {
        let canonical: Vec<String> = files
            .iter()
            .filter(|path| is_canonical_doc_path(path))
            .cloned()
            .collect();
        if !canonical.is_empty() {
            return Ok(canonical);
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::{
        is_canonical_doc_path, is_dated_doc, section_priority, source_priority,
        token_fallback_pattern,
    };

    #[test]
    fn source_priority_prefers_canonical_docs() {
        assert!(
            source_priority("docs/SOURCE_OF_TRUTH.md")
                > source_priority("docs/124_analysis_2026-04-06.md")
        );
        assert!(
            source_priority("docs/CURRENT_STATE.md")
                > source_priority("docs/14_dev_notes_2026-04-23.md")
        );
        assert!(
            source_priority("docs/00_orientation.md")
                > source_priority("docs/55_archived_experiment.md")
        );
        assert!(source_priority("docs/roadmap.md") > source_priority("docs/team_chat.md"));
    }

    #[test]
    fn dated_docs_are_penalized() {
        assert!(is_dated_doc("124_analysis_2026-04-06.md"));
        assert!(is_dated_doc("notes_2025-11-01.md"));
        assert!(!is_dated_doc("SOURCE_OF_TRUTH.md"));
        assert!(!is_dated_doc("00_orientation.md"));
    }

    #[test]
    fn section_priority_prefers_formula_like_headings() {
        let formula = vec!["Source of Truth".to_string(), "Formula".to_string()];
        let generic = vec!["Misc Notes".to_string()];
        assert!(section_priority(&formula) > section_priority(&generic));
    }

    #[test]
    fn canonical_doc_filter_prefers_source_of_truth_docs() {
        assert!(is_canonical_doc_path("docs/SOURCE_OF_TRUTH.md"));
        assert!(is_canonical_doc_path("docs/canonical_policy.md"));
        assert!(is_canonical_doc_path("docs/CURRENT_STATE.md"));
        assert!(is_canonical_doc_path("docs/current.md"));
        assert!(is_canonical_doc_path("docs/01_benchmark_protocol.md"));
        assert!(is_canonical_doc_path("docs/00_orientation.md"));
        assert!(is_canonical_doc_path("docs/README.md"));
        assert!(!is_canonical_doc_path("docs/124_analysis_2026-04-06.md"));
        assert!(!is_canonical_doc_path("docs/55_archived_run.md"));
    }

    #[test]
    fn token_fallback_requires_multiword_plain_queries() {
        assert_eq!(
            token_fallback_pattern("headline metric").as_deref(),
            Some("headline|metric")
        );
        assert_eq!(token_fallback_pattern("metric"), None);
        assert_eq!(token_fallback_pattern("to be").as_deref(), None);
    }
}
