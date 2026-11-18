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
    /// A font family name or the path of a font file; empty = the built-in list (SF Pro first).
    pub font: String,
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
            font: String::new(),
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
    s.timer_sound = path("timer_sound");
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
    u.battery_alerts = b("battery_alerts", u.battery_alerts);
    u.media_auto_expand = b("media_auto_expand", u.media_auto_expand);
    u.clock_in_pill = b("clock_in_pill", u.clock_in_pill);
    u.status_icons = b("status_icons", u.status_icons);
    u.lyrics = b("lyrics", u.lyrics);
    u.clipboard_history = b("clipboard_history", u.clipboard_history);
    if let Some(m) = v.get("meeting_lead_minutes").and_then(|x| x.as_i64()).filter(|m| (1..=120).contains(m)) {
        u.meeting_lead_ms = m * 60_000;
    }
    if let Some(m) = v.get("notification_seconds").and_then(|x| x.as_i64()).filter(|m| (2..=60).contains(m)) {
        u.notification_ms = m * 1000;
    }
    if let Some(p) = v.get("low_battery_percent").and_then(|x| x.as_f64()).filter(|p| (1.0..=90.0).contains(p)) {
        u.low_battery = p as f32;
    }
    Ok(s)
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
    fn reads_the_known_keys() {
        let s = parse(r##"{"latitude":43.7,"longitude":10.4,"calendar_urls":["https://x/y.ics"],"calendars":[{"name":"Uni","url":"https://x/u.ics","color":"#0a84ff"},{"type":"icloud","user":"me@icloud.com","password":"abcd"}],"timer_presets":["5m",10],"meeting_lead_minutes":15,"lyrics":false}"##).unwrap();
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
        assert!(!s.ui.lyrics);
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
