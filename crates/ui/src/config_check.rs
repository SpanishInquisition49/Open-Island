//! Small checks shared by the settings menu.

/// A calendar address people can paste: https://, http:// or webcal://.
pub fn is_calendar_url(s: &str) -> bool {
    let s = s.trim();
    let lower = s.to_lowercase();
    ["https://", "http://", "webcal://"].iter().any(|p| lower.starts_with(p)) && s.len() > 12 && !s.contains(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_calendar_addresses() {
        assert!(is_calendar_url("https://calendar.example.org/u/1/basic.ics"));
        assert!(is_calendar_url("  webcal://p01-caldav.icloud.com/published/2/x "));
        assert!(!is_calendar_url("ftp://x.org/a.ics"));
        assert!(!is_calendar_url("https://"));
        assert!(!is_calendar_url("https://a b.org/c.ics"));
        assert!(!is_calendar_url(""));
    }
}
