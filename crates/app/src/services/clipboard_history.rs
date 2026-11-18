//! The clipboard history list: newest first, no duplicates, no blank text, capped. Shared by every platform.

pub const MAX: usize = 25;
pub const MAX_CHARS: usize = 4000;

/// Adds an entry to the history: newest first, no duplicates, no blank text, capped.
pub fn push(history: &mut Vec<String>, text: &str) -> bool {
    let text: String = text.chars().take(MAX_CHARS).collect();
    if text.trim().is_empty() {
        return false;
    }
    history.retain(|t| *t != text);
    history.insert(0, text);
    history.truncate(MAX);
    true
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_first_without_duplicates() {
        let mut h = vec![];
        assert!(push(&mut h, "a"));
        assert!(push(&mut h, "b"));
        assert!(push(&mut h, "a"));
        assert_eq!(h, vec!["a", "b"]);
        assert!(!push(&mut h, "   \n"));
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn caps_the_list_and_the_length() {
        let mut h = vec![];
        for i in 0..40 {
            push(&mut h, &i.to_string());
        }
        assert_eq!(h.len(), MAX);
        assert_eq!(h[0], "39");
        push(&mut h, &"x".repeat(10_000));
        assert_eq!(h[0].chars().count(), MAX_CHARS);
    }
}
