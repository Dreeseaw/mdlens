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
    pub line_start: usize,
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

    /// Find a section by heading path (case-insensitive).
    pub fn find_by_path(&self, path: &[String]) -> Option<&Section> {
        if path.is_empty() {
            return Some(self);
        }
        let target = path[0].to_lowercase();
        if self.title.to_lowercase() == target {
            if path.len() == 1 {
                return Some(self);
            }
            for child in &self.children {
                if let Some(found) = child.find_by_path(&path[1..]) {
                    return Some(found);
                }
            }
        }
        for child in &self.children {
            if let Some(found) = child.find_by_path(path) {
                return Some(found);
            }
        }
        None
    }

    /// Collect all sections matching a path, for ambiguity detection.
    pub fn find_all_by_path(&self, path: &[String]) -> Vec<&Section> {
        let mut results = Vec::new();
        if path.is_empty() {
            results.push(self);
            return results;
        }
        let target = path[0].to_lowercase();
        if self.title.to_lowercase() == target {
            if path.len() == 1 {
                results.push(self);
            } else {
                for child in &self.children {
                    results.extend(child.find_all_by_path(&path[1..]));
                }
            }
        }
        for child in &self.children {
            results.extend(child.find_all_by_path(path));
        }
        results
    }

    /// Get the full content text for this section from the document lines.
    pub fn extract_content<'a>(&self, lines: &'a [String]) -> &'a [String] {
        &lines[(self.line_start - 1)..self.line_end]
    }

    /// Get parent chain of sections from root to this section.
    pub fn parent_chain(&self) -> Vec<&Section> {
        let mut chain = Vec::new();
        chain.push(self);
        chain
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
            results.extend(section.find_all_by_path(path));
        }
        results
    }

    /// Find a single section by path.
    pub fn find_section_by_path(&self, path: &[String]) -> Option<&Section> {
        for section in &self.sections {
            if let Some(found) = section.find_by_path(path) {
                return Some(found);
            }
        }
        None
    }

    /// Get lines for a section by ID.
    pub fn get_section_lines<'a>(&self, id: &str, lines: &'a [String]) -> Option<&'a [String]> {
        self.find_section_by_id(id).map(|s| s.extract_content(lines))
    }
}
