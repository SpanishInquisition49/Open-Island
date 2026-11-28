//! The part of the application that does not depend on the windowing system: it owns the island, the
//! fonts and the channel the services report on, and turns window events into island events.

use crate::services::{self, Msg};
use crate::config::CalendarSource;
use island_ui::model::{Command, Data};
use island_ui::text::Fonts;
use island_ui::Island;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Wakes the event loop from another thread.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// The configuration as the settings menu shows it.
pub fn config_view_of(fonts: &Fonts) -> island_ui::model::ConfigView {
    let s = crate::config::load();
    let face = face_choice(&s);
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
        map_tile_url: if s.ui.map_tile_url == island_core::map::DEFAULT_TILES { String::new() } else { s.ui.map_tile_url.clone() },
        map_key_set: s.map_key_in_keyring || !s.map_api_key.is_empty(),
        home: crate::config::home_label(&s.home),
        home_pos: s.home.as_ref().map(|h| (h.0, h.1)),
        face,
    }
}

/// How a face answers a password request, as the settings page offers it: only while the island asks for passwords.
fn face_choice(s: &crate::config::Settings) -> Option<island_ui::model::FaceChoice> {
    #[cfg(target_os = "linux")]
    {
        use crate::services::polkit_face;
        s.polkit_agent.then(|| polkit_face::describe(&s.polkit_face, polkit_face::in_stack(), polkit_face::confirms()))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = s;
        None
    }
}

/// The keyring account a calendar signs in with, if it signs in.
fn account_of(c: &CalendarSource) -> Option<String> {
    match &c.kind {
        crate::config::CalendarKind::CalDav { server, user, .. } => Some(services::secrets::caldav_account(server, user)),
        _ => None,
    }
}

/// The island as it starts: with the options saved in the configuration, before what else it remembers from the last run
/// is put back.
fn new_island(settings: &crate::config::Settings) -> Island {
    Island::new(settings.ui.clone())
}

pub struct App {
    attention_saved_at: i64,
    drag_out: Option<Vec<String>>,
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

/// The least time between two frames while nothing moves: news that come closer together than this are drawn by one
/// frame. Just under a frame at 60 Hz, so that a timer of 16 ms is not made to wait.
pub const GATHER: Duration = Duration::from_millis(15);

/// How long until the next frame, zero for now: at once while the shape moves (the display paces those), soon after
/// news (`dirty`), else when the island asked to be drawn again (`due`, counted like `since` from the last frame).
pub fn until_frame(animating: bool, dirty: bool, since: Duration, due: Duration) -> Duration {
    let asked = due.saturating_sub(since);
    if animating {
        Duration::ZERO
    } else if dirty {
        GATHER.saturating_sub(since).min(asked)
    } else {
        asked
    }
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
        // a password that is being asked must be seen, over a fullscreen window too
        self.fullscreen && self.island.cfg.hide_on_fullscreen && !self.island.auth_open()
    }

    pub fn new(waker: Waker) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let cmd_tx = services::start(tx, waker);
        let settings = crate::config::load();
        let wanted = settings.font.clone();
        let fonts = Fonts::load_system(Some(&wanted));
        if !wanted.is_empty() && !fonts.preferred_found() {
            log::warn!("font {wanted:?} not found: using the default");
        }
        if !fonts.has_text_font() {
            log::warn!("no text font found: the island will show icons only");
        }
        let epoch_at_start = chrono::Utc::now().timestamp_millis();
        let now = Instant::now();
        let mut island = new_island(&settings);
        if let Some(until) = services::load_dnd(services::now_ms()).1 {
            island.restore_dnd_until(until);
        }
        island.restore_announced(services::load_announced());
        island.restore_reminders(services::load_reminders());
        island.restore_todos(services::load_todos());
        island.restore_pins(services::load_pins());
        island.restore_bring(services::load_bring());
        island.restore_drain(services::load_drain());
        island.restore_page_usage(services::load_page_usage());
        island.restore_deadlines(services::load_deadlines());
        island.restore_study_log(services::load_study_log());
        island.restore_bar_history(services::load_bar_history());
        if let Some(d) = services::load_recap_day("evening-recap") {
            island.restore_evening_day(d);
        }
        if let Some(d) = services::load_recap_day("morning-recap") {
            island.restore_morning_day(d);
        }
        let (places, waiting) = services::load_places();
        island.restore_places(places, waiting);
        island.restore_shelf(services::load_shelf());
        island.restore_attention_history(services::load_attention_history());
        if let Some((day, apps)) = services::load_attention() {
            island.restore_attention(day, apps);
        }
        if settings.persist_history {
            island.restore_history(services::load_history());
        }
        island.data.config = config_view_of(&fonts);
        Self { drag_out: None, attention_saved_at: 0, island, fonts, rx, cmd_tx, start: now, epoch_at_start, last_tick: now, was_animating: false, quit: false, persist_history: settings.persist_history, fullscreen: false, maximized: false, pointer_near: false, last_pointer: (0.0, 0.0) }
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
                Msg::ConfigSaved(result) => self.config_saved(result),
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
            let f = self.island.current_face();
            services::signal::set(&services::SYSTEM_WANTED, f == Face::Sys);
            services::signal::set(&services::SPECTRUM_WANTED, self.island.wants_spectrum());
            // the quick settings are read as their page opens
            let qs = f == Face::Qs;
            if !services::signal::set(&services::QUICK_WANTED, qs) && qs {
                let _ = self.cmd_tx.send(island_ui::model::Command::RefreshQuick);
            }
        }
        services::save_reminders_if_changed(self.island.reminders());
        services::save_todos_if_changed(self.island.todos());
        services::save_pins_if_changed(self.island.pins());
        let (places, waiting) = self.island.places_saved();
        services::save_places_if_changed(places, waiting);
        services::save_shelf_if_changed(self.island.shelf());
        services::save_bring_if_changed(self.island.bring_list());
        services::save_drain_if_changed(self.island.drain_saved());
        services::save_page_usage_if_changed(&self.island.page_usage());
        services::save_deadlines_if_changed(self.island.deadlines());
        services::save_study_log_if_changed(self.island.study_log());
        services::save_bar_history_if_changed(self.island.bar_history());
        services::save_recap_day_if_changed("evening-recap", self.island.evening_day());
        services::save_recap_day_if_changed("morning-recap", self.island.morning_day());
        // today's attention: written at most once a minute
        let now_s = self.now() / 1000;
        if now_s - self.attention_saved_at >= 60 {
            self.attention_saved_at = now_s;
            let (day, apps) = self.island.attention_export();
            if !apps.is_empty() {
                services::save_attention(day, &apps, self.island.attention_history());
            }
        }
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

    /// The files the island asked to have dragged out, for the window backend that can do it.
    pub fn take_drag(&mut self) -> Option<Vec<String>> {
        self.drag_out.take()
    }

    pub fn exec(&mut self, cmds: Vec<Command>) {
        for c in cmds {
            // a drag is the window's business
            if let Command::DragFiles(paths) = c {
                self.drag_out = Some(paths);
                continue;
            }
            if self.settings_command(&c) || crate::services::cards::on_command(&c) {
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
        crate::config::update_calendars_at(&crate::config::path().ok_or("no configuration directory")?, change)
    }

    /// Removes a calendar, and from the keyring the password no other calendar signs in with.
    fn remove_calendar(&self, index: usize) -> Result<(), String> {
        let mut unused = None;
        self.edit_calendars(|list| {
            if index < list.len() {
                let gone = list.remove(index);
                unused = account_of(&gone).filter(|a| !list.iter().any(|c| account_of(c).as_ref() == Some(a)));
            }
        })?;
        if let Some(account) = unused {
            std::thread::spawn(move || {
                if let Err(e) = services::secrets::delete(&account) {
                    log::warn!("the password of a removed calendar stays in the keyring: {e}");
                }
            });
        }
        Ok(())
    }

    /// What follows a change of the settings file, or the reason it was not made.
    fn config_saved(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.island.data.config = config_view_of(&self.fonts);
                services::signal::bump(&services::CONFIG_GEN);
            }
            Err(e) => {
                log::error!("settings: {e}");
                self.note(&e);
            }
        }
    }

    /// Handles the commands of the settings menu: writes config.json, applies the change, tells the services.
    fn settings_command(&mut self, c: &Command) -> bool {
        if let Command::LoadCalendarMonth { year, month } = c {
            services::net::load_month(*year, *month);
            return true;
        }
        if matches!(c, Command::RefreshCalendars) {
            services::signal::bump(&services::CALENDAR_REFRESH);
            return true;
        }
        // a password or a key goes to the keyring first, off this thread; the answer comes back as `Msg::ConfigSaved`
        if services::secrets::handles(c) {
            return false;
        }
        use serde_json::json;
        let result = match c {
            Command::AddCalendar(input) => match crate::config::calendar_from_input(input) {
                Some(crate::config::CalendarKind::File { path }) if !std::path::Path::new(&crate::config::expand_home(&path)).exists() => Err(format!("No such file or folder: {path}")),
                Some(kind) => self.edit_calendars(|list| crate::config::add_source(list, kind)),
                None => Err("Not a calendar: a feed address, an .ics file, or \"holidays\"".to_string()),
            },
            Command::RemoveCalendar(i) => self.remove_calendar(*i),
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
        if result.is_ok() {
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
            if matches!(c, Command::SetOption { .. }) {
                self.island.cfg = crate::config::load().ui;
            }
        }
        self.config_saved(result);
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

    /// A key of the real keyboard. While a password is asked it goes there and nowhere else (keys that arrive from the
    /// control socket come as inputs, which the island then drops).
    pub fn key(&mut self, k: island_ui::model::KeyInput) {
        let now = self.now();
        if self.island.auth_open() {
            let cmds = self.island.auth_key(now, k);
            self.exec(cmds);
            return;
        }
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
        let cmds = self.island.take_commands();
        self.exec(cmds);
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
    use super::{frame_dt, new_island, reanchor};
    use island_ui::model::OptionValue;
    use island_ui::options;

    /// Writes one option as the options page does, into a file of its own, and returns the island the next start makes.
    fn restarted_after(name: &str, changes: &[(&str, OptionValue)]) -> island_ui::Island {
        let dir = std::env::temp_dir().join(format!("open-island-restart-{}-{name}", std::process::id()));
        let file = dir.join("config.json");
        let _ = std::fs::remove_file(&file);
        for (key, value) in changes {
            crate::config::set_option(&mut serde_json::Map::new(), key, value).unwrap_or_else(|e| panic!("{key}: {e}"));
            crate::config::update_at(&file, |o| crate::config::set_option(o, key, value).unwrap()).unwrap();
        }
        let settings = crate::config::parse(&std::fs::read_to_string(&file).unwrap()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        new_island(&settings)
    }

    #[test]
    fn every_switch_of_the_options_page_is_still_set_after_a_restart() {
        let defaults = island_ui::Config::default();
        for tab in 0..options::TABS.len() {
            for (key, _) in options::toggles(tab) {
                let changed = !options::get_bool(&defaults, key);
                let island = restarted_after(key, &[(key, OptionValue::Bool(changed))]);
                assert_eq!(options::get_bool(&island.cfg, key), changed, "{key} is forgotten");
            }
        }
    }

    #[test]
    fn every_value_a_stepper_can_reach_is_still_set_after_a_restart() {
        for tab in 0..options::TABS.len() {
            for (key, _, _, step, min, max) in options::steppers(tab) {
                let steps = ((max - min) / step).round() as i64;
                for n in 0..=steps {
                    // rounded as a click on the page rounds it
                    let value = ((min + n as f64 * step).min(*max) * 10.0).round() / 10.0;
                    let island = restarted_after(key, &[(key, OptionValue::Num(value))]);
                    assert!((options::get_num(&island.cfg, key) - value).abs() < 1e-6, "{key}: {value} came back as {}", options::get_num(&island.cfg, key));
                }
            }
        }
    }

    #[test]
    fn the_study_hours_are_still_set_after_a_restart() {
        let text = "10:00-13:00+14:00-18:30,09:00-17:00,09:00-17:00,09:00-17:00,09:00-13:00,off 10:00-13:00,off 09:00-17:00";
        let island = restarted_after("study-hours", &[("study_hours", OptionValue::Text(text.into()))]);
        assert_eq!(island_core::plan::week_text(&island.cfg.study_week), text);
    }

    #[test]
    fn the_text_options_and_the_way_of_travelling_are_still_set_after_a_restart() {
        let text = |t: &str| OptionValue::Text(t.into());
        // the presets as the field shows them, and so as it writes them back
        let island = restarted_after("texts", &[("timer_presets", text("2 min, 45 min, 1 h 30 min")), ("travel_mode", text("train")), ("map_tile_url", text("opentopomap")), ("map_dark", OptionValue::Bool(false))]);
        assert_eq!(island.cfg.timer_presets, [120, 2700, 5400]);
        assert_eq!(island.cfg.travel_mode, island_core::travel::Mode::Train);
        assert_eq!(Some(island.cfg.map_tile_url.clone()), island_core::map::resolve("opentopomap"));
        assert!(!island.cfg.map_dark);
    }

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

    #[test]
    fn news_that_come_close_together_wait_for_one_frame() {
        use super::until_frame;
        let ms = std::time::Duration::from_millis;
        assert_eq!(until_frame(true, false, ms(0), ms(1000)), ms(0), "the shape moves");
        assert_eq!(until_frame(false, false, ms(10), ms(1000)), ms(990), "nothing to draw before its time");
        assert_eq!(until_frame(false, true, ms(5), ms(1000)), ms(10), "news just after a frame wait a little");
        assert_eq!(until_frame(false, true, ms(40), ms(1000)), ms(0), "and are drawn at once otherwise");
        assert_eq!(until_frame(false, true, ms(12), ms(8)), ms(0), "never later than the island asked");
        assert_eq!(until_frame(false, false, ms(20), ms(16)), ms(0));
    }
}
