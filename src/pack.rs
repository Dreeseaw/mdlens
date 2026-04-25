use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet};

use crate::model::Section;
use crate::parse::{load_markdown, ParsedMarkdown};
use crate::search::search_files;
use crate::tokens::estimate_tokens;

const SECTION_SEPARATOR: &str = "\n\n";
const TRUNCATION_NOTICE: &str = "\n\n<!-- mdlens: truncated at token budget -->";

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

pub struct PackSearchOptions {
    pub include_parents: bool,
    pub dedupe: bool,
    pub case_sensitive: bool,
    pub use_regex: bool,
    pub max_results: usize,
    pub context_lines: usize,
}

/// Pack sections by IDs from a single file.
pub fn pack_by_ids(
    path: &str,
    ids: &[String],
    max_tokens: usize,
    include_parents: bool,
    dedupe: bool,
) -> Result<PackResult> {
    let mut cache = HashMap::new();
    let parsed = get_or_load(&mut cache, path)?;
    let mut included_sections: Vec<OwnedSectionRef> = Vec::new();
    let mut seen = HashSet::new();

    for id in ids {
        let section = parsed
            .doc
            .find_section_by_id(id)
            .ok_or_else(|| anyhow!("section id not found: {id}"))?;

        if include_parents {
            collect_parents(
                &parsed.doc,
                section,
                &mut included_sections,
                &mut seen,
                dedupe,
            );
        }

        collect_section_and_children(section, &mut included_sections, &mut seen, path, dedupe);
    }

    build_pack_result(&cache, &included_sections, max_tokens)
}

/// Pack sections by search query.
pub fn pack_by_search(
    root: &str,
    query: &str,
    max_tokens: usize,
    options: PackSearchOptions,
) -> Result<PackResult> {
    let results = search_files(
        root,
        query,
        options.case_sensitive,
        options.use_regex,
        options.max_results,
        options.context_lines,
    )?;
    let mut cache = HashMap::new();
    let mut included_sections: Vec<OwnedSectionRef> = Vec::new();
    let mut seen = HashSet::new();

    for result in &results {
        let parsed = get_or_load(&mut cache, &result.path)?;
        if let Some(section) = parsed.doc.find_section_by_id(&result.section_id) {
            if options.include_parents {
                collect_parents(
                    &parsed.doc,
                    section,
                    &mut included_sections,
                    &mut seen,
                    options.dedupe,
                );
            }
            collect_section_and_children(
                section,
                &mut included_sections,
                &mut seen,
                &result.path,
                options.dedupe,
            );
        }
    }

    build_pack_result(&cache, &included_sections, max_tokens)
}

struct OwnedSectionRef {
    path: String,
    id: String,
    is_parent_context: bool,
}

fn get_or_load<'a>(
    cache: &'a mut HashMap<String, ParsedMarkdown>,
    path: &str,
) -> Result<&'a ParsedMarkdown> {
    if !cache.contains_key(path) {
        cache.insert(path.to_string(), load_markdown(path)?);
    }
    Ok(cache.get(path).expect("parsed markdown should be cached"))
}

fn collect_parents(
    doc: &crate::model::Document,
    target: &Section,
    included: &mut Vec<OwnedSectionRef>,
    seen: &mut HashSet<String>,
    dedupe: bool,
) {
    let mut parent_map = HashMap::new();
    build_parent_map(&doc.sections, None, &mut parent_map);

    let mut chain = Vec::new();
    let mut current_id = target.id.clone();
    while let Some(Some(parent_id)) = parent_map.get(&current_id) {
        if let Some(parent) = doc.find_section_by_id(parent_id) {
            chain.push(parent);
        }
        current_id = parent_id.clone();
    }
    chain.reverse();

    for parent in chain {
        push_ref(
            OwnedSectionRef {
                path: doc.path.clone(),
                id: parent.id.clone(),
                is_parent_context: true,
            },
            included,
            seen,
            dedupe,
        );
    }
}

fn build_parent_map(
    sections: &[Section],
    parent_id: Option<String>,
    map: &mut HashMap<String, Option<String>>,
) {
    for section in sections {
        map.insert(section.id.clone(), parent_id.clone());
        build_parent_map(&section.children, Some(section.id.clone()), map);
    }
}

fn collect_section_and_children(
    section: &Section,
    included: &mut Vec<OwnedSectionRef>,
    seen: &mut HashSet<String>,
    file_path: &str,
    dedupe: bool,
) {
    push_ref(
        OwnedSectionRef {
            path: file_path.to_string(),
            id: section.id.clone(),
            is_parent_context: false,
        },
        included,
        seen,
        dedupe,
    );

    for child in &section.children {
        collect_section_and_children(child, included, seen, file_path, dedupe);
    }
}

fn push_ref(
    section_ref: OwnedSectionRef,
    included: &mut Vec<OwnedSectionRef>,
    seen: &mut HashSet<String>,
    dedupe: bool,
) {
    if !dedupe {
        included.push(section_ref);
        return;
    }

    let key = format!(
        "{}::{}::{}",
        section_ref.path, section_ref.id, section_ref.is_parent_context
    );
    if seen.insert(key) {
        included.push(section_ref);
    }
}

fn build_pack_result(
    cache: &HashMap<String, ParsedMarkdown>,
    included_sections: &[OwnedSectionRef],
    max_tokens: usize,
) -> Result<PackResult> {
    let mut content = String::new();
    let mut total_tokens = 0usize;
    let mut included = Vec::new();
    let mut truncated = false;

    for section_ref in included_sections {
        let parsed = cache
            .get(&section_ref.path)
            .ok_or_else(|| anyhow!("missing parsed markdown cache for {}", section_ref.path))?;
        let Some(section) = parsed.doc.find_section_by_id(&section_ref.id) else {
            continue;
        };

        let section_text = if section_ref.is_parent_context {
            parsed.lines[section.line_start - 1].clone()
        } else {
            section.extract_content(&parsed.lines).join("\n")
        };

        let appended = if content.is_empty() {
            section_text.clone()
        } else {
            format!("{SECTION_SEPARATOR}{section_text}")
        };
        let appended_tokens = estimate_tokens(&appended);

        if total_tokens + appended_tokens > max_tokens {
            if section_ref.is_parent_context {
                truncated = true;
                break;
            }

            let remaining = max_tokens.saturating_sub(total_tokens);
            if remaining > 0 {
                let truncated_segment = truncate_segment_to_tokens(&appended, remaining);
                if !truncated_segment.is_empty() {
                    total_tokens += estimate_tokens(&truncated_segment);
                    content.push_str(&truncated_segment);
                }
            }
            truncated = true;
            break;
        }

        content.push_str(&appended);
        total_tokens += appended_tokens;
        included.push(PackIncludedSection {
            path: section_ref.path.clone(),
            section_id: section.id.clone(),
            section_path: section.path.clone(),
            line_start: section.line_start,
            line_end: section.line_end,
            token_estimate: estimate_tokens(&section_text),
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

/// Truncate text to fit within a token budget and reserve room for the notice.
fn truncate_segment_to_tokens(text: &str, max_tokens: usize) -> String {
    if max_tokens == 0 {
        return String::new();
    }

    if estimate_tokens(text) <= max_tokens {
        return text.to_string();
    }

    let notice_tokens = estimate_tokens(TRUNCATION_NOTICE);
    if max_tokens <= notice_tokens {
        return String::new();
    }

    let target_chars = (max_tokens - notice_tokens) * 4;
    let mut char_count = 0usize;
    let mut truncate_at = 0usize;

    for (idx, ch) in text.char_indices() {
        char_count += 1;
        if char_count > target_chars {
            break;
        }
        truncate_at = idx + ch.len_utf8();
    }

    if truncate_at == 0 {
        return String::new();
    }

    format!("{}{}", &text[..truncate_at], TRUNCATION_NOTICE)
}
