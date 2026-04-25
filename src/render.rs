use crate::model::{Document, Section};
use std::collections::{HashMap, HashSet};

pub fn render_tree(doc: &Document, max_depth: Option<usize>, include_preamble: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{}  lines={}  ~{}t\n\n",
        doc.path, doc.line_count, doc.token_estimate
    ));

    for section in &doc.sections {
        if section.title == "<preamble>" && !include_preamble {
            continue;
        }
        render_section_tree(&mut out, section, 0, max_depth);
    }

    out
}

fn render_section_tree(
    out: &mut String,
    section: &Section,
    depth: usize,
    max_depth: Option<usize>,
) {
    if let Some(max) = max_depth {
        if depth >= max {
            return;
        }
    }

    let indent = "  ".repeat(depth);
    out.push_str(&format!(
        "{}{} {} l{}-{} ~{}t\n",
        indent,
        section.id,
        section.title,
        section.line_start,
        section.line_end,
        section.token_estimate
    ));

    for child in &section.children {
        render_section_tree(out, child, depth + 1, max_depth);
    }
}

pub fn render_read(section: &Section, content: &str, truncated: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{} | id={} l{}-{} ~{}t\n",
        section.path.join(" > "),
        section.id,
        section.line_start,
        section.line_end,
        section.token_estimate
    ));
    out.push_str(content);
    if truncated && !content.contains("<!-- mdlens: truncated at token budget -->") {
        out.push_str("\n\n<!-- mdlens: truncated at token budget -->\n");
    }
    out
}

pub struct SearchResult {
    pub path: String,
    pub section_id: String,
    pub section_title: String,
    pub section_path: Vec<String>,
    pub line_start: usize,
    pub line_end: usize,
    pub token_estimate: usize,
    pub match_count: usize,
    pub snippets: Vec<SearchSnippet>,
    pub body: Option<String>,
    pub preview: Option<String>,
}

pub struct SearchSnippet {
    pub line_start: usize,
    pub line_end: usize,
    pub text: String,
}

/// Maps file path → (matched_section_ids, all_section_summaries [(id, title)])
pub type FileSectionsMap = HashMap<String, Vec<(String, String)>>;

pub fn render_search(
    results: &[SearchResult],
    with_content: bool,
    file_sections: &FileSectionsMap,
) -> String {
    let matched_ids_by_file: HashMap<&str, HashSet<&str>> =
        results.iter().fold(HashMap::new(), |mut acc, r| {
            acc.entry(r.path.as_str()).or_default().insert(r.section_id.as_str());
            acc
        });

    let mut out = String::new();
    let mut last_file: Option<&str> = None;
    let mut shown_sidebars: HashSet<&str> = HashSet::new();

    for (i, result) in results.iter().enumerate() {
        let file_changed = last_file != Some(result.path.as_str());

        if i > 0 {
            out.push('\n');
        }

        // Show sidebar at the START of each file's first result (before content)
        if file_changed {
            if shown_sidebars.insert(result.path.as_str()) {
                append_file_sidebar(&mut out, &result.path, file_sections, &matched_ids_by_file);
            }
        }
        last_file = Some(result.path.as_str());

        out.push_str(&format!(
            "{} > {} [id={} l{}-{} ~{}t matches={}]\n",
            result.path,
            result.section_path.join(" > "),
            result.section_id,
            result.line_start,
            result.line_end,
            result.token_estimate,
            result.match_count,
        ));
        for snippet in &result.snippets {
            out.push_str(&format!("{}:{}\n", snippet.line_start, snippet.text));
        }

        if with_content {
            if let Some(body) = &result.body {
                if !body.is_empty() {
                    out.push('\n');
                    out.push_str(body);
                    if !body.ends_with('\n') {
                        out.push('\n');
                    }
                }
            }
        } else if let Some(preview) = &result.preview {
            if !preview.is_empty() {
                out.push('\n');
                out.push_str(preview);
                if !preview.ends_with('\n') {
                    out.push('\n');
                }
            }
        }
    }

    out
}

fn append_file_sidebar(
    out: &mut String,
    file_path: &str,
    file_sections: &FileSectionsMap,
    matched_ids_by_file: &HashMap<&str, HashSet<&str>>,
) {
    let Some(all_sections) = file_sections.get(file_path) else { return };
    let matched = matched_ids_by_file.get(file_path);
    let others: Vec<String> = all_sections
        .iter()
        .filter(|(id, title)| {
            title != "<preamble>"
                && matched.map_or(true, |m| !m.contains(id.as_str()))
        })
        .map(|(id, title)| format!("§{} {}", id, title))
        .take(8)
        .collect();
    if !others.is_empty() {
        out.push_str(&format!(
            "[{}: also — {}]\n",
            std::path::Path::new(file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(file_path),
            others.join(" · ")
        ));
    }
}

pub struct StatsEntry {
    pub path: String,
    pub lines: usize,
    pub words: usize,
    pub tokens: usize,
}

pub fn render_stats(entries: &[StatsEntry]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{:<30} {:>6} {:>6} {:>6}\n",
        "path", "lines", "words", "~toks"
    ));

    for entry in entries {
        out.push_str(&format!(
            "{:<30} {:>6} {:>6} {:>6}\n",
            entry.path, entry.lines, entry.words, entry.tokens
        ));
    }
    out
}

pub struct PackIncluded {
    pub section_id: String,
    pub section_title: String,
    pub line_range: String,
    pub token_estimate: usize,
}

pub fn render_pack(
    source: &str,
    budget: usize,
    included: &[PackIncluded],
    content: &str,
    truncated: bool,
) -> String {
    let included_str: Vec<String> = included
        .iter()
        .map(|inc| {
            format!(
                "{} {} l={} ~{}t",
                inc.section_id, inc.section_title, inc.line_range, inc.token_estimate
            )
        })
        .collect();
    let mut header = format!(
        "<!-- pack src={} budget=~{}t included=[{}]",
        source,
        budget,
        included_str.join(", ")
    );
    if truncated {
        header.push_str(" truncated=true");
    }
    header.push_str(" -->\n\n");
    header.push_str(content);
    header
}

pub struct SectionsEntry {
    pub file_path: String,
    pub id: String,
    pub title: String,
    pub heading_path: Option<Vec<String>>,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    pub token_estimate: usize,
    pub body: Option<String>,
    pub preview: Option<String>,
}

pub fn render_sections(entries: &[SectionsEntry], with_content: bool) -> String {
    let mut out = String::new();
    let mut current_file: Option<&str> = None;

    for entry in entries {
        if Some(entry.file_path.as_str()) != current_file {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&entry.file_path);
            out.push('\n');
            current_file = Some(entry.file_path.as_str());
        }

        out.push_str(&format!("§{} {}", entry.id, entry.title));

        if let (Some(start), Some(end)) = (entry.line_start, entry.line_end) {
            out.push_str(&format!(" l{}-{} ~{}t", start, end, entry.token_estimate));
        } else {
            out.push_str(&format!(" ~{}t", entry.token_estimate));
        }
        out.push('\n');

        if let Some(ref hp) = entry.heading_path {
            out.push_str(&format!("  path: {}\n", hp.join(" > ")));
        }

        if with_content {
            if let Some(ref body) = entry.body {
                out.push_str(body);
                if !body.ends_with('\n') {
                    out.push('\n');
                }
            }
            out.push('\n');
        } else if let Some(ref preview) = entry.preview {
            if !preview.is_empty() {
                out.push_str(preview);
                if !preview.ends_with('\n') {
                    out.push('\n');
                }
            }
        }
    }

    out
}
