//! Looking ahead: how full each day of the week is, how much to study today to reach a deadline, whether a new event
//! collides with another, and finding a word across what the island knows. Pure.

use crate::calendar::Event;
use crate::task::fmt_minutes;

/// Timed events that overlap [start, end) (an all-day event does not block a time).
pub fn conflicts(events: &[Event], start_ms: i64, end_ms: i64) -> Vec<&Event> {
    events.iter().filter(|e| !e.all_day && e.start_ms < end_ms && e.end_ms > start_ms).collect()
}

/// One day of the week at a glance.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DayLoad {
    pub day: i64,
    /// Minutes of timed events.
    pub event_min: u32,
    pub events: u32,
    /// Tasks and deadlines that fall on the day.
    pub due: u32,
    /// Minutes of study planned for the day (see `study_share`).
    pub study_min: u32,
}

impl DayLoad {
    /// A day to be careful with: a lot of lecture time, several things due, or a long day in all.
    pub fn heavy(&self) -> bool {
        self.event_min >= 6 * 60 || self.due >= 3 || self.event_min + self.study_min >= 9 * 60
    }
}

/// What the island has to plan around: a task or a deadline with its day and an estimate of the work left.
#[derive(Clone, Debug, PartialEq)]
pub struct Load {
    pub label: String,
    pub course: String,
    pub due: i64,
    pub est_min: u32,
}

/// The minutes to give this load on `day`: what is left, spread over the days until it is due (all of it on the
/// day itself), in steps of five minutes. The work starts today, so each share is small and the days stay level.
pub fn study_share(l: &Load, day: i64) -> u32 {
    if l.est_min == 0 || l.due < day {
        return 0;
    }
    let days = (l.due - day).max(1) as u32;
    let raw = if l.due == day { l.est_min } else { l.est_min.div_ceil(days) };
    raw.div_ceil(5) * 5
}

/// The week from `days[0]` on: `ranges` are the (start, end) of each day in ms, `loads` what is due and planned.
pub fn week(events: &[Event], ranges: &[(i64, i64)], first_day: i64, loads: &[Load]) -> Vec<DayLoad> {
    ranges
        .iter()
        .enumerate()
        .map(|(i, (s, e))| {
            let day = first_day + i as i64;
            let timed: Vec<&Event> = events.iter().filter(|ev| !ev.all_day && ev.start_ms < *e && ev.end_ms > *s).collect();
            let event_min = timed.iter().map(|ev| ((ev.end_ms.min(*e) - ev.start_ms.max(*s)) / 60_000).max(0) as u32).sum();
            DayLoad {
                day,
                event_min,
                events: timed.len() as u32,
                due: loads.iter().filter(|l| l.due == day).count() as u32,
                study_min: loads.iter().map(|l| study_share(l, day)).sum(),
            }
        })
        .collect()
}

/// "3 events · 4h30", "free", for a day's load.
pub fn day_summary(d: &DayLoad) -> String {
    let mut p = vec![];
    if d.events > 0 {
        p.push(format!("{} event{} · {}", d.events, if d.events == 1 { "" } else { "s" }, fmt_minutes(d.event_min)));
    }
    if d.due > 0 {
        p.push(format!("{} due", d.due));
    }
    if d.study_min > 0 {
        p.push(format!("study {}", fmt_minutes(d.study_min)));
    }
    if p.is_empty() { "free".into() } else { p.join(" · ") }
}

/// Does a course name belong to an item (its own course, or a word of its text)? Case does not matter.
pub fn course_matches(course: &str, item_course: &str, item_text: &str) -> bool {
    let c = course.trim().to_lowercase();
    !c.is_empty() && (item_course.to_lowercase() == c || item_text.to_lowercase().contains(&c))
}

/// Where a studied time goes: the load of that course that is due first (a position in `loads`).
pub fn load_for_course(loads: &[Load], course: &str) -> Option<usize> {
    loads.iter().enumerate().filter(|(_, l)| course_matches(course, &l.course, &l.label)).min_by_key(|(_, l)| l.due).map(|(i, _)| i)
}

/// Minutes studied per course over the entries (day, course, minutes) that fall on or after `since_day`, most first.
pub fn studied_per_course(log: &[(i64, String, u32)], since_day: i64) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = vec![];
    for (_, course, min) in log.iter().filter(|(d, _, _)| *d >= since_day) {
        let name = if course.is_empty() { "(no course)".to_string() } else { course.to_lowercase() };
        match out.iter_mut().find(|(c, _)| *c == name) {
            Some((_, m)) => *m += min,
            None => out.push((name, *min)),
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

/// What kind of thing a search found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind {
    Event,
    Task,
    Deadline,
    Pin,
    Note,
    File,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub kind: HitKind,
    pub title: String,
    /// When or where: "Tue 24 Nov · 09:30", "notes/2026-11-20.md".
    pub detail: String,
    /// What a click does: a file to open, text to copy; empty when nothing.
    pub target: String,
}

/// Every word of `query` appears in `hay` (case and accents of à è ì ò ù do not matter).
pub fn matches(hay: &str, query: &str) -> bool {
    let fold = |s: &str| s.to_lowercase().replace(['à', 'á'], "a").replace(['è', 'é'], "e").replace(['ì', 'í'], "i").replace(['ò', 'ó'], "o").replace(['ù', 'ú'], "u");
    let hay = fold(hay);
    let words: Vec<String> = query.split_whitespace().map(fold).collect();
    !words.is_empty() && words.iter().all(|w| hay.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 3_600_000;
    fn ev(title: &str, start_h: i64, end_h: i64) -> Event {
        Event { id: title.into(), title: title.into(), start_ms: start_h * H, end_ms: end_h * H, ..Default::default() }
    }

    #[test]
    fn a_new_event_collides_only_with_timed_events_that_overlap() {
        let mut all_day = ev("Holiday", 0, 24);
        all_day.all_day = true;
        let evs = vec![ev("Algoritmi", 9, 11), ev("Reti", 11, 13), all_day];
        assert_eq!(conflicts(&evs, 10 * H, 12 * H).iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["Algoritmi", "Reti"]);
        assert!(conflicts(&evs, 13 * H, 14 * H).is_empty(), "touching is not overlapping");
        assert!(conflicts(&evs, 11 * H - 1, 11 * H).iter().any(|e| e.title == "Algoritmi"));
    }

    #[test]
    fn study_is_spread_over_the_days_left() {
        let l = Load { label: "Algoritmi".into(), course: String::new(), due: 10, est_min: 600 };
        assert_eq!(study_share(&l, 0), 60, "ten hours over ten days");
        assert_eq!(study_share(&l, 9), 600, "one day left: all of it");
        assert_eq!(study_share(&Load { due: 10, ..l.clone() }, 10), 600, "on the day itself");
        assert_eq!(study_share(&l, 11), 0, "passed");
        assert_eq!(study_share(&Load { est_min: 0, ..l.clone() }, 0), 0);
        assert_eq!(study_share(&Load { est_min: 100, due: 3, ..l }, 0), 35, "33.4 rounds up to the next five");
    }

    #[test]
    fn a_week_counts_hours_and_what_is_due_and_flags_a_heavy_day() {
        let day0 = 100;
        let ranges: Vec<(i64, i64)> = (0..7).map(|i| (i * 24 * H, (i + 1) * 24 * H)).collect();
        let evs = vec![ev("A", 9, 11), ev("B", 12, 15), ev("C", 24 + 9, 24 + 10)];
        let loads = vec![Load { label: "x".into(), course: String::new(), due: day0 + 1, est_min: 0 }, Load { label: "y".into(), course: String::new(), due: day0 + 1, est_min: 0 }, Load { label: "z".into(), course: String::new(), due: day0 + 1, est_min: 0 }];
        let w = week(&evs, &ranges, day0, &loads);
        assert_eq!(w.len(), 7);
        assert_eq!((w[0].events, w[0].event_min, w[0].due), (2, 300, 0));
        assert_eq!((w[1].events, w[1].event_min, w[1].due), (1, 60, 3));
        assert!(!w[0].heavy() && w[1].heavy(), "three things due is a heavy day");
        assert_eq!(day_summary(&w[0]), "2 events · 5h");
        assert_eq!(day_summary(&w[3]), "free");
        assert!(DayLoad { event_min: 360, ..Default::default() }.heavy());
    }

    #[test]
    fn studied_time_goes_to_the_course_due_first() {
        let loads = vec![
            Load { label: "Reti exam".into(), course: String::new(), due: 20, est_min: 0 },
            Load { label: "Algoritmi exam".into(), course: String::new(), due: 30, est_min: 0 },
            Load { label: "Report".into(), course: "algoritmi".into(), due: 12, est_min: 0 },
        ];
        assert_eq!(load_for_course(&loads, "Algoritmi"), Some(2), "the report is due before the exam");
        assert_eq!(load_for_course(&loads, "reti"), Some(0));
        assert_eq!(load_for_course(&loads, "chimica"), None);
        assert_eq!(load_for_course(&loads, ""), None);
    }

    #[test]
    fn studied_minutes_add_up_per_course_since_a_day() {
        let log = vec![(1, "Reti".to_string(), 30), (5, "reti".to_string(), 60), (6, "Algoritmi".to_string(), 45), (7, String::new(), 10)];
        assert_eq!(studied_per_course(&log, 5), vec![("reti".to_string(), 60), ("algoritmi".to_string(), 45), ("(no course)".to_string(), 10)]);
        assert_eq!(studied_per_course(&log, 0)[0], ("reti".to_string(), 90));
    }

    #[test]
    fn search_needs_every_word_and_ignores_case_and_accents() {
        assert!(matches("Lezione di Programmazione", "programmazione lezione"));
        assert!(matches("Università", "universita"));
        assert!(!matches("Reti", "reti algoritmi"));
        assert!(!matches("anything", "  "));
    }
}
