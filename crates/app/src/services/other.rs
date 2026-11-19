//! Services for Windows and macOS. What is implemented here is the cross-platform part (battery through
//! the platform API, opening URLs); media controls and notification capture need each OS's own API and
//! are not implemented yet (see the project README).

use super::Reporter;
use island_ui::model::Command;
use std::sync::mpsc::Receiver;

pub fn start(rep: Reporter, cmd_rx: Receiver<Command>, _settings: std::sync::Arc<crate::config::Settings>) {
    super::generic::start_system(rep.clone());
    super::generic::start_battery(rep.clone());
    super::generic::start_clipboard(rep.clone());
    #[cfg(target_os = "windows")]
    super::windows::start(rep.clone());
    #[cfg(target_os = "macos")]
    super::macos::start(rep.clone());
    let rep2 = rep.clone();
    std::thread::spawn(move || {
        for c in cmd_rx {
            match c {
                Command::OpenUrl(u) if u.starts_with("http://") || u.starts_with("https://") || u.starts_with("mailto:") => open_url(&u),
                Command::RestoreClipboard(t) => super::generic::restore_clipboard(&t),
                Command::PlanTrip { .. } => super::travel::command(&rep2, c),
                Command::RefreshPosition => {}
                Command::LoadTile { z, x, y, template, dark } => super::tiles::request(&rep2, z, x, y, template, dark),
                Command::MuteMicStream { .. } => {}
                Command::ReconnectNetwork => {}
                Command::PauseProcs { .. } | Command::ResumeProcs(_) | Command::QuitProcs(_) => {}
                Command::ScanPeers => super::send::scan(rep2.clone()),
                Command::SendFiles { peer, paths } => super::send::send(rep2.clone(), peer, paths),
                Command::CopyCode(c) => super::generic::copy_code(&c),
                Command::PinClipboard(t) => super::clipboard_history::toggle_pin(&rep2, &t),
                Command::ForgetClipboard(t) => super::clipboard_history::forget(&rep2, &t),
                #[cfg(target_os = "windows")]
                other => super::windows::command(other),
                #[cfg(target_os = "macos")]
                other => super::macos::command(other),
                #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                _ => {}
            }
        }
    });
}

#[cfg(target_os = "windows")]
fn open_url(u: &str) {
    let _ = std::process::Command::new("cmd").args(["/C", "start", "", u]).spawn();
}

#[cfg(target_os = "macos")]
fn open_url(u: &str) {
    let _ = std::process::Command::new("open").arg(u).spawn();
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn open_url(u: &str) {
    let _ = std::process::Command::new("xdg-open").arg(u).spawn();
}
