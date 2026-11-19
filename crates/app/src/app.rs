//! The part of the application that does not depend on the windowing system: it owns the island, the
//! fonts and the channel the services report on, and turns window events into island events.

use crate::services::{self, Msg};
use crate::config::CalendarSource;
use island_ui::model::{Command, Data};
use island_ui::text::Fonts;
use island_ui::{Config, Island};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

/// Wakes the event loop from another thread.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// The configuration as the settings menu shows it.
pub fn config_view_of(fonts: &Fonts) -> island_ui::model::ConfigView {
    let s = crate::config::load();
    island_ui::model::ConfigView {
        font_missing: !s.font.is_empty() && !fonts.preferred_found(),
        font: s.font,
        calendars: s
            .calendars
            .iter()
            .enumerate()
            .map(|(i, c)| island_ui::model::CalendarView {
                name: c.name.clone(),
                detail: c.detail(),
                color: c.color.unwrap_or(crate::config::PALETTE[i % crate::config::PALETTE.len()]),
            })
            .collect(),
        latitude: s.latitude,
        longitude: s.longitude,
        notification_sound: s.notification_sound.unwrap_or_default(),
        filters: s.filters.iter().map(|f| island_ui::model::FilterView { app: f.app.clone(), content: f.content.clone(), show: f.show, sound: f.sound }).collect(),
        timer_presets: s.ui.timer_presets.iter().map(|p| island_core::duration::preset_label(*p)).collect::<Vec<_>>().join(", "),
        screen: s.screen,
    }
}

pub struct App {
    pub island: Island,
    pub fonts: Fonts,
    rx: Receiver<Msg>,
    cmd_tx: Sender<Command>,
    start: Instant,
    epoch_at_start: i64,
    last_tick: Instant,
    /// The previous frame asked for another one at once (an animation is under way).
    was_animating: bool,
    pub quit: bool,
    persist_history: bool,
    /// The focused window covers the whole screen (the island hides under it).
    pub fullscreen: bool,
    /// A maximized window covers the screen the island is on; the island stays out of sight unless something needs
    /// it or the pointer asks for it at the top edge.
    pub maximized: bool,
    pub pointer_near: bool,
    last_pointer: (f64, f64),
}

/// How far the island's motion advances in a frame: the time since the last frame, except for the first frame of an
/// animation that starts from rest. The last frame was drawn long ago then (a second, for the clock), and the
/// springs would leap by their whole step limit in that one frame, a visible jump at the start of every
/// animation; a normal frame's worth is used instead.
/// The start epoch to use: unchanged while the monotonic clock agrees with the wall clock to within 1.5 s (the
/// jitter of reading two clocks), the wall clock's otherwise.
pub fn reanchor(epoch_at_start: i64, elapsed_ms: i64, wall_ms: i64) -> i64 {
    let drift = wall_ms - (epoch_at_start + elapsed_ms);
    if drift.abs() > 1500 { epoch_at_start + drift } else { epoch_at_start }
}

pub fn frame_dt(gap_ms: f64, was_animating: bool) -> f64 {
    const FIRST_FRAME_MS: f64 = 10.0;
    if !gap_ms.is_finite() || gap_ms < 0.0 {
        return FIRST_FRAME_MS;
    }
    if was_animating { gap_ms } else { gap_ms.min(FIRST_FRAME_MS) }
}

impl App {
    /// Fullscreen windows hide the island, unless the option says not to.
    pub fn fullscreen_hides(&self) -> bool {
        self.fullscreen && self.island.cfg.hide_on_fullscreen
    }

    pub fn new(waker: Waker) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let cmd_tx = services::start(tx, waker);
        let wanted = crate::config::load().font;
        let fonts = Fonts::load_system(Some(&wanted));
        if !wanted.is_empty() && !fonts.preferred_found() {
            log::warn!("font {wanted:?} not found: using the default");
        }
        if !fonts.has_text_font() {
            log::warn!("no text font found: the island will show icons only");
        }
        let epoch_at_start = chrono::Utc::now().timestamp_millis();
        let now = Instant::now();
        let mut island = Island::new(Config::default());
        island.restore_announced(services::load_announced());
        island.restore_reminders(services::load_reminders());
        if crate::config::load().persist_history {
            island.restore_history(services::load_history());
        }
        island.data.config = config_view_of(&fonts);
        Self { island, fonts, rx, cmd_tx, start: now, epoch_at_start, last_tick: now, was_animating: false, quit: false, persist_history: crate::config::load().persist_history, fullscreen: false, maximized: false, pointer_near: false, last_pointer: (0.0, 0.0) }
    }

    /// Epoch milliseconds: the monotonic clock since the start, so a clock that is adjusted a little does not make
    /// time jump, kept in step with the wall clock (see `reanchor`).
    pub fn now(&self) -> i64 {
        self.epoch_at_start + self.start.elapsed().as_millis() as i64
    }

    /// The monotonic clock stops while the machine sleeps, so after a suspend the island's time would stay behind the
    /// real one for ever (the clock, the countdowns, the position of the lyrics). Called every frame.
    fn follow_wall_clock(&mut self) {
        let elapsed = self.start.elapsed().as_millis() as i64;
        let wall = chrono::Utc::now().timestamp_millis();
        let fixed = reanchor(self.epoch_at_start, elapsed, wall);
        if fixed != self.epoch_at_start {
            log::info!("the clock moved by {} ms (sleep or a clock change): following it", fixed - self.epoch_at_start);
            self.epoch_at_start = fixed;
        }
    }

    /// Applies everything the services have reported.
    pub fn pump(&mut self) {
        let now = self.now();
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Patch(f) => {
                    let mut data: Data = self.island.data.clone();
                    f(&mut data);
                    self.island.data = data;
                }
                Msg::Input(i) => {
                    log::debug!("input at {now}");
                    self.island.push(now, i)
                }
                Msg::Exec(c) => self.exec(vec![c]),
                Msg::Quit => self.quit = true,
                Msg::Fullscreen(f) => self.fullscreen = f,
                Msg::Cover { maximized, pointer_near } => {
                    self.maximized = maximized;
                    self.pointer_near = pointer_near;
                }
            }
        }
    }

    /// Advances the island; returns the surface size it needs and whether another frame is wanted.
    pub fn tick(&mut self) -> island_ui::Frame {
        self.follow_wall_clock();
        self.pump();
        let dt = frame_dt(self.last_tick.elapsed().as_secs_f64() * 1000.0, self.was_animating);
        self.last_tick = Instant::now();
        let now = self.now();
        let frame = self.island.tick(now, dt);
        self.was_animating = frame.animating;
        let cmds = self.island.take_commands();
        self.exec(cmds);
        {
            use island_core::face::Face;
            use std::sync::atomic::Ordering;
            let f = self.island.current_face();
            services::SYSTEM_WANTED.store(f == Face::Sys, Ordering::Relaxed);
            services::SPECTRUM_WANTED.store(self.island.wants_spectrum(), Ordering::Relaxed);
            let qs = f == Face::Qs;
            if qs && !services::QUICK_WANTED.swap(true, Ordering::Relaxed) {
                let _ = self.cmd_tx.send(island_ui::model::Command::RefreshQuick);
            }
            if !qs {
                services::QUICK_WANTED.store(false, Ordering::Relaxed);
            }
        }
        services::save_reminders_if_changed(self.island.reminders());
        let keys = self.island.announced_keys();
        services::save_announced_if_changed(&keys);
        if self.persist_history {
            services::save_history_if_changed(self.island.history());
        }
        if let Some(dnd) = self.island.dnd_changed() {
            services::save_dnd(dnd);
        }
        frame
    }

    pub fn exec(&mut self, cmds: Vec<Command>) {
        for c in cmds {
            if self.settings_command(&c) {
                continue;
            }
            if matches!(c, Command::PlaySound) && !self.island.cfg.timer_sound {
                continue;
            }
            let _ = self.cmd_tx.send(c);
        }
    }

    fn note(&mut self, text: &str) {
        let now = self.now();
        self.island.push(now, island_ui::model::Input::SettingsNote(text.to_string()));
    }

    /// Changes the list of calendars and writes it back (the old `calendar_urls` are migrated into it).
    fn edit_calendars(&self, change: impl FnOnce(&mut Vec<CalendarSource>)) -> Result<(), String> {
        let mut list = crate::config::load().calendars;
        change(&mut list);
        crate::config::update(|o| crate::config::set_calendars(o, &list))
    }

    /// Handles the commands of the settings menu: writes config.json, applies the change, tells the services.
    fn settings_command(&mut self, c: &Command) -> bool {
        if matches!(c, Command::RefreshCalendars) {
            services::CALENDAR_REFRESH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return true;
        }
        use serde_json::json;
        let result = match c {
            Command::AddCalendar(url) => self.edit_calendars(|list| crate::config::add_feed(list, url)),
            Command::AddICloud { user, password } => self.edit_calendars(|list| crate::config::add_icloud(list, user, password)),
            Command::RemoveCalendar(i) => self.edit_calendars(|list| {
                if *i < list.len() {
                    list.remove(*i);
                }
            }),
            Command::RenameCalendar { index, name } => self.edit_calendars(|list| {
                if let Some(c) = list.get_mut(*index) {
                    c.name = name.trim().to_string();
                }
            }),
            Command::SetCalendarColor { index, color } => self.edit_calendars(|list| {
                if let Some(c) = list.get_mut(*index) {
                    c.color = Some(*color);
                }
            }),
            Command::SetFont(f) => crate::config::update(|o| {
                if f.is_empty() {
                    o.remove("font");
                } else {
                    o.insert("font".into(), json!(f));
                }
            }),
            Command::SetLatitude(v) => crate::config::update(|o| {
                o.insert("latitude".into(), json!(v));
            }),
            Command::SetLongitude(v) => crate::config::update(|o| {
                o.insert("longitude".into(), json!(v));
            }),
            Command::SetOption { key, value } => crate::config::set_option(&mut serde_json::Map::new(), key, value).and_then(|()| {
                crate::config::update(|o| {
                    let _ = crate::config::set_option(o, key, value);
                })
            }),
            Command::AddFilter { app, content, hide } => crate::config::update(|o| crate::config::add_filter(o, app, content, *hide)),
            Command::RemoveFilter(i) => crate::config::update(|o| crate::config::remove_filter(o, *i)),
            Command::SetNotificationSound(f) => crate::config::update(|o| {
                if f.is_empty() {
                    o.remove("notification_sound");
                } else {
                    o.insert("notification_sound".into(), json!(f));
                }
            }),
            _ => return false,
        };
        match result {
            Ok(()) => {
                if let Command::SetFont(f) = c {
                    self.fonts = Fonts::load_system(Some(f));
                    if f.is_empty() {
                        self.note("Font: default");
                    } else if let Some(fam) = self.fonts.family().map(str::to_string) {
                        self.note(&format!("Font: {fam}"));
                    } else {
                        self.note("Font not found: using the default");
                    }
                }
                self.island.data.config = config_view_of(&self.fonts);
                if matches!(c, Command::SetOption { .. }) {
                    self.island.cfg = crate::config::load().ui;
                }
                services::CONFIG_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            Err(e) => {
                log::error!("settings: {e}");
                self.note(&e);
            }
        }
        true
    }

    pub fn pointer_moved(&mut self, x: f64, y: f64) {
        self.last_pointer = (x, y);
        let now = self.now();
        let cmds = self.island.pointer_moved(now, x, y);
        self.exec(cmds);
    }

    /// Feeds an input from the platform layer (a file dropped on the island).
    pub fn input(&mut self, i: island_ui::model::Input) {
        let now = self.now();
        self.island.push(now, i);
        let cmds = self.island.take_commands();
        self.exec(cmds);
    }

    pub fn pointer_left(&mut self) {
        let now = self.now();
        self.island.pointer_left(now);
    }

    pub fn pointer_down_at_last(&mut self) {
        let (x, y) = self.last_pointer;
        self.pointer_down(x, y);
    }

    pub fn pointer_down(&mut self, x: f64, y: f64) {
        let now = self.now();
        let mut cmds = self.island.pointer_down(now, x, y);
        cmds.extend(self.island.take_commands());
        self.exec(cmds);
    }

    /// Feeds a key to the inline reply field.
    pub fn key(&mut self, k: island_ui::model::KeyInput) {
        let now = self.now();
        self.island.push(now, island_ui::model::Input::Key(k));
        let cmds = self.island.take_commands();
        self.exec(cmds);
    }

    pub fn pointer_up(&mut self) {
        let now = self.now();
        self.island.pointer_up(now);
    }

    pub fn right_click(&mut self) {
        let now = self.now();
        self.island.right_click(now);
    }

    pub fn middle_click(&mut self) {
        let now = self.now();
        let cmds = self.island.middle_click(now);
        self.exec(cmds);
    }

    pub fn scroll(&mut self, dx: f64, dy: f64) {
        let now = self.now();
        let cmds = self.island.scroll(now, dx, dy);
        self.exec(cmds);
    }
}

#[cfg(test)]
mod tests {
    use super::{frame_dt, reanchor};

    #[test]
    fn the_clock_follows_the_wall_clock_after_a_sleep_but_not_its_jitter() {
        let start = 1_000_000;
        assert_eq!(reanchor(start, 5000, start + 5400), start, "a few hundred ms of difference is noise");
        // an hour asleep: the monotonic clock did not move, the wall clock did
        assert_eq!(reanchor(start, 5000, start + 5000 + 3_600_000), start + 3_600_000);
        // the wall clock stepped back by ten seconds
        assert_eq!(reanchor(start, 60_000, start + 50_000), start - 10_000);
    }

    #[test]
    fn the_first_frame_of_an_animation_does_not_leap() {
        assert_eq!(frame_dt(1000.0, false), 10.0, "a second of rest is one ordinary frame of motion");
        assert_eq!(frame_dt(4.0, false), 4.0, "a short gap is kept");
        assert_eq!(frame_dt(8.3, true), 8.3, "inside an animation the real time counts");
        assert_eq!(frame_dt(40.0, true), 40.0, "so does a dropped frame, or the motion would lag behind the clock");
        assert_eq!(frame_dt(f64::NAN, true), 10.0);
        assert_eq!(frame_dt(-3.0, false), 10.0);
    }
}
