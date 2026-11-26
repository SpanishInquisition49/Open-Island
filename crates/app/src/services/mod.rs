//! Everything that talks to the operating system: sampling the clock, battery, volume, media players,
//! notifications, weather, calendars, and the local control socket. Each service runs on its own thread
//! and reports through a channel; the UI thread never blocks on I/O.

use island_ui::model::{Command, Data, Input};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;

pub mod art;
pub mod awake;
pub mod clipboard_history;
pub mod caldav;
pub mod cards;
pub mod clock;
pub mod icons;
pub mod parse;
#[cfg(target_os = "linux")]
pub mod devices;
#[cfg(target_os = "linux")]
pub mod audio_mixer;
pub mod culprit;
pub mod downloads;
pub mod materials;
pub mod organizer;
pub mod ocr;
pub mod phone;
pub mod watchpid;
pub mod location;
pub mod tiles;
// nothing reads the network's health on macOS yet
#[cfg(any(target_os = "linux", windows))]
pub mod netwatch;
pub mod travel;
#[cfg(target_os = "linux")]
pub mod audio_route;
pub mod windows_focus;
pub mod sound;
pub mod tz;
pub mod ipc;
pub mod net;
pub mod rates;
pub mod trains;
pub mod send;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
pub mod notif;
#[cfg(target_os = "linux")]
pub mod clipboard;
#[cfg(target_os = "linux")]
pub mod focus;
#[cfg(target_os = "linux")]
pub mod niri;
#[cfg(target_os = "linux")]
pub mod quick;
#[cfg(target_os = "linux")]
pub mod spectrum;
#[cfg(target_os = "linux")]
pub mod sysmon;
#[cfg(not(target_os = "linux"))]
pub mod generic;
#[cfg(not(target_os = "linux"))]
pub mod other;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "windows")]
pub mod win_more;
#[cfg(target_os = "windows")]
pub mod win_audio;

/// Set by the app while the system page / the quick settings page is open: the sampling threads idle otherwise.
pub static SYSTEM_WANTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Bumped whenever the settings change, so the services that read them (weather, calendars) refetch at once.
/// Bumped when the position changes enough to matter to the weather (and asked for when a new fix is wanted).
pub static LOCATION_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static LOCATION_ASK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static CONFIG_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Bumped to make the calendar thread read the calendars now (the refresh button, opening a stale page).
pub static CALENDAR_REFRESH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Sleeps up to `secs`, or until one of the counters changes. Returns true when one did.
pub fn wait_for(secs: u64, gens: &[&std::sync::atomic::AtomicU64]) -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    let start: Vec<u64> = gens.iter().map(|g| g.load(Relaxed)).collect();
    let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < end {
        std::thread::sleep(std::time::Duration::from_millis(400));
        if gens.iter().zip(&start).any(|(g, s)| g.load(Relaxed) != *s) {
            return true;
        }
    }
    false
}

pub static SPECTRUM_WANTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static QUICK_WANTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[allow(dead_code)] // Quit and Fullscreen are only sent by some platforms
pub enum Msg {
    /// Change one part of the sampled data.
    Patch(Box<dyn FnOnce(&mut Data) + Send>),
    /// A one-off event.
    Input(Input),
    /// A command as if the island had issued it (settings from a script).
    Exec(Command),
    Quit,
    Fullscreen(bool),
    /// A maximized window covers the island's screen, and whether the pointer is at the top edge asking for it.
    Cover { maximized: bool, pointer_near: bool },
}

/// Where services report to, with a way to wake the event loop.
#[derive(Clone)]
pub struct Reporter {
    tx: Sender<Msg>,
    waker: Arc<dyn Fn() + Send + Sync>,
}

impl Reporter {
    pub fn patch(&self, f: impl FnOnce(&mut Data) + Send + 'static) {
        let _ = self.tx.send(Msg::Patch(Box::new(f)));
        (self.waker)();
    }

    pub fn exec(&self, c: Command) {
        let _ = self.tx.send(Msg::Exec(c));
        (self.waker)();
    }

    pub fn input(&self, i: Input) {
        let _ = self.tx.send(Msg::Input(i));
        (self.waker)();
    }

    #[allow(dead_code)]
    pub fn fullscreen(&self, on: bool) {
        let _ = self.tx.send(Msg::Fullscreen(on));
        (self.waker)();
    }

    #[allow(dead_code)]
    pub fn cover(&self, maximized: bool, pointer_near: bool) {
        let _ = self.tx.send(Msg::Cover { maximized, pointer_near });
        (self.waker)();
    }

    #[allow(dead_code)]
    pub fn quit(&self) {
        let _ = self.tx.send(Msg::Quit);
        (self.waker)();
    }
}

/// Starts every service; returns the channel the island's commands go to.
pub fn start(tx: Sender<Msg>, waker: Arc<dyn Fn() + Send + Sync>) -> Sender<Command> {
    let rep = Reporter { tx, waker };
    let (cmd_tx, cmd_rx) = channel::<Command>();
    let settings = Arc::new(crate::config::load());
    let dnd = load_dnd();
    rep.patch(move |d| d.dnd = dnd);
    clock::start(rep.clone());
    ipc::start(rep.clone());
    net::start(rep.clone());
    rates::start(rep.clone());
    downloads::start(rep.clone());
    phone::start(rep.clone());
    trains::start(rep.clone());
    organizer::start(rep.clone());
    culprit::start(rep.clone());
    #[cfg(any(target_os = "linux", windows))]
    netwatch::start(rep.clone());
    #[cfg(target_os = "linux")]
    {
        niri::start(rep.clone());
        devices::start(rep.clone());
        audio_route::start(rep.clone());
        audio_mixer::start(rep.clone());
        location::start(rep.clone());
        linux::start(rep.clone(), cmd_rx, settings, dnd);
    }
    #[cfg(not(target_os = "linux"))]
    other::start(rep, cmd_rx, settings);
    cmd_tx
}

pub(crate) fn state_file(name: &str) -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state")))?;
    let dir = base.join("open-island");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(name))
}

/// Meetings already announced, remembered across restarts (a restart must not announce one twice).
pub fn load_announced() -> Vec<String> {
    state_file("meeting-announced").and_then(|p| std::fs::read_to_string(p).ok()).map(|s| s.lines().map(str::to_string).collect()).unwrap_or_default()
}

pub fn save_announced_if_changed(keys: &[String]) {
    use std::sync::Mutex;
    static LAST: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let Ok(mut last) = LAST.lock() else { return };
    if *last == keys {
        return;
    }
    *last = keys.to_vec();
    if let Some(p) = state_file("meeting-announced") {
        let _ = std::fs::write(p, keys.join("\n"));
    }
}

pub fn load_dnd() -> bool {
    state_file("dnd").and_then(|p| std::fs::read_to_string(p).ok()).is_some_and(|s| s.trim() == "on")
}

pub fn save_dnd(on: bool) {
    if let Some(p) = state_file("dnd") {
        let _ = std::fs::write(p, if on { "on" } else { "off" });
    }
}

/// Today's attention per app, kept between runs (day start in epoch ms, then app and milliseconds).
pub fn load_attention() -> Option<(i64, Vec<(String, i64)>)> {
    let v: serde_json::Value = serde_json::from_str(&state_file("attention.json").and_then(|p| std::fs::read_to_string(p).ok())?).ok()?;
    let day = v.get("day")?.as_i64()?;
    let apps = v.get("apps")?.as_array()?.iter().filter_map(|a| Some((a.get(0)?.as_str()?.to_string(), a.get(1)?.as_i64()?))).collect();
    Some((day, apps))
}

pub fn save_attention(day: i64, apps: &[(String, i64)]) {
    let Some(p) = state_file("attention.json") else { return };
    let _ = std::fs::write(p, serde_json::json!({"day": day, "apps": apps}).to_string());
}

/// The reminders set from the command bar, kept between runs.
pub fn load_reminders() -> Vec<island_ui::island::Reminder> {
    let Some(text) = state_file("reminders.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
    items
        .iter()
        .filter_map(|v| Some(island_ui::island::Reminder { due_ms: v.get("due")?.as_i64()?, text: v.get("text")?.as_str()?.to_string(), repeat: v.get("repeat").and_then(|r| r.as_str()).map_or(island_core::task::Repeat::Never, island_core::task::Repeat::from_rrule) }))
        .collect()
}

pub fn save_reminders_if_changed(reminders: &[island_ui::island::Reminder]) {
    use std::sync::Mutex;
    static LAST: Mutex<Option<Vec<island_ui::island::Reminder>>> = Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return };
    if last.as_deref() == Some(reminders) || (last.is_none() && reminders.is_empty()) {
        return;
    }
    *last = Some(reminders.to_vec());
    let Some(p) = state_file("reminders.json") else { return };
    let items: Vec<serde_json::Value> = reminders.iter().map(|r| match r.repeat.to_rrule() {
        Some(rule) => serde_json::json!({"due": r.due_ms, "text": r.text, "repeat": rule}),
        None => serde_json::json!({"due": r.due_ms, "text": r.text}),
    }).collect();
    if let Ok(text) = serde_json::to_string(&items) {
        let _ = std::fs::write(p, text);
    }
}

/// The last day a recap card ("evening-recap", "morning-recap") came, so that a restart does not show it again.
pub fn load_recap_day(name: &str) -> Option<i64> {
    state_file(name).and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| s.trim().parse().ok())
}

pub fn save_recap_day_if_changed(name: &'static str, day: i64) {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static LAST: Mutex<Option<HashMap<&'static str, i64>>> = Mutex::new(None);
    if day == i64::MIN {
        return;
    }
    let Ok(mut g) = LAST.lock() else { return };
    if g.get_or_insert_with(HashMap::new).insert(name, day) == Some(day) {
        return;
    }
    if let Some(p) = state_file(name) {
        let _ = std::fs::write(p, day.to_string());
    }
}

/// The lines run from the command bar, kept between runs (Up recalls them).
pub fn load_bar_history() -> Vec<String> {
    let Some(text) = state_file("bar-history.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    serde_json::from_str::<Vec<String>>(&text).unwrap_or_default()
}

pub fn save_bar_history_if_changed(history: &[String]) {
    use std::sync::Mutex;
    static LAST: Mutex<Option<Vec<String>>> = Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return };
    if last.as_deref() == Some(history) || (last.is_none() && history.is_empty()) {
        return;
    }
    *last = Some(history.to_vec());
    if let (Some(p), Ok(text)) = (state_file("bar-history.json"), serde_json::to_string(history)) {
        let _ = std::fs::write(p, text);
    }
}

/// Exams and hand-ins, kept between runs.
pub fn load_deadlines() -> Vec<island_ui::island::Deadline> {
    let Some(text) = state_file("deadlines.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
    items
        .iter()
        .filter_map(|v| Some(island_ui::island::Deadline { day: v.get("day")?.as_i64()?, text: v.get("text")?.as_str()?.to_string(), course: v.get("course").and_then(|c| c.as_str()).unwrap_or("").to_string(), est_min: v.get("est").and_then(|e| e.as_u64()).unwrap_or(0) as u32 }))
        .collect()
}

pub fn save_deadlines_if_changed(d: &[island_ui::island::Deadline]) {
    use std::sync::Mutex;
    static LAST: Mutex<Option<Vec<island_ui::island::Deadline>>> = Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return };
    if last.as_deref() == Some(d) || (last.is_none() && d.is_empty()) {
        return;
    }
    *last = Some(d.to_vec());
    let items: Vec<serde_json::Value> = d.iter().map(|x| serde_json::json!({"day": x.day, "text": x.text, "course": x.course, "est": x.est_min})).collect();
    if let (Some(p), Ok(text)) = (state_file("deadlines.json"), serde_json::to_string(&items)) {
        let _ = std::fs::write(p, text);
    }
}

/// How often each page was opened (by page key), kept between runs for the dock.
pub fn load_page_usage() -> Vec<(String, u32)> {
    let Some(text) = state_file("page-usage.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    serde_json::from_str::<std::collections::BTreeMap<String, u32>>(&text).map(|m| m.into_iter().collect()).unwrap_or_default()
}

pub fn save_page_usage_if_changed(u: &[(String, u32)]) {
    use std::sync::Mutex;
    static LAST: Mutex<Option<Vec<(String, u32)>>> = Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return };
    if last.as_deref() == Some(u) || (last.is_none() && u.is_empty()) {
        return;
    }
    *last = Some(u.to_vec());
    let map: std::collections::BTreeMap<&str, u32> = u.iter().map(|(k, n)| (k.as_str(), *n)).collect();
    if let (Some(p), Ok(text)) = (state_file("page-usage.json"), serde_json::to_string(&map)) {
        let _ = std::fs::write(p, text);
    }
}

/// How fast the battery drains (percent per hour), learned from use, kept between runs.
pub fn load_drain() -> Option<f64> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(state_file("power.json")?).ok()?).ok()?;
    v.get("drain")?.as_f64()
}

pub fn save_drain_if_changed(rate: Option<f64>) {
    use std::sync::Mutex;
    static LAST: Mutex<Option<Option<u64>>> = Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return };
    // a tenth of a percent is the same drain
    let key = rate.map(|r| (r * 10.0).round() as u64);
    if *last == Some(key) || (last.is_none() && key.is_none()) {
        return;
    }
    *last = Some(key);
    if let (Some(r), Some(p)) = (rate, state_file("power.json")) {
        let _ = std::fs::write(p, serde_json::json!({"drain": (r * 10.0).round() / 10.0}).to_string());
    }
}

/// What to take along, kept between runs: (thing, course).
pub fn load_bring() -> Vec<island_ui::island::Bring> {
    let Some(text) = state_file("bring.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
    items.iter().filter_map(|v| Some(island_ui::island::Bring { text: v.get("text")?.as_str()?.to_string(), course: v.get("course").and_then(|c| c.as_str()).unwrap_or("").to_string() })).collect()
}

pub fn save_bring_if_changed(b: &[island_ui::island::Bring]) {
    use std::sync::Mutex;
    static LAST: Mutex<Option<Vec<island_ui::island::Bring>>> = Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return };
    if last.as_deref() == Some(b) || (last.is_none() && b.is_empty()) {
        return;
    }
    *last = Some(b.to_vec());
    let items: Vec<serde_json::Value> = b.iter().map(|x| serde_json::json!({"text": x.text, "course": x.course})).collect();
    if let (Some(p), Ok(text)) = (state_file("bring.json"), serde_json::to_string(&items)) {
        let _ = std::fs::write(p, text);
    }
}

/// The notes pinned to the Today page, kept between runs.
pub fn load_pins() -> Vec<String> {
    let Some(text) = state_file("pins.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    serde_json::from_str::<Vec<String>>(&text).unwrap_or_default()
}

pub fn save_pins_if_changed(pins: &[String]) {
    use std::sync::Mutex;
    static LAST: Mutex<Option<Vec<String>>> = Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return };
    if last.as_deref() == Some(pins) || (last.is_none() && pins.is_empty()) {
        return;
    }
    *last = Some(pins.to_vec());
    if let (Some(p), Ok(text)) = (state_file("pins.json"), serde_json::to_string(pins)) {
        let _ = std::fs::write(p, text);
    }
}

/// One task as it is kept on disk (`todos.json`).
pub fn todo_to_json(t: &island_ui::island::Todo) -> serde_json::Value {
    let mut o = serde_json::json!({"uid": t.uid, "text": t.text});
    let m = o.as_object_mut().expect("an object");
    if let Some(d) = t.due {
        m.insert("due".into(), d.into());
    }
    if !t.course.is_empty() {
        m.insert("course".into(), t.course.clone().into());
    }
    if t.est_min > 0 {
        m.insert("est".into(), t.est_min.into());
    }
    if let Some(r) = t.repeat.to_rrule() {
        m.insert("repeat".into(), r.into());
    }
    if t.synced {
        m.insert("synced".into(), true.into());
    }
    o
}

/// Reads a task from `todos.json`: the current form, or an older one (a bare string, or `daily` with the day it was last
/// ticked), which becomes a task that repeats every day and rests until tomorrow when it was ticked today.
pub fn todo_from_json(v: &serde_json::Value, today: i64, index: usize) -> Option<island_ui::island::Todo> {
    use island_core::task::Repeat;
    let uid = |v: &serde_json::Value| v.get("uid").and_then(|u| u.as_str()).filter(|u| !u.is_empty()).map_or_else(|| format!("oi-old-{today:x}-{index}-{}", std::process::id()), str::to_string);
    match v {
        serde_json::Value::String(t) => Some(island_ui::island::Todo { uid: uid(v), text: t.clone(), ..Default::default() }),
        o => {
            let text = o.get("text")?.as_str()?.to_string();
            if o.get("daily").and_then(|d| d.as_bool()).unwrap_or(false) {
                let done = o.get("done_day").and_then(|d| d.as_i64()).unwrap_or(i64::MIN);
                return Some(island_ui::island::Todo { uid: uid(v), text, repeat: Repeat::Daily, due: Some(if done >= today { done + 1 } else { today }), ..Default::default() });
            }
            Some(island_ui::island::Todo {
                uid: uid(v),
                text,
                due: o.get("due").and_then(|d| d.as_i64()),
                course: o.get("course").and_then(|c| c.as_str()).unwrap_or("").to_string(),
                est_min: o.get("est").and_then(|e| e.as_u64()).unwrap_or(0) as u32,
                repeat: o.get("repeat").and_then(|r| r.as_str()).map_or(Repeat::Never, Repeat::from_rrule),
                synced: o.get("synced").and_then(|s| s.as_bool()).unwrap_or(false),
            })
        }
    }
}

/// Today as days since 1970 (local).
pub fn local_day() -> i64 {
    use chrono::Datelike;
    chrono::Local::now().date_naive().num_days_from_ce() as i64 - 719_163
}

/// The to-do list, kept between runs.
pub fn load_todos() -> Vec<island_ui::island::Todo> {
    let Some(text) = state_file("todos.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
    let today = local_day();
    items.iter().enumerate().filter_map(|(i, v)| todo_from_json(v, today, i)).collect()
}

pub fn save_todos_if_changed(todos: &[island_ui::island::Todo]) {
    use std::sync::Mutex;
    static LAST: Mutex<Option<Vec<island_ui::island::Todo>>> = Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return };
    if last.as_deref() == Some(todos) || (last.is_none() && todos.is_empty()) {
        return;
    }
    *last = Some(todos.to_vec());
    let Some(p) = state_file("todos.json") else { return };
    let items: Vec<serde_json::Value> = todos.iter().map(todo_to_json).collect();
    if let Ok(text) = serde_json::to_string(&items) {
        let _ = std::fs::write(p, text);
    }
}

/// Study time done (day, course, minutes), kept between runs.
pub fn load_study_log() -> Vec<(i64, String, u32)> {
    let Some(text) = state_file("study-log.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
    items.iter().filter_map(|v| Some((v.get("day")?.as_i64()?, v.get("course").and_then(|c| c.as_str()).unwrap_or("").to_string(), v.get("min")?.as_u64()? as u32))).collect()
}

pub fn save_study_log_if_changed(log: &[(i64, String, u32)]) {
    use std::sync::Mutex;
    static LAST: Mutex<Option<Vec<(i64, String, u32)>>> = Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return };
    if last.as_deref() == Some(log) || (last.is_none() && log.is_empty()) {
        return;
    }
    *last = Some(log.to_vec());
    let items: Vec<serde_json::Value> = log.iter().map(|(d, c, m)| serde_json::json!({"day": d, "course": c, "min": m})).collect();
    if let (Some(p), Ok(text)) = (state_file("study-log.json"), serde_json::to_string(&items)) {
        let _ = std::fs::write(p, text);
    }
}

/// The notification history, kept between runs: (id, app, summary, body, epoch ms). Pictures are not kept.
pub fn load_history() -> Vec<(island_ui::model::Incoming, i64)> {
    let Some(text) = state_file("notification-history.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
    let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    items
        .iter()
        .map(|v| (island_ui::model::Incoming { id: v.get("id").and_then(|x| x.as_u64()).unwrap_or(0) as u32, app: s(v, "app"), summary: s(v, "summary"), body: s(v, "body"), desktop_entry: s(v, "desktop_entry"), ..Default::default() }, v.get("at").and_then(|x| x.as_i64()).unwrap_or(0)))
        .collect()
}

pub fn save_history_if_changed(history: &[(island_ui::model::Incoming, i64)]) {
    use std::sync::Mutex;
    static LAST: Mutex<Vec<(u32, i64)>> = Mutex::new(Vec::new());
    let sig: Vec<(u32, i64)> = history.iter().map(|(n, at)| (n.id, *at)).collect();
    let Ok(mut last) = LAST.lock() else { return };
    if *last == sig {
        return;
    }
    *last = sig;
    let Some(p) = state_file("notification-history.json") else { return };
    let items: Vec<serde_json::Value> = history.iter().map(|(n, at)| serde_json::json!({"id": n.id, "app": n.app, "summary": n.summary, "body": n.body, "desktop_entry": n.desktop_entry, "at": at})).collect();
    if let Ok(text) = serde_json::to_string(&items) {
        // messages are private: owner-only
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            if let Ok(mut f) = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&p) {
                let _ = f.write_all(text.as_bytes());
            }
        }
        #[cfg(not(unix))]
        let _ = std::fs::write(&p, text);
    }
}

#[cfg(test)]
mod organizer_state_tests {
    use super::*;
    use island_core::task::Repeat;

    #[test]
    fn a_task_survives_the_disk() {
        let t = island_ui::island::Todo { uid: "oi-1".into(), text: "Report".into(), due: Some(20_470), course: "algoritmi".into(), est_min: 120, repeat: Repeat::Weekly(0b0001010), synced: true };
        assert_eq!(todo_from_json(&todo_to_json(&t), 20_000, 0), Some(t));
        let plain = island_ui::island::Todo { uid: "oi-2".into(), text: "x".into(), ..Default::default() };
        assert_eq!(todo_to_json(&plain).as_object().unwrap().len(), 2, "only what is set is written");
    }

    #[test]
    fn the_older_forms_of_the_list_still_load() {
        let today = 20_000;
        let bare = todo_from_json(&serde_json::json!("Buy milk"), today, 3).unwrap();
        assert_eq!((bare.text.as_str(), bare.repeat, bare.due), ("Buy milk", Repeat::Never, None));
        assert!(!bare.uid.is_empty(), "it gets an id, so that it can be kept on a server");
        let ticked = todo_from_json(&serde_json::json!({"text": "charger", "daily": true, "done_day": today}), today, 0).unwrap();
        assert_eq!((ticked.repeat, ticked.due), (Repeat::Daily, Some(today + 1)), "ticked today: it rests until tomorrow");
        let waiting = todo_from_json(&serde_json::json!({"text": "charger", "daily": true, "done_day": today - 5}), today, 0).unwrap();
        assert_eq!(waiting.due, Some(today), "not ticked today: it is due today");
        assert!(todo_from_json(&serde_json::json!({"nope": 1}), today, 0).is_none());
        assert_ne!(bare.uid, todo_from_json(&serde_json::json!("Other"), today, 4).unwrap().uid, "each gets its own id");
    }
}
