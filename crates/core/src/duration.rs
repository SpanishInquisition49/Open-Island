//! Parsing and formatting of durations (timer presets, countdowns).

/// "5", "5m", "90s", "1h30m", "1h 5m 10s": a bare number is minutes. Capped at 24 h. Returns seconds.
pub fn parse(text: &str) -> Option<u64> {
    let text = text.to_lowercase();
    let b = text.as_bytes();
    let (mut i, mut total, mut found) = (0, 0u64, false);
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let n: u64 = text[start..i].parse().ok()?;
        let mut j = i;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        let unit = b.get(j).copied().unwrap_or(0);
        let mult = match unit {
            b'h' => {
                i = j + 1;
                3600
            }
            b's' => {
                i = j + 1;
                1
            }
            b'm' => {
                i = j + 1;
                60
            }
            _ => 60,
        };
        total = total.saturating_add(n.saturating_mul(mult));
        found = true;
    }
    if !found || total == 0 {
        return None;
    }
    Some(total.min(24 * 3600))
}

fn hms(s: u64) -> String {
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// 65000 -> "1:05", 3725000 -> "1:02:05" (rounded up: a timer never shows 0:00 early).
pub fn fmt_remaining(ms: i64) -> String {
    hms((ms.max(0) as u64).div_ceil(1000))
}

/// Elapsed time, rounded down.
pub fn fmt_elapsed(ms: i64) -> String {
    hms(ms.max(0) as u64 / 1000)
}

/// Compact form for the side bubble: under an hour "m:ss", else "1h05".
pub fn fmt_bubble(ms: i64) -> String {
    let s = (ms.max(0) as u64).div_ceil(1000);
    if s >= 3600 {
        format!("{}h{:02}", s / 3600, (s % 3600) / 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// 300 -> "5 min", 5400 -> "1 h 30", 90 -> "90 s".
pub fn preset_label(secs: u64) -> String {
    if secs.is_multiple_of(3600) {
        format!("{} h", secs / 3600)
    } else if secs >= 3600 {
        format!("{} h {}", secs / 3600, (secs % 3600) / 60)
    } else if secs.is_multiple_of(60) {
        format!("{} min", secs / 60)
    } else {
        format!("{secs} s")
    }
}

/// Time until an event less than an hour away: "now", "in 7 min"; None otherwise.
pub fn countdown(delta_ms: i64) -> Option<String> {
    if delta_ms <= 0 {
        return Some("now".into());
    }
    let m = (delta_ms as u64).div_ceil(60_000);
    (m < 60).then(|| format!("in {m} min"))
}

/// 190 -> "3 h 10 min", 45 -> "45 min".
pub fn fmt_minutes(m: Option<u64>) -> String {
    match m {
        None => "–".into(),
        Some(m) if m < 60 => format!("{m} min"),
        Some(m) if m % 60 == 0 => format!("{} h", m / 60),
        Some(m) => format!("{} h {} min", m / 60, m % 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_presets() {
        assert_eq!(parse("5"), Some(300));
        assert_eq!(parse("5m"), Some(300));
        assert_eq!(parse("90s"), Some(90));
        assert_eq!(parse("1h30m"), Some(5400));
        assert_eq!(parse("1h 5m 10s"), Some(3910));
        assert_eq!(parse("25 M"), Some(1500));
        assert_eq!(parse("99h"), Some(86400));
        assert_eq!(parse("0"), None);
        assert_eq!(parse("abc"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn formats() {
        assert_eq!(fmt_remaining(65_000), "1:05");
        assert_eq!(fmt_remaining(64_001), "1:05");
        assert_eq!(fmt_remaining(3_725_000), "1:02:05");
        assert_eq!(fmt_remaining(-5), "0:00");
        assert_eq!(fmt_elapsed(65_999), "1:05");
        assert_eq!(fmt_bubble(3_900_000), "1h05");
        assert_eq!(fmt_bubble(61_000), "1:01");
        assert_eq!(preset_label(300), "5 min");
        assert_eq!(preset_label(5400), "1 h 30");
        assert_eq!(preset_label(90), "90 s");
        assert_eq!(preset_label(7200), "2 h");
        assert_eq!(countdown(0).as_deref(), Some("now"));
        assert_eq!(countdown(61_000).as_deref(), Some("in 2 min"));
        assert_eq!(countdown(3_600_000), None);
        assert_eq!(fmt_minutes(None), "–");
        assert_eq!(fmt_minutes(Some(190)), "3 h 10 min");
        assert_eq!(fmt_minutes(Some(120)), "2 h");
    }
}
