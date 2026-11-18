//! An iCalendar (.ics) reader: the events of a calendar feed or a CalDAV answer, with recurrence.
//!
//! Handles UTC ("...Z"), floating and TZID-tagged local times (the caller supplies the time-zone rules through
//! an offset function, so the core needs no time-zone database), all-day dates, folded lines, escapes, DURATION,
//! STATUS:CANCELLED, EXDATE, RECURRENCE-ID overrides and RRULE (DAILY, WEEKLY, MONTHLY, YEARLY with INTERVAL,
//! COUNT, UNTIL, BYDAY with ordinals, BYMONTHDAY, BYMONTH, WKST). Recurrence runs on the wall clock of the event's
//! own time zone, so a 14:00 lecture stays at 14:00 across a daylight-saving change.

use crate::calendar::{civil_from_days, days_from_civil, days_in_month, weekday_of_days, Event};
use std::collections::HashSet;

/// (time-zone id of the event, UTC seconds) -> offset from UTC in seconds. `None` is the local zone.
pub type OffsetFn<'a> = &'a dyn Fn(Option<&str>, i64) -> i32;

/// A time as written in the file.
#[derive(Clone, Debug, PartialEq)]
struct When {
    /// Seconds since 1970-01-01: UTC when `utc`, else the wall clock read as if it were UTC.
    secs: i64,
    utc: bool,
    date_only: bool,
    tzid: Option<String>,
}

impl When {
    fn to_ms(&self, off: OffsetFn) -> i64 {
        if self.utc {
            return self.secs * 1000;
        }
        let tz = self.tzid.as_deref();
        // the offset at that wall time: ask at the instant it would be if the offset were the one found
        let first = off(tz, self.secs) as i64;
        let second = off(tz, self.secs - first) as i64;
        (self.secs - second) * 1000
    }
}

fn digits(s: &str) -> Option<i64> {
    s.parse::<i64>().ok()
}

fn parse_when(head: &str, value: &str) -> Option<When> {
    let v = value.trim();
    if v.len() < 8 || !v.is_char_boundary(8) {
        return None;
    }
    let tzid = head.split(';').skip(1).find_map(|p| p.strip_prefix("TZID=")).map(|t| t.trim_matches('"').to_string());
    let days = days_from_civil(digits(&v[0..4])?, digits(&v[4..6])?, digits(&v[6..8])?);
    if v.len() == 8 {
        return Some(When { secs: days * 86_400, utc: false, date_only: true, tzid });
    }
    let t = v.get(9..)?;
    if t.len() < 4 {
        return None;
    }
    let secs = days * 86_400 + digits(&t[0..2])? * 3600 + digits(&t[2..4])? * 60 + t.get(4..6).and_then(digits).unwrap_or(0);
    Some(When { secs, utc: v.ends_with('Z'), date_only: false, tzid })
}

/// "PT1H30M", "P1D", "P1W" -> milliseconds.
fn parse_duration(v: &str) -> Option<i64> {
    let v = v.trim().trim_start_matches('+');
    let rest = v.strip_prefix('P')?;
    let (mut total, mut num, mut in_time) = (0i64, String::new(), false);
    for c in rest.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => num.push(c),
            _ => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                total += n * match (c, in_time) {
                    ('W', _) => 604_800,
                    ('D', _) => 86_400,
                    ('H', true) => 3600,
                    ('M', true) => 60,
                    ('S', true) => 1,
                    _ => return None,
                };
            }
        }
    }
    Some(total * 1000)
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
fn meeting_url(url: &Option<String>, conference: &str, location: &str, description: &str) -> Option<String> {
    let known = ["teams.microsoft.com", "teams.live.com", "meet.google.com", "zoom.us", "zoomgov.com", "webex.com", "whereby.com", "meet.jit.si", "gotomeet.me", "gotomeeting.com", "bluejeans.com", "around.co", "slack.com/huddle", "discord.gg", "skype.com", "chime.aws", "meet.goto.com", "whereby.com", "tuple.app"];
    for src in [conference, location, description] {
        if let Some(u) = first_url(src).filter(|u| known.iter().any(|k| u.contains(k))) {
            return Some(u);
        }
    }
    url.clone().or_else(|| first_url(location))
}

/// The calendar's own name (X-WR-CALNAME), when the feed has one.
pub fn calendar_name(text: &str) -> Option<String> {
    unfold(text).into_iter().find_map(|l| {
        let (head, value) = l.split_once(':')?;
        (head.split(';').next()?.eq_ignore_ascii_case("X-WR-CALNAME")).then(|| unescape(value).trim().to_string()).filter(|n| !n.is_empty())
    })
}

// ---- recurrence rules -----------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Clone, Debug)]
struct Rule {
    freq: Freq,
    interval: i64,
    count: Option<usize>,
    until: Option<When>,
    /// (ordinal, weekday 0 = Monday): `2TU` is (Some(2), 1), `-1FR` is (Some(-1), 4), `MO` is (None, 0).
    by_day: Vec<(Option<i64>, i64)>,
    by_month_day: Vec<i64>,
    by_month: Vec<i64>,
    wkst: i64,
}

fn weekday_code(s: &str) -> Option<i64> {
    ["MO", "TU", "WE", "TH", "FR", "SA", "SU"].iter().position(|d| *d == s).map(|i| i as i64)
}

fn parse_rule(value: &str) -> Option<Rule> {
    let get = |k: &str| value.split(';').find_map(|p| p.strip_prefix(&format!("{k}=")));
    let freq = match get("FREQ")? {
        "DAILY" => Freq::Daily,
        "WEEKLY" => Freq::Weekly,
        "MONTHLY" => Freq::Monthly,
        "YEARLY" => Freq::Yearly,
        _ => return None,
    };
    let list = |k: &str| -> Vec<i64> { get(k).map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default() };
    let by_day = get("BYDAY")
        .map(|v| {
            v.split(',')
                .filter_map(|d| {
                    let d = d.trim();
                    let split = d.len().saturating_sub(2);
                    let (ord, code) = d.split_at(split);
                    Some((if ord.is_empty() { None } else { Some(ord.parse().ok()?) }, weekday_code(code)?))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Rule {
        freq,
        interval: get("INTERVAL").and_then(digits).unwrap_or(1).max(1),
        count: get("COUNT").and_then(|c| c.parse().ok()),
        until: get("UNTIL").and_then(|u| parse_when("", u)),
        by_day,
        by_month_day: list("BYMONTHDAY"),
        by_month: list("BYMONTH"),
        wkst: get("WKST").and_then(weekday_code).unwrap_or(0),
    })
}

/// The days of a month matching BYMONTHDAY / BYDAY (with ordinals), ascending; `default_day` when neither is set.
fn month_days(rule: &Rule, y: i64, m: i64, default_day: i64) -> Vec<i64> {
    let dim = days_in_month(y as i32, m as u32) as i64;
    let first = days_from_civil(y, m, 1);
    let mut out: Vec<i64> = vec![];
    if !rule.by_month_day.is_empty() {
        out.extend(rule.by_month_day.iter().filter_map(|d| {
            let d = if *d < 0 { dim + 1 + d } else { *d };
            (1..=dim).contains(&d).then_some(d)
        }));
    }
    if !rule.by_day.is_empty() {
        for (ord, wd) in &rule.by_day {
            let matching: Vec<i64> = (1..=dim).filter(|d| weekday_of_days(first + d - 1) == *wd).collect();
            match ord {
                None => out.extend(matching),
                Some(n) if *n > 0 => out.extend(matching.get(*n as usize - 1).copied()),
                Some(n) => out.extend(matching.len().checked_sub(n.unsigned_abs() as usize).and_then(|i| matching.get(i)).copied()),
            }
        }
        if !rule.by_month_day.is_empty() {
            // both set: only the days in both
            let by_md: HashSet<i64> = rule.by_month_day.iter().map(|d| if *d < 0 { dim + 1 + d } else { *d }).collect();
            out.retain(|d| by_md.contains(d) && rule.by_day.iter().any(|(_, wd)| weekday_of_days(first + d - 1) == *wd));
        }
    }
    if rule.by_month_day.is_empty() && rule.by_day.is_empty() && default_day <= dim {
        out.push(default_day);
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// The wall-clock start times (seconds, as in `When::secs`) of a rule, from `start` while `keep(secs)` says the
/// instant is still worth looking at. `keep` returning false stops the expansion (past the window or UNTIL).
fn expand(rule: &Rule, start: i64, keep: &dyn Fn(i64) -> bool) -> Vec<i64> {
    const MAX: usize = 4000;
    let (start_day, tod) = (start.div_euclid(86_400), start.rem_euclid(86_400));
    let (sy, sm, sd) = civil_from_days(start_day);
    let mut out = vec![];
    let emit = |day: i64, out: &mut Vec<i64>| -> bool {
        if day < start_day {
            return true;
        }
        let secs = day * 86_400 + tod;
        if !keep(secs) {
            return false;
        }
        out.push(secs);
        rule.count.is_none_or(|c| out.len() < c) && out.len() < MAX
    };
    match rule.freq {
        Freq::Daily => {
            let mut day = start_day;
            'daily: for _ in 0..MAX * 4 {
                let (y, m, d) = civil_from_days(day);
                let ok = (rule.by_day.is_empty() || rule.by_day.iter().any(|(_, wd)| *wd == weekday_of_days(day)))
                    && (rule.by_month_day.is_empty() || rule.by_month_day.iter().any(|x| *x == d || (*x < 0 && days_in_month(y as i32, m as u32) as i64 + 1 + x == d)))
                    && (rule.by_month.is_empty() || rule.by_month.contains(&m));
                if ok && !emit(day, &mut out) {
                    break 'daily;
                }
                day += rule.interval;
                if !keep(day * 86_400 + tod) && day > start_day {
                    break;
                }
            }
        }
        Freq::Weekly => {
            let days: Vec<i64> = if rule.by_day.is_empty() { vec![weekday_of_days(start_day)] } else { rule.by_day.iter().map(|(_, wd)| *wd).collect() };
            let week0 = start_day - (weekday_of_days(start_day) - rule.wkst).rem_euclid(7);
            'weekly: for k in 0..MAX as i64 {
                let week = week0 + 7 * rule.interval * k;
                let mut offsets: Vec<i64> = days.iter().map(|wd| (wd - rule.wkst).rem_euclid(7)).collect();
                offsets.sort_unstable();
                offsets.dedup();
                for o in offsets {
                    if !emit(week + o, &mut out) {
                        break 'weekly;
                    }
                }
                if !keep((week + 7 * rule.interval) * 86_400) && week > start_day {
                    break;
                }
            }
        }
        Freq::Monthly => {
            'monthly: for k in 0..MAX as i64 {
                let idx = sy * 12 + (sm - 1) + k * rule.interval;
                let (y, m) = (idx.div_euclid(12), idx.rem_euclid(12) + 1);
                if rule.by_month.is_empty() || rule.by_month.contains(&m) {
                    for d in month_days(rule, y, m, sd) {
                        if !emit(days_from_civil(y, m, d), &mut out) {
                            break 'monthly;
                        }
                    }
                }
                if !keep(days_from_civil(y, m, 1) * 86_400 + tod) && k > 0 {
                    break;
                }
            }
        }
        Freq::Yearly => {
            'yearly: for k in 0..400i64 {
                let y = sy + k * rule.interval;
                let months: Vec<i64> = if rule.by_month.is_empty() { vec![sm] } else { let mut v = rule.by_month.clone(); v.sort_unstable(); v };
                for m in months {
                    for d in month_days(rule, y, m, sd) {
                        if !emit(days_from_civil(y, m, d), &mut out) {
                            break 'yearly;
                        }
                    }
                }
                if !keep(days_from_civil(y + rule.interval, 1, 1) * 86_400) && k > 0 {
                    break;
                }
            }
        }
    }
    out
}

// ---- events ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Raw {
    uid: String,
    summary: String,
    location: String,
    description: String,
    url: Option<String>,
    /// Google's `X-GOOGLE-CONFERENCE` (the Meet link of an invitation).
    conference: String,
    start: Option<When>,
    end: Option<When>,
    duration_ms: Option<i64>,
    rrule: Option<Rule>,
    exdates: Vec<When>,
    recurrence_id: Option<When>,
    cancelled: bool,
}

/// Events overlapping [from_ms, to_ms), sorted by start. `local_offset_secs` is the offset used for every
/// non-UTC time (no time-zone rules): see `parse_in` for the real thing.
pub fn parse(text: &str, local_offset_secs: i32, from_ms: i64, to_ms: i64) -> Vec<Event> {
    parse_in(text, &|_, _| local_offset_secs, from_ms, to_ms)
}

/// Events overlapping [from_ms, to_ms), sorted by start, with their recurrences expanded.
pub fn parse_in(text: &str, off: OffsetFn, from_ms: i64, to_ms: i64) -> Vec<Event> {
    let mut raws: Vec<Raw> = vec![];
    let mut cur: Option<Raw> = None;
    for line in unfold(text) {
        if line == "BEGIN:VEVENT" {
            cur = Some(Raw::default());
            continue;
        }
        if line == "END:VEVENT" {
            raws.extend(cur.take());
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
            "X-GOOGLE-CONFERENCE" | "X-MICROSOFT-SKYPETEAMSMEETINGURL" => r.conference = value.to_string(),
            "DTSTART" => r.start = parse_when(head, value),
            "DTEND" => r.end = parse_when(head, value),
            "DURATION" => r.duration_ms = parse_duration(value),
            "RRULE" => r.rrule = parse_rule(value),
            "EXDATE" => r.exdates.extend(value.split(',').filter_map(|v| parse_when(head, v))),
            "RECURRENCE-ID" => r.recurrence_id = parse_when(head, value),
            "STATUS" => r.cancelled = value.trim().eq_ignore_ascii_case("CANCELLED"),
            _ => {}
        }
    }
    // an override replaces the occurrence it names, even when it moves or cancels it
    let overridden: HashSet<(String, i64)> = raws.iter().filter_map(|r| Some((r.uid.clone(), r.recurrence_id.as_ref()?.to_ms(off)))).collect();
    let mut events = vec![];
    for r in &raws {
        if r.cancelled {
            continue;
        }
        let Some(start) = &r.start else { continue };
        let start_ms = start.to_ms(off);
        let len = match (&r.end, r.duration_ms) {
            (Some(e), _) => (e.to_ms(off) - start_ms).max(0),
            (None, Some(d)) => d,
            (None, None) if start.date_only => 86_400_000,
            _ => 0,
        };
        let url = meeting_url(&r.url, &r.conference, &r.location, &r.description);
        let mut push = |s: i64| {
            if s < to_ms && s + len.max(1) > from_ms {
                events.push(Event { id: format!("{}@{}", r.uid, s), title: r.summary.clone(), start_ms: s, end_ms: s + len, all_day: start.date_only, url: url.clone(), color: None, calendar: String::new() });
            }
        };
        match (&r.rrule, r.recurrence_id.is_some()) {
            (Some(rule), false) => {
                let until_ms = rule.until.as_ref().map(|u| u.to_ms(off));
                let excluded: HashSet<i64> = r.exdates.iter().map(|e| e.to_ms(off)).collect();
                let keep = |secs: i64| {
                    let ms = When { secs, ..start.clone() }.to_ms(off);
                    ms < to_ms && until_ms.is_none_or(|u| ms <= u)
                };
                for secs in expand(rule, start.secs, &keep) {
                    let ms = When { secs, ..start.clone() }.to_ms(off);
                    if !excluded.contains(&ms) && !overridden.contains(&(r.uid.clone(), ms)) {
                        push(ms);
                    }
                }
            }
            _ => push(start_ms),
        }
    }
    events.sort_by_key(|e| e.start_ms);
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(y: i64, m: i64, d: i64, h: i64, mi: i64) -> i64 {
        (days_from_civil(y, m, d) * 86_400 + h * 3600 + mi * 60) * 1000
    }

    /// Rome: UTC+1 until the last Sunday of October, then UTC+1 again after... for these tests a fixed rule:
    /// +2 from 2026-03-29 01:00 UTC to 2026-10-25 01:00 UTC, else +1.
    fn rome(_tz: Option<&str>, utc: i64) -> i32 {
        let (dst_start, dst_end) = (days_from_civil(2026, 3, 29) * 86_400 + 3600, days_from_civil(2026, 10, 25) * 86_400 + 3600);
        if (dst_start..dst_end).contains(&utc) { 7200 } else { 3600 }
    }

    fn ev(vevent: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\n{vevent}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n")
    }

    fn titles_on(evs: &[Event], y: i64, m: i64, d: i64) -> Vec<String> {
        let (a, b) = (ms(y, m, d, 0, 0) - 3_600_000, ms(y, m, d + 1, 0, 0) - 3_600_000);
        evs.iter().filter(|e| e.start_ms >= a && e.start_ms < b).map(|e| e.title.clone()).collect()
    }

    const ICS: &str = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a1\r\nSUMMARY:Reti di calcolatori\\, lezione\r\nDTSTART:20261118T153000Z\r\nDTEND:20261118T170000Z\r\nLOCATION:https://teams.microsoft.com/l/meetup-join/abc\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:a2\r\nSUMMARY:Holiday\r\nDTSTART;VALUE=DATE:20261120\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:a3\r\nSUMMARY:Weekly\r\n sync\r\nDTSTART:20261116T090000\r\nDTEND:20261116T093000\r\nRRULE:FREQ=WEEKLY;COUNT=3\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn finds_the_meeting_link_wherever_the_invitation_puts_it() {
        let ev = |extra: &str| parse(&format!("BEGIN:VEVENT\r\nUID:m\r\nSUMMARY:Sync\r\nDTSTART:20261118T100000Z\r\nDTEND:20261118T103000Z\r\n{extra}\r\nEND:VEVENT"), 0, 0, i64::MAX).remove(0).url;
        assert_eq!(ev("X-GOOGLE-CONFERENCE:https://meet.google.com/abc-defg-hij"), Some("https://meet.google.com/abc-defg-hij".into()));
        assert_eq!(ev("DESCRIPTION:Join: https://us02web.zoom.us/j/123?pwd=x\\nBye"), Some("https://us02web.zoom.us/j/123?pwd=x".into()), "in the description, not swallowing the next line");
        assert_eq!(ev("DESCRIPTION:Notes at https://example.org/doc\r\nLOCATION:https://webex.com/meet/me"), Some("https://webex.com/meet/me".into()), "a known service beats a random link");
        assert_eq!(ev("DESCRIPTION:no link here"), None);
    }

    #[test]
    fn reads_events() {
        let evs = parse(ICS, 3600, 0, ms(2027, 1, 1, 0, 0));
        assert_eq!(evs.len(), 5, "{evs:#?}");
        let a = evs.iter().find(|e| e.id.starts_with("a1")).unwrap();
        assert_eq!(a.title, "Reti di calcolatori, lezione");
        assert_eq!((a.start_ms, a.end_ms), (ms(2026, 11, 18, 15, 30), ms(2026, 11, 18, 17, 0)));
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

    #[test]
    fn reads_the_calendar_name() {
        assert_eq!(calendar_name("BEGIN:VCALENDAR\r\nX-WR-CALNAME:Corsi\\, 2026\r\nEND:VCALENDAR").as_deref(), Some("Corsi, 2026"));
        assert_eq!(calendar_name("BEGIN:VCALENDAR\r\nX-WR-CALNAME;VALUE=TEXT:Lavoro\r\n").as_deref(), Some("Lavoro"));
        assert_eq!(calendar_name(ICS), None);
        assert_eq!(calendar_name("X-WR-CALNAME:  "), None);
    }

    /// The case that went missing: a lecture on Wednesdays and Thursdays, started on a Wednesday.
    #[test]
    fn a_weekly_rule_with_several_days_gives_every_one_of_them() {
        let text = ev("UID:ap\r\nSUMMARY:Advanced Programming\r\nDTSTART;TZID=W. Europe Standard Time:20261014T140000\r\nDTEND;TZID=W. Europe Standard Time:20261014T160000\r\nRRULE:FREQ=WEEKLY;UNTIL=20261203T130000Z;INTERVAL=1;BYDAY=WE,TH;WKST=MO");
        let evs = parse_in(&text, &rome, ms(2026, 11, 17, 0, 0), ms(2026, 11, 22, 0, 0));
        assert_eq!(titles_on(&evs, 2026, 11, 18), vec!["Advanced Programming"], "Wednesday");
        assert_eq!(titles_on(&evs, 2026, 11, 19), vec!["Advanced Programming"], "Thursday: the one that was missing");
        assert!(titles_on(&evs, 2026, 11, 20).is_empty());
        let thursday = evs.iter().find(|e| e.start_ms > ms(2026, 11, 19, 0, 0)).unwrap();
        assert_eq!(thursday.start_ms, ms(2026, 11, 19, 13, 0), "14:00 Rome in November is 13:00 UTC");
        assert_eq!(thursday.end_ms - thursday.start_ms, 7_200_000);
    }

    #[test]
    fn until_exdate_count_and_interval_are_honoured() {
        let base = "UID:x\r\nSUMMARY:T\r\nDTSTART:20261102T090000Z\r\nDTEND:20261102T100000Z\r\n";
        let all = |extra: &str| parse_in(&ev(&format!("{base}{extra}")), &rome, 0, ms(2027, 3, 1, 0, 0));
        assert_eq!(all("RRULE:FREQ=WEEKLY;UNTIL=20261116T090000Z").len(), 3, "UNTIL is inclusive");
        assert_eq!(all("RRULE:FREQ=WEEKLY;COUNT=4;BYDAY=MO,TU").len(), 4, "COUNT counts occurrences, not weeks");
        assert_eq!(all("RRULE:FREQ=WEEKLY;INTERVAL=2;COUNT=3").iter().map(|e| e.start_ms).collect::<Vec<_>>(), vec![ms(2026, 11, 2, 9, 0), ms(2026, 11, 16, 9, 0), ms(2026, 11, 30, 9, 0)]);
        let ex = all("RRULE:FREQ=WEEKLY;COUNT=3\r\nEXDATE:20261109T090000Z");
        assert_eq!(ex.iter().map(|e| e.start_ms).collect::<Vec<_>>(), vec![ms(2026, 11, 2, 9, 0), ms(2026, 11, 16, 9, 0)], "the excluded week is skipped");
        assert_eq!(all("RRULE:FREQ=DAILY;COUNT=5;BYDAY=MO,WE,FR").len(), 5);
    }

    #[test]
    fn monthly_and_yearly_rules() {
        let base = |start: &str, rule: &str| ev(&format!("UID:m\r\nSUMMARY:M\r\nDTSTART:{start}\r\nDTEND:{start}\r\nRRULE:{rule}"));
        let days = |text: &str| parse_in(text, &|_, _| 0, 0, ms(2028, 1, 1, 0, 0)).iter().map(|e| (civil_from_days(e.start_ms / 86_400_000).1, civil_from_days(e.start_ms / 86_400_000).2)).collect::<Vec<_>>();
        // the 31st skips the short months
        assert_eq!(days(&base("20260131T100000Z", "FREQ=MONTHLY;COUNT=4")), vec![(1, 31), (3, 31), (5, 31), (7, 31)]);
        // the second Tuesday of each month (November 2026: the 10th)
        assert_eq!(days(&base("20261110T100000Z", "FREQ=MONTHLY;BYDAY=2TU;COUNT=3")), vec![(11, 10), (12, 8), (1, 12)]);
        // the last Friday
        assert_eq!(days(&base("20261127T100000Z", "FREQ=MONTHLY;BYDAY=-1FR;COUNT=3")), vec![(11, 27), (12, 25), (1, 29)]);
        // a birthday every year, and the Outlook daylight-saving rule shape
        assert_eq!(days(&base("20260314T100000Z", "FREQ=YEARLY")), vec![(3, 14), (3, 14)], "2026 and 2027, then the window ends");
        // Outlook's daylight-saving rule shape: the last Sunday of October. 2026's (the 25th) is before the start
        // date (the 26th), so the first one is 2027's (the 31st); the next, 2028's, is past the window
        assert_eq!(days(&base("20261026T100000Z", "FREQ=YEARLY;BYDAY=-1SU;BYMONTH=10;COUNT=2")), vec![(10, 31)]);
    }

    #[test]
    fn the_wall_clock_survives_a_daylight_saving_change() {
        // 14:00 Rome every week across the end of summer time (2026-10-25)
        let text = ev("UID:d\r\nSUMMARY:L\r\nDTSTART;TZID=Europe/Rome:20261019T140000\r\nDTEND;TZID=Europe/Rome:20261019T150000\r\nRRULE:FREQ=WEEKLY;COUNT=2");
        let evs = parse_in(&text, &rome, 0, ms(2027, 1, 1, 0, 0));
        assert_eq!(evs[0].start_ms, ms(2026, 10, 19, 12, 0), "UTC+2 in October");
        assert_eq!(evs[1].start_ms, ms(2026, 10, 26, 13, 0), "UTC+1 a week later: still 14:00 on the wall");
    }

    #[test]
    fn overrides_replace_their_occurrence_and_cancelled_ones_vanish() {
        let text = format!(
            "{}{}{}",
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:r\r\nSUMMARY:Lecture\r\nDTSTART:20261102T090000Z\r\nDTEND:20261102T100000Z\r\nRRULE:FREQ=WEEKLY;COUNT=3\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:r\r\nRECURRENCE-ID:20261109T090000Z\r\nSUMMARY:Lecture (moved)\r\nDTSTART:20261110T150000Z\r\nDTEND:20261110T160000Z\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:r\r\nRECURRENCE-ID:20261116T090000Z\r\nSTATUS:CANCELLED\r\nSUMMARY:Lecture\r\nDTSTART:20261116T090000Z\r\nDTEND:20261116T100000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
        );
        let evs = parse_in(&text, &rome, 0, ms(2027, 1, 1, 0, 0));
        assert_eq!(evs.iter().map(|e| (e.title.as_str(), e.start_ms)).collect::<Vec<_>>(), vec![("Lecture", ms(2026, 11, 2, 9, 0)), ("Lecture (moved)", ms(2026, 11, 10, 15, 0))]);
        assert!(parse_in(&ev("UID:c\r\nSUMMARY:Gone\r\nSTATUS:CANCELLED\r\nDTSTART:20261102T090000Z"), &rome, 0, i64::MAX).is_empty());
    }

    #[test]
    fn duration_and_all_day_lengths() {
        let evs = parse_in(&ev("UID:d\r\nSUMMARY:D\r\nDTSTART:20261102T090000Z\r\nDURATION:PT1H30M"), &rome, 0, i64::MAX);
        assert_eq!(evs[0].end_ms - evs[0].start_ms, 5_400_000);
        let two_days = parse_in(&ev("UID:d\r\nSUMMARY:D\r\nDTSTART;VALUE=DATE:20261102\r\nDTEND;VALUE=DATE:20261104"), &rome, 0, i64::MAX);
        assert_eq!(two_days[0].end_ms - two_days[0].start_ms, 2 * 86_400_000);
        assert_eq!(parse_duration("P1DT2H"), Some(93_600_000));
        assert_eq!(parse_duration("P1W"), Some(604_800_000));
        assert_eq!(parse_duration("nonsense"), None);
    }

    #[test]
    fn a_runaway_rule_is_bounded() {
        let text = ev("UID:z\r\nSUMMARY:Z\r\nDTSTART:19700101T000000Z\r\nDTEND:19700101T010000Z\r\nRRULE:FREQ=DAILY");
        let evs = parse_in(&text, &rome, ms(2026, 11, 1, 0, 0), ms(2026, 12, 1, 0, 0));
        assert!(evs.len() <= 4000, "bounded, got {}", evs.len());
    }
}
