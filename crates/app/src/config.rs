//! User configuration: `config.json` in the platform's config directory.
//!
//! ```json
//! { "latitude": 43.72, "longitude": 10.40,
//!   "calendars": [
//!     {"name": "University", "url": "https://example.org/calendar.ics", "color": "#ff9f0a"},
//!     {"type": "icloud", "name": "iCloud", "user": "me@icloud.com", "password": "abcd-efgh-ijkl-mnop"} ],
//!   "timer_presets": ["5m", "10m", "25m"], "meeting_lead_minutes": 10, "lyrics": true,
//!   "font": "Inter" }
//! ```
//! Weather needs a location (the island never guesses one); calendars are read from iCal feed URLs.

use island_core::color;
use island_ui::Config as UiConfig;

/// Where a calendar's events come from.
#[derive(Clone, Debug, PartialEq)]
pub enum CalendarKind {
    /// An iCalendar feed (a public or secret .ics / webcal:// address).
    Ics { url: String },
    /// A CalDAV account (iCloud, Nextcloud, Fastmail...): every calendar of the account is read. iCloud needs an
    /// app-specific password (appleid.apple.com), not the Apple ID password. `password_cmd` (a shell command that
    /// prints it, for a password manager) wins over `password`.
    CalDav { server: String, user: String, password: String, password_cmd: String },
}

pub const ICLOUD_SERVER: &str = "https://caldav.icloud.com/";

/// The macOS system colours a new calendar gets in turn.
pub const PALETTE: [(u8, u8, u8); 8] = [
    (0xff, 0x45, 0x3a),
    (0xff, 0x9f, 0x0a),
    (0xff, 0xd6, 0x0a),
    (0x30, 0xd1, 0x58),
    (0x0a, 0x84, 0xff),
    (0xbf, 0x5a, 0xf2),
    (0xff, 0x37, 0x5f),
    (0x98, 0x98, 0x9d),
];

#[derive(Clone, Debug, PartialEq)]
pub struct CalendarSource {
    pub name: String,
    pub kind: CalendarKind,
    /// None: the calendar's own colour (a CalDAV server's) or the first palette entry.
    pub color: Option<(u8, u8, u8)>,
}

impl CalendarSource {
    pub fn is_icloud(&self) -> bool {
        matches!(&self.kind, CalendarKind::CalDav { server, .. } if server.contains("icloud.com"))
    }

    /// A short line for the settings page: the host of a feed, or the account of a CalDAV server.
    pub fn detail(&self) -> String {
        match &self.kind {
            CalendarKind::Ics { url } => url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or("").to_string(),
            CalendarKind::CalDav { user, .. } if self.is_icloud() => format!("iCloud · {user}"),
            CalendarKind::CalDav { server, user, .. } => format!("{} · {user}", server.split("://").nth(1).unwrap_or(server).split('/').next().unwrap_or("")),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        let mut o = serde_json::Map::new();
        if !self.name.is_empty() {
            o.insert("name".into(), self.name.clone().into());
        }
        if let Some(c) = self.color {
            o.insert("color".into(), color::to_hex(c).into());
        }
        match &self.kind {
            CalendarKind::Ics { url } => {
                o.insert("url".into(), url.clone().into());
            }
            CalendarKind::CalDav { server, user, password, password_cmd } => {
                o.insert("type".into(), if self.is_icloud() { "icloud" } else { "caldav" }.into());
                if !self.is_icloud() {
                    o.insert("server".into(), server.clone().into());
                }
                o.insert("user".into(), user.clone().into());
                if !password.is_empty() {
                    o.insert("password".into(), password.clone().into());
                }
                if !password_cmd.is_empty() {
                    o.insert("password_cmd".into(), password_cmd.clone().into());
                }
            }
        }
        o.into()
    }

    pub fn from_json(v: &serde_json::Value) -> Option<Self> {
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
        if let Some(url) = v.as_str() {
            return Some(Self { name: String::new(), kind: CalendarKind::Ics { url: url.trim().to_string() }, color: None });
        }
        let col = color::parse_hex(&s("color"));
        let kind = if !s("url").is_empty() {
            CalendarKind::Ics { url: s("url") }
        } else if !s("user").is_empty() {
            let server = if s("server").is_empty() || s("type") == "icloud" { ICLOUD_SERVER.to_string() } else { s("server") };
            CalendarKind::CalDav { server, user: s("user"), password: s("password"), password_cmd: s("password_cmd") }
        } else {
            return None;
        };
        Some(Self { name: s("name"), kind, color: col })
    }
}

/// Adds a feed unless it is already there; it gets the next system colour.
pub fn add_feed(list: &mut Vec<CalendarSource>, url: &str) {
    if !list.iter().any(|c| matches!(&c.kind, CalendarKind::Ics { url: u } if u == url)) {
        let color = PALETTE[list.len() % PALETTE.len()];
        list.push(CalendarSource { name: String::new(), kind: CalendarKind::Ics { url: url.to_string() }, color: Some(color) });
    }
}

/// Adds an iCloud account (replacing one with the same Apple ID). It keeps its calendars' own colours.
pub fn add_icloud(list: &mut Vec<CalendarSource>, user: &str, password: &str) {
    list.retain(|c| !(c.is_icloud() && matches!(&c.kind, CalendarKind::CalDav { user: u, .. } if u == user)));
    list.push(CalendarSource {
        name: String::new(),
        kind: CalendarKind::CalDav { server: ICLOUD_SERVER.into(), user: user.to_string(), password: password.to_string(), password_cmd: String::new() },
        color: None,
    });
}

/// Writes the calendars into `config.json`: the `calendars` list replaces the old `calendar_urls`.
pub fn set_calendars(o: &mut serde_json::Map<String, serde_json::Value>, calendars: &[CalendarSource]) {
    o.remove("calendar_urls");
    o.insert("calendars".into(), calendars.iter().map(CalendarSource::to_json).collect::<Vec<_>>().into());
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub calendars: Vec<CalendarSource>,
    pub ui: UiConfig,
    /// Played for every notification unless a filter or an app sound says otherwise.
    pub notification_sound: Option<String>,
    /// (case-insensitive substring of the app name, sound file)
    pub app_sounds: Vec<(String, String)>,
    pub volume_sound: Option<String>,
    pub timer_sound: Option<String>,
    pub filters: Vec<Filter>,
    /// Keep the notification history on disk between runs.
    pub persist_history: bool,
    /// Stop another notification daemon (mako, dunst, ...) so notifications show once, in the island.
    pub take_over_notifications: bool,
    /// A font family name or the path of a font file; empty = the built-in list (SF Pro first).
    pub font: String,
    /// "focused", or the name of the screen to pin the island to.
    pub screen: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            latitude: None,
            longitude: None,
            calendars: vec![],
            ui: UiConfig::default(),
            notification_sound: None,
            app_sounds: vec![],
            volume_sound: None,
            timer_sound: None,
            filters: vec![],
            persist_history: true,
            take_over_notifications: true,
            font: String::new(),
            screen: "focused".into(),
        }
    }
}

/// Drops or silences notifications that match (case-insensitive substrings; empty matches anything).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    pub app: String,
    pub content: String,
    pub show: bool,
    pub sound: bool,
}

impl Filter {
    pub fn matches(&self, app: &str, summary: &str, body: &str) -> bool {
        let has = |hay: &str, needle: &str| needle.is_empty() || hay.to_lowercase().contains(&needle.to_lowercase());
        has(app, &self.app) && (has(summary, &self.content) || has(body, &self.content))
    }
}

pub fn path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(std::path::PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| {
            let h = std::path::PathBuf::from(h);
            if cfg!(target_os = "macos") { h.join("Library/Application Support") } else { h.join(".config") }
        }))?;
    Some(base.join("open-island").join("config.json"))
}

fn expand_home(p: &str) -> String {
    match (p.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(h)) => format!("{}/{}", h.to_string_lossy(), rest),
        _ => p.to_string(),
    }
}

pub fn parse(text: &str) -> Result<Settings, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let mut s = Settings::default();
    let path = |k: &str| v.get(k).and_then(|x| x.as_str()).map(expand_home);
    s.notification_sound = path("notification_sound");
    s.volume_sound = path("volume_sound");
    if let Some(o) = v.get("app_sounds").and_then(|x| x.as_object()) {
        s.app_sounds = o.iter().filter_map(|(k, v)| Some((k.clone(), expand_home(v.as_str()?)))).collect();
    }
    if let Some(a) = v.get("notification_filters").and_then(|x| x.as_array()) {
        s.filters = a
            .iter()
            .map(|f| Filter {
                app: f.get("app").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                content: f.get("content").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                show: f.get("show").and_then(|x| x.as_bool()).unwrap_or(true),
                sound: f.get("sound").and_then(|x| x.as_bool()).unwrap_or(true),
            })
            .collect();
    }
    s.font = v.get("font").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    s.take_over_notifications = v.get("take_over_notifications").and_then(|x| x.as_bool()).unwrap_or(true);
    s.persist_history = v.get("persist_history").and_then(|x| x.as_bool()).unwrap_or(true);
    s.latitude = v.get("latitude").and_then(|x| x.as_f64()).filter(|x| (-90.0..=90.0).contains(x));
    s.longitude = v.get("longitude").and_then(|x| x.as_f64()).filter(|x| (-180.0..=180.0).contains(x));
    // the old `calendar_urls` (plain addresses) still work, then the `calendars` objects
    for key in ["calendar_urls", "calendars"] {
        if let Some(a) = v.get(key).and_then(|x| x.as_array()) {
            s.calendars.extend(a.iter().filter_map(CalendarSource::from_json));
        }
    }
    let u = &mut s.ui;
    if let Some(a) = v.get("timer_presets").and_then(|x| x.as_array()) {
        let p: Vec<u64> = a.iter().filter_map(|x| x.as_str().and_then(island_core::duration::parse).or_else(|| x.as_u64().map(|m| m * 60))).collect();
        if !p.is_empty() {
            u.timer_presets = p;
        }
    }
    let b = |k: &str, d: bool| v.get(k).and_then(|x| x.as_bool()).unwrap_or(d);
    u.meeting_alerts = b("meeting_alerts", u.meeting_alerts);
    u.rain_alerts = b("rain_alerts", u.rain_alerts);
    u.quiet_in_meetings = b("quiet_in_meetings", u.quiet_in_meetings);
    u.quiet_on_mic = b("quiet_on_mic", u.quiet_on_mic);
    u.away_recap = b("away_recap", u.away_recap);
    u.quiet_on_screen_share = b("quiet_on_screen_share", u.quiet_on_screen_share);
    u.battery_alerts = b("battery_alerts", u.battery_alerts);
    u.media_auto_expand = b("media_auto_expand", u.media_auto_expand);
    u.clock_in_pill = b("clock_in_pill", u.clock_in_pill);
    u.status_icons = b("status_icons", u.status_icons);
    u.lyrics = b("lyrics", u.lyrics);
    u.clipboard_history = b("clipboard_history", u.clipboard_history);
    u.lyrics_in_pill = b("lyrics_in_pill", u.lyrics_in_pill);
    u.show_notifications = b("show_notifications", u.show_notifications);
    u.show_osd = b("show_osd", u.show_osd);
    u.art_tint = b("art_tint", u.art_tint);
    u.hide_on_fullscreen = b("hide_on_fullscreen", u.hide_on_fullscreen);
    u.weather = b("weather", u.weather);
    u.auto_audio_output = b("auto_audio_output", u.auto_audio_output);
    // `timer_sound` is on or off (as in the Noctalia island) and `timer_sound_file` the file; an older `timer_sound`
    // holding a path still means that file, played
    match v.get("timer_sound") {
        Some(serde_json::Value::Bool(on)) => u.timer_sound = *on,
        Some(serde_json::Value::String(f)) => {
            s.timer_sound = Some(expand_home(f));
            u.timer_sound = true;
        }
        _ => {}
    }
    if let Some(f) = v.get("timer_sound_file").and_then(|x| x.as_str()).filter(|f| !f.trim().is_empty()) {
        s.timer_sound = Some(expand_home(f));
    }
    s.screen = v.get("screen").and_then(|x| x.as_str()).map(str::trim).filter(|x| !x.is_empty()).unwrap_or("focused").to_string();
    if let Some(sec) = v.get("osd_seconds").and_then(|x| x.as_f64()).filter(|s| (0.5..=5.0).contains(s)) {
        u.osd_ms = (sec * 1000.0).round() as i64;
    }
    if let Some(m) = v.get("meeting_lead_minutes").and_then(|x| x.as_i64()).filter(|m| (1..=120).contains(m)) {
        u.meeting_lead_ms = m * 60_000;
    }
    if let Some(m) = v.get("meeting_pill_minutes").and_then(|x| x.as_i64()).filter(|m| (1..=240).contains(m)) {
        u.meeting_pill_ms = m * 60_000;
    }
    if let Some(m) = v.get("notification_seconds").and_then(|x| x.as_i64()).filter(|m| (2..=60).contains(m)) {
        u.notification_ms = m * 1000;
    }
    if let Some(p) = v.get("low_battery_percent").and_then(|x| x.as_f64()).filter(|p| (1.0..=90.0).contains(p)) {
        u.low_battery = p as f32;
    }
    Ok(s)
}

/// The options of the options page: the keys that are on or off.
pub const BOOL_OPTIONS: &[&str] = &[
    "show_notifications", "show_osd", "media_auto_expand", "timer_sound", "battery_alerts", "weather", "rain_alerts", "meeting_alerts", "lyrics", "lyrics_in_pill", "art_tint", "status_icons",
    "clipboard_history", "clock_in_pill", "hide_on_fullscreen", "auto_audio_output", "quiet_in_meetings", "quiet_on_mic", "away_recap", "quiet_on_screen_share",
];

/// The numeric options: key, smallest, largest, and whether it is a whole number.
pub const NUM_OPTIONS: &[(&str, f64, f64, bool)] = &[("notification_seconds", 2.0, 30.0, true), ("osd_seconds", 0.5, 5.0, false), ("low_battery_percent", 5.0, 50.0, true), ("meeting_lead_minutes", 1.0, 60.0, true), ("meeting_pill_minutes", 1.0, 120.0, true)];

/// Adds a notification rule (once): `hide` drops the notifications, otherwise they only lose their sound.
pub fn add_filter(o: &mut serde_json::Map<String, serde_json::Value>, app: &str, content: &str, hide: bool) {
    let rule = serde_json::json!({"app": app.trim(), "content": content.trim(), "show": !hide, "sound": false});
    let list = o.entry("notification_filters").or_insert_with(|| serde_json::json!([]));
    if let Some(a) = list.as_array_mut() {
        if !a.contains(&rule) {
            a.push(rule);
        }
    }
}

pub fn remove_filter(o: &mut serde_json::Map<String, serde_json::Value>, index: usize) {
    if let Some(a) = o.get_mut("notification_filters").and_then(|v| v.as_array_mut()) {
        if index < a.len() {
            a.remove(index);
        }
        if a.is_empty() {
            o.remove("notification_filters");
        }
    }
}

/// Sets one option in the configuration object; an unknown key or a value out of range is an error.
pub fn set_option(o: &mut serde_json::Map<String, serde_json::Value>, key: &str, value: &island_ui::model::OptionValue) -> Result<(), String> {
    use island_ui::model::OptionValue::*;
    use serde_json::json;
    match value {
        Bool(b) if BOOL_OPTIONS.contains(&key) => {
            o.insert(key.into(), json!(b));
        }
        Num(n) => {
            let (_, lo, hi, whole) = NUM_OPTIONS.iter().find(|(k, ..)| *k == key).ok_or_else(|| format!("{key} is not a number option"))?;
            if !n.is_finite() || n < lo || n > hi {
                return Err(format!("{key} is between {lo} and {hi}"));
            }
            o.insert(key.into(), if *whole { json!(n.round() as i64) } else { json!((n * 10.0).round() / 10.0) });
        }
        Text(t) if key == "timer_presets" => {
            let list: Vec<String> = t.split([',', ';']).map(str::trim).filter(|p| island_core::duration::parse(p).is_some_and(|s| s > 0)).map(str::to_string).take(6).collect();
            if list.is_empty() {
                o.remove("timer_presets"); // back to the defaults
            } else {
                o.insert("timer_presets".into(), json!(list));
            }
        }
        Text(t) if key == "screen" => {
            let t = t.trim();
            if t.is_empty() || t.eq_ignore_ascii_case("focused") {
                o.remove("screen");
            } else {
                o.insert("screen".into(), json!(t));
            }
        }
        _ => return Err(format!("{key}: not an option, or the wrong kind of value")),
    }
    Ok(())
}

/// Changes `config.json` in place: the keys the island does not know are kept. The file and its directory are
/// created when missing; a broken file is not overwritten.
pub fn update_at(path: &std::path::Path, change: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>)) -> Result<(), String> {
    let mut obj = match std::fs::read_to_string(path) {
        Ok(t) if t.trim().is_empty() => serde_json::Map::new(),
        Ok(t) => match serde_json::from_str::<serde_json::Value>(&t) {
            Ok(serde_json::Value::Object(o)) => o,
            _ => return Err(format!("{} is not a JSON object: fix it by hand first", path.display())),
        },
        Err(_) => serde_json::Map::new(),
    };
    change(&mut obj);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&serde_json::Value::Object(obj)).map_err(|e| e.to_string())?;
    // write next to it and rename: a crash never leaves half a file
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text + "\n").map_err(|e| e.to_string())?;
    // the file can hold an account password: owner only
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn update(change: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>)) -> Result<(), String> {
    update_at(&path().ok_or("no configuration directory")?, change)
}

/// Loads the settings; a missing file gives the defaults, a broken one is reported and ignored.
pub fn load() -> Settings {
    let Some(p) = path() else { return Settings::default() };
    match std::fs::read_to_string(&p) {
        Ok(t) => parse(&t).unwrap_or_else(|e| {
            log::error!("{}: {e}; using the defaults", p.display());
            Settings::default()
        }),
        Err(_) => Settings::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_options_of_the_noctalia_island() {
        let s = parse(r#"{"show_notifications":false,"show_osd":false,"art_tint":false,"hide_on_fullscreen":false,"weather":false,"osd_seconds":2.5,"screen":"eDP-1","timer_sound":false,"timer_sound_file":"~/a.ogg"}"#).unwrap();
        let u = &s.ui;
        assert!(!u.show_notifications && !u.show_osd && !u.art_tint && !u.hide_on_fullscreen && !u.weather && !u.timer_sound);
        assert_eq!(u.osd_ms, 2500);
        assert_eq!(s.screen, "eDP-1");
        assert!(s.timer_sound.as_deref().is_some_and(|f| f.ends_with("/a.ogg")), "the file is kept even while the sound is off");
        let d = parse("{}").unwrap();
        assert!(d.ui.show_notifications && d.ui.show_osd && d.ui.art_tint && d.ui.hide_on_fullscreen && d.ui.weather && d.ui.timer_sound);
        assert_eq!((d.screen.as_str(), d.ui.osd_ms), ("focused", 1600));
        // an older `timer_sound` that holds a path still means that file, on
        let old = parse(r#"{"timer_sound":"/x/y.wav"}"#).unwrap();
        assert!(old.ui.timer_sound && old.timer_sound.as_deref() == Some("/x/y.wav"));
        assert_eq!(parse(r#"{"osd_seconds":99}"#).unwrap().ui.osd_ms, 1600, "out of range: the default");
    }

    #[test]
    fn notification_rules_are_added_once_and_removed_by_position() {
        let mut o = serde_json::Map::new();
        add_filter(&mut o, " Zen Browser ", "", true);
        add_filter(&mut o, "Zen Browser", "", true);
        add_filter(&mut o, "Telegram", "", false);
        let rules = serde_json::Value::Object(o.clone());
        assert_eq!(rules["notification_filters"].as_array().unwrap().len(), 2, "the same rule twice is one");
        // what the island reads back
        let s = parse(&rules.to_string()).unwrap();
        assert_eq!(s.filters.len(), 2);
        assert!(!s.filters[0].show && s.filters[0].matches("zen browser", "x", "y"), "hidden, matched without case");
        assert!(s.filters[1].show && !s.filters[1].sound, "silent");
        remove_filter(&mut o, 0);
        remove_filter(&mut o, 9);
        assert_eq!(parse(&serde_json::Value::Object(o.clone()).to_string()).unwrap().filters.len(), 1);
        remove_filter(&mut o, 0);
        assert!(!o.contains_key("notification_filters"), "no empty list left behind");
    }

    #[test]
    fn options_are_written_checked_and_can_be_reset() {
        use island_ui::model::OptionValue::*;
        let mut o = serde_json::Map::new();
        set_option(&mut o, "lyrics", &Bool(false)).unwrap();
        set_option(&mut o, "notification_seconds", &Num(7.4)).unwrap();
        set_option(&mut o, "osd_seconds", &Num(1.26)).unwrap();
        set_option(&mut o, "timer_presets", &Text("5m, 90s ; nonsense, 1h30m".into())).unwrap();
        set_option(&mut o, "screen", &Text("DP-2".into())).unwrap();
        assert_eq!(serde_json::Value::Object(o.clone()), serde_json::json!({"lyrics": false, "notification_seconds": 7, "osd_seconds": 1.3, "timer_presets": ["5m", "90s", "1h30m"], "screen": "DP-2"}));
        assert!(set_option(&mut o, "lyrics", &Num(1.0)).is_err(), "a number for a switch");
        assert!(set_option(&mut o, "nonsense", &Bool(true)).is_err());
        assert!(set_option(&mut o, "notification_seconds", &Num(500.0)).is_err(), "out of range");
        assert!(set_option(&mut o, "notification_seconds", &Num(f64::NAN)).is_err());
        set_option(&mut o, "screen", &Text("focused".into())).unwrap();
        set_option(&mut o, "timer_presets", &Text("".into())).unwrap();
        assert!(!o.contains_key("screen") && !o.contains_key("timer_presets"), "back to the defaults");
        for k in BOOL_OPTIONS {
            let mut m = serde_json::Map::new();
            set_option(&mut m, k, &Bool(true)).unwrap_or_else(|e| panic!("{k}: {e}"));
        }
    }

    #[test]
    fn reads_the_known_keys() {
        let s = parse(r##"{"latitude":43.7,"longitude":10.4,"calendar_urls":["https://x/y.ics"],"calendars":[{"name":"Uni","url":"https://x/u.ics","color":"#0a84ff"},{"type":"icloud","user":"me@icloud.com","password":"abcd"}],"timer_presets":["5m",10],"meeting_lead_minutes":15,"lyrics":false,"quiet_on_mic":false}"##).unwrap();
        assert_eq!(s.latitude, Some(43.7));
        assert_eq!(s.calendars.len(), 3);
        assert_eq!(s.calendars[0], CalendarSource { name: String::new(), kind: CalendarKind::Ics { url: "https://x/y.ics".into() }, color: None });
        assert_eq!(s.calendars[1].name, "Uni");
        assert_eq!(s.calendars[1].color, Some((0x0a, 0x84, 0xff)));
        assert!(s.calendars[2].is_icloud());
        assert_eq!(s.calendars[2].detail(), "iCloud · me@icloud.com");
        assert!(matches!(&s.calendars[2].kind, CalendarKind::CalDav { server, .. } if server == ICLOUD_SERVER));
        assert_eq!(s.ui.timer_presets, vec![300, 600]);
        assert_eq!(s.ui.meeting_lead_ms, 15 * 60_000);
        assert_eq!(s.ui.meeting_pill_ms, UiConfig::default().meeting_pill_ms);
        assert_eq!(parse(r#"{"meeting_pill_minutes":45}"#).unwrap().ui.meeting_pill_ms, 45 * 60_000);
        assert_eq!(parse(r#"{"meeting_pill_minutes":0}"#).unwrap().ui.meeting_pill_ms, UiConfig::default().meeting_pill_ms);
        assert!(!s.ui.lyrics);
        assert!(!s.ui.quiet_on_mic && s.ui.quiet_in_meetings && s.ui.quiet_on_screen_share, "quiet: only what is switched off is off");
    }

    #[test]
    fn updates_keep_the_unknown_keys_and_refuse_a_broken_file() {
        let dir = std::env::temp_dir().join(format!("open-island-test-{}", std::process::id()));
        let file = dir.join("config.json");
        update_at(&file, |o| {
            o.insert("font".into(), "Inter".into());
        })
        .unwrap();
        update_at(&file, |o| {
            o.insert("something_else".into(), 7.into());
            o.insert("calendars".into(), serde_json::json!([{"url": "https://x.org/a.ics"}]));
        })
        .unwrap();
        let s = parse(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!((s.font.as_str(), s.calendars.len()), ("Inter", 1));
        assert!(std::fs::read_to_string(&file).unwrap().contains("something_else"), "unknown keys survive");
        std::fs::write(&file, "{ not json").unwrap();
        assert!(update_at(&file, |_| {}).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ not json", "a broken file is left alone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn calendars_round_trip_and_replace_the_old_list() {
        let list = vec![
            CalendarSource { name: "Uni".into(), kind: CalendarKind::Ics { url: "https://x/u.ics".into() }, color: Some((255, 69, 58)) },
            CalendarSource { name: "".into(), kind: CalendarKind::CalDav { server: ICLOUD_SERVER.into(), user: "me@icloud.com".into(), password: "abcd".into(), password_cmd: "pass show icloud".into() }, color: None },
            CalendarSource { name: "Work".into(), kind: CalendarKind::CalDav { server: "https://cloud.example.org/dav/".into(), user: "me".into(), password: "".into(), password_cmd: "".into() }, color: None },
        ];
        let mut o = serde_json::Map::new();
        o.insert("calendar_urls".into(), serde_json::json!(["https://old.org/a.ics"]));
        set_calendars(&mut o, &list);
        assert!(!o.contains_key("calendar_urls"), "the old list is migrated away");
        let back = parse(&serde_json::Value::Object(o).to_string()).unwrap();
        assert_eq!(back.calendars, list);
        assert_eq!(back.calendars[1].detail(), "iCloud · me@icloud.com");
        assert_eq!(back.calendars[2].detail(), "cloud.example.org · me");
        assert_eq!(list[0].detail(), "x");
    }

    #[test]
    fn adding_calendars_gives_colours_in_turn_and_never_duplicates() {
        let mut l = vec![];
        add_feed(&mut l, "https://a.org/1.ics");
        add_feed(&mut l, "https://a.org/2.ics");
        add_feed(&mut l, "https://a.org/1.ics");
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].color, Some(PALETTE[0]));
        assert_eq!(l[1].color, Some(PALETTE[1]));
        add_icloud(&mut l, "me@icloud.com", "one");
        add_icloud(&mut l, "me@icloud.com", "two");
        add_icloud(&mut l, "other@icloud.com", "x");
        assert_eq!(l.iter().filter(|c| c.is_icloud()).count(), 2, "one per Apple ID");
        assert!(matches!(&l.iter().find(|c| c.detail().contains("me@")).unwrap().kind, CalendarKind::CalDav { password, .. } if password == "two"), "the newer password replaced the old");
        assert_eq!(l.iter().find(|c| c.is_icloud()).unwrap().color, None, "an account keeps its calendars' own colours");
    }

    #[test]
    fn taking_over_the_notification_daemon_is_on_unless_switched_off() {
        assert!(parse("{}").unwrap().take_over_notifications);
        assert!(!parse(r#"{"take_over_notifications": false}"#).unwrap().take_over_notifications);
    }

    #[test]
    fn reads_the_font() {
        assert_eq!(parse(r#"{"font":"  Inter "}"#).unwrap().font, "Inter");
        assert_eq!(parse(r#"{"font":"~/fonts/My Font.ttf"}"#).unwrap().font, "~/fonts/My Font.ttf");
        assert_eq!(parse("{}").unwrap().font, "");
        assert_eq!(parse(r#"{"font":3}"#).unwrap().font, "", "not a string");
    }

    #[test]
    fn reads_sounds_and_filters() {
        let s = parse(r#"{"notification_sound":"/s/uwu.ogg","app_sounds":{"telegram":"/s/t.ogg"},
            "notification_filters":[{"app":"zen browser","content":"unseen","show":false}],"persist_history":false}"#).unwrap();
        assert_eq!(s.notification_sound.as_deref(), Some("/s/uwu.ogg"));
        assert_eq!(s.app_sounds, vec![("telegram".to_string(), "/s/t.ogg".to_string())]);
        assert!(!s.persist_history);
        let f = &s.filters[0];
        assert!(!f.show && f.sound, "unset flags default to true");
        assert!(f.matches("Zen Browser", "You have unseen notifications.", ""));
        assert!(!f.matches("Zen Browser", "A real message", ""));
        assert!(!f.matches("Telegram", "unseen", ""));
        assert!(Filter { show: true, ..Default::default() }.matches("anything", "x", "y"), "empty filter matches all");
        assert!(parse("{}").unwrap().persist_history, "history is kept by default");
    }

    #[test]
    fn rejects_out_of_range_and_bad_json() {
        let s = parse(r#"{"latitude":95,"meeting_lead_minutes":0}"#).unwrap();
        assert_eq!(s.latitude, None);
        assert_eq!(s.ui.meeting_lead_ms, UiConfig::default().meeting_lead_ms);
        assert!(parse("{nope").is_err());
        assert!(parse("{}").unwrap().calendars.is_empty());
    }
}
