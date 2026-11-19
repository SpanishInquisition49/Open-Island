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

/// A connected Bluetooth device (headphones, a mouse...).
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub kind: island_core::bluetooth::Kind,
    pub battery: Option<u8>,
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
    /// The hours of today and tomorrow.
    pub hours: Vec<island_core::weather::Hour>,
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

/// Where the machine is.
#[derive(Clone, Debug, PartialEq)]
pub struct Position {
    pub lat: f64,
    pub lon: f64,
    /// Radius of doubt in metres (0 when it is a place the person set).
    pub accuracy_m: f64,
    pub at_ms: i64,
    pub source: PosSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PosSource {
    /// GeoClue (Wi-Fi or GeoIP, as the system sees it).
    Geoclue,
    /// The location set for the weather.
    Settings,
}

/// What it takes to get to an event by one way of travelling.
#[derive(Clone, Debug, PartialEq)]
pub struct TripInfo {
    pub event_id: String,
    pub mode: island_core::travel::Mode,
    pub trip: Option<island_core::travel::Trip>,
    /// Why there is no trip.
    pub error: Option<String>,
    pub at_ms: i64,
    /// The coordinates of the place, once found.
    pub dest: Option<(f64, f64)>,
}

/// An application recording from the microphone.
#[derive(Clone, Debug, PartialEq)]
pub struct MicUser {
    /// The PipeWire/PulseAudio stream (source output) index.
    pub stream: u32,
    pub name: String,
    pub pid: Option<u32>,
    pub muted: bool,
}

/// A program that is hogging the machine.
#[derive(Clone, Debug, PartialEq)]
pub struct Culprit {
    pub name: String,
    pub pids: Vec<u32>,
    /// CPU in cores (1.0 is one core fully busy).
    pub cores: f32,
    pub mem_mb: f32,
    /// True when memory is what runs short, not the CPU.
    pub memory: bool,
    /// The session lives on it: it is shown but cannot be stopped from the island.
    pub protected: bool,
}

/// An app that is playing sound, with its own volume.
#[derive(Clone, Debug, PartialEq)]
pub struct AppStream {
    pub id: u32,
    pub name: String,
    pub volume: u8,
    pub muted: bool,
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

/// A notification rule: matches by app and text, and hides the notification or only silences it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FilterView {
    pub app: String,
    pub content: String,
    /// false: the notification is dropped.
    pub show: bool,
    /// false: it shows without a sound.
    pub sound: bool,
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
    /// What the user told the island to hide or silence.
    pub filters: Vec<FilterView>,
    /// The timer presets as typed (`5m, 10m, 25m`).
    pub timer_presets: String,
    /// "focused", or the name of the screen the island is pinned to.
    pub screen: String,
    /// The map tile URL as set (empty: the default), and whether an API key is stored (the key itself never leaves the app).
    pub map_tile_url: String,
    pub map_key_set: bool,
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
    /// Connected Bluetooth devices.
    pub devices: Vec<Device>,
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
    /// Audio inputs, the default one, and the apps that are playing (the audio page).
    pub sources: Vec<Sink>,
    pub source: Option<String>,
    pub streams: Vec<AppStream>,
    /// Devices that can receive files (LocalSend) and whether the network is still being scanned.
    pub peers: Vec<island_core::localsend::Peer>,
    pub peers_scanning: bool,
    /// The program hogging the CPU or the memory, and the programs the island paused.
    pub culprit: Option<Culprit>,
    pub paused: Vec<String>,
    /// Whether the network works, and where it breaks when it does not; whether a VPN is up.
    pub net: island_core::netstate::Health,
    pub vpn: bool,
    /// Trips to events with a place, as the service worked them out.
    pub trips: Vec<TripInfo>,
    pub position: Option<Position>,
    /// Who is using the microphone and the camera right now (read while the system page is open).
    pub mic_users: Vec<MicUser>,
    pub camera_users: Vec<(String, Vec<u32>)>,
    pub system: Option<SystemStats>,
    pub config: ConfigView,
    /// None when there is no focus script on this machine.
    pub focus: Option<Focus>,
    /// Text copied recently, newest first (password-manager entries are never in it).
    pub clipboard: Vec<String>,
    /// The entries the user pinned (they are also in `clipboard`, first).
    pub clipboard_pinned: Vec<String>,
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
    /// A Bluetooth device just connected.
    DeviceConnected(Device),
    /// A connected device's battery is low.
    DeviceLow(Device),
    /// A script's activity: running, done or failed (see `open-island activity`).
    Activity(island_core::progress::State, island_core::progress::Activity),
    /// Open the command bar (from a shortcut).
    OpenBar,
    /// The person is back after this many milliseconds without touching the keyboard or the pointer.
    Returned { away_ms: i64 },
    /// No keyboard or pointer input for this long (the person is probably away).
    Idled { after_ms: i64 },
    /// A map tile arrived.
    Tile { z: u8, x: i64, y: i64, image: Image },
    /// The window with the focus changed (app id and title).
    Focused { app: String, title: String },
    /// Files to send: opens the page that lists the devices (from `open-island send`, or a file dropped on the island).
    SendFiles(Vec<String>),
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
/// The value of an option.
#[derive(Clone, Debug, PartialEq)]
pub enum OptionValue {
    Bool(bool),
    Num(f64),
    Text(String),
}

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
    /// Make this output (a sink name) the default.
    SetSink(String),
    SetSource(String),
    SetStreamVolume { id: u32, percent: u32 },
    ToggleStreamMute(u32),
    /// Look for devices to send files to.
    ScanPeers,
    /// Freeze a program (SIGSTOP); it is remembered by name so it can be resumed.
    PauseProcs { name: String, pids: Vec<u32> },
    ResumeProcs(String),
    /// Ask programs to quit (SIGTERM).
    QuitProcs(Vec<u32>),
    /// Switch the connection off and on again.
    ReconnectNetwork,
    /// Look for where the machine is.
    RefreshPosition,
    /// Fetch a map tile (the template has {z} {x} {y}).
    LoadTile { z: u8, x: i64, y: i64, template: String, dark: bool },
    /// Mute or unmute one application's microphone stream.
    MuteMicStream { stream: u32, mute: bool },
    /// Work out how to get to an event that arrives by `start_ms` (the place is sent to public map services).
    PlanTrip { event_id: String, location: String, start_ms: i64, mode: island_core::travel::Mode, margin_secs: i64 },
    /// Send these files (paths) to a device.
    SendFiles { peer: island_core::localsend::Peer, paths: Vec<String> },
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
    /// Change one option of the options page; the platform layer writes it to the configuration.
    SetOption { key: String, value: OptionValue },
    /// Add a notification rule (an app name and/or a piece of text); `hide` drops the notifications, otherwise they only lose their sound.
    AddFilter { app: String, content: String, hide: bool },
    RemoveFilter(usize),
    StartFocus(u32),
    StopFocus,
    /// Put this text back on the clipboard.
    RestoreClipboard(String),
    /// Pin a clipboard entry (or unpin it when it is pinned).
    PinClipboard(String),
    /// Copy a one-time code; it leaves the clipboard (and is never kept in the history) after thirty seconds.
    CopyCode(String),
    /// Drop a clipboard entry from the history.
    ForgetClipboard(String),
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
