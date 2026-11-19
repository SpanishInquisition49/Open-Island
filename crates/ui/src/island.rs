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
/// Height of the page dock at the bottom of a page.
pub const DOCK_H: f64 = 34.0;
pub const BUBBLE_W: f64 = 66.0;
pub const BUBBLE_GAP: f64 = 6.0;
pub const ICON_W: f64 = 19.0;
const REPLY_IDLE_MS: i64 = 20_000;
const BAR_IDLE_MS: i64 = 30_000;
/// How far a sideways swipe has to go (in scroll units) to skip a track, and the pause after one.
const SWIPE_TRACK: f64 = 3.0;
const SWIPE_COOLDOWN_MS: i64 = 700;
/// How long a press of play/pause is believed before the player has confirmed it.
const PLAY_CONFIRM_MS: i64 = 1500;
const SNOOZE_MS: i64 = 10 * 60_000;
/// Notifications the island makes itself (confirmations, reminders, the recap) have ids from here.
const OWN_ID_BASE: u32 = 3_000_000;
const KB_IDLE_MS: i64 = 9_000;
const EDIT_IDLE_MS: i64 = 40_000;
const LYRIC_LEAD_MS: f64 = 250.0;
/// Events older than this are read again when a page that shows them opens.
const STALE_EVENTS_MS: i64 = 120_000;
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
    /// How long before an event the pill on the side reminds of it (the card is `meeting_lead_ms` before).
    pub meeting_pill_ms: i64,
    pub meeting_alerts: bool,
    pub rain_alerts: bool,
    pub battery_alerts: bool,
    pub low_battery: f32,
    pub media_auto_expand: bool,
    pub clock_in_pill: bool,
    pub status_icons: bool,
    pub lyrics: bool,
    pub clipboard_history: bool,
    /// The current lyric line in the small pill instead of the title.
    pub lyrics_in_pill: bool,
    pub timer_presets: Vec<u64>,
    /// Notifications are held (not shown) during a calendar event / while the microphone or the screen share is on.
    pub quiet_in_meetings: bool,
    pub quiet_on_mic: bool,
    pub quiet_on_screen_share: bool,
    /// Show notifications as cards (off: they only go to the history).
    pub show_notifications: bool,
    /// Show the volume and brightness bars.
    pub show_osd: bool,
    /// Colour the media card from the cover.
    pub art_tint: bool,
    /// Go out of sight under a fullscreen window.
    pub hide_on_fullscreen: bool,
    /// Fetch and show the weather.
    pub weather: bool,
    /// Play a sound when a timer ends.
    pub timer_sound: bool,
    /// Move the audio output to headphones when they connect, and back when they go.
    pub auto_audio_output: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            notification_ms: 5000,
            osd_ms: 1600,
            meeting_lead_ms: 10 * 60_000,
            meeting_pill_ms: 10 * 60_000,
            meeting_alerts: true,
            rain_alerts: true,
            battery_alerts: true,
            low_battery: 15.0,
            media_auto_expand: true,
            clock_in_pill: true,
            status_icons: true,
            lyrics: true,
            clipboard_history: true,
            lyrics_in_pill: false,
            timer_presets: vec![300, 600, 1500],
            quiet_in_meetings: true,
            quiet_on_mic: true,
            quiet_on_screen_share: true,
            show_notifications: true,
            show_osd: true,
            art_tint: true,
            hide_on_fullscreen: true,
            weather: true,
            timer_sound: true,
            auto_audio_output: true,
        }
    }
}

/// What a click on a drawn region does.
/// Rows of the clipboard page.
pub const CLIP_ROWS: usize = 7;

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
    /// A tab of the options page.
    OptionsTab(usize),
    /// Silence the app of the n-th notification in the history (hide it from now on).
    MuteApp(usize),
    RemoveRule(usize),
    /// Flip a switch of the options page.
    Toggle(&'static str),
    /// Raise (1) or lower (-1) a number of the options page.
    Step(&'static str, i32),
    /// Hide the notification on screen and bring it back in ten minutes.
    Snooze,
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
    /// Switch to a page from the dock (does not add to the back history).
    Goto(Page),
    EditField(Field),
    RemoveCalendar(usize),
    /// Open the colour choice of a calendar.
    PickColor(usize),
    SetColor(usize, (u8, u8, u8)),
    Back,
    CalToday,
    HistoryRemove(usize),
    HistoryOpen(usize),
    /// A click on the notification card: go to the app that sent it.
    NotifOpen,
    ClipRestore(usize),
    ClipPin(usize),
    /// Send the files waiting on the send page to the device at this index.
    SendTo(usize),
    ClipForget(usize),
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

pub fn is_own_notification(n: &Incoming) -> bool {
    is_own(n)
}

fn is_own(n: &Incoming) -> bool {
    (OWN_ID_BASE..OWN_ID_BASE + 1_000_000).contains(&n.id)
}

/// The command bar being typed in.
#[derive(Clone, Debug, Default)]
pub struct Bar {
    pub text: String,
    pub cursor: usize,
    pub until: i64,
}

/// A reminder set from the command bar.
#[derive(Clone, Debug, PartialEq)]
pub struct Reminder {
    pub due_ms: i64,
    pub text: String,
}

/// A setting being edited in the settings menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Calendar,
    /// Rename the calendar at this index.
    CalendarName(usize),
    ICloudUser,
    ICloudPassword,
    Font,
    Latitude,
    Longitude,
    Sound,
    TimerPresets,
    Screen,
    /// An app (or a word) whose notifications are hidden.
    RuleHide,
    /// An app (or a word) whose notifications lose their sound.
    RuleSilence,
}

#[derive(Clone, Debug)]
pub struct Edit {
    pub field: Field,
    pub text: String,
    pub cursor: usize,
    pub until: i64,
}

impl Edit {
    /// The text is drawn as dots (a password).
    pub fn secret(&self) -> bool {
        self.field == Field::ICloudPassword
    }
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
    /// Another frame is needed at every vsync (the shape is moving).
    pub animating: bool,
    /// How long the platform may wait before the next frame when nothing moves: 33 ms for the equalizer, 120 ms
    /// for things that expire, 500 ms for a running timer, 1000 ms for the clock. 0 when `animating`.
    pub next_ms: u32,
}

/// A command a shell started (see `open-island hook`).
pub(crate) struct ShellCmd {
    pub id: String,
    pub label: String,
    pub start_ms: i64,
    pub after_ms: i64,
    pub shown: bool,
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
    pub(crate) bar: Option<Bar>,
    pub(crate) options_tab: usize,
    swipe_acc: f64,
    swipe_until: i64,
    /// The play/pause button answers at once: the state a press asked for, until the player confirms it.
    play_override: Option<(bool, i64)>,
    /// 0 shows the play icon, 1 the pause icon; eased, so the icons cross-fade.
    pub(crate) play_anim: f64,
    /// 0..1: how much the pointer is over the button, and a press that is dying away.
    pub(crate) disc_hover: f64,
    pub(crate) disc_press: f64,
    /// A skip button was pressed: +1 next, -1 previous, dying away; the icon nudges in its direction.
    pub(crate) skip_press: f64,
    pub(crate) reminders: Vec<Reminder>,
    /// Files waiting for a device to be picked (the send page).
    pub(crate) send_files: Vec<String>,
    next_own_id: u32,
    /// Keyboard navigation: the focused control among the drawn ones.
    pub(crate) kb: Option<Kb>,
    /// Pages opened from other pages, for the back button.
    pub(crate) page_stack: Vec<Page>,
    /// Which way the last page change went (1 = to the right, -1 = to the left, 0 = no page change).
    pub(crate) slide: f64,
    pub(crate) edit: Option<Edit>,
    /// The calendar whose colours are shown (settings page).
    pub(crate) picking: Option<usize>,
    /// The Apple ID typed so far while the password is asked.
    icloud_user: Option<String>,
    pub(crate) note: Option<(String, i64)>,
    pub(crate) queue: Vec<notify::Notification>,
    waiting: std::collections::VecDeque<Incoming>,
    pub(crate) history: Vec<(Incoming, i64)>,
    pub(crate) osd: Option<Osd>,
    /// What the bars show: the level eased towards the real one, so a level that arrives in steps moves smoothly.
    pub(crate) osd_shown: f64,
    levels_init: bool,
    pub(crate) volume_shown: f64,
    pub(crate) brightness_shown: f64,
    osd_until: i64,
    pub(crate) power: Option<PowerEvent>,
    power_until: i64,
    pub(crate) device_ev: Option<(Device, bool)>,
    device_until: i64,
    /// Devices whose low battery was already announced.
    device_low_told: Vec<String>,
    low_battery_sent: bool,
    last_plugged: Option<bool>,
    pub(crate) meeting: Option<calendar::Event>,
    meeting_until: i64,
    pub(crate) rain_in: Option<i32>,
    rain_until: i64,
    rain_announced: bool,
    pub(crate) ops: Tracker,
    /// Commands the shell hook reported and that still run.
    pub(crate) cmds: Vec<ShellCmd>,
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
    /// When the calendars were last asked to refresh (so opening pages does not ask over and over).
    last_refresh_req: i64,
    pub(crate) copied: Option<(usize, i64)>,
    pub(crate) pending: Vec<Command>,
    dnd_seen: bool,
    /// Why notifications are held without the user having asked (a meeting, the microphone, a screen share).
    pub(crate) auto_quiet: Option<&'static str>,
    /// When the notifications were first held, for the recap that comes when they are let through again.
    quiet_since: Option<i64>,
    recap_id: u32,
    dnd_sent: bool,
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
            bar: None,
            options_tab: 0,
            swipe_acc: 0.0,
            swipe_until: 0,
            play_override: None,
            play_anim: 0.0,
            disc_hover: 0.0,
            disc_press: 0.0,
            skip_press: 0.0,
            reminders: vec![],
            send_files: vec![],
            next_own_id: OWN_ID_BASE,
            kb: None,
            page_stack: vec![],
            slide: 0.0,
            edit: None,
            picking: None,
            icloud_user: None,
            note: None,
            queue: vec![],
            waiting: Default::default(),
            history: vec![],
            osd: None,
            osd_shown: 0.0,
            levels_init: false,
            volume_shown: 0.0,
            brightness_shown: 0.0,
            osd_until: 0,
            power: None,
            power_until: 0,
            device_ev: None,
            device_until: 0,
            device_low_told: vec![],
            low_battery_sent: false,
            last_plugged: None,
            meeting: None,
            meeting_until: 0,
            rain_in: None,
            rain_until: 0,
            rain_announced: false,
            ops: Tracker::default(),
            cmds: vec![],
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
            last_refresh_req: -100_000,
            copied: None,
            pending: vec![],
            dnd_seen: false,
            auto_quiet: None,
            quiet_since: None,
            recap_id: 0,
            dnd_sent: false,
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
            Input::OpenBar => self.bar = Some(Bar { until: now + BAR_IDLE_MS, ..Default::default() }),
            Input::Click(x, y) => {
                let cmds = self.pointer_down(now, x, y);
                self.pending.extend(cmds);
                self.pointer_up(now);
            }
            Input::DumpHits => {
                let (ox, oy) = self.pill_origin;
                log::info!("pill at ({ox:.0}, {oy:.0}), {:.0} x {:.0}; face {:?}", self.spring.w.pos, self.spring.h.pos, self.shown);
                for h in &self.hits {
                    log::info!("  hit {:?} at surface ({:.0}, {:.0}) size {:.0} x {:.0}", h.action, h.rect.center().x + ox, h.rect.center().y + oy, h.rect.width(), h.rect.height());
                }
            }
            Input::CyclePage(dir) => {
                self.cycle_page(dir);
                self.expand_until = now + 5000;
            }
            Input::SettingsNote(t) => self.note = Some((t, now + 5000)),
            Input::OpenKeyboard(p) => {
                self.kb = Some(Kb { index: 0, last_key: now });
                self.page = p.or(Some(Page::Home));
                self.hover_page = self.page;
                self.expand_until = now + KB_IDLE_MS;
            }
            Input::SetDnd(on) => {
                self.data.dnd = on;
                self.dnd_sent = self.quiet();
                self.pending.push(Command::SetDnd(self.dnd_sent));
            }
            Input::Spectrum(v) => {
                self.spectrum = v;
                self.spectrum_at = now;
            }
            Input::Osd { .. } if !self.cfg.show_osd => {}
            Input::Osd { kind, level, muted } => {
                // a bar that was not on screen starts from the level it had before, and slides to the new one
                let before = match kind {
                    OsdKind::Volume => self.data.volume,
                    OsdKind::Brightness => self.data.brightness,
                };
                if self.osd.as_ref().is_none_or(|o| o.kind != kind) {
                    self.osd_shown = before.unwrap_or(level) as f64;
                }
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
            Input::DeviceConnected(d) => {
                self.device_ev = Some((d, false));
                self.device_until = now + 3500;
            }
            Input::DeviceLow(d) => {
                if !self.device_low_told.contains(&d.id) {
                    self.device_low_told.push(d.id.clone());
                    self.device_ev = Some((d, true));
                    self.device_until = now + 6000;
                }
            }
            Input::Activity(state, a) => match state {
                progress::State::Run => self.ops.run_activity(now, &a),
                s => self.ops.finish_activity(now, s, &a),
            },
            Input::CommandStart { id, label, after_secs } => {
                self.cmds.retain(|c| c.id != id);
                self.cmds.truncate(31);
                self.cmds.push(ShellCmd { id, label, start_ms: now, after_ms: after_secs as i64 * 1000, shown: false });
            }
            Input::CommandGone(id) => self.cmds.retain(|c| c.id != id),
            Input::CommandEnd { id, code } => {
                if let Some(at) = self.cmds.iter().position(|c| c.id == id) {
                    let c = self.cmds.remove(at);
                    if c.shown {
                        let took = duration::fmt_took(now - c.start_ms);
                        if code == 0 {
                            self.ops.finish(now, progress::State::Done, &format!("{} · {took}", c.label));
                        } else {
                            self.ops.finish(now, progress::State::Fail, &format!("{} · exit {code} · {took}", c.label));
                        }
                    }
                }
            }
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
            Input::SendFiles(paths) => {
                if !paths.is_empty() {
                    self.send_files = paths;
                    self.activate(Action::Open(Page::Send));
                    self.expand_until = now + 30_000;
                    self.pending.push(Command::ScanPeers);
                }
            }
        }
    }

    fn on_notification(&mut self, n: Incoming) {
        log::debug!("notification {} from {:?}: dnd={} showing={}", n.id, n.app, self.data.dnd, self.notif.is_some());
        if !(is_own(&n) && n.app == "open-island") {
            self.history.insert(0, (n.clone(), self.now));
            self.history.truncate(30);
        }
        if (self.quiet() || !self.cfg.show_notifications) && !n.critical && !is_own(&n) {
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

    /// What a click on a notification does: the sender's default action (while the notification is live), and
    /// the app's window to the front.
    fn open_commands(n: &Incoming, live: bool) -> Vec<Command> {
        let mut c = vec![];
        if live && n.actions.iter().any(|a| a.key == "default") {
            c.push(Command::InvokeAction { id: n.id, key: "default".into() });
        }
        c.push(Command::FocusApp { desktop_entry: n.desktop_entry.clone(), app: n.app.clone() });
        c
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

    /// A message from the island itself (a confirmation, a copied result).
    fn say(&mut self, summary: &str, body: &str) {
        self.next_own_id += 1;
        let id = self.next_own_id;
        let n = Incoming { id, app: "open-island".into(), summary: summary.into(), body: body.into(), expire_ms: Some(3000), ..Default::default() };
        self.push(self.now, Input::Notification(n));
    }

    pub(crate) fn bar_cmd(&self) -> Option<island_core::bar::Cmd> {
        let b = self.bar.as_ref()?;
        let local = (self.now / 1000 + self.data.utc_offset_secs as i64).rem_euclid(86_400);
        island_core::bar::parse(&b.text, local)
    }

    fn bar_key(&mut self, k: KeyInput) {
        let now = self.now;
        let Some(b) = self.bar.as_mut() else { return };
        b.until = now + BAR_IDLE_MS;
        if edit_text(&mut b.text, &mut b.cursor, &k) {
            return;
        }
        match k {
            KeyInput::Escape => self.bar = None,
            KeyInput::Enter => {
                // nothing understood: the bar stays, its preview line says so
                if let Some(c) = self.bar_cmd() {
                    self.bar = None;
                    self.run_bar(c);
                }
            }
            _ => {}
        }
    }

    fn run_bar(&mut self, c: island_core::bar::Cmd) {
        use island_core::bar::Cmd;
        let now = self.now;
        match c {
            Cmd::Timer { secs } => self.start_timer(secs),
            Cmd::Pomodoro => self.start_pomodoro(),
            Cmd::Stopwatch => self.push(now, Input::StartStopwatch),
            Cmd::Dnd(on) => self.pending.push(Command::SetDnd(on)),
            Cmd::Focus { minutes } => self.pending.push(Command::StartFocus(minutes)),
            Cmd::Remind { secs, text } => {
                self.reminders.push(Reminder { due_ms: now + secs * 1000, text: text.clone() });
                self.reminders.sort_by_key(|r| r.due_ms);
                self.reminders.truncate(50);
                self.say("Reminder set", &format!("in {} · {text}", island_core::duration::fmt_took(secs * 1000).replace(' ', "")));
            }
            Cmd::Value(v) => {
                let copy = v.rsplit_once(" = ").map_or(v.clone(), |(_, r)| r.to_string());
                self.pending.push(Command::RestoreClipboard(copy.clone()));
                self.say("Copied", &copy);
            }
        }
    }

    /// Reminders that are due become notifications.
    fn fire_reminders(&mut self, now: i64) {
        while self.reminders.first().is_some_and(|r| r.due_ms <= now) {
            let r = self.reminders.remove(0);
            self.next_own_id += 1;
            let n = Incoming { id: self.next_own_id, app: "Reminder".into(), summary: r.text, body: String::new(), expire_ms: Some(15_000), ..Default::default() };
            self.pending.push(Command::PlaySound);
            self.push(now, Input::Notification(n));
        }
    }

    pub fn reminders(&self) -> &[Reminder] {
        &self.reminders
    }

    pub fn restore_reminders(&mut self, r: Vec<Reminder>) {
        self.reminders = r;
        self.reminders.truncate(50);
    }

    fn on_key(&mut self, k: KeyInput) {
        let now = self.now;
        if self.edit.is_some() {
            self.edit_key(k);
            return;
        }
        if self.bar.is_some() {
            self.bar_key(k);
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

    /// Opens a tab of the options page (0 Island, 1 Alerts, 2 Music).
    pub fn show_options_tab(&mut self, tab: usize) {
        self.options_tab = tab.min(crate::options::TABS.len() - 1);
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
            Field::Calendar | Field::ICloudUser | Field::ICloudPassword => String::new(),
            Field::CalendarName(i) => c.calendars.get(i).map(|x| x.name.clone()).unwrap_or_default(),
            Field::Font => c.font.clone(),
            Field::Latitude => c.latitude.map(|v| v.to_string()).unwrap_or_default(),
            Field::Longitude => c.longitude.map(|v| v.to_string()).unwrap_or_default(),
            Field::Sound => c.notification_sound.clone(),
            Field::RuleHide | Field::RuleSilence => String::new(),
            Field::TimerPresets => c.timer_presets.clone(),
            Field::Screen => if c.screen == "focused" { String::new() } else { c.screen.clone() },
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
            KeyInput::Escape => {
                self.edit = None;
                self.icloud_user = None;
            }
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
                    Field::CalendarName(i) => self.pending.push(Command::RenameCalendar { index: i, name: t }),
                    Field::ICloudUser if t.contains('@') && t.len() > 3 => {
                        self.icloud_user = Some(t);
                        self.note = Some(("Now the app-specific password (appleid.apple.com > Sign-In and Security)".into(), now + 8000));
                        self.start_edit(Field::ICloudPassword);
                    }
                    Field::ICloudUser => {
                        self.note = note("Type the Apple ID (an email address)");
                        self.edit = Some(Edit { text: t.clone(), cursor: t.chars().count(), ..e });
                    }
                    Field::ICloudPassword if !t.is_empty() => {
                        if let Some(user) = self.icloud_user.take() {
                            self.pending.push(Command::AddICloud { user, password: t });
                            self.note = note("Signing in to iCloud...");
                        }
                    }
                    Field::ICloudPassword => {
                        self.note = note("An app-specific password is needed (not the Apple ID password)");
                        self.edit = Some(e);
                    }
                    Field::Font => self.pending.push(Command::SetFont(t)),
                    Field::Sound => self.pending.push(Command::SetNotificationSound(t)),
                    Field::RuleHide | Field::RuleSilence if t.is_empty() => {}
                    Field::RuleHide | Field::RuleSilence => {
                        // a word is matched against the app and the text: the app first
                        self.pending.push(Command::AddFilter { app: t, content: String::new(), hide: e.field == Field::RuleHide });
                    }
                    Field::TimerPresets => self.pending.push(Command::SetOption { key: "timer_presets".into(), value: OptionValue::Text(t) }),
                    Field::Screen => self.pending.push(Command::SetOption { key: "screen".into(), value: OptionValue::Text(t) }),
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
            KeyInput::NextPage | KeyInput::PrevPage => {
                self.cycle_page(if k == KeyInput::NextPage { 1 } else { -1 });
                kb.index = 0;
            }
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
        self.reply.is_some() || self.kb.is_some() || self.edit.is_some() || self.bar.is_some()
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
        // what opens depends on where the pointer rests: the side bubble opens its own activity, the pill its own
        if inside && self.shown.is_compact() {
            let over_bubble = self.shown_side.is_some() && x > px + self.spring.w.pos;
            self.hover_page = if over_bubble { self.shown_side.map(side_page) } else { main_page(self.shown) };
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
        // the most specific control wins: a button on a row beats the row, whatever was drawn first
        let at = vello::kurbo::Point::new(lx, ly);
        self.hits.iter().rev().filter(|h| h.rect.contains(at)).min_by(|a, b| (a.rect.width() * a.rect.height()).total_cmp(&(b.rect.width() * b.rect.height())))
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
            Face::Device => self.device_until = 0,
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

    /// The middle button on the small pill mutes the sound.
    pub fn middle_click(&mut self, now: i64) -> Vec<Command> {
        self.now = now;
        if self.shown.is_compact() {
            return vec![Command::ToggleMute];
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
            if self.shown.size().1 > 40.0 {
                self.cycle_page(if dx > 0.0 { 1 } else { -1 });
                self.expand_until = now + 4000;
                return vec![];
            }
            // over the small pill with music: next / previous track (a swipe is many small events: add them up,
            // act once past a threshold, then leave a moment before the next one)
            if self.data.media.is_some() && now >= self.swipe_until {
                self.swipe_acc += dx;
                if self.swipe_acc.abs() >= SWIPE_TRACK {
                    let next = self.swipe_acc > 0.0;
                    self.swipe_acc = 0.0;
                    self.swipe_until = now + SWIPE_COOLDOWN_MS;
                    return vec![if next { Command::MediaNext } else { Command::MediaPrevious }];
                }
            }
            return vec![];
        }
        // vertical scroll over the pill or a media face changes the volume
        if self.shown.is_compact() || matches!(self.shown, Face::MediaFull | Face::MediaLyric | Face::MediaMini | Face::Osd | Face::Qs) {
            let v = (self.data.volume.unwrap_or(0.5) + (dy as f32) * 0.02).clamp(0.0, 1.0);
            self.data.volume = Some(v);
            return vec![Command::SetVolume(v)];
        }
        vec![]
    }

    /// Moves to the next (1) or previous (-1) page in the dock's order, wrapping around.
    pub(crate) fn cycle_page(&mut self, dir: i32) {
        let pages = self.pages();
        let current = self.page.or_else(|| page_of(self.shown));
        self.page = face::cycle_page(&pages, current, dir);
        self.hover_page = self.page;
    }

    /// The pages the dock shows and a swipe cycles through: the live activities first, then the fixed pages.
    pub(crate) fn pages(&self) -> Vec<Page> {
        let mut v = face::swipe_pages(
            self.data.media.is_some(),
            self.timer.is_some(),
            self.focus_active(),
            self.soon().is_some(),
            self.ops.op.is_some(),
            !self.history.is_empty(),
            self.data.weather.is_some(),
        );
        if let Some(at) = v.iter().position(|p| *p == Page::Home) {
            v.insert(at + 1, Page::Today);
        }
        if self.cfg.clipboard_history {
            v.push(Page::Clip);
        }
        v.push(Page::Options);
        v.push(Page::Settings);
        v
    }

    /// When it rains next, from the forecast.
    pub(crate) fn rain_outlook(&self) -> Option<weather::Outlook> {
        let w = self.data.weather.as_ref()?;
        Some(weather::outlook(&w.rain_slots, &w.hours, self.now))
    }

    fn soon(&self) -> Option<&calendar::Event> {
        if !self.cfg.meeting_alerts {
            return None;
        }
        calendar::soon(&self.data.events, self.now, self.cfg.meeting_pill_ms)
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
            Action::Cmd(c @ (Command::MediaNext | Command::MediaPrevious)) => {
                self.skip_press = if c == Command::MediaNext { 1.0 } else { -1.0 };
                return vec![c];
            }
            Action::Cmd(Command::MediaPlayPause) => {
                // show the answer now; the player's own report comes a moment later and agrees
                self.play_override = Some((!self.playing_shown(), now + PLAY_CONFIRM_MS));
                self.disc_press = 1.0;
                return vec![Command::MediaPlayPause];
            }
            Action::Cmd(c) => {
                if c == Command::RefreshCalendars {
                    self.last_refresh_req = now;
                    self.note = Some(("Reading the calendars…".into(), now + 3000));
                }
                // the chips answer at once; the next sample confirms
                match &c {
                    Command::ToggleWifi => self.data.wifi = self.data.wifi.map(|v| !v),
                    Command::ToggleBluetooth => self.data.bluetooth = self.data.bluetooth.map(|v| !v),
                    Command::ToggleKeepAwake => self.data.keep_awake = !self.data.keep_awake,
                    Command::ToggleNightLight => self.data.night_light = self.data.night_light.map(|v| !v),
                    _ => {}
                }
                return vec![c];
            }
            Action::Back => {
                // back to the page that led here, else to whatever the island shows by default
                let prev = loop {
                    match self.page_stack.pop() {
                        Some(p) if Some(p) == self.page => continue,
                        other => break other,
                    }
                };
                self.page = prev;
                self.hover_page = prev;
            }
            Action::Goto(p) => {
                self.page = Some(p);
                self.hover_page = Some(p);
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
            Action::OptionsTab(t) => self.options_tab = t.min(crate::options::TABS.len() - 1),
            Action::MuteApp(n) => {
                if let Some((inc, _)) = self.history.get(n) {
                    let app = inc.app.clone();
                    self.say("Notifications hidden", &format!("{app} (Options > Rules to undo)"));
                    return vec![Command::AddFilter { app, content: String::new(), hide: true }];
                }
            }
            Action::RemoveRule(n) => return vec![Command::RemoveFilter(n)],
            Action::Toggle(key) => {
                let on = !crate::options::get_bool(&self.cfg, key);
                crate::options::set_bool(&mut self.cfg, key, on);
                return vec![Command::SetOption { key: key.into(), value: OptionValue::Bool(on) }];
            }
            Action::Step(key, dir) => {
                if let Some((_, _, _, step, min, max)) = (0..crate::options::TABS.len()).flat_map(crate::options::steppers).find(|s| s.0 == key) {
                    let v = ((crate::options::get_num(&self.cfg, key) + dir as f64 * step).clamp(*min, *max) * 10.0).round() / 10.0;
                    crate::options::set_num(&mut self.cfg, key, v);
                    return vec![Command::SetOption { key: key.into(), value: OptionValue::Num(v) }];
                }
            }
            Action::Snooze => {
                if let Some(n) = self.notif.clone() {
                    let text = match (n.summary.is_empty(), n.app.is_empty()) {
                        (false, false) => format!("{}: {}", n.app, n.summary),
                        (false, true) => n.summary.clone(),
                        _ => n.app.clone(),
                    };
                    self.reminders.push(Reminder { due_ms: now + SNOOZE_MS, text });
                    self.reminders.sort_by_key(|r| r.due_ms);
                    self.close_notif(CloseReason::Dismissed);
                }
            }
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
                if let Some(cur) = self.page.or_else(|| page_of(self.shown)).filter(|c| *c != p) {
                    if self.page_stack.last() != Some(&cur) {
                        self.page_stack.push(cur);
                    }
                    self.page_stack.truncate(8);
                }
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
            Action::EditField(f) => {
                self.picking = None;
                self.start_edit(f)
            }
            Action::PickColor(i) => self.picking = if self.picking == Some(i) { None } else { Some(i) },
            Action::SetColor(i, color) => {
                self.picking = None;
                return vec![Command::SetCalendarColor { index: i, color }];
            }
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
            Action::HistoryOpen(i) => {
                if i < self.history.len() {
                    let (n, _) = self.history.remove(i);
                    self.pending.extend(Self::open_commands(&n, false));
                }
            }
            Action::NotifOpen if self.notif.as_ref().is_some_and(|n| n.id == self.recap_id && self.recap_id != 0) => {
                self.notif = None;
                self.notif_until = 0;
                return self.activate(Action::Open(Page::Notifs));
            }
            Action::NotifOpen => {
                if let Some(n) = self.notif.clone() {
                    self.pending.extend(Self::open_commands(&n, true));
                    if n.resident {
                        self.notif = None;
                        self.notif_until = 0;
                    } else {
                        self.close_notif(CloseReason::Dismissed);
                    }
                }
            }
            Action::ClipRestore(i) => {
                if let Some(t) = self.data.clipboard.get(i).cloned() {
                    self.copied = Some((i, now + 1500));
                    return vec![Command::RestoreClipboard(t)];
                }
            }
            Action::SendTo(i) => {
                if let Some(peer) = self.data.peers.get(i).cloned() {
                    let paths = std::mem::take(&mut self.send_files);
                    let mut out = vec![Command::SendFiles { peer, paths }];
                    out.extend(self.activate(Action::Back));
                    return out;
                }
            }
            Action::ClipPin(i) => {
                if let Some(t) = self.data.clipboard.get(i).cloned() {
                    return vec![Command::PinClipboard(t)];
                }
            }
            Action::ClipForget(i) => {
                if let Some(t) = self.data.clipboard.get(i).cloned() {
                    return vec![Command::ForgetClipboard(t)];
                }
            }
            Action::ToggleDnd => {
                self.data.dnd = !self.data.dnd;
                self.dnd_sent = self.quiet();
                return vec![Command::SetDnd(self.dnd_sent)];
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
                h = 14.0 + 34.0 + 4.0 + 16.0 + weeks * 30.0 + 10.0 + n * 24.0 + 20.0;
            }
            Face::Notif if self.notif.as_ref().is_some_and(|n| n.progress.is_some()) => h += 14.0,
            Face::Notifs => h = 14.0 + 34.0 + 6.0 + self.history.len().clamp(1, 5) as f64 * 46.0 + if self.history.is_empty() { 20.0 } else { 8.0 },
            Face::Clip => h = 14.0 + 34.0 + 4.0 + self.data.clipboard.len().clamp(1, CLIP_ROWS) as f64 * 32.0 + 10.0,
            Face::FocusMini => w += self.icon_count(true) as f64 * ICON_W,
            Face::Settings => h = 14.0 + 34.0 + self.settings_content_height() + 14.0,
            Face::Options => h = 14.0 + 34.0 + 40.0 + crate::options::content_height(self.options_tab, self.data.config.filters.len()) + 14.0,
            Face::Qs => h = self.qs_layout().height,
            Face::Audio => h = self.audio_height(),
            Face::Today => h = self.today_height(),
            Face::Send => h = self.send_height(),
            Face::Sys => h = 14.0 + 34.0 + self.sys_rows() as f64 * 28.0 + 14.0,
            Face::Idle => w += self.icon_count(false) as f64 * ICON_W,
            Face::MediaMini | Face::TimerMini | Face::MeetMini => w += self.icon_count(true) as f64 * ICON_W,
            _ => {}
        }
        if has_dock(face) {
            h += DOCK_H;
            w = w.max(self.pages().len() as f64 * 30.0 + 28.0);
        }
        (w, h)
    }

    /// Rows of the system page that have data.
    pub(crate) fn sys_rows(&self) -> usize {
        let Some(st) = &self.data.system else { return 1 };
        let mut n = 2; // cpu, cores
        n += st.memory.is_some() as usize + st.gpu.is_some() as usize + self.data.battery.is_some() as usize + st.disk.is_some() as usize;
        n + self.data.devices.len().min(4) + 1 // devices, network
    }

    /// Height of the settings page below its header: four sections and their rows.
    pub(crate) fn settings_content_height(&self) -> f64 {
        let cals = self.data.config.calendars.len().min(4) as f64;
        // section titles (4 x 22), calendar rows, the two add rows, latitude, longitude, font, sound, the note line
        4.0 * 22.0 + (cals + 2.0 + 2.0 + 1.0 + 1.0) * 34.0 + 20.0
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
        p.mic as usize + p.camera as usize + p.screen as usize + self.quiet() as usize
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
                // one line on is a scroll; a jump (a seek, the island waking from a long pause) is just there
                let jump = idx.abs_diff(self.lyric_idx) > 1;
                self.lyric_dir = if idx > self.lyric_idx { 1.0 } else { -1.0 };
                self.lyric_idx = idx;
                self.lyric_at = if jump { -100_000 } else { self.now };
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

    /// The line being sung and how far its entrance has come (0..=1), when the track has synced lyrics and one is due.
    pub(crate) fn lyric_now(&self) -> Option<(String, f64)> {
        let lines = self.lyric_lines.as_ref()?;
        let line = lines.get(self.lyric_idx.checked_sub(1)?)?;
        (!line.text.is_empty()).then(|| (line.text.clone(), lyrics::ease_out((self.now - self.lyric_at) as f64 / 300.0)))
    }

    /// The strip is still scrolling: frames are needed.
    fn lyrics_scrolling(&self) -> bool {
        self.lyric_lines.is_some() && ((self.now - self.lyric_at) as f64) < lyrics::SCROLL_MS && matches!(self.shown, Face::MediaLyric | Face::MediaMini)
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
            barring: self.bar.is_some(),
            replying: self.reply.is_some(),
            osd: self.osd.is_some() && now < self.osd_until,
            notif,
            timer_done: now < self.timer_done_until,
            power: self.power.is_some() && now < self.power_until,
            device: self.device_ev.is_some() && now < self.device_until,
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

    /// Notifications are held: asked for, or one of the automatic reasons holds.
    pub fn quiet(&self) -> bool {
        self.data.dnd || self.auto_quiet.is_some()
    }

    /// Why the island holds notifications by itself right now.
    pub fn auto_quiet_reason(&self) -> Option<&'static str> {
        self.auto_quiet
    }

    /// One card for what was held while the island was quiet.
    fn recap(&mut self, since: i64) {
        let mut apps: Vec<(String, usize)> = vec![];
        for (n, at) in self.history.iter().filter(|(n, at)| *at >= since && !is_own(n)) {
            let _ = at;
            match apps.iter_mut().find(|(a, _)| *a == n.app) {
                Some((_, c)) => *c += 1,
                None => apps.push((n.app.clone(), 1)),
            }
        }
        let total: usize = apps.iter().map(|(_, c)| c).sum();
        if total == 0 {
            return;
        }
        apps.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
        let body = apps.iter().take(4).map(|(a, c)| if *c > 1 { format!("{a} {c}") } else { a.clone() }).collect::<Vec<_>>().join(" · ");
        self.next_own_id += 1;
        self.recap_id = self.next_own_id;
        let n = Incoming { id: self.recap_id, app: "open-island".into(), summary: format!("{total} held back"), body, expire_ms: Some(8000), ..Default::default() };
        self.push(self.now, Input::Notification(n));
    }

    fn update_quiet(&mut self, now: i64) {
        let p = self.data.privacy;
        let in_meeting = self.cfg.quiet_in_meetings
            && self.data.events.iter().any(|e| !e.all_day && e.start_ms <= now && now < e.end_ms && e.end_ms - e.start_ms <= 3 * 3_600_000);
        self.auto_quiet = if in_meeting {
            Some("meeting")
        } else if self.cfg.quiet_on_screen_share && p.screen {
            Some("screen share")
        } else if self.cfg.quiet_on_mic && p.mic {
            Some("microphone")
        } else if self.data.focus.as_ref().is_some_and(|f| f.active) {
            Some("focus")
        } else {
            None
        };
        match (self.quiet(), self.quiet_since) {
            (true, None) => self.quiet_since = Some(now),
            (false, Some(since)) => {
                self.quiet_since = None;
                self.recap(since);
            }
            _ => {}
        }
        // the daemon holds the sounds back too: tell it whenever the state it should keep changes
        if self.quiet() != self.dnd_sent {
            self.dnd_sent = self.quiet();
            self.pending.push(Command::SetDnd(self.dnd_sent));
        }
    }

    fn slow_checks(&mut self) {
        let now = self.now;
        self.update_quiet(now);
        // a device whose battery went back up may warn again next time
        let devs = &self.data.devices;
        self.device_low_told.retain(|id| devs.iter().any(|d| d.id == *id && d.battery.is_some_and(|b| b <= 20)));
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

    /// A command that outlasts its threshold becomes the operation on show (when none other is); the one on show
    /// is refreshed so it is not dropped as stale.
    fn shell_commands(&mut self, now: i64) {
        if self.cmds.is_empty() {
            return;
        }
        let running = matches!(&self.ops.op, Some(o) if o.state == progress::State::Run);
        let on_show = self.cmds.iter().find(|c| c.shown).map(|c| c.label.clone());
        match (running, on_show) {
            (true, Some(label)) => {
                if matches!(&self.ops.op, Some(o) if o.label == label && now - o.updated_ms >= 30_000) {
                    self.ops.report(now, &format!("? {label}"));
                }
            }
            (false, _) => {
                if let Some(c) = self.cmds.iter_mut().find(|c| !c.shown && now - c.start_ms >= c.after_ms) {
                    c.shown = true;
                    let label = c.label.clone();
                    self.ops.report(now, &format!("? {label}"));
                }
            }
            _ => {}
        }
    }

    /// Whether the music is shown as playing: what the player says, except just after a press of the button.
    pub(crate) fn playing_shown(&self) -> bool {
        let base = self.data.media.as_ref().is_some_and(|m| m.playing);
        match self.play_override {
            Some((v, until)) if self.now < until => v,
            _ => base,
        }
    }

    /// Moves the play/pause button: the icons cross-fade, the disc grows under the pointer and gives under a press.
    /// Returns true while something is still on its way.
    fn ease_media_button(&mut self, dt_ms: f64) -> bool {
        let dt = if dt_ms.is_finite() { dt_ms.clamp(0.0, 100.0) } else { 0.0 };
        let base = self.data.media.as_ref().is_some_and(|m| m.playing);
        if matches!(self.play_override, Some((v, until)) if v == base || self.now >= until) {
            self.play_override = None; // the player agrees (or never did): back to what it says
        }
        let on_screen = matches!(self.shown, Face::MediaFull | Face::MediaLyric);
        let over = self.pointer.is_some_and(|(x, y)| {
            let (lx, ly) = self.local(x, y);
            self.hits.iter().any(|h| h.action == Action::Cmd(Command::MediaPlayPause) && h.rect.contains(vello::kurbo::Point::new(lx, ly)))
        });
        let mut moving = false;
        let mut ease = |v: &mut f64, target: f64, tau: f64| {
            if !on_screen || (target - *v).abs() < 0.004 {
                *v = target;
            } else {
                *v += (target - *v) * (1.0 - (-dt / tau).exp());
                moving = true;
            }
        };
        let target = if self.playing_shown() { 1.0 } else { 0.0 };
        ease(&mut self.play_anim, target, 70.0);
        ease(&mut self.disc_hover, if over { 1.0 } else { 0.0 }, 60.0);
        ease(&mut self.disc_press, 0.0, 90.0);
        ease(&mut self.skip_press, 0.0, 110.0);
        moving
    }

    /// Moves the levels the bars show towards the real ones: about a twentieth of a second to cover most of the
    /// distance, so the 10 or 30 changes a second that arrive look like a continuous slide at any refresh rate.
    /// A slider being dragged follows the pointer exactly. Returns true while something is still on its way.
    fn ease_levels(&mut self, dt_ms: f64) -> bool {
        let k = 1.0 - (-(if dt_ms.is_finite() { dt_ms.clamp(0.0, 100.0) } else { 0.0 }) / 55.0).exp();
        let dragging = matches!(self.dragging, Some(Action::VolumeBar | Action::BrightnessBar));
        let mut moving = false;
        let mut ease = |shown: &mut f64, target: f64, snap: bool| {
            if snap || (target - *shown).abs() < 0.002 {
                *shown = target;
            } else {
                *shown += (target - *shown) * k;
                moving = true;
            }
        };
        let vol = self.data.volume.unwrap_or(0.0) as f64;
        let bri = self.data.brightness.unwrap_or(0.0) as f64;
        if !self.levels_init && (self.data.volume.is_some() || self.data.brightness.is_some()) {
            // the first reading is where the bars are, not something to slide to
            self.levels_init = true;
            (self.volume_shown, self.brightness_shown) = (vol, bri);
        }
        ease(&mut self.volume_shown, vol, dragging && self.dragging == Some(Action::VolumeBar));
        ease(&mut self.brightness_shown, bri, dragging && self.dragging == Some(Action::BrightnessBar));
        if let Some(o) = self.osd {
            ease(&mut self.osd_shown, if o.muted { 0.0 } else { o.level as f64 }, false);
        }
        moving
    }

    /// Advances the island to `now` (epoch ms) and says how big the surface has to be.
    pub fn tick(&mut self, now: i64, dt_ms: f64) -> Frame {
        self.now = now;
        self.ops.expire(now);
        self.shell_commands(now);
        self.fire_reminders(now);
        if self.bar.as_ref().is_some_and(|b| now > b.until) {
            self.bar = None;
        }
        self.slow_checks();
        self.sync_lyrics();
        // notifications: expiry and the next waiting one
        if self.edit.as_ref().is_some_and(|e| now > e.until) || (self.edit.is_some() && self.page != Some(Page::Settings)) {
            self.edit = None;
            self.icloud_user = None;
        }
        if self.page != Some(Page::Settings) {
            self.picking = None;
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
        }
        // whatever took the card off the screen (expiry, the x, a click), the next waiting one comes up
        if self.notif.is_none() && self.reply.is_none() {
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
        // the calendar opens on today however it was reached (a swipe, the back button, a shortcut)
        if want == Face::Cal && (want != self.want || self.cal.year == 1970) {
            let c = crate::clock::civil(now, self.data.utc_offset_secs);
            self.cal = Cal { year: c.year, month: c.month, day: c.day };
        }
        // opening the calendar or the home page with stale events asks for a fresh read
        if want != self.want && matches!(want, Face::Cal | Face::IdleHover) && !self.data.config.calendars.is_empty() && now - self.data.events_synced_at > STALE_EVENTS_MS && now - self.last_refresh_req > 30_000 {
            self.last_refresh_req = now;
            self.pending.push(Command::RefreshCalendars);
        }
        if want != self.want {
            // between two pages the content slides the way the page order goes
            let pages = self.pages();
            let idx = |f: Face| page_of(f).and_then(|p| pages.iter().position(|x| *x == p));
            self.slide = match (idx(self.shown), idx(want)) {
                (Some(a), Some(b)) if a != b => if b > a { 1.0 } else { -1.0 },
                _ => 0.0,
            };
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
        // what needs frames: motion at every vsync, an equalizer at 30 fps, things that expire a few times a second,
        // a timer twice a second, otherwise the clock once a second
        let levels_moving = self.ease_levels(dt_ms) | self.ease_media_button(dt_ms);
        let waiting_for_hover = (self.hovering && now - self.hover_since < HOVER_INTENT_MS + 60) || now < self.hover_left + HOVER_GRACE_MS + 60;
        let moving = !self.settled || self.lyrics_scrolling() || waiting_for_hover || levels_moving;
        let equalizer = self.data.media.as_ref().is_some_and(|m| m.playing) && matches!(self.shown, Face::MediaMini | Face::MediaFull | Face::MediaLyric);
        let expiring = snap.osd || snap.notif.is_some() || snap.timer_done || snap.power || snap.meeting || snap.rain || snap.op_flash || self.edit.is_some() || self.reply.is_some() || self.kb.is_some() || self.bar.is_some();
        let next_ms: u32 = if moving {
            0
        } else if equalizer {
            16 // sixty bars a second: the bars follow the music, and a slower step shows on a fast display
        } else if expiring {
            120
        } else if self.timer.is_some() || self.ops.op.is_some_and_running() {
            500
        } else {
            1000
        };
        let animating = next_ms == 0;
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
        Frame { surface: (sw.max(40.0) + 8.0, sh.max(20.0) + TOP_MARGIN + 4.0), animating, next_ms }
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

    /// The island's outline as rounded rectangles (x, y, width, height, radius) in surface coordinates: what is
    /// visible, for a platform that has to cut its window to that shape because it cannot be transparent.
    pub fn shape_rects(&self) -> Vec<(f64, f64, f64, f64, f64)> {
        let (w, h) = (self.spring.w.pos, self.spring.h.pos);
        let r = (h / 2.0).min(30.0);
        let mut v = vec![(self.pill_origin.0, self.pill_origin.1, w, h, r)];
        if self.shown_side.is_some() && self.bubble_k > 0.5 {
            v.push((self.pill_origin.0 + w + BUBBLE_GAP, self.pill_origin.1, BUBBLE_W, h, r));
        }
        v
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

/// The page a compact pill opens when hovered.
fn main_page(f: Face) -> Option<Page> {
    match f {
        Face::MediaMini => Some(Page::Media),
        Face::TimerMini => Some(Page::Timer),
        Face::MeetMini => Some(Page::Meeting),
        Face::FocusMini => Some(Page::Focus),
        _ => None,
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

/// Faces that are pages: they carry the dock at the bottom.
pub(crate) fn has_dock(f: Face) -> bool {
    matches!(f, Face::IdleHover | Face::MediaFull | Face::MediaLyric | Face::TimerFull | Face::FocusFull | Face::Cal | Face::Weather | Face::Qs | Face::Sys | Face::Notifs | Face::Clip | Face::Settings | Face::Options | Face::Audio | Face::Today | Face::Send)
}

pub(crate) fn page_of(f: Face) -> Option<Page> {
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
        Face::Clip => Page::Clip,
        Face::Settings => Page::Settings,
        Face::Weather => Page::Weather,
        Face::Options => Page::Options,
        Face::Audio => Page::Audio,
        Face::Today => Page::Today,
        Face::Send => Page::Send,
        Face::FocusFull => Page::Focus,
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

    #[allow(clippy::field_reassign_with_default)]
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
    fn a_shell_command_becomes_a_pill_only_after_its_threshold_and_ends_green_or_red() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::CommandStart { id: "1".into(), label: "make".into(), after_secs: 10 });
        run(&mut i, 5000);
        assert!(i.ops.op.is_none(), "a short command never shows");
        i.push(i.now, Input::CommandEnd { id: "1".into(), code: 0 });
        run(&mut i, 500);
        assert!(i.ops.op.is_none(), "and says nothing when it ends");

        let t = i.now;
        i.push(t, Input::CommandStart { id: "2".into(), label: "cargo build".into(), after_secs: 10 });
        run(&mut i, 11_000);
        let op = i.ops.op.clone().expect("past the threshold");
        assert_eq!((op.label.as_str(), op.state), ("cargo build", progress::State::Run));
        run(&mut i, 100_000);
        assert!(matches!(&i.ops.op, Some(o) if o.state == progress::State::Run), "refreshed, not dropped as stale after two minutes");
        i.push(i.now, Input::CommandEnd { id: "2".into(), code: 0 });
        let op = i.ops.op.clone().unwrap();
        assert_eq!(op.state, progress::State::Done);
        assert!(op.label.starts_with("cargo build · 1m 5"), "{}", op.label);

        i.push(i.now, Input::ProgressClear);
        let t = i.now;
        i.push(t, Input::CommandStart { id: "3".into(), label: "pytest".into(), after_secs: 2 });
        run(&mut i, 3000);
        i.push(i.now, Input::CommandEnd { id: "3".into(), code: 2 });
        let op = i.ops.op.clone().unwrap();
        assert_eq!(op.state, progress::State::Fail);
        assert!(op.label.starts_with("pytest · exit 2 · "), "{}", op.label);
    }

    #[test]
    fn a_second_command_waits_for_the_pill_to_be_free() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::CommandStart { id: "a".into(), label: "one".into(), after_secs: 1 });
        run(&mut i, 2000);
        i.push(i.now, Input::CommandStart { id: "b".into(), label: "two".into(), after_secs: 1 });
        run(&mut i, 3000);
        assert_eq!(i.ops.op.as_ref().unwrap().label, "one", "the first keeps the pill");
        i.push(i.now, Input::CommandEnd { id: "b".into(), code: 0 });
        assert_eq!(i.ops.op.as_ref().unwrap().label, "one", "an unseen command ends silently");
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

    fn type_line(i: &mut Island, text: &str) {
        i.push(i.now, Input::Key(KeyInput::Text(text.into())));
    }

    #[test]
    fn the_command_bar_starts_a_timer() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::OpenBar);
        run(&mut i, 600);
        assert_eq!(i.current_face(), Face::Bar);
        assert!(i.wants_keyboard());
        type_line(&mut i, "10m tea");
        assert!(matches!(i.bar_cmd(), Some(island_core::bar::Cmd::Timer { secs: 600 })));
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert!(i.bar.is_none() && i.timer.is_some(), "the bar closes and the timer runs");
        assert!(!i.wants_keyboard());
    }

    #[test]
    fn the_command_bar_stays_open_on_what_it_does_not_understand_and_escape_closes_it() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::OpenBar);
        type_line(&mut i, "blah blah");
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert!(i.bar.is_some(), "nothing happened, the preview says why");
        assert!(i.timer.is_none() && i.take_commands().is_empty());
        i.push(i.now, Input::Key(KeyInput::Escape));
        assert!(i.bar.is_none());
        i.push(i.now, Input::OpenBar);
        run(&mut i, 31_000);
        assert!(i.bar.is_none(), "an idle bar closes itself");
    }

    #[test]
    fn a_sum_is_copied_to_the_clipboard() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::OpenBar);
        type_line(&mut i, "5 km in mi");
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert!(i.take_commands().contains(&Command::RestoreClipboard("3.106855961 mi".into())), "the result with its unit, not the whole line");
        i.push(T0, Input::OpenBar);
        type_line(&mut i, "12*7");
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert!(i.take_commands().contains(&Command::RestoreClipboard("84".into())));
    }

    #[test]
    fn a_reminder_comes_back_as_a_notification_and_survives_a_restart() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::OpenBar);
        type_line(&mut i, "remind 2m Call Anna");
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert_eq!(i.reminders().len(), 1);
        let saved = i.reminders().to_vec();
        run(&mut i, 5000);
        assert_eq!(i.reminders().len(), 1, "not yet");
        // a new run of the app reads it back
        let mut j = Island::new(Config::default());
        j.restore_reminders(saved);
        run(&mut j, 130_000);
        assert!(j.reminders().is_empty(), "it fired");
        assert!(j.history.iter().any(|(n, _)| n.summary == "Call Anna" && n.app == "Reminder"));
    }

    #[test]
    fn the_do_not_disturb_and_focus_words_reach_the_platform() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::OpenBar);
        type_line(&mut i, "dnd on");
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert!(i.take_commands().contains(&Command::SetDnd(true)));
        i.push(T0, Input::OpenBar);
        type_line(&mut i, "focus 45");
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert!(i.take_commands().contains(&Command::StartFocus(45)));
    }

    fn count_cards(i: &Island) -> usize {
        i.notif.is_some() as usize + i.waiting.len()
    }

    #[test]
    fn notifications_are_held_during_a_calendar_event_and_come_back_after() {
        let mut i = Island::new(Config::default());
        i.data.events = vec![Event { id: "e".into(), title: "Lesson".into(), start_ms: T0 + 60_000, end_ms: T0 + 3_600_000, ..Default::default() }];
        let n = |id: u32| Input::Notification(Incoming { id, app: "Chat".into(), summary: format!("m{id}"), ..Default::default() });
        run(&mut i, 5000);
        i.push(i.now, n(1));
        assert_eq!(count_cards(&i), 1, "before the event: shown");
        run(&mut i, 20_000);
        run(&mut i, 61_000 - 25_000);
        assert_eq!(i.auto_quiet_reason(), Some("meeting"));
        assert!(i.take_commands().contains(&Command::SetDnd(true)), "the daemon is told, so it stays silent too");
        i.push(i.now, n(2));
        assert_eq!(count_cards(&i), 0, "during the event: held back");
        assert_eq!(i.history.len(), 2, "but kept in the history");
        i.push(i.now, Input::Notification(Incoming { id: 3, critical: true, ..Default::default() }));
        assert_eq!(count_cards(&i), 1, "a critical one still gets through");
        run(&mut i, 3_600_000);
        assert_eq!(i.auto_quiet_reason(), None);
        assert!(i.take_commands().contains(&Command::SetDnd(false)));
    }

    #[test]
    fn the_microphone_and_a_screen_share_hold_notifications_unless_switched_off() {
        let mut i = Island::new(Config::default());
        i.data.privacy.mic = true;
        run(&mut i, 2000);
        assert_eq!(i.auto_quiet_reason(), Some("microphone"));
        i.data.privacy.screen = true;
        run(&mut i, 2000);
        assert_eq!(i.auto_quiet_reason(), Some("screen share"));
        i.data.privacy = Default::default();
        run(&mut i, 2000);
        assert!(!i.quiet());
        let mut j = Island::new(Config { quiet_on_mic: false, quiet_on_screen_share: false, quiet_in_meetings: false, ..Config::default() });
        j.data.privacy.mic = true;
        j.data.privacy.screen = true;
        j.data.events = vec![Event { id: "e".into(), start_ms: 0, end_ms: i64::MAX / 2, ..Default::default() }];
        run(&mut j, 2000);
        assert!(!j.quiet(), "all three can be switched off");
    }

    #[test]
    fn an_all_day_or_very_long_event_does_not_silence_the_island() {
        let mut i = Island::new(Config::default());
        i.data.events = vec![
            Event { id: "a".into(), start_ms: 0, end_ms: i64::MAX / 2, all_day: true, ..Default::default() },
            Event { id: "b".into(), start_ms: 0, end_ms: i64::MAX / 2, ..Default::default() },
        ];
        run(&mut i, 2000);
        assert!(!i.quiet());
    }

    #[test]
    fn snoozing_a_notification_brings_it_back_in_ten_minutes() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::Notification(Incoming { id: 7, app: "Mail".into(), summary: "Invoice".into(), ..Default::default() }));
        draw_frames(&mut i, 200);
        assert!(press(&mut i, Action::Snooze), "the bell is drawn on the card");
        assert!(i.notif.is_none());
        assert!(i.take_commands().iter().any(|c| matches!(c, Command::NotificationClosed { id: 7, .. })));
        run(&mut i, 9 * 60_000);
        assert!(i.notif.is_none(), "not yet");
        run(&mut i, 61_000);
        assert_eq!(i.notif.as_ref().map(|n| n.summary.as_str()), Some("Mail: Invoice"), "and then it is back");
        draw_frames(&mut i, 200);
        assert!(!press(&mut i, Action::Snooze), "a reminder's own card has no snooze");
    }

    #[test]
    fn what_was_held_back_comes_as_one_recap_that_opens_the_list() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::SetDnd(true));
        let n = |id: u32, app: &str| Input::Notification(Incoming { id, app: app.into(), summary: format!("m{id}"), ..Default::default() });
        run(&mut i, 1000);
        for (id, app) in [(1, "Telegram"), (2, "Telegram"), (3, "Mail"), (4, "Telegram")] {
            i.push(i.now, n(id, app));
        }
        assert!(i.notif.is_none(), "held");
        i.push(i.now, Input::SetDnd(false));
        run(&mut i, 1000);
        let card = i.notif.clone().expect("the recap");
        assert_eq!((card.summary.as_str(), card.body.as_str()), ("4 held back", "Telegram 3 · Mail"));
        assert_eq!(i.history.len(), 4, "the recap is not a notification of its own");
        draw_frames(&mut i, 200);
        assert!(press(&mut i, Action::NotifOpen), "a click on it");
        assert!(i.take_commands().iter().all(|c| !matches!(c, Command::FocusApp { .. })), "does not look for a window");
        run(&mut i, 600);
        assert_eq!(i.current_face(), Face::Notifs);
    }

    #[test]
    fn nothing_held_means_no_recap_and_a_reminder_gets_through_a_meeting() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::SetDnd(true));
        run(&mut i, 1000);
        i.push(i.now, Input::SetDnd(false));
        run(&mut i, 1000);
        assert!(i.notif.is_none(), "quiet but nothing came: no card");
        i.push(i.now, Input::SetDnd(true));
        i.restore_reminders(vec![Reminder { due_ms: i.now + 2000, text: "Call".into() }]);
        run(&mut i, 3000);
        assert_eq!(i.notif.as_ref().map(|n| n.summary.as_str()), Some("Call"), "a reminder is not held back");
    }

    #[test]
    fn a_study_focus_holds_notifications_too() {
        let mut i = Island::new(Config::default());
        i.data.focus = Some(Focus { active: true, end_epoch: None });
        run(&mut i, 2000);
        assert_eq!(i.auto_quiet_reason(), Some("focus"));
    }

    #[test]
    fn a_script_activity_shows_with_its_own_icon_colour_and_words() {
        let mut i = Island::new(Config::default());
        let a = island_core::progress::Activity { label: "CI main".into(), detail: "building".into(), icon: Some("git-branch".into()), tint: Some((255, 159, 10)), frac: None };
        i.push(T0, Input::Activity(progress::State::Run, a.clone()));
        run(&mut i, 600);
        assert_eq!(i.current_face(), Face::Progress);
        run(&mut i, 4000);
        assert_eq!(i.shown_side, Some(Side::Op), "then a bubble");
        i.push(i.now, Input::Activity(progress::State::Fail, island_core::progress::Activity { label: "CI main".into(), detail: "2 tests failed".into(), ..Default::default() }));
        run(&mut i, 600);
        let o = i.ops.op.clone().unwrap();
        assert_eq!((o.state, o.detail.as_str(), o.icon.as_deref()), (progress::State::Fail, "2 tests failed", Some("git-branch")));
        // it draws without trouble in every state
        let f = fonts();
        let mut scene = Scene::new();
        let fr = i.tick(i.now + 8, 8.0);
        i.draw(&mut scene, &f, fr.surface, 1.0);
    }

    fn headphones(battery: Option<u8>) -> Device {
        Device { id: "88:C9".into(), name: "Sony XM5".into(), kind: island_core::bluetooth::Kind::Headphones, battery }
    }

    #[test]
    fn a_connecting_device_gets_a_card_and_a_low_battery_warns_once_until_it_recovers() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::DeviceConnected(headphones(Some(80))));
        run(&mut i, 600);
        assert_eq!(i.current_face(), Face::Device);
        run(&mut i, 4000);
        assert_ne!(i.current_face(), Face::Device, "it goes away by itself");
        i.data.devices = vec![headphones(Some(12))];
        i.push(i.now, Input::DeviceLow(headphones(Some(12))));
        run(&mut i, 600);
        assert_eq!(i.current_face(), Face::Device);
        assert!(i.device_ev.as_ref().is_some_and(|(_, low)| *low));
        run(&mut i, 7000);
        i.push(i.now, Input::DeviceLow(headphones(Some(11))));
        run(&mut i, 600);
        assert_ne!(i.current_face(), Face::Device, "told once");
        // charged again, then low again: a new warning
        i.data.devices = vec![headphones(Some(90))];
        run(&mut i, 1000);
        i.push(i.now, Input::DeviceLow(headphones(Some(10))));
        run(&mut i, 600);
        assert_eq!(i.current_face(), Face::Device);
    }

    #[test]
    fn the_system_page_lists_connected_devices_and_grows_for_them() {
        let mut i = Island::new(Config::default());
        i.data.system = Some(Default::default());
        let before = i.sys_rows();
        i.data.devices = vec![headphones(Some(50)), Device { id: "m".into(), name: "MX".into(), kind: island_core::bluetooth::Kind::Mouse, battery: None }];
        assert_eq!(i.sys_rows(), before + 2);
        i.push(T0, Input::OpenPage(Page::Sys));
        draw_frames(&mut i, 300);
    }

    #[test]
    fn the_quick_settings_page_lists_the_devices_and_their_battery() {
        let mut i = Island::new(Config::default());
        let h0 = i.qs_layout().height;
        i.data.devices = vec![headphones(Some(85))];
        assert_eq!(i.qs_layout().height, h0 + 10.0 + 32.0, "one row");
        i.data.devices = vec![headphones(Some(85)), headphones(Some(12)), headphones(None), headphones(Some(5))];
        assert_eq!(i.qs_layout().height, h0 + 10.0 + 3.0 * 32.0 + 2.0 * 6.0, "three at most");
        i.push(T0, Input::OpenPage(Page::Qs));
        draw_frames(&mut i, 300);
        i.data.devices.clear();
        assert_eq!(i.qs_layout().height, h0, "and nothing when none is connected");
    }

    #[test]
    fn the_home_page_is_left_as_it_was() {
        let mut i = Island::new(Config::default());
        let h0 = i.face_size(Face::IdleHover).1;
        i.data.devices = vec![headphones(Some(85))];
        assert_eq!(i.face_size(Face::IdleHover).1, h0);
    }

    #[test]
    fn device_icons_exist_in_the_bundled_font() {
        use island_core::bluetooth::Kind::*;
        for k in [Earbuds, Headphones, Headset, Keyboard, Mouse, Gamepad, Other] {
            assert!(crate::is_known_icon(k.icon()), "{}", k.icon());
        }
    }

    fn forecast(now: i64, rain_at_h: Option<i64>) -> Weather {
        let hours = (0..48)
            .map(|k| {
                let t = (now / 3_600_000 - 12 + k) * 3_600_000;
                let wet = rain_at_h.is_some_and(|h| t >= now + h * 3_600_000 && t < now + (h + 2) * 3_600_000);
                island_core::weather::Hour { time_ms: t, temp_c: 12.0, code: if wet { 61 } else { 2 }, is_day: true, rain_prob: if wet { 80 } else { 0 }, mm: if wet { 1.0 } else { 0.0 } }
            })
            .collect();
        Weather { temp_c: 12.0, feels_c: 10.0, code: 2, day: true, high_c: 15.0, low_c: 7.0, rain_slots: (0..8).map(|k| island_core::weather::Slot { offset_min: k * 15, mm: 0.0 }).collect(), hours }
    }

    #[test]
    fn the_weather_page_exists_once_there_is_a_forecast_and_the_home_weather_opens_it() {
        let mut i = Island::new(Config::default());
        assert!(!i.pages().contains(&Page::Weather), "no forecast, no page");
        i.data.weather = Some(forecast(T0, None));
        let pages = i.pages();
        let at = pages.iter().position(|p| *p == Page::Weather).unwrap();
        assert_eq!(pages[at - 1], Page::Cal, "after the calendar");
        i.push(T0, Input::OpenPage(Page::Weather));
        draw_frames(&mut i, 300);
        assert_eq!(i.current_face(), Face::Weather);
        i.push(i.now, Input::OpenPage(Page::Home));
        draw_frames(&mut i, 300);
        assert!(press(&mut i, Action::Open(Page::Weather)), "the weather block on the home page is a button");
        draw_frames(&mut i, 300);
        assert_eq!(i.current_face(), Face::Weather);
    }

    #[test]
    fn rain_is_told_in_a_way_that_fits_the_distance() {
        use island_core::weather::Outlook::*;
        let mut i = Island::new(Config::default());
        i.data.utc_offset_secs = 0;
        i.now = 14 * 3_600_000; // 14:00
        assert_eq!(i.rain_text(Rain { in_min: 25 }, false).as_deref(), Some("Rain in 25 min"));
        assert_eq!(i.rain_text(Rain { in_min: 180 }, false).as_deref(), Some("Rain in 3 h"));
        assert_eq!(i.rain_text(Rain { in_min: 180 }, true).as_deref(), Some("Rain around 17:00 · in 3 h"));
        assert_eq!(i.rain_text(Rain { in_min: 25 }, true).as_deref(), Some("Rain in 25 min"), "near rain needs no clock time");
        assert_eq!(i.rain_text(Raining { clears_in_min: Some(40) }, true).as_deref(), Some("Raining · clears in 40 min"));
        assert_eq!(i.rain_text(Raining { clears_in_min: None }, false).as_deref(), Some("Raining now"));
        assert_eq!(i.rain_text(Dry, true), None);
        assert_eq!(Island::span_text(1), "1 min");
        assert_eq!(Island::span_text(95), "1 h 40 min", "rounded to ten minutes");
    }

    #[test]
    fn the_outlook_follows_the_forecast_and_home_only_cares_when_it_is_near() {
        use island_core::weather::{worth_showing_at_home, Outlook};
        let mut i = Island::new(Config::default());
        i.now = T0;
        assert_eq!(i.rain_outlook(), None, "no forecast");
        i.data.weather = Some(forecast(T0, None));
        assert_eq!(i.rain_outlook(), Some(Outlook::Dry));
        i.data.weather = Some(forecast(T0, Some(3)));
        let o = i.rain_outlook().unwrap();
        assert!(matches!(o, Outlook::Rain { in_min } if (170..=190).contains(&in_min)), "{o:?}");
        assert!(worth_showing_at_home(o));
        i.data.weather = Some(forecast(T0, Some(9)));
        assert!(!worth_showing_at_home(i.rain_outlook().unwrap()), "nine hours away is not a home page matter");
    }

    #[test]
    fn a_level_that_arrives_in_steps_slides_instead_of_jumping() {
        let mut i = Island::new(Config::default());
        i.data.volume = Some(0.30);
        run(&mut i, 100);
        assert!((i.volume_shown - 0.30).abs() < 1e-6, "the first reading is not slid to");
        i.data.volume = Some(0.60);
        let fr = i.tick(i.now + 8, 8.0);
        assert!(i.volume_shown > 0.30 && i.volume_shown < 0.40, "one frame moves a little: {}", i.volume_shown);
        assert!(fr.animating, "and frames keep coming at the display rate until it arrives");
        let mut shown = vec![i.volume_shown];
        for _ in 0..80 {
            i.tick(i.now + 8, 8.0);
            shown.push(i.volume_shown);
        }
        assert!(shown.windows(2).all(|w| w[1] >= w[0]), "monotonic, no overshoot");
        assert!((*shown.last().unwrap() - 0.60).abs() < 1e-6, "it lands on the level");
    }

    #[test]
    fn a_slider_under_the_pointer_follows_it_exactly() {
        let mut i = Island::new(Config::default());
        i.data.volume = Some(0.30);
        run(&mut i, 100);
        i.dragging = Some(Action::VolumeBar);
        i.data.volume = Some(0.90);
        i.tick(i.now + 8, 8.0);
        assert!((i.volume_shown - 0.90).abs() < 1e-6, "no lag behind the finger");
        i.dragging = None;
        i.data.brightness = Some(0.5);
        i.tick(i.now + 8, 8.0);
        assert!(i.brightness_shown <= 0.5);
    }

    #[test]
    fn the_volume_bar_starts_from_the_level_it_had() {
        let mut i = Island::new(Config::default());
        i.data.volume = Some(0.50);
        run(&mut i, 100);
        i.push(i.now, Input::Osd { kind: OsdKind::Volume, level: 0.56, muted: false });
        assert!((i.osd_shown - 0.50).abs() < 1e-6, "it starts where the bar last was");
        draw_frames(&mut i, 60);
        assert!((i.osd_shown - 0.56).abs() < 0.001, "and slides to the new one: {}", i.osd_shown);
        // muting slides to empty
        i.push(i.now, Input::Osd { kind: OsdKind::Volume, level: 0.56, muted: true });
        draw_frames(&mut i, 80);
        assert!(i.osd_shown < 0.01);
    }

    #[test]
    fn the_options_page_flips_switches_and_steps_numbers() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::OpenPage(Page::Options));
        draw_frames(&mut i, 300);
        assert_eq!(i.current_face(), Face::Options);
        // a switch on the first tab
        assert!(press(&mut i, Action::Toggle("clock_in_pill")));
        assert!(!i.cfg.clock_in_pill, "the island follows at once");
        assert_eq!(i.take_commands(), vec![Command::SetOption { key: "clock_in_pill".into(), value: OptionValue::Bool(false) }]);
        // another tab
        assert!(press(&mut i, Action::OptionsTab(1)));
        draw_frames(&mut i, 300);
        assert!(press(&mut i, Action::Step("notification_seconds", 1)));
        assert_eq!(i.cfg.notification_ms, 6000);
        assert_eq!(i.take_commands(), vec![Command::SetOption { key: "notification_seconds".into(), value: OptionValue::Num(6.0) }]);
        assert!(press(&mut i, Action::Step("osd_seconds", -1)));
        assert_eq!(i.cfg.osd_ms, 1500);
        // the ends of the range hold
        i.cfg.notification_ms = 30_000;
        i.activate(Action::Step("notification_seconds", 1));
        assert_eq!(i.cfg.notification_ms, 30_000);
        // the page changes height with the tab
        assert_ne!(i.face_size(Face::Options).1, { i.activate(Action::OptionsTab(2)); i.face_size(Face::Options).1 });
    }

    #[test]
    fn options_that_switch_a_behaviour_off_do_so() {
        let n = |id: u32| Input::Notification(Incoming { id, app: "a".into(), summary: "s".into(), ..Default::default() });
        // no cards, but the history keeps them
        let mut i = Island::new(Config { show_notifications: false, ..Config::default() });
        i.push(T0, n(1));
        assert!(i.notif.is_none() && i.history.len() == 1);
        // no volume bar
        let mut j = Island::new(Config { show_osd: false, ..Config::default() });
        j.push(T0, Input::Osd { kind: OsdKind::Volume, level: 0.5, muted: false });
        run(&mut j, 600);
        assert_ne!(j.current_face(), Face::Osd);
        let mut k = Island::new(Config::default());
        k.push(T0, Input::Osd { kind: OsdKind::Volume, level: 0.5, muted: false });
        run(&mut k, 600);
        assert_eq!(k.current_face(), Face::Osd, "and with it on, it shows");
    }

    #[test]
    fn scrolling_over_the_small_pill_changes_the_volume_and_the_middle_button_mutes() {
        let mut i = Island::new(Config::default());
        i.data.volume = Some(0.50);
        run(&mut i, 600);
        assert!(i.current_face().is_compact());
        let cmds = i.scroll(i.now, 0.0, 2.0);
        assert!(matches!(cmds.as_slice(), [Command::SetVolume(v)] if (*v - 0.54).abs() < 1e-6), "{cmds:?}");
        let cmds = i.scroll(i.now, 0.0, -50.0);
        assert!(matches!(cmds.as_slice(), [Command::SetVolume(v)] if *v == 0.0), "down to zero, no further");
        assert_eq!(i.middle_click(i.now), vec![Command::ToggleMute]);
        // not while a page is open: there the middle button does nothing
        i.push(i.now, Input::OpenPage(Page::Cal));
        run(&mut i, 600);
        assert!(i.middle_click(i.now).is_empty());
    }

    #[test]
    fn a_sideways_swipe_over_the_pill_skips_a_track_once() {
        let mut i = Island::new(Config::default());
        i.data = data();
        run(&mut i, 600);
        assert_eq!(i.current_face(), Face::MediaMini);
        assert!(i.scroll(i.now, 1.0, 0.0).is_empty(), "a small movement is added up, not acted on");
        assert!(i.scroll(i.now, 1.0, 0.0).is_empty());
        assert_eq!(i.scroll(i.now, 1.5, 0.0), vec![Command::MediaNext], "past the threshold: next");
        assert!(i.scroll(i.now + 100, 5.0, 0.0).is_empty(), "the rest of the same swipe does nothing");
        i.now += 800;
        assert_eq!(i.scroll(i.now, -4.0, 0.0), vec![Command::MediaPrevious], "the other way: previous");
        // without music a sideways swipe does nothing
        let mut j = Island::new(Config::default());
        run(&mut j, 600);
        assert!(j.scroll(j.now, 9.0, 0.0).is_empty());
    }

    #[test]
    fn an_app_can_be_hidden_from_the_history_and_rules_are_listed_and_removed() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::Notification(Incoming { id: 1, app: "Zen Browser".into(), summary: "unseen".into(), ..Default::default() }));
        i.activate(Action::Dismiss); // the card first, then the page behind it
        i.take_commands();
        i.push(i.now, Input::OpenPage(Page::Notifs));
        draw_frames(&mut i, 300);
        assert!(press(&mut i, Action::MuteApp(0)), "a bell on the row");
        assert_eq!(i.take_commands(), vec![Command::AddFilter { app: "Zen Browser".into(), content: String::new(), hide: true }]);
        // the confirmation card away, then the rules tab
        i.activate(Action::Dismiss);
        i.take_commands();
        i.data.config.filters = vec![FilterView { app: "Zen Browser".into(), content: String::new(), show: false, sound: false }, FilterView { app: "Telegram".into(), content: String::new(), show: true, sound: false }];
        i.push(i.now, Input::OpenPage(Page::Options));
        i.activate(Action::OptionsTab(crate::options::RULES_TAB));
        draw_frames(&mut i, 300);
        assert!(press(&mut i, Action::RemoveRule(1)));
        assert_eq!(i.take_commands(), vec![Command::RemoveFilter(1)]);
        // typing an app name into the add row
        i.activate(Action::EditField(Field::RuleSilence));
        i.push(i.now, Input::Key(KeyInput::Text("Mail".into())));
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert_eq!(i.take_commands(), vec![Command::AddFilter { app: "Mail".into(), content: String::new(), hide: false }]);
        // an empty name adds nothing
        i.activate(Action::EditField(Field::RuleHide));
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert!(i.take_commands().is_empty());
        assert_eq!(i.face_size(Face::Options).1, 14.0 + 34.0 + 40.0 + 4.0 * 34.0 + 14.0 + DOCK_H, "two rules and two add rows, and the dock");
    }

    fn media_page(playing: bool) -> Island {
        let mut i = Island::new(Config::default());
        i.data = data();
        i.data.media.as_mut().unwrap().playing = playing;
        i.push(T0, Input::OpenPage(Page::Media));
        draw_frames(&mut i, 300);
        assert!(matches!(i.current_face(), Face::MediaFull | Face::MediaLyric));
        i
    }

    #[test]
    fn the_play_pause_button_answers_at_once_and_the_icons_cross_fade() {
        let mut i = media_page(false);
        assert_eq!((i.play_anim, i.playing_shown()), (0.0, false));
        assert!(press(&mut i, Action::Cmd(Command::MediaPlayPause)));
        assert_eq!(i.take_commands(), vec![Command::MediaPlayPause], "the player is asked once");
        assert!(i.playing_shown(), "it shows playing before the player has said so");
        assert!(i.disc_press > 0.9, "and the disc gives under the press");
        i.tick(i.now + 8, 8.0);
        assert!(i.play_anim > 0.0 && i.play_anim < 0.5, "one frame in: part of the way ({})", i.play_anim);
        draw_frames(&mut i, 40);
        assert!(i.play_anim > 0.99, "and then the pause icon is fully there");
        assert!(i.disc_press < 0.05, "the press has died away");
        // the player agrees: nothing changes, the override is dropped
        i.data.media.as_mut().unwrap().playing = true;
        draw_frames(&mut i, 5);
        assert!(i.play_override.is_none() && i.playing_shown());
    }

    #[test]
    fn a_press_the_player_never_confirms_is_taken_back() {
        let mut i = media_page(true);
        assert!(press(&mut i, Action::Cmd(Command::MediaPlayPause)));
        assert!(!i.playing_shown());
        draw_frames(&mut i, 40);
        assert!(i.play_anim < 0.1, "shown as paused: {}", i.play_anim);
        i.now += 2000; // the player still says playing
        draw_frames(&mut i, 40);
        assert!(i.playing_shown(), "back to what the player says");
        assert!(i.play_anim > 0.9);
    }

    #[test]
    fn a_skip_button_nudges_its_icon_and_comes_back() {
        let mut i = media_page(true);
        assert!(press(&mut i, Action::Cmd(Command::MediaNext)));
        assert_eq!(i.take_commands(), vec![Command::MediaNext]);
        assert!(i.skip_press > 0.9, "{}", i.skip_press);
        draw_frames(&mut i, 120);
        assert!(i.skip_press.abs() < 0.01, "back in place: {}", i.skip_press);
        assert!(press(&mut i, Action::Cmd(Command::MediaPrevious)));
        assert!(i.skip_press < -0.9, "the other way for previous: {}", i.skip_press);
    }

    #[test]
    fn the_disc_swells_under_the_pointer() {
        let mut i = media_page(true);
        let disc = i.hits.iter().find(|h| h.action == Action::Cmd(Command::MediaPlayPause)).unwrap().rect;
        assert_eq!(i.disc_hover, 0.0);
        i.pointer_moved(i.now, disc.center().x + i.pill_origin.0, disc.center().y + i.pill_origin.1);
        draw_frames(&mut i, 30);
        assert!(i.disc_hover > 0.95, "{}", i.disc_hover);
        i.pointer_left(i.now);
        draw_frames(&mut i, 30);
        assert!(i.disc_hover < 0.05);
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
        i.data.night_light = Some(false);
        i.activate(Action::Cmd(Command::ToggleNightLight));
        assert_eq!(i.data.night_light, Some(true));
        i.data.night_light = None;
        i.activate(Action::Cmd(Command::ToggleNightLight));
        assert_eq!(i.data.night_light, None, "no wlsunset: the chip stays hidden");
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
        i.data.config.calendars = vec![CalendarView { name: "A".into(), detail: "a.org".into(), color: (10, 132, 255) }; 6];
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

    #[test]
    fn the_calendar_always_opens_on_today() {
        let mut i = Island::new(Config::default());
        i.data.utc_offset_secs = 3600;
        // reached by a swipe: the page is set without the Open action
        i.page = Some(Page::Cal);
        i.expand_until = T0 + 5000;
        run(&mut i, 400);
        let c = crate::clock::civil(i.now, 3600);
        assert_eq!((i.cal.year, i.cal.month, i.cal.day), (c.year, c.month, c.day));
        // browsing to another month, closing and opening again returns to today
        i.activate(Action::CalNext);
        assert_ne!((i.cal.year, i.cal.month), (c.year, c.month));
        i.page = None;
        i.expand_until = 0;
        run(&mut i, 3000);
        i.page = Some(Page::Cal);
        i.expand_until = i.now + 5000;
        run(&mut i, 400);
        assert_eq!((i.cal.year, i.cal.month, i.cal.day), (c.year, c.month, c.day));
    }

    fn draw_frames(i: &mut Island, n: usize) -> (f64, f64) {
        let f = fonts();
        let mut surface = (0.0, 0.0);
        for _ in 0..n {
            let fr = i.tick(i.now + 8, 8.0);
            let mut scene = Scene::new();
            i.draw(&mut scene, &f, fr.surface, 1.0);
            surface = fr.surface;
        }
        surface
    }

    #[test]
    fn pages_carry_a_dock_that_jumps_straight_to_a_page() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::OpenPage(Page::Qs));
        draw_frames(&mut i, 200);
        let gotos: Vec<Page> = i.hits.iter().filter_map(|h| if let Action::Goto(p) = h.action { Some(p) } else { None }).collect();
        assert_eq!(gotos, vec![Page::Home, Page::Today, Page::Cal, Page::Qs, Page::Sys, Page::Clip, Page::Options, Page::Settings], "no media, timer, notifications or forecast yet");
        let home = i.hits.iter().find(|h| h.action == Action::Goto(Page::Sys)).unwrap().rect;
        i.pointer_down(i.now, home.center().x + i.pill_origin.0, home.center().y + i.pill_origin.1);
        assert_eq!(i.page, Some(Page::Sys));
        assert!(i.page_stack.is_empty(), "the dock does not add to the back history");
        draw_frames(&mut i, 200);
        assert_eq!(i.current_face(), Face::Sys);
        // a card without a page has no dock
        let mut j = Island::new(Config::default());
        j.push(T0, Input::StartTimer(60));
        j.push(j.now, Input::Notification(Incoming { id: 1, ..Default::default() }));
        draw_frames(&mut j, 200);
        assert!(!j.hits.iter().any(|h| matches!(h.action, Action::Goto(_))));
    }

    #[test]
    fn the_dock_lists_live_activities_first_and_grows_the_face() {
        let mut i = Island::new(Config::default());
        i.data = data();
        i.push(T0, Input::StartTimer(600));
        assert_eq!(i.pages()[..2], [Page::Media, Page::Timer]);
        assert_eq!(i.face_size(Face::Qs).1, i.qs_layout().height + DOCK_H, "room for the dock");
        assert_eq!(i.face_size(Face::Idle).1, 28.0, "the compact pill has none");
    }

    #[test]
    fn back_returns_to_the_page_that_led_here() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::OpenPage(Page::Home));
        i.activate(Action::Open(Page::Cal));
        i.activate(Action::Open(Page::Settings));
        assert_eq!(i.page, Some(Page::Settings));
        i.activate(Action::Back);
        assert_eq!(i.page, Some(Page::Cal));
        i.activate(Action::Back);
        assert_eq!(i.page, Some(Page::Home));
        i.activate(Action::Back);
        assert_eq!(i.page, None, "nothing left: the island goes back to what it shows by default");
    }

    #[test]
    fn next_and_previous_page_wrap_and_the_content_slides_the_right_way() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::OpenPage(Page::Home));
        draw_frames(&mut i, 150);
        i.push(i.now, Input::CyclePage(1));
        assert_eq!(i.page, Some(Page::Today));
        i.tick(i.now + 8, 8.0);
        assert_eq!(i.slide, 1.0, "forward slides in from the right");
        draw_frames(&mut i, 150);
        i.push(i.now, Input::CyclePage(-1));
        i.push(i.now, Input::CyclePage(-1));
        assert_eq!(i.page, Some(Page::Settings), "wraps around to the last page");
        i.tick(i.now + 8, 8.0);
        assert_eq!(i.slide, 1.0, "Home -> Settings is still forward in the dock's order");
        draw_frames(&mut i, 150);
        i.push(i.now, Input::CyclePage(-1));
        i.tick(i.now + 8, 8.0);
        assert_eq!(i.slide, -1.0);
    }

    #[test]
    fn tab_changes_page_in_keyboard_mode() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::OpenKeyboard(Some(Page::Home)));
        draw_frames(&mut i, 150);
        i.push(i.now, Input::Key(KeyInput::NextPage));
        assert_eq!(i.page, Some(Page::Today));
        i.push(i.now, Input::Key(KeyInput::PrevPage));
        i.push(i.now, Input::Key(KeyInput::PrevPage));
        assert_eq!(i.page, Some(Page::Settings));
        assert_eq!(i.kb.unwrap().index, 0, "the focus ring starts again on the new page");
    }

    /// Rests the pointer on the pill (`bubble` = on the side bubble) and lets the intent delay pass.
    fn hover(i: &mut Island, bubble: bool) {
        let (px, py, _, ph) = i.pill_rect();
        let x = if bubble { px + i.spring.w.pos + BUBBLE_GAP + BUBBLE_W / 2.0 } else { px + i.spring.w.pos / 2.0 };
        i.pointer_moved(i.now, x, py + ph / 2.0);
        draw_frames(i, 60);
    }

    #[test]
    fn hovering_the_timer_bubble_opens_the_timer_not_the_music() {
        let mut i = Island::new(Config::default());
        i.data = data();
        i.push(T0, Input::StartTimer(600));
        draw_frames(&mut i, 200);
        assert_eq!((i.current_face(), i.shown_side), (Face::MediaMini, Some(Side::Timer)));
        hover(&mut i, true);
        assert_eq!(i.page, Some(Page::Timer));
        assert_eq!(i.current_face(), Face::TimerFull, "the bubble's activity opens");
    }

    #[test]
    fn hovering_the_pill_opens_its_own_activity() {
        let mut i = Island::new(Config::default());
        i.data = data();
        i.push(T0, Input::StartTimer(600));
        draw_frames(&mut i, 200);
        hover(&mut i, false);
        assert_eq!(i.page, Some(Page::Media));
        assert_eq!(i.current_face(), Face::MediaFull);
    }

    #[test]
    fn the_pointer_crossing_from_the_pill_to_the_bubble_before_it_opens_picks_the_bubble() {
        let mut i = Island::new(Config::default());
        i.data = data();
        i.push(T0, Input::StartTimer(600));
        draw_frames(&mut i, 200);
        let (px, py, _, ph) = i.pill_rect();
        i.pointer_moved(i.now, px + 20.0, py + ph / 2.0);
        let bx = px + i.spring.w.pos + BUBBLE_GAP + BUBBLE_W / 2.0;
        i.pointer_moved(i.now + 20, bx, py + ph / 2.0);
        draw_frames(&mut i, 60);
        assert_eq!(i.page, Some(Page::Timer));
        // leaving forgets it: the next visit starts from the pill again
        i.pointer_left(i.now);
        i.expand_until = 0;
        draw_frames(&mut i, 200);
        hover(&mut i, false);
        assert_eq!(i.page, Some(Page::Media));
    }

    #[test]
    fn a_lone_timer_or_meeting_pill_opens_itself_and_an_idle_pill_opens_home() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::StartTimer(600));
        draw_frames(&mut i, 200);
        hover(&mut i, false);
        assert_eq!(i.current_face(), Face::TimerFull);
        let mut j = Island::new(Config::default());
        draw_frames(&mut j, 100);
        hover(&mut j, false);
        assert_eq!(j.current_face(), Face::IdleHover);
    }

    #[test]
    fn a_click_on_the_pill_and_a_click_on_the_bubble_open_different_pages() {
        let mut i = Island::new(Config::default());
        i.data = data();
        i.push(T0, Input::StartTimer(600));
        draw_frames(&mut i, 200);
        let (px, py, _, ph) = i.pill_rect();
        let y = py + ph / 2.0;
        // the left part of the pill used to be covered by the bubble's hit region
        i.pointer_down(i.now, px + 20.0, y);
        assert_eq!(i.page, Some(Page::Media), "a click on the pill opens the music");
        i.activate(Action::Back);
        i.page = None;
        i.expand_until = 0;
        draw_frames(&mut i, 300);
        let (px, py, _, ph) = i.pill_rect();
        i.pointer_down(i.now, px + i.spring.w.pos + BUBBLE_GAP + BUBBLE_W / 2.0, py + ph / 2.0);
        assert_eq!(i.page, Some(Page::Timer), "a click on the bubble opens the timer");
        for h in &i.hits {
            assert!(h.rect.x1 <= i.spring.w.pos + BUBBLE_GAP + BUBBLE_W + 1.0, "no hit outside the group");
        }
    }

    #[test]
    fn adding_an_icloud_account_asks_for_the_apple_id_then_the_app_password() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::OpenPage(Page::Settings));
        i.activate(Action::EditField(Field::ICloudUser));
        type_text(&mut i, "not an email");
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert!(i.take_commands().is_empty() && i.edit.as_ref().unwrap().field == Field::ICloudUser, "an Apple ID is an email address");
        i.push(i.now, Input::Key(KeyInput::Home));
        for _ in 0..12 {
            i.push(i.now, Input::Key(KeyInput::Delete));
        }
        type_text(&mut i, "me@icloud.com");
        i.push(i.now, Input::Key(KeyInput::Enter));
        let e = i.edit.as_ref().expect("the password field opens");
        assert_eq!(e.field, Field::ICloudPassword);
        assert!(e.secret(), "the password is drawn as dots");
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert!(i.take_commands().is_empty() && i.edit.is_some(), "an empty password is refused");
        type_text(&mut i, "abcd-efgh-ijkl-mnop");
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert_eq!(i.take_commands(), vec![Command::AddICloud { user: "me@icloud.com".into(), password: "abcd-efgh-ijkl-mnop".into() }]);
        assert!(i.edit.is_none());
        // escape in the middle forgets the Apple ID
        i.activate(Action::EditField(Field::ICloudUser));
        type_text(&mut i, "a@b.co");
        i.push(i.now, Input::Key(KeyInput::Enter));
        i.push(i.now, Input::Key(KeyInput::Escape));
        i.activate(Action::EditField(Field::ICloudPassword));
        type_text(&mut i, "x");
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert!(i.take_commands().is_empty(), "no Apple ID left: nothing is sent");
    }

    #[test]
    fn calendars_can_be_renamed_and_recoloured() {
        let mut i = Island::new(Config::default());
        i.data.config.calendars = vec![CalendarView { name: "Uni".into(), detail: "x.org".into(), color: (255, 69, 58) }];
        i.push(T0, Input::OpenPage(Page::Settings));
        i.activate(Action::EditField(Field::CalendarName(0)));
        assert_eq!(i.edit.as_ref().unwrap().text, "Uni", "starts from the current name");
        for _ in 0..3 {
            i.push(i.now, Input::Key(KeyInput::Backspace));
        }
        type_text(&mut i, "Lezioni");
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert_eq!(i.take_commands(), vec![Command::RenameCalendar { index: 0, name: "Lezioni".into() }]);
        i.activate(Action::PickColor(0));
        assert_eq!(i.picking, Some(0));
        i.activate(Action::PickColor(0));
        assert_eq!(i.picking, None, "a second click closes the choice");
        i.activate(Action::PickColor(0));
        assert_eq!(i.activate(Action::SetColor(0, (48, 209, 88))), vec![Command::SetCalendarColor { index: 0, color: (48, 209, 88) }]);
        assert_eq!(i.picking, None);
    }

    #[test]
    fn the_settings_page_draws_calendars_pickers_and_secret_fields() {
        let f = fonts();
        let mut i = Island::new(Config::default());
        i.data.config.calendars = (0..3).map(|k| CalendarView { name: if k == 1 { String::new() } else { format!("Cal {k}") }, detail: "iCloud · me@icloud.com".into(), color: (10, 132, 255) }).collect();
        i.push(T0, Input::OpenPage(Page::Settings));
        i.activate(Action::PickColor(1));
        for _ in 0..200 {
            let fr = i.tick(i.now + 8, 8.0);
            i.push(i.now, Input::OpenPage(Page::Settings));
            let mut scene = Scene::new();
            i.draw(&mut scene, &f, fr.surface, 1.0);
            assert!(fr.surface.1 < 1000.0);
        }
        assert!(i.hits.iter().filter(|h| matches!(h.action, Action::SetColor(1, _))).count() == 8, "eight swatches");
        i.picking = None;
        i.activate(Action::EditField(Field::ICloudUser));
        type_text(&mut i, "me@icloud.com");
        i.push(i.now, Input::Key(KeyInput::Enter));
        type_text(&mut i, "secret");
        draw_frames(&mut i, 5);
    }

    #[test]
    fn opening_the_calendar_with_stale_events_asks_for_a_refresh_once() {
        let mut i = Island::new(Config::default());
        i.data.config.calendars = vec![CalendarView { name: "A".into(), detail: "a.org".into(), color: (1, 2, 3) }];
        i.data.events_synced_at = T0 - 600_000;
        i.push(T0, Input::OpenPage(Page::Cal));
        run(&mut i, 300);
        let asks = |i: &mut Island| i.take_commands().iter().filter(|c| **c == Command::RefreshCalendars).count();
        assert_eq!(asks(&mut i), 1, "stale: read again");
        i.page = None;
        i.expand_until = 0;
        run(&mut i, 3000);
        i.push(i.now, Input::OpenPage(Page::Cal));
        run(&mut i, 300);
        assert_eq!(asks(&mut i), 0, "not again within 30 seconds");
        // fresh events and no calendars never ask
        let mut j = Island::new(Config::default());
        j.data.config.calendars = i.data.config.calendars.clone();
        j.data.events_synced_at = T0 - 1000;
        j.push(T0, Input::OpenPage(Page::Cal));
        run(&mut j, 300);
        assert_eq!(asks(&mut j), 0);
        let mut k = Island::new(Config::default());
        k.push(T0, Input::OpenPage(Page::Cal));
        run(&mut k, 300);
        assert_eq!(asks(&mut k), 0, "nothing to read");
    }

    #[test]
    fn the_refresh_button_asks_at_once() {
        let mut i = Island::new(Config::default());
        assert_eq!(i.activate(Action::Cmd(Command::RefreshCalendars)), vec![Command::RefreshCalendars]);
        assert!(i.note.is_some());
    }

    #[test]
    fn the_lyrics_scroll_one_line_but_a_jump_just_lands() {
        let mut i = Island::new(Config { lyrics_in_pill: true, ..Config::default() });
        i.data = data();
        let lrc: String = (1..=40).map(|n| format!("[{:02}:{:02}.00] line {n}\n", n * 3 / 60, n * 3 % 60)).collect();
        i.data.lyrics_lrc = Some(lrc);
        i.data.media.as_mut().unwrap().position_ms = 10_000.0;
        i.data.media.as_mut().unwrap().sampled_at_ms = T0;
        run(&mut i, 600);
        let idx = i.lyric_idx;
        // the next line comes up: a scroll in progress
        i.data.media.as_mut().unwrap().position_ms = 13_000.0;
        i.data.media.as_mut().unwrap().sampled_at_ms = i.now;
        run(&mut i, 16);
        assert_eq!(i.lyric_idx, idx + 1);
        assert!(i.lyric_at > 0, "one line on is animated");
        // the island wakes after a long pause (the screen was locked): thirty lines on, no scroll through them
        i.now += 1000;
        i.data.media.as_mut().unwrap().position_ms = 100_000.0;
        i.data.media.as_mut().unwrap().sampled_at_ms = i.now;
        run(&mut i, 16);
        assert!(i.lyric_idx > idx + 20);
        assert!(i.lyric_at < 0, "a jump has no animation to play");
        assert!(i.lyric_strip().is_some_and(|s| s.iter().any(|(_, slot)| *slot == 0.0)), "the current line sits in its place");
    }

    #[test]
    fn the_pill_can_show_the_line_being_sung() {
        let f = fonts();
        let mut i = Island::new(Config { lyrics_in_pill: true, ..Config::default() });
        i.data = data();
        i.data.lyrics_lrc = Some("[00:01.00] First line\n[00:30.00] Second line".into());
        i.data.media.as_mut().unwrap().position_ms = 5000.0;
        i.data.media.as_mut().unwrap().sampled_at_ms = T0;
        run(&mut i, 600);
        assert_eq!(i.current_face(), Face::MediaMini);
        assert_eq!(i.lyric_now().map(|(t, _)| t).as_deref(), Some("First line"));
        let mut scene = Scene::new();
        let fr = i.tick(i.now + 8, 8.0);
        i.draw(&mut scene, &f, fr.surface, 1.0);
        // time moves on past the second line: the entrance restarts from nothing
        i.data.media.as_mut().unwrap().position_ms = 29_900.0;
        i.data.media.as_mut().unwrap().sampled_at_ms = i.now;
        run(&mut i, 200);
        let (t, e) = i.lyric_now().unwrap();
        assert_eq!(t, "Second line");
        assert!(e < 1.0, "still coming in");
        run(&mut i, 600);
        assert_eq!(i.lyric_now().unwrap().1, 1.0);
        // a blank line (an instrumental gap) shows nothing
        let mut j = Island::new(Config::default());
        j.data = data();
        j.data.lyrics_lrc = Some("[00:01.00] \n[00:30.00] x".into());
        j.data.media.as_mut().unwrap().position_ms = 5000.0;
        j.data.media.as_mut().unwrap().sampled_at_ms = T0;
        run(&mut j, 600);
        assert!(j.lyric_now().is_none());
    }

    #[test]
    fn the_outline_follows_the_pill_and_its_bubble() {
        let mut i = Island::new(Config::default());
        draw_frames(&mut i, 100);
        let r = i.shape_rects();
        assert_eq!(r.len(), 1);
        assert_eq!((r[0].2, r[0].3, r[0].4), (150.0, 28.0, 14.0), "a 150 x 28 pill with full-round ends");
        i.data = data();
        i.push(T0, Input::StartTimer(60));
        draw_frames(&mut i, 300);
        let r = i.shape_rects();
        assert_eq!(r.len(), 2, "the bubble is part of the outline");
        assert!(r[1].0 > r[0].0 + r[0].2, "to the right of the pill, past the gap");
    }

    #[test]
    fn quick_settings_hide_what_the_machine_does_not_have() {
        let mut i = Island::new(Config::default());
        // a bare system: DND and keep awake only, no brightness, no outputs
        assert_eq!(i.qs_chips().len(), 2);
        let bare = i.qs_layout();
        assert!(bare.bri_cy.is_none() && bare.out_top.is_none());
        i.data = data();
        i.data.wifi = Some(true);
        i.data.bluetooth = Some(false);
        i.data.profile = Some("balanced".into());
        i.data.night_light = Some(false);
        i.data.brightness = Some(0.5);
        i.data.sinks = vec![Sink { name: "a".into(), label: "A".into() }];
        let full = i.qs_layout();
        assert_eq!(i.qs_chips().len(), 6);
        assert!(full.height > bare.height + 44.0 + 36.0, "two chip rows, a brightness row and the output row");
        assert!(full.bri_cy.is_some() && full.out_top.is_some());
        // the page draws at every size without escaping its own height
        let f = fonts();
        for (page_data, label) in [(Data::default(), "bare"), (i.data.clone(), "full")] {
            let mut j = Island::new(Config::default());
            j.data = page_data;
            j.push(T0, Input::OpenPage(Page::Qs));
            for _ in 0..150 {
                let fr = j.tick(j.now + 8, 8.0);
                let mut scene = Scene::new();
                j.draw(&mut scene, &f, fr.surface, 1.0);
            }
            assert_eq!(j.current_face(), Face::Qs, "{label}");
        }
    }

    /// Presses the middle of the first drawn control with this action.
    fn press(i: &mut Island, action: Action) -> bool {
        let Some(h) = i.hits.iter().find(|h| h.action == action).map(|h| h.rect) else { return false };
        let cmds = i.pointer_down(i.now, h.center().x + i.pill_origin.0, h.center().y + i.pill_origin.1);
        i.pending.extend(cmds);
        true
    }

    #[test]
    fn the_x_on_a_notification_in_the_list_removes_only_that_one() {
        let mut i = Island::new(Config::default());
        for n in 0..3u32 {
            i.history.push((Incoming { id: n, app: "a".into(), summary: format!("n{n}"), ..Default::default() }, T0));
        }
        i.push(T0, Input::OpenPage(Page::Notifs));
        draw_frames(&mut i, 200);
        assert!(press(&mut i, Action::HistoryRemove(1)), "the x of the second row is drawn");
        assert_eq!(i.history.iter().map(|(n, _)| n.summary.as_str()).collect::<Vec<_>>(), ["n0", "n2"]);
        draw_frames(&mut i, 10);
        assert!(press(&mut i, Action::HistoryRemove(0)));
        assert_eq!(i.history.len(), 1, "and the first one");
    }

    #[test]
    fn the_clipboard_page_pins_and_forgets_an_entry() {
        let mut i = Island::new(Config::default());
        i.data.clipboard = vec!["one".into(), "two".into(), "three".into()];
        i.push(T0, Input::OpenPage(Page::Clip));
        draw_frames(&mut i, 200);
        assert!(press(&mut i, Action::ClipPin(1)), "a star on each row");
        assert!(i.take_commands().contains(&Command::PinClipboard("two".into())));
        draw_frames(&mut i, 10);
        assert!(press(&mut i, Action::ClipForget(2)), "and an x");
        assert!(i.take_commands().contains(&Command::ForgetClipboard("three".into())));
        i.data.clipboard = (0..12).map(|n| n.to_string()).collect();
        draw_frames(&mut i, 200);
        assert!(i.hits.iter().any(|h| h.action == Action::ClipPin(CLIP_ROWS - 1)));
        assert!(!i.hits.iter().any(|h| h.action == Action::ClipPin(CLIP_ROWS)), "only the first rows are listed");
    }

    #[test]
    fn the_sound_page_picks_devices_and_sets_the_volume_of_an_app() {
        let mut i = Island::new(Config::default());
        let sink = |n: &str| Sink { name: n.into(), label: n.into() };
        i.data.sinks = vec![sink("speaker"), sink("bluez_output.x")];
        i.data.sink = Some("speaker".into());
        i.data.sources = vec![sink("mic")];
        i.data.source = Some("mic".into());
        i.data.streams = vec![AppStream { id: 7, name: "Spotify".into(), volume: 60, muted: false }];
        i.push(T0, Input::OpenPage(Page::Audio));
        draw_frames(&mut i, 200);
        assert_eq!(i.current_face(), Face::Audio);
        assert!(press(&mut i, Action::Cmd(Command::SetSink("bluez_output.x".into()))), "an output row");
        assert!(i.take_commands().contains(&Command::SetSink("bluez_output.x".into())));
        draw_frames(&mut i, 10);
        assert!(press(&mut i, Action::Cmd(Command::SetStreamVolume { id: 7, percent: 70 })), "plus raises by ten");
        draw_frames(&mut i, 10);
        assert!(press(&mut i, Action::Cmd(Command::SetStreamVolume { id: 7, percent: 50 })), "minus lowers by ten");
        draw_frames(&mut i, 10);
        assert!(press(&mut i, Action::Cmd(Command::ToggleStreamMute(7))));
        let (_, h) = draw_frames(&mut i, 200);
        assert!(h >= i.audio_height(), "the page is as tall as its rows");
    }

    #[test]
    fn the_output_row_of_quick_settings_opens_the_sound_page() {
        let mut i = Island::new(Config::default());
        i.data.sinks = vec![Sink { name: "a".into(), label: "A".into() }, Sink { name: "b".into(), label: "B".into() }];
        i.data.sink = Some("a".into());
        i.push(T0, Input::OpenPage(Page::Qs));
        draw_frames(&mut i, 200);
        assert!(press(&mut i, Action::Open(Page::Audio)));
        draw_frames(&mut i, 200);
        assert_eq!(i.current_face(), Face::Audio);
    }

    #[test]
    fn the_today_page_lists_events_and_reminders_with_a_now_line() {
        let mut i = Island::new(Config::default());
        let t = crate::clock::civil(T0, 0);
        let at = |h: i64| crate::clock::midnight_ms(t.year, t.month, t.day, 0) + h * 3_600_000;
        let now = at(10);
        let ev = |title: &str, s: i64, e: i64| Event { id: title.into(), title: title.into(), start_ms: at(s), end_ms: at(e), all_day: false, url: None, color: None, calendar: "c".into() };
        i.data.events = vec![ev("early", 1, 2), ev("later", 16, 17)];
        i.reminders = vec![Reminder { due_ms: at(14), text: "call".into() }];
        i.push(now, Input::OpenPage(Page::Today));
        i.now = now;
        let (items, now_at, _) = i.today_view();
        assert_eq!(items.iter().map(|x| x.title.as_str()).collect::<Vec<_>>(), ["early", "call", "later"]);
        assert_eq!(now_at, 1, "the now line is after the finished event");
        assert!(i.pages().windows(2).any(|w| w == [Page::Home, Page::Today]), "the dock shows it after the home page");
        draw_frames(&mut i, 200);
        assert_eq!(i.current_face(), Face::Today);
    }

    #[test]
    fn files_to_send_open_a_page_of_devices_and_a_click_sends_them() {
        let mut i = Island::new(Config::default());
        let peer = |a: &str| island_core::localsend::Peer { alias: a.into(), ip: "10.0.0.2".into(), port: 53317, https: true, model: "Pixel 8".into(), kind: "mobile".into() };
        i.push(T0, Input::SendFiles(vec![]));
        assert!(i.take_commands().is_empty(), "no files, no page");
        i.push(T0, Input::SendFiles(vec!["/tmp/a.txt".into(), "/tmp/b.txt".into()]));
        assert!(i.take_commands().contains(&Command::ScanPeers), "the network is scanned at once");
        i.data.peers_scanning = true;
        draw_frames(&mut i, 200);
        assert_eq!(i.current_face(), Face::Send);
        i.data.peers = vec![peer("Pixel"), peer("Laptop")];
        i.data.peers_scanning = false;
        draw_frames(&mut i, 200);
        assert!(press(&mut i, Action::SendTo(1)));
        let cmds = i.take_commands();
        assert!(cmds.contains(&Command::SendFiles { peer: peer("Laptop"), paths: vec!["/tmp/a.txt".into(), "/tmp/b.txt".into()] }));
        assert!(i.send_files.is_empty(), "the files went");
        draw_frames(&mut i, 300);
        assert_ne!(i.current_face(), Face::Send, "the page closes once the files are on their way");
    }

    #[test]
    fn the_pill_reminds_earlier_than_the_card_when_asked() {
        let mut cfg = Config::default();
        cfg.meeting_lead_ms = 10 * 60_000;
        cfg.meeting_pill_ms = 45 * 60_000;
        let mut i = Island::new(cfg);
        i.data.events = vec![Event { id: "e".into(), title: "Seminar".into(), start_ms: T0 + 30 * 60_000, end_ms: T0 + 60 * 60_000, all_day: false, url: None, color: None, calendar: "c".into() }];
        i.push(T0, Input::OpenKeyboard(None));
        i.now = T0;
        assert!(i.soon().is_some(), "30 min away: inside the 45 min of the pill");
        assert!(i.announcer.new_heads_up(&i.data.events, T0, i.cfg.meeting_lead_ms).is_none(), "but outside the 10 min of the card");
        i.cfg.meeting_pill_ms = 20 * 60_000;
        assert!(i.soon().is_none(), "a shorter pill lead hides it");
    }

    #[test]
    fn the_x_on_the_notification_card_skips_to_the_next_one() {
        let mut i = Island::new(Config::default());
        let n = |id: u32, app: &str| Input::Notification(Incoming { id, app: app.into(), summary: format!("n{id}"), ..Default::default() });
        i.push(T0, n(1, "a"));
        i.push(T0, n(2, "b"));
        draw_frames(&mut i, 200);
        assert_eq!(i.notif.as_ref().map(|n| n.id), Some(1));
        assert!(press(&mut i, Action::Dismiss), "the x is drawn on the card");
        assert!(i.take_commands().iter().any(|c| matches!(c, Command::NotificationClosed { id: 1, reason: CloseReason::Dismissed })));
        draw_frames(&mut i, 400);
        assert_eq!(i.notif.as_ref().map(|n| n.id), Some(2), "the next one is shown");
    }

    #[test]
    fn a_button_inside_a_row_beats_the_row_in_every_page() {
        let mut i = Island::new(Config::default());
        i.data.config.calendars = vec![CalendarView { name: "A".into(), detail: "a.org".into(), color: (1, 2, 3) }];
        i.push(T0, Input::OpenPage(Page::Settings));
        draw_frames(&mut i, 200);
        assert!(press(&mut i, Action::RemoveCalendar(0)), "the calendar's x");
        assert!(i.take_commands().contains(&Command::RemoveCalendar(0)));
        assert!(i.edit.is_none(), "and not the rename field under it");
    }

    #[test]
    fn clicking_a_notification_goes_to_the_app_that_sent_it() {
        let mut i = Island::new(Config::default());
        let tg = Incoming { id: 4, app: "Telegram Desktop".into(), summary: "Anna".into(), desktop_entry: "org.telegram.desktop".into(), actions: vec![NotifAction { key: "default".into(), label: "".into() }], ..Default::default() };
        i.push(T0, Input::Notification(tg));
        draw_frames(&mut i, 200);
        assert!(press(&mut i, Action::NotifOpen), "the card body is a hit");
        let cmds = i.take_commands();
        assert_eq!(cmds[0], Command::InvokeAction { id: 4, key: "default".into() }, "the sender's own default action first");
        assert_eq!(cmds[1], Command::FocusApp { desktop_entry: "org.telegram.desktop".into(), app: "Telegram Desktop".into() });
        assert!(cmds.iter().any(|c| matches!(c, Command::NotificationClosed { id: 4, .. })), "and the card goes away");
        assert!(i.notif.is_none());
        // a sender without a default action still gets its window raised
        i.push(i.now, Input::Notification(Incoming { id: 5, app: "Zen".into(), summary: "site".into(), desktop_entry: "zen".into(), ..Default::default() }));
        draw_frames(&mut i, 200);
        assert!(press(&mut i, Action::NotifOpen));
        let cmds = i.take_commands();
        assert_eq!(cmds[0], Command::FocusApp { desktop_entry: "zen".into(), app: "Zen".into() });
        assert!(!cmds.iter().any(|c| matches!(c, Command::InvokeAction { .. })));
    }

    #[test]
    fn clicking_a_row_of_the_history_focuses_its_app_and_removes_it() {
        let mut i = Island::new(Config::default());
        i.restore_history(vec![(Incoming { id: 1, app: "Mail".into(), summary: "a".into(), desktop_entry: "mail".into(), ..Default::default() }, 0), (Incoming { id: 2, app: "Zen".into(), summary: "b".into(), desktop_entry: "zen".into(), ..Default::default() }, 0)]);
        i.push(T0, Input::OpenPage(Page::Notifs));
        draw_frames(&mut i, 200);
        i.activate(Action::HistoryOpen(1));
        assert_eq!(i.take_commands(), vec![Command::FocusApp { desktop_entry: "zen".into(), app: "Zen".into() }]);
        assert_eq!(i.history.len(), 1, "an opened notification is read");
    }

    #[test]
    fn a_single_notification_card_has_an_x_that_dismisses_it() {
        let mut i = Island::new(Config::default());
        i.push(T0, Input::Notification(Incoming { id: 9, app: "a".into(), summary: "alone".into(), ..Default::default() }));
        draw_frames(&mut i, 200);
        assert!(i.queue.is_empty() && i.notif.is_some(), "nothing else waits");
        assert!(press(&mut i, Action::Dismiss), "the x is there even for a lone notification");
        assert!(i.take_commands().iter().any(|c| matches!(c, Command::NotificationClosed { id: 9, reason: CloseReason::Dismissed })));
        assert!(i.notif.is_none());
        // and one with buttons (an actions card) has it too
        i.push(i.now, Input::Notification(Incoming { id: 10, actions: vec![NotifAction { key: "open".into(), label: "Open".into() }], ..Default::default() }));
        draw_frames(&mut i, 300);
        assert!(press(&mut i, Action::Dismiss));
        assert!(i.notif.is_none());
    }
}
