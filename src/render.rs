use crate::model::{Document, Section};

/// Render a tree view of a document for human consumption.
pub fn render_tree(doc: &Document, max_depth: Option<usize>, include_preamble: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{}  lines={}  tokens\u{2248}{}\n\n",
        doc.path, doc.line_count, doc.token_estimate
    ));

    for section in &doc.sections {
        if section.title == "<preamble>" && !include_preamble {
            continue;
        }
        let depth = 0;
        render_section(&mut out, section, depth, max_depth);
    }

    out
}

fn render_section(out: &mut String, section: &Section, depth: usize, max_depth: Option<usize>) {
    if let Some(max) = max_depth {
        if depth >= max {
            return;
        }
    }

    let indent = "  ".repeat(depth);
    out.push_str(&format!(
        "{}{} {} lines {}-{}  tokens\u{2248}{}\n",
        indent,
        section.id,
        section.title,
        section.line_start,
        section.line_end,
        section.token_estimate
    ));

    for child in &section.children {
        render_section(out, child, depth + 1, max_depth);
    }
}

/// Render a section read output for human consumption.
pub fn render_read(section: &Section, content: &str, truncated: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{} > {}\n",
        section.path.join(" > "),
        section.title
    ));
    out.push_str(&format!(
        "id={}  lines={}  tokens\u{2248}{}\n\n",
        section.id, section.line_start, section.token_estimate
    ));
    out.push_str(content);
    if truncated && !content.contains("<!-- mdlens: truncated at token budget -->") {
        out.push_str("\n\n<!-- mdlens: truncated at token budget -->\n");
    }
    out
}

/// Render search results for human consumption.
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
}

pub struct SearchSnippet {
    pub line_start: usize,
    pub line_end: usize,
    pub text: String,
}

pub fn render_search(results: &[SearchResult]) -> String {
    let mut out = String::new();
    for (i, result) in results.iter().enumerate() {
        if i > 0 {
            out.push_str("\n---\n\n");
        }
        out.push_str(&format!(
            "{} > {}\nid={}  lines={}-{}  tokens\u{2248}{}  matches={}\n\n",
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
    }
    out
}

/// Render stats table for human consumption.
pub fn render_stats(entries: &[StatsEntry]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{:<30} {:>8} {:>8} {:>8}\n",
        "path", "lines", "words", "tokens\u{2248}"
    ));

    for entry in entries {
        out.push_str(&format!(
            "{:<30} {:>8} {:>8} {:>8}\n",
            entry.path, entry.lines, entry.words, entry.tokens
        ));
    }
    out
}

pub struct StatsEntry {
    pub path: String,
    pub lines: usize,
    pub words: usize,
    pub tokens: usize,
}

/// Render pack output for human consumption.
pub fn render_pack(
    source: &str,
    budget: usize,
    included: &[PackIncluded],
    content: &str,
    truncated: bool,
) -> String {
    let mut out = String::new();
    out.push_str("<!-- mdlens pack\n");
    out.push_str(&format!("source: {}\n", source));
    out.push_str(&format!("budget: tokens\u{2248}{}\n", budget));
    out.push_str("included:\n");
    for inc in included {
        out.push_str(&format!(
            "- {} {} lines={}  tokens\u{2248}{}\n",
            inc.section_id, inc.section_title, inc.line_range, inc.token_estimate
        ));
    }
    if truncated {
        out.push_str("truncated: true\n");
    }
    out.push_str("-->\n\n");
    out.push_str(content);
    out
}

pub struct PackIncluded {
    pub section_id: String,
    pub section_title: String,
    pub line_range: String,
    pub token_estimate: usize,
}
