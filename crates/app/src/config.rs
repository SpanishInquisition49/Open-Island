//! User configuration: `config.json` in the platform's config directory.
//!
//! ```json
//! { "latitude": 43.72, "longitude": 10.40,
//!   "calendar_urls": ["https://example.org/calendar.ics"],
//!   "timer_presets": ["5m", "10m", "25m"], "meeting_lead_minutes": 10, "lyrics": true }
//! ```
//! Weather needs a location (the island never guesses one); calendars are read from iCal feed URLs.

use island_ui::Config as UiConfig;

#[derive(Clone, Debug)]
pub struct Settings {
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub calendar_urls: Vec<String>,
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            latitude: None,
            longitude: None,
            calendar_urls: vec![],
            ui: UiConfig::default(),
            notification_sound: None,
            app_sounds: vec![],
            volume_sound: None,
            timer_sound: None,
            filters: vec![],
            persist_history: true,
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
    s.persist_history = v.get("persist_history").and_then(|x| x.as_bool()).unwrap_or(true);
    s.latitude = v.get("latitude").and_then(|x| x.as_f64()).filter(|x| (-90.0..=90.0).contains(x));
    s.longitude = v.get("longitude").and_then(|x| x.as_f64()).filter(|x| (-180.0..=180.0).contains(x));
    if let Some(a) = v.get("calendar_urls").and_then(|x| x.as_array()) {
        s.calendar_urls = a.iter().filter_map(|u| u.as_str()).map(str::to_string).collect();
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
        let s = parse(r#"{"latitude":43.7,"longitude":10.4,"calendar_urls":["https://x/y.ics"],"timer_presets":["5m",10],"meeting_lead_minutes":15,"lyrics":false}"#).unwrap();
        assert_eq!(s.latitude, Some(43.7));
        assert_eq!(s.calendar_urls, vec!["https://x/y.ics"]);
        assert_eq!(s.ui.timer_presets, vec![300, 600]);
        assert_eq!(s.ui.meeting_lead_ms, 15 * 60_000);
        assert!(!s.ui.lyrics);
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
        assert!(parse("{}").unwrap().calendar_urls.is_empty());
    }
}
