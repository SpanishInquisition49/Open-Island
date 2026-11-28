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
    /// Minutes of timed events, overlaps counted once, and how they divide among the calendars.
    pub event_min: u32,
    pub calendars: Vec<Busy>,
    pub events: u32,
    /// Tasks and deadlines that fall on the day.
    pub due: u32,
    /// Minutes of study planned for the day (see `spread`).
    pub study_min: u32,
    /// Minutes of study only suggested for it (see `share_out`): they fill free time, so they do not make a day heavy.
    pub suggested_min: u32,
}

impl DayLoad {
    /// A day to be careful with: a lot of lecture time, several things due, or a long day in all.
    pub fn heavy(&self) -> bool {
        self.event_min >= 6 * 60 || self.due >= 3 || self.event_min + self.study_min >= 9 * 60
    }
}

/// The step of a change to the study hours, and the shortest stretch they can be.
const HOURS_STEP: u16 = 30;
const DAY_MIN: u16 = 24 * 60;

/// The hours of a day of the week in which one studies, in minutes from midnight: from the start to the end, with a
/// pause (lunch) in between if there is one, which makes them two stretches. The plan counts only these: what happens
/// before, after or during the pause takes nothing from them. A day off keeps its hours, to come back to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StudyHours {
    pub on: bool,
    pub from: u16,
    pub to: u16,
    /// (when it begins, when it ends), inside the day.
    pub pause: Option<(u16, u16)>,
}

impl Default for StudyHours {
    fn default() -> Self {
        Self { on: true, from: 9 * 60, to: 17 * 60, pause: None }
    }
}

/// One of the times of a day that can be moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Start,
    PauseStart,
    PauseEnd,
    End,
}

/// The seven days, Monday first.
pub type StudyWeek = [StudyHours; 7];

impl StudyHours {
    /// The stretches of study, in minutes from midnight: none on a day off, two with a pause.
    pub fn stretches(&self) -> Vec<(u16, u16)> {
        match self.pause {
            _ if !self.on => vec![],
            Some((out, back)) => vec![(self.from, out), (back, self.to)],
            None => vec![(self.from, self.to)],
        }
    }

    /// The same on the day that begins at `midnight_ms`.
    pub fn windows(&self, midnight_ms: i64) -> Vec<(i64, i64)> {
        let at = |min: u16| midnight_ms + min as i64 * 60_000;
        self.stretches().into_iter().map(|(a, b)| (at(a), at(b))).collect()
    }

    /// The minutes of study the hours hold (also those of a day off, to come back to).
    pub fn minutes(&self) -> u32 {
        u32::from(self.to - self.from) - self.pause.map_or(0, |(out, back)| u32::from(back - out))
    }

    /// Moves one of the times half an hour later or earlier, as far as the day and the times beside it allow.
    pub fn step(&mut self, edge: Edge, later: bool) {
        let moved = |t: u16| if later { t.saturating_add(HOURS_STEP) } else { t.saturating_sub(HOURS_STEP) };
        match (edge, self.pause) {
            (Edge::Start, pause) => self.from = moved(self.from).min(pause.map_or(self.to, |p| p.0) - HOURS_STEP),
            (Edge::End, pause) => self.to = moved(self.to).clamp(pause.map_or(self.from, |p| p.1) + HOURS_STEP, DAY_MIN),
            (Edge::PauseStart, Some((out, back))) => self.pause = Some((moved(out).clamp(self.from + HOURS_STEP, back - HOURS_STEP), back)),
            (Edge::PauseEnd, Some((out, back))) => self.pause = Some((out, moved(back).clamp(out + HOURS_STEP, self.to - HOURS_STEP))),
            (_, None) => {}
        }
    }

    /// Puts a pause in the day, or takes it out: from 13 to 14 when the day goes well past both, else an hour in its
    /// middle. A day too short to be cut in two stays whole.
    pub fn toggle_pause(&mut self) {
        if self.pause.take().is_some() {
            return;
        }
        let lunch = (13 * 60, 14 * 60);
        if self.from < lunch.0 && lunch.1 < self.to {
            self.pause = Some(lunch);
            return;
        }
        let middle = (self.from + self.to) / 2 / HOURS_STEP * HOURS_STEP;
        let (out, back) = (middle.saturating_sub(HOURS_STEP), middle + HOURS_STEP);
        if self.from < out && back < self.to {
            self.pause = Some((out, back));
        }
    }

    /// "09:00-17:00", or "09:00-13:00+14:00-18:00" with a pause; "off " before it on a day off.
    pub fn text(&self) -> String {
        let span = |a: u16, b: u16| format!("{}-{}", fmt_clock(a), fmt_clock(b));
        let hours = match self.pause {
            Some((out, back)) => format!("{}+{}", span(self.from, out), span(back, self.to)),
            None => span(self.from, self.to),
        };
        format!("{}{hours}", if self.on { "" } else { "off " })
    }

    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (on, hours) = text.strip_prefix("off ").map_or((true, text), |rest| (false, rest.trim()));
        let spans: Vec<(u16, u16)> = hours
            .split('+')
            .map(|span| {
                let (from, to) = span.split_once('-')?;
                let (from, to) = (parse_clock(from)?, parse_clock(to)?);
                (from < to).then_some((from, to))
            })
            .collect::<Option<_>>()?;
        match spans[..] {
            [(from, to)] => Some(Self { on, from, to, pause: None }),
            [(from, out), (back, to)] if out < back => Some(Self { on, from, to, pause: Some((out, back)) }),
            _ => None,
        }
    }
}

/// "09:00" (up to "24:00", the end of the day).
pub fn fmt_clock(min: u16) -> String {
    format!("{:02}:{:02}", min / 60, min % 60)
}

fn parse_clock(text: &str) -> Option<u16> {
    let (h, m) = text.trim().split_once(':')?;
    let (h, m) = (h.parse::<u16>().ok()?, m.parse::<u16>().ok()?);
    (m < 60 && (h < 24 || (h == 24 && m == 0))).then_some(h * 60 + m)
}

/// The week as one line, the days separated by commas.
pub fn week_text(week: &StudyWeek) -> String {
    week.iter().map(StudyHours::text).collect::<Vec<_>>().join(",")
}

pub fn parse_week(text: &str) -> Option<StudyWeek> {
    let days: Vec<StudyHours> = text.split(',').map(StudyHours::parse).collect::<Option<_>>()?;
    days.try_into().ok()
}

/// What the island has to plan around: a task or a deadline with its day and an estimate of the work left.
#[derive(Clone, Debug, PartialEq)]
pub struct Load {
    pub label: String,
    pub course: String,
    pub due: i64,
    pub est_min: u32,
}

/// The step the study is planned in, in minutes.
const STEP: u32 = 5;

/// What to study on which day.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StudyPlan {
    /// For each day from today on: (position of the load, minutes).
    pub days: Vec<Vec<(usize, u32)>>,
    /// For each load, the minutes that do not fit in the free time left before it is due: they are not planned.
    pub short: Vec<u32>,
    /// For each day, the free minutes the loads leave (what `share_out` can still suggest something for).
    pub free: Vec<u32>,
}

impl StudyPlan {
    /// The minutes planned on the day this many days from today.
    pub fn minutes_on(&self, offset: usize) -> u32 {
        self.days.get(offset).map_or(0, |d| d.iter().map(|(_, m)| m).sum())
    }

    /// The minutes given to a load on a day.
    pub fn share(&self, load: usize, offset: usize) -> u32 {
        self.days.get(offset).and_then(|d| d.iter().find(|(l, _)| *l == load)).map_or(0, |(_, m)| *m)
    }
}

/// `units` over days that have `free` units each, in proportion to what each has free and never more than that. Whole
/// units, the leftover ones to the days that were rounded down the most.
fn allocate(units: u32, free: &[u32]) -> Vec<u32> {
    let total: u64 = free.iter().map(|f| *f as u64).sum();
    if total == 0 {
        return vec![0; free.len()];
    }
    let units = (units as u64).min(total) as u32;
    let exact: Vec<u64> = free.iter().map(|f| units as u64 * *f as u64).collect();
    let mut out: Vec<u32> = exact.iter().map(|e| (e / total) as u32).collect();
    let mut order: Vec<usize> = (0..free.len()).collect();
    order.sort_by_key(|i| (std::cmp::Reverse(exact[*i] % total), *i));
    let left = (units - out.iter().sum::<u32>()) as usize;
    for i in order.into_iter().take(left) {
        out[i] += 1;
    }
    out
}

/// Spreads the work left of every load over the days before it is due (today when it is due today), the nearest first:
/// each day gets in proportion to the time it still has free, so a day full of lectures gets little and an empty one
/// much, and what one load takes is no longer free for the next. A day is never given more than it has: what does not
/// fit is left out and told (`short`).
///
/// `free[i]` is the time for study on the day `first_day + i`, `today_room` what of today can still be used (it may be
/// late in the day); `done[l]` is what was already studied today for the load `l`. Today's part is worked out as it was
/// this morning and what is done comes off it, so that having studied does not ask for more on the same day.
pub fn spread(loads: &[Load], done: &[u32], first_day: i64, free: &[u32], today_room: u32) -> StudyPlan {
    let mut plan = StudyPlan { days: vec![vec![]; free.len()], short: vec![0; loads.len()], free: vec![] };
    let mut free: Vec<u32> = free.iter().map(|m| m / STEP).collect();
    let mut room = today_room / STEP * STEP;
    let mut order: Vec<usize> = (0..loads.len()).filter(|i| loads[*i].est_min > 0 && loads[*i].due >= first_day).collect();
    order.sort_by_key(|i| (loads[*i].due, *i));
    for l in order {
        let (est, done) = (loads[l].est_min, done.get(l).copied().unwrap_or(0));
        let window = ((loads[l].due - first_day).max(1) as usize).min(free.len());
        if window == 0 {
            continue;
        }
        let share = allocate((est + done).div_ceil(STEP), &free[..window])[0];
        let later: u32 = free[1..window].iter().sum::<u32>() * STEP;
        // due today there is no later; and what the later days cannot hold is for today, if today can
        let wanted = if window == 1 { est } else { (share * STEP).saturating_sub(done).max(est.saturating_sub(later)) };
        let today = wanted.min(est).min(room);
        room -= today / STEP * STEP;
        free[0] = free[0].saturating_sub(share.max(if today > 0 { (today + done).div_ceil(STEP) } else { 0 }));
        if today > 0 {
            plan.days[0].push((l, today));
        }
        let mut rest = est - today;
        if window > 1 {
            for (i, units) in allocate(rest.div_ceil(STEP), &free[1..window]).into_iter().enumerate() {
                let day = i + 1;
                free[day] -= units;
                let min = (units * STEP).min(rest);
                rest -= min;
                if min > 0 {
                    plan.days[day].push((l, min));
                }
            }
        }
        plan.short[l] = rest;
    }
    plan.free = free.into_iter().map(|units| units * STEP).collect();
    plan
}

/// The pace is measured over the days since the first study for something, never fewer than the first of these (one long
/// afternoon is not a habit) nor more than the second (what was done a month ago says little about now).
pub const PACE_DAYS: (i64, i64) = (7, 14);

/// How the study of the last days compares with the work still to do before the day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pace {
    /// Minutes studied for it a study day, and those each study day left would need.
    pub now_per_day: u32,
    pub needed_per_day: u32,
    /// Where the same pace ends up: the percent of the work done by the day, and the minutes missing (0: on track).
    pub percent: u32,
    pub short_min: u32,
}

/// The pace for `est` minutes of work of which `done` are done: `studied` minutes in the last `measured` study days, and
/// `left` study days still to come before the day (none: what is not done by now will not be).
pub fn pace(est: u32, done: u32, studied: u32, measured: u32, left: u32) -> Pace {
    let measured = measured.max(1);
    let todo = est.saturating_sub(done);
    let more = (u64::from(studied) * u64::from(left) / u64::from(measured)).min(u64::from(todo)) as u32;
    let reached = done.min(est) + more;
    Pace {
        now_per_day: (studied + measured / 2) / measured,
        needed_per_day: if left == 0 { todo } else { todo.div_ceil(left).div_ceil(STEP) * STEP },
        percent: (u64::from(reached) * 100 / u64::from(est.max(1))) as u32,
        short_min: (est - reached).div_ceil(STEP) * STEP,
    }
}

/// An exam whose hours were never said: only how big it is (its credits) and when it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weighted {
    pub weight: u32,
    pub due: i64,
}

/// The step of a suggestion: it is only an indication, a quarter of an hour is as fine as it gets.
const SHARE_STEP: u32 = 15;

/// `units` in proportion to `weights`, whole, the leftover ones to the shares rounded down the most.
fn proportional(units: u32, weights: &[f64]) -> Vec<u32> {
    let total: f64 = weights.iter().sum();
    if total <= 0.0 {
        return vec![0; weights.len()];
    }
    let exact: Vec<f64> = weights.iter().map(|w| units as f64 * w / total).collect();
    let mut out: Vec<u32> = exact.iter().map(|e| e.floor() as u32).collect();
    let mut order: Vec<usize> = (0..weights.len()).collect();
    order.sort_by(|a, b| (exact[*b].fract()).total_cmp(&exact[*a].fract()).then(a.cmp(b)));
    let left = units.saturating_sub(out.iter().sum()) as usize;
    for i in order.into_iter().take(left) {
        out[i] += 1;
    }
    out
}

/// Suggests how to divide the free time of each day among the exams still to come: each in proportion to its weight over
/// the days it has left, so that a bigger exam gets more and so does a nearer one. `free[i]` is the free time of the day
/// `first_day + i`, and no day is given more; `done[e]` is what was studied today for the exam `e`, which comes off its
/// part of today, where `today_room` is the most that can still be done.
/// The result is, for each day, (position of the exam, minutes).
pub fn share_out(exams: &[Weighted], done: &[u32], first_day: i64, free: &[u32], today_room: u32) -> Vec<Vec<(usize, u32)>> {
    free.iter()
        .enumerate()
        .map(|(day, free)| {
            let weights: Vec<f64> = exams
                .iter()
                .map(|e| {
                    let left = e.due - first_day - day as i64;
                    // the day of the exam is not a day to study for it, unless it is already today
                    if left > 0 || (day == 0 && left == 0) { e.weight as f64 / left.max(1) as f64 } else { 0.0 }
                })
                .collect();
            let mut mins: Vec<u32> = proportional(free / SHARE_STEP, &weights).into_iter().map(|u| u * SHARE_STEP).collect();
            if day == 0 {
                for (m, d) in mins.iter_mut().zip(done.iter().chain(std::iter::repeat(&0))) {
                    *m = m.saturating_sub(*d);
                }
                // more than a part was done for one exam: the others share what is left of the day
                let room = free.saturating_sub(done.iter().sum()).min(today_room);
                if mins.iter().sum::<u32>() > room {
                    mins = proportional(room / SHARE_STEP, &mins.iter().map(|m| *m as f64).collect::<Vec<_>>()).into_iter().map(|u| u * SHARE_STEP).collect();
                }
            }
            mins.into_iter().enumerate().filter(|(_, m)| *m > 0).collect()
        })
        .collect()
}

/// The time one calendar takes of a stretch of time.
#[derive(Clone, Debug, PartialEq)]
pub struct Busy {
    pub calendar: String,
    pub color: Option<(u8, u8, u8)>,
    pub min: u32,
}

/// What the timed events take of [from, to), calendar by calendar: a moment two events share is counted once, for the
/// one that began first (the longer one when they began together). The calendars come in the order they first take time.
pub fn busy(events: &[Event], from_ms: i64, to_ms: i64) -> Vec<Busy> {
    let mut within: Vec<&Event> = events.iter().filter(|e| !e.all_day && e.start_ms < to_ms && e.end_ms > from_ms).collect();
    within.sort_by(|a, b| a.start_ms.cmp(&b.start_ms).then(b.end_ms.cmp(&a.end_ms)).then(a.calendar.cmp(&b.calendar)));
    let mut covered = from_ms;
    let mut taken: Vec<(&Event, i64)> = vec![];
    for e in within {
        let new = e.end_ms.min(to_ms) - e.start_ms.max(covered);
        if new <= 0 {
            continue;
        }
        covered = e.end_ms.min(to_ms);
        match taken.iter_mut().find(|(t, _)| t.calendar == e.calendar) {
            Some((_, ms)) => *ms += new,
            None => taken.push((e, new)),
        }
    }
    taken.into_iter().map(|(e, ms)| Busy { calendar: e.calendar.clone(), color: e.color, min: (ms / 60_000) as u32 }).collect()
}

/// What the timed events take of several stretches of time, calendar by calendar.
pub fn busy_in(events: &[Event], windows: &[(i64, i64)]) -> Vec<Busy> {
    let mut all: Vec<Busy> = vec![];
    for (from, to) in windows {
        for part in busy(events, *from, *to) {
            match all.iter_mut().find(|b| b.calendar == part.calendar) {
                Some(b) => b.min += part.min,
                None => all.push(part),
            }
        }
    }
    all
}

/// The minutes of [from, to) that timed events take, overlaps counted once.
pub fn busy_minutes(events: &[Event], from_ms: i64, to_ms: i64) -> u32 {
    busy(events, from_ms, to_ms).iter().map(|b| b.min).sum()
}

/// The minutes of timed events on each of `days` days of 24 hours from `start_ms` on.
pub fn event_minutes(events: &[Event], start_ms: i64, days: usize) -> Vec<u32> {
    const DAY: i64 = 86_400_000;
    (0..days as i64).map(|i| busy_minutes(events, start_ms + i * DAY, start_ms + (i + 1) * DAY)).collect()
}

/// The week from `first_day` on: `windows` are the stretches of each day that count, in ms, `loads` what is due, `study`
/// the minutes of study planned for each day.
pub fn week(events: &[Event], windows: &[Vec<(i64, i64)>], first_day: i64, loads: &[Load], study: &[u32]) -> Vec<DayLoad> {
    windows
        .iter()
        .enumerate()
        .map(|(i, spans)| {
            let day = first_day + i as i64;
            // an event that is in both stretches of a day is one event
            let timed = events.iter().filter(|ev| !ev.all_day && spans.iter().any(|(s, e)| ev.start_ms < *e && ev.end_ms > *s)).count();
            let calendars = busy_in(events, spans);
            let event_min = calendars.iter().map(|b| b.min).sum();
            DayLoad {
                day,
                event_min,
                calendars,
                events: timed as u32,
                due: loads.iter().filter(|l| l.due == day).count() as u32,
                study_min: study.get(i).copied().unwrap_or(0),
                suggested_min: 0,
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

    /// `spread` early in the day: today has all its free time, less what is done.
    fn morning(loads: &[Load], done: &[u32], first_day: i64, free: &[u32]) -> StudyPlan {
        let room = free.first().copied().unwrap_or(0).saturating_sub(done.iter().sum());
        spread(loads, done, first_day, free, room)
    }

    fn load(due: i64, est_min: u32) -> Load {
        Load { label: "x".into(), course: String::new(), due, est_min }
    }

    #[test]
    fn study_is_spread_over_the_days_left() {
        let p = morning(&[load(10, 600)], &[], 0, &[480; 10]);
        assert!((0..10).all(|d| p.minutes_on(d) == 60), "ten hours over ten equal days: {p:?}");
        assert_eq!(p.short, [0]);
        assert_eq!(morning(&[load(0, 300)], &[], 0, &[480]).minutes_on(0), 300, "due today: all of it");
        assert_eq!(morning(&[load(3, 100)], &[], 0, &[480; 3]).days.iter().map(|d| d.iter().map(|x| x.1).sum()).collect::<Vec<u32>>(), [35, 35, 30], "in steps of five, no more than the work");
        assert_eq!(morning(&[load(-1, 60), load(5, 0)], &[], 0, &[480; 5]), StudyPlan { days: vec![vec![]; 5], short: vec![0, 0], free: vec![480; 5] }, "passed, or no work");
    }

    #[test]
    fn a_day_full_of_lectures_gets_little_and_a_free_one_much() {
        // free time: a day of lectures, an empty one, a half day
        let p = morning(&[load(3, 360)], &[], 0, &[60, 480, 180]);
        assert_eq!([p.minutes_on(0), p.minutes_on(1), p.minutes_on(2)], [30, 240, 90]);
        assert_eq!(morning(&[load(2, 120)], &[], 0, &[0, 480]).minutes_on(0), 0, "no time today: none today");
    }

    #[test]
    fn the_same_work_is_never_planned_twice() {
        // two things due tomorrow and an exam in twelve days
        let loads = [load(1, 180), load(1, 120), load(12, 900)];
        let p = morning(&loads, &[], 0, &[480; 12]);
        for (l, w) in loads.iter().enumerate() {
            assert_eq!((0..12).map(|d| p.share(l, d)).sum::<u32>(), w.est_min, "load {l}");
        }
        assert_eq!(p.minutes_on(1), p.minutes_on(2), "nothing due tomorrow is done again on the day itself");
        assert!(p.share(2, 0) < p.share(2, 5), "the exam takes less of the day the two others fill");
    }

    #[test]
    fn what_one_load_takes_is_not_free_for_the_next() {
        let p = morning(&[load(1, 480), load(2, 240)], &[], 0, &[480, 480]);
        assert_eq!((p.share(0, 0), p.share(1, 0), p.share(1, 1)), (480, 0, 240), "the second one gets today's none, and all of it falls the day after it can");
    }

    #[test]
    fn a_day_is_never_given_more_than_it_has() {
        let p = morning(&[load(2, 600)], &[], 0, &[120, 240]);
        assert_eq!(([p.minutes_on(0), p.minutes_on(1)], p.short[0]), ([120, 240], 240), "four hours do not fit, and are told");
        assert_eq!(morning(&[load(0, 600)], &[], 0, &[480]).short, [120], "nor on the day itself");
        // many things at once: no day ever beyond its free time, and every minute either planned or told
        let loads: Vec<Load> = (0..9).map(|i| load(i % 5, 95 + 130 * i as u32)).collect();
        let free = [200, 0, 480, 55, 300];
        let p = morning(&loads, &[], 0, &free);
        for (d, f) in free.iter().enumerate() {
            assert!(p.minutes_on(d) <= *f, "day {d}: {} of {f}", p.minutes_on(d));
        }
        for (l, w) in loads.iter().enumerate() {
            assert_eq!((0..5).map(|d| p.share(l, d)).sum::<u32>() + p.short[l], w.est_min, "load {l}");
        }
    }

    #[test]
    fn late_in_the_day_only_what_can_still_be_done_is_asked() {
        // ten hours in two days, one hour left tonight
        let p = spread(&[load(2, 600)], &[], 0, &[480, 480], 60);
        assert_eq!(([p.minutes_on(0), p.minutes_on(1)], p.short[0]), ([60, 480], 60));
        let p = spread(&[load(10, 600)], &[], 0, &[480; 10], 0);
        assert_eq!((p.minutes_on(0), p.minutes_on(1), p.short[0]), (0, 70, 0), "nothing tonight: the other days take it");
        assert_eq!(spread(&[load(3, 900)], &[], 0, &[480, 240, 240], 480).minutes_on(0), 450, "a tight but possible one: today takes its part");
    }

    #[test]
    fn what_was_studied_today_comes_off_todays_part() {
        // ten hours in ten days, an hour and a half of it done this morning: 8h30 left
        assert_eq!(morning(&[load(10, 510)], &[90], 0, &[480; 10]).minutes_on(0), 0, "today's hour is done");
        assert_eq!(morning(&[load(10, 510)], &[90], 0, &[480; 10]).minutes_on(1), 60, "the nine other days share the rest");
        let half = morning(&[load(10, 570)], &[30], 0, &[480; 10]);
        assert_eq!((half.minutes_on(0), half.minutes_on(1)), (30, 60), "half an hour left today");
    }

    #[test]
    fn study_hours_move_by_half_hours_and_stay_a_stretch_of_the_day() {
        let mut h = StudyHours::default();
        h.step(Edge::Start, true);
        h.step(Edge::End, false);
        assert_eq!((h.from, h.to, h.text().as_str()), (570, 990, "09:30-16:30"));
        for _ in 0..60 {
            h.step(Edge::Start, true);
        }
        assert_eq!((h.from, h.to), (960, 990), "the start stops half an hour before the end");
        for _ in 0..60 {
            h.step(Edge::End, true);
        }
        assert_eq!(h.to, 24 * 60);
        for _ in 0..60 {
            h.step(Edge::Start, false);
            h.step(Edge::End, false);
        }
        assert_eq!((h.from, h.to), (0, 30));
        assert_eq!(StudyHours::default().windows(1000), [(1000 + 9 * H, 1000 + 17 * H)]);
        let off = StudyHours { on: false, ..Default::default() };
        assert!(off.windows(0).is_empty() && off.minutes() == 480, "a day off has no time, and remembers its hours");
    }

    #[test]
    fn a_lunch_pause_cuts_the_day_in_two() {
        let mut h = StudyHours { to: 18 * 60, ..Default::default() };
        h.toggle_pause();
        assert_eq!((h.pause, h.minutes(), h.text().as_str()), (Some((780, 840)), 480, "09:00-13:00+14:00-18:00"));
        assert_eq!(h.windows(0), [(9 * H, 13 * H), (14 * H, 18 * H)]);
        // every time stays in its order, half an hour at least between two of them
        for _ in 0..60 {
            h.step(Edge::PauseStart, true);
            h.step(Edge::Start, true);
        }
        assert_eq!((h.from, h.pause), (780, Some((810, 840))));
        for _ in 0..60 {
            h.step(Edge::PauseEnd, true);
            h.step(Edge::End, false);
        }
        assert_eq!((h.pause, h.to), (Some((810, 960)), 990), "moved towards each other, they stop half an hour apart");
        for _ in 0..60 {
            h.step(Edge::End, false);
            h.step(Edge::PauseEnd, false);
        }
        assert_eq!((h.pause, h.to), (Some((810, 840)), 870), "the end stops after the pause, the pause stays half an hour");
        h.toggle_pause();
        assert_eq!(h.pause, None);
        // a day that does not go past lunch gets its pause in the middle; a very short one gets none
        let mut evening = StudyHours { from: 15 * 60, to: 21 * 60, ..Default::default() };
        evening.toggle_pause();
        assert_eq!(evening.pause, Some((1050, 1110)));
        let mut short = StudyHours { from: 600, to: 690, ..Default::default() };
        short.toggle_pause();
        assert_eq!(short.pause, None);
        let mut untouched = StudyHours::default();
        untouched.step(Edge::PauseStart, true);
        assert_eq!(untouched, StudyHours::default(), "no pause to move");
    }

    #[test]
    fn the_week_of_study_hours_is_written_and_read_back() {
        let mut week = StudyWeek::default();
        week[0] = StudyHours { on: true, from: 540, to: 1080, pause: Some((780, 870)) };
        week[5] = StudyHours { on: false, from: 600, to: 780, pause: None };
        week[6] = StudyHours { on: true, from: 0, to: 24 * 60, pause: None };
        let text = week_text(&week);
        assert!(text.starts_with("09:00-13:00+14:30-18:00,09:00-17:00,") && text.ends_with(",off 10:00-13:00,00:00-24:00"), "{text}");
        assert_eq!(parse_week(&text), Some(week));
        for bad in ["", "09:00-17:00", "17:00-09:00,", "09:00-25:00", "9-17"] {
            assert_eq!(parse_week(bad), None, "{bad}");
        }
        for bad in ["24:30-25:00", "10:00-10:00", "09:00-13:00+12:00-18:00", "09:00-13:00+13:00-18:00", "09:00-10:00+11:00-12:00+13:00-14:00"] {
            assert_eq!(StudyHours::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn events_in_a_pause_count_for_nothing() {
        let day = StudyHours { on: true, from: 540, to: 1080, pause: Some((780, 840)) };
        let evs = vec![ev("lunch", 13, 14), ev("long", 12, 15), ev("early", 8, 10)];
        let w = week(&evs, &[day.windows(0)], 0, &[], &[]);
        assert_eq!((w[0].events, w[0].event_min), (2, 180), "9-10, 12-13 and 14-15; the lunch is not one of them");
    }

    #[test]
    fn the_pace_says_where_the_last_days_lead() {
        // 20h, 5h done, an hour a study day lately, 10 study days left: 15h of 20h
        let p = pace(1200, 300, 7 * 60, 7, 10);
        assert_eq!(p, Pace { now_per_day: 60, needed_per_day: 90, percent: 75, short_min: 300 });
        // fast enough: all of it, and never more than all of it
        let p = pace(600, 300, 14 * 60, 7, 10);
        assert_eq!((p.percent, p.short_min, p.needed_per_day), (100, 0, 30));
        // nothing lately: it stays where it is
        let p = pace(600, 150, 0, 7, 4);
        assert_eq!((p.now_per_day, p.percent, p.short_min), (0, 25, 450));
        // no study day left, and no days measured: no division by zero
        let p = pace(600, 480, 60, 0, 0);
        assert_eq!((p.percent, p.short_min, p.needed_per_day), (80, 120, 120));
    }

    #[test]
    fn free_time_is_suggested_by_weight_and_nearness() {
        let ex = |weight, due| Weighted { weight, due };
        let day = |p: &[Vec<(usize, u32)>], d: usize| [0, 1].map(|e| p[d].iter().find(|(i, _)| *i == e).map_or(0, |x| x.1));
        // the same day: the bigger exam gets more, in proportion
        assert_eq!(day(&share_out(&[ex(12, 10), ex(6, 10)], &[], 0, &[360; 10], 360), 0), [240, 120]);
        // the same size: the nearer one gets more, and all of it once the other has passed
        let p = share_out(&[ex(6, 2), ex(6, 10)], &[], 0, &[360; 12], 360);
        assert_eq!((day(&p, 0), day(&p, 1)), ([300, 60], [330, 30]));
        assert_eq!(day(&p, 2), [0, 360], "no study on the day of the first, nor after");
        assert!(p[10].is_empty() && p[11].is_empty(), "nothing left to study for");
        assert!(share_out(&[ex(6, 5)], &[], 0, &[0, 20], 0).iter().map(|d| d.iter().map(|x| x.1).sum::<u32>()).eq([0, 15]), "in quarters of an hour");
    }

    #[test]
    fn what_was_done_today_comes_off_the_suggestion() {
        let exams = [Weighted { weight: 6, due: 10 }, Weighted { weight: 6, due: 10 }];
        let today = |done: &[u32]| share_out(&exams, done, 0, &[360], 360)[0].clone();
        assert_eq!(share_out(&exams, &[], 0, &[360], 40)[0], [(0, 15), (1, 15)], "late in the day: only what can still be done");
        assert_eq!(today(&[60, 0]), [(0, 120), (1, 180)]);
        assert_eq!(today(&[300, 0]), [(1, 60)], "only an hour of the day is left");
        assert!(today(&[360, 60]).is_empty());
    }

    #[test]
    fn overlapping_events_are_counted_once_and_each_calendar_has_its_part() {
        let on = |title: &str, calendar: &str, from: i64, to: i64| Event { calendar: calendar.into(), ..ev(title, from, to) };
        let evs = vec![on("Reti", "Uni", 9, 11), on("Reti bis", "Uni", 9, 11), on("Dentist", "Personal", 10, 13), on("Lab", "Uni", 14, 16), on("inside", "Personal", 14, 15)];
        assert_eq!(busy_minutes(&evs, 0, 24 * H), 360, "9-13 and 14-16");
        let parts: Vec<(String, u32)> = busy(&evs, 0, 24 * H).into_iter().map(|b| (b.calendar, b.min)).collect();
        assert_eq!(parts, [("Uni".to_string(), 240), ("Personal".to_string(), 120)], "the shared hour is the lecture's, which began first");
        assert_eq!(busy_minutes(&evs, 10 * H, 12 * H), 120, "cut to the stretch asked for");
        let w = week(&evs, &[vec![(0, 24 * H)]], 0, &[], &[]);
        assert_eq!((w[0].events, w[0].event_min, w[0].calendars.len()), (5, 360, 2));
    }

    #[test]
    fn the_time_of_events_is_counted_day_by_day() {
        let mut all_day = ev("Holiday", 0, 24);
        all_day.all_day = true;
        let evs = vec![ev("A", 9, 11), ev("night", 23, 26), ev("old", -5, -1), all_day];
        assert_eq!(event_minutes(&evs, 0, 3), [180, 120, 0]);
    }

    #[test]
    fn a_week_counts_hours_and_what_is_due_and_flags_a_heavy_day() {
        let day0 = 100;
        let ranges: Vec<Vec<(i64, i64)>> = (0..7).map(|i| vec![(i * 24 * H, (i + 1) * 24 * H)]).collect();
        let evs = vec![ev("A", 9, 11), ev("B", 12, 15), ev("C", 24 + 9, 24 + 10)];
        let loads = vec![Load { label: "x".into(), course: String::new(), due: day0 + 1, est_min: 0 }, Load { label: "y".into(), course: String::new(), due: day0 + 1, est_min: 0 }, Load { label: "z".into(), course: String::new(), due: day0 + 1, est_min: 0 }];
        let w = week(&evs, &ranges, day0, &loads, &[]);
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
