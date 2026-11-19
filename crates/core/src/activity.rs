//! Where the attention went: which window had the focus, for how long. A ledger of focus spans, with the totals of the
//! day, the streak in one kind of app, and the recent past for the rewind page. No I/O: the clock comes as an argument.

use std::collections::{HashMap, VecDeque};

const MAX_SPANS: usize = 400;

/// Apps that count as work by default (part of the name, any case): editors, terminals, readers of papers and notes.
pub const DEEP_WORK_APPS: &[&str] = &[
    "code", "zed", "vim", "emacs", "kitty", "foot", "alacritty", "wezterm", "ghostty", "konsole", "terminal", "evince", "okular", "zathura", "sioyek", "xournal", "obsidian", "jetbrains", "idea",
    "pycharm", "clion", "rustrover", "libreoffice", "texstudio", "texmaker", "typora", "logseq", "anki", "foliate", "calibre",
];

/// A window id as a name for people: `org.telegram.desktop` is Telegram, `zen` is Zen.
pub fn pretty_app(app_id: &str) -> String {
    let id = app_id.trim();
    if id.is_empty() {
        return "Desktop".into();
    }
    let parts: Vec<&str> = id.split('.').filter(|p| !p.is_empty()).collect();
    // reverse-DNS ids end in the name, or in "desktop"/"app"/"client" after it
    let name = match parts.as_slice() {
        [] => id,
        [one] => one,
        [.., name, "desktop" | "app" | "client" | "Desktop" | "App"] => name,
        [.., name] => name,
    };
    let name = name.trim_end_matches("-stable").replace(['-', '_'], " ");
    let mut c = name.chars();
    c.next().map_or(name.clone(), |f| f.to_uppercase().collect::<String>() + c.as_str())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub app: String,
    pub title: String,
    pub start_ms: i64,
    pub end_ms: i64,
}

#[derive(Clone, Debug, Default)]
pub struct Ledger {
    spans: VecDeque<Span>,
    /// The span in progress: app, title, start. None while the person is away.
    open: Option<(String, String, i64)>,
    /// The window to come back to when the person returns.
    resume_with: Option<(String, String)>,
    /// Totals of the day in progress, per app, in milliseconds (the open span is not in them).
    day_start: i64,
    totals: HashMap<String, i64>,
}

impl Ledger {
    fn roll_day(&mut self, now: i64, day_start: i64) {
        if day_start != self.day_start {
            self.day_start = day_start;
            self.totals.clear();
        }
        let _ = now;
    }

    fn close(&mut self, end_ms: i64, day_start: i64) {
        let Some((app, title, start)) = self.open.take() else { return };
        let end = end_ms.max(start);
        if end > start {
            // a span that began before midnight counts only from midnight on
            *self.totals.entry(app.clone()).or_insert(0) += end - start.max(day_start);
            self.spans.push_back(Span { app, title, start_ms: start, end_ms: end });
            while self.spans.len() > MAX_SPANS {
                self.spans.pop_front();
            }
        }
    }

    /// The window with the focus changed (or its title did). `day_start` is the start of today in epoch ms.
    pub fn focus(&mut self, now: i64, day_start: i64, app: &str, title: &str) {
        self.roll_day(now, day_start);
        match &mut self.open {
            Some((a, t, _)) if a == app => *t = title.to_string(),
            _ => {
                if self.open.is_some() {
                    self.close(now, day_start);
                }
                // while away nothing is open: the window is only noted for the return
                if self.resume_with.is_some() {
                    self.resume_with = Some((app.to_string(), title.to_string()));
                } else {
                    self.open = Some((app.to_string(), title.to_string(), now));
                }
            }
        }
    }

    /// Nobody has touched the keyboard or the pointer since `since_ms`: time stops counting from then.
    pub fn idle(&mut self, since_ms: i64, day_start: i64) {
        if let Some((a, t, _)) = &self.open {
            self.resume_with = Some((a.clone(), t.clone()));
            self.close(since_ms, day_start);
        }
    }

    /// The person is back.
    pub fn resume(&mut self, now: i64, day_start: i64) {
        self.roll_day(now, day_start);
        if let Some((a, t)) = self.resume_with.take() {
            self.open = Some((a, t, now));
        }
    }

    pub fn current(&self) -> Option<(&str, &str, i64)> {
        self.open.as_ref().map(|(a, t, s)| (a.as_str(), t.as_str(), *s))
    }

    /// Today's time per app, biggest first, with the span in progress.
    pub fn today(&self, now: i64, day_start: i64) -> Vec<(String, i64)> {
        let mut m = if self.day_start == day_start { self.totals.clone() } else { HashMap::new() };
        if let Some((a, _, s)) = &self.open {
            *m.entry(a.clone()).or_insert(0) += (now - s.max(&day_start)).max(0);
        }
        let mut v: Vec<(String, i64)> = m.into_iter().filter(|(_, ms)| *ms > 0).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }

    /// How long the attention has been on apps that satisfy `is_it`, counting through gaps shorter than `gap_ms`.
    pub fn streak(&self, now: i64, gap_ms: i64, is_it: impl Fn(&str) -> bool) -> i64 {
        // newest first: the open span, then the closed ones backwards
        let open = self.open.as_ref().map(|(a, _, s)| (a.as_str(), *s, now));
        let closed = self.spans.iter().rev().map(|s| (s.app.as_str(), s.start_ms, s.end_ms));
        // `away` is the time since the last matching app, gaps included: past `gap_ms` the streak is broken
        let (mut away, mut began, mut cursor) = (0i64, None::<i64>, now);
        for (app, start, end) in open.into_iter().chain(closed) {
            away += cursor - end;
            if away > gap_ms {
                break;
            }
            if is_it(app) {
                began = Some(start);
                away = 0;
            } else {
                away += end - start;
                if away > gap_ms {
                    break;
                }
            }
            cursor = start;
        }
        began.map_or(0, |b| now - b)
    }

    /// The spans that overlap the last `window_ms`, oldest first, with the open one cut at `now`.
    pub fn recent(&self, now: i64, window_ms: i64) -> Vec<Span> {
        let from = now - window_ms;
        let mut v: Vec<Span> = self.spans.iter().filter(|s| s.end_ms > from).cloned().collect();
        if let Some((a, t, s)) = &self.open {
            v.push(Span { app: a.clone(), title: t.clone(), start_ms: *s, end_ms: now });
        }
        v
    }

    /// The totals of the day, to be kept between runs.
    pub fn export(&self, now: i64, day_start: i64) -> (i64, Vec<(String, i64)>) {
        (day_start, self.today(now, day_start))
    }

    /// Takes back what `export` gave, when it is still the same day.
    pub fn restore(&mut self, day_start: i64, totals: Vec<(String, i64)>) {
        if self.day_start == 0 || self.day_start == day_start {
            self.day_start = day_start;
            for (a, ms) in totals {
                *self.totals.entry(a).or_insert(0) += ms;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000;
    const DAY: i64 = 0;

    #[test]
    fn app_ids_become_names() {
        assert_eq!(pretty_app("org.telegram.desktop"), "Telegram");
        assert_eq!(pretty_app("zen"), "Zen");
        assert_eq!(pretty_app("Spotify"), "Spotify");
        assert_eq!(pretty_app("com.github.Eloston.UngoogledChromium"), "UngoogledChromium");
        assert_eq!(pretty_app("code-oss"), "Code oss");
        assert_eq!(pretty_app(""), "Desktop");
    }

    #[test]
    fn time_is_counted_per_app_and_the_open_span_counts() {
        let mut l = Ledger::default();
        l.focus(0, DAY, "code", "main.rs");
        l.focus(10 * MIN, DAY, "zen", "docs");
        l.focus(14 * MIN, DAY, "code", "lib.rs");
        assert_eq!(l.today(20 * MIN, DAY), vec![("code".into(), 16 * MIN), ("zen".into(), 4 * MIN)]);
        l.focus(21 * MIN, DAY, "code", "other.rs");
        assert_eq!(l.current().map(|c| (c.0, c.1)), Some(("code", "other.rs")), "a new title in the same app does not split the span");
        assert_eq!(l.recent(30 * MIN, 25 * MIN).len(), 3);
    }

    #[test]
    fn being_away_stops_the_clock() {
        let mut l = Ledger::default();
        l.focus(0, DAY, "code", "x");
        // two minutes without input: the idle notification fires at 12 min, inactivity began at 10
        l.idle(10 * MIN, DAY);
        assert!(l.current().is_none());
        l.focus(60 * MIN, DAY, "zen", "y"); // a window changes while away: noted for the return
        l.resume(70 * MIN, DAY);
        assert_eq!(l.current().map(|c| c.0), Some("zen"));
        assert_eq!(l.today(80 * MIN, DAY), vec![("code".into(), 10 * MIN), ("zen".into(), 10 * MIN)]);
    }

    #[test]
    fn a_new_day_starts_from_zero_and_midnight_cuts_a_span() {
        let mut l = Ledger::default();
        l.focus(0, DAY, "code", "x");
        let next = 1440 * MIN;
        l.focus(next + 30 * MIN, next, "zen", "y");
        assert_eq!(l.today(next + 40 * MIN, next), vec![("code".into(), 30 * MIN), ("zen".into(), 10 * MIN)]);
    }

    #[test]
    fn a_streak_bridges_short_detours_and_breaks_on_long_ones() {
        let work = |a: &str| a == "code" || a == "pdf";
        let mut l = Ledger::default();
        l.focus(0, DAY, "code", "");
        l.focus(10 * MIN, DAY, "pdf", "");
        l.focus(20 * MIN, DAY, "chat", ""); // a 30 s peek
        l.focus(20 * MIN + 30_000, DAY, "code", "");
        assert_eq!(l.streak(30 * MIN, MIN, work), 30 * MIN, "the peek is bridged");
        let mut b = Ledger::default();
        b.focus(0, DAY, "code", "");
        b.focus(10 * MIN, DAY, "chat", "");
        b.focus(15 * MIN, DAY, "code", "");
        assert_eq!(b.streak(25 * MIN, MIN, work), 10 * MIN, "five minutes away breaks it");
        let mut c = Ledger::default();
        c.focus(0, DAY, "code", "");
        c.focus(10 * MIN, DAY, "chat", "");
        assert_eq!(c.streak(20 * MIN, MIN, work), 0, "now it is somewhere else");
    }

    #[test]
    fn totals_survive_a_restart_on_the_same_day_only() {
        let mut l = Ledger::default();
        l.restore(DAY, vec![("code".into(), 5 * MIN)]);
        l.focus(0, DAY, "code", "");
        assert_eq!(l.today(MIN, DAY), vec![("code".into(), 6 * MIN)]);
        let mut other = Ledger::default();
        other.focus(1440 * MIN, 1440 * MIN, "x", "");
        other.restore(DAY, vec![("code".into(), 5 * MIN)]);
        assert_eq!(other.today(1441 * MIN, 1440 * MIN), vec![("x".into(), MIN)], "yesterday's totals are not today's");
    }
}
