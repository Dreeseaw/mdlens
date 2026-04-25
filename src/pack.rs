use anyhow::{anyhow, Result};
use std::fs;

use crate::model::Section;
use crate::parse::parse_markdown;
use crate::search::search_files;
use crate::tokens::estimate_tokens;

/// Pack selected sections into a bounded token budget.
pub struct PackResult {
    pub token_budget: usize,
    pub token_estimate: usize,
    pub truncated: bool,
    pub included: Vec<PackIncludedSection>,
    pub content: String,
}

pub struct PackIncludedSection {
    pub path: String,
    pub section_id: String,
    pub section_path: Vec<String>,
    pub line_start: usize,
    pub line_end: usize,
    pub token_estimate: usize,
    pub truncated: bool,
}

/// Pack sections by IDs from a single file.
pub fn pack_by_ids(
    path: &str,
    ids: &[String],
    max_tokens: usize,
    include_parents: bool,
) -> Result<PackResult> {
    let doc = parse_markdown(path)?;
    let lines: Vec<String> = fs::read_to_string(path)?
        .lines()
        .map(|l| l.to_string())
        .collect();

    let mut included_sections: Vec<OwnedSectionRef> = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for id in ids {
        if let Some(section) = doc.find_section_by_id(id) {
            // Collect parent headings if requested
            if include_parents {
                collect_parents(&doc, section, &mut included_sections, &mut seen_ids);
            }

            // Add the section and handle deduplication (dedup handled inside collect function)
            collect_section_and_unvisited_children(section, &mut included_sections, &mut seen_ids, path);
        } else {
            return Err(anyhow!("section id not found: {}", id));
        }
    }

    build_pack_result(&doc, &lines, &included_sections, max_tokens)
}

/// Pack sections by search query.
pub fn pack_by_search(
    root: &str,
    query: &str,
    max_tokens: usize,
    include_parents: bool,
) -> Result<PackResult> {
    let results = search_files(root, query, false, false, 20, 2)?;
    let mut included_sections: Vec<OwnedSectionRef> = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for result in &results {
        let doc = parse_markdown(&result.path)?;
        if let Some(section) = doc.find_section_by_id(&result.section_id) {
            if include_parents {
                collect_parents(&doc, section, &mut included_sections, &mut seen_ids);
            }
            collect_section_and_unvisited_children(section, &mut included_sections, &mut seen_ids, &result.path);
        }
    }

    // Build combined content - need to resolve each section ref to actual content
    let mut content = String::new();
    let mut total_tokens: usize = 0;
    let mut included = Vec::new();
    let mut truncated = false;

    for ref_section in &included_sections {
        let doc = parse_markdown(&ref_section.path)?;
        let lines: Vec<String> = fs::read_to_string(&ref_section.path)?
            .lines()
            .map(|l| l.to_string())
            .collect();

        if let Some(section) = doc.find_section_by_id(&ref_section.id) {
            let section_text = if ref_section.is_parent_context {
                // For parent context, only include the heading line
                lines[section.line_start - 1].clone()
            } else {
                section.extract_content(&lines).join("\n")
            };
            let section_tokens = estimate_tokens(&section_text);

            if total_tokens + section_tokens > max_tokens && !ref_section.is_parent_context {
                let remaining = max_tokens - total_tokens;
                if remaining > 0 {
                    let truncated_text = truncate_to_tokens(&section_text, remaining);
                    content.push_str("\n\n");
                    content.push_str(&truncated_text);
                    content.push_str("\n\n<!-- mdlens: truncated at token budget -->\n");
                }
                total_tokens = max_tokens;
                truncated = true;
                break;
            }

            if !content.is_empty() {
                content.push_str("\n\n");
            }
            content.push_str(&section_text);
            total_tokens += section_tokens;

            included.push(PackIncludedSection {
                path: ref_section.path.clone(),
                section_id: section.id.clone(),
                section_path: section.path.clone(),
                line_start: section.line_start,
                line_end: section.line_end,
                token_estimate: section_tokens,
                truncated: false,
            });
        }
    }

    Ok(PackResult {
        token_budget: max_tokens,
        token_estimate: total_tokens,
        truncated,
        included,
        content,
    })
}

struct OwnedSectionRef {
    path: String,
    id: String,
    is_parent_context: bool,
}

fn collect_parents(
    doc: &crate::model::Document,
    target: &Section,
    included: &mut Vec<OwnedSectionRef>,
    seen: &mut std::collections::HashSet<String>,
) {
    let mut parent_map: std::collections::HashMap<String, Option<String>> = std::collections::HashMap::new();
    build_parent_map(&doc.sections, None, &mut parent_map);

    let mut chain = Vec::new();
    let mut current_id = target.id.clone();
    while let Some(Some(pid)) = parent_map.get(&current_id) {
        if let Some(parent_sec) = doc.find_section_by_id(pid) {
            chain.push(parent_sec);
        }
        current_id = pid.clone();
    }
    chain.reverse();

    for parent in &chain {
        if seen.insert(parent.id.clone()) {
            included.push(OwnedSectionRef {
                path: doc.path.clone(),
                id: parent.id.clone(),
                is_parent_context: true,
            });
        }
    }
}

fn build_parent_map(
    sections: &[Section],
    parent_id: Option<String>,
    map: &mut std::collections::HashMap<String, Option<String>>,
) {
    for section in sections {
        map.insert(section.id.clone(), parent_id.clone());
        build_parent_map(&section.children, Some(section.id.clone()), map);
    }
}

fn collect_section_and_unvisited_children(
    section: &Section,
    included: &mut Vec<OwnedSectionRef>,
    seen: &mut std::collections::HashSet<String>,
    file_path: &str,
) {
    if seen.insert(section.id.clone()) {
        included.push(OwnedSectionRef {
            path: file_path.to_string(),
            id: section.id.clone(),
            is_parent_context: false,
        });
        for child in &section.children {
            collect_section_and_unvisited_children(child, included, seen, file_path);
        }
    }
}

fn build_pack_result(
    doc: &crate::model::Document,
    lines: &[String],
    included_sections: &[OwnedSectionRef],
    max_tokens: usize,
) -> Result<PackResult> {
    let mut content = String::new();
    let mut total_tokens: usize = 0;
    let mut included = Vec::new();
    let mut truncated = false;

    for ref_section in included_sections {
        let actual_path = if ref_section.path.is_empty() {
            doc.path.clone()
        } else {
            ref_section.path.clone()
        };

        // Read section info and content
        let (section_info, section_text) = if ref_section.path.is_empty() {
            if let Some(section) = doc.find_section_by_id(&ref_section.id) {
                let text = if ref_section.is_parent_context {
                    lines[section.line_start - 1].clone()
                } else {
                    section.extract_content(lines).join("\n")
                };
                (SectionInfo::from(section), text)
            } else {
                continue;
            }
        } else {
            let other_doc = parse_markdown(&ref_section.path)?;
            let other_lines: Vec<String> = fs::read_to_string(&ref_section.path)?
                .lines()
                .map(|l| l.to_string())
                .collect();
            if let Some(section) = other_doc.find_section_by_id(&ref_section.id) {
                let text = if ref_section.is_parent_context {
                    other_lines[section.line_start - 1].clone()
                } else {
                    section.extract_content(&other_lines).join("\n")
                };
                (SectionInfo::from(section), text)
            } else {
                continue;
            }
        };

        let section_tokens = estimate_tokens(&section_text);

        if total_tokens + section_tokens > max_tokens && !ref_section.is_parent_context {
            let remaining = max_tokens - total_tokens;
            if remaining > 0 {
                let truncated_text = truncate_to_tokens(&section_text, remaining);
                content.push_str("\n\n");
                content.push_str(&truncated_text);
                content.push_str("\n\n<!-- mdlens: truncated at token budget -->\n");
            }
            total_tokens = max_tokens;
            truncated = true;
            break;
        }

        if !content.is_empty() {
            content.push_str("\n\n");
        }
        content.push_str(&section_text);
        total_tokens += section_tokens;

        included.push(PackIncludedSection {
            path: actual_path,
            section_id: section_info.id,
            section_path: section_info.path,
            line_start: section_info.line_start,
            line_end: section_info.line_end,
            token_estimate: section_tokens,
            truncated: false,
        });
    }

    Ok(PackResult {
        token_budget: max_tokens,
        token_estimate: total_tokens,
        truncated,
        included,
        content,
    })
}

/// Owned section info for use across function boundaries.
struct SectionInfo {
    id: String,
    path: Vec<String>,
    line_start: usize,
    line_end: usize,
}

impl<'a> From<&'a Section> for SectionInfo {
    fn from(s: &'a Section) -> Self {
        SectionInfo {
            id: s.id.clone(),
            path: s.path.clone(),
            line_start: s.line_start,
            line_end: s.line_end,
        }
    }
}

/// Truncate text to fit within a token budget.
fn truncate_to_tokens(text: &str, max_tokens: usize) -> String {
    let max_chars = max_tokens * 4;
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let mut char_count = 0;
    let mut truncate_at = 0;
    for (idx, ch) in text.char_indices() {
        char_count += 1;
        if char_count >= max_chars {
            truncate_at = idx + ch.len_utf8();
            break;
        }
    }

    text[..truncate_at].to_string()
}
