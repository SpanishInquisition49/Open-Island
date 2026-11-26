//! The Plan page (the week ahead and the study to do) and the Find page (what a search found).

use super::*;
use island_core::plan::{self, HitKind};
use island_core::task;

const DAY_ROW: f64 = 28.0;
const FIND_ROWS: usize = 8;

impl Island {
    pub(super) fn face_plan(&self, p: P, w: f64, _h: f64) {
        self.page_header(p, w, "Plan");
        let today = self.today_day();
        let mut y = 14.0 + 34.0;
        // the longest day sets the scale of the bars: at least a working day
        let days = self.week_loads();
        let scale = days.iter().map(|d| d.event_min + d.study_min).max().unwrap_or(0).max(8 * 60) as f64;
        for (n, d) in days.iter().enumerate() {
            p.stagger(1 + n);
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
            // lectures solid, study dim, side by side in one bar
            let bar = rect(78.0, cy - 3.0, 90.0, 6.0);
            p.rrect(bar, 3.0, theme::TRACK);
            let (e, s) = (d.event_min as f64 / scale * bar.width(), d.study_min as f64 / scale * bar.width());
            let fill = if heavy { theme::ORANGE } else { theme::ACCENT };
            if e > 0.0 {
                p.rrect(rect(bar.x0, bar.y0, e.max(3.0), 6.0), 3.0, fill);
            }
            if s > 0.0 {
                p.rrect(rect(bar.x0 + e, bar.y0, s.max(3.0), 6.0), 3.0, with_alpha(fill, 0.45));
            }
            p.text(&plan::day_summary(d), w - 18.0, cy, 10.5, if heavy { SEMI } else { REG }, if heavy { theme::ORANGE } else { theme::DIM }, Align::Right, (w - 190.0) as f32);
            y += DAY_ROW;
        }
        y += 8.0;
        if let Some(line) = self.study_line(today) {
            p.stagger(9);
            p.icon("target", 26.0, y + 10.0, 13.0, theme::ACCENT);
            p.text(&line, 44.0, y + 10.0, 11.5, REG, theme::FG, Align::Left, (w - 44.0 - 16.0) as f32);
            y += 24.0;
        }
        let studied = self.studied_this_week();
        if !studied.is_empty() {
            p.stagger(10);
            p.text("Studied this week", 20.0, y + 8.0, 10.0, SEMI, theme::DIM, Align::Left, 160.0);
            y += 22.0;
            let biggest = studied[0].1.max(1) as f64;
            for (course, min) in studied.iter().take(4) {
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
