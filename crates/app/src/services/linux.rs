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
    let set_volume_tx = super::latest_wins(|v| drop(run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{:.2}", v)])));
    let set_brightness_tx = super::latest_wins(|v| drop(run("brightnessctl", &["set", &format!("{}%", (v * 100.0).round() as i32)])));
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
                Command::ToggleWifi | Command::ToggleBluetooth | Command::ToggleKeepAwake | Command::ToggleNightLight | Command::NextProfile | Command::NextSink | Command::RefreshQuick => {
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
                Command::RefreshCalendars => {}
                // the app itself writes the configuration for these
                Command::AddFilter { .. } | Command::RemoveFilter(_) | Command::SetOption { .. } | Command::AddCalendar(_) | Command::AddCalDav { .. } | Command::AddICloud { .. } | Command::RenameCalendar { .. } | Command::SetCalendarColor { .. } | Command::RemoveCalendar(_) | Command::SetFont(_) | Command::SetLatitude(_) | Command::SetLongitude(_) | Command::SetNotificationSound(_) => {}
                Command::RestoreClipboard(t) => clipboard::restore(&t),
                Command::PlanTrip { .. } | Command::SearchPlaces { .. } | Command::PlanRoute { .. } => super::travel::command(&rep, c),
                Command::RefreshPosition => super::location::refresh(),
                Command::LoadTile { z, x, y, template, dark } => super::tiles::request(&rep, z, x, y, template, dark),
                Command::MuteMicStream { stream, mute } => {
                    run("pactl", &["set-source-output-mute", &stream.to_string(), if mute { "1" } else { "0" }]);
                }
                Command::ReconnectNetwork => {
                    std::thread::spawn(super::netwatch::reconnect);
                }
                Command::PauseProcs { .. } | Command::ResumeProcs(_) | Command::QuitProcs(_) => super::culprit::command(&rep, &c),
                Command::PinTrain(_) | Command::UnpinTrain | Command::SetHome(_) => super::trains::command(&rep, &c),
                Command::PhoneOpen(_) | Command::PhoneSend { .. } | Command::PhoneSearch(_) | Command::PhoneCall(_) | Command::PhoneAudio(_) => super::phone::command(&rep, &c),
                Command::Ocr => super::ocr::start(rep.clone()),
                Command::FindMaterials(_) | Command::OpenFile(_) => super::materials::command(&rep, &c),
                Command::SaveNote(_) | Command::Find(_) | Command::CreateEvent { .. } | Command::PushTask { .. } | Command::SyncTasks | Command::DeleteTask { .. } | Command::MoveEvent { .. } | Command::CancelEvent { .. } => {
                    super::organizer::command(&rep, &c);
                }
                Command::ScanPeers => super::send::scan(rep.clone()),
                Command::SendFiles { peer, paths } => super::send::send(rep.clone(), peer, paths),
                Command::SetSink(_) | Command::SetSource(_) | Command::SetStreamVolume { .. } | Command::ToggleStreamMute(_) => {
                    super::audio_mixer::command(&c);
                }
                Command::CopyCode(c) => clipboard::copy_code(&c),
                Command::PinClipboard(t) => super::clipboard_history::toggle_pin(&rep, &t),
                Command::ForgetClipboard(t) => super::clipboard_history::forget(&rep, &t),
                // a slider being dragged sends dozens of these a second: only the latest is worth a process
                Command::SetVolume(v) => {
                    let _ = set_volume_tx.send(v);
                }
                Command::ToggleMute => {
                    run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"]);
                }
                Command::SetBrightness(v) => {
                    let _ = set_brightness_tx.send(v);
                }
                Command::OpenUrl(u) => {
                    if u.starts_with("http://") || u.starts_with("https://") || u.starts_with("mailto:") {
                        run("xdg-open", &[&u]);
                    }
                }
                Command::FocusApp { desktop_entry, app } => {
                    std::thread::spawn(move || super::windows_focus::focus_app(&desktop_entry, &app));
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
    std::thread::spawn(move || {
        let mut last: Option<Option<Battery>> = None;
        loop {
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
        // only a change is worth waking the island for
        if last != Some(b) {
            last = Some(b);
            rep.patch(move |d| d.battery = b);
        }
        std::thread::sleep(Duration::from_secs(5));
        }
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

/// Which lines of `pactl subscribe` a watcher wants to hear.
pub(crate) type Wanted = fn(&str) -> bool;

/// Who listens to the audio server's events, and for which.
#[derive(Default)]
struct Hub {
    listeners: Vec<(Wanted, std::sync::mpsc::Sender<String>)>,
}

impl Hub {
    fn publish(&mut self, line: &str) {
        self.listeners.retain(|(wanted, tx)| !wanted(line) || tx.send(line.to_string()).is_ok());
    }
}

/// The lines of `pactl subscribe` (what PulseAudio / PipeWire report as it happens) that `wanted` accepts, so the audio
/// server is read when something changes instead of every few hundred milliseconds. None when pactl is missing.
///
/// Every watcher says what it cares about because each `pactl` command it then runs is itself reported (a client comes
/// and goes): a watcher that read again at any line fed itself, some thirty commands a second for as long as the island
/// ran. One `pactl subscribe` serves them all.
pub(crate) fn pactl_events(wanted: Wanted) -> Option<std::sync::mpsc::Receiver<String>> {
    static HUB: std::sync::OnceLock<Option<Arc<std::sync::Mutex<Hub>>>> = std::sync::OnceLock::new();
    let hub = HUB.get_or_init(start_hub).as_ref()?;
    let (tx, rx) = std::sync::mpsc::channel();
    hub.lock().ok()?.listeners.push((wanted, tx));
    Some(rx)
}

fn spawn_subscribe() -> Option<std::process::Child> {
    use std::os::unix::process::CommandExt;
    let mut cmd = Proc::new("pactl");
    cmd.arg("subscribe").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    // SAFETY: only prctl is called between fork and exec, which is async-signal-safe. Without this a `pactl subscribe`
    // outlives an island that is killed (it only notices when it next has something to write).
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
    cmd.spawn().ok()
}

fn start_hub() -> Option<Arc<std::sync::Mutex<Hub>>> {
    use std::io::{BufRead, BufReader};
    let hub: Arc<std::sync::Mutex<Hub>> = Default::default();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let shared = hub.clone();
    // the child dies with the thread that spawned it, so one thread spawns it and stays to read it
    std::thread::spawn(move || {
        let mut started = Some(started_tx);
        let mut wait = 2;
        loop {
            let child = spawn_subscribe();
            if let Some(tx) = started.take() {
                let _ = tx.send(child.is_some());
                if child.is_none() {
                    return;
                }
            } else if child.is_some() {
                // the audio server was away: what it holds now may be anything
                if let Ok(mut h) = shared.lock() {
                    for line in ["Event 'change' on server #0", "Event 'change' on sink #0", "Event 'change' on source #0"] {
                        h.publish(line);
                    }
                }
            }
            if let Some(mut child) = child {
                let began = std::time::Instant::now();
                if let Some(out) = child.stdout.take() {
                    for line in BufReader::new(out).lines().map_while(Result::ok) {
                        if let Ok(mut h) = shared.lock() {
                            h.publish(&line);
                        }
                    }
                }
                let _ = child.kill();
                let _ = child.wait();
                if began.elapsed() > Duration::from_secs(60) {
                    wait = 2;
                }
            }
            std::thread::sleep(Duration::from_secs(wait));
            wait = (wait * 2).min(60);
        }
    });
    started_rx.recv().unwrap_or(false).then_some(hub)
}

/// Waits for an event, then for the burst it belongs to to end: `quiet` without another one, and never longer than
/// `most` after the first. False when no more events will come.
pub(crate) fn next_change(events: &std::sync::mpsc::Receiver<String>, quiet: Duration, most: Duration) -> bool {
    if events.recv().is_err() {
        return false;
    }
    let end = std::time::Instant::now() + most;
    loop {
        let left = end.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() || events.recv_timeout(quiet.min(left)).is_err() {
            return true;
        }
    }
}

/// The volume bar: an output changed, or which one is the default.
fn volume_event(line: &str) -> bool {
    line.contains(" on sink #") || line.contains(" on server")
}

/// The microphone indicator: a recording stream or an input changed.
fn recording_event(line: &str) -> bool {
    line.contains(" on source-output #") || line.contains(" on source #")
}

fn audio_and_brightness(rep: Reporter, settings: Arc<Settings>) {
    std::thread::spawn(move || {
        let (mut last_vol, mut last_br): (Option<(f32, bool)>, Option<f32>) = (None, None);
        let mut last_bloop = std::time::Instant::now();
        let mut last_vol_read = std::time::Instant::now();
        let mut last_change = std::time::Instant::now() - Duration::from_secs(60);
        let events = pactl_events(volume_event);
        let mut first = true;
        loop {
            // the volume is read when the audio server says something changed (and once a few seconds as a
            // safety net); the backlight has no events, a sysfs read every second and a half costs nothing
            // While something is changing both are looked at often (so a bar can follow a key held down or a
            // slider being dragged at nearly the display's rate); at rest the backlight is read four times a second
            // and the volume when the audio server says so.
            let busy = last_change.elapsed() < Duration::from_secs(2);
            let poll = Duration::from_millis(if busy { 30 } else { 250 });
            let mut read_volume_now = first;
            match &events {
                Some(rx) => match rx.recv_timeout(poll) {
                    Ok(_) => {
                        // events that piled up while the last reading ran are the same news: read once
                        while rx.try_recv().is_ok() {}
                        read_volume_now = true;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => std::thread::sleep(poll),
                },
                None => std::thread::sleep(poll),
            }
            // (with the server's events to say when it changed, this is only in case one was missed)
            if last_vol_read.elapsed() > Duration::from_millis(if events.is_some() { 5000 } else { 1500 }) {
                read_volume_now = true;
            }
            first = false;
            let br = read_brightness();
            if read_volume_now {
                last_vol_read = std::time::Instant::now();
                let vol = read_volume();
                if vol != last_vol {
                    last_change = std::time::Instant::now();
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
            }
            if br != last_br {
                last_change = std::time::Instant::now();
                if let (Some(b), Some(_)) = (br, last_br) {
                    rep.input(Input::Osd { kind: OsdKind::Brightness, level: b, muted: false });
                }
                if let Some(b) = br {
                    rep.patch(move |d| d.brightness = Some(b));
                }
                last_br = br;
            }
        }
    });
}

// ---- privacy indicators -------------------------------------------------------------------------

/// Programs that have a camera open: name and pids.
pub fn camera_users() -> Vec<(String, Vec<u32>)> {
    let Ok(procs) = std::fs::read_dir("/proc") else { return vec![] };
    let mut by_name: std::collections::BTreeMap<String, Vec<u32>> = Default::default();
    for p in procs.flatten() {
        let Some(pid) = p.file_name().to_string_lossy().parse::<u32>().ok() else { continue };
        let Ok(fds) = std::fs::read_dir(p.path().join("fd")) else { continue };
        let opens = fds.flatten().filter_map(|fd| std::fs::read_link(fd.path()).ok()).any(|t| t.to_string_lossy().starts_with("/dev/video"));
        if opens {
            let name = std::fs::read_to_string(p.path().join("comm")).map(|c| c.trim().to_string()).unwrap_or_default();
            by_name.entry(if name.is_empty() { "An app".into() } else { name }).or_default().push(pid);
        }
    }
    by_name.into_iter().collect()
}

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

/// The applications really recording from a microphone. Streams that capture what the speakers play (the equalizer
/// of a shell, a visualiser, this island's own spectrum) are not the microphone, nor are paused streams or this
/// program's own.
pub fn mic_users(source_outputs_json: &str, sources_json: &str) -> Vec<island_ui::model::MicUser> {
    use serde_json::Value;
    let outputs: Vec<Value> = serde_json::from_str(source_outputs_json).unwrap_or_default();
    let sources: Vec<Value> = serde_json::from_str(sources_json).unwrap_or_default();
    let is_monitor_source = |idx: Option<u64>| {
        sources.iter().any(|s| {
            s.get("index").and_then(Value::as_u64) == idx
                && (s.get("monitor_of_sink").is_some_and(|m| !m.is_null()) || s.get("name").and_then(Value::as_str).is_some_and(|n| n.ends_with(".monitor")))
        })
    };
    outputs
        .iter()
        .filter_map(|o| {
            let props = o.get("properties");
            let prop = |k: &str| props.and_then(|p| p.get(k)).and_then(Value::as_str).unwrap_or("");
            let corked = o.get("corked").and_then(Value::as_bool).unwrap_or(false);
            let captures_a_sink = prop("stream.capture.sink") == "true" || is_monitor_source(o.get("source").and_then(Value::as_u64));
            let ours = ["open-island", "open_island"].iter().any(|n| prop("application.name").contains(n) || prop("application.process.binary").contains(n) || prop("node.name").contains(n));
            if corked || captures_a_sink || ours {
                return None;
            }
            let name = [prop("application.name"), prop("application.process.binary"), prop("node.name")].into_iter().find(|n| !n.is_empty()).unwrap_or("An app").to_string();
            Some(island_ui::model::MicUser { stream: o.get("index")?.as_u64()? as u32, name, pid: prop("application.process.id").parse().ok(), muted: o.get("mute").and_then(Value::as_bool).unwrap_or(false) })
        })
        .collect()
}

pub fn mic_streams_active(source_outputs_json: &str, sources_json: &str) -> bool {
    !mic_users(source_outputs_json, sources_json).is_empty()
}

fn mic_in_use() -> bool {
    match (run("pactl", &["-f", "json", "list", "source-outputs"]), run("pactl", &["-f", "json", "list", "sources"])) {
        (Some(outs), Some(srcs)) => mic_streams_active(&outs, &srcs),
        _ => false,
    }
}

/// True when an application is sharing the screen: PipeWire has a running video *producer* stream (the portal's
/// screen-cast node). Cameras are read from /dev/video instead, and video players only consume.
pub fn screen_capture_active(pw_dump_json: &str) -> bool {
    let Ok(serde_json::Value::Array(objects)) = serde_json::from_str::<serde_json::Value>(pw_dump_json) else { return false };
    objects.iter().any(|o| {
        let info = o.get("info");
        info.and_then(|i| i.pointer("/props/media.class")).and_then(|c| c.as_str()) == Some("Stream/Output/Video") && info.and_then(|i| i.get("state")).and_then(|s| s.as_str()) == Some("running")
    })
}

fn screen_sharing() -> bool {
    run("pw-dump", &[]).is_some_and(|j| screen_capture_active(&j))
}

fn video_devices_exist() -> bool {
    std::fs::read_dir("/dev").is_ok_and(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().starts_with("video")))
}

fn privacy(rep: Reporter) {
    std::thread::spawn(move || {
        let mut last = Privacy::default();
        let events = pactl_events(recording_event);
        let has_camera = video_devices_exist();
        let (mut camera, mut screen) = (false, false);
        let (mut camera_at, mut screen_at, mut mic_at) = (None::<std::time::Instant>, None::<std::time::Instant>, None::<std::time::Instant>);
        let stale = |t: &Option<std::time::Instant>, secs: u64| t.is_none_or(|t| t.elapsed() > Duration::from_secs(secs));
        loop {
            // the microphone is re-read when a recording stream appears or goes; the camera (a scan of /proc) and the
            // screen share (pw-dump) are slower and only looked at every few seconds
            let mut mic_changed = match &events {
                Some(rx) => {
                    // a time-out counts too: that is the look taken every few seconds anyway
                    let _ = rx.recv_timeout(Duration::from_secs(4));
                    while rx.try_recv().is_ok() {}
                    true
                }
                None => {
                    std::thread::sleep(Duration::from_secs(3));
                    true
                }
            };
            // a stream that was already there when the island started raises no event, and a busy bus (a call) never
            // times out: read the microphone at the start and at least every few seconds
            if stale(&mic_at, 4) {
                mic_changed = true;
            }
            if mic_changed {
                mic_at = Some(std::time::Instant::now());
            }
            if has_camera && stale(&camera_at, 5) {
                camera_at = Some(std::time::Instant::now());
                camera = camera_in_use();
            }
            if stale(&screen_at, 8) {
                screen_at = Some(std::time::Instant::now());
                screen = screen_sharing();
            }
            let now = Privacy { mic: if mic_changed { mic_in_use() } else { last.mic }, camera, screen };
            if now != last {
                last = now;
                log::debug!("privacy: mic {} camera {} screen {}", now.mic, now.camera, now.screen);
                rep.patch(move |d| d.privacy = now);
            }
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

/// Looks the lyrics up in the background, trying again a few times when the lookup fails (the network is not
/// always there the moment a track starts), and only keeps the answer if that track is still the one playing.
fn spawn_lyrics(rep: Reporter, current: Arc<std::sync::Mutex<String>>, track_id: String, title: String, artist: String, secs: f64) {
    std::thread::spawn(move || {
        for (attempt, wait) in [0u64, 4, 15, 45].into_iter().enumerate() {
            std::thread::sleep(Duration::from_secs(wait));
            if *current.lock().unwrap() != track_id {
                return;
            }
            match net::fetch_lyrics(&title, &artist, secs) {
                Ok(lrc) => {
                    log::info!("lyrics for {title:?}: {}", lrc.as_ref().map_or("none".to_string(), |l| format!("{} lines", l.lines().count())));
                    if *current.lock().unwrap() == track_id {
                        rep.patch(move |d| d.lyrics_lrc = lrc);
                    }
                    return;
                }
                Err(e) => log::warn!("lyrics for {title:?}: {e} (attempt {})", attempt + 1),
            }
        }
    });
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
        let mut last_sent: Option<Media> = None;
        // the track the lyrics are wanted for: an answer for a track that has gone by is dropped
        let current_track = Arc::new(std::sync::Mutex::new(String::new()));
        let (mut identity_for, mut identity_name): (Option<String>, String) = (None, String::new());
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
                    last_sent = None;
                    // the lyrics stay: when the same track is back (a player that restarted its bus name, a bus
                    // hiccup) there is nothing to fetch again, and a different one replaces them
                    rep.patch(|d| d.media = None);
                }
                std::thread::sleep(Duration::from_millis(1500));
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
                *current_track.lock().unwrap() = track_id.clone();
                rep.patch(|d| d.lyrics_lrc = None);
                if !title.is_empty() {
                    spawn_lyrics(rep.clone(), current_track.clone(), track_id.clone(), title.clone(), artist.clone(), length_us as f64 / 1e6);
                }
            }
            let now = chrono::Utc::now().timestamp_millis();
            if identity_for.as_deref() != Some(name.as_str()) {
                identity_for = Some(name.clone());
                identity_name = identity(&conn, &name);
            }
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
                player: identity_name.clone(),
            };
            // the island extrapolates the position while playing, so only a change (or a jump of the position,
            // a seek) is worth waking it for
            let worth = match &last_sent {
                None => true,
                Some(l) => {
                    let expected = if l.playing { l.position_ms + (now - l.sampled_at_ms) as f64 } else { l.position_ms };
                    l.track_id != m.track_id
                        || l.title != m.title
                        || l.artist != m.artist
                        || l.playing != m.playing
                        || l.length_ms != m.length_ms
                        || l.can_seek != m.can_seek
                        || l.players != m.players
                        || l.player != m.player
                        || l.art.as_ref().map(|a| a.id) != m.art.as_ref().map(|a| a.id)
                        || (m.position_ms - expected).abs() > 1500.0
                }
            };
            let playing = m.playing;
            if worth {
                last_sent = Some(m.clone());
                rep.patch(move |d| d.media = Some(m));
            }
            std::thread::sleep(Duration::from_millis(if playing { 1000 } else { 2000 }));
        }
    });
}

#[cfg(test)]
mod event_tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::time::Instant;

    #[test]
    fn each_watcher_hears_only_what_it_asked_for_and_a_gone_one_is_forgotten() {
        let mut hub = Hub::default();
        let ((vtx, volume), (rtx, recording)) = (channel(), channel());
        hub.listeners.push((volume_event, vtx));
        hub.listeners.push((recording_event, rtx));
        for line in ["Event 'new' on client #1", "Event 'change' on sink #2", "Event 'new' on source-output #3", "Event 'change' on sink-input #4", "Event 'remove' on client #1"] {
            hub.publish(line);
        }
        assert_eq!(volume.try_iter().collect::<Vec<_>>(), ["Event 'change' on sink #2"]);
        assert_eq!(recording.try_iter().collect::<Vec<_>>(), ["Event 'new' on source-output #3"]);
        drop(volume);
        hub.publish("Event 'change' on sink #2");
        assert_eq!(hub.listeners.len(), 1);
    }

    #[test]
    fn a_burst_is_one_change_and_one_that_never_ends_is_still_looked_at() {
        let (tx, rx) = channel();
        for n in 0..5 {
            tx.send(n.to_string()).unwrap();
        }
        assert!(next_change(&rx, Duration::from_millis(20), Duration::from_secs(1)));
        assert!(rx.try_recv().is_err(), "the whole burst was taken");
        // events every 5 ms, never 50 ms apart
        let feed = std::thread::spawn(move || {
            for _ in 0..100 {
                if tx.send(String::new()).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        let began = Instant::now();
        assert!(next_change(&rx, Duration::from_millis(50), Duration::from_millis(100)));
        assert!(began.elapsed() < Duration::from_millis(400), "{:?}", began.elapsed());
        drop(rx);
        feed.join().unwrap();
        let (tx, rx) = channel::<String>();
        drop(tx);
        assert!(!next_change(&rx, Duration::from_millis(1), Duration::from_millis(1)), "no more will come");
    }

    /// `cargo test -p open-island live_audio_events -- --ignored --nocapture`: how often the sound page would be read
    /// again in five seconds, with the real audio server and whatever else is talking to it.
    #[test]
    #[ignore = "reads the real pactl"]
    fn live_audio_events() {
        let events = pactl_events(volume_event).expect("pactl");
        let end = Instant::now() + Duration::from_secs(5);
        let mut reads = 0;
        while events.recv_timeout(end.saturating_duration_since(Instant::now())).is_ok() {
            reads += 1;
            let _ = run("pactl", &["-f", "json", "list", "sinks"]);
        }
        println!("{reads} events asked for a reading in 5 s");
        assert!(reads < 20, "the readings feed themselves");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCES: &str = r#"[
      {"index": 53, "name": "alsa_output.pci.Speaker__sink.monitor", "monitor_of_sink": 1},
      {"index": 54, "name": "alsa_input.pci.Mic2__source", "monitor_of_sink": null}]"#;

    #[test]
    #[ignore = "reads the real pactl"]
    fn live_mic() {
        let outs = run("pactl", &["-f", "json", "list", "source-outputs"]).unwrap_or_default();
        let srcs = run("pactl", &["-f", "json", "list", "sources"]).unwrap_or_default();
        eprintln!("outs {} srcs {}", outs.len(), srcs.len());
        eprintln!("users {:?}", mic_users(&outs, &srcs));
    }

    #[test]
    fn the_microphone_users_have_names_and_streams() {
        let outs = r#"[{"index":9,"source":1,"mute":true,"corked":false,"properties":{"application.name":"Zen","application.process.id":"7147"}},
                       {"index":10,"source":2,"corked":false,"properties":{"application.name":"cava"}},
                       {"index":11,"source":1,"corked":true,"properties":{"application.name":"paused"}}]"#;
        let srcs = r#"[{"index":1,"name":"alsa_input.mic","monitor_of_sink":null},{"index":2,"name":"alsa_output.x.monitor","monitor_of_sink":"x"}]"#;
        let users = mic_users(outs, srcs);
        assert_eq!(users, vec![island_ui::model::MicUser { stream: 9, name: "Zen".into(), pid: Some(7147), muted: true }]);
        assert!(mic_streams_active(outs, srcs) && !mic_streams_active("[]", srcs));
    }

    #[test]
    fn a_running_video_producer_is_a_screen_share() {
        let sharing = r#"[{"id": 1, "info": {"state": "running", "props": {"media.class": "Stream/Output/Video", "node.name": "xdg-desktop-portal"}}},
                          {"id": 2, "info": {"state": "running", "props": {"media.class": "Audio/Sink"}}}]"#;
        assert!(screen_capture_active(sharing));
        let idle = r#"[{"id": 1, "info": {"state": "idle", "props": {"media.class": "Stream/Output/Video"}}}]"#;
        assert!(!screen_capture_active(idle), "a stream that is not running is not sharing");
        let consumer = r#"[{"id": 1, "info": {"state": "running", "props": {"media.class": "Stream/Input/Video"}}}]"#;
        assert!(!screen_capture_active(consumer), "a consumer is a player or a camera app");
        assert!(!screen_capture_active("[]") && !screen_capture_active("nope"));
    }

    #[test]
    fn no_streams_means_no_microphone() {
        assert!(!mic_streams_active("[]", SOURCES));
        assert!(!mic_streams_active("not json", SOURCES), "unreadable output shows nothing rather than a false alarm");
    }

    #[test]
    fn a_stream_on_a_real_microphone_counts() {
        let outs = r#"[{"index": 7, "source": 54, "corked": false, "properties": {"application.name": "Microsoft Teams"}}]"#;
        assert!(mic_streams_active(outs, SOURCES));
    }

    #[test]
    fn spectrum_streams_and_paused_streams_do_not() {
        let monitor = r#"[{"index": 8, "source": 53, "corked": false, "properties": {"application.name": "noctalia"}}]"#;
        assert!(!mic_streams_active(monitor, SOURCES), "capturing a monitor source");
        let flagged = r#"[{"index": 8, "source": 54, "corked": false, "properties": {"stream.capture.sink": "true"}}]"#;
        assert!(!mic_streams_active(flagged, SOURCES), "stream.capture.sink");
        let paused = r#"[{"index": 9, "source": 54, "corked": true, "properties": {"application.name": "Firefox"}}]"#;
        assert!(!mic_streams_active(paused, SOURCES), "corked");
        let ours = r#"[{"index": 10, "source": 54, "corked": false, "properties": {"application.name": "open-island"}}]"#;
        assert!(!mic_streams_active(ours, SOURCES), "this program's own stream");
        let mixed = r#"[{"index": 8, "source": 53, "corked": false, "properties": {}}, {"index": 7, "source": 54, "corked": false, "properties": {"application.name": "Zoom"}}]"#;
        assert!(mic_streams_active(mixed, SOURCES), "one real recorder among spectrum streams");
    }
}
