//! `open-island doctor`: what works on this machine and what is missing, in plain words.

use std::path::Path;

/// True when `name` is an executable in one of the `path` directories (`;` or `:` separated).
pub fn tool_in_path(name: &str, path: &str) -> bool {
    let sep = if cfg!(windows) { ';' } else { ':' };
    path.split(sep).filter(|d| !d.is_empty()).any(|d| {
        let base = Path::new(d).join(name);
        base.is_file() || (cfg!(windows) && base.with_extension("exe").is_file())
    })
}

/// (command, what it is for), per platform.
pub fn tools() -> Vec<(&'static str, &'static str)> {
    if cfg!(target_os = "linux") {
        vec![
            ("wpctl", "volume and mute"),
            ("pactl", "audio outputs, the microphone indicator"),
            ("parec", "the equalizer (real audio levels)"),
            ("pw-dump", "the screen-share indicator"),
            ("nmcli", "the Wi-Fi switch"),
            ("bluetoothctl", "the Bluetooth switch"),
            ("brightnessctl", "setting the brightness"),
            ("wl-paste", "the clipboard history"),
            ("wl-copy", "putting an entry back on the clipboard"),
            ("wlsunset", "night light"),
            ("systemd-inhibit", "keep awake"),
            ("pw-play", "notification sounds (or paplay / ffplay)"),
            ("niri", "hiding under fullscreen windows (niri only)"),
        ]
    } else {
        vec![]
    }
}

pub fn run() {
    println!("open-island {}\n", env!("CARGO_PKG_VERSION"));
    let ok = |b: bool| if b { "ok     " } else { "missing" };
    let path = std::env::var("PATH").unwrap_or_default();

    println!("Session");
    let wayland = std::env::var("WAYLAND_DISPLAY").ok();
    println!("  {} Wayland display ({})", ok(wayland.is_some()), wayland.as_deref().unwrap_or("not set: a plain window will be used"));
    println!("  {} DBUS_SESSION_BUS_ADDRESS (media and notifications on Linux)", ok(std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() || !cfg!(target_os = "linux")));

    println!("\nGraphics");
    match gpu_name() {
        Some(n) => println!("  ok      GPU: {n}"),
        None => println!("  missing no GPU adapter wgpu can use (Vulkan, Metal or DX12 is needed)"),
    }
    let fonts = island_ui::text::Fonts::load_system(Some(&crate::config::load().font));
    match fonts.family() {
        Some(f) => println!("  ok      font: {f} (from the configuration)"),
        None if crate::config::load().font.is_empty() => println!("  ok      font: the built-in list (SF Pro, Helvetica Neue, Inter, Segoe UI, ...)"),
        None => println!("  missing the configured font was not found: the built-in list is used"),
    }
    println!("  {} a text font", ok(fonts.has_text_font()));

    println!("\nConfiguration");
    match crate::config::path() {
        Some(p) => {
            let exists = p.exists();
            println!("  {} {} {}", ok(exists), p.display(), if exists { "" } else { "(optional: nothing set yet, open the settings menu)" });
            let s = crate::config::load();
            println!("  calendars: {}   weather location: {}", s.calendars.len(), if s.latitude.is_some() && s.longitude.is_some() { "set" } else { "not set (no weather)" });
        }
        None => println!("  missing no configuration directory"),
    }

    let tools = tools();
    if !tools.is_empty() {
        println!("\nTools (each one only enables its own feature)");
        for (t, why) in tools {
            println!("  {} {t:<16} {why}", ok(tool_in_path(t, &path)));
        }
    }
    #[cfg(target_os = "linux")]
    notification_owner();
}

fn gpu_name() -> Option<String> {
    let instance = vello::wgpu::Instance::new(vello::wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&vello::wgpu::RequestAdapterOptions::default())).ok()?;
    Some(adapter.get_info().name)
}

#[cfg(target_os = "linux")]
fn notification_owner() {
    println!("\nNotifications");
    let Ok(conn) = zbus::blocking::Connection::session() else {
        println!("  missing no session bus");
        return;
    };
    let Ok(dbus) = zbus::blocking::Proxy::new(&conn, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus") else { return };
    let owner: Result<String, _> = dbus.call("GetNameOwner", &("org.freedesktop.Notifications",));
    match owner {
        Err(_) => println!("  ok      nobody owns the notification name: the island will"),
        Ok(o) => {
            let pid: Option<u32> = dbus.call("GetConnectionUnixProcessID", &(o,)).ok();
            let comm = pid.and_then(|p| std::fs::read_to_string(format!("/proc/{p}/comm")).ok()).unwrap_or_default();
            let comm = comm.trim();
            match comm {
                "open-island" => println!("  ok      the island is the notification daemon"),
                c if crate::services::notif::plan(c) == crate::services::notif::Plan::Stop => println!("  note    {c} owns the name: the island replaces it at start (take_over_notifications)"),
                "noctalia" => println!("  note    Noctalia owns the name: the island asks it to stop (noctalia msg config-set notification.enable_daemon false) at start"),
                c => println!("  missing {c} owns the name and is not a daemon the island stops: set it to not claim notifications"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_tools_on_a_path() {
        let dir = std::env::temp_dir().join(format!("open-island-doctor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let name = if cfg!(windows) { "faketool.exe" } else { "faketool" };
        std::fs::write(dir.join(name), "").unwrap();
        let sep = if cfg!(windows) { ";" } else { ":" };
        let path = format!("/nonexistent{sep}{}{sep}", dir.display());
        assert!(tool_in_path("faketool", &path));
        assert!(!tool_in_path("othertool", &path));
        assert!(!tool_in_path("faketool", ""));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
