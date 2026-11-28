//! A study session (a Pomodoro on an exam or a task: quiet while the focus runs, its time counted by itself) and the pace
//! per exam (whether what is being studied gets there in time).

use super::organizer::Source;
use super::*;
use island_core::plan::{self, Load, Pace};

/// A focus left before this many minutes was a false start: nothing is counted.
const MIN_SESSION_MIN: i64 = 5;
/// The furthest ahead the study days before an exam are counted.
const PACE_HORIZON: i64 = 400;

impl Island {
    /// The course of the running Pomodoro when it is a study session: one that was started for something.
    pub(crate) fn session(&self) -> Option<&Timer> {
        self.timer.as_ref().filter(|t| t.mode == TimerMode::Pomodoro && !t.label.is_empty())
    }

    /// Starts a Pomodoro whose focus time counts for `course`.
    pub(super) fn start_session(&mut self, course: String) {
        self.start_pomodoro();
        if let Some(t) = self.timer.as_mut() {
            t.label = course;
        }
    }

    /// The row of today's study at this position of the Plan page: starts its session.
    pub(super) fn study_start(&mut self, row: usize) {
        let Some(label) = self.plan_view().today.get(row).map(|r| r.session.clone()) else { return };
        self.start_session(label);
    }

    /// The minutes of a focus that ran, when they are enough to count. Time past its end is not focus.
    pub(super) fn focus_minutes(&self, t: &Timer) -> Option<u32> {
        let end = t.end_ms.map_or(self.now, |end| end.min(self.now));
        let minutes = (end - t.start_ms) / 60_000;
        (t.mode == TimerMode::Pomodoro && t.phase == Phase::Focus && minutes >= MIN_SESSION_MIN).then_some(minutes as u32)
    }

    /// A session about to be stopped or replaced: what was studied of its focus is not lost.
    pub(super) fn leave_session(&mut self) {
        let Some(t) = self.session().cloned() else { return };
        if let Some(minutes) = self.focus_minutes(&t) {
            self.log_study(minutes, &t.label);
        }
    }

    /// What a study of `course` on `day` counts for. Something named exactly so wins; else the task or deadline of that
    /// course with work left that is due first, else its nearest deadline.
    pub(super) fn study_target(&self, sources: &[(Source, Load)], course: &str, day: i64) -> Option<Source> {
        let name = course.trim().to_lowercase();
        let exact = sources.iter().filter(|(_, l)| !name.is_empty() && l.label.to_lowercase() == name).min_by_key(|(_, l)| l.due).map(|(s, _)| *s);
        let named = || self.deadlines.iter().position(|d| !name.is_empty() && d.day >= day && d.text.to_lowercase() == name).map(Source::Deadline);
        let of_course = || {
            let loads: Vec<Load> = sources.iter().map(|(_, l)| l.clone()).collect();
            plan::load_for_course(&loads, course).map(|i| sources[i].0)
        };
        exact.or_else(named).or_else(of_course).or_else(|| self.deadline_for(course, day).map(Source::Deadline))
    }

    /// The name a session on this task or deadline is logged under: its course when that leads back to it, else its text.
    pub(super) fn session_name(&self, sources: &[(Source, Load)], source: Source) -> String {
        let (course, text) = match source {
            Source::Task(n) => self.todos.get(n).map(|t| (t.course.clone(), t.text.clone())),
            Source::Deadline(n) => self.deadlines.get(n).map(|d| (d.course.clone(), d.text.clone())),
        }
        .unwrap_or_default();
        if !course.is_empty() && self.study_target(sources, &course, self.today_day()) == Some(source) {
            course
        } else {
            text
        }
    }

    /// Whether the study of the last days gets through the hours of the deadline at this position before it comes. None
    /// without hours (there is nothing to measure against), once they are done, and from its day on.
    pub(crate) fn pace_of(&self, sources: &[(Source, Load)], n: usize) -> Option<Pace> {
        let today = self.today_day();
        let d = self.deadlines.get(n)?;
        if d.day <= today || d.est_min == 0 || d.done_min >= d.est_min {
            return None;
        }
        let log: Vec<(i64, u32)> = self
            .study_log
            .iter()
            .filter(|(day, course, _)| self.deadline_for(course, *day) == Some(n) && !matches!(self.study_target(sources, course, *day), Some(Source::Task(_))))
            .map(|(day, _, min)| (*day, *min))
            .collect();
        // today counts once something was studied: before that it is still to come
        let last = if log.iter().any(|(day, _)| *day == today) { today } else { today - 1 };
        let first = log.iter().map(|(day, _)| *day).min().unwrap_or(last);
        let from = last - (last - first + 1).clamp(plan::PACE_DAYS.0, plan::PACE_DAYS.1) + 1;
        let studied: u32 = log.iter().filter(|(day, _)| (from..=last).contains(day)).map(|(_, m)| m).sum();
        let measured = (from..=last).filter(|day| self.study_hours(*day).on || log.iter().any(|(d, _)| d == day)).count();
        let left = (last + 1..d.day.min(today + PACE_HORIZON)).filter(|day| self.study_hours(*day).on).count();
        Some(plan::pace(d.est_min, d.done_min, studied, measured as u32, left as u32))
    }
}
