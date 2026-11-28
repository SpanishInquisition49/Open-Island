//! A second screen on niri: says when one is connected, switches between the screens when asked, and undoes a choice
//! that is not confirmed. The kernel's list of connectors is watched (reading it costs next to nothing); niri is only
//! asked when it changes.
//!
//! Nothing here can leave the machine dark for long: a screen is switched off only after niri says another is on, a
//! choice that switches one off is undone unless confirmed within `CONFIRM`, and whenever every connected screen is off
//! they are switched back on.

use super::Reporter;
use island_core::screens::{self, Mode, Screen};
use island_ui::model::{Command as Asked, Input};
use std::process::Command;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// How long a choice that switches a screen off waits to be confirmed.
pub const CONFIRM: Duration = Duration::from_millis(island_ui::model::SCREEN_CONFIRM_MS as u64);

static ASKED: OnceLock<Sender<Asked>> = OnceLock::new();

/// The connectors with something plugged in, as the kernel names them (`eDP-1`).
fn connected() -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir("/sys/class/drm")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| std::fs::read_to_string(e.path().join("status")).is_ok_and(|s| s.trim() == "connected"))
        .filter_map(|e| e.file_name().to_str()?.split_once('-').map(|(_, name)| name.to_string()))
        .collect();
    out.sort();
    out
}

fn outputs() -> Option<Vec<Screen>> {
    let out = Command::new("niri").args(["msg", "-j", "outputs"]).output().ok().filter(|o| o.status.success())?;
    let all = screens::parse_niri(&serde_json::from_slice(&out.stdout).ok()?);
    let there = connected();
    // niri may still list a screen that was just pulled out
    Some(all.into_iter().filter(|s| there.is_empty() || there.contains(&s.name)).collect())
}

fn switch(name: &str, on: bool) -> bool {
    Command::new("niri").args(["msg", "output", name, if on { "on" } else { "off" }]).status().is_ok_and(|s| s.success())
}

/// Gives `mode`. Screens go off only once niri shows every one asked for as on. Returns whether one was switched off.
fn apply(mode: Mode) -> Result<bool, String> {
    let now = outputs().ok_or("niri did not answer")?;
    let plan = screens::plan(mode, &now).ok_or("there is no such screen")?;
    for (name, _) in plan.iter().filter(|(_, on)| *on) {
        if !switch(name, true) {
            return Err(format!("{name} could not be switched on"));
        }
    }
    let off: Vec<&String> = plan.iter().filter(|(_, on)| !on).map(|(n, _)| n).collect();
    if off.is_empty() {
        return Ok(false);
    }
    // wait for the ones that stay to be really on
    let ready = (0..20).any(|_| {
        std::thread::sleep(Duration::from_millis(250));
        outputs().is_some_and(|s| s.iter().any(|s| s.on && !off.contains(&&s.name)))
    });
    if !ready {
        return Err("the other screen did not come on: nothing was switched off".into());
    }
    for name in off {
        switch(name, false);
    }
    Ok(true)
}

fn rescue() {
    for name in outputs().map_or_else(Vec::new, |s| screens::rescue(&s)) {
        log::warn!("every screen was off: switching {name} on");
        switch(&name, true);
    }
}

pub fn command(c: Asked) {
    if let Some(tx) = ASKED.get() {
        let _ = tx.send(c);
    }
}

pub fn start(rep: Reporter) {
    if std::env::var_os("NIRI_SOCKET").is_none() {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = ASKED.set(tx);
    std::thread::spawn(move || watch(rep, rx));
}

fn watch(rep: Reporter, rx: Receiver<Asked>) {
    // the island may be starting again after it stopped with a screen switched off
    rescue();
    let mut seen: Option<Vec<String>> = None;
    // until when the last choice can still be confirmed
    let mut undo_at: Option<Instant> = None;
    loop {
        let wait = undo_at.map_or(Duration::from_secs(2), |at| at.saturating_duration_since(Instant::now()).min(Duration::from_secs(2)));
        match rx.recv_timeout(wait) {
            Ok(Asked::SetScreens(mode)) => match apply(mode) {
                Ok(true) => {
                    undo_at = Some(Instant::now() + CONFIRM);
                    rep.input(Input::ScreensChanged);
                }
                Ok(false) => undo_at = None,
                Err(e) => rep.input(Input::Toast { summary: "Screens unchanged".into(), body: e }),
            },
            Ok(Asked::KeepScreens) => undo_at = None,
            Ok(Asked::UndoScreens) => undo_at = Some(Instant::now()),
            Ok(Asked::ShowScreens) => rep.input(Input::Screens { all: outputs().unwrap_or_default(), asked: true }),
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if undo_at.is_some_and(|at| Instant::now() >= at) {
            undo_at = None;
            if let Err(e) = apply(Mode::Extend) {
                log::error!("screens: {e}");
                rescue();
            }
        }
        let there = connected();
        if seen.as_ref() != Some(&there) {
            // niri takes a moment to set up a screen that was just plugged in
            std::thread::sleep(Duration::from_millis(800));
            rescue();
            if let Some(all) = outputs() {
                // the first reading is what was there at the start: not news
                if seen.is_some() {
                    rep.input(Input::Screens { all, asked: false });
                }
                seen = Some(there);
            }
        }
    }
}
