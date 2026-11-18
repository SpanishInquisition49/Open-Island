//! What the platform layer feeds the island (data it samples) and what the island asks back (commands).

use island_core::calendar::Event;
use island_core::weather::Slot;
use std::sync::Arc;

/// A decoded image (album art, notification picture): straight RGBA8.
#[derive(Clone)]
pub struct Image {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<Vec<u8>>,
}

impl Image {
    /// Average colour of the opaque pixels, for the album-art tint.
    pub fn average(&self) -> Option<(u8, u8, u8)> {
        let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
        for px in self.rgba.as_chunks::<4>().0.iter().step_by(7) {
            if px[3] > 200 {
                r += px[0] as u64;
                g += px[1] as u64;
                b += px[2] as u64;
                n += 1;
            }
        }
        (n > 0).then(|| ((r / n) as u8, (g / n) as u8, (b / n) as u8))
    }
}

#[derive(Clone, Default)]
pub struct Media {
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    /// Position at `sampled_at_ms` (epoch ms); the island extrapolates while playing.
    pub position_ms: f64,
    pub sampled_at_ms: i64,
    pub length_ms: f64,
    pub art: Option<Image>,
    pub can_seek: bool,
    /// How many players are open (the switch button shows when there is more than one).
    pub players: u32,
    /// The player's name for display ("Spotify", "Firefox").
    pub player: String,
}

#[derive(Clone, Copy, Default, PartialEq)]
pub struct Battery {
    pub percent: f32,
    pub charging: bool,
    pub on_ac: bool,
}

#[derive(Clone, Default)]
pub struct Weather {
    pub temp_c: f32,
    pub feels_c: f32,
    pub code: i32,
    pub day: bool,
    pub high_c: f32,
    pub low_c: f32,
    pub rain_slots: Vec<Slot>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Privacy {
    pub mic: bool,
    pub camera: bool,
    pub screen: bool,
}

/// An audio output.
#[derive(Clone, Debug, PartialEq)]
pub struct Sink {
    pub name: String,
    pub label: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gpu {
    pub pstate: String,
    pub watts: f32,
    pub temp_c: f32,
    pub util: f32,
}

/// The numbers of the system page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemStats {
    pub cpu_percent: f32,
    pub cpu_temp_c: Option<f32>,
    pub cpu_ghz: Option<f32>,
    pub cores: Vec<f32>,
    pub memory: Option<island_core::sysmon::Memory>,
    pub gpu: Option<Gpu>,
    pub battery_watts: Option<f32>,
    /// Minutes to empty (discharging) or to full (charging).
    pub battery_eta_min: Option<u64>,
    pub net_rx: f64,
    pub net_tx: f64,
    /// (used MB, total MB) of the root filesystem.
    pub disk: Option<(f64, f64)>,
    pub load: Option<[f32; 3]>,
}

/// A calendar source as the settings menu lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct CalendarView {
    pub name: String,
    /// The feed's host, or the account ("iCloud · me@icloud.com").
    pub detail: String,
    pub color: (u8, u8, u8),
}

/// The settings the menu shows and changes (the platform layer owns the file).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigView {
    pub font: String,
    /// The configured font was not found.
    pub font_missing: bool,
    pub calendars: Vec<CalendarView>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub notification_sound: String,
}

/// The study mode (`~/.local/bin/focus`): sites blocked and notifications silenced until `end_epoch`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Focus {
    pub active: bool,
    /// Epoch seconds of the planned end, when known.
    pub end_epoch: Option<i64>,
}

/// Everything sampled from the system, replaced as a whole whenever something changes.
#[derive(Clone, Default)]
pub struct Data {
    pub media: Option<Media>,
    pub battery: Option<Battery>,
    pub weather: Option<Weather>,
    pub events: Vec<Event>,
    /// When the calendars were last read successfully (epoch ms; 0 = not yet).
    pub events_synced_at: i64,
    pub privacy: Privacy,
    pub volume: Option<f32>,
    pub muted: bool,
    pub brightness: Option<f32>,
    pub dnd: bool,
    pub wifi: Option<bool>,
    pub bluetooth: Option<bool>,
    /// The idle inhibitor is held ("keep awake").
    pub keep_awake: bool,
    /// Night light (wlsunset): None when it is not installed.
    pub night_light: Option<bool>,
    /// "performance", "balanced" or "powersave".
    pub profile: Option<String>,
    pub sinks: Vec<Sink>,
    pub sink: Option<String>,
    pub system: Option<SystemStats>,
    pub config: ConfigView,
    /// None when there is no focus script on this machine.
    pub focus: Option<Focus>,
    /// Text copied recently, newest first (password-manager entries are never in it).
    pub clipboard: Vec<String>,
    /// Lyrics of the current track, as LRC text (the platform layer fetches them).
    pub lyrics_lrc: Option<String>,
    pub utc_offset_secs: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NotifAction {
    pub key: String,
    pub label: String,
}

#[derive(Clone, Default)]
pub struct Incoming {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    pub critical: bool,
    pub actions: Vec<NotifAction>,
    /// The sender's picture or the notification's own image (a photo, a cover).
    pub image: Option<Image>,
    /// The application's icon, used when there is no picture.
    pub app_icon: Option<Image>,
    /// The sender's requested timeout in ms: None = default, Some(0) = until dismissed.
    pub expire_ms: Option<i64>,
    /// A progress value (0..=100) the notification carries (volume, downloads).
    pub progress: Option<u8>,
    /// Stays in the history even when dismissed from the screen (and is not removed by an action).
    pub resident: bool,
    /// The sender's desktop entry (`org.telegram.desktop`), used to find its window.
    pub desktop_entry: String,
}

impl Incoming {
    /// The action key of an inline reply field, and its placeholder, when the sender offers one.
    pub fn reply_action(&self) -> Option<&NotifAction> {
        self.actions.iter().find(|a| a.key == "inline-reply")
    }
}

/// Why a notification left the screen (the values of the freedesktop `NotificationClosed` signal).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseReason {
    Expired = 1,
    Dismissed = 2,
    Closed = 3,
}

/// A key press for the inline reply field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyInput {
    Text(String),
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    Up,
    Down,
    /// Tab / Page Down: the next page; Shift+Tab / Page Up: the previous one.
    NextPage,
    PrevPage,
    Enter,
    Escape,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OsdKind {
    Volume,
    Brightness,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PowerEvent {
    Plugged,
    Unplugged,
    Low,
}

/// One-off events from the platform layer.
pub enum Input {
    Notification(Incoming),
    NotificationClosed(u32),
    Osd { kind: OsdKind, level: f32, muted: bool },
    Power(PowerEvent),
    /// "<percent|?> [label]" (see island_core::progress)
    Progress(String),
    ProgressDone(String),
    ProgressFail(String),
    ProgressClear,
    /// Open the command bar (from a shortcut).
    OpenBar,
    /// A shell started a command: it becomes a progress pill when it runs longer than `after_secs`.
    CommandStart { id: String, label: String, after_secs: u32 },
    /// The shell command ended with this exit code.
    CommandEnd { id: String, code: i32 },
    /// The shell that started a command is gone: forget the command without a word.
    CommandGone(String),
    StartTimer(u64),
    StartPomodoro,
    StartStopwatch,
    CancelTimer,
    Key(KeyInput),
    /// Reply to the notification on screen (from a shortcut).
    StartReply,
    /// Open the island for keyboard use (arrows move the focus, Enter activates, Escape closes).
    OpenKeyboard(Option<island_core::face::Page>),
    /// Levels (0..=1) of the audio spectrum bands, for the equalizer.
    Spectrum(Vec<f32>),
    /// A line for the settings page ("Font not found"); shown for a few seconds.
    SettingsNote(String),
    /// Open the island for a few seconds ("peek").
    Peek,
    /// Do not disturb on/off, from a script (focus mode).
    SetDnd(bool),
    /// A click at this point of the surface (logical pixels), as if made with the pointer: for scripts and tests.
    Click(f64, f64),
    /// Write every control currently drawn, with its position, to the log.
    DumpHits,
    /// The next (1) or previous (-1) page of the open island (shortcut, IPC).
    CyclePage(i32),
    /// Open the island on a page (shortcut, IPC).
    OpenPage(island_core::face::Page),
}

/// What the island asks the platform layer to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    MediaPrevious,
    MediaPlayPause,
    MediaNext,
    MediaSeek { position_ms: f64 },
    /// Show the next open player.
    MediaCyclePlayer,
    SetVolume(f32),
    SetBrightness(f32),
    ToggleMute,
    SetDnd(bool),
    ToggleWifi,
    ToggleBluetooth,
    ToggleKeepAwake,
    ToggleNightLight,
    NextProfile,
    NextSink,
    /// Re-read the quick-settings state (the page was just opened).
    RefreshQuick,
    /// Read the calendars again now.
    RefreshCalendars,
    /// A calendar feed address (https://, http:// or webcal://).
    AddCalendar(String),
    /// Add an iCloud account: the Apple ID and an app-specific password.
    AddICloud { user: String, password: String },
    RenameCalendar { index: usize, name: String },
    SetCalendarColor { index: usize, color: (u8, u8, u8) },
    RemoveCalendar(usize),
    SetFont(String),
    SetLatitude(f64),
    SetLongitude(f64),
    SetNotificationSound(String),
    StartFocus(u32),
    StopFocus,
    /// Put this text back on the clipboard.
    RestoreClipboard(String),
    OpenUrl(String),
    InvokeAction { id: u32, key: String },
    /// Bring the window of the app that sent a notification to the front (starting it when it is not open).
    FocusApp { desktop_entry: String, app: String },
    CloseNotification(u32),
    /// The island closed a notification (expired on screen, dismissed): tell the sender.
    NotificationClosed { id: u32, reason: CloseReason },
    /// The text typed in an inline reply.
    NotificationReplied { id: u32, text: String },
    /// The timer ended.
    PlaySound,
}
