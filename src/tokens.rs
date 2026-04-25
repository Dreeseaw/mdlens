/// Estimate tokens using a simple heuristic: ~1 token per 4 UTF-8 characters.
/// This is approximate but deterministic.
pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let char_count = text.chars().count();
    // Heuristic: ~1 token per 4 chars is a common approximation
    // Code blocks and whitespace may differ but this is good enough for v1
    (char_count as f64 / 4.0).ceil() as usize
}

/// Count words in text (split on whitespace).
pub fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Count bytes in text.
pub fn count_bytes(text: &str) -> usize {
    text.len()
}

/// Count characters in text.
pub fn count_chars(text: &str) -> usize {
    text.chars().count()
}

/// Count lines in text.
pub fn count_lines(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    text.lines().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_estimate_tokens_basic() {
        // "Hello world" is 11 chars, ~3 tokens
        let tokens = estimate_tokens("Hello world");
        assert!(tokens >= 2 && tokens <= 4);
    }

    #[test]
    fn test_count_words() {
        assert_eq!(count_words("hello world foo bar"), 4);
        assert_eq!(count_words(""), 0);
    }

    #[test]
    fn test_count_lines() {
        assert_eq!(count_lines("a\nb\nc"), 3);
        assert_eq!(count_lines("single"), 1);
        assert_eq!(count_lines(""), 0);
    }

    #[test]
    fn test_token_estimate_monotonic() {
        let t1 = estimate_tokens("hello");
        let t2 = estimate_tokens("hello world this is a longer text");
        assert!(t2 > t1);
    }
}
