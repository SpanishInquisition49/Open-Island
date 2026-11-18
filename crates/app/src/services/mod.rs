//! Everything that talks to the operating system: sampling the clock, battery, volume, media players,
//! notifications, weather, calendars, and the local control socket. Each service runs on its own thread
//! and reports through a channel; the UI thread never blocks on I/O.

use island_ui::model::{Command, Data, Input};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;

pub mod art;
pub mod clipboard_history;
pub mod caldav;
pub mod clock;
pub mod icons;
pub mod parse;
pub mod windows_focus;
pub mod sound;
pub mod tz;
pub mod ipc;
pub mod net;
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

/// Set by the app while the system page / the quick settings page is open: the sampling threads idle otherwise.
pub static SYSTEM_WANTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Bumped whenever the settings change, so the services that read them (weather, calendars) refetch at once.
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
    #[cfg(target_os = "linux")]
    {
        niri::start(rep.clone());
        linux::start(rep.clone(), cmd_rx, settings, dnd);
    }
    #[cfg(not(target_os = "linux"))]
    other::start(rep, cmd_rx, settings);
    cmd_tx
}

fn state_file(name: &str) -> Option<std::path::PathBuf> {
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

/// The reminders set from the command bar, kept between runs.
pub fn load_reminders() -> Vec<island_ui::island::Reminder> {
    let Some(text) = state_file("reminders.json").and_then(|p| std::fs::read_to_string(p).ok()) else { return vec![] };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
    items.iter().filter_map(|v| Some(island_ui::island::Reminder { due_ms: v.get("due")?.as_i64()?, text: v.get("text")?.as_str()?.to_string() })).collect()
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
    let items: Vec<serde_json::Value> = reminders.iter().map(|r| serde_json::json!({"due": r.due_ms, "text": r.text})).collect();
    if let Ok(text) = serde_json::to_string(&items) {
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
