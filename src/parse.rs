use anyhow::Result;
use std::fs;

use crate::model::{Document, PreambleSection, Section};
use crate::tokens::{count_words, estimate_tokens};

#[derive(Debug, Clone)]
pub struct ParsedMarkdown {
    pub doc: Document,
    pub lines: Vec<String>,
}

/// Parse a Markdown file and keep both the document tree and source lines.
pub fn load_markdown(path: &str) -> Result<ParsedMarkdown> {
    let content = fs::read_to_string(path)?;
    parse_markdown_content(path, &content)
}

/// Parse a Markdown file into a Document with section tree.
pub fn parse_markdown(path: &str) -> Result<Document> {
    Ok(load_markdown(path)?.doc)
}

/// Parse Markdown content string into a Document.
pub fn parse_markdown_str(path: &str, content: &str) -> Result<Document> {
    Ok(parse_markdown_content(path, content)?.doc)
}

fn parse_markdown_content(path: &str, content: &str) -> Result<ParsedMarkdown> {
    let line_index = build_line_index(content);
    let line_count = line_index.lines.len();
    let byte_count = content.len();
    let char_count = content.chars().count();
    let word_count = count_words(content);
    let token_estimate = estimate_tokens(content);

    let content_start_line = detect_frontmatter_end(&line_index.lines);
    let sections = build_section_tree(
        &line_index.lines,
        &line_index.line_starts,
        line_index.total_bytes,
        content_start_line,
    );
    let sections = compute_section_stats(&line_index.lines, sections);

    Ok(ParsedMarkdown {
        doc: Document {
            path: path.to_string(),
            line_count,
            byte_count,
            char_count,
            word_count,
            token_estimate,
            sections,
        },
        lines: line_index.lines,
    })
}

struct LineIndex {
    lines: Vec<String>,
    line_starts: Vec<usize>,
    total_bytes: usize,
}

fn build_line_index(content: &str) -> LineIndex {
    let mut lines = Vec::new();
    let mut line_starts = Vec::new();
    let mut offset = 0usize;

    for chunk in content.split_inclusive('\n') {
        line_starts.push(offset);
        let line = chunk
            .strip_suffix('\n')
            .unwrap_or(chunk)
            .strip_suffix('\r')
            .unwrap_or_else(|| chunk.strip_suffix('\n').unwrap_or(chunk));
        lines.push(line.to_string());
        offset += chunk.len();
    }

    LineIndex {
        lines,
        line_starts,
        total_bytes: content.len(),
    }
}

/// Detect YAML frontmatter and return the line index after it.
fn detect_frontmatter_end(lines: &[String]) -> usize {
    if lines.len() >= 3 && lines[0].trim() == "---" {
        for (idx, line) in lines.iter().enumerate().skip(1) {
            if line.trim() == "---" {
                return idx + 1;
            }
        }
    }
    0
}

/// Build the section tree from Markdown content by scanning lines for ATX headings.
fn build_section_tree(
    lines: &[String],
    line_starts: &[usize],
    total_bytes: usize,
    content_start: usize,
) -> Vec<Section> {
    let mut headings: Vec<HeadingInfo> = Vec::new();
    let mut fence_state: Option<FenceState> = None;

    for (idx, line) in lines.iter().enumerate() {
        let line_num = idx + 1;
        let trimmed = line.trim();

        if let Some(fence) = parse_fence(trimmed) {
            match fence_state {
                Some(open_fence) if open_fence.ch == fence.ch && fence.len >= open_fence.len => {
                    fence_state = None;
                    continue;
                }
                None => {
                    fence_state = Some(fence);
                    continue;
                }
                _ => {
                    continue;
                }
            }
        }

        if fence_state.is_some() {
            continue;
        }

        if let Some((level, title)) = parse_atx_heading(trimmed) {
            headings.push(HeadingInfo {
                level,
                title,
                line: line_num,
                byte_offset: byte_offset_for_line(line_starts, idx, total_bytes),
            });
        }
    }

    build_tree_from_headings(&headings, lines, line_starts, total_bytes, content_start)
}

#[derive(Clone, Copy)]
struct FenceState {
    ch: char,
    len: usize,
}

fn parse_fence(line: &str) -> Option<FenceState> {
    let ch = line.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }

    let len = line
        .chars()
        .take_while(|candidate| *candidate == ch)
        .count();
    if len < 3 {
        return None;
    }

    Some(FenceState { ch, len })
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
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }

    let rest = rest.trim_start();
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

fn byte_offset_for_line(line_starts: &[usize], line_idx: usize, total_bytes: usize) -> usize {
    line_starts.get(line_idx).copied().unwrap_or(total_bytes)
}

fn byte_end_for_line(
    line_starts: &[usize],
    line_end: usize,
    total_lines: usize,
    total_bytes: usize,
) -> usize {
    if line_end >= total_lines {
        total_bytes
    } else {
        byte_offset_for_line(line_starts, line_end, total_bytes)
    }
}

/// Build a hierarchical section tree from flat heading list.
fn build_tree_from_headings(
    headings: &[HeadingInfo],
    lines: &[String],
    line_starts: &[usize],
    total_bytes: usize,
    content_start: usize,
) -> Vec<Section> {
    if headings.is_empty() {
        let preamble_end = lines.len();
        let preamble_content = if content_start > 0 {
            &lines[content_start..]
        } else {
            lines
        };

        if preamble_content.is_empty() {
            return Vec::new();
        }

        let preamble_text = preamble_content.join("\n");
        let char_count = preamble_text.chars().count();
        let word_count = count_words(&preamble_text);
        let token_estimate = estimate_tokens(&preamble_text);

        return vec![PreambleSection {
            id: "0".to_string(),
            slug: "preamble".to_string(),
            title: "<preamble>".to_string(),
            level: 1,
            path: vec!["<preamble>".to_string()],
            line_start: 1,
            line_end: preamble_end,
            content_line_start: 1,
            byte_start: byte_offset_for_line(line_starts, content_start, total_bytes),
            byte_end: total_bytes,
            char_count,
            word_count,
            token_estimate,
        }
        .into()];
    }

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
                    byte_end: byte_offset_for_line(line_starts, preamble_end, total_bytes),
                    char_count,
                    word_count,
                    token_estimate,
                }
                .into(),
            );
        }
    }

    sections.extend(build_hierarchy(headings, lines, line_starts, total_bytes));
    sections
}

/// Build hierarchical section tree using a stack-based approach with proper dotted IDs.
fn build_hierarchy(
    headings: &[HeadingInfo],
    lines: &[String],
    line_starts: &[usize],
    total_bytes: usize,
) -> Vec<Section> {
    let total_lines = lines.len();
    let line_ends = compute_line_ends(headings, total_lines);

    struct StackEntry {
        section: Section,
        id_parts: Vec<usize>,
    }

    let mut root_sections: Vec<Section> = Vec::new();
    let mut stack: Vec<StackEntry> = Vec::new();

    for (idx, heading) in headings.iter().enumerate() {
        while stack
            .last()
            .is_some_and(|entry| entry.section.level >= heading.level)
        {
            let popped = stack.pop().expect("stack entry should exist");
            if let Some(parent) = stack.last_mut() {
                parent.section.children.push(popped.section);
            } else {
                root_sections.push(popped.section);
            }
        }

        let mut id_parts = stack
            .last()
            .map(|entry| entry.id_parts.clone())
            .unwrap_or_default();
        let last_child_idx = stack
            .last()
            .map(|entry| entry.section.children.len())
            .unwrap_or(root_sections.len());
        id_parts.push(last_child_idx + 1);

        let id = id_parts
            .iter()
            .map(|part| part.to_string())
            .collect::<Vec<_>>()
            .join(".");
        let line_end = line_ends[idx];
        let byte_end = byte_end_for_line(line_starts, line_end, total_lines, total_bytes);
        let path = stack
            .iter()
            .map(|entry| entry.section.title.clone())
            .chain(std::iter::once(heading.title.clone()))
            .collect();

        stack.push(StackEntry {
            section: Section {
                id,
                slug: Section::slugify(&heading.title),
                title: heading.title.clone(),
                level: heading.level,
                path,
                line_start: heading.line,
                line_end,
                content_line_start: heading.line + 1,
                byte_start: heading.byte_offset,
                byte_end,
                char_count: 0,
                word_count: 0,
                token_estimate: 0,
                children: Vec::new(),
            },
            id_parts,
        });
    }

    while let Some(popped) = stack.pop() {
        if let Some(parent) = stack.last_mut() {
            parent.section.children.push(popped.section);
        } else {
            root_sections.push(popped.section);
        }
    }

    root_sections
}

fn compute_line_ends(headings: &[HeadingInfo], total_lines: usize) -> Vec<usize> {
    let mut line_ends = vec![total_lines; headings.len()];
    let mut open_sections: Vec<usize> = Vec::new();

    for (idx, heading) in headings.iter().enumerate() {
        while open_sections
            .last()
            .is_some_and(|open_idx| headings[*open_idx].level >= heading.level)
        {
            let open_idx = open_sections.pop().expect("open section should exist");
            line_ends[open_idx] = heading.line.saturating_sub(1);
        }
        open_sections.push(idx);
    }

    line_ends
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
        assert_eq!(result.map(|(_, title)| title), Some("Hello".to_string()));
        assert_eq!(parse_atx_heading("##NoSpace"), None);
        assert_eq!(parse_atx_heading("not a heading"), None);
        assert_eq!(parse_atx_heading("##"), None);
    }

    #[test]
    fn test_parse_simple() {
        let content =
            "# Overview\n\nSome text.\n\n## Install\n\nInstall it.\n\n## Usage\n\nUse it.\n";
        let doc = parse_markdown_str("test.md", content).unwrap();
        assert_eq!(doc.line_count, 11);
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].title, "Overview");
        assert_eq!(doc.sections[0].children.len(), 2);
        assert_eq!(doc.sections[0].children[0].title, "Install");
        assert_eq!(doc.sections[0].children[1].title, "Usage");
        assert_eq!(doc.sections[0].line_end, 11);
    }

    #[test]
    fn test_parse_with_preamble() {
        let content = "Some intro text.\n\n# Heading\n\nContent.\n";
        let doc = parse_markdown_str("test.md", content).unwrap();
        assert_eq!(doc.sections.len(), 2);
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
        assert_eq!(count_sections(&doc.sections), 2);
    }

    #[test]
    fn test_parse_tilde_code_block_headings() {
        let content = "# Real Heading\n\n~~~markdown\n# Fake Heading\n~~~\n\n## Real Child\n";
        let doc = parse_markdown_str("test.md", content).unwrap();
        assert_eq!(count_sections(&doc.sections), 2);
    }

    #[test]
    fn test_parse_duplicate_headings() {
        let content = "# Report\n\n## Results\n\nFirst.\n\n## Results\n\nSecond.\n";
        let doc = parse_markdown_str("test.md", content).unwrap();
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].children.len(), 2);
        assert_eq!(doc.sections[0].children[0].id, "1.1");
        assert_eq!(doc.sections[0].children[1].id, "1.2");
    }

    #[test]
    fn test_parse_distinct_top_level_ids() {
        let content = "# A\n\nContent.\n\n# B\n\nMore content.\n";
        let doc = parse_markdown_str("test.md", content).unwrap();
        assert_eq!(doc.sections.len(), 2);
        assert_eq!(doc.sections[0].id, "1");
        assert_eq!(doc.sections[1].id, "2");
    }

    #[test]
    fn test_parse_heading_jumps() {
        let content = "# A\n\n### B\n\nContent.\n";
        let doc = parse_markdown_str("test.md", content).unwrap();
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].title, "A");
        assert_eq!(doc.sections[0].children.len(), 1);
        assert_eq!(doc.sections[0].children[0].title, "B");
    }

    #[test]
    fn test_parse_frontmatter() {
        let content = "---\ntitle: Test\n---\n\n# Heading\n\nContent.\n";
        let doc = parse_markdown_str("test.md", content).unwrap();
        assert!(doc
            .sections
            .iter()
            .any(|section| section.title == "<preamble>" || section.title == "Heading"));
    }

    #[test]
    fn test_crlf_offsets_do_not_overshoot() {
        let parsed = parse_markdown_content("test.md", "# Heading\r\nBody\r\n").unwrap();
        assert_eq!(parsed.doc.sections[0].byte_end, parsed.doc.byte_count);
    }

    #[test]
    fn test_no_trailing_newline_byte_end_matches_file_size() {
        let parsed = parse_markdown_content("test.md", "# Heading\nBody").unwrap();
        assert_eq!(parsed.doc.sections[0].byte_end, parsed.doc.byte_count);
    }

    fn count_sections(sections: &[Section]) -> usize {
        let mut count = sections.len();
        for section in sections {
            count += count_sections(&section.children);
        }
        count
    }
}
