use anyhow::anyhow;

/// Error for section ID not found.
pub fn section_not_found(id: &str, nearby: &[&crate::model::Section]) -> anyhow::Error {
    let mut msg = format!(
        "error: section id not found: {}\n\nAvailable nearby sections:\n",
        id
    );
    for s in nearby {
        msg.push_str(&format!("  {} {}\n", s.id, s.title));
    }
    anyhow!(msg.trim().to_string())
}

/// Error for ambiguous path match.
pub fn ambiguous_path(path: &str, candidates: &[&crate::model::Section]) -> anyhow::Error {
    let mut msg = format!(
        "error: path matched multiple sections: {}\n\nCandidates:\n",
        path
    );
    for s in candidates {
        msg.push_str(&format!("  {} {}\n", s.id, s.path.join(" > ")));
    }
    anyhow!(msg.trim().to_string())
}

/// Error for invalid line range.
pub fn invalid_line_range(start: usize, end: usize) -> anyhow::Error {
    anyhow!(format!(
        "error: invalid line range: {}: {}; start must be <= end",
        start, end
    ))
}

/// Error for file not found.
pub fn file_not_found(path: &str) -> anyhow::Error {
    anyhow!(format!("error: file not found: {}", path))
}
