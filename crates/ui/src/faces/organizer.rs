//! The Plan page (the week ahead and the study to do) and the Find page (what a search found).

use super::*;
use crate::island::{PLAN_ROWS, SPLIT_ROWS};
use island_core::plan::HitKind;
use island_core::task;

/// A calendar's own colour, white when it has none.
fn calendar_color(c: Option<(u8, u8, u8)>) -> Color {
    c.map_or(theme::ACCENT, theme::rgb)
}

/// The heights of the Plan page: a day of the week, a section's title, a row of study or of deadlines, the room between.
const DAY_ROW: f64 = 26.0;
const SECTION: f64 = 22.0;
const PLAN_ROW: f64 = 26.0;
const GAP: f64 = 6.0;
const FIND_ROWS: usize = 8;

impl Island {
    pub(crate) fn plan_height(&self) -> f64 {
        let v = self.plan_view();
        let today = v.today.len().clamp(1, PLAN_ROWS) as f64;
        let deadlines = v.deadlines.len().clamp(1, PLAN_ROWS) as f64;
        let studied = v.studied.len().min(4);
        let split = v.split.len().min(SPLIT_ROWS);
        14.0 + 34.0
            + SECTION + today * PLAN_ROW + GAP
            + SECTION + v.week.len() as f64 * DAY_ROW + GAP
            + SECTION + deadlines * PLAN_ROW
            + if split > 0 { GAP + SECTION + split as f64 * 24.0 } else { 0.0 }
            + if studied > 0 { GAP + SECTION + studied as f64 * 24.0 } else { 0.0 }
            + 10.0
    }

    pub(super) fn face_plan(&self, p: P, w: f64, _h: f64) {
        let head = self.page_header(p, w, "Plan");
        let add = p.button_width("+ Exam");
        p.button(rect(w - 16.0 - add, head - 12.0, add, 24.0), "+ Exam", Action::DeadlineAdd);
        let view = self.plan_view();
        let today = self.today_day();
        let mut y = 14.0 + 34.0;
        let section = |p: P, y: f64, title: &str| {
            p.text(title, 20.0, y + 10.0, 10.0, SEMI, theme::DIM, Align::Left, 200.0);
        };

        // today: what to study, and how much of each
        p.stagger(1);
        let total: u32 = view.today.iter().map(|r| r.min).sum();
        let more = view.today.len().saturating_sub(PLAN_ROWS);
        let mut title = if total > 0 { format!("Study today · {}", task::fmt_minutes(total)) } else { "Study today".to_string() };
        if more > 0 {
            title.push_str(&format!(" · +{more} more"));
        }
        section(p, y, &title);
        if view.studied_today > 0 {
            p.text(&format!("{} done", task::fmt_minutes(view.studied_today)), w - 18.0, y + 10.0, 10.0, SEMI, theme::GREEN, Align::Right, 90.0);
        }
        y += SECTION;
        if view.today.is_empty() {
            let msg = if !view.deadlines.is_empty() && view.deadlines.iter().all(|d| d.est_min == 0 && d.cfu == 0) {
                "Click an exam and add its CFU (9cfu) to get a daily plan"
            } else if view.studied_today > 0 {
                "Done for today"
            } else if view.deadlines.is_empty() {
                "Add an exam with its CFU to get a daily plan"
            } else if view.room_today == 0 {
                "No study time left today"
            } else {
                "Nothing planned today"
            };
            p.text(msg, 20.0, y + PLAN_ROW / 2.0, 11.5, REG, theme::DIM, Align::Left, (w - 40.0) as f32);
            y += PLAN_ROW;
        }
        for row in view.today.iter().take(PLAN_ROWS) {
            let cy = y + PLAN_ROW / 2.0;
            p.icon("target", 26.0, cy, 13.0, theme::ACCENT);
            p.text(&row.what, 44.0, cy, 12.0, REG, theme::FG, Align::Left, (w - 44.0 - 160.0) as f32);
            p.text(&format!("due {}", task::due_label(row.due, today)), w - 78.0, cy, 10.5, REG, theme::DIM, Align::Right, 90.0);
            // a suggestion from the credits is marked as one
            let amount = format!("{}{}", if row.suggested { "≈ " } else { "" }, task::fmt_minutes(row.min));
            p.text(&amount, w - 18.0, cy, 12.0, SEMI, theme::FG, Align::Right, 58.0);
            y += PLAN_ROW;
        }
        y += GAP;

        // the week: each calendar in its colour, then the study, side by side in one bar; the longest day sets the scale,
        // at least a working day
        p.stagger(2);
        section(p, y, "Week");
        // the legend, from the right: the study, and before it the calendars of the week, the busiest first
        let mut calendars: Vec<(&str, Color, u32)> = vec![];
        for b in view.week.iter().flat_map(|d| &d.calendars) {
            match calendars.iter_mut().find(|c| c.0 == b.calendar) {
                Some(c) => c.2 += b.min,
                None => calendars.push((&b.calendar, calendar_color(b.color), b.min)),
            }
        }
        calendars.sort_by_key(|c| std::cmp::Reverse(c.2));
        let study = with_alpha(theme::ACCENT, 0.55);
        let mut x = w - 18.0;
        for (name, color) in std::iter::once(("study", study)).chain(calendars.iter().take(3).rev().map(|c| (if c.0.is_empty() { "events" } else { c.0 }, c.1))) {
            let text_w = (p.fonts.measure(name, 10.0, REG) as f64).min(72.0);
            x -= text_w;
            p.text(name, x, y + 10.0, 10.0, REG, theme::DIM, Align::Left, 72.0);
            p.rrect(rect(x - 14.0, y + 7.0, 10.0, 6.0), 3.0, color);
            x -= 26.0;
        }
        y += SECTION;
        let scale = view.week.iter().map(|d| d.event_min + d.study_min + d.suggested_min).max().unwrap_or(0).max(8 * 60) as f64;
        for (n, d) in view.week.iter().enumerate() {
            p.stagger(2 + n);
            let cy = y + DAY_ROW / 2.0;
            let row = rect(8.0, y, w - 16.0, DAY_ROW - 2.0);
            let (_, _, dom) = island_core::calendar::civil_from_days(d.day);
            let name = &clock::WEEKDAYS[island_core::calendar::weekday_of_days(d.day) as usize][..3];
            let label = if d.day == today { "Today".to_string() } else { format!("{name} {dom}") };
            let heavy = d.heavy();
            if d.day == today {
                p.rrect(row, 8.0, theme::CHIP);
            }
            p.text(&label, 20.0, cy, 12.0, if d.day == today { SEMI } else { REG }, theme::FG, Align::Left, 56.0);
            let bar = rect(78.0, cy - 3.0, w - 78.0 - 112.0, 6.0);
            p.rrect(bar, 3.0, theme::TRACK);
            // the calendars in the order of the legend, so that each keeps its place from day to day
            let parts = calendars.iter().filter_map(|c| d.calendars.iter().find(|b| b.calendar == c.0).map(|b| (c.1, b.min))).chain([(study, d.study_min + d.suggested_min)]);
            let mut x = bar.x0;
            for (color, min) in parts.filter(|(_, min)| *min > 0) {
                let part = (min as f64 / scale * bar.width()).max(3.0).min(bar.x1 - x);
                if part > 0.0 {
                    p.rrect(rect(x, bar.y0, part, 6.0), 3.0, color);
                    x += part;
                }
            }
            if d.due > 0 {
                p.icon("flame", w - 90.0, cy, 11.0, theme::ORANGE);
                p.text(&d.due.to_string(), w - 82.0, cy, 10.5, SEMI, theme::ORANGE, Align::Left, 16.0);
            }
            let hours = d.event_min + d.study_min + d.suggested_min;
            let hours = if hours == 0 { "free".to_string() } else { task::fmt_minutes(hours) };
            p.text(&hours, w - 18.0, cy, 11.0, if heavy { SEMI } else { REG }, if heavy { theme::ORANGE } else { theme::DIM }, Align::Right, 50.0);
            y += DAY_ROW;
        }
        y += GAP;

        // the deadlines: a click changes one (its day first), the cross removes it
        p.stagger(9);
        let more = view.deadlines.len().saturating_sub(PLAN_ROWS);
        section(p, y, &if more > 0 { format!("Deadlines · +{more} more") } else { "Deadlines".to_string() });
        y += SECTION;
        if view.deadlines.is_empty() {
            p.text("No exam yet: + Exam, or type  exam 3 feb Algorithms ~20h", 20.0, y + PLAN_ROW / 2.0, 11.5, REG, theme::DIM, Align::Left, (w - 40.0) as f32);
            y += PLAN_ROW;
        }
        for d in view.deadlines.iter().take(PLAN_ROWS) {
            let row = rect(8.0, y, w - 16.0, PLAN_ROW - 2.0);
            if p.hovered(row) {
                p.rrect(row, 8.0, theme::CHIP);
            }
            let cy = y + PLAN_ROW / 2.0;
            let days = d.day - today;
            let urgent = days <= 3;
            p.icon("flame", 26.0, cy, 13.0, if urgent { theme::ORANGE } else { theme::DIM });
            // the credits, when said, take part of the name's room
            let room = w - 44.0 - if d.cfu > 0 { 262.0 } else { 214.0 };
            let name_w = p.text(&d.name, 44.0, cy, 12.0, REG, theme::FG, Align::Left, room as f32) as f64;
            if d.cfu > 0 {
                p.text(&format!("{} CFU", d.cfu), 44.0 + name_w + 8.0, cy, 10.0, REG, theme::DIM, Align::Left, 44.0);
            }
            let (_, month, dom) = island_core::calendar::civil_from_days(d.day);
            let left = match days {
                0 => "today".to_string(),
                1 => "tomorrow".to_string(),
                n => format!("{n} d"),
            };
            let when = format!("{dom} {} · {left}", &clock::MONTHS[month as usize - 1][..3]);
            p.text(&when, w - 118.0, cy, 10.5, if urgent { SEMI } else { REG }, if urgent { theme::ORANGE } else { theme::DIM }, Align::Right, 100.0);
            let (amount, color) = if d.short_min > 0 {
                (format!("{} won't fit", task::fmt_minutes(d.short_min)), theme::ORANGE)
            } else {
                match (d.est_min, d.done_min) {
                    (0, 0) if d.cfu > 0 => ("by CFU".to_string(), theme::DIM),
                    (0, 0) => ("no CFU".to_string(), theme::DIM),
                    (0, done) => (format!("{} done", task::fmt_minutes(done)), theme::GREEN),
                    (est, 0) => (format!("{} left", task::fmt_minutes(est)), theme::FG),
                    _ if d.left_min == 0 => ("all done".to_string(), theme::GREEN),
                    (est, done) => (format!("{} of {}", task::fmt_minutes(done), task::fmt_minutes(est)), theme::FG),
                }
            };
            p.text(&amount, w - 38.0, cy, 10.5, REG, color, Align::Right, 76.0);
            p.hit(row, Action::DeadlineEdit(d.index));
            p.icon_button(row.x1 - 12.0, cy, 20.0, "x", 10.0, Action::DeadlineDone(d.index));
            y += PLAN_ROW;
        }

        // the suggestion at a glance: how the study of the next seven days divides among the exams
        if !view.split.is_empty() {
            y += GAP;
            p.stagger(10);
            section(p, y, "Suggested split · next 7 days");
            y += SECTION;
            let total = view.split.iter().map(|(_, m)| m).sum::<u32>().max(1);
            for (name, min) in view.split.iter().take(SPLIT_ROWS) {
                p.text(name, 20.0, y + 10.0, 12.0, REG, theme::FG, Align::Left, 110.0);
                p.bar(rect(136.0, y + 7.0, w - 136.0 - 112.0, 6.0), *min as f64 / total as f64, theme::ACCENT, theme::TRACK);
                p.text(&task::fmt_minutes(*min), w - 58.0, y + 10.0, 11.0, REG, theme::FG, Align::Right, 48.0);
                p.text(&format!("{}%", (*min * 100 + total / 2) / total), w - 20.0, y + 10.0, 11.0, SEMI, theme::DIM, Align::Right, 36.0);
                y += 24.0;
            }
        }

        if !view.studied.is_empty() {
            y += GAP;
            p.stagger(10);
            section(p, y, "Studied this week");
            y += SECTION;
            let biggest = view.studied[0].1.max(1) as f64;
            for (course, min) in view.studied.iter().take(4) {
                p.text(course, 20.0, y + 10.0, 12.0, REG, theme::FG, Align::Left, 110.0);
                p.bar(rect(136.0, y + 7.0, w - 136.0 - 70.0, 6.0), *min as f64 / biggest, theme::ACCENT, theme::TRACK);
                p.text(&task::fmt_minutes(*min), w - 20.0, y + 10.0, 11.0, REG, theme::FG, Align::Right, 52.0);
                y += 24.0;
            }
        }
    }

    pub(crate) fn find_height(&self) -> f64 {
        14.0 + 34.0 + 6.0 + self.found.hits.len().clamp(1, FIND_ROWS) as f64 * 30.0 + 10.0
    }

    pub(super) fn face_find(&self, p: P, w: f64, _h: f64) {
        let title = if self.found.query.is_empty() { "Find".to_string() } else { format!("Find · {}", self.found.query) };
        self.page_header(p, w, &title);
        let mut y = 14.0 + 34.0 + 6.0;
        if self.found.hits.is_empty() {
            let msg = if self.found.searching { "Looking through your notes and files…" } else if self.found.query.is_empty() { "Type  find <words>  in the command bar" } else { "Nothing found" };
            p.text(msg, w / 2.0, y + 16.0, 12.0, REG, theme::DIM, Align::Center, (w - 40.0) as f32);
            return;
        }
        for (n, h) in self.found.hits.iter().take(FIND_ROWS).enumerate() {
            p.stagger(1 + n);
            let row = rect(8.0, y, w - 16.0, 28.0);
            if p.hovered(row) {
                p.rrect(row, 8.0, theme::CHIP);
            }
            let cy = y + 14.0;
            let icon = match h.kind {
                HitKind::Event => "calendar-event",
                HitKind::Task => "check",
                HitKind::Deadline => "flame",
                HitKind::Pin => "pin",
                HitKind::Note | HitKind::File => "file",
            };
            p.icon(icon, 26.0, cy, 13.0, theme::ACCENT);
            let detail_w = (p.fonts.measure(&h.detail, 10.0, REG) as f64).min(150.0);
            p.text(&h.title, 44.0, cy, 12.0, REG, theme::FG, Align::Left, (w - 44.0 - detail_w - 30.0).max(60.0) as f32);
            p.text(&h.detail, w - 18.0, cy, 10.0, REG, theme::DIM, Align::Right, detail_w as f32 + 2.0);
            p.hit(row, Action::FindOpen(n));
            y += 30.0;
        }
    }
}
