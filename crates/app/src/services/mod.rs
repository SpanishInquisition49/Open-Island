//! Everything that talks to the operating system: sampling the clock, battery, volume, media players,
//! notifications, weather, calendars, and the local control socket. Each service runs on its own thread
//! and reports through a channel; the UI thread never blocks on I/O.

use island_ui::model::{Command, Data, Input};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;

pub mod art;
#[cfg(target_os = "linux")]
pub mod avatar;
pub mod awake;
pub mod clipboard_history;
pub mod caldav;
pub mod cards;
pub mod clock;
pub mod icons;
pub mod parse;
#[cfg(unix)]
pub mod peer;
#[cfg(target_os = "linux")]
pub mod devices;
#[cfg(target_os = "linux")]
pub mod audio_mixer;
pub mod culprit;
pub mod downloads;
pub mod materials;
pub mod organizer;
pub mod ocr;
pub mod screenshots;
#[cfg(target_os = "linux")]
pub mod screens;
#[cfg(target_os = "linux")]
pub mod polkit;
#[cfg(target_os = "linux")]
pub mod polkit_face;
#[cfg(target_os = "linux")]
mod polkit_talk;
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
pub mod secrets;
pub mod signal;
pub mod sound;
mod state;
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
pub mod receive;
#[cfg(target_os = "linux")]
pub mod keys;
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

// What the services wait for. They are changed through `signal` only, which wakes whoever waits.

/// Set by the app while the system page / the quick settings page is open, and while the equalizer is on screen with
/// music playing: the sampling threads sleep otherwise.
pub static SYSTEM_WANTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static QUICK_WANTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static SPECTRUM_WANTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Bumped when the settings change, so that the services that read them (weather, calendars) read again at once.
pub static CONFIG_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Bumped when the position changes enough to matter to the weather, and when a new fix is wanted.
pub static LOCATION_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static LOCATION_ASK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Bumped to make the calendar thread read the calendars now (the refresh button, opening a stale page).
pub static CALENDAR_REFRESH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub use signal::wait_for;
pub use state::*;

#[allow(dead_code)] // Quit and Fullscreen are only sent by some platforms
pub enum Msg {
    /// Change one part of the sampled data.
    Patch(Box<dyn FnOnce(&mut Data) + Send>),
    /// A one-off event.
    Input(Input),
    /// A command as if the island had issued it (settings from a script).
    Exec(Command),
    /// A change of the settings file that a service made (or the reason it could not).
    ConfigSaved(Result<(), String>),
    Quit,
    Fullscreen(bool),
    /// A window is over the island's place on its screen, and whether the pointer is at the top edge asking for it
    /// (for the platform whose window cannot be pointed at while it is hidden).
    Cover { covered: bool, pointer_near: bool },
    /// The screens, by name, on which a window is over the island's place (a compositor that has the island on any).
    CoveredScreens(Vec<String>),
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

    /// A reporter whose messages a test reads.
    #[cfg(test)]
    pub fn for_tests(tx: Sender<Msg>) -> Self {
        Self { tx, waker: Arc::new(|| {}) }
    }

    pub fn config_saved(&self, result: Result<(), String>) {
        let _ = self.tx.send(Msg::ConfigSaved(result));
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
    pub fn cover(&self, covered: bool, pointer_near: bool) {
        let _ = self.tx.send(Msg::Cover { covered, pointer_near });
        (self.waker)();
    }

    #[allow(dead_code)]
    pub fn covered_screens(&self, screens: Vec<String>) {
        let _ = self.tx.send(Msg::CoveredScreens(screens));
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
    let dnd = load_dnd(now_ms()).0;
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
        receive::start(rep.clone());
        keys::start(rep.clone());
        screens::start(rep.clone());
        polkit::start(rep.clone());
        location::start(rep.clone());
        linux::start(rep.clone(), cmd_rx, settings, dnd);
    }
    #[cfg(not(target_os = "linux"))]
    other::start(rep, cmd_rx, settings);
    cmd_tx
}

/// A system tool whose answers the island reads. `nmcli`, `pactl` and `wpctl` translate their words (and the decimal
/// point) into the language of the session: they are asked in the C locale, the one the island knows.
#[cfg(target_os = "linux")]
pub(crate) fn tool(program: &str) -> std::process::Command {
    let mut c = std::process::Command::new(program);
    c.env("LC_ALL", "C.UTF-8");
    c
}

#[cfg(all(test, target_os = "linux"))]
mod tool_tests {
    #[test]
    fn a_tool_is_asked_in_the_c_locale_whatever_the_session_speaks() {
        let out = super::tool("sh").args(["-c", "echo $LC_ALL"]).env("LANGUAGE", "it").env("LANG", "it_IT.UTF-8").output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "C.UTF-8");
    }
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// A worker that does `job` with the newest value sent to it: values that arrive while it is busy are skipped, so a
/// flood of slider positions costs one command at a time and the last one always lands.
#[cfg(any(target_os = "linux", windows))]
pub(crate) fn latest_wins(job: impl Fn(f32) + Send + 'static) -> std::sync::mpsc::Sender<f32> {
    let (tx, rx) = std::sync::mpsc::channel::<f32>();
    std::thread::spawn(move || {
        while let Ok(mut v) = rx.recv() {
            while let Ok(newer) = rx.try_recv() {
                v = newer;
            }
            job(v);
        }
    });
    tx
}

#[cfg(all(test, any(target_os = "linux", windows)))]
mod latest_tests {
    use super::latest_wins;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[test]
    fn a_flood_of_values_costs_few_jobs_and_the_last_always_lands() {
        let done = Arc::new(Mutex::new(vec![]));
        let d = done.clone();
        let tx = latest_wins(move |v| {
            std::thread::sleep(Duration::from_millis(20)); // a slow command
            d.lock().unwrap().push(v);
        });
        for k in 0..50 {
            tx.send(k as f32).unwrap();
        }
        std::thread::sleep(Duration::from_millis(300));
        let got = done.lock().unwrap().clone();
        assert_eq!(*got.last().unwrap(), 49.0, "the final position is applied");
        assert!(got.len() < 10, "and not fifty processes: {got:?}");
    }
}
