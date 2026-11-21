//! Small checks shared by the settings menu.

/// A calendar address people can paste: https://, http:// or webcal://.
pub fn is_calendar_url(s: &str) -> bool {
    let s = s.trim();
    let lower = s.to_lowercase();
    ["https://", "http://", "webcal://"].iter().any(|p| lower.starts_with(p)) && s.len() > 12 && !s.contains(char::is_whitespace)
}

/// Anything "add a calendar" takes: a feed address, a file or folder on this machine, or the words for public holidays.
pub fn is_calendar_source(s: &str) -> bool {
    let words: Vec<String> = s.split_whitespace().map(str::to_lowercase).collect();
    is_calendar_url(s) || looks_like_path(s) || (matches!(words.first().map(String::as_str), Some("holidays" | "festività" | "festivita" | "feste")) && words.len() <= 2)
}

/// Looks like a place on a disk: `/home/me/uni.ics`, `~/uni.ics`, `file:///...`, `C:\\Users\\me\\uni.ics`. Whether it exists is
/// the platform layer's business.
pub fn looks_like_path(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || s.contains('\n') {
        return false;
    }
    let drive = s.len() > 3 && s.as_bytes()[1] == b':' && s.as_bytes()[0].is_ascii_alphabetic() && matches!(s.as_bytes()[2], b'\\' | b'/');
    (s.starts_with('/') || s.starts_with("~/") || s.starts_with("file://") || drive) && s.len() > 3
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

    #[test]
    fn the_add_a_calendar_line_takes_feeds_files_and_holidays() {
        assert!(is_calendar_source("https://calendar.example.org/u/1/basic.ics") && is_calendar_source("~/uni.ics") && is_calendar_source("festività") && is_calendar_source("holidays IT"));
        assert!(!is_calendar_source("holidays IT extra") && !is_calendar_source("my calendar") && !is_calendar_source(""));
    }

    #[test]
    fn recognises_places_on_a_disk() {
        for p in ["/home/me/uni.ics", "~/Documents/uni.ics", "file:///home/me/uni.ics", "C:\\Users\\me\\uni.ics", "d:/cal/uni.ics"] {
            assert!(looks_like_path(p), "{p}");
        }
        for p in ["", "uni.ics", "https://x.org/a.ics", "/", "hello world", "~"] {
            assert!(!looks_like_path(p), "{p}");
        }
    }
}

