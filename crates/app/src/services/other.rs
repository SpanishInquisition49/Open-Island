//! Services for Windows and macOS. What is implemented here is the cross-platform part (battery through
//! the platform API, opening URLs); media controls and notification capture need each OS's own API and
//! are not implemented yet (see the project README).

use super::Reporter;
use island_ui::model::Command;
use std::sync::mpsc::Receiver;

pub fn start(_rep: Reporter, cmd_rx: Receiver<Command>, _settings: std::sync::Arc<crate::config::Settings>) {
    std::thread::spawn(move || {
        for c in cmd_rx {
            if let Command::OpenUrl(u) = c {
                if u.starts_with("http://") || u.starts_with("https://") {
                    open_url(&u);
                }
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
