//! A small iCalendar (.ics) reader: the events of a calendar feed, without recurrence rules.
//!
//! Supports UTC ("...Z"), floating and TZID-tagged local times (the latter two are read in the offset the
//! caller passes: no time-zone database here), all-day dates, folded lines and the usual escapes. Events with
//! an RRULE are expanded for DAILY/WEEKLY rules with COUNT/UNTIL/INTERVAL within a window; other rules show
//! their first occurrence only.

use crate::calendar::Event;

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// "20261118", "20261118T163000", "20261118T153000Z" -> (epoch ms, is_date_only).
fn parse_time(v: &str, local_offset_secs: i32) -> Option<(i64, bool)> {
    let v = v.trim();
    let digits = |s: &str| s.parse::<i64>().ok();
    if v.len() < 8 || !v.is_char_boundary(8) {
        return None;
    }
    let (y, m, d) = (digits(&v[0..4])?, digits(&v[4..6])?, digits(&v[6..8])?);
    let days = days_from_civil(y, m, d);
    if v.len() == 8 {
        return Some(((days * 86_400 - local_offset_secs as i64) * 1000, true));
    }
    let t = v.get(9..)?;
    if t.len() < 4 {
        return None;
    }
    let (hh, mm) = (digits(&t[0..2])?, digits(&t[2..4])?);
    let ss = t.get(4..6).and_then(digits).unwrap_or(0);
    let secs = days * 86_400 + hh * 3600 + mm * 60 + ss;
    let off = if v.ends_with('Z') { 0 } else { local_offset_secs as i64 };
    Some(((secs - off) * 1000, false))
}

fn unescape(s: &str) -> String {
    s.replace("\\n", "\n").replace("\\N", "\n").replace("\\,", ",").replace(r"\;", ";").replace("\\\\", "\\")
}

/// Unfolds continuation lines (a line starting with a space or tab continues the previous one).
fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix([' ', '\t']) {
            if let Some(last) = out.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        out.push(line.to_string());
    }
    out
}

fn first_url(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == '"').find(|w| w.starts_with("https://") || w.starts_with("http://")).map(|s| s.trim_end_matches(['.', ',', ')']).to_string())
}

/// Meeting links people put in the location, description or URL (Teams, Meet, Zoom, ...).
fn meeting_url(url: &Option<String>, location: &str, description: &str) -> Option<String> {
    let known = ["teams.microsoft.com", "meet.google.com", "zoom.us", "webex.com", "whereby.com", "meet.jit.si"];
    for src in [location, description] {
        if let Some(u) = first_url(src).filter(|u| known.iter().any(|k| u.contains(k))) {
            return Some(u);
        }
    }
    url.clone().or_else(|| first_url(location))
}

struct Raw {
    uid: String,
    summary: String,
    location: String,
    description: String,
    url: Option<String>,
    start: Option<(i64, bool)>,
    end: Option<(i64, bool)>,
    rrule: Option<String>,
}

/// Events overlapping [from_ms, to_ms), sorted by start. `local_offset_secs` reads floating times.
pub fn parse(text: &str, local_offset_secs: i32, from_ms: i64, to_ms: i64) -> Vec<Event> {
    let mut events = Vec::new();
    let mut cur: Option<Raw> = None;
    for line in unfold(text) {
        if line == "BEGIN:VEVENT" {
            cur = Some(Raw { uid: String::new(), summary: String::new(), location: String::new(), description: String::new(), url: None, start: None, end: None, rrule: None });
            continue;
        }
        if line == "END:VEVENT" {
            if let Some(r) = cur.take() {
                expand(&r, from_ms, to_ms, &mut events);
            }
            continue;
        }
        let Some(r) = cur.as_mut() else { continue };
        let Some((head, value)) = line.split_once(':') else { continue };
        let name = head.split(';').next().unwrap_or("").to_ascii_uppercase();
        match name.as_str() {
            "UID" => r.uid = value.to_string(),
            "SUMMARY" => r.summary = unescape(value),
            "LOCATION" => r.location = unescape(value),
            "DESCRIPTION" => r.description = unescape(value),
            "URL" => r.url = Some(value.to_string()),
            "DTSTART" => r.start = parse_time(value, local_offset_secs),
            "DTEND" => r.end = parse_time(value, local_offset_secs),
            "RRULE" => r.rrule = Some(value.to_string()),
            _ => {}
        }
    }
    events.sort_by_key(|e| e.start_ms);
    events
}

fn expand(r: &Raw, from_ms: i64, to_ms: i64, out: &mut Vec<Event>) {
    let Some((start, all_day)) = r.start else { return };
    let end = r.end.map(|e| e.0).unwrap_or(if all_day { start + 86_400_000 } else { start });
    let len = (end - start).max(0);
    let mut starts = vec![start];
    if let Some(rule) = &r.rrule {
        let get = |k: &str| rule.split(';').find_map(|p| p.strip_prefix(&format!("{k}=")));
        let step_days = match get("FREQ") {
            Some("DAILY") => Some(1),
            Some("WEEKLY") => Some(7),
            _ => None,
        };
        if let Some(step) = step_days {
            let interval = get("INTERVAL").and_then(|v| v.parse::<i64>().ok()).unwrap_or(1).max(1);
            let count = get("COUNT").and_then(|v| v.parse::<usize>().ok());
            let until = get("UNTIL").and_then(|v| parse_time(v, 0)).map(|t| t.0);
            let mut n = 1usize;
            let mut t = start;
            while n < count.unwrap_or(usize::MAX) && n < 2000 {
                t += step * interval * 86_400_000;
                if until.is_some_and(|u| t > u) || t > to_ms {
                    break;
                }
                starts.push(t);
                n += 1;
            }
        }
    }
    let url = meeting_url(&r.url, &r.location, &r.description);
    for s in starts {
        if s < to_ms && s + len.max(1) > from_ms {
            out.push(Event { id: format!("{}@{}", r.uid, s), title: r.summary.clone(), start_ms: s, end_ms: s + len, all_day, url: url.clone(), color: None });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICS: &str = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a1\r\nSUMMARY:Reti di calcolatori\\, lezione\r\nDTSTART:20261118T153000Z\r\nDTEND:20261118T170000Z\r\nLOCATION:https://teams.microsoft.com/l/meetup-join/abc\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:a2\r\nSUMMARY:Holiday\r\nDTSTART;VALUE=DATE:20261120\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:a3\r\nSUMMARY:Weekly\r\n sync\r\nDTSTART:20261116T090000\r\nDTEND:20261116T093000\r\nRRULE:FREQ=WEEKLY;COUNT=3\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    fn ms(y: i64, m: i64, d: i64, h: i64, mi: i64) -> i64 {
        (days_from_civil(y, m, d) * 86_400 + h * 3600 + mi * 60) * 1000
    }

    #[test]
    fn reads_events() {
        let evs = parse(ICS, 3600, 0, ms(2027, 1, 1, 0, 0));
        assert_eq!(evs.len(), 5, "{evs:#?}");
        let a = evs.iter().find(|e| e.id.starts_with("a1")).unwrap();
        assert_eq!(a.title, "Reti di calcolatori, lezione");
        assert_eq!(a.start_ms, ms(2026, 11, 18, 15, 30));
        assert_eq!(a.end_ms, ms(2026, 11, 18, 17, 0));
        assert_eq!(a.url.as_deref(), Some("https://teams.microsoft.com/l/meetup-join/abc"));
        let h = evs.iter().find(|e| e.id.starts_with("a2")).unwrap();
        assert!(h.all_day);
        assert_eq!(h.start_ms, ms(2026, 11, 19, 23, 0), "all-day starts at local midnight (UTC+1)");
        assert_eq!(h.end_ms - h.start_ms, 86_400_000);
    }

    #[test]
    fn floating_times_use_the_local_offset_and_folding_works() {
        let evs = parse(ICS, 3600, 0, ms(2027, 1, 1, 0, 0));
        let w: Vec<_> = evs.iter().filter(|e| e.id.starts_with("a3")).collect();
        assert_eq!(w.len(), 3);
        assert_eq!(w[0].title, "Weeklysync");
        assert_eq!(w[0].start_ms, ms(2026, 11, 16, 8, 0));
        assert_eq!(w[1].start_ms - w[0].start_ms, 7 * 86_400_000);
    }

    #[test]
    fn the_window_filters_and_garbage_is_ignored() {
        let evs = parse(ICS, 0, ms(2026, 11, 18, 0, 0), ms(2026, 11, 19, 0, 0));
        assert_eq!(evs.len(), 1);
        assert!(parse("not a calendar", 0, 0, i64::MAX).is_empty());
        assert!(parse("BEGIN:VEVENT\nSUMMARY:x\nEND:VEVENT", 0, 0, i64::MAX).is_empty(), "no DTSTART");
    }
}
