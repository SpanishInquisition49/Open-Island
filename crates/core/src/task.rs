//! Tasks: something to do, with an optional day, course, time estimate and repetition, and how a task travels as a
//! VTODO (the to-do of CalDAV). Pure: the day is always passed in, as days since 1970-01-01 (local).

use crate::calendar::{civil_from_days, days_from_civil, days_in_month, weekday_of_days};
use crate::quick::Ymd;

const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const BYDAY: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
const WEEKDAYS: u8 = 0b001_1111;

/// How often something comes back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Repeat {
    #[default]
    Never,
    Daily,
    /// Days of the week, bit 0 Monday .. bit 6 Sunday.
    Weekly(u8),
    /// A day of the month (a short month uses its last day).
    Monthly(u8),
}

impl Repeat {
    /// The first day strictly after `day` that it falls on; None for `Never`.
    pub fn next_after(self, day: i64) -> Option<i64> {
        match self {
            Repeat::Never => None,
            Repeat::Daily => Some(day + 1),
            Repeat::Weekly(0) => None,
            Repeat::Weekly(mask) => (1..=7).map(|n| day + n).find(|d| mask & (1 << weekday_of_days(*d)) != 0),
            Repeat::Monthly(dom) => {
                let (y, m, _) = civil_from_days(day);
                (0..3).find_map(|k| {
                    let idx = y * 12 + (m - 1) + k;
                    let (ny, nm) = (idx.div_euclid(12), idx.rem_euclid(12) + 1);
                    let d = days_from_civil(ny, nm, (dom as i64).min(days_in_month(ny as i32, nm as u32) as i64));
                    (d > day).then_some(d)
                })
            }
        }
    }

    /// The first day on or after `day` that it falls on.
    pub fn on_or_after(self, day: i64) -> Option<i64> {
        self.next_after(day - 1)
    }

    /// "every day", "every weekday", "every Mon, Thu", "monthly on the 27th".
    pub fn label(self) -> String {
        match self {
            Repeat::Never => String::new(),
            Repeat::Daily => "every day".into(),
            Repeat::Weekly(m) if m == WEEKDAYS => "every weekday".into(),
            Repeat::Weekly(m) => format!("every {}", (0..7).filter(|i| m & (1 << i) != 0).map(|i| DAYS[i]).collect::<Vec<_>>().join(", ")),
            Repeat::Monthly(d) => format!("monthly on the {}", ordinal(d as i64)),
        }
    }

    /// The short form for a row: "daily", "weekdays", "Mon, Thu", "27th".
    pub fn short(self) -> String {
        match self {
            Repeat::Never => String::new(),
            Repeat::Daily => "daily".into(),
            Repeat::Weekly(m) if m == WEEKDAYS => "weekdays".into(),
            Repeat::Weekly(m) => (0..7).filter(|i| m & (1 << i) != 0).map(|i| DAYS[i]).collect::<Vec<_>>().join(", "),
            Repeat::Monthly(d) => ordinal(d as i64),
        }
    }

    /// The RRULE of an iCalendar entry (without the `RRULE:`).
    pub fn to_rrule(self) -> Option<String> {
        match self {
            Repeat::Never | Repeat::Weekly(0) => None,
            Repeat::Daily => Some("FREQ=DAILY".into()),
            Repeat::Weekly(m) => Some(format!("FREQ=WEEKLY;BYDAY={}", (0..7).filter(|i| m & (1 << i) != 0).map(|i| BYDAY[i]).collect::<Vec<_>>().join(","))),
            Repeat::Monthly(d) => Some(format!("FREQ=MONTHLY;BYMONTHDAY={d}")),
        }
    }

    /// Reads the RRULE of an entry; a rule this cannot express (an interval, a count, a yearly one) is `Never`.
    pub fn from_rrule(rule: &str) -> Repeat {
        let part = |key: &str| rule.split(';').find_map(|p| p.strip_prefix(key).and_then(|v| v.strip_prefix('=')));
        if part("INTERVAL").is_some_and(|i| i != "1") || part("COUNT").is_some() {
            return Repeat::Never;
        }
        match part("FREQ") {
            Some("DAILY") => Repeat::Daily,
            Some("WEEKLY") => {
                let mask = part("BYDAY").map_or(0, |v| v.split(',').filter_map(|d| BYDAY.iter().position(|b| *b == d)).fold(0u8, |m, i| m | 1 << i));
                if mask == 0 { Repeat::Never } else { Repeat::Weekly(mask) }
            }
            Some("MONTHLY") => part("BYMONTHDAY").and_then(|d| d.parse::<u8>().ok()).filter(|d| (1..=31).contains(d)).map_or(Repeat::Never, Repeat::Monthly),
            _ => Repeat::Never,
        }
    }
}

fn ordinal(n: i64) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// A repetition at the start of `words` (lowercase), and how many words it took: `daily`, `every day`, `every weekday`,
/// `every monday`, `every mon and thu`, `ogni martedì e giovedì`, `every month on the 27th`, `ogni mese il 27`.
pub fn parse_repeat(words: &[&str]) -> Option<(Repeat, usize)> {
    let first = *words.first()?;
    if matches!(first, "daily" | "giornaliero" | "quotidiano") {
        return Some((Repeat::Daily, 1));
    }
    if !matches!(first, "every" | "ogni") {
        return None;
    }
    let second = words.get(1)?.trim_end_matches(',');
    match second {
        "day" | "giorno" => return Some((Repeat::Daily, 2)),
        "weekday" | "weekdays" | "feriale" | "lavorativo" => return Some((Repeat::Weekly(WEEKDAYS), 2)),
        "month" | "mese" => {
            let mut i = 2;
            if matches!(words.get(i), Some(&("on" | "il" | "the" | "on the"))) {
                i += 1;
            }
            if words.get(i) == Some(&"the") {
                i += 1;
            }
            let n = words.get(i)?.trim_end_matches(|c: char| c.is_alphabetic() || c == '°' || c == 'º').parse::<u8>().ok().filter(|d| (1..=31).contains(d))?;
            return Some((Repeat::Monthly(n), i + 1));
        }
        _ => {}
    }
    let (mut mask, mut j) = (0u8, 1);
    while let Some(w) = words.get(j) {
        let Some(d) = crate::bar::weekday_of(w.trim_end_matches(',')) else { break };
        mask |= 1 << d;
        j += 1;
        if matches!(words.get(j), Some(&("and" | "e" | "&"))) && words.get(j + 1).is_some_and(|n| crate::bar::weekday_of(n.trim_end_matches(',')).is_some()) {
            j += 1;
        }
    }
    (mask != 0).then_some((Repeat::Weekly(mask), j))
}

/// The longest piece of work: an exam can take hundreds of hours.
const MAX_MINUTES: f64 = 999.0 * 60.0;

/// "2h", "30m", "1h30m", "1h30", "1.5h", "90min", "45" (a bare number is minutes) as minutes.
pub fn parse_minutes(s: &str) -> Option<u32> {
    let s = s.trim().to_lowercase();
    let (mut total, mut rest, mut any) = (0.0f64, s.as_str(), false);
    while !rest.is_empty() {
        let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',')).unwrap_or(rest.len());
        let n: f64 = rest[..end].replace(',', ".").parse().ok()?;
        rest = &rest[end..];
        let uend = rest.find(|c: char| !c.is_alphabetic()).unwrap_or(rest.len());
        let unit = &rest[..uend];
        rest = &rest[uend..];
        let per = match unit {
            "h" | "hr" | "hrs" | "hour" | "hours" | "o" | "ora" | "ore" => 60.0,
            "m" | "min" | "mins" | "minute" | "minutes" | "minuti" | "" => 1.0,
            _ => return None,
        };
        total += n * per;
        any = true;
    }
    (any && (1.0..=MAX_MINUTES).contains(&total)).then_some(total.round() as u32)
}

/// "1h30", "45m", "2h" for a count of minutes.
pub fn fmt_minutes(m: u32) -> String {
    match (m / 60, m % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h{m:02}"),
    }
}

/// A day at the start of `words`: anything `quick::parse_date` reads, or a weekday name (the next one, never today).
pub fn parse_day(words: &[&str], today: Ymd) -> Option<(i64, usize)> {
    let t = days_from_civil(today.0, today.1, today.2);
    let skip = usize::from(matches!(words.first(), Some(&("next" | "prossimo" | "prossima" | "this" | "questo" | "questa")))) ;
    if let Some(d) = words.get(skip).and_then(|w| crate::bar::weekday_of(w)) {
        let ahead = (d as i64 - weekday_of_days(t)).rem_euclid(7);
        return Some((t + if ahead == 0 { 7 } else { ahead }, skip + 1));
    }
    crate::quick::parse_date(words, today).map(|(d, used)| (days_from_civil(d.0, d.1, d.2), used))
}

/// "today", "tomorrow", "Fri", "3 Feb", "2 d late".
pub fn due_label(due: i64, today: i64) -> String {
    match due - today {
        n if n < 0 => format!("{} d late", -n),
        0 => "today".into(),
        1 => "tomorrow".into(),
        2..=6 => DAYS[weekday_of_days(due) as usize].into(),
        _ => {
            let (_, m, d) = civil_from_days(due);
            format!("{d} {}", MONTHS[(m - 1) as usize])
        }
    }
}

/// What a line about a task says: the words left, and what was read out of them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spec {
    pub text: String,
    pub due: Option<i64>,
    pub course: String,
    pub est_min: u32,
    pub repeat: Repeat,
}

impl Spec {
    /// "Fri · #algoritmi · 2h · every Mon": what was understood, for the preview.
    pub fn summary(&self, today: Option<i64>) -> String {
        let mut p = vec![];
        if let (Some(d), Some(t)) = (self.due, today) {
            p.push(due_label(d, t));
        }
        if !self.course.is_empty() {
            p.push(format!("#{}", self.course));
        }
        if self.est_min > 0 {
            p.push(fmt_minutes(self.est_min));
        }
        if self.repeat != Repeat::Never {
            p.push(self.repeat.label());
        }
        p.join(" · ")
    }
}

/// Reads the words of a task: `due fri`, `entro 3 feb`, `tomorrow`, `#course`, `~2h`, `every monday`, `daily`.
/// The rest is the text, in its own capitals. Without `today` no day is read (the words stay in the text).
pub fn parse_spec(orig: &[&str], today: Option<Ymd>) -> Spec {
    let lower: Vec<String> = orig.iter().map(|w| w.to_lowercase()).collect();
    let lw: Vec<&str> = lower.iter().map(String::as_str).collect();
    let mut spec = Spec::default();
    let mut keep = vec![true; orig.len()];
    let mut i = 0;
    while i < lw.len() {
        let w = lw[i];
        if spec.course.is_empty() && w.len() > 1 && w.starts_with('#') {
            spec.course = w[1..].trim_end_matches([',', '.']).replace('_', " ");
            keep[i] = false;
        } else if spec.est_min == 0 && w.starts_with('~') && parse_minutes(&w[1..]).is_some() {
            spec.est_min = parse_minutes(&w[1..]).unwrap_or(0);
            keep[i] = false;
        } else if let Some(t) = today.filter(|_| spec.due.is_none() && matches!(w, "due" | "by" | "entro" | "scadenza" | "until")) {
            if let Some((day, used)) = parse_day(&lw[i + 1..], t) {
                spec.due = Some(day);
                keep[i..=i + used].fill(false);
                i += 1 + used;
                continue;
            }
        } else if today.is_some() && spec.due.is_none() && matches!(w, "today" | "tomorrow" | "oggi" | "domani") {
            let t = today.unwrap_or((1970, 1, 1));
            spec.due = parse_day(&lw[i..=i], t).map(|(d, _)| d);
            keep[i] = false;
        } else if spec.repeat == Repeat::Never {
            if let Some((r, used)) = parse_repeat(&lw[i..]) {
                spec.repeat = r;
                keep[i..i + used].fill(false);
                i += used;
                continue;
            }
        }
        i += 1;
    }
    spec.text = orig.iter().zip(&keep).filter(|(_, k)| **k).map(|(w, _)| *w).collect::<Vec<_>>().join(" ");
    if let (Some(t), true) = (today, spec.repeat != Repeat::Never) {
        let t = days_from_civil(t.0, t.1, t.2);
        // a repeating task starts on the first day it falls on, from the day asked for (or today)
        spec.due = spec.repeat.on_or_after(spec.due.unwrap_or(t)).or(spec.due);
    }
    spec
}

/// A task: what is on the to-do list, and what travels to the server.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Task {
    /// The id on a server (and for ever on this machine); empty until one is made.
    pub uid: String,
    pub text: String,
    /// The day it is due. For a repeating task: the next day it comes round.
    pub due: Option<i64>,
    /// A word of an event title (a course), or empty.
    pub course: String,
    pub est_min: u32,
    pub repeat: Repeat,
    /// The server has (had) it: when it is no longer there, it was finished somewhere else.
    pub synced: bool,
}

impl Task {
    pub fn from_spec(s: &Spec, uid: String) -> Task {
        Task { uid, text: s.text.clone(), due: s.due, course: s.course.clone(), est_min: s.est_min, repeat: s.repeat, synced: false }
    }

    /// A repeating task rests until its day comes.
    pub fn visible(&self, today: i64) -> bool {
        self.repeat == Repeat::Never || self.due.is_none_or(|d| d <= today)
    }

    /// Ticks it off. True: it is done for good. False: it comes back (its `due` is now the next day).
    pub fn complete(&mut self, today: i64) -> bool {
        match self.repeat.next_after(today.max(self.due.unwrap_or(today))) {
            Some(next) => {
                self.due = Some(next);
                false
            }
            None => true,
        }
    }

    /// Sorts the late first, then today's, then the rest by day, then those without one.
    pub fn sort_key(&self) -> i64 {
        self.due.unwrap_or(i64::MAX)
    }
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace(';', r"\;").replace(',', "\\,").replace('\n', "\\n")
}

fn ymd(day: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    format!("{y:04}{m:02}{d:02}")
}

/// The iCalendar document of a task (`stamp` is now in UTC, "20261122T101500Z").
pub fn to_vtodo(t: &Task, done: bool, stamp: &str) -> String {
    let mut l = vec!["BEGIN:VCALENDAR".to_string(), "VERSION:2.0".into(), "PRODID:-//open-island//tasks//EN".into(), "BEGIN:VTODO".into()];
    l.push(format!("UID:{}", t.uid));
    l.push(format!("DTSTAMP:{stamp}"));
    l.push(format!("SUMMARY:{}", esc(&t.text)));
    if let Some(d) = t.due {
        l.push(format!("DUE;VALUE=DATE:{}", ymd(d)));
    }
    if !t.course.is_empty() {
        l.push(format!("CATEGORIES:{}", esc(&t.course)));
    }
    if let Some(r) = t.repeat.to_rrule() {
        l.push(format!("RRULE:{r}"));
    }
    if t.est_min > 0 {
        l.push(format!("X-OPEN-ISLAND-ESTIMATE:{}", t.est_min));
    }
    if done {
        l.push("STATUS:COMPLETED".into());
        l.push("PERCENT-COMPLETE:100".into());
        l.push(format!("COMPLETED:{stamp}"));
    } else {
        l.push("STATUS:NEEDS-ACTION".into());
    }
    l.extend(["END:VTODO".to_string(), "END:VCALENDAR".into()]);
    l.join("\r\n") + "\r\n"
}

/// The tasks of an iCalendar document, with whether each is finished (completed or cancelled).
pub fn from_vtodo(text: &str) -> Vec<(Task, bool)> {
    let mut out = vec![];
    let mut cur: Option<(Task, bool)> = None;
    for line in crate::ics::unfold(text) {
        let Some((head, value)) = line.split_once(':') else { continue };
        let name = head.split(';').next().unwrap_or("").to_ascii_uppercase();
        match (name.as_str(), cur.as_mut()) {
            ("BEGIN", _) if value.eq_ignore_ascii_case("VTODO") => cur = Some((Task { synced: true, ..Default::default() }, false)),
            ("END", _) if value.eq_ignore_ascii_case("VTODO") => out.extend(cur.take().filter(|(t, _)| !t.uid.is_empty() && !t.text.is_empty())),
            ("UID", Some((t, _))) => t.uid = value.trim().to_string(),
            ("SUMMARY", Some((t, _))) => t.text = crate::ics::unescape(value).trim().to_string(),
            ("DUE", Some((t, _))) => t.due = date_of(value),
            ("DTSTART", Some((t, _))) => t.due = t.due.or_else(|| date_of(value)),
            ("CATEGORIES", Some((t, _))) => t.course = crate::ics::unescape(value.split(',').next().unwrap_or("")).trim().to_string(),
            ("RRULE", Some((t, _))) => t.repeat = Repeat::from_rrule(value),
            ("X-OPEN-ISLAND-ESTIMATE", Some((t, _))) => t.est_min = value.trim().parse().unwrap_or(0),
            ("STATUS", Some((_, done))) => *done = matches!(value.trim().to_ascii_uppercase().as_str(), "COMPLETED" | "CANCELLED"),
            ("COMPLETED", Some((_, done))) => *done = true,
            _ => {}
        }
    }
    out
}

fn date_of(v: &str) -> Option<i64> {
    let v = v.trim();
    let (y, m, d) = (v.get(0..4)?.parse().ok()?, v.get(4..6)?.parse().ok()?, v.get(6..8)?.parse().ok()?);
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then(|| days_from_civil(y, m, d))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Sunday 22 November 2026
    const TODAY: Ymd = (2026, 11, 22);
    fn day(y: i64, m: i64, d: i64) -> i64 {
        days_from_civil(y, m, d)
    }
    fn spec(line: &str) -> Spec {
        parse_spec(&line.split_whitespace().collect::<Vec<_>>(), Some(TODAY))
    }

    #[test]
    fn a_task_line_gives_the_day_the_course_the_estimate_and_the_repetition() {
        let s = spec("Finish the Report due fri #algoritmi ~2h");
        assert_eq!((s.text.as_str(), s.course.as_str(), s.est_min), ("Finish the Report", "algoritmi", 120));
        assert_eq!(s.due, Some(day(2026, 11, 27)), "the next Friday");
        let s = spec("entro 3 feb relazione Reti #reti_di_calcolatori ~1h30m");
        assert_eq!((s.text.as_str(), s.course.as_str(), s.est_min, s.due), ("relazione Reti", "reti di calcolatori", 90, Some(day(2027, 2, 3))));
        assert_eq!(spec("call mum tomorrow").due, Some(day(2026, 11, 23)));
        assert_eq!(spec("call mum tomorrow").text, "call mum");
        assert_eq!(spec("by the book").due, None, "'by' without a day stays in the text");
        assert_eq!(spec("by the book").text, "by the book");
    }

    #[test]
    fn a_repeating_task_starts_on_its_first_day() {
        let s = spec("bring the charger daily");
        assert_eq!((s.repeat, s.due, s.text.as_str()), (Repeat::Daily, Some(day(2026, 11, 22)), "bring the charger"));
        let s = spec("lab report every tuesday and thursday");
        assert_eq!(s.repeat, Repeat::Weekly(0b0001010));
        assert_eq!(s.due, Some(day(2026, 11, 24)), "Tuesday");
        assert_eq!(spec("daily lunch").text, "lunch", "the word may come first, as before");
        assert_eq!(spec("rent every month on the 27th").repeat, Repeat::Monthly(27));
        assert_eq!(spec("pagare ogni mese il 5").repeat, Repeat::Monthly(5));
        assert_eq!(spec("palestra ogni lunedì e giovedì").repeat, Repeat::Weekly(0b0001001));
        assert_eq!(spec("standup every weekday").repeat, Repeat::Weekly(WEEKDAYS));
        assert_eq!(spec("every now and then").repeat, Repeat::Never);
    }

    #[test]
    fn without_today_no_day_is_read() {
        let s = parse_spec(&["call", "tomorrow", "#reti"], None);
        assert_eq!((s.due, s.text.as_str(), s.course.as_str()), (None, "call tomorrow", "reti"));
    }

    #[test]
    fn repetition_finds_the_next_day() {
        let sun = day(2026, 11, 22);
        assert_eq!(Repeat::Daily.next_after(sun), Some(sun + 1));
        assert_eq!(Repeat::Weekly(0b0000001).next_after(sun), Some(sun + 1), "Monday");
        assert_eq!(Repeat::Weekly(0b1000000).next_after(sun), Some(sun + 7), "a week to the next Sunday");
        assert_eq!(Repeat::Weekly(0b1000000).on_or_after(sun), Some(sun));
        assert_eq!(Repeat::Monthly(31).next_after(day(2027, 2, 1)), Some(day(2027, 2, 28)), "a short month uses its last day");
        assert_eq!(Repeat::Monthly(15).next_after(day(2026, 12, 20)), Some(day(2027, 1, 15)));
        assert_eq!(Repeat::Never.next_after(sun), None);
        assert_eq!((Repeat::Daily.short().as_str(), Repeat::Weekly(0b0001001).short().as_str(), Repeat::Weekly(WEEKDAYS).short().as_str(), Repeat::Monthly(2).short().as_str()), ("daily", "Mon, Thu", "weekdays", "2nd"));
    }

    #[test]
    fn ticking_a_repeating_task_moves_it_to_the_next_day() {
        let mut t = Task { text: "gym".into(), due: Some(day(2026, 11, 19)), repeat: Repeat::Weekly(0b0001001), ..Default::default() };
        assert!(!t.complete(day(2026, 11, 22)), "three days late, done today");
        assert_eq!(t.due, Some(day(2026, 11, 23)), "Monday");
        assert!(!t.visible(day(2026, 11, 22)), "it rests until Monday");
        assert!(t.visible(day(2026, 11, 23)));
        let mut once = Task { text: "x".into(), ..Default::default() };
        assert!(once.complete(0));
    }

    #[test]
    fn minutes_and_labels() {
        for (s, m) in [("2h", 120), ("30m", 30), ("1h30m", 90), ("1h30", 90), ("1.5h", 90), ("90min", 90), ("45", 45), ("2ore", 120)] {
            assert_eq!(parse_minutes(s), Some(m), "{s}");
        }
        assert_eq!((parse_minutes("x"), parse_minutes(""), parse_minutes("0m"), parse_minutes("3d")), (None, None, None, None));
        assert_eq!((parse_minutes("120h"), parse_minutes("1000h")), (Some(7200), None), "the work for an exam");
        assert_eq!((fmt_minutes(90), fmt_minutes(120), fmt_minutes(45)), ("1h30".into(), "2h".into(), "45m".into()));
        let t = day(2026, 11, 22);
        assert_eq!((due_label(t, t).as_str(), due_label(t + 1, t).as_str(), due_label(t + 5, t).as_str(), due_label(t + 20, t).as_str(), due_label(t - 2, t).as_str()), ("today", "tomorrow", "Fri", "12 Dec", "2 d late"));
    }

    #[test]
    fn a_task_makes_the_round_trip_as_a_vtodo() {
        let t = Task { uid: "oi-1".into(), text: "Report, final; v2".into(), due: Some(day(2026, 11, 27)), course: "algoritmi".into(), est_min: 120, repeat: Repeat::Weekly(0b0001010), synced: true };
        let ics = to_vtodo(&t, false, "20261122T101500Z");
        assert!(ics.contains("DUE;VALUE=DATE:20261127\r\n") && ics.contains("RRULE:FREQ=WEEKLY;BYDAY=TU,TH\r\n") && ics.contains("STATUS:NEEDS-ACTION"));
        assert_eq!(from_vtodo(&ics), vec![(t.clone(), false)]);
        let done = to_vtodo(&t, true, "20261122T101500Z");
        assert!(from_vtodo(&done)[0].1);
        assert!(done.contains("COMPLETED:20261122T101500Z"));
    }

    #[test]
    fn a_vtodo_from_another_app_is_read_leniently() {
        let ics = "BEGIN:VCALENDAR\nBEGIN:VTODO\nUID:abc\nSUMMARY:Buy\n  milk\nDUE:20270105T090000Z\nCATEGORIES:home,errand\nRRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=MO\nEND:VTODO\nBEGIN:VTODO\nUID:def\nSUMMARY:Old\nSTATUS:CANCELLED\nEND:VTODO\nBEGIN:VTODO\nSUMMARY:No id\nEND:VTODO\nEND:VCALENDAR";
        let v = from_vtodo(ics);
        assert_eq!(v.len(), 2, "a to-do without an id is skipped");
        assert_eq!((v[0].0.text.as_str(), v[0].0.due, v[0].0.course.as_str(), v[0].0.repeat), ("Buy milk", Some(day(2027, 1, 5)), "home", Repeat::Never), "a fortnightly rule is not claimed");
        assert!(!v[0].1 && v[1].1);
    }
}
