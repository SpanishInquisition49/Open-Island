//! Linux services: battery (sysfs), volume (wpctl / pactl), brightness (sysfs), media players (MPRIS),
//! notifications (the island becomes the notification daemon when the name is free, otherwise it watches
//! the bus), and the camera / microphone indicators.

use super::{art, clipboard, focus, net, notif, quick, sound, spectrum, sysmon, Reporter};
use island_ui::model::*;
use std::collections::HashMap;
use std::process::Command as Proc;
use std::sync::mpsc::Receiver;
use std::time::Duration;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Value};
use std::sync::Arc;
use crate::config::Settings;

pub fn start(rep: Reporter, cmd_rx: Receiver<Command>, settings: Arc<Settings>, initial_dnd: bool) {
    battery(rep.clone());
    audio_and_brightness(rep.clone(), settings.clone());
    privacy(rep.clone());
    let (media_cmd_tx, media_cmd_rx) = std::sync::mpsc::channel::<Command>();
    let (notif_cmd_tx, notif_cmd_rx) = std::sync::mpsc::channel::<Command>();
    media(rep.clone(), media_cmd_rx);
    sysmon::start(rep.clone());
    clipboard::start(rep.clone());
    let focus_script = focus::start(rep.clone());
    spectrum::start(rep.clone());
    let (quick_tx, quick_rx) = std::sync::mpsc::channel::<Command>();
    quick::start(rep.clone(), quick_rx);
    notif::start(rep.clone(), notif_cmd_rx, settings.clone(), initial_dnd);
    // route the island's commands to the thread that handles them
    std::thread::spawn(move || {
        for c in cmd_rx {
            match c {
                Command::MediaPlayPause | Command::MediaNext | Command::MediaPrevious | Command::MediaSeek { .. } | Command::MediaCyclePlayer => {
                    let _ = media_cmd_tx.send(c);
                }
                Command::InvokeAction { .. } | Command::CloseNotification(_) | Command::SetDnd(_) | Command::NotificationClosed { .. } | Command::NotificationReplied { .. } => {
                    let _ = notif_cmd_tx.send(c);
                }
                Command::ToggleWifi | Command::ToggleBluetooth | Command::ToggleKeepAwake | Command::NextProfile | Command::NextSink | Command::RefreshQuick => {
                    let _ = quick_tx.send(c);
                }
                Command::StartFocus(m) => {
                    if let Some(sc) = &focus_script {
                        focus::run(sc, &["on", &m.to_string()]);
                    }
                }
                Command::StopFocus => {
                    if let Some(sc) = &focus_script {
                        focus::run(sc, &["off"]);
                    }
                }
                // the app itself writes the configuration for these
                Command::AddCalendar(_) | Command::RemoveCalendar(_) | Command::SetFont(_) | Command::SetLatitude(_) | Command::SetLongitude(_) | Command::SetNotificationSound(_) => {}
                Command::RestoreClipboard(t) => clipboard::restore(&t),
                Command::SetVolume(v) => {
                    run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{:.2}", v)]);
                }
                Command::ToggleMute => {
                    run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"]);
                }
                Command::SetBrightness(v) => {
                    run("brightnessctl", &["set", &format!("{}%", (v * 100.0).round() as i32)]);
                }
                Command::OpenUrl(u) => {
                    if u.starts_with("http://") || u.starts_with("https://") {
                        run("xdg-open", &[&u]);
                    }
                }
                Command::PlaySound => match &settings.timer_sound {
                    Some(f) => sound::play(f),
                    None => {
                        run("canberra-gtk-play", &["-i", "complete"]);
                    }
                },
            }
        }
    });
}

fn run(prog: &str, args: &[&str]) -> Option<String> {
    let out = Proc::new(prog).args(args).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

// ---- battery ------------------------------------------------------------------------------------

fn read_num(p: &std::path::Path) -> Option<f64> {
    std::fs::read_to_string(p).ok()?.trim().parse().ok()
}

fn battery(rep: Reporter) {
    std::thread::spawn(move || loop {
        let dir = std::path::Path::new("/sys/class/power_supply");
        let mut found = None;
        let mut ac = None;
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                let kind = std::fs::read_to_string(p.join("type")).unwrap_or_default();
                match kind.trim() {
                    "Battery" if found.is_none() => {
                        let status = std::fs::read_to_string(p.join("status")).unwrap_or_default();
                        found = read_num(&p.join("capacity")).map(|c| (c as f32, status.trim() == "Charging"));
                    }
                    "Mains" => ac = ac.or(read_num(&p.join("online")).map(|v| v > 0.5)),
                    _ => {}
                }
            }
        }
        let b = found.map(|(percent, charging)| Battery { percent, charging, on_ac: ac.unwrap_or(charging) });
        rep.patch(move |d| d.battery = b);
        std::thread::sleep(Duration::from_secs(5));
    });
}

// ---- volume and brightness ----------------------------------------------------------------------

fn read_volume() -> Option<(f32, bool)> {
    let out = run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])?; // "Volume: 0.62 [MUTED]"
    let v: f32 = out.split_whitespace().nth(1)?.parse().ok()?;
    Some((v.clamp(0.0, 1.0), out.contains("MUTED")))
}

fn read_brightness() -> Option<f32> {
    for e in std::fs::read_dir("/sys/class/backlight").ok()?.flatten() {
        let p = e.path();
        if let (Some(cur), Some(max)) = (read_num(&p.join("brightness")), read_num(&p.join("max_brightness"))) {
            if max > 0.0 {
                return Some((cur / max) as f32);
            }
        }
    }
    None
}

fn audio_and_brightness(rep: Reporter, settings: Arc<Settings>) {
    std::thread::spawn(move || {
        let (mut last_vol, mut last_br): (Option<(f32, bool)>, Option<f32>) = (None, None);
        let mut last_bloop = std::time::Instant::now();
        loop {
            let vol = read_volume();
            let br = read_brightness();
            if vol != last_vol {
                if let (Some((v, m)), Some(_)) = (vol, last_vol) {
                    rep.input(Input::Osd { kind: OsdKind::Volume, level: v, muted: m });
                    if let Some(f) = &settings.volume_sound {
                        if last_bloop.elapsed() > Duration::from_millis(250) {
                            last_bloop = std::time::Instant::now();
                            sound::play(f);
                        }
                    }
                }
                if let Some((v, m)) = vol {
                    rep.patch(move |d| {
                        d.volume = Some(v);
                        d.muted = m;
                    });
                }
                last_vol = vol;
            }
            if br != last_br {
                if let (Some(b), Some(_)) = (br, last_br) {
                    rep.input(Input::Osd { kind: OsdKind::Brightness, level: b, muted: false });
                }
                if let Some(b) = br {
                    rep.patch(move |d| d.brightness = Some(b));
                }
                last_br = br;
            }
            std::thread::sleep(Duration::from_millis(400));
        }
    });
}

// ---- privacy indicators -------------------------------------------------------------------------

fn camera_in_use() -> bool {
    let Ok(procs) = std::fs::read_dir("/proc") else { return false };
    for p in procs.flatten() {
        let name = p.file_name();
        if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if let Ok(fds) = std::fs::read_dir(p.path().join("fd")) {
            for fd in fds.flatten() {
                if let Ok(t) = std::fs::read_link(fd.path()) {
                    if t.to_string_lossy().starts_with("/dev/video") {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn mic_in_use() -> bool {
    // PipeWire/PulseAudio: any recording stream that is not a monitor
    run("pactl", &["list", "short", "source-outputs"]).is_some_and(|o| o.lines().any(|l| !l.trim().is_empty()))
}

fn privacy(rep: Reporter) {
    std::thread::spawn(move || {
        let mut last = Privacy::default();
        loop {
            let now = Privacy { mic: mic_in_use(), camera: camera_in_use(), screen: false };
            if now != last {
                last = now;
                rep.patch(move |d| d.privacy = now);
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

// ---- media players (MPRIS) ----------------------------------------------------------------------

const MPRIS: &str = "org.mpris.MediaPlayer2";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";

fn players(conn: &Connection) -> Vec<String> {
    let Ok(dbus) = Proxy::new(conn, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus") else { return vec![] };
    let names: Vec<String> = dbus.call("ListNames", &()).unwrap_or_default();
    names.into_iter().filter(|n| n.starts_with(MPRIS)).collect()
}

fn player_proxy<'a>(conn: &'a Connection, name: &'a str) -> Option<Proxy<'a>> {
    // no property cache: it spawns a background task per proxy, and we poll
    zbus::blocking::proxy::Builder::new(conn)
        .destination(name)
        .ok()?
        .path(MPRIS_PATH)
        .ok()?
        .interface("org.mpris.MediaPlayer2.Player")
        .ok()?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .ok()
}

fn str_of(m: &HashMap<String, OwnedValue>, k: &str) -> String {
    match m.get(k).map(|v| &**v) {
        Some(Value::Str(s)) => s.to_string(),
        Some(Value::Array(a)) => a.iter().filter_map(|v| if let Value::Str(s) = v { Some(s.to_string()) } else { None }).collect::<Vec<_>>().join(", "),
        Some(Value::ObjectPath(p)) => p.to_string(),
        _ => String::new(),
    }
}

/// The player to show: a playing one, else the first.
fn pick_player(conn: &Connection, names: &[String]) -> Option<String> {
    let mut first = None;
    for n in names {
        let Some(p) = player_proxy(conn, n) else { continue };
        let status: String = p.get_property("PlaybackStatus").unwrap_or_default();
        if status == "Playing" {
            return Some(n.clone());
        }
        first = first.or(Some(n.clone()));
    }
    first
}

/// The player the user picked while it is still open, otherwise the one that is playing (else the first).
fn choose_player(conn: &Connection, names: &[String], selected: &Option<String>) -> Option<String> {
    match selected {
        Some(s) if names.contains(s) => Some(s.clone()),
        _ => pick_player(conn, names),
    }
}

/// "Spotify", "Firefox": the player's own name, else the bus name's tail.
fn identity(conn: &Connection, name: &str) -> String {
    Proxy::new(conn, name, MPRIS_PATH, MPRIS).ok().and_then(|p| p.get_property::<String>("Identity").ok()).filter(|s| !s.is_empty()).unwrap_or_else(|| name.trim_start_matches("org.mpris.MediaPlayer2.").to_string())
}

fn media(rep: Reporter, cmd_rx: Receiver<Command>) {
    std::thread::spawn(move || {
        let Ok(conn) = Connection::session() else {
            log::warn!("no session bus: media and notifications are unavailable");
            return;
        };
        let mut current: Option<String> = None;
        // the player the user switched to (None: follow the one that is playing)
        let mut selected: Option<String> = None;
        let mut last_track = String::new();
        let mut art_url = String::new();
        let mut cached_art: Option<Image> = None;
        loop {
            // commands first, so a click feels instant
            while let Ok(c) = cmd_rx.try_recv() {
                let mut names = players(&conn);
                names.sort();
                let Some(name) = choose_player(&conn, &names, &selected) else { continue };
                if c == Command::MediaCyclePlayer {
                    let at = names.iter().position(|n| *n == name).unwrap_or(0);
                    selected = names.get((at + 1) % names.len()).cloned();
                    continue;
                }
                let Some(p) = player_proxy(&conn, &name) else { continue };
                match c {
                    Command::MediaPlayPause => drop(p.call_method("PlayPause", &())),
                    Command::MediaNext => drop(p.call_method("Next", &())),
                    Command::MediaPrevious => drop(p.call_method("Previous", &())),
                    Command::MediaSeek { position_ms } => {
                        let md: HashMap<String, OwnedValue> = p.get_property("Metadata").unwrap_or_default();
                        if let Some(Value::ObjectPath(path)) = md.get("mpris:trackid").map(|v| &**v) {
                            drop(p.call_method("SetPosition", &(path.clone(), (position_ms * 1000.0) as i64)));
                        }
                    }
                    _ => {}
                }
            }
            let mut names = players(&conn);
            names.sort();
            let pick = choose_player(&conn, &names, &selected);
            let Some(name) = pick else {
                if current.take().is_some() {
                    rep.patch(|d| {
                        d.media = None;
                        d.lyrics_lrc = None;
                    });
                }
                std::thread::sleep(Duration::from_millis(700));
                continue;
            };
            current = Some(name.clone());
            let Some(p) = player_proxy(&conn, &name) else { continue };
            let md: HashMap<String, OwnedValue> = p.get_property("Metadata").unwrap_or_default();
            let status: String = p.get_property("PlaybackStatus").unwrap_or_default();
            let position_us: i64 = p.get_property("Position").unwrap_or(0);
            let can_seek: bool = p.get_property("CanSeek").unwrap_or(false);
            let length_us = match md.get("mpris:length").map(|v| &**v) {
                Some(Value::I64(v)) => *v,
                Some(Value::U64(v)) => *v as i64,
                _ => 0,
            };
            let (title, artist) = (str_of(&md, "xesam:title"), str_of(&md, "xesam:artist"));
            let track_id = format!("{name}|{}|{title}|{artist}", str_of(&md, "mpris:trackid"));
            let url = str_of(&md, "mpris:artUrl");
            if url != art_url {
                art_url = url.clone();
                cached_art = if url.is_empty() { None } else { art::load(&url) };
            }
            if track_id != last_track {
                last_track = track_id.clone();
                rep.patch(|d| d.lyrics_lrc = None);
                let (t, a, secs) = (title.clone(), artist.clone(), length_us as f64 / 1e6);
                let rep2 = rep.clone();
                if !t.is_empty() {
                    std::thread::spawn(move || {
                        let lrc = net::fetch_lyrics(&t, &a, secs);
                        rep2.patch(move |d| d.lyrics_lrc = lrc);
                    });
                }
            }
            let now = chrono::Utc::now().timestamp_millis();
            let m = Media {
                track_id,
                title,
                artist,
                playing: status == "Playing",
                position_ms: position_us as f64 / 1000.0,
                sampled_at_ms: now,
                length_ms: length_us as f64 / 1000.0,
                art: cached_art.clone(),
                can_seek,
                players: names.len() as u32,
                player: identity(&conn, &name),
            };
            rep.patch(move |d| d.media = Some(m));
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}
