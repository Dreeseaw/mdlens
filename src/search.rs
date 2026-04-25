use anyhow::Result;
use regex::Regex;
use std::fs;
use std::path::Path;

use crate::model::Section;
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
        let doc = crate::parse::parse_markdown(file_path)?;
        let lines: Vec<String> = fs::read_to_string(file_path)?
            .lines()
            .map(|l| l.to_string())
            .collect();

        let results = search_document(&doc, &lines, query, case_sensitive, use_regex, context_lines)?;
        all_results.extend(results);
    }

    // Sort by match count descending, then by token estimate ascending
    all_results.sort_by(|a, b| {
        b.match_count
            .cmp(&a.match_count)
            .then(a.token_estimate.cmp(&b.token_estimate))
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
    let mut results: Vec<SearchResult> = Vec::new();

    // Build regex pattern
    let pattern = if use_regex {
        query.to_string()
    } else {
        regex::escape(query)
    };

    let regex = if case_sensitive {
        Regex::new(&pattern)?
    } else {
        Regex::new(&format!("(?i){}", pattern))?
    };

    // Search each line and group by section
    let mut section_matches: std::collections::HashMap<String, Vec<MatchLine>> =
        std::collections::HashMap::new();

    for (line_idx, line) in lines.iter().enumerate() {
        let line_num = line_idx + 1;
        if regex.is_match(line) {
            // Find which section this line belongs to
            if let Some(section) = find_section_for_line(&doc.sections, line_num) {
                section_matches
                    .entry(section.id.clone())
                    .or_default()
                    .push(MatchLine {
                        line_num,
                    });
            }
        }
    }

    for (section_id, matches) in section_matches {
        if let Some(section) = doc.find_section_by_id(&section_id) {
            let snippets = build_snippets(&matches, context_lines, lines);
            results.push(SearchResult {
                path: doc.path.clone(),
                section_id: section.id.clone(),
                section_title: section.title.clone(),
                section_path: section.path.clone(),
                line_start: section.line_start,
                line_end: section.line_end,
                token_estimate: section.token_estimate,
                match_count: matches.len(),
                snippets,
            });
        }
    }

    Ok(results)
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
        let end = std::cmp::min(
            match_line.line_num + context_lines,
            lines.len(),
        );

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

/// Find which section contains a given line number.
fn find_section_for_line(sections: &[Section], line_num: usize) -> Option<&Section> {
    for section in sections {
        if line_num >= section.line_start && line_num <= section.line_end {
            return Some(section);
        }
        if let Some(child) = find_section_for_line(&section.children, line_num) {
            return Some(child);
        }
    }
    None
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
