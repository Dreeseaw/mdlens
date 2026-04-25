use anyhow::Result;
use regex::Regex;
use std::collections::BTreeMap;
use std::path::Path;

use crate::model::Section;
use crate::parse::load_markdown;
use crate::render::{SearchResult, SearchSnippet};

/// Search markdown files for a query and return section-level results.
pub fn search_files(
    root: &str,
    query: &str,
    case_sensitive: bool,
    use_regex: bool,
    max_results: usize,
    context_lines: usize,
    canonical_only: bool,
) -> Result<Vec<SearchResult>> {
    let files = discover_markdown_files_with_mode(root, canonical_only)?;
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
    let lower = path.to_ascii_lowercase();
    let file = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase();
    let stem = Path::new(path)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase();

    let mut score = 0i32;
    if file == "sgocr_champion.md" {
        score += 140;
    } else if file == "champion.md" {
        score += 120;
    }
    if file == "cleo_state.md" || stem.ends_with("_state") || file == "research_state.md" {
        score += 110;
    }
    if file.starts_with("00_") || lower.contains("orientation") {
        score += 95;
    }
    if file.starts_with("01_") || lower.contains("benchmark_protocol") {
        score += 90;
    }
    if lower.contains("global_task_context") {
        score += 85;
    }
    if lower.contains("experiment_db") {
        score += 80;
    }
    if is_dated_doc(&file) {
        score -= 25;
    }
    score
}

fn is_canonical_doc_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let file = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase();
    let stem = Path::new(path)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase();

    file == "sgocr_champion.md"
        || file == "champion.md"
        || file == "cleo_state.md"
        || file == "research_state.md"
        || stem.ends_with("_state")
        || file.starts_with("00_")
        || file.starts_with("01_")
        || lower.contains("orientation")
        || lower.contains("benchmark_protocol")
        || lower.contains("global_task_context")
        || lower.contains("experiment_db")
}

fn section_priority(path: &[String]) -> i32 {
    let joined = path.join(" > ").to_ascii_lowercase();
    let mut score = 0i32;
    for marker in [
        "source of truth",
        "current champion",
        "champion",
        "active hypotheses",
        "benchmark protocol",
        "metric",
        "formula",
        "operational score",
        "precision_first_score",
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
    let mut parts = file_name.split('_');
    if let Some(first) = parts.next() {
        if !first.is_empty() && first.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    file_name.contains("2026-")
        || file_name.contains("2025-")
        || file_name.contains("2024-")
        || file_name.contains("2023-")
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
    use super::{is_canonical_doc_path, is_dated_doc, section_priority, source_priority};

    #[test]
    fn source_priority_prefers_canonical_docs() {
        assert!(
            source_priority("tasks/mm_bridge/docs/SGOCR_CHAMPION.md")
                > source_priority("tasks/mm_bridge/docs/124_sgocr_dev200_coverage_and_eval_analysis_2026-04-06.md")
        );
        assert!(
            source_priority("tasks/vlm_cleo/docs/CLEO_STATE.md")
                > source_priority(
                    "tasks/vlm_cleo/docs/14_vlm_cleo_cleo423dinner_turn_2026-04-23.md"
                )
        );
    }

    #[test]
    fn dated_docs_are_penalized() {
        assert!(is_dated_doc(
            "124_sgocr_dev200_coverage_and_eval_analysis_2026-04-06.md"
        ));
        assert!(!is_dated_doc("SGOCR_CHAMPION.md"));
    }

    #[test]
    fn section_priority_prefers_formula_like_headings() {
        let formula = vec![
            "Source of Truth".to_string(),
            "precision_first_score".to_string(),
        ];
        let generic = vec!["Misc Notes".to_string()];
        assert!(section_priority(&formula) > section_priority(&generic));
    }

    #[test]
    fn canonical_doc_filter_prefers_source_of_truth_docs() {
        assert!(is_canonical_doc_path(
            "tasks/mm_bridge/docs/SGOCR_CHAMPION.md"
        ));
        assert!(is_canonical_doc_path("tasks/vlm_cleo/docs/CLEO_STATE.md"));
        assert!(is_canonical_doc_path(
            "tasks/mm_bridge/docs/01_benchmark_protocol.md"
        ));
        assert!(!is_canonical_doc_path(
            "tasks/mm_bridge/docs/124_sgocr_dev200_coverage_and_eval_analysis_2026-04-06.md"
        ));
    }
}
