//! The island's state machine: which face is shown, the shape spring, hover and clicks, timers,
//! notifications, long operations. It samples nothing and draws nothing itself: the platform layer
//! feeds it `Data`, `Input` events and a clock, and the faces module draws it.

use crate::model::*;
use crate::paint::Hit;
use island_core::calendar::{self, Announcer};
use island_core::duration;
use island_core::face::{self, Face, Page, Side, Snapshot};
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
/// The widest the compact pill grows to show a line of lyrics.
const LYRIC_PILL_MAX: f64 = 640.0;
const SNOOZE_MS: i64 = 10 * 60_000;
/// Notifications the island makes itself (confirmations, reminders, the recap) have ids from here.
const OWN_ID_BASE: u32 = 3_000_000;
/// The cards a script asks for (`open-island ok ...`, `run`, the agent hook) take their ids from here.
pub const SCRIPT_ID_BASE: u32 = 5_000_000;
/// The longest a card that waits for the user stays, should nothing click or replace it.
pub(crate) const STAY_MS: i64 = 600_000;
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
    /// Work out how to get to an event with a place (the place is sent to public map services).
    pub travel_times: bool,
    pub travel_mode: island_core::travel::Mode,
    /// How much earlier than needed to leave.
    pub travel_margin_ms: i64,
    /// How many small pills may sit beside the main one (1-4): a timer, focus, an event, each running activity.
    pub mini_pills: u32,
    /// How many page icons the dock shows (Home and the most used ones); the rest are on the grid.
    pub dock_icons: u32,
    /// Where the map tiles come from, with {z} {x} {y}.
    pub map_tile_url: String,
    /// Darken the (light) tiles to suit the black island.
    pub map_dark: bool,
    /// Weather, trips and the map use where the machine is; off: the coordinates set in the settings.
    pub follow_position: bool,
    /// Look the position up from the nearby Wi-Fi networks (they are sent to beacondb.net). Off by default.
    pub wifi_position: bool,
    /// Notifications are held after this many minutes in a row on work apps (0: never).
    pub deep_work_minutes: i64,
    /// Names of apps that count as work (part of the name, any case).
    pub deep_work_apps: Vec<String>,
    /// A card after this many minutes on the same app without a break (0: never).
    pub break_minutes: i64,
    pub meeting_alerts: bool,
    pub rain_alerts: bool,
    pub battery_alerts: bool,
    pub low_battery: f32,
    pub media_auto_expand: bool,
    pub clock_in_pill: bool,
    pub status_icons: bool,
    pub lyrics: bool,
    pub clipboard_history: bool,
    /// A copied link, address or e-mail drops a card that opens it.
    pub link_cards: bool,
    /// A drop beside the pill fills with the progress of a long job, or drains with a countdown.
    pub progress_drop: bool,
    /// The current lyric line in the small pill instead of the title.
    pub lyrics_in_pill: bool,
    pub timer_presets: Vec<u64>,
    /// Notifications are held (not shown) during a calendar event / while the microphone or the screen share is on.
    pub quiet_in_meetings: bool,
    pub quiet_on_mic: bool,
    /// A card with what came in while the person was away, when they come back.
    pub away_recap: bool,
    /// Fetch the ECB's daily rates for currency conversions in the command bar.
    pub currency_rates: bool,
    /// Show browser downloads (read from the download folder) as activities.
    pub watch_downloads: bool,
    /// A screenshot's card offers to copy the text in it (OCR).
    pub ocr_screenshots: bool,
    /// Minutes after midnight when the card with tomorrow comes (0: off), and whether it comes on Saturday and Sunday too.
    pub evening_recap_min: i64,
    /// Door-to-door minutes to where lectures are (0: off): the cards say when to leave, and one comes at that moment.
    pub commute_minutes: i64,
    /// The same for the card with today, shown from this time until 14:00.
    pub morning_recap_min: i64,
    pub evening_weekends: bool,
    /// A card when one program hogs the CPU or the memory.
    pub culprit_alerts: bool,
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
            travel_times: true,
            travel_mode: island_core::travel::Mode::Car,
            travel_margin_ms: 5 * 60_000,
            mini_pills: 2,
            dock_icons: 7,
            map_tile_url: island_core::map::DEFAULT_TILES.to_string(),
            map_dark: true,
            follow_position: true,
            wifi_position: false,
            deep_work_minutes: 20,
            deep_work_apps: island_core::activity::DEEP_WORK_APPS.iter().map(|s| s.to_string()).collect(),
            break_minutes: 0,
            meeting_alerts: true,
            rain_alerts: true,
            battery_alerts: true,
            low_battery: 15.0,
            media_auto_expand: true,
            clock_in_pill: true,
            status_icons: true,
            lyrics: true,
            clipboard_history: true,
            link_cards: true,
            progress_drop: true,
            lyrics_in_pill: false,
            timer_presets: vec![300, 600, 1500],
            quiet_in_meetings: true,
            quiet_on_mic: true,
            away_recap: true,
            currency_rates: true,
            watch_downloads: true,
            ocr_screenshots: true,
            evening_recap_min: 0,
            commute_minutes: 0,
            morning_recap_min: 0,
            evening_weekends: false,
            culprit_alerts: true,
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
/// Size of the map on the map page, in logical pixels.
pub const MAP_W: f64 = 392.0;
pub const MAP_H: f64 = 190.0;

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
    /// Ticks off (removes) the to-do at this position.
    TodoDone(usize),
    /// Puts the to-do at this position off by this many days.
    TodoPostpone(usize, i64),
    /// Opens the bar to change the to-do at this position.
    TodoEdit(usize),
    /// Removes the to-do at this position.
    TodoRemove(usize),
    /// Closes the menu of a row.
    TodoMenuClose,
    /// A row of the search page: open the file or note, copy the rest.
    FindOpen(usize),
    /// Opens a file found for the lecture.
    OpenFile(String),
    /// Opens the conversation at this position of the iPhone's list.
    PhoneThread(usize),
    /// Back from a conversation or a search to the list.
    PhoneList,
    /// Write to the contact at this position of the search result (its first number).
    PhoneContact(usize),
    /// Answer, hang up or turn call control on.
    PhoneCallDo(PhoneCall),
    /// Choose where the phone's audio plays.
    PhoneAudioDo(bool),
    /// Copies the pin at this position.
    PinCopy(usize),
    /// Takes the pin at this position off.
    PinRemove(usize),
    /// Removes the deadline at this position of the soonest-first list.
    DeadlineDone(usize),
    /// Cancels one of the countdowns running beside the main one.
    TimerCancelExtra(usize),
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
    /// Something done on one card of the stack (its notification's id).
    OnCard(u32, Box<Action>),
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
    /// Zoom the map in or out.
    MapZoom(i8),
    /// Switch the map between "around me" and "me and the next event".
    MapView,
    /// Drag the map: the press starts it, the pointer moves it.
    MapPan,
    /// Back to following the position.
    MapRecenter,
    /// Pick a place of the search results.
    MapPick(usize),
    /// The pin of home: the way there from where you are.
    MapHome,
    /// Forget the picked place and its route.
    MapClearDest,
    MapCopy,
    MapOpen,
    /// Directions to the next event, in the browser.
    MapRoute,
    MapRefresh,
    ActivityTab(usize),
    /// Put a line of the rewind page back on the clipboard.
    RewindCopy(usize),
    /// How to get to the next event with a place.
    TravelMode(island_core::travel::Mode),
    /// The action of a clipboard entry (open the link, write the mail, show the place).
    ClipAct(usize),
    CopyCode(String),
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
    /// What the user called it (`10m tea`), empty when nothing.
    pub label: String,
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

/// A card a script asked for.
pub(crate) fn is_script(n: &Incoming) -> bool {
    (SCRIPT_ID_BASE..SCRIPT_ID_BASE + 1_000_000).contains(&n.id)
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
    /// Which earlier line Up / Down has brought back (None: the one being typed, kept in `draft`).
    pub recall: Option<usize>,
    pub draft: String,
}

/// Something that happened, for the rewind page.
#[derive(Clone, Debug, PartialEq)]
pub struct LogEntry {
    pub at_ms: i64,
    pub kind: LogKind,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogKind {
    Notification,
    Track,
    Copy,
}

/// A line of the rewind page.
#[derive(Clone, Debug, PartialEq)]
pub struct RewindRow {
    pub at_ms: i64,
    pub icon: &'static str,
    pub text: String,
    /// The text to put back on the clipboard when the row is clicked.
    pub copy: Option<String>,
}

/// What the map page draws.
#[derive(Clone, Debug, PartialEq)]
pub struct MapView {
    pub center: (f64, f64),
    pub zoom: u8,
    pub me: (f64, f64),
    pub accuracy_m: f64,
    /// The position is the one set for the weather, not a fix.
    pub from_settings: bool,
    /// The next event's place and its title.
    pub dest: Option<((f64, f64), String)>,
}

/// Something to do, from the command bar, a script or a server: a day, a course, a time estimate, a repetition.
pub use island_core::task::Task as Todo;

/// Something to take along, for the lectures of a course (or any day).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Bring {
    pub text: String,
    /// A word of the event title (case does not matter); empty: every day.
    pub course: String,
}

/// A date to count down to: an exam, a project hand-in.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Deadline {
    /// Days since 1970-01-01 (local).
    pub day: i64,
    pub text: String,
    /// What to study for it (a word of an event title), and the minutes of work still to do: the study plan is made of these.
    pub course: String,
    pub est_min: u32,
}

/// A reminder set from the command bar.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Reminder {
    pub due_ms: i64,
    pub text: String,
    /// A reminder that comes back: when it fires it is set again for the next day it falls on.
    pub repeat: island_core::task::Repeat,
}

/// The answer to a search: what was asked and what was found so far.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    pub query: String,
    pub hits: Vec<island_core::plan::Hit>,
    /// The notes and files are still being read.
    pub searching: bool,
}

/// A setting being edited in the settings menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Calendar,
    /// Rename the calendar at this index.
    CalendarName(usize),
    ICloudUser,
    ICloudPassword,
    /// The three steps of adding a CalDAV account: its address, the user, the password.
    CalDavServer,
    CalDavUser,
    CalDavPassword,
    Font,
    Latitude,
    Longitude,
    /// Where home is: coordinates or an address (empty clears it).
    Home,
    /// The usual train: "pisa centrale 17:12 verso lucca" (empty unpins it).
    Train,
    /// A reply to the iPhone conversation that is open.
    PhoneReply,
    /// A name to look for in the phone's contacts.
    PhoneSearch,
    Sound,
    TimerPresets,
    Screen,
    /// The map tile URL (empty: OpenStreetMap's).
    MapTiles,
    /// The API key of the map tile provider (kept in the config file, never shown).
    MapKey,
    /// A place to search on the map.
    MapSearch,
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
        matches!(self.field, Field::ICloudPassword | Field::CalDavPassword | Field::MapKey)
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

/// A window this big holds every page (with its overshoot and the side bubble): a platform that cannot resize its
/// window without a hitch keeps one of this size and only changes which part of it takes the pointer.
pub const MAX_SURFACE: (f64, f64) = (820.0, 1080.0);

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
    /// The small pills on their way out, resting or on their way back.
    pub(crate) bubbles: Vec<liquid::Bubble>,
    /// The drop beside the pill that shows a progress or a countdown.
    pub(crate) fill: Option<liquid::Fill>,
    /// The short animations of single events.
    pub(crate) fx: effects::Effects,
    pub cfg: Config,
    pub data: Data,
    pub now: i64,
    pub(crate) spring: Spring,
    pub(crate) shown: Face,
    pub(crate) shown_side: Option<Side>,
    /// The small pills after the first one, in order.
    pub(crate) shown_more: Vec<Side>,
    want_more: Vec<Side>,
    want: Face,
    want_side: Option<Side>,
    pub(crate) alpha: f64,
    from_size: (f64, f64),
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
    /// Countdowns running beside `timer`, soonest first.
    pub(crate) extra_timers: Vec<Timer>,
    pub(crate) todos: Vec<Todo>,
    /// Tasks finished here whose server copy may still say open for one more sync (so they do not come back).
    pub(crate) tasks_done: Vec<String>,
    /// The task (by uid) whose row shows its menu: put off, edit, remove.
    pub row_menu: Option<String>,
    /// A line too long for its place is being scrolled (set while drawing; the next tick asks for frames), and what
    /// scrolls with the moment it began.
    pub(crate) marquee_on: std::cell::Cell<bool>,
    pub(crate) marquee_since: std::cell::RefCell<(String, i64)>,
    /// Study time done: (day, course, minutes), kept for a few months.
    pub(crate) study_log: Vec<(i64, String, u32)>,
    pub(crate) found: Found,
    /// Notes kept on the Today page until removed, newest first.
    pub(crate) pins: Vec<String>,
    /// When each event title was last looked up for files, and when the lookups were last considered.
    pub(crate) materials_asked: std::collections::HashMap<String, i64>,
    pub(crate) materials_checked: i64,
    /// (time, percent) while on battery, for the drain.
    pub(crate) battery_log: Vec<(i64, f32)>,
    /// Percent per hour measured now / learned before (kept between runs), and when it was last blended in.
    pub(crate) drain_live: Option<f64>,
    pub(crate) drain_saved: Option<f64>,
    pub(crate) drain_blended_at: i64,
    /// The day number each battery card was last given (the day will not last / charge more).
    pub(crate) power_told: [i64; 2],
    /// What to take along, with the course it is for (empty: any day).
    pub(crate) bring: Vec<Bring>,
    /// How often each page was opened (the dock keeps the ones that are).
    pub(crate) page_uses: std::collections::HashMap<Page, u32>,
    /// The iPhone conversation that is open on its page (its key).
    pub(crate) phone_open: Option<String>,
    /// The Phone page shows the result of a contact search (a new message) instead of the conversations.
    pub(crate) phone_search: bool,
    /// How many messages of one conversation the card on screen stands for (a new one of the same conversation joins it).
    pub(crate) notif_group: u32,
    pub(crate) deadlines: Vec<Deadline>,
    /// Lines run from the command bar, the latest last (Up recalls them).
    pub(crate) bar_history: Vec<String>,
    pub(crate) notif: Option<Incoming>,
    notif_until: i64,
    /// The cards on screen besides the first one (`notif`), at most `STACK_MAX` - 1.
    pub(crate) below: Vec<card::Shown>,
    /// The notification cards under the pill, as they are drawn.
    pub(crate) cards: card::Cards,
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
    /// The notification whose code was just copied, and until when the card says so.
    pub(crate) code_copied: Option<(u32, i64)>,
    /// The program the last culprit card was about, and when.
    culprit_told: Option<(String, i64)>,
    culprit_id: u32,
    /// The card with tomorrow, and the day it was last shown (so that it comes once a day, restarts included).
    evening_id: u32,
    evening_day: i64,
    morning_id: u32,
    leave_id: u32,
    /// The news about the pinned train that has been said (train, time, kind), so that a card comes once.
    train_alerted: Option<String>,
    /// The event the "time to leave" card was shown for (id and start), so that it comes once.
    left_for: Option<(String, i64)>,
    first_tick_ms: Option<i64>,
    morning_day: i64,
    pub(crate) ledger: island_core::activity::Ledger,
    log: std::collections::VecDeque<LogEntry>,
    log_track: String,
    last_copy: String,
    pub(crate) activity_tab: usize,
    pub(crate) map_zoom_delta: i8,
    /// The place picked from a search on the map.
    pub(crate) map_dest: Option<island_core::travel::Place>,
    /// Where the map was dragged to (and the zoom it was at): None follows the position.
    map_center: Option<(f64, f64)>,
    map_fixed_zoom: Option<u8>,
    map_drag_last: (f64, f64),
    map_scroll_acc: f64,
    pub(crate) map_both: bool,
    map_asked_at: i64,
    /// Map tiles that arrived (and which were asked for), by zoom, x, y.
    pub(crate) tiles: std::collections::HashMap<(u8, i64, i64), Image>,
    tiles_asked: std::collections::HashSet<(u8, i64, i64)>,
    break_told: Option<i64>,
    /// Trips already asked for (event, way) and when; events already told to leave for.
    trip_asked: std::collections::HashMap<(String, island_core::travel::Mode), i64>,
    leave_told: std::collections::HashSet<String>,
    headsup_told: std::collections::HashSet<String>,
    /// What the last network card was about, and the card's id (a click on it reconnects).
    net_seen: island_core::netstate::Health,
    net_id: u32,
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
    /// The server and user of a CalDAV account being added, between its steps.
    caldav_draft: (Option<String>, Option<String>),
    pub(crate) note: Option<(String, i64)>,
    pub(crate) queue: Vec<notify::Notification>,
    pub(crate) waiting: std::collections::VecDeque<Incoming>,
    pub(crate) history: Vec<(Incoming, i64)>,
    pub(crate) osd: Option<Osd>,
    /// What the bars show: the level eased towards the real one, so a level that arrives in steps moves smoothly.
    pub(crate) osd_shown: f64,
    levels_init: bool,
    pub(crate) volume_shown: f64,
    pub(crate) brightness_shown: f64,
    osd_until: i64,
    /// Devices whose low battery was already announced.
    device_low_told: Vec<String>,
    low_battery_sent: bool,
    last_plugged: Option<bool>,
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
            shown_more: Vec::new(),
            want_more: Vec::new(),
            want: Face::Idle,
            want_side: None,
            alpha: 1.0,
            from_size: (w, h),
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
            extra_timers: Vec::new(),
            todos: Vec::new(),
            tasks_done: Vec::new(),
            row_menu: None,
            marquee_on: std::cell::Cell::new(false),
            marquee_since: std::cell::RefCell::new((String::new(), 0)),
            study_log: Vec::new(),
            found: Found::default(),
            notif_group: 1,
            deadlines: Vec::new(),
            pins: Vec::new(),
            materials_asked: std::collections::HashMap::new(),
            materials_checked: i64::MIN / 2,
            battery_log: Vec::new(),
            drain_live: None,
            drain_saved: None,
            drain_blended_at: i64::MIN / 2,
            power_told: [i64::MIN; 2],
            bring: Vec::new(),
            page_uses: std::collections::HashMap::new(),
            phone_open: None,
            phone_search: false,
            bar_history: Vec::new(),
            notif: None,
            notif_until: 0,
            below: Vec::new(),
            cards: Default::default(),
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
            code_copied: None,
            culprit_told: None,
            culprit_id: 0,
            evening_id: 0,
            evening_day: i64::MIN,
            morning_id: 0,
            leave_id: 0,
            train_alerted: None,
            left_for: None,
            first_tick_ms: None,
            morning_day: i64::MIN,
            ledger: Default::default(),
            log: Default::default(),
            log_track: String::new(),
            last_copy: String::new(),
            activity_tab: 0,
            map_zoom_delta: 0,
            map_dest: None,
            map_center: None,
            map_fixed_zoom: None,
            map_drag_last: (0.0, 0.0),
            map_scroll_acc: 0.0,
            map_both: true,
            map_asked_at: i64::MIN / 2,
            tiles: Default::default(),
            tiles_asked: Default::default(),
            break_told: None,
            trip_asked: Default::default(),
            leave_told: Default::default(),
            headsup_told: Default::default(),
            net_seen: island_core::netstate::Health::Online,
            net_id: 0,
            next_own_id: OWN_ID_BASE,
            bubbles: Vec::new(),
            fill: None,
            fx: Default::default(),
            kb: None,
            page_stack: vec![],
            slide: 0.0,
            edit: None,
            picking: None,
            icloud_user: None,
            caldav_draft: (None, None),
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
            device_low_told: vec![],
            low_battery_sent: false,
            last_plugged: None,
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
}

pub(crate) mod card;
pub(crate) mod effects;
mod events;
pub(crate) mod liquid;
use card::{Shown, STACK_MAX};
mod frame;
mod inputs;
mod organizer;
mod pointer;
mod power;
mod study;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_effects;

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
        Side::Op | Side::OpAt(_) => Page::Op,
    }
}

/// The one-time code in a notification (not in the island's own cards).
pub(crate) fn notif_code(n: &Incoming) -> Option<String> {
    if is_own(n) {
        return None;
    }
    island_core::codes::extract(&format!("{} {}", n.summary, n.body))
}

/// Faces that are pages: they carry the dock at the bottom.
pub(crate) fn has_dock(f: Face) -> bool {
    matches!(f, Face::IdleHover | Face::MediaFull | Face::MediaLyric | Face::TimerFull | Face::FocusFull | Face::Cal | Face::Weather | Face::Qs | Face::Sys | Face::Notifs | Face::Clip | Face::Settings | Face::Options | Face::Audio | Face::Today | Face::Plan | Face::Find | Face::Send | Face::Activity | Face::Map | Face::Phone | Face::Grid)
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
        Face::Plan => Page::Plan,
        Face::Find => Page::Find,
        Face::Send => Page::Send,
        Face::Activity => Page::Activity,
        Face::Map => Page::Map,
        Face::Phone => Page::Phone,
        Face::Grid => Page::Grid,
        Face::FocusFull => Page::Focus,
        _ => return None,
    })
}
