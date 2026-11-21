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
            ("niri", "hiding under fullscreen windows, and the activity page (niri only)"),
            ("ping", "the router check of the network warning"),
            ("xdg-open", "opening links, mail and the map in the browser"),
        ]
    } else if cfg!(windows) {
        vec![("powershell", "the brightness slider (laptops and tablets only)")]
    } else if cfg!(target_os = "macos") {
        vec![("osascript", "volume, and the track of Spotify or Music"), ("caffeinate", "keep awake"), ("open", "going to the app behind a notification")]
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

    your_day();

    let tools = tools();
    if !tools.is_empty() {
        println!("\nTools (each one only enables its own feature)");
        for (t, why) in tools {
            println!("  {} {t:<16} {why}", ok(tool_in_path(t, &path)));
        }
    }
    #[cfg(target_os = "linux")]
    println!("  {} {:<16} where you are, for the map, the weather and the trips (else the settings location)", ok(Path::new("/usr/lib/geoclue-2.0/demos/where-am-i").is_file()), "geoclue");
    #[cfg(target_os = "linux")]
    notification_owner();
    #[cfg(windows)]
    {
        println!("\nNotifications");
        match crate::services::windows::notification_db() {
            Some(p) => println!("  ok      other applications' notifications are read from {}", p.display()),
            None => println!("  missing the Windows notification database was not found: only `open-island notify` will show"),
        }
    }
}

/// What the day features have to work with: where home is, the commute, the cards, the pinned train, what is kept for offline.
fn your_day() {
    let s = crate::config::load();
    let on = |b: bool| if b { "ok     " } else { "off    " };
    let age = |ms: i64| {
        let mins = (chrono::Utc::now().timestamp_millis() - ms).max(0) / 60_000;
        if mins < 90 { format!("{mins} min ago") } else if mins < 2 * 24 * 60 { format!("{} h ago", mins / 60) } else { format!("{} days ago", mins / 1440) }
    };
    let clock = |m: i64| if m <= 0 { "off".to_string() } else { format!("{:02}:{:02}", m / 60, m % 60) };
    println!("\nYour day");
    println!("  {} home: {}", on(s.home.is_some()), if s.home.is_some() { crate::config::home_label(&s.home) } else { "not set (the station near home and the pin on the map need it: `home <coordinates or address>`)".to_string() });
    println!("  {} commute: {} min   cards: morning {}, evening {}", on(s.ui.commute_minutes > 0), s.ui.commute_minutes, clock(s.ui.morning_recap_min), clock(s.ui.evening_recap_min));
    match crate::services::state_file("pinned-train.json").and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).and_then(|v| island_core::train::TrainPin::from_json(&v)) {
        Some(t) => println!("  ok      train: {} {} {} -> {}", t.label(), t.time, t.from_name, t.to_name),
        None => println!("  off     train: none pinned (`train <station> <HH:MM>` in the command bar)"),
    }
    match crate::services::state_file("calendar-cache.json").and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
        Some(v) => {
            let n: usize = v.get("sources").and_then(|x| x.as_object()).map_or(0, |o| o.values().filter_map(|l| l.as_array()).map(Vec::len).sum());
            println!("  ok      the schedule kept for offline: {n} events, fetched {}", age(v.get("saved_at").and_then(|x| x.as_i64()).unwrap_or(0)));
        }
        None => println!("  off     no schedule kept for offline yet (it is saved after the first fetch)"),
    }
    match crate::services::state_file("rates.json").and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
        Some(v) => println!("  ok      currency rates: {} currencies, fetched {}", v.get("rates").and_then(|x| x.as_object()).map_or(0, |o| o.len()), age(v.get("at").and_then(|x| x.as_i64()).unwrap_or(0) * 1000)),
        None => println!("  {} currency rates: none yet{}", on(s.ui.currency_rates), if s.ui.currency_rates { " (fetched at the next start)" } else { " (switched off)" }),
    }
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
