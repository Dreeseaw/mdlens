use anyhow::Result;
use std::fs;

use crate::model::{Document, PreambleSection, Section};
use crate::tokens::{count_words, estimate_tokens};

/// Parse a Markdown file into a Document with section tree.
pub fn parse_markdown(path: &str) -> Result<Document> {
    let content = fs::read_to_string(path)?;
    parse_markdown_str(path, &content)
}

/// Parse Markdown content string into a Document.
pub fn parse_markdown_str(path: &str, content: &str) -> Result<Document> {
    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let line_count = lines.len();
    let byte_count = content.len();
    let char_count = content.chars().count();
    let word_count = count_words(content);
    let token_estimate = estimate_tokens(content);

    // Detect and skip YAML frontmatter
    let content_start_line = detect_frontmatter_end(&lines);

    // Build section tree from line scanning
    let sections = build_section_tree(&lines, content_start_line);

    // Compute per-section stats
    let sections = compute_section_stats(&lines, sections);

    Ok(Document {
        path: path.to_string(),
        line_count,
        byte_count,
        char_count,
        word_count,
        token_estimate,
        sections,
    })
}

/// Detect YAML frontmatter and return the line index after it.
fn detect_frontmatter_end(lines: &[String]) -> usize {
    if lines.len() >= 3 && lines[0].trim() == "---" {
        for i in 1..lines.len() {
            if lines[i].trim() == "---" {
                return i + 1;
            }
        }
    }
    0
}

/// Build the section tree from Markdown content by scanning lines for ATX headings.
fn build_section_tree(lines: &[String], content_start: usize) -> Vec<Section> {
    // Scan lines for ATX headings, skipping fenced code blocks
    let mut headings: Vec<HeadingInfo> = Vec::new();
    let mut in_fenced_code = false;

    for (idx, line) in lines.iter().enumerate() {
        let line_num = idx + 1; // 1-indexed
        let trimmed = line.trim();

        // Track fenced code blocks
        if trimmed.starts_with("```") {
            in_fenced_code = !in_fenced_code;
            continue;
        }

        if in_fenced_code {
            continue;
        }

        // Check for ATX headings
        if let Some((level, title)) = parse_atx_heading(trimmed) {
            let byte_start = byte_offset_for_line(lines, idx);
            headings.push(HeadingInfo {
                level,
                title,
                line: line_num,
                byte_offset: byte_start,
            });
        }
    }

    build_tree_from_headings(&headings, lines, content_start)
}

struct HeadingInfo {
    level: u8,
    title: String,
    line: usize,
    byte_offset: usize,
}

fn parse_atx_heading(line: &str) -> Option<(u8, String)> {
    if !line.starts_with('#') {
        return None;
    }
    let mut level = 0u8;
    for ch in line.chars() {
        if ch == '#' {
            level += 1;
        } else {
            break;
        }
    }
    if level == 0 || level > 6 {
        return None;
    }

    let rest = &line[level as usize..];
    let rest = rest.trim_start();
    // Remove trailing # characters
    let rest = if rest.ends_with('#') {
        rest.trim_end_matches('#').trim_end()
    } else {
        rest
    };

    if rest.is_empty() {
        return None;
    }

    Some((level, rest.to_string()))
}

fn byte_offset_for_line(lines: &[String], line_idx: usize) -> usize {
    let mut offset = 0;
    for i in 0..line_idx {
        offset += lines[i].len() + 1; // +1 for newline
    }
    offset
}

/// Build a hierarchical section tree from flat heading list.
fn build_tree_from_headings(
    headings: &[HeadingInfo],
    lines: &[String],
    content_start: usize,
) -> Vec<Section> {
    if headings.is_empty() {
        // No headings: create a preamble section covering the whole file
        let preamble_end = if lines.is_empty() { 0 } else { lines.len() };
        let preamble_content = if content_start > 0 {
            &lines[content_start..]
        } else {
            lines
        };

        if !preamble_content.is_empty() {
            let preamble_text = preamble_content.join("\n");
            let char_count = preamble_text.chars().count();
            let word_count = count_words(&preamble_text);
            let token_estimate = estimate_tokens(&preamble_text);

            let byte_start = byte_offset_for_line(lines, content_start);
            let byte_end = lines.iter().map(|l| l.len() + 1).sum();

            vec![PreambleSection {
                id: "0".to_string(),
                slug: "preamble".to_string(),
                title: "<preamble>".to_string(),
                level: 1,
                path: vec!["<preamble>".to_string()],
                line_start: 1,
                line_end: preamble_end,
                content_line_start: 1,
                byte_start,
                byte_end,
                char_count,
                word_count,
                token_estimate,
            }
            .into()]
        } else {
            Vec::new()
        }
    } else {
        // Check if there's content before the first heading
        let first_heading_line = headings[0].line;
        let mut sections = Vec::new();

        if first_heading_line > 1 || content_start > 0 {
            let preamble_end = first_heading_line - 1;
            let preamble_lines = &lines[..preamble_end];
            if !preamble_lines.is_empty() {
                let preamble_text = preamble_lines.join("\n");
                let char_count = preamble_text.chars().count();
                let word_count = count_words(&preamble_text);
                let token_estimate = estimate_tokens(&preamble_text);

                sections.push(
                    PreambleSection {
                        id: "0".to_string(),
                        slug: "preamble".to_string(),
                        title: "<preamble>".to_string(),
                        level: 1,
                        path: vec!["<preamble>".to_string()],
                        line_start: 1,
                        line_end: preamble_end,
                        content_line_start: 1,
                        byte_start: 0,
                        byte_end: byte_offset_for_line(lines, preamble_end),
                        char_count,
                        word_count,
                        token_estimate,
                    }
                    .into(),
                );
            }
        }

        // Build hierarchical tree from headings
        let root_sections = build_hierarchy(headings, lines);
        sections.extend(root_sections);
        sections
    }
}

/// Build hierarchical section tree using a stack-based approach with proper dotted IDs.
fn build_hierarchy(headings: &[HeadingInfo], lines: &[String]) -> Vec<Section> {
    let total_lines = lines.len();

    // Assign line ends: each section extends to the line before the next heading
    // at same or higher level (lower number), or to end of file.
    let mut line_ends = Vec::with_capacity(headings.len());
    for i in 0..headings.len() {
        let end_line = if i + 1 < headings.len() {
            headings[i + 1].line - 1
        } else {
            total_lines
        };
        line_ends.push(end_line);
    }

    // Stack-based tree building with dotted IDs
    struct StackEntry {
        section: Section,
        id_parts: Vec<usize>,
    }

    let mut root_sections: Vec<Section> = Vec::new();
    let mut stack: Vec<StackEntry> = Vec::new();

    for (i, heading) in headings.iter().enumerate() {
        // Pop entries from stack until we find a parent with lower level
        while !stack.is_empty() && stack.last().unwrap().section.level >= heading.level {
            let popped = stack.pop().unwrap();
            if stack.is_empty() {
                root_sections.push(popped.section);
            } else {
                stack.last_mut().unwrap().section.children.push(popped.section);
            }
        }

        // Compute dotted ID based on stack depth
        let parent_parts = stack.last().map(|e| e.id_parts.clone()).unwrap_or_default();
        let last_child_idx = stack
            .last()
            .map(|e| {
                e.section
                    .children
                    .len()
            })
            .unwrap_or(0);
        let mut id_parts = parent_parts;
        id_parts.push(last_child_idx + 1);
        let id = id_parts
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(".");

        let line_start = heading.line;
        let line_end = line_ends[i];
        let byte_start = heading.byte_offset;
        let byte_end = if i + 1 < headings.len() {
            byte_offset_for_line(lines, headings[i + 1].line - 1)
        } else {
            lines.iter().map(|l| l.len() + 1).sum()
        };

        // Build path from stack
        let path: Vec<String> = stack
            .iter()
            .map(|e| e.section.title.clone())
            .chain(std::iter::once(heading.title.clone()))
            .collect();

        let section = Section {
            id: id.clone(),
            slug: Section::slugify(&heading.title),
            title: heading.title.clone(),
            level: heading.level,
            path,
            line_start,
            line_end,
            content_line_start: line_start + 1,
            byte_start,
            byte_end,
            char_count: 0,
            word_count: 0,
            token_estimate: 0,
            children: Vec::new(),
        };

        stack.push(StackEntry {
            section,
            id_parts,
        });
    }

    // Pop remaining sections
    while !stack.is_empty() {
        let popped = stack.pop().unwrap();
        if stack.is_empty() {
            root_sections.push(popped.section);
        } else {
            stack.last_mut().unwrap().section.children.push(popped.section);
        }
    }

    root_sections
}

/// Recompute section stats (char_count, word_count, token_estimate) from actual content.
fn compute_section_stats(lines: &[String], mut sections: Vec<Section>) -> Vec<Section> {
    for section in &mut sections {
        compute_section_stats_recursive(lines, section);
    }
    sections
}

fn compute_section_stats_recursive(lines: &[String], section: &mut Section) {
    let content = section.extract_content(lines);
    let text = content.join("\n");
    section.char_count = text.chars().count();
    section.word_count = count_words(&text);
    section.token_estimate = estimate_tokens(&text);

    for child in &mut section.children {
        compute_section_stats_recursive(lines, child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_atx_heading() {
        let result = parse_atx_heading("# Hello");
        assert_eq!(result.map(|(_, t)| t), Some("Hello".to_string()));
        let result = parse_atx_heading("## World");
        assert_eq!(result.map(|(l, _)| l), Some(2));
        let result = parse_atx_heading("### Deep");
        assert_eq!(result, Some((3, "Deep".to_string())));
        assert_eq!(parse_atx_heading("not a heading"), None);
        assert_eq!(parse_atx_heading("##"), None);
    }

    #[test]
    fn test_parse_simple() {
        let content = "# Overview\n\nSome text.\n\n## Install\n\nInstall it.\n\n## Usage\n\nUse it.\n";
        let doc = parse_markdown_str("test.md", content).unwrap();
        assert_eq!(doc.line_count, 11);
        // Overview (level 1) is the root, Install and Usage (level 2) are its children
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].title, "Overview");
        assert_eq!(doc.sections[0].children.len(), 2);
        assert_eq!(doc.sections[0].children[0].title, "Install");
        assert_eq!(doc.sections[0].children[1].title, "Usage");
    }

    #[test]
    fn test_parse_with_preamble() {
        let content = "Some intro text.\n\n# Heading\n\nContent.\n";
        let doc = parse_markdown_str("test.md", content).unwrap();
        assert_eq!(doc.sections.len(), 2); // preamble + Heading
        assert_eq!(doc.sections[0].id, "0");
        assert_eq!(doc.sections[0].title, "<preamble>");
        assert_eq!(doc.sections[1].id, "1");
        assert_eq!(doc.sections[1].title, "Heading");
    }

    #[test]
    fn test_parse_no_headings() {
        let content = "Just some text.\nNo headings here.\n";
        let doc = parse_markdown_str("test.md", content).unwrap();
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].id, "0");
        assert_eq!(doc.sections[0].title, "<preamble>");
    }

    #[test]
    fn test_parse_code_block_headings() {
        let content = "# Real Heading\n\n```\n# Fake Heading\n```\n\n## Real Child\n\nContent.\n";
        let doc = parse_markdown_str("test.md", content).unwrap();
        // Should have 2 sections: Real Heading and Real Child (Fake Heading ignored)
        let total_sections = count_sections(&doc.sections);
        assert_eq!(total_sections, 2);
    }

    #[test]
    fn test_parse_duplicate_headings() {
        let content = "# Report\n\n## Results\n\nFirst.\n\n## Results\n\nSecond.\n";
        let doc = parse_markdown_str("test.md", content).unwrap();
        // Report has children: Results (1.1) and Results (1.2)
        assert_eq!(doc.sections.len(), 1); // Report
        assert_eq!(doc.sections[0].children.len(), 2);
        assert_eq!(doc.sections[0].children[0].id, "1.1");
        assert_eq!(doc.sections[0].children[1].id, "1.2");
    }

    #[test]
    fn test_parse_heading_jumps() {
        let content = "# A\n\n### B\n\nContent.\n";
        let doc = parse_markdown_str("test.md", content).unwrap();
        // A (level 1) should have B (level 3) as child
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].title, "A");
        assert_eq!(doc.sections[0].children.len(), 1);
        assert_eq!(doc.sections[0].children[0].title, "B");
    }

    #[test]
    fn test_parse_frontmatter() {
        let content = "---\ntitle: Test\n---\n\n# Heading\n\nContent.\n";
        let doc = parse_markdown_str("test.md", content).unwrap();
        // Frontmatter should be part of preamble
        assert!(doc.sections.iter().any(|s| s.title == "<preamble>" || s.title == "Heading"));
    }

    fn count_sections(sections: &[Section]) -> usize {
        let mut count = sections.len();
        for s in sections {
            count += count_sections(&s.children);
        }
        count
    }
}
