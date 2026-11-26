//! The organizer: tasks (a day, a course, an estimate, a repetition), events made from the bar, study time, the week ahead
//! and the search. Everything is decided here from what the island holds; the files and servers are the platform layer's.

use super::*;
use crate::model::OptionValue;
use island_core::plan::{self, DayLoad, Hit, HitKind, Load, StudyHours, StudyPlan, Weighted};
use island_core::task::{self, Repeat, Spec, Task};

/// Where a load of work comes from, to take studied time off it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Task(usize),
    Deadline(usize),
}

/// The days kept in the study log.
const LOG_DAYS: i64 = 120;
/// How far ahead the calendars are read: past it a day is taken to be like the same weekday of the last week known.
const CALENDAR_DAYS: usize = 60;
/// The most rows of today's study and of the deadlines the Plan page shows.
pub(crate) const PLAN_ROWS: usize = 5;
/// The most exams the split of the next seven days names.
pub(crate) const SPLIT_ROWS: usize = 4;

/// The study ahead, worked out from what the island holds.
pub(crate) struct Outlook {
    pub sources: Vec<(Source, Load)>,
    pub plan: StudyPlan,
    /// The exams with credits and no hours (their positions among the deadlines), and what is suggested for them each day.
    pub weighted: Vec<usize>,
    pub suggested: Vec<Vec<(usize, u32)>>,
    /// Minutes of today that can still be used for study.
    pub room_today: u32,
}

/// One thing to study on a day.
pub(crate) struct StudyRow {
    pub what: String,
    pub min: u32,
    pub due: i64,
    /// Only an indication, from the credits: no hours were said for it.
    pub suggested: bool,
}

/// What the Plan page shows.
pub(crate) struct PlanView {
    /// What to study today, the nearest first.
    pub today: Vec<StudyRow>,
    /// How the study of the next seven days divides among the exams: (name, minutes), most first.
    pub split: Vec<(String, u32)>,
    /// Today is a day off.
    pub day_off: bool,
    /// Minutes studied today, and those that can still be.
    pub studied_today: u32,
    pub room_today: u32,
    pub week: Vec<DayLoad>,
    /// The deadlines still to come, soonest first.
    pub deadlines: Vec<DeadlineRow>,
    /// Minutes studied this week, per course.
    pub studied: Vec<(String, u32)>,
}

pub(crate) struct DeadlineRow {
    /// Its position in the island's list (what the actions take).
    pub index: usize,
    pub name: String,
    pub day: i64,
    /// Minutes of work it takes in all (0: none was said), of study done for it, and still to do.
    pub est_min: u32,
    pub done_min: u32,
    pub left_min: u32,
    /// Minutes of it that do not fit in the free time before the day.
    pub short_min: u32,
    pub cfu: u32,
}

impl Island {
    pub(crate) fn today_day(&self) -> i64 {
        self.day_number(self.now)
    }

    /// The moment a local day begins.
    pub(crate) fn day_start_ms(&self, day: i64) -> i64 {
        (day * 86_400 - self.data.utc_offset_secs as i64) * 1000
    }

    fn new_uid(&self) -> String {
        format!("oi-{:x}-{:06x}", self.now, super::inputs::os_random() & 0xff_ffff)
    }

    /// The to-dos to show today, soonest first (the late ones on top), with their place in the list: a repeating one waits for its day.
    pub(crate) fn open_todos(&self) -> Vec<(usize, &Todo)> {
        let today = self.today_day();
        let mut v: Vec<(usize, &Todo)> = self.todos.iter().enumerate().filter(|(_, t)| t.visible(today)).collect();
        v.sort_by_key(|(_, t)| t.sort_key());
        v
    }

    /// A task from the bar or a script.
    pub(super) fn add_task(&mut self, mut spec: Spec) {
        spec.text = spec.text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(120).collect();
        if spec.text.is_empty() || self.todos.iter().any(|t| t.text == spec.text) {
            return;
        }
        let task = Task::from_spec(&spec, self.new_uid());
        let detail = spec.summary(Some(self.today_day()));
        self.todos.push(task.clone());
        self.todos.truncate(30);
        self.pending.push(Command::PushTask { task, done: false });
        self.say(if spec.repeat == Repeat::Never { "Added to your list" } else { "Added, it comes back" }, &if detail.is_empty() { spec.text } else { format!("{} · {detail}", spec.text) });
    }

    /// A line of text from a script (`open-island todo ...`): read like the bar reads it.
    pub(super) fn add_task_text(&mut self, text: &str) {
        let (words, today) = (text.split_whitespace().collect::<Vec<_>>(), self.today_ymd());
        self.add_task(task::parse_spec(&words, Some(today)));
    }

    pub(crate) fn today_ymd(&self) -> island_core::quick::Ymd {
        island_core::calendar::civil_from_days(self.today_day())
    }

    /// Ticks the task at this position: gone for good, or moved to its next day when it repeats.
    pub(super) fn complete_task(&mut self, n: usize) {
        let today = self.today_day();
        let Some(mut t) = self.todos.get(n).cloned() else { return };
        let finished = t.complete(today);
        if finished {
            self.todos.remove(n);
            self.tasks_done.push(t.uid.clone());
            let extra = self.tasks_done.len().saturating_sub(20);
            self.tasks_done.drain(..extra);
        } else {
            self.todos[n] = t.clone();
        }
        self.pending.push(Command::PushTask { task: t, done: finished });
    }

    /// Brings the list in line with what a server says: new tasks come in, changed ones are updated, the finished ones go.
    pub(super) fn merge_tasks(&mut self, remote: Vec<(Task, bool)>) {
        let known: std::collections::HashSet<String> = remote.iter().map(|(t, _)| t.uid.clone()).collect();
        for (r, done) in remote {
            match self.todos.iter().position(|t| t.uid == r.uid) {
                Some(i) if done => {
                    self.todos.remove(i);
                }
                Some(i) => self.todos[i] = Task { synced: true, ..r },
                None if !done && !self.tasks_done.contains(&r.uid) && self.todos.len() < 30 => self.todos.push(Task { synced: true, ..r }),
                None => {}
            }
        }
        // one the server had and no longer has was finished or deleted elsewhere
        self.todos.retain(|t| !t.synced || known.contains(&t.uid));
        // what the server has never seen (made offline, before sync was on, or when a send failed) is sent now
        let unsent: Vec<Task> = self.todos.iter().filter(|t| !t.synced && !known.contains(&t.uid)).cloned().collect();
        self.pending.extend(unsent.into_iter().map(|task| Command::PushTask { task, done: false }));
    }

    /// A reminder that comes back: the first time is the first day it falls on (today when the time is still ahead).
    pub(super) fn add_repeating_reminder(&mut self, repeat: Repeat, at_secs: i64, text: String) {
        let today = self.today_day();
        let at_ms = |d: i64| self.day_start_ms(d) + at_secs * 1000;
        let first = repeat.on_or_after(today).filter(|d| at_ms(*d) > self.now).or_else(|| repeat.next_after(today));
        let Some(day) = first else { return };
        self.reminders.push(Reminder { due_ms: at_ms(day), text: text.clone(), repeat });
        self.reminders.sort_by_key(|r| r.due_ms);
        self.reminders.truncate(50);
        self.say("Reminder set", &format!("{} at {:02}:{:02} · {text}", repeat.label(), at_secs / 3600, at_secs % 3600 / 60));
    }

    /// When a repeating reminder has rung: the next time it is due, at the same time of day.
    pub(super) fn next_reminder(&self, r: &Reminder) -> Option<Reminder> {
        let off = self.data.utc_offset_secs as i64;
        let (day, at) = ((r.due_ms / 1000 + off).div_euclid(86_400), (r.due_ms / 1000 + off).rem_euclid(86_400));
        let mut next = r.repeat.next_after(day)?;
        while self.day_start_ms(next) + at * 1000 <= self.now {
            next = r.repeat.next_after(next)?;
        }
        Some(Reminder { due_ms: self.day_start_ms(next) + at * 1000, ..r.clone() })
    }

    /// An event from the bar: shown at once, sent to the calendar, and a word when it overlaps another.
    pub(super) fn add_event(&mut self, day: i64, start_min: u32, dur_min: u32, title: String) {
        let whole = start_min == 0 && dur_min == 1440;
        let start = self.day_start_ms(day) + start_min as i64 * 60_000;
        let end = start + dur_min as i64 * 60_000;
        // an entry without hours blocks nothing
        let clash: Vec<String> = if whole { vec![] } else { plan::conflicts(&self.data.events, start, end).iter().map(|e| e.title.clone()).collect() };
        self.data.events.push(island_core::calendar::Event { id: format!("new-{start}-{}", title.len()), title: title.clone(), start_ms: start, end_ms: end, all_day: whole, calendar: "Island".into(), ..Default::default() });
        self.data.events.sort_by_key(|e| e.start_ms);
        self.pending.push(Command::CreateEvent { title: title.clone(), start_ms: start, end_ms: end, whole_day: whole.then_some(day) });
        let when = island_core::quick::fmt_date(island_core::calendar::civil_from_days(day));
        if clash.is_empty() {
            let at = if whole { "all day".to_string() } else { format!("{:02}:{:02}", start_min / 60, start_min % 60) };
            self.say("Event added", &format!("{title} · {when} {at}"));
        } else {
            self.say("Event added, but it overlaps", &format!("{title} · {}", clash.join(", ")));
        }
    }

    // ---- changing what is there

    /// The task whose words match, the nearest first.
    fn find_task(&self, query: &str) -> Option<usize> {
        let mut v: Vec<(usize, &Todo)> = self.todos.iter().enumerate().filter(|(_, t)| plan::matches(&format!("{} {}", t.text, t.course), query)).collect();
        v.sort_by_key(|(_, t)| t.sort_key());
        v.first().map(|(i, _)| *i)
    }

    /// The next event (not over yet) whose title matches.
    fn find_event(&self, query: &str) -> Option<usize> {
        self.data.events.iter().enumerate().filter(|(_, e)| e.end_ms > self.now && plan::matches(&e.title, query)).min_by_key(|(_, e)| e.start_ms).map(|(i, _)| i)
    }

    pub(super) fn edit_task(&mut self, query: &str, spec: Spec) {
        let Some(i) = self.find_task(query) else { return self.say("No task like that", query) };
        let t = &mut self.todos[i];
        let text: String = spec.text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(120).collect();
        if !text.is_empty() {
            t.text = text;
        }
        t.due = spec.due.or(t.due);
        if !spec.course.is_empty() {
            t.course = spec.course;
        }
        if spec.est_min > 0 {
            t.est_min = spec.est_min;
        }
        if spec.repeat != Repeat::Never {
            t.repeat = spec.repeat;
        }
        let t = t.clone();
        self.say("Task changed", &t.text);
        self.pending.push(Command::PushTask { task: t, done: false });
    }

    pub(super) fn drop_task(&mut self, query: &str) {
        let Some(i) = self.find_task(query) else { return self.say("No task like that", query) };
        let t = self.todos.remove(i);
        // a late answer from the server must not bring it back
        self.tasks_done.push(t.uid.clone());
        let extra = self.tasks_done.len().saturating_sub(20);
        self.tasks_done.drain(..extra);
        self.pending.push(Command::DeleteTask { uid: t.uid.clone() });
        self.say("Task removed", &t.text);
    }

    pub(super) fn cancel_event(&mut self, query: &str) {
        let Some(i) = self.find_event(query) else { return self.say("No event like that", query) };
        let e = self.data.events.remove(i);
        self.pending.push(Command::CancelEvent { id: e.id.clone(), title: e.title.clone(), start_ms: e.start_ms });
        self.say("Event cancelled", &e.title);
    }

    /// `move <words> to <day> [hours]`: with hours it is an event, with only a day it is a task when one matches (else an event).
    pub(super) fn move_item(&mut self, query: &str, day: Option<i64>, start_min: Option<u32>, dur_min: Option<u32>) {
        let (task, event) = (self.find_task(query), self.find_event(query));
        let as_event = match (task, event) {
            (_, None) => false,
            (None, Some(_)) => true,
            (Some(_), Some(_)) => start_min.is_some(),
        };
        if !as_event {
            let (Some(i), Some(day)) = (task, day) else { return self.say("Nothing to move", query) };
            self.todos[i].due = Some(day);
            let t = self.todos[i].clone();
            self.say("Task moved", &format!("{} · {}", t.text, task::due_label(day, self.today_day())));
            self.pending.push(Command::PushTask { task: t, done: false });
            return;
        }
        let Some(i) = event else { return self.say("Nothing to move", query) };
        let old = self.data.events[i].clone();
        let was = crate::clock::civil(old.start_ms, self.data.utc_offset_secs);
        let old_day = self.day_number(old.start_ms);
        let day = day.unwrap_or(old_day);
        let (start_min, dur) = (start_min.unwrap_or(was.hour * 60 + was.minute), dur_min.unwrap_or(((old.end_ms - old.start_ms) / 60_000).max(1) as u32));
        let whole = old.all_day.then_some(day);
        let start = if old.all_day { self.day_start_ms(day) } else { self.day_start_ms(day) + start_min as i64 * 60_000 };
        let end = if old.all_day { self.day_start_ms(day + 1) } else { start + dur as i64 * 60_000 };
        let others: Vec<island_core::calendar::Event> = self.data.events.iter().filter(|e| e.id != old.id).cloned().collect();
        let clash: Vec<String> = if old.all_day { vec![] } else { plan::conflicts(&others, start, end).iter().map(|e| e.title.clone()).collect() };
        self.data.events[i].start_ms = start;
        self.data.events[i].end_ms = end;
        self.data.events.sort_by_key(|e| e.start_ms);
        self.pending.push(Command::MoveEvent { id: old.id.clone(), title: old.title.clone(), old_start_ms: old.start_ms, start_ms: start, end_ms: end, whole_day: whole });
        if clash.is_empty() {
            self.say("Event moved", &old.title);
        } else {
            self.say("Event moved, but it overlaps", &format!("{} · {}", old.title, clash.join(", ")));
        }
    }

    /// A reminder `secs` before the next event whose title has these words. It is set for that event's time as it is now:
    /// when the event moves, say it again.
    pub(super) fn remind_before(&mut self, secs: i64, query: &str) {
        let now = self.now;
        let found = self.data.events.iter().filter(|e| !e.all_day && e.start_ms - secs * 1000 > now && plan::matches(&e.title, query)).min_by_key(|e| e.start_ms).cloned();
        let Some(e) = found else { return self.say("No coming event like that", query) };
        let text = format!("{} in {}", e.title, island_core::duration::fmt_took(secs * 1000).replace(' ', ""));
        self.reminders.push(Reminder { due_ms: e.start_ms - secs * 1000, text: text.clone(), ..Default::default() });
        self.reminders.sort_by_key(|r| r.due_ms);
        self.reminders.truncate(50);
        self.say("Reminder set", &format!("{} · {}", crate::clock::civil(e.start_ms - secs * 1000, self.data.utc_offset_secs).hhmm(), text));
    }

    // ---- putting things off

    /// Open to-dos with a day that is today or gone and that do not repeat: what is left of the day.
    pub(crate) fn unfinished(&self, late_only: bool) -> Vec<usize> {
        let today = self.today_day();
        self.todos.iter().enumerate().filter(|(_, t)| t.repeat == Repeat::Never && t.due.is_some_and(|d| if late_only { d < today } else { d <= today })).map(|(i, _)| i).collect()
    }

    /// Gives these to-dos a new day, and tells the server.
    pub(super) fn postpone(&mut self, idxs: &[usize], day: i64) {
        let today = self.today_day();
        let mut names = vec![];
        for &i in idxs {
            if let Some(t) = self.todos.get_mut(i) {
                t.due = Some(day);
                names.push(t.text.clone());
                let t = t.clone();
                self.pending.push(Command::PushTask { task: t, done: false });
            }
        }
        if !names.is_empty() {
            self.say(&format!("Put off to {}", task::due_label(day, today)), &names.join(" · "));
        }
    }

    pub(super) fn remove_task(&mut self, n: usize) {
        if let Some(t) = self.todos.get(n) {
            let text = t.text.clone();
            self.drop_task(&text);
        }
    }

    /// The card with tomorrow (or today), with a line for what is left and what a tap can do about it.
    pub(super) fn with_leftovers(&self, card: Option<(String, String)>, morning: bool) -> (Option<(String, String)>, Vec<NotifAction>) {
        let idxs = self.unfinished(morning);
        if idxs.is_empty() {
            return (card, vec![]);
        }
        let names: Vec<String> = idxs.iter().map(|&i| self.todos[i].text.clone()).collect();
        let line = format!("{}: {}", if morning { "Late" } else { "Left today" }, names.join(", "));
        let card = match card {
            Some((s, b)) => (s, format!("{line}\n{b}")),
            None => (if morning { "Today · late" } else { "Today · unfinished" }.to_string(), line),
        };
        let action = if morning { NotifAction { key: "open-island:late".into(), label: "Move to today".into() } } else { NotifAction { key: "open-island:carry".into(), label: "To tomorrow".into() } };
        (Some(card), vec![action])
    }

    /// The choices a reminder's card offers: in an hour, tonight (when it is not yet evening), tomorrow morning.
    pub(super) fn snooze_actions(&self) -> Vec<NotifAction> {
        let mut v = vec![NotifAction { key: "open-island:snooze:1h".into(), label: "+1 hour".into() }];
        if self.tonight_ms() > self.now + 30 * 60_000 {
            v.push(NotifAction { key: "open-island:snooze:tonight".into(), label: "Tonight".into() });
        }
        v.push(NotifAction { key: "open-island:snooze:tomorrow".into(), label: "Tomorrow".into() });
        v
    }

    fn tonight_ms(&self) -> i64 {
        self.day_start_ms(self.today_day()) + 20 * 3_600_000
    }

    /// What a button of the island's own cards does (the key says it).
    pub(super) fn own_card_action(&mut self, key: &str, summary: &str) {
        let today = self.today_day();
        match key {
            "open-island:carry" => {
                let idxs = self.unfinished(false);
                self.postpone(&idxs, today + 1);
            }
            "open-island:late" => {
                let idxs = self.unfinished(true);
                self.postpone(&idxs, today);
            }
            k if k.starts_with("open-island:snooze:") => {
                let due = match &k["open-island:snooze:".len()..] {
                    "1h" => self.now + 3_600_000,
                    "tonight" => self.tonight_ms(),
                    _ => self.day_start_ms(today + 1) + 9 * 3_600_000,
                };
                self.reminders.push(Reminder { due_ms: due, text: summary.to_string(), ..Default::default() });
                self.reminders.sort_by_key(|r| r.due_ms);
                self.reminders.truncate(50);
                self.say("Reminder put off", &format!("{} · {summary}", crate::clock::civil(due, self.data.utc_offset_secs).hhmm()));
            }
            _ => {}
        }
    }

    // ---- the heavy day ahead

    /// Looks at the card's day and the two after it: when one is heavy, says so, and when a task due on it could be done
    /// on a lighter day before it, says which (`move <task> to <day>` does it).
    pub(crate) fn heavy_lines(&self, from_day: i64) -> Vec<String> {
        let today = self.today_day();
        let week = self.week_loads();
        let load = |d: &DayLoad| d.event_min + d.study_min + d.due * 60;
        let Some(heavy) = week.iter().find(|d| d.day >= from_day && d.day <= from_day + 2 && d.heavy()) else { return vec![] };
        let mut out = vec![format!("{} is heavy: {}", cap(&task::due_label(heavy.day, today)), plan::day_summary(heavy))];
        // the task with the most work among those due that day (a repeating one has no deadline to bring forward)
        let cand = self.todos.iter().filter(|t| t.due == Some(heavy.day) && t.repeat == Repeat::Never).max_by_key(|t| t.est_min);
        let lighter = week.iter().filter(|d| d.day >= today && d.day < heavy.day && load(d) + 60 < load(heavy) && self.study_hours(d.day).on).min_by_key(|d| load(d));
        if let (Some(t), Some(d)) = (cand, lighter) {
            out.push(format!("{} is lighter: move \"{}\" there?", cap(&task::due_label(d.day, today)), t.text));
        }
        out
    }

    // ---- study

    /// Everything with work still to do and a day it is due, and where it comes from. Tasks that repeat are not planned.
    pub(crate) fn load_sources(&self) -> Vec<(Source, Load)> {
        let today = self.today_day();
        let mut v = vec![];
        for (i, t) in self.todos.iter().enumerate() {
            if let (Some(due), true) = (t.due, t.est_min > 0 && t.repeat == Repeat::Never) {
                if due >= today {
                    v.push((Source::Task(i), Load { label: t.text.clone(), course: t.course.clone(), due, est_min: t.est_min }));
                }
            }
        }
        for (i, d) in self.deadlines.iter().enumerate() {
            let left = d.est_min.saturating_sub(d.done_min);
            if left > 0 && d.day >= today {
                v.push((Source::Deadline(i), Load { label: d.text.clone(), course: d.course.clone(), due: d.day, est_min: left }));
            }
        }
        v
    }

    /// The study plan: the work left, spread over the days by the time their events leave free (see `plan::spread`),
    /// and a suggestion for the time that is still free among the exams that only have credits (see `plan::share_out`).
    pub(crate) fn outlook(&self) -> Outlook {
        let today = self.today_day();
        let sources = self.load_sources();
        let loads: Vec<Load> = sources.iter().map(|(_, l)| l.clone()).collect();
        let weighted: Vec<usize> = (0..self.deadlines.len()).filter(|n| Some(&self.deadlines[*n]).is_some_and(|d| d.est_min == 0 && d.cfu > 0 && d.day >= today)).collect();
        let last = loads.iter().map(|l| l.due).chain(weighted.iter().map(|n| self.deadlines[*n].day)).max().unwrap_or(today);
        let horizon = (last - today).clamp(7, 366) as usize;
        let mut free: Vec<u32> = (today..today + horizon.min(CALENDAR_DAYS) as i64).map(|day| self.free_in(&self.study_windows(day))).collect();
        for day in free.len()..horizon {
            free.push(free[day - 7]);
        }
        // what was studied today counts where `log_study` counted it
        let (mut done, mut done_weighted) = (vec![0; loads.len()], vec![0; weighted.len()]);
        for (_, course, min) in self.study_log.iter().filter(|(d, _, _)| *d == today) {
            if let Some(i) = plan::load_for_course(&loads, course) {
                done[i] += min;
            } else if let Some(i) = self.deadline_for(course, today).and_then(|n| weighted.iter().position(|w| *w == n)) {
                done_weighted[i] += min;
            }
        }
        let room_today = self.room_today();
        let planned = plan::spread(&loads, &done, today, &free, room_today);
        let exams: Vec<Weighted> = weighted.iter().map(|n| Weighted { weight: self.deadlines[*n].cfu, due: self.deadlines[*n].day }).collect();
        let suggested = plan::share_out(&exams, &done_weighted, today, &planned.free, room_today.saturating_sub(planned.minutes_on(0)));
        Outlook { plan: planned, sources, weighted, suggested, room_today }
    }

    pub(crate) fn study_hours(&self, day: i64) -> StudyHours {
        self.cfg.study_week[island_core::calendar::weekday_of_days(day) as usize]
    }

    /// The stretches of a day in which one studies (two around a lunch pause).
    fn study_windows(&self, day: i64) -> Vec<(i64, i64)> {
        self.study_hours(day).windows(self.day_start_ms(day))
    }

    /// The minutes of some stretches of time that no event takes.
    fn free_in(&self, windows: &[(i64, i64)]) -> u32 {
        let whole: u32 = windows.iter().map(|(from, to)| ((to - from).max(0) / 60_000) as u32).sum();
        let taken: u32 = plan::busy_in(&self.data.events, windows).iter().map(|b| b.min).sum();
        whole.saturating_sub(taken)
    }

    /// What of today can still be studied: no more than today's study hours leave after their events and what is done,
    /// and no more than the free time still ahead in them.
    fn room_today(&self) -> u32 {
        let today = self.today_day();
        let windows = self.study_windows(today);
        let done: u32 = self.study_log.iter().filter(|(d, _, _)| *d == today).map(|(_, _, m)| m).sum();
        let ahead: Vec<(i64, i64)> = windows.iter().filter(|(_, to)| *to > self.now).map(|(from, to)| (self.now.max(*from), *to)).collect();
        self.free_in(&windows).saturating_sub(done).min(self.free_in(&ahead))
    }

    /// The command that keeps the study hours as they are now.
    pub(super) fn save_study_week(&self) -> Command {
        Command::SetOption { key: "study_hours".into(), value: OptionValue::Text(plan::week_text(&self.cfg.study_week)) }
    }

    /// What to study on a day: (what, minutes), the most pressing first.
    pub(crate) fn study_on(&self, day: i64) -> Vec<(String, u32)> {
        self.study_rows(&self.outlook(), day).into_iter().map(|r| (r.what, r.min)).collect()
    }

    fn study_rows(&self, outlook: &Outlook, day: i64) -> Vec<StudyRow> {
        let Ok(offset) = usize::try_from(day - self.today_day()) else { return vec![] };
        let planned = outlook.plan.days.get(offset).into_iter().flatten().map(|(l, min)| {
            let load = &outlook.sources[*l].1;
            StudyRow { what: load.label.clone(), min: *min, due: load.due, suggested: false }
        });
        let suggested = outlook.suggested.get(offset).into_iter().flatten().map(|(e, min)| {
            let d = &self.deadlines[outlook.weighted[*e]];
            StudyRow { what: d.text.clone(), min: *min, due: d.day, suggested: true }
        });
        let mut rows: Vec<StudyRow> = planned.chain(suggested).collect();
        rows.sort_by_key(|r| r.due);
        rows
    }

    /// The study of the next seven days for each exam, planned or suggested: (name, minutes), most first.
    fn week_split(&self, outlook: &Outlook) -> Vec<(String, u32)> {
        let mut by_exam = vec![0u32; self.deadlines.len()];
        for day in 0..7 {
            for (l, min) in outlook.plan.days.get(day).into_iter().flatten() {
                if let Source::Deadline(n) = outlook.sources[*l].0 {
                    by_exam[n] += min;
                }
            }
            for (e, min) in outlook.suggested.get(day).into_iter().flatten() {
                by_exam[outlook.weighted[*e]] += min;
            }
        }
        let mut split: Vec<(String, u32)> = by_exam.into_iter().enumerate().filter(|(_, m)| *m > 0).map(|(n, m)| (self.deadlines[n].text.clone(), m)).collect();
        split.sort_by_key(|(_, m)| std::cmp::Reverse(*m));
        split
    }

    /// Everything the Plan page shows, worked out once.
    pub(crate) fn plan_view(&self) -> PlanView {
        let today = self.today_day();
        let outlook = self.outlook();
        let deadlines = self
            .deadlines
            .iter()
            .enumerate()
            .filter(|(_, d)| d.day >= today)
            .map(|(index, d)| {
                let planned = outlook.sources.iter().position(|(s, _)| *s == Source::Deadline(index));
                DeadlineRow { index, name: d.text.clone(), day: d.day, est_min: d.est_min, done_min: d.done_min, left_min: d.est_min.saturating_sub(d.done_min), short_min: planned.map_or(0, |l| outlook.plan.short[l]), cfu: d.cfu }
            })
            .collect();
        PlanView {
            today: self.study_rows(&outlook, today),
            split: self.week_split(&outlook),
            studied_today: self.study_log.iter().filter(|(d, _, _)| *d == today).map(|(_, _, m)| m).sum(),
            room_today: outlook.room_today,
            day_off: !self.study_hours(today).on,
            week: self.week_of(&outlook),
            deadlines,
            studied: self.studied_this_week(),
        }
    }

    /// Adds an exam or a hand-in, or moves the one it names: one by the same name, or the one the line was opened to
    /// change (`replaces`).
    pub(super) fn set_deadline(&mut self, mut new: Deadline, replaces: Option<String>) {
        new.text = new.text.chars().take(80).collect();
        let (day, text) = (new.day, new.text.clone());
        // "Exam" is what a line without a name makes: two of those are two exams
        let named = replaces.as_deref().or((text != "Exam").then_some(text.as_str())).map(str::to_lowercase);
        let old = named.and_then(|n| self.deadlines.iter().position(|d| d.text.to_lowercase() == n)).map(|i| self.deadlines.remove(i));
        new.done_min = old.as_ref().map_or(0, |o| o.done_min);
        // typed again from scratch: what is not said stays. A line opened to change it says everything.
        if let (Some(o), None) = (&old, &replaces) {
            if new.course.is_empty() {
                new.course = o.course.clone();
            }
            if new.est_min == 0 {
                new.est_min = o.est_min;
            }
            if new.cfu == 0 {
                new.cfu = o.cfu;
            }
        }
        self.deadlines.push(new);
        self.deadlines.sort_by_key(|d| d.day);
        self.deadlines.truncate(20);
        // a new one may have been studied for before it was written down
        if old.is_none() {
            if let Some(n) = self.deadlines.iter().position(|d| d.day == day && d.text == text) {
                self.deadlines[n].done_min = self.logged_for(n);
            }
        }
        let left = day - self.today_day();
        let head = match &old {
            None => "Deadline added",
            Some(o) if o.day != day => "Deadline moved",
            Some(_) => "Deadline updated",
        };
        self.say(head, &format!("{text} · {}", if left <= 0 { "today".to_string() } else { format!("in {left} day{}", if left == 1 { "" } else { "s" }) }));
    }

    /// Opens the bar on the line that makes this deadline, the cursor after its day.
    pub(super) fn edit_deadline(&mut self, n: usize) {
        let Some(d) = self.deadlines.get(n) else { return };
        let (y, m, day) = island_core::calendar::civil_from_days(d.day);
        let start = format!("exam {y:04}-{m:02}-{day:02}");
        let mut text = format!("{start} {}", d.text);
        if !d.course.is_empty() {
            text.push_str(&format!(" #{}", d.course.replace(' ', "_")));
        }
        if d.cfu > 0 {
            text.push_str(&format!(" {}cfu", d.cfu));
        }
        if d.est_min > 0 {
            text.push_str(&format!(" ~{}", task::fmt_minutes(d.est_min)));
        }
        self.bar = Some(Bar { until: self.now + BAR_IDLE_MS, cursor: start.chars().count(), text, replaces: Some(d.text.clone()), ..Default::default() });
    }

    /// "Study 1h20: Algoritmi exam 45m, Report 35m", or None when there is nothing to study.
    pub(crate) fn study_line(&self, day: i64) -> Option<String> {
        let plan = self.study_on(day);
        let total: u32 = plan.iter().map(|(_, m)| m).sum();
        (total > 0).then(|| format!("Study {}: {}", task::fmt_minutes(total), plan.iter().map(|(l, m)| format!("{l} {}", task::fmt_minutes(*m))).collect::<Vec<_>>().join(", ")))
    }

    /// The deadline a study of `course` counts for: the nearest one of it that had not passed on `day`.
    fn deadline_for(&self, course: &str, day: i64) -> Option<usize> {
        self.deadlines.iter().enumerate().filter(|(_, d)| d.day >= day && plan::course_matches(course, &d.course, &d.text)).min_by_key(|(_, d)| d.day).map(|(i, _)| i)
    }

    /// The minutes in the study log that count for the deadline at this position.
    fn logged_for(&self, n: usize) -> u32 {
        self.study_log.iter().filter(|(day, course, _)| self.deadline_for(course, *day) == Some(n)).map(|(_, _, m)| m).sum()
    }

    /// A deadline with no hours and nothing counted takes what the log holds for it (earlier versions counted nothing for
    /// one without hours). One with hours is left alone: they were already reduced by what was studied.
    pub(super) fn catch_up_study(&mut self) {
        for n in 0..self.deadlines.len() {
            if self.deadlines[n].est_min == 0 && self.deadlines[n].done_min == 0 {
                self.deadlines[n].done_min = self.logged_for(n);
            }
        }
    }

    /// Study time done: kept in the log and counted for the nearest task or deadline of that course that has work left,
    /// else for the nearest deadline of it.
    pub(super) fn log_study(&mut self, minutes: u32, course: &str) {
        let today = self.today_day();
        self.study_log.push((today, course.trim().to_string(), minutes));
        self.study_log.retain(|(d, _, _)| *d > today - LOG_DAYS);
        let loads = self.load_sources();
        let all: Vec<Load> = loads.iter().map(|(_, l)| l.clone()).collect();
        let to = plan::load_for_course(&all, course).map(|i| loads[i].0).or_else(|| self.deadline_for(course, today).map(Source::Deadline));
        let name = if course.trim().is_empty() { "Study time".to_string() } else { course.trim().to_string() };
        let body = match to {
            Some(Source::Task(n)) => {
                self.todos[n].est_min = self.todos[n].est_min.saturating_sub(minutes);
                left_line(&self.todos[n].text, self.todos[n].est_min)
            }
            Some(Source::Deadline(n)) => {
                let d = &mut self.deadlines[n];
                d.done_min += minutes;
                if d.est_min > 0 {
                    left_line(&d.text, d.est_min.saturating_sub(d.done_min))
                } else {
                    let hint = if d.cfu > 0 { "" } else { " · add its CFU for a plan" };
                    format!("{}: {} studied{hint}", d.text, task::fmt_minutes(d.done_min))
                }
            }
            None if course.trim().is_empty() => String::new(),
            None => "No exam or task by that name".to_string(),
        };
        self.say(&format!("Logged {} · {name}", task::fmt_minutes(minutes)), &body);
    }

    pub fn study_log(&self) -> &[(i64, String, u32)] {
        &self.study_log
    }

    pub fn restore_study_log(&mut self, log: Vec<(i64, String, u32)>) {
        self.study_log = log;
        self.catch_up_study();
    }

    /// Minutes studied this week (from Monday), per course, most first.
    pub(crate) fn studied_this_week(&self) -> Vec<(String, u32)> {
        let today = self.today_day();
        plan::studied_per_course(&self.study_log, today - island_core::calendar::weekday_of_days(today))
    }

    // ---- the week

    /// The seven days from today, each with its load.
    pub(crate) fn week_loads(&self) -> Vec<DayLoad> {
        self.week_of(&self.outlook())
    }

    fn week_of(&self, outlook: &Outlook) -> Vec<DayLoad> {
        let today = self.today_day();
        // only what falls in the study hours of each day is counted
        let ranges: Vec<Vec<(i64, i64)>> = (0..7).map(|i| self.study_windows(today + i)).collect();
        let mut loads: Vec<Load> = outlook.sources.iter().map(|(_, l)| l.clone()).collect();
        // what is due without an estimate still counts as "due" on its day
        loads.extend(self.todos.iter().filter(|t| t.est_min == 0 && t.repeat == Repeat::Never).filter_map(|t| Some(Load { label: t.text.clone(), course: t.course.clone(), due: t.due?, est_min: 0 })));
        loads.extend(self.deadlines.iter().filter(|d| d.est_min == 0).map(|d| Load { label: d.text.clone(), course: d.course.clone(), due: d.day, est_min: 0 }));
        let study: Vec<u32> = (0..7).map(|i| outlook.plan.minutes_on(i)).collect();
        let mut week = plan::week(&self.data.events, &ranges, today, &loads, &study);
        for (i, day) in week.iter_mut().enumerate() {
            day.suggested_min = outlook.suggested.get(i).map_or(0, |d| d.iter().map(|(_, m)| m).sum());
        }
        week
    }

    // ---- search

    /// What the island itself knows about some words: events, tasks, deadlines, pins.
    fn local_hits(&self, query: &str) -> Vec<Hit> {
        let off = self.data.utc_offset_secs;
        let mut out = vec![];
        for (_, e) in self.data.events.iter().enumerate().filter(|(_, e)| plan::matches(&format!("{} {} {}", e.title, e.location, e.calendar), query)) {
            let t = crate::clock::civil(e.start_ms, off);
            out.push(Hit { kind: HitKind::Event, title: e.title.clone(), detail: format!("{} · {}", island_core::quick::fmt_date((t.year as i64, t.month as i64, t.day as i64)), if e.all_day { "all day".into() } else { t.hhmm() }), target: e.url.clone().unwrap_or_default() });
        }
        let today = self.today_day();
        for t in self.todos.iter().filter(|t| plan::matches(&format!("{} {}", t.text, t.course), query)) {
            out.push(Hit { kind: HitKind::Task, title: t.text.clone(), detail: t.due.map_or("to do".into(), |d| task::due_label(d, today)), target: String::new() });
        }
        for d in self.deadlines.iter().filter(|d| plan::matches(&format!("{} {}", d.text, d.course), query)) {
            out.push(Hit { kind: HitKind::Deadline, title: d.text.clone(), detail: task::due_label(d.day, today), target: String::new() });
        }
        for p in self.pins.iter().filter(|p| plan::matches(p, query)) {
            out.push(Hit { kind: HitKind::Pin, title: p.clone(), detail: "pinned".into(), target: p.clone() });
        }
        out
    }

    /// Starts a search: what the island knows is shown at once, the notes and files follow.
    pub(super) fn start_find(&mut self, query: String) {
        let hits = self.local_hits(&query);
        self.found = Found { query: query.clone(), hits, searching: true };
        self.pending.push(Command::Find(query));
        self.push(self.now, Input::OpenPage(Page::Find));
    }

    pub(super) fn found_more(&mut self, query: String, hits: Vec<Hit>) {
        if self.found.query == query {
            self.found.hits.extend(hits);
            self.found.hits.truncate(40);
            self.found.searching = false;
        }
    }

    pub(super) fn open_hit(&mut self, i: usize) {
        let Some(h) = self.found.hits.get(i).cloned() else { return };
        match h.kind {
            HitKind::File | HitKind::Note if !h.target.is_empty() => self.pending.push(Command::OpenFile(h.target)),
            HitKind::Event if !h.target.is_empty() => self.pending.push(Command::OpenUrl(h.target)),
            HitKind::Pin | HitKind::Task | HitKind::Event | HitKind::Deadline | HitKind::File | HitKind::Note => {
                self.pending.push(Command::RestoreClipboard(h.title.clone()));
                self.say("Copied", &h.title);
            }
        }
    }
}

/// "Report: 2h left", or that it is all done.
fn left_line(what: &str, minutes: u32) -> String {
    if minutes == 0 {
        format!("{what}: all the work is done")
    } else {
        format!("{what}: {} left", task::fmt_minutes(minutes))
    }
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}
