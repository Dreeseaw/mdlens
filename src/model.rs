use serde::Serialize;

/// A document parsed from a Markdown file.
#[derive(Debug, Clone, Serialize)]
pub struct Document {
    pub path: String,
    pub line_count: usize,
    pub byte_count: usize,
    pub char_count: usize,
    pub word_count: usize,
    pub token_estimate: usize,
    pub sections: Vec<Section>,
}

/// A section in a Markdown document, corresponding to a heading and its content.
#[derive(Debug, Clone, Serialize)]
pub struct Section {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub level: u8,
    pub path: Vec<String>,
    /// First line of this section (1-indexed, includes heading).
    pub line_start: usize,
    /// Last line of this section's full subtree (1-indexed).
    /// This spans all nested children, not just direct content.
    /// Use `extract_direct_content` to get content before the first child.
    pub line_end: usize,
    pub content_line_start: usize,
    pub byte_start: usize,
    pub byte_end: usize,
    pub char_count: usize,
    pub word_count: usize,
    pub token_estimate: usize,
    pub children: Vec<Section>,
}

/// Preamble section for content before the first heading.
#[derive(Debug, Clone, Serialize)]
pub struct PreambleSection {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub level: u8,
    pub path: Vec<String>,
    pub line_start: usize,
    pub line_end: usize,
    pub content_line_start: usize,
    pub byte_start: usize,
    pub byte_end: usize,
    pub char_count: usize,
    pub word_count: usize,
    pub token_estimate: usize,
}

impl From<PreambleSection> for Section {
    fn from(p: PreambleSection) -> Self {
        Section {
            id: p.id,
            slug: p.slug,
            title: p.title,
            level: p.level,
            path: p.path,
            line_start: p.line_start,
            line_end: p.line_end,
            content_line_start: p.content_line_start,
            byte_start: p.byte_start,
            byte_end: p.byte_end,
            char_count: p.char_count,
            word_count: p.word_count,
            token_estimate: p.token_estimate,
            children: Vec::new(),
        }
    }
}

impl Section {
    /// Generate a stable slug from heading text.
    pub fn slugify(text: &str) -> String {
        text.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || c == &'-' || c == &'_' || c.is_whitespace())
            .map(|c| match c {
                ' ' => '-',
                _ => c,
            })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-")
    }

    /// Find a child section by ID recursively.
    pub fn find_by_id(&self, id: &str) -> Option<&Section> {
        if self.id == id {
            return Some(self);
        }
        for child in &self.children {
            if let Some(found) = child.find_by_id(id) {
                return Some(found);
            }
        }
        None
    }

    /// Get the full content text for this section from the document lines.
    pub fn extract_content<'a>(&self, lines: &'a [String]) -> &'a [String] {
        &lines[(self.line_start - 1)..self.line_end]
    }

    /// Get only this section's heading and direct body before the first child heading.
    pub fn extract_direct_content<'a>(&self, lines: &'a [String]) -> &'a [String] {
        let line_end = self
            .children
            .first()
            .map(|child| child.line_start.saturating_sub(1))
            .unwrap_or(self.line_end);
        &lines[(self.line_start - 1)..line_end]
    }
}

impl Document {
    /// Find a section by ID.
    pub fn find_section_by_id(&self, id: &str) -> Option<&Section> {
        for section in &self.sections {
            if let Some(found) = section.find_by_id(id) {
                return Some(found);
            }
        }
        None
    }

    /// Find sections by path.
    pub fn find_sections_by_path(&self, path: &[String]) -> Vec<&Section> {
        let mut results = Vec::new();
        for section in &self.sections {
            collect_sections_by_exact_path(section, path, &mut results);
        }
        results
    }
}

fn collect_sections_by_exact_path<'a>(
    section: &'a Section,
    path: &[String],
    results: &mut Vec<&'a Section>,
) {
    if path.len() == section.path.len()
        && section
            .path
            .iter()
            .zip(path.iter())
            .all(|(lhs, rhs)| lhs.eq_ignore_ascii_case(rhs))
    {
        results.push(section);
    }

    for child in &section.children {
        collect_sections_by_exact_path(child, path, results);
    }
}
