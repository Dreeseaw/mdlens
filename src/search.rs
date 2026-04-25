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

    all_results.sort_by(|lhs, rhs| {
        rhs.match_count
            .cmp(&lhs.match_count)
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

/// Discover markdown files in a directory or return a single file.
pub fn discover_markdown_files(root: &str) -> Result<Vec<String>> {
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

    Ok(files)
}
