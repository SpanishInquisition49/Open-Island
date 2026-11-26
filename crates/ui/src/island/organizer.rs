//! The organizer: tasks (a day, a course, an estimate, a repetition), events made from the bar, study time, the week ahead
//! and the search. Everything is decided here from what the island holds; the files and servers are the platform layer's.

use super::*;
use island_core::plan::{self, DayLoad, Hit, HitKind, Load};
use island_core::task::{self, Repeat, Spec, Task};

/// Where a load of work comes from, to take studied time off it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Task(usize),
    Deadline(usize),
}

/// The days kept in the study log.
const LOG_DAYS: i64 = 120;

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
        let names: Vec<String> = idxs.iter().take(3).map(|&i| self.todos[i].text.clone()).collect();
        let more = idxs.len().saturating_sub(3);
        let line = format!("{}: {}{}", if morning { "Late" } else { "Left today" }, names.join(", "), if more > 0 { format!(" +{more}") } else { String::new() });
        let card = match card {
            Some((s, b)) => (s, format!("{line} · {b}")),
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
        let lighter = week.iter().filter(|d| d.day >= today && d.day < heavy.day && load(d) + 60 < load(heavy)).min_by_key(|d| load(d));
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
            if d.est_min > 0 && d.day >= today {
                v.push((Source::Deadline(i), Load { label: d.text.clone(), course: d.course.clone(), due: d.day, est_min: d.est_min }));
            }
        }
        v
    }

    /// What to study on a day: (what, minutes), the most pressing first.
    pub(crate) fn study_on(&self, day: i64) -> Vec<(String, u32)> {
        let mut v: Vec<(i64, String, u32)> = self.load_sources().into_iter().filter_map(|(_, l)| Some((l.due, l.label.clone(), Some(plan::study_share(&l, day)).filter(|m| *m > 0)?))).collect();
        v.sort_by_key(|(due, _, _)| *due);
        v.into_iter().map(|(_, l, m)| (l, m)).collect()
    }

    /// "Study 1h20: Algoritmi exam 45m, Report 35m", or None when there is nothing to study.
    pub(crate) fn study_line(&self, day: i64) -> Option<String> {
        let plan = self.study_on(day);
        let total: u32 = plan.iter().map(|(_, m)| m).sum();
        (total > 0).then(|| format!("Study {}: {}", task::fmt_minutes(total), plan.iter().take(3).map(|(l, m)| format!("{l} {}", task::fmt_minutes(*m))).collect::<Vec<_>>().join(", ")))
    }

    /// Study time done: kept in the log and taken off the work still to do for that course.
    pub(super) fn log_study(&mut self, minutes: u32, course: &str) {
        let today = self.today_day();
        self.study_log.push((today, course.trim().to_string(), minutes));
        self.study_log.retain(|(d, _, _)| *d > today - LOG_DAYS);
        let loads = self.load_sources();
        let all: Vec<Load> = loads.iter().map(|(_, l)| l.clone()).collect();
        let left = plan::load_for_course(&all, course).map(|i| {
            match loads[i].0 {
                Source::Task(n) => {
                    self.todos[n].est_min = self.todos[n].est_min.saturating_sub(minutes);
                    (self.todos[n].text.clone(), self.todos[n].est_min)
                }
                Source::Deadline(n) => {
                    self.deadlines[n].est_min = self.deadlines[n].est_min.saturating_sub(minutes);
                    (self.deadlines[n].text.clone(), self.deadlines[n].est_min)
                }
            }
        });
        let name = if course.trim().is_empty() { "Study time".to_string() } else { course.trim().to_string() };
        match left {
            Some((what, 0)) => self.say(&format!("Logged {} · {name}", task::fmt_minutes(minutes)), &format!("{what}: all the work is done")),
            Some((what, m)) => self.say(&format!("Logged {} · {name}", task::fmt_minutes(minutes)), &format!("{what}: {} left", task::fmt_minutes(m))),
            None => self.say(&format!("Logged {} · {name}", task::fmt_minutes(minutes)), ""),
        }
    }

    pub fn study_log(&self) -> &[(i64, String, u32)] {
        &self.study_log
    }

    pub fn restore_study_log(&mut self, log: Vec<(i64, String, u32)>) {
        self.study_log = log;
    }

    /// Minutes studied this week (from Monday), per course, most first.
    pub(crate) fn studied_this_week(&self) -> Vec<(String, u32)> {
        let today = self.today_day();
        plan::studied_per_course(&self.study_log, today - island_core::calendar::weekday_of_days(today))
    }

    // ---- the week

    /// The seven days from today, each with its load.
    pub(crate) fn week_loads(&self) -> Vec<DayLoad> {
        let today = self.today_day();
        let ranges: Vec<(i64, i64)> = (0..7).map(|i| (self.day_start_ms(today + i), self.day_start_ms(today + i + 1))).collect();
        let mut loads: Vec<Load> = self.load_sources().into_iter().map(|(_, l)| l).collect();
        // what is due without an estimate still counts as "due" on its day
        loads.extend(self.todos.iter().filter(|t| t.est_min == 0 && t.repeat == Repeat::Never).filter_map(|t| Some(Load { label: t.text.clone(), course: t.course.clone(), due: t.due?, est_min: 0 })));
        loads.extend(self.deadlines.iter().filter(|d| d.est_min == 0).map(|d| Load { label: d.text.clone(), course: d.course.clone(), due: d.day, est_min: 0 }));
        plan::week(&self.data.events, &ranges, today, &loads)
    }

    pub(crate) fn plan_height(&self) -> f64 {
        let studied = self.studied_this_week().len().min(4);
        14.0 + 34.0 + 7.0 * 28.0 + 8.0 + if self.study_line(self.today_day()).is_some() { 24.0 } else { 0.0 } + if studied > 0 { 22.0 + studied as f64 * 24.0 } else { 0.0 } + 10.0
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

fn cap(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}
