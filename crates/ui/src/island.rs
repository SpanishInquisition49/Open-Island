//! The island's state machine: which face is shown, the shape spring, hover and clicks, timers,
//! notifications, long operations. It samples nothing and draws nothing itself: the platform layer
//! feeds it `Data`, `Input` events and a clock, and the faces module draws it.

use crate::model::*;
use crate::paint::Hit;
use island_core::calendar::{self, Announcer};
use crate::config_check::is_calendar_url;
use island_core::duration;
use island_core::face::{self, Face, NotifFace, Page, Side, Snapshot};
use island_core::lyrics::{self, Line};
use island_core::notify;
use island_core::pomodoro::{self, Phase};
use island_core::progress::{self, Tracker};
use island_core::spring::Spring;
use island_core::weather;

pub const TOP_MARGIN: f64 = 4.0;
pub const BUBBLE_W: f64 = 66.0;
pub const BUBBLE_GAP: f64 = 6.0;
pub const ICON_W: f64 = 19.0;
const REPLY_IDLE_MS: i64 = 20_000;
const KB_IDLE_MS: i64 = 9_000;
const EDIT_IDLE_MS: i64 = 40_000;
const LYRIC_LEAD_MS: f64 = 250.0;
const HOVER_INTENT_MS: i64 = 140;
const HOVER_GRACE_MS: i64 = 320;
const FADE_OUT_MS: f64 = 70.0;
const FADE_IN_MS: f64 = 170.0;
const FADE_IN_AT: f64 = 0.5;

#[derive(Clone, Debug)]
pub struct Config {
    pub notification_ms: i64,
    pub osd_ms: i64,
    pub meeting_lead_ms: i64,
    pub meeting_alerts: bool,
    pub rain_alerts: bool,
    pub battery_alerts: bool,
    pub low_battery: f32,
    pub media_auto_expand: bool,
    pub clock_in_pill: bool,
    pub status_icons: bool,
    pub lyrics: bool,
    pub clipboard_history: bool,
    pub timer_presets: Vec<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            notification_ms: 5000,
            osd_ms: 1600,
            meeting_lead_ms: 10 * 60_000,
            meeting_alerts: true,
            rain_alerts: true,
            battery_alerts: true,
            low_battery: 15.0,
            media_auto_expand: true,
            clock_in_pill: true,
            status_icons: true,
            lyrics: true,
            clipboard_history: true,
            timer_presets: vec![300, 600, 1500],
        }
    }
}

/// What a click on a drawn region does.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Cmd(Command),
    SeekBar,
    VolumeBar,
    BrightnessBar,
    TimerStart(u64),
    PomodoroStart,
    StopwatchStart,
    TimerCancel,
    TimerAddMinute,
    PomodoroSkip,
    Dismiss,
    NextNotif,
    NotifAction(String),
    Open(Page),
    CalPrev,
    CalNext,
    CalDay(u32),
    Join(String),
    ClearHistory,
    ToggleDnd,
    ReplySend,
    ReplyCancel,
    CyclePlayer,
    EditField(Field),
    RemoveCalendar(usize),
    Back,
    CalToday,
    HistoryRemove(usize),
    HistoryOpen(usize),
    ClipRestore(usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimerMode {
    Countdown,
    Pomodoro,
    Stopwatch,
}

#[derive(Clone, Debug)]
pub struct Timer {
    pub mode: TimerMode,
    pub start_ms: i64,
    pub end_ms: Option<i64>,
    pub total_ms: i64,
    pub phase: Phase,
    pub round: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Osd {
    pub kind: OsdKind,
    pub level: f32,
    pub muted: bool,
}

/// An inline reply being typed.
#[derive(Clone, Debug)]
pub struct Reply {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    pub placeholder: String,
    pub text: String,
    /// Caret position in characters.
    pub cursor: usize,
    pub until: i64,
}

/// A setting being edited in the settings menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Calendar,
    Font,
    Latitude,
    Longitude,
    Sound,
}

#[derive(Clone, Debug)]
pub struct Edit {
    pub field: Field,
    pub text: String,
    pub cursor: usize,
    pub until: i64,
}

/// Edits `text` with a text-field key (insert, delete, move). Returns false for any other key.
pub fn edit_text(text: &mut String, cursor: &mut usize, k: &KeyInput) -> bool {
    let chars = text.chars().count();
    *cursor = (*cursor).min(chars);
    let byte = |t: &str, ci: usize| t.char_indices().nth(ci).map_or(t.len(), |(b, _)| b);
    match k {
        KeyInput::Text(t) => {
            let t: String = t.chars().filter(|c| !c.is_control()).collect();
            let at = byte(text, *cursor);
            text.insert_str(at, &t);
            *cursor += t.chars().count();
        }
        KeyInput::Backspace => {
            if *cursor > 0 {
                let (a, b) = (byte(text, *cursor - 1), byte(text, *cursor));
                text.replace_range(a..b, "");
                *cursor -= 1;
            }
        }
        KeyInput::Delete => {
            if *cursor < chars {
                let (a, b) = (byte(text, *cursor), byte(text, *cursor + 1));
                text.replace_range(a..b, "");
            }
        }
        KeyInput::Left => *cursor = cursor.saturating_sub(1),
        KeyInput::Right => *cursor = (*cursor + 1).min(chars),
        KeyInput::Home => *cursor = 0,
        KeyInput::End => *cursor = chars,
        _ => return false,
    }
    true
}

#[derive(Clone, Copy, Debug)]
pub struct Kb {
    pub index: usize,
    pub last_key: i64,
}

pub struct Cal {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

/// Result of a tick: how big the window surface must be, and whether another frame is needed soon.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub surface: (f64, f64),
    pub animating: bool,
}

pub struct Island {
    pub cfg: Config,
    pub data: Data,
    pub now: i64,
    pub(crate) spring: Spring,
    pub(crate) shown: Face,
    pub(crate) shown_side: Option<Side>,
    want: Face,
    want_side: Option<Side>,
    pub(crate) alpha: f64,
    from_size: (f64, f64),
    pub(crate) bubble_k: f64,
    anim_box: Option<(f64, f64, f64, f64)>, // target (w,h) it was sized for, box (w,h)
    settled: bool,
    pub(crate) page: Option<Page>,
    hover_page: Option<Page>,
    expand_until: i64,
    pointer: Option<(f64, f64)>,
    hovering: bool,
    hover_since: i64,
    hover_left: i64,
    pub(crate) hits: Vec<Hit<Action>>,
    pub(crate) pill_origin: (f64, f64),
    dragging: Option<Action>,
    swapped: bool,
    // activities
    pub(crate) timer: Option<Timer>,
    timer_done_until: i64,
    pub(crate) timer_done_text: (String, String),
    pub(crate) notif: Option<Incoming>,
    notif_until: i64,
    pub(crate) reply: Option<Reply>,
    /// Keyboard navigation: the focused control among the drawn ones.
    pub(crate) kb: Option<Kb>,
    pub(crate) edit: Option<Edit>,
    pub(crate) note: Option<(String, i64)>,
    pub(crate) queue: Vec<notify::Notification>,
    waiting: std::collections::VecDeque<Incoming>,
    pub(crate) history: Vec<(Incoming, i64)>,
    pub(crate) osd: Option<Osd>,
    osd_until: i64,
    pub(crate) power: Option<PowerEvent>,
    power_until: i64,
    low_battery_sent: bool,
    last_plugged: Option<bool>,
    pub(crate) meeting: Option<calendar::Event>,
    meeting_until: i64,
    pub(crate) rain_in: Option<i32>,
    rain_until: i64,
    rain_announced: bool,
    pub(crate) ops: Tracker,
    announcer: Announcer,
    last_track: Option<String>,
    track_changed_at: i64,
    lyric_lines: Option<Vec<Line>>,
    lyric_src: Option<String>,
    /// The line being sung (1-based, 0 before the first), when it began, and which way the song moved.
    lyric_idx: usize,
    lyric_at: i64,
    lyric_dir: f64,
    pub(crate) cal: Cal,
    /// The clipboard entry just restored, and until when it shows a check mark.
    /// When the shown face began to appear (drives the stagger of page rows).
    pub(crate) enter_at: i64,
    pub(crate) spectrum: Vec<f32>,
    pub(crate) spectrum_at: i64,
    pub(crate) copied: Option<(usize, i64)>,
    pub(crate) pending: Vec<Command>,
    dnd_seen: bool,
}

impl Island {
    pub fn new(cfg: Config) -> Self {
        let (w, h) = Face::Idle.size();
        Self {
            cfg,
            data: Data::default(),
            now: 0,
            spring: Spring::new(w, h),
            shown: Face::Idle,
            shown_side: None,
            want: Face::Idle,
            want_side: None,
            alpha: 1.0,
            from_size: (w, h),
            bubble_k: 0.0,
            anim_box: None,
            settled: true,
            page: None,
            hover_page: None,
            expand_until: 0,
            pointer: None,
            hovering: false,
            hover_since: 0,
            hover_left: -100_000,
            hits: vec![],
            pill_origin: (0.0, TOP_MARGIN),
            dragging: None,
            swapped: false,
            timer: None,
            timer_done_until: 0,
            timer_done_text: Default::default(),
            notif: None,
            notif_until: 0,
            reply: None,
            kb: None,
            edit: None,
            note: None,
            queue: vec![],
            waiting: Default::default(),
            history: vec![],
            osd: None,
            osd_until: 0,
            power: None,
            power_until: 0,
            low_battery_sent: false,
            last_plugged: None,
            meeting: None,
            meeting_until: 0,
            rain_in: None,
            rain_until: 0,
            rain_announced: false,
            ops: Tracker::default(),
            announcer: Announcer::default(),
            last_track: None,
            track_changed_at: -100_000,
            lyric_lines: None,
            lyric_src: None,
            lyric_idx: 0,
            lyric_at: -100_000,
            lyric_dir: 1.0,
            cal: Cal { year: 1970, month: 1, day: 1 },
            enter_at: -100_000,
            spectrum: vec![],
            spectrum_at: -100_000,
            copied: None,
            pending: vec![],
            dnd_seen: false,
        }
    }

    /// Meeting keys already announced (persist them across restarts).
    pub fn announced_keys(&self) -> Vec<String> {
        self.announcer.keys()
    }

    pub fn restore_announced(&mut self, keys: impl IntoIterator<Item = String>) {
        self.announcer = Announcer::from_keys(keys);
    }

    // ---- inputs -------------------------------------------------------------------------------

    pub fn push(&mut self, now: i64, input: Input) {
        self.now = now;
        match input {
            Input::Notification(n) => self.on_notification(n),
            Input::NotificationClosed(id) => {
                // closed by the sender: no signal back
                if self.notif.as_ref().is_some_and(|n| n.id == id) {
                    self.notif = None;
                    self.notif_until = 0;
                }
                if self.reply.as_ref().is_some_and(|r| r.id == id) {
                    self.reply = None;
                }
                self.waiting.retain(|n| n.id != id);
            }
            Input::Key(k) => self.on_key(k),
            Input::StartReply => self.start_reply(),
            Input::SettingsNote(t) => self.note = Some((t, now + 5000)),
            Input::OpenKeyboard(p) => {
                self.kb = Some(Kb { index: 0, last_key: now });
                self.page = p.or(Some(Page::Home));
                self.hover_page = self.page;
                self.expand_until = now + KB_IDLE_MS;
            }
            Input::SetDnd(on) => {
                self.data.dnd = on;
                self.pending.push(Command::SetDnd(on));
            }
            Input::Spectrum(v) => {
                self.spectrum = v;
                self.spectrum_at = now;
            }
            Input::Osd { kind, level, muted } => {
                self.osd = Some(Osd { kind, level, muted });
                self.osd_until = now + self.cfg.osd_ms;
            }
            Input::Power(p) => {
                self.power = Some(p);
                self.power_until = now + 3500;
            }
            Input::Progress(p) => self.ops.report(now, &p),
            Input::ProgressDone(l) => self.ops.finish(now, progress::State::Done, &l),
            Input::ProgressFail(l) => self.ops.finish(now, progress::State::Fail, &l),
            Input::ProgressClear => self.ops.clear(),
            Input::StartTimer(secs) => self.start_timer(secs),
            Input::StartPomodoro => self.start_pomodoro(),
            Input::StartStopwatch => {
                self.timer = Some(Timer { mode: TimerMode::Stopwatch, start_ms: now, end_ms: None, total_ms: 0, phase: Phase::Focus, round: 1 })
            }
            Input::CancelTimer => self.timer = None,
            Input::Peek => self.expand_until = now + 3000,
            Input::OpenPage(p) => {
                self.activate(Action::Open(p));
                self.expand_until = now + 5000;
            }
        }
    }

    fn on_notification(&mut self, n: Incoming) {
        log::debug!("notification {} from {:?}: dnd={} showing={}", n.id, n.app, self.data.dnd, self.notif.is_some());
        self.history.insert(0, (n.clone(), self.now));
        self.history.truncate(30);
        if self.data.dnd && !n.critical {
            return;
        }
        if self.notif.as_ref().is_some_and(|c| c.id == n.id) {
            self.notif = Some(n); // an update replaces the card on screen
            self.notif_until = self.now + self.cfg.notification_ms;
            return;
        }
        if self.notif.is_none() {
            self.show_notif(n);
        } else {
            let folded = notify::enqueue(
                &mut self.queue,
                notify::Notification { id: n.id, app: n.app.clone(), summary: n.summary.clone(), body: n.body.clone(), critical: n.critical, group_count: 1 },
                8,
            );
            if folded {
                self.waiting.retain(|w| w.app != n.app);
            }
            self.waiting.push_back(n);
        }
    }

    fn show_notif(&mut self, n: Incoming) {
        let default = if n.critical { self.cfg.notification_ms * 2 } else { self.cfg.notification_ms };
        // the sender's timeout wins, within reason: 0 (never) means "until dismissed", capped at 30 s on screen
        let ms = match n.expire_ms {
            Some(0) => 30_000,
            Some(ms) if ms > 0 => ms.clamp(2000, 30_000),
            _ => default,
        };
        self.notif_until = self.now + ms;
        self.notif = Some(n);
    }

    /// Takes the notification off the screen and tells its sender why.
    fn close_notif(&mut self, reason: CloseReason) {
        if let Some(n) = self.notif.take() {
            self.pending.push(Command::NotificationClosed { id: n.id, reason });
        }
        self.notif_until = 0;
    }

    fn start_reply(&mut self) {
        let Some(n) = self.notif.clone() else { return };
        let Some(a) = n.reply_action() else { return };
        self.reply = Some(Reply {
            id: n.id,
            app: n.app.clone(),
            summary: n.summary.clone(),
            body: n.body.clone(),
            placeholder: if a.label.is_empty() { "Reply".into() } else { a.label.clone() },
            text: String::new(),
            cursor: 0,
            until: self.now + REPLY_IDLE_MS,
        });
    }

    fn end_reply(&mut self, send: bool) {
        let Some(r) = self.reply.take() else { return };
        if send && !r.text.trim().is_empty() {
            self.pending.push(Command::NotificationReplied { id: r.id, text: r.text.trim().to_string() });
            self.pending.push(Command::NotificationClosed { id: r.id, reason: CloseReason::Dismissed });
        }
        if self.notif.as_ref().is_some_and(|n| n.id == r.id) {
            self.notif = None;
            self.notif_until = 0;
        }
    }

    fn on_key(&mut self, k: KeyInput) {
        let now = self.now;
        if self.edit.is_some() {
            self.edit_key(k);
            return;
        }
        if self.reply.is_none() {
            self.kb_key(k);
            return;
        }
        let Some(r) = self.reply.as_mut() else { return };
        r.until = now + REPLY_IDLE_MS;
        if edit_text(&mut r.text, &mut r.cursor, &k) {
            return;
        }
        match k {
            KeyInput::Enter => self.end_reply(true),
            KeyInput::Escape => self.end_reply(false),
            _ => {}
        }
    }

    pub fn history(&self) -> &[(Incoming, i64)] {
        &self.history
    }

    pub fn restore_history(&mut self, h: Vec<(Incoming, i64)>) {
        self.history = h;
        self.history.truncate(30);
    }

    /// The new do-not-disturb state when it changed since the last call (to persist it).
    pub fn dnd_changed(&mut self) -> Option<bool> {
        (self.dnd_seen != self.data.dnd).then(|| {
            self.dnd_seen = self.data.dnd;
            self.data.dnd
        })
    }

    /// What the island asks the platform to do, since the last call.
    pub fn take_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.pending)
    }

    /// The controls the keyboard can reach: every drawn hit except the ones that cover the whole face.
    pub(crate) fn nav_hits(&self) -> Vec<usize> {
        let (w, h) = (self.spring.w.pos, self.spring.h.pos);
        self.hits.iter().enumerate().filter(|(_, hit)| !(hit.rect.width() > w * 0.9 && hit.rect.height() > h * 0.5)).map(|(i, _)| i).collect()
    }

    fn start_edit(&mut self, field: Field) {
        let c = &self.data.config;
        let text = match field {
            Field::Calendar => String::new(),
            Field::Font => c.font.clone(),
            Field::Latitude => c.latitude.map(|v| v.to_string()).unwrap_or_default(),
            Field::Longitude => c.longitude.map(|v| v.to_string()).unwrap_or_default(),
            Field::Sound => c.notification_sound.clone(),
        };
        let cursor = text.chars().count();
        self.edit = Some(Edit { field, text, cursor, until: self.now + EDIT_IDLE_MS });
        self.expand_until = self.now + EDIT_IDLE_MS;
    }

    fn edit_key(&mut self, k: KeyInput) {
        let now = self.now;
        let Some(e) = self.edit.as_mut() else { return };
        e.until = now + EDIT_IDLE_MS;
        self.expand_until = now + EDIT_IDLE_MS;
        if edit_text(&mut e.text, &mut e.cursor, &k) {
            return;
        }
        match k {
            KeyInput::Escape => self.edit = None,
            KeyInput::Enter => {
                let Some(e) = self.edit.take() else { return };
                let t = e.text.trim().to_string();
                let note = |s: &str| Some((s.to_string(), now + 5000));
                match e.field {
                    Field::Calendar if is_calendar_url(&t) => self.pending.push(Command::AddCalendar(t)),
                    Field::Calendar => {
                        self.note = note("Paste an https:// or webcal:// calendar address");
                        self.edit = Some(Edit { text: t.clone(), cursor: t.chars().count(), ..e });
                    }
                    Field::Font => self.pending.push(Command::SetFont(t)),
                    Field::Sound => self.pending.push(Command::SetNotificationSound(t)),
                    Field::Latitude | Field::Longitude => {
                        let (range, lat) = if e.field == Field::Latitude { (-90.0..=90.0, true) } else { (-180.0..=180.0, false) };
                        match t.replace(',', ".").parse::<f64>() {
                            Ok(v) if range.contains(&v) => self.pending.push(if lat { Command::SetLatitude(v) } else { Command::SetLongitude(v) }),
                            _ => {
                                self.note = note(if lat { "Latitude is a number from -90 to 90" } else { "Longitude is a number from -180 to 180" });
                                self.edit = Some(Edit { text: t.clone(), cursor: t.chars().count(), ..e });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn kb_key(&mut self, k: KeyInput) {
        let now = self.now;
        let Some(mut kb) = self.kb else { return };
        kb.last_key = now;
        self.expand_until = now + KB_IDLE_MS;
        let nav = self.nav_hits();
        let n = nav.len().max(1);
        match k {
            KeyInput::Right | KeyInput::Down => kb.index = (kb.index + 1) % n,
            KeyInput::Left | KeyInput::Up => kb.index = (kb.index + n - 1) % n,
            KeyInput::Enter => {
                if let Some(action) = nav.get(kb.index).and_then(|i| self.hits.get(*i)).map(|h| h.action.clone()) {
                    match action {
                        Action::SeekBar | Action::VolumeBar | Action::BrightnessBar => {}
                        a => {
                            let cmds = self.activate(a);
                            self.pending.extend(cmds);
                        }
                    }
                }
            }
            KeyInput::Escape => {
                self.kb = None;
                self.expand_until = 0;
                self.page = None;
                self.hover_page = None;
                return;
            }
            _ => {}
        }
        self.kb = Some(kb);
    }

    /// True while the island needs the keyboard (an inline reply is being typed).
    /// True when the equalizer is on screen with music playing: the audio analysis is worth running.
    pub fn wants_spectrum(&self) -> bool {
        let playing = self.data.media.as_ref().is_some_and(|m| m.playing);
        let on_ac = self.data.battery.is_none_or(|b| b.on_ac);
        playing && on_ac && matches!(self.shown, Face::MediaMini | Face::MediaFull | Face::MediaLyric)
    }

    pub fn wants_keyboard(&self) -> bool {
        self.reply.is_some() || self.kb.is_some() || self.edit.is_some()
    }

    fn start_timer(&mut self, secs: u64) {
        let total = secs as i64 * 1000;
        self.timer = Some(Timer { mode: TimerMode::Countdown, start_ms: self.now, end_ms: Some(self.now + total), total_ms: total, phase: Phase::Focus, round: 1 });
    }

    fn start_pomodoro(&mut self) {
        let total = pomodoro::FOCUS as i64 * 1000;
        self.timer = Some(Timer { mode: TimerMode::Pomodoro, start_ms: self.now, end_ms: Some(self.now + total), total_ms: total, phase: Phase::Focus, round: 1 });
    }

    pub fn remaining_ms(&self) -> i64 {
        self.timer.as_ref().and_then(|t| t.end_ms).map_or(0, |e| (e - self.now).max(0))
    }

    fn on_timer_end(&mut self) {
        let Some(t) = self.timer.clone() else { return };
        self.pending.push(Command::PlaySound);
        match t.mode {
            TimerMode::Pomodoro => {
                let (phase, round, secs) = pomodoro::next(t.phase, t.round);
                self.timer_done_text = match t.phase {
                    Phase::Focus => ("Focus done".into(), format!("Break: {}", duration::preset_label(secs))),
                    Phase::Break => ("Break over".into(), format!("Round {round}: back to focus")),
                };
                let total = secs as i64 * 1000;
                self.timer = Some(Timer { phase, round, start_ms: self.now, end_ms: Some(self.now + total), total_ms: total, ..t });
            }
            _ => {
                self.timer_done_text = ("Time's up".into(), duration::preset_label((t.total_ms / 1000) as u64));
                self.timer = None;
            }
        }
        self.timer_done_until = self.now + 6000;
    }

    // ---- pointer ------------------------------------------------------------------------------

    fn local(&self, x: f64, y: f64) -> (f64, f64) {
        (x - self.pill_origin.0, y - self.pill_origin.1)
    }

    fn pill_rect(&self) -> (f64, f64, f64, f64) {
        let (w, h) = (self.spring.w.pos, self.spring.h.pos);
        let side = if self.shown_side.is_some() { ((BUBBLE_W + BUBBLE_GAP) * self.bubble_k).round() } else { 0.0 };
        (self.pill_origin.0, self.pill_origin.1, w + side, h)
    }

    pub fn pointer_moved(&mut self, now: i64, x: f64, y: f64) -> Vec<Command> {
        self.now = now;
        self.pointer = Some((x, y));
        let (px, py, pw, ph) = self.pill_rect();
        let inside = x >= px && x <= px + pw && y >= py && y <= py + ph;
        if inside && !self.hovering {
            self.hovering = true;
            self.hover_since = now;
        } else if !inside && self.hovering {
            self.hovering = false;
            self.hover_left = now;
            self.hover_page = None;
        }
        if let Some(Action::SeekBar | Action::VolumeBar | Action::BrightnessBar) = self.dragging.clone() {
            return self.slider_at(x, y);
        }
        std::mem::take(&mut self.pending)
    }

    pub fn pointer_left(&mut self, now: i64) {
        self.now = now;
        self.pointer = None;
        if self.hovering {
            self.hovering = false;
            self.hover_left = now;
            self.hover_page = None;
        }
        self.dragging = None;
    }

    fn hit_at(&self, x: f64, y: f64) -> Option<&Hit<Action>> {
        let (lx, ly) = self.local(x, y);
        self.hits.iter().rev().find(|h| h.rect.contains(vello::kurbo::Point::new(lx, ly)))
    }

    fn slider_at(&mut self, x: f64, _y: f64) -> Vec<Command> {
        let Some(action) = self.dragging.clone() else { return vec![] };
        let (lx, _) = self.local(x, 0.0);
        let Some(h) = self.hits.iter().rev().find(|h| h.action == action) else { return vec![] };
        let frac = ((lx - h.rect.x0) / h.rect.width()).clamp(0.0, 1.0);
        let mut cmds = vec![];
        match action {
            Action::SeekBar => {
                if let Some(m) = self.data.media.as_ref().filter(|m| m.can_seek && m.length_ms > 0.0) {
                    cmds.push(Command::MediaSeek { position_ms: frac * m.length_ms });
                }
            }
            Action::VolumeBar => {
                self.data.volume = Some(frac as f32);
                cmds.push(Command::SetVolume(frac as f32));
            }
            Action::BrightnessBar => {
                self.data.brightness = Some(frac as f32);
                cmds.push(Command::SetBrightness(frac as f32));
            }
            _ => {}
        }
        cmds
    }

    pub fn pointer_down(&mut self, now: i64, x: f64, y: f64) -> Vec<Command> {
        self.now = now;
        self.pointer = Some((x, y));
        let Some(action) = self.hit_at(x, y).map(|h| h.action.clone()) else {
            return self.click_background(x, y);
        };
        match action {
            Action::SeekBar | Action::VolumeBar | Action::BrightnessBar => {
                self.dragging = Some(action);
                self.slider_at(x, y)
            }
            a => self.activate(a),
        }
    }

    pub fn pointer_up(&mut self, now: i64) {
        self.now = now;
        self.dragging = None;
    }

    fn click_background(&mut self, x: f64, y: f64) -> Vec<Command> {
        let (px, py, pw, ph) = self.pill_rect();
        if !(x >= px && x <= px + pw && y >= py && y <= py + ph) {
            return vec![];
        }
        let now = self.now;
        // a click on the side bubble opens its own activity
        let main_w = self.spring.w.pos;
        if self.shown_side.is_some() && x > px + main_w {
            self.hover_page = self.shown_side.map(side_page);
            self.page = self.hover_page;
            self.expand_until = now + 6000;
            return vec![];
        }
        match self.shown {
            Face::TimerDone => self.timer_done_until = 0,
            Face::Power => self.power_until = 0,
            Face::Notif | Face::NotifAct => self.close_notif(CloseReason::Dismissed),
            Face::Meeting => self.meeting_until = 0,
            Face::Rain => self.rain_until = 0,
            Face::Progress => {
                if self.ops.op.as_ref().is_some_and(|o| o.state != progress::State::Run) {
                    self.ops.clear();
                }
                self.ops.flash_until = 0;
            }
            f if f.is_compact() => self.expand_until = now + 6000,
            _ => {}
        }
        vec![]
    }

    pub fn right_click(&mut self, now: i64) {
        self.now = now;
        // close an open page, or swap the main activity and the bubble
        if self.shown.is_compact() && self.shown_side.is_some() {
            self.swapped = !self.swapped;
        } else {
            self.expand_until = 0;
            self.page = None;
            self.hover_page = None;
        }
    }

    pub fn scroll(&mut self, now: i64, dx: f64, dy: f64) -> Vec<Command> {
        self.now = now;
        if dx.abs() > dy.abs() && dx.abs() > 0.5 {
            // horizontal swipe: next/previous page
            let big = self.shown.size().1 > 40.0;
            let pages = self.pages();
            let dir = if dx > 0.0 { 1 } else { -1 };
            if big {
                let current = self.page.or_else(|| page_of(self.shown));
                self.page = face::cycle_page(&pages, current, dir);
                self.expand_until = now + 4000;
            }
            return vec![];
        }
        // vertical scroll over a media face changes the volume
        if matches!(self.shown, Face::MediaFull | Face::MediaLyric | Face::MediaMini | Face::Osd | Face::Qs) {
            let v = (self.data.volume.unwrap_or(0.5) + (dy as f32) * 0.02).clamp(0.0, 1.0);
            self.data.volume = Some(v);
            return vec![Command::SetVolume(v)];
        }
        vec![]
    }

    fn pages(&self) -> Vec<Page> {
        face::swipe_pages(
            self.data.media.is_some(),
            self.timer.is_some(),
            self.focus_active(),
            self.soon().is_some(),
            self.ops.op.is_some(),
            !self.history.is_empty(),
        )
    }

    fn soon(&self) -> Option<&calendar::Event> {
        if !self.cfg.meeting_alerts {
            return None;
        }
        calendar::soon(&self.data.events, self.now, self.cfg.meeting_lead_ms)
    }

    pub(crate) fn focus_active(&self) -> bool {
        self.data.focus.is_some_and(|f| f.active)
    }

    /// Minutes of focus left, when the end is known.
    pub(crate) fn focus_minutes_left(&self) -> Option<i64> {
        let f = self.data.focus.filter(|f| f.active)?;
        let end = f.end_epoch?;
        Some(((end * 1000 - self.now) as f64 / 60_000.0).ceil().max(0.0) as i64)
    }

    pub(crate) fn next_event(&self) -> Option<&calendar::Event> {
        calendar::next_event(&self.data.events, self.now)
    }

    fn activate(&mut self, a: Action) -> Vec<Command> {
        let now = self.now;
        self.expand_until = self.expand_until.max(now + 4000);
        match a {
            Action::Cmd(c) => {
                // the chips answer at once; the next sample confirms
                match &c {
                    Command::ToggleWifi => self.data.wifi = self.data.wifi.map(|v| !v),
                    Command::ToggleBluetooth => self.data.bluetooth = self.data.bluetooth.map(|v| !v),
                    Command::ToggleKeepAwake => self.data.keep_awake = !self.data.keep_awake,
                    _ => {}
                }
                return vec![c];
            }
            Action::Back => {
                self.page = None;
                self.hover_page = None;
            }
            Action::TimerStart(s) => self.start_timer(s),
            Action::PomodoroStart => self.start_pomodoro(),
            Action::StopwatchStart => self.push(now, Input::StartStopwatch),
            Action::TimerCancel => self.timer = None,
            Action::TimerAddMinute => {
                if let Some(t) = self.timer.as_mut() {
                    if let Some(e) = t.end_ms.as_mut() {
                        *e += 60_000;
                        t.total_ms += 60_000;
                    }
                }
            }
            Action::PomodoroSkip => {
                if let Some(t) = self.timer.as_mut() {
                    t.end_ms = Some(now);
                }
            }
            Action::Dismiss | Action::NextNotif => self.close_notif(CloseReason::Dismissed),
            Action::NotifAction(key) => {
                if key == "inline-reply" {
                    self.start_reply();
                } else if let Some(n) = self.notif.clone() {
                    self.pending.push(Command::InvokeAction { id: n.id, key });
                    if n.resident {
                        self.notif = None;
                        self.notif_until = 0;
                    } else {
                        self.close_notif(CloseReason::Dismissed);
                    }
                }
            }
            Action::ReplySend => self.end_reply(true),
            Action::ReplyCancel => self.end_reply(false),
            Action::CyclePlayer => return vec![Command::MediaCyclePlayer],
            Action::Open(p) => {
                self.page = Some(p);
                self.hover_page = Some(p);
                if p == Page::Cal {
                    let c = crate::clock::civil(now, self.data.utc_offset_secs);
                    self.cal = Cal { year: c.year, month: c.month, day: c.day };
                }
            }
            Action::CalPrev | Action::CalNext => {
                let (y, m) = calendar::add_months(self.cal.year, self.cal.month, if a == Action::CalNext { 1 } else { -1 });
                self.cal = Cal { year: y, month: m, day: 1 };
            }
            Action::CalDay(d) => self.cal.day = d,
            Action::Join(url) => {
                self.meeting_until = 0;
                return vec![Command::OpenUrl(url)];
            }
            Action::ClearHistory => self.history.clear(),
            Action::EditField(f) => self.start_edit(f),
            Action::RemoveCalendar(i) => return vec![Command::RemoveCalendar(i)],
            Action::CalToday => {
                let c = crate::clock::civil(now, self.data.utc_offset_secs);
                self.cal = Cal { year: c.year, month: c.month, day: c.day };
            }
            Action::HistoryRemove(i) => {
                if i < self.history.len() {
                    self.history.remove(i);
                }
            }
            Action::HistoryOpen(_) => {}
            Action::ClipRestore(i) => {
                if let Some(t) = self.data.clipboard.get(i).cloned() {
                    self.copied = Some((i, now + 1500));
                    return vec![Command::RestoreClipboard(t)];
                }
            }
            Action::ToggleDnd => {
                self.data.dnd = !self.data.dnd;
                return vec![Command::SetDnd(self.data.dnd)];
            }
            Action::SeekBar | Action::VolumeBar | Action::BrightnessBar => {}
        }
        vec![]
    }

    // ---- the frame ----------------------------------------------------------------------------

    /// Size of a face, which can depend on the data it shows.
    pub fn face_size(&self, face: Face) -> (f64, f64) {
        let (mut w, mut h) = face.size();
        match face {
            Face::IdleHover => {
                h = 28.0 + 44.0 + 10.0 + 34.0 + 10.0;
                if self.next_event().is_some() {
                    h += 10.0 + 18.0 + 4.0;
                }
            }
            Face::Cal => {
                let weeks = calendar::month_grid(self.cal.year, self.cal.month).len() as f64;
                let n = self.day_events().len().clamp(1, 4) as f64;
                h = 14.0 + 34.0 + 4.0 + 16.0 + weeks * 30.0 + 10.0 + n * 24.0 + 6.0;
            }
            Face::Notif if self.notif.as_ref().is_some_and(|n| n.progress.is_some()) => h += 14.0,
            Face::Notifs => h = 14.0 + 34.0 + 6.0 + self.history.len().clamp(1, 5) as f64 * 46.0 + if self.history.is_empty() { 20.0 } else { 8.0 },
            Face::Clip => h = 14.0 + 34.0 + 4.0 + self.data.clipboard.len().clamp(1, 5) as f64 * 32.0 + 10.0,
            Face::FocusMini => w += self.icon_count(true) as f64 * ICON_W,
            Face::Settings => h = 14.0 + 34.0 + self.settings_content_height() + 14.0,
            Face::Qs => h = 14.0 + 34.0 + 8.0 + 36.0 + 8.0 + 36.0 + 8.0 + 28.0 + 8.0 + 28.0 + 8.0 + 32.0 + 14.0 + if self.data.focus.is_some() { 44.0 } else { 0.0 },
            Face::Sys => h = 14.0 + 34.0 + self.sys_rows() as f64 * 28.0 + 14.0,
            Face::Idle => w += self.icon_count(false) as f64 * ICON_W,
            Face::MediaMini | Face::TimerMini | Face::MeetMini => w += self.icon_count(true) as f64 * ICON_W,
            _ => {}
        }
        (w, h)
    }

    /// Rows of the system page that have data.
    pub(crate) fn sys_rows(&self) -> usize {
        let Some(st) = &self.data.system else { return 1 };
        let mut n = 2; // cpu, cores
        n += st.memory.is_some() as usize + st.gpu.is_some() as usize + self.data.battery.is_some() as usize + st.disk.is_some() as usize;
        n + 1 // network
    }

    /// Height of the settings page below its header: four sections and their rows.
    pub(crate) fn settings_content_height(&self) -> f64 {
        let cals = self.data.config.calendars.len().min(4) as f64;
        // section titles (4 x 22), calendar rows, the add row, latitude, longitude, font, sound, the note line
        4.0 * 22.0 + (cals + 1.0 + 2.0 + 1.0 + 1.0) * 34.0 + 20.0
    }

    pub(crate) fn day_events(&self) -> Vec<&calendar::Event> {
        let c = &self.cal;
        let off = self.data.utc_offset_secs;
        let start = crate::clock::midnight_ms(c.year, c.month, c.day, off);
        let end = crate::clock::midnight_ms(c.year, c.month, c.day + 1, off);
        calendar::events_between(&self.data.events, start, end)
    }

    pub(crate) fn icon_count(&self, compact_busy: bool) -> usize {
        if !self.cfg.status_icons {
            return 0;
        }
        let p = self.data.privacy;
        let _ = compact_busy;
        p.mic as usize + p.camera as usize + p.screen as usize
    }

    fn sync_lyrics(&mut self) {
        if !self.cfg.lyrics {
            self.lyric_lines = None;
            return;
        }
        if self.lyric_src != self.data.lyrics_lrc {
            self.lyric_src = self.data.lyrics_lrc.clone();
            self.lyric_lines = self.lyric_src.as_deref().and_then(lyrics::parse_lrc);
            self.lyric_idx = self.lyric_lines.as_ref().map_or(0, |l| lyrics::index_at(l, self.media_position(), LYRIC_LEAD_MS));
            self.lyric_at = -100_000;
        }
        if let Some(lines) = &self.lyric_lines {
            let idx = lyrics::index_at(lines, self.media_position(), LYRIC_LEAD_MS);
            if idx != self.lyric_idx {
                self.lyric_dir = if idx > self.lyric_idx { 1.0 } else { -1.0 };
                self.lyric_idx = idx;
                self.lyric_at = self.now;
            }
        }
    }

    pub(crate) fn media_position(&self) -> f64 {
        match &self.data.media {
            Some(m) if m.playing => (m.position_ms + (self.now - m.sampled_at_ms).max(0) as f64).min(m.length_ms.max(m.position_ms)),
            Some(m) => m.position_ms,
            None => 0.0,
        }
    }

    /// The lyric lines to draw with their slot (rows from the current one, fractional while scrolling), when
    /// the track has synced lyrics.
    pub(crate) fn lyric_strip(&self) -> Option<Vec<(String, f32)>> {
        let lines = self.lyric_lines.as_ref()?;
        let progress = (self.now - self.lyric_at) as f64 / lyrics::SCROLL_MS;
        let text = |n: i64| if n >= 1 && (n as usize) <= lines.len() { lines[n as usize - 1].text.clone() } else { String::new() };
        Some(lyrics::strip(self.lyric_idx, progress, self.lyric_dir).into_iter().map(|(n, slot)| (text(n), slot)).collect())
    }

    /// The strip is still scrolling: frames are needed.
    fn lyrics_scrolling(&self) -> bool {
        self.lyric_lines.is_some() && ((self.now - self.lyric_at) as f64) < lyrics::SCROLL_MS && matches!(self.shown, Face::MediaLyric)
    }

    fn snapshot(&mut self) -> (Snapshot, bool) {
        let now = self.now;
        let hover_open = (self.hovering && now - self.hover_since >= HOVER_INTENT_MS) || now < self.hover_left + HOVER_GRACE_MS;
        let big = hover_open || now < self.expand_until;
        if !big {
            self.page = None;
            if !self.hovering {
                self.hover_page = None;
            }
        }
        if big && self.page.is_none() {
            self.page = self.hover_page;
        }
        let media = self.data.media.as_ref().is_some_and(|m| !m.title.is_empty());
        let notif = self.notif.as_ref().map(|n| if n.actions.is_empty() { NotifFace::Plain } else { NotifFace::Actions });
        let snap = Snapshot {
            replying: self.reply.is_some(),
            osd: self.osd.is_some() && now < self.osd_until,
            notif,
            timer_done: now < self.timer_done_until,
            power: self.power.is_some() && now < self.power_until,
            op: self.ops.op.is_some(),
            op_flash: self.ops.flashing(now),
            meeting: self.meeting.is_some() && now < self.meeting_until,
            rain: self.rain_in.is_some() && now < self.rain_until,
            update: false,
            big,
            page: self.page,
            clipboard: self.cfg.clipboard_history,
            media,
            lyrics: media && self.lyric_lines.is_some(),
            timer: self.timer.is_some(),
            focus: self.focus_active(),
            soon: self.soon().is_some(),
        };
        (snap, big)
    }

    fn slow_checks(&mut self) {
        let now = self.now;
        // meeting heads-up: once per event
        if self.cfg.meeting_alerts {
            if let Some(e) = self.announcer.new_heads_up(&self.data.events, now, self.cfg.meeting_lead_ms).cloned() {
                self.meeting = Some(e);
                self.meeting_until = now + 10_000;
            }
        }
        // rain
        if self.cfg.rain_alerts {
            let soon = self.data.weather.as_ref().and_then(|w| weather::rain_soon(&w.rain_slots, 60));
            match soon {
                Some(m) if !self.rain_announced => {
                    self.rain_in = Some(m);
                    self.rain_until = now + 8000;
                    self.rain_announced = true;
                }
                None => self.rain_announced = false,
                _ => {}
            }
        }
        // power
        if let Some(b) = self.data.battery {
            if let Some(prev) = self.last_plugged {
                if prev != b.on_ac && self.cfg.battery_alerts {
                    self.push(now, Input::Power(if b.on_ac { PowerEvent::Plugged } else { PowerEvent::Unplugged }));
                }
            }
            self.last_plugged = Some(b.on_ac);
            if !b.on_ac && b.percent <= self.cfg.low_battery {
                if !self.low_battery_sent && self.cfg.battery_alerts {
                    self.low_battery_sent = true;
                    self.push(now, Input::Power(PowerEvent::Low));
                }
            } else if b.percent > self.cfg.low_battery + 3.0 || b.on_ac {
                self.low_battery_sent = false;
            }
        }
    }

    /// Advances the island to `now` (epoch ms) and says how big the surface has to be.
    pub fn tick(&mut self, now: i64, dt_ms: f64) -> Frame {
        self.now = now;
        self.ops.expire(now);
        self.slow_checks();
        self.sync_lyrics();
        // notifications: expiry and the next waiting one
        if self.edit.as_ref().is_some_and(|e| now > e.until) || (self.edit.is_some() && self.page != Some(Page::Settings)) {
            self.edit = None;
        }
        if self.note.as_ref().is_some_and(|(_, until)| now > *until) {
            self.note = None;
        }
        if self.kb.is_some_and(|k| now - k.last_key > KB_IDLE_MS) {
            self.kb = None;
        }
        if self.reply.as_ref().is_some_and(|r| now > r.until) {
            self.end_reply(false);
        }
        if self.notif.is_some() && self.reply.is_none() && now >= self.notif_until {
            self.close_notif(CloseReason::Expired);
            if let Some(n) = self.waiting.pop_front() {
                self.queue.retain(|q| q.id != n.id);
                self.show_notif(n);
            }
        }
        if self.notif.is_none() && self.waiting.is_empty() {
            self.queue.clear();
        }
        if self.timer.as_ref().and_then(|t| t.end_ms).is_some_and(|e| now >= e) {
            self.on_timer_end();
        }
        // track change: peek the player
        let track = self.data.media.as_ref().map(|m| m.track_id.clone());
        if track != self.last_track {
            if self.last_track.is_some() && track.is_some() && self.cfg.media_auto_expand {
                self.expand_until = now + 3500;
            }
            if self.last_track.is_some() {
                self.track_changed_at = now;
            }
            self.last_track = track;
        }

        let (snap, _big) = self.snapshot();
        let (mut want, mut side) = face::choose(&snap);
        if side.is_none() {
            self.swapped = false;
        } else if self.swapped {
            let kind = match want {
                Face::MediaMini => Some(Side::Media),
                Face::TimerMini => Some(Side::Timer),
                _ => None,
            };
            match side {
                Some(Side::Timer) => want = Face::TimerMini,
                Some(Side::Meeting) => want = Face::MeetMini,
                _ => {}
            }
            side = kind;
        }
        if want != self.want {
            log::debug!("face {:?} -> {:?} (shown {:?}, notif {})", self.want, want, self.shown, self.notif.is_some());
        }
        self.want = want;
        self.want_side = side;

        // content crossfade: out fast, in once the shape is about halfway to its target
        let dt = if dt_ms.is_finite() { dt_ms.clamp(0.0, 1000.0) } else { 0.0 };
        let (mut tw, mut th) = self.face_size(self.want);
        if self.want != self.shown {
            self.alpha = (self.alpha - dt / FADE_OUT_MS).max(0.0);
            if self.alpha <= 0.0 {
                self.shown = self.want;
                self.shown_side = self.want_side;
                self.from_size = (self.spring.w.pos, self.spring.h.pos);
            }
        } else {
            if let Some(s) = self.want_side {
                self.shown_side = Some(s);
            }
            let span = (tw - self.from_size.0).abs().max((th - self.from_size.1).abs());
            let done = if span < 1.0 {
                1.0
            } else {
                1.0 - (tw - self.spring.w.pos).abs().max((th - self.spring.h.pos).abs()) / span
            };
            if done >= FADE_IN_AT {
                if self.alpha <= 0.0 && dt > 0.0 {
                    self.enter_at = now;
                }
                self.alpha = (self.alpha + dt / FADE_IN_MS).min(1.0);
            }
        }
        // while the old content fades out the shape holds still
        if self.want != self.shown && self.alpha > 0.3 {
            (tw, th) = self.face_size(self.shown);
        }
        let at_rest = self.spring.step(tw, th, dt);
        // bubble split follows the width spring
        if self.shown_side.is_some() && self.want == self.shown {
            let base = self.face_size(self.shown).0;
            self.bubble_k = ((self.spring.w.pos - base) / (BUBBLE_W + BUBBLE_GAP)).clamp(0.0, 1.0);
            // the bubble keeps its slot while an activity is shown; the width spring is not what drives it here
            self.bubble_k = if self.want_side.is_some() { 1.0 } else { 0.0 };
            if self.want_side.is_none() {
                self.shown_side = None;
            }
        } else if self.shown_side.is_none() {
            self.bubble_k = 0.0;
        }
        self.settled = at_rest && self.alpha >= 1.0 && self.want == self.shown && self.want_side == self.shown_side;
        let animating = !self.settled
            || self.hovering
            || self.shown == Face::Osd
            || self.data.media.as_ref().is_some_and(|m| m.playing)
            || self.timer.is_some()
            || self.lyrics_scrolling()
            || self.ops.op.is_some_and_running();
        // the surface: holds both shapes plus room for the overshoot while animating
        let group_w = self.spring.w.pos + if self.shown_side.is_some() { BUBBLE_W + BUBBLE_GAP } else { 0.0 };
        let (sw, sh) = if self.settled {
            self.anim_box = None;
            (group_w.ceil(), self.spring.h.pos.ceil())
        } else {
            let target_w = tw + if self.want_side.is_some() { BUBBLE_W + BUBBLE_GAP } else { 0.0 };
            let bx = match self.anim_box {
                Some((aw, ah, bw, bh)) if aw == target_w && ah == th => (bw, bh),
                _ => (group_w.max(target_w).ceil() + 24.0, self.spring.h.pos.max(th).ceil() + 16.0),
            };
            let bx = (bx.0.max(group_w.ceil() + 2.0), bx.1.max(self.spring.h.pos.ceil() + 2.0));
            self.anim_box = Some((target_w, th, bx.0, bx.1));
            bx
        };
        Frame { surface: (sw.max(40.0) + 8.0, sh.max(20.0) + TOP_MARGIN + 4.0), animating }
    }

    /// Content opacity for the crossfade (eased).
    pub(crate) fn content_opacity(&self) -> f32 {
        let a = self.alpha.clamp(0.0, 1.0);
        let op = if self.want != self.shown { a } else { 1.0 - (1.0 - a).powi(3) };
        op as f32
    }

    pub(crate) fn pointer_pos(&self) -> Option<(f64, f64)> {
        self.pointer
    }

    /// The part of the surface that should take pointer input (the pill and its bubble), in surface
    /// coordinates: (x, y, width, height). The rest of the surface is transparent and must click through.
    pub fn input_rect(&self) -> (f64, f64, f64, f64) {
        self.pill_rect()
    }

    pub fn current_face(&self) -> Face {
        self.shown
    }
}

trait OpRunning {
    fn is_some_and_running(&self) -> bool;
}

impl OpRunning for Option<progress::Op> {
    fn is_some_and_running(&self) -> bool {
        self.as_ref().is_some_and(|o| o.state == progress::State::Run)
    }
}

fn side_page(s: Side) -> Page {
    match s {
        Side::Timer => Page::Timer,
        Side::Meeting => Page::Meeting,
        Side::Focus => Page::Focus,
        Side::Media => Page::Media,
        Side::Op => Page::Op,
    }
}

fn page_of(f: Face) -> Option<Page> {
    Some(match f {
        Face::MediaFull | Face::MediaLyric => Page::Media,
        Face::TimerFull => Page::Timer,
        Face::Meeting => Page::Meeting,
        Face::Progress => Page::Op,
        Face::IdleHover => Page::Home,
        Face::Cal => Page::Cal,
        Face::Qs => Page::Qs,
        Face::Sys => Page::Sys,
        Face::Notifs => Page::Notifs,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Fonts;
    use island_core::calendar::Event;
    use vello::Scene;
    use island_core::pomodoro::Phase;

    const T0: i64 = 1_800_000_000_000;

    fn fonts() -> Fonts {
        Fonts::from_db(&fontdb::Database::new())
    }

    fn run(i: &mut Island, ms: i64) {
        let mut t = i.now.max(T0);
        let end = t + ms;
        while t < end {
            t += 8;
            i.tick(t, 8.0);
        }
    }

    fn data() -> Data {
        let mut d = Data::default();
        d.battery = Some(Battery { percent: 50.0, charging: false, on_ac: true });
        d.media = Some(Media { track_id: "a".into(), title: "Song".into(), artist: "Artist".into(), playing: true, length_ms: 200_000.0, sampled_at_ms: T0, can_seek: true, ..Default::default() });
        d
    }

    #[test]
    fn every_face_draws_without_panicking_and_stays_finite() {
        let f = fonts();
        for page in [Page::Home, Page::Cal, Page::Qs, Page::Sys, Page::Notifs, Page::Media, Page::Timer, Page::Op, Page::Meeting] {
            let mut i = Island::new(Config::default());
            i.data = data();
            i.data.events = vec![Event { id: "e".into(), title: "Meeting".into(), start_ms: T0 + 5 * 60_000, end_ms: T0 + 3_600_000, url: Some("https://x".into()), ..Default::default() }];
            i.push(T0, Input::StartTimer(60));
            i.push(T0, Input::Progress("10 x".into()));
            i.push(T0, Input::Notification(Incoming { id: 1, app: "a".into(), summary: "s".into(), body: "b".into(), ..Default::default() }));
            i.push(T0, Input::OpenPage(page));
            for _ in 0..300 {
                let t = i.now + 8;
                let fr = i.tick(t, 8.0);
                assert!(fr.surface.0.is_finite() && fr.surface.1.is_finite() && fr.surface.0 < 2000.0 && fr.surface.1 < 1500.0, "{page:?}: {fr:?}");
                i.push(t, Input::OpenPage(page));
                let mut scene = Scene::new();
                i.draw(&mut scene, &f, fr.surface, 1.75);
            }
        }
    }

    #[test]
    fn survives_absurd_time_steps() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::OpenPage(Page::Cal));
        for dt in [-1e9, f64::NAN, f64::INFINITY, 1e12, 0.0] {
            let fr = i.tick(i.now + 8, dt);
            assert!(fr.surface.0.is_finite() && fr.surface.1.is_finite(), "dt {dt}");
        }
        run(&mut i, 3000);
        let fr = i.tick(i.now + 8, 8.0);
        assert!(fr.surface.1 > 100.0, "the calendar opened: {fr:?}");
    }

    #[test]
    fn a_timer_counts_down_ends_and_announces() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::StartTimer(1));
        run(&mut i, 500);
        assert!(i.timer.is_some());
        run(&mut i, 1500);
        assert!(i.timer.is_none(), "a one-second timer is over");
        assert!(i.pending.contains(&Command::PlaySound));
        run(&mut i, 100);
        assert_eq!(i.current_face(), Face::TimerDone);
        run(&mut i, 7000);
        assert_eq!(i.current_face(), Face::Idle, "the card goes away on its own");
    }

    #[test]
    fn pomodoro_goes_on_to_the_break() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::StartPomodoro);
        i.activate(Action::PomodoroSkip);
        run(&mut i, 100);
        let t = i.timer.as_ref().expect("the break timer");
        assert_eq!(t.phase, Phase::Break);
        assert_eq!(t.total_ms, 5 * 60_000);
    }

    #[test]
    fn progress_flashes_once_then_becomes_a_bubble() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::Progress("20 Backup".into()));
        run(&mut i, 600);
        assert_eq!(i.current_face(), Face::Progress);
        run(&mut i, 4000);
        assert_eq!(i.current_face(), Face::Idle);
        assert_eq!(i.shown_side, Some(Side::Op));
        i.push(i.now, Input::ProgressDone("done".into()));
        run(&mut i, 600);
        assert_eq!(i.current_face(), Face::Progress);
        run(&mut i, 6000);
        assert!(i.ops.op.is_none());
    }

    #[test]
    fn notifications_queue_and_do_not_disturb() {
        let mut i = Island::new(Config::default());
        let n = |id: u32, app: &str, crit: bool| Input::Notification(Incoming { id, app: app.into(), summary: format!("n{id}"), critical: crit, ..Default::default() });
        i.push(T0, n(1, "a", false));
        i.push(T0, n(2, "b", false));
        assert_eq!(i.queue.len(), 1);
        run(&mut i, 600);
        assert_eq!(i.current_face(), Face::Notif);
        assert_eq!(i.notif.as_ref().unwrap().id, 1);
        run(&mut i, 5500);
        assert_eq!(i.notif.as_ref().map(|n| n.id), Some(2), "the next one is shown");
        i.data.dnd = true;
        run(&mut i, 6000);
        i.push(i.now, n(3, "c", false));
        assert!(i.notif.is_none(), "do not disturb hides it");
        i.push(i.now, n(4, "d", true));
        assert_eq!(i.notif.as_ref().map(|n| n.id), Some(4), "but not a critical one");
        assert_eq!(i.history.len(), 4, "everything lands in the history");
    }

    #[test]
    fn the_meeting_heads_up_fires_once_even_across_a_restart() {
        let ev = Event { id: "m".into(), title: "Class".into(), start_ms: T0 + 8 * 60_000, end_ms: T0 + 3_600_000, ..Default::default() };
        let mut i = Island::new(Config::default());
        i.data.events = vec![ev.clone()];
        i.tick(T0, 8.0);
        assert!(i.meeting.is_some() && i.meeting_until > T0);
        i.meeting_until = 0;
        run(&mut i, 3000);
        assert!(i.meeting_until == 0, "not announced twice");
        let mut j = Island::new(Config::default());
        j.restore_announced(i.announced_keys());
        j.data.events = vec![ev];
        j.tick(T0 + 100, 8.0);
        assert!(j.meeting.is_none(), "a restart does not announce it again");
    }

    #[test]
    fn low_battery_and_plugging_raise_one_event_each() {
        let mut i = Island::new(Config::default());
        i.data.battery = Some(Battery { percent: 80.0, charging: false, on_ac: true });
        i.tick(T0, 8.0);
        i.data.battery = Some(Battery { percent: 80.0, charging: false, on_ac: false });
        i.tick(T0 + 8, 8.0);
        assert_eq!(i.power, Some(PowerEvent::Unplugged));
        i.power_until = 0;
        i.data.battery = Some(Battery { percent: 10.0, charging: false, on_ac: false });
        i.tick(T0 + 16, 8.0);
        assert_eq!(i.power, Some(PowerEvent::Low));
        i.power = None;
        i.tick(T0 + 24, 8.0);
        assert_eq!(i.power, None, "low battery is announced once");
    }

    #[test]
    fn clicks_hit_the_buttons_that_were_drawn() {
        let f = fonts();
        let mut i = Island::new(Config::default());
        i.push(T0, Input::OpenPage(Page::Home));
        let mut surface = (0.0, 0.0);
        for _ in 0..200 {
            let fr = i.tick(i.now + 8, 8.0);
            i.push(i.now, Input::OpenPage(Page::Home));
            surface = fr.surface;
            let mut scene = Scene::new();
            i.draw(&mut scene, &f, fr.surface, 1.0);
        }
        let hit = i.hits.iter().find(|h| h.action == Action::TimerStart(300)).expect("the 5 min preset is drawn");
        let (x, y) = (hit.rect.center().x + i.pill_origin.0, hit.rect.center().y + i.pill_origin.1);
        assert!(x < surface.0 && y < surface.1);
        i.pointer_down(i.now, x, y);
        assert!(i.timer.is_some(), "clicking the preset starts the timer");
    }

    fn incoming(id: u32) -> Incoming {
        Incoming { id, app: "chat".into(), summary: "Anna".into(), body: "Are you coming?".into(), ..Default::default() }
    }

    fn closes(i: &mut Island) -> Vec<(u32, CloseReason)> {
        i.take_commands()
            .into_iter()
            .filter_map(|c| if let Command::NotificationClosed { id, reason } = c { Some((id, reason)) } else { None })
            .collect()
    }

    #[test]
    fn an_expired_notification_tells_its_sender() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::Notification(incoming(7)));
        run(&mut i, 6500);
        assert_eq!(closes(&mut i), vec![(7, CloseReason::Expired)]);
        assert!(i.notif.is_none());
    }

    #[test]
    fn a_dismissed_notification_says_dismissed_and_a_sender_close_says_nothing() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::Notification(incoming(1)));
        i.activate(Action::Dismiss);
        assert_eq!(closes(&mut i), vec![(1, CloseReason::Dismissed)]);
        i.push(i.now, Input::Notification(incoming(2)));
        i.push(i.now, Input::NotificationClosed(2));
        assert!(closes(&mut i).is_empty(), "closed by the sender: no signal back");
        assert!(i.notif.is_none());
    }

    #[test]
    fn the_senders_timeout_is_honoured() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::Notification(Incoming { expire_ms: Some(12_000), ..incoming(1) }));
        run(&mut i, 9000);
        assert!(i.notif.is_some(), "stays for the 12 s it asked for");
        run(&mut i, 4000);
        assert!(i.notif.is_none());
        i.push(i.now, Input::Notification(Incoming { expire_ms: Some(0), ..incoming(2) }));
        run(&mut i, 20_000);
        assert!(i.notif.is_some(), "a persistent one waits for the user (up to 30 s on screen)");
        run(&mut i, 12_000);
        assert!(i.notif.is_none());
    }

    #[test]
    fn an_action_invokes_it_and_closes_unless_resident() {
        let mut i = Island::new(Config::default());
        let act = vec![NotifAction { key: "open".into(), label: "Open".into() }];
        i.push(T0, Input::Notification(Incoming { actions: act.clone(), ..incoming(1) }));
        i.activate(Action::NotifAction("open".into()));
        let cmds = i.take_commands();
        assert_eq!(cmds[0], Command::InvokeAction { id: 1, key: "open".into() });
        assert_eq!(cmds[1], Command::NotificationClosed { id: 1, reason: CloseReason::Dismissed });
        i.push(i.now, Input::Notification(Incoming { actions: act, resident: true, ..incoming(2) }));
        i.activate(Action::NotifAction("open".into()));
        let cmds = i.take_commands();
        assert_eq!(cmds.len(), 1, "a resident notification is not closed by its action");
    }

    #[test]
    fn inline_reply_types_edits_and_sends() {
        let mut i = Island::new(Config::default());
        let reply = vec![NotifAction { key: "inline-reply".into(), label: "Write back".into() }];
        i.push(T0, Input::Notification(Incoming { actions: reply, ..incoming(5) }));
        i.activate(Action::NotifAction("inline-reply".into()));
        assert!(i.wants_keyboard());
        assert_eq!(i.reply.as_ref().unwrap().placeholder, "Write back");
        let key = |i: &mut Island, k: KeyInput| i.push(i.now, Input::Key(k));
        key(&mut i, KeyInput::Text("Hello wörld".into()));
        for _ in 0..5 {
            key(&mut i, KeyInput::Left);
        }
        key(&mut i, KeyInput::Backspace); // removes the space
        key(&mut i, KeyInput::Text(", ".into()));
        key(&mut i, KeyInput::End);
        key(&mut i, KeyInput::Text("!\n\u{7}".into())); // control characters are dropped
        assert_eq!(i.reply.as_ref().unwrap().text, "Hello, wörld!");
        key(&mut i, KeyInput::Home);
        key(&mut i, KeyInput::Delete);
        assert_eq!(i.reply.as_ref().unwrap().text, "ello, wörld!");
        key(&mut i, KeyInput::Enter);
        assert!(!i.wants_keyboard());
        let cmds = i.take_commands();
        assert_eq!(cmds[0], Command::NotificationReplied { id: 5, text: "ello, wörld!".into() });
        assert_eq!(cmds[1], Command::NotificationClosed { id: 5, reason: CloseReason::Dismissed });
        assert!(i.notif.is_none());
    }

    #[test]
    fn escape_and_idle_cancel_a_reply_without_sending() {
        let mut i = Island::new(Config::default());
        let reply = vec![NotifAction { key: "inline-reply".into(), label: String::new() }];
        i.push(T0, Input::Notification(Incoming { actions: reply.clone(), ..incoming(1) }));
        i.activate(Action::NotifAction("inline-reply".into()));
        i.push(i.now, Input::Key(KeyInput::Text("x".into())));
        i.push(i.now, Input::Key(KeyInput::Escape));
        assert!(i.take_commands().iter().all(|c| !matches!(c, Command::NotificationReplied { .. })));
        i.push(i.now, Input::Notification(Incoming { actions: reply, ..incoming(2) }));
        i.activate(Action::NotifAction("inline-reply".into()));
        assert_eq!(i.reply.as_ref().unwrap().placeholder, "Reply", "a default placeholder");
        run(&mut i, 21_000);
        assert!(i.reply.is_none(), "an abandoned reply times out");
        assert!(i.take_commands().iter().all(|c| !matches!(c, Command::NotificationReplied { .. })));
    }

    #[test]
    fn every_new_face_draws() {
        let f = fonts();
        let mut i = Island::new(Config::default());
        let reply = vec![NotifAction { key: "inline-reply".into(), label: "Reply".into() }];
        i.push(T0, Input::Notification(Incoming { actions: reply, progress: Some(40), ..incoming(1) }));
        i.activate(Action::NotifAction("inline-reply".into()));
        i.push(i.now, Input::Key(KeyInput::Text("hi".into())));
        i.data = data();
        i.data.media.as_mut().unwrap().players = 2;
        for _ in 0..200 {
            let fr = i.tick(i.now + 8, 8.0);
            let mut scene = Scene::new();
            i.draw(&mut scene, &f, fr.surface, 2.0);
        }
        assert_eq!(i.current_face(), Face::Reply);
    }

    #[test]
    fn focus_shows_its_minutes_and_a_bubble_beside_music() {
        let mut i = Island::new(Config::default());
        i.data.focus = Some(Focus { active: true, end_epoch: Some(T0 / 1000 + 23 * 60 - 5) });
        run(&mut i, 600);
        assert_eq!(i.current_face(), Face::FocusMini);
        assert_eq!(i.focus_minutes_left(), Some(23));
        i.data = data();
        i.data.focus = Some(Focus { active: true, end_epoch: None });
        run(&mut i, 600);
        assert_eq!(i.current_face(), Face::MediaMini);
        assert_eq!(i.shown_side, Some(Side::Focus));
        assert_eq!(i.focus_minutes_left(), None);
        i.data.focus = Some(Focus { active: false, end_epoch: None });
        assert!(!i.focus_active());
    }

    #[test]
    fn quick_settings_toggles_answer_at_once() {
        let mut i = Island::new(Config::default());
        i.data.wifi = Some(true);
        i.data.bluetooth = Some(false);
        assert_eq!(i.activate(Action::Cmd(Command::ToggleWifi)), vec![Command::ToggleWifi]);
        assert_eq!(i.data.wifi, Some(false));
        i.activate(Action::Cmd(Command::ToggleBluetooth));
        assert_eq!(i.data.bluetooth, Some(true));
        i.activate(Action::Cmd(Command::ToggleKeepAwake));
        assert!(i.data.keep_awake);
        i.data.wifi = None;
        i.activate(Action::Cmd(Command::ToggleWifi));
        assert_eq!(i.data.wifi, None, "unknown stays unknown until the next sample");
    }

    #[test]
    fn clipboard_rows_copy_back_and_show_a_check() {
        let mut i = Island::new(Config::default());
        i.data.clipboard = vec!["one".into(), "two".into()];
        assert_eq!(i.activate(Action::ClipRestore(1)), vec![Command::RestoreClipboard("two".into())]);
        assert!(i.copied.is_some_and(|(j, until)| j == 1 && until > i.now));
        assert!(i.activate(Action::ClipRestore(9)).is_empty());
    }

    #[test]
    fn dnd_from_a_script_and_spectrum_levels() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::SetDnd(true));
        assert!(i.data.dnd);
        assert_eq!(i.take_commands(), vec![Command::SetDnd(true)]);
        assert_eq!(i.dnd_changed(), Some(true));
        assert_eq!(i.dnd_changed(), None);
        i.data = data();
        run(&mut i, 600);
        assert!(i.wants_spectrum(), "music plays on mains power");
        i.data.battery = Some(Battery { percent: 50.0, charging: false, on_ac: false });
        assert!(!i.wants_spectrum(), "not on battery");
        i.push(i.now, Input::Spectrum(vec![0.5; 8]));
        assert_eq!(i.spectrum.len(), 8);
    }

    #[test]
    fn the_new_pages_draw() {
        let f = fonts();
        for page in [Page::Clip, Page::Focus, Page::Qs, Page::Sys] {
            let mut i = Island::new(Config::default());
            i.data = data();
            i.data.clipboard = vec!["a".into(); 7];
            i.data.focus = Some(Focus { active: true, end_epoch: Some(T0 / 1000 + 600) });
            i.data.system = Some(SystemStats { cores: vec![10.0; 12], ..Default::default() });
            for _ in 0..200 {
                let t = i.now + 8;
                let fr = i.tick(t, 8.0);
                i.push(t, Input::OpenPage(page));
                let mut scene = Scene::new();
                i.draw(&mut scene, &f, fr.surface, 1.0);
                assert!(fr.surface.0 < 1500.0 && fr.surface.1 < 1200.0 && fr.surface.1.is_finite(), "{page:?}");
            }
        }
    }

    #[test]
    fn the_keyboard_reaches_the_controls() {
        let f = fonts();
        let mut i = Island::new(Config::default());
        i.push(T0, Input::OpenKeyboard(Some(Page::Home)));
        assert!(i.wants_keyboard());
        for _ in 0..200 {
            let fr = i.tick(i.now + 8, 8.0);
            let mut scene = Scene::new();
            i.draw(&mut scene, &f, fr.surface, 1.0);
        }
        let nav = i.nav_hits();
        assert!(nav.len() >= 5, "the preset chips and the calendar button: {}", nav.len());
        let key = |i: &mut Island, k: KeyInput| i.push(i.now, Input::Key(k));
        key(&mut i, KeyInput::Right);
        assert_eq!(i.kb.unwrap().index, 1);
        key(&mut i, KeyInput::Left);
        key(&mut i, KeyInput::Left);
        assert_eq!(i.kb.unwrap().index, nav.len() - 1, "wraps around");
        let target = nav.iter().position(|ix| i.hits[*ix].action == Action::TimerStart(300)).expect("the 5 min chip is drawn");
        for _ in 0..(target + 1) % nav.len() {
            key(&mut i, KeyInput::Right);
        }
        key(&mut i, KeyInput::Enter);
        assert!(i.timer.is_some(), "Enter on the 5 min chip starts the timer");
        key(&mut i, KeyInput::Escape);
        assert!(i.kb.is_none() && !i.wants_keyboard());
    }

    #[test]
    fn the_keyboard_lets_go_when_idle() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::OpenKeyboard(None));
        run(&mut i, 10_000);
        assert!(i.kb.is_none(), "an abandoned keyboard session ends");
    }

    fn type_text(i: &mut Island, t: &str) {
        i.push(i.now, Input::Key(KeyInput::Text(t.into())));
    }

    #[test]
    fn the_settings_menu_adds_a_calendar_and_checks_what_you_type() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::OpenPage(Page::Settings));
        i.activate(Action::EditField(Field::Calendar));
        assert!(i.wants_keyboard());
        type_text(&mut i, "not a calendar");
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert!(i.take_commands().is_empty(), "rejected");
        assert!(i.note.is_some() && i.edit.is_some(), "the field stays open with a hint");
        i.push(i.now, Input::Key(KeyInput::Home));
        for _ in 0..14 {
            i.push(i.now, Input::Key(KeyInput::Delete));
        }
        type_text(&mut i, "webcal://p01-caldav.icloud.com/published/2/abc");
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert_eq!(i.take_commands(), vec![Command::AddCalendar("webcal://p01-caldav.icloud.com/published/2/abc".into())]);
        assert!(i.edit.is_none() && !i.wants_keyboard());
    }

    #[test]
    fn coordinates_font_and_sound_are_validated_and_escape_cancels() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::OpenPage(Page::Settings));
        i.data.config.latitude = Some(43.7);
        i.activate(Action::EditField(Field::Latitude));
        assert_eq!(i.edit.as_ref().unwrap().text, "43.7", "starts from the current value");
        i.push(i.now, Input::Key(KeyInput::Home));
        i.push(i.now, Input::Key(KeyInput::Delete));
        type_text(&mut i, "9");
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert!(i.take_commands().is_empty(), "93.7 is not a latitude");
        i.push(i.now, Input::Key(KeyInput::Escape));
        assert!(i.edit.is_none());
        i.activate(Action::EditField(Field::Longitude));
        type_text(&mut i, "10,4017");
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert_eq!(i.take_commands(), vec![Command::SetLongitude(10.4017)], "a decimal comma is accepted");
        i.activate(Action::EditField(Field::Font));
        type_text(&mut i, "Inter");
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert_eq!(i.take_commands(), vec![Command::SetFont("Inter".into())]);
        i.activate(Action::EditField(Field::Sound));
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert_eq!(i.take_commands(), vec![Command::SetNotificationSound(String::new())], "empty clears it");
        assert_eq!(i.activate(Action::RemoveCalendar(2)), vec![Command::RemoveCalendar(2)]);
    }

    #[test]
    fn leaving_the_settings_page_drops_an_open_edit_and_the_page_draws_with_an_edit() {
        let f = fonts();
        let mut i = Island::new(Config::default());
        i.data.config.calendars = vec!["https://a.org/x.ics".into(); 6];
        i.push(T0, Input::OpenPage(Page::Settings));
        i.activate(Action::EditField(Field::Font));
        type_text(&mut i, &"long ".repeat(30));
        for _ in 0..200 {
            let fr = i.tick(i.now + 8, 8.0);
            i.push(i.now, Input::OpenPage(Page::Settings));
            let mut scene = Scene::new();
            i.draw(&mut scene, &f, fr.surface, 1.0);
            assert!(fr.surface.1 < 1000.0);
        }
        assert_eq!(i.current_face(), Face::Settings);
        i.push(i.now, Input::Key(KeyInput::Escape));
        i.activate(Action::EditField(Field::Font));
        i.activate(Action::Back);
        run(&mut i, 600);
        assert!(i.edit.is_none(), "the edit does not outlive its page");
    }
}
