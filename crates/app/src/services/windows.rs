//! Windows services: the media session (SMTC), the master volume, the microphone / camera / screen-capture
//! indicators (the privacy registry), fullscreen detection, keep awake and sounds.
//!
//! None of this has been run on a Windows machine yet: it is written against the `windows` crate and
//! type-checked with `cargo check --target x86_64-pc-windows-msvc`.

use super::{art, Reporter};
use island_core::cover;
use island_ui::model::{Command, Input, LoopMode, Media, OsdKind, Privacy};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::Duration;

static MEDIA_TX: Mutex<Option<Sender<Command>>> = Mutex::new(None);
static KEEP_AWAKE_TX: Mutex<Option<Sender<bool>>> = Mutex::new(None);
static VOLUME_TX: Mutex<Option<Sender<Command>>> = Mutex::new(None);

pub fn start(rep: Reporter) {
    let (mtx, mrx) = channel();
    let (vtx, vrx) = channel();
    let (ktx, krx) = channel();
    *MEDIA_TX.lock().unwrap() = Some(mtx);
    *VOLUME_TX.lock().unwrap() = Some(vtx);
    *KEEP_AWAKE_TX.lock().unwrap() = Some(ktx);
    media(rep.clone(), mrx);
    volume(rep.clone(), vrx);
    privacy(rep.clone());
    fullscreen(rep.clone());
    toasts(rep.clone());
    brightness(rep.clone());
    keep_awake(rep.clone(), krx);
    super::win_more::start(rep.clone());
    super::win_audio::start(rep);
}

/// Routes the commands this platform handles itself.
pub fn command(c: Command) {
    let send = |tx: &Mutex<Option<Sender<Command>>>, c: Command| {
        if let Some(t) = tx.lock().unwrap().as_ref() {
            let _ = t.send(c);
        }
    };
    match c {
        Command::MediaPlayPause | Command::MediaNext | Command::MediaPrevious | Command::MediaSeek { .. } | Command::MediaCyclePlayer | Command::MediaShuffle(_) | Command::MediaLoop(_) => send(&MEDIA_TX, c),
        Command::SetVolume(_) | Command::ToggleMute | Command::SetMic(_) => send(&VOLUME_TX, c),
        Command::ToggleKeepAwake => {
            if let Some(t) = KEEP_AWAKE_TX.lock().unwrap().as_ref() {
                let _ = t.send(true);
            }
        }
        Command::SetBrightness(v) => set_brightness(v),
        Command::SetSink(_) | Command::SetSource(_) | Command::SetStreamVolume { .. } | Command::ToggleStreamMute(_) | Command::NextSink => super::win_audio::command(c),
        Command::PlaySound => play_default_sound(),
        other => super::win_more::command(other),
    }
}

// ---- sounds -------------------------------------------------------------------------------------------

fn powershell(script: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", script]).creation_flags(0x0800_0000).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The panel brightness (laptops and tablets answer WMI; desktop monitors do not, and then the slider stays hidden).
/// Read once at the start, so that the page knows whether to have the slider, then only while the page is open.
fn brightness(rep: Reporter) {
    std::thread::spawn(move || {
        let read = || powershell("(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness | Select-Object -First 1).CurrentBrightness").and_then(|t| super::parse::win_brightness(&t));
        let (mut last, mut read_at, mut was_open) = (read(), std::time::Instant::now(), false);
        rep.patch(move |d| d.brightness = last);
        loop {
            if !was_open {
                super::signal::wait_until(&super::QUICK_WANTED);
            } else {
                std::thread::sleep(Duration::from_millis(300));
            }
            let open = super::QUICK_WANTED.load(std::sync::atomic::Ordering::Relaxed);
            if super::parse::brightness_due(open, was_open, read_at.elapsed()) {
                let v = read();
                read_at = std::time::Instant::now();
                if v != last {
                    last = v;
                    rep.patch(move |d| d.brightness = v);
                }
            }
            was_open = open;
        }
    });
}

/// Sets the panel brightness; a drag of the slider costs one PowerShell at a time, not one for every position.
fn set_brightness(level: f32) {
    static TX: Mutex<Option<Sender<f32>>> = Mutex::new(None);
    let Ok(mut tx) = TX.lock() else { return };
    let tx = tx.get_or_insert_with(|| {
        super::latest_wins(|v| {
            let pct = (v.clamp(0.0, 1.0) * 100.0).round() as i32;
            drop(powershell(&format!("(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods) | Invoke-CimMethod -MethodName WmiSetBrightness -Arguments @{{Timeout=0;Brightness={pct}}}")));
        })
    });
    let _ = tx.send(level);
}

fn play_default_sound() {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC};
    unsafe {
        let _ = PlaySoundW(windows::core::w!("SystemAsterisk"), None, SND_ALIAS | SND_ASYNC);
    }
}

// ---- keep awake ---------------------------------------------------------------------------------------

/// SetThreadExecutionState holds for the thread that called it: a thread of its own waits for the toggles.
fn keep_awake(rep: Reporter, rx: Receiver<bool>) {
    use windows::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED};
    std::thread::spawn(move || {
        let memory = super::awake::memory();
        let apply = |on: bool| {
            unsafe {
                SetThreadExecutionState(if on { ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED } else { ES_CONTINUOUS });
            }
            rep.patch(move |d| d.keep_awake = on);
        };
        // held again from the start when it was on at the end of the last run
        let mut on = super::awake::remembered(memory.as_deref());
        apply(on);
        for _ in rx {
            on = !on;
            super::awake::remember(memory.as_deref(), on);
            apply(on);
        }
    });
}

// ---- fullscreen ---------------------------------------------------------------------------------------

/// A full-screen application, a window over the island's place, and (while there is one) the pointer at the top edge
/// calling for the island. The pointer is only watched while a window is in the way: a hidden window is told nothing.
fn fullscreen(rep: Reporter) {
    use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN};
    std::thread::spawn(move || {
        let (mut full, mut covered, mut near) = (false, false, false);
        let (mut full_at, mut cover_at) = (None::<std::time::Instant>, None::<std::time::Instant>);
        loop {
            let due = |at: Option<std::time::Instant>, every: u64| at.is_none_or(|at| at.elapsed() >= Duration::from_millis(every));
            // the shell's own "a full-screen application is running" answer (games, video, presentations)
            if due(full_at, 1000) {
                full_at = Some(std::time::Instant::now());
                let now = unsafe { SHQueryUserNotificationState() }.is_ok_and(|s| s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE);
                if now != full {
                    full = now;
                    rep.fullscreen(now);
                }
            }
            let (was_covered, was_near) = (covered, near);
            if due(cover_at, 240) {
                cover_at = Some(std::time::Instant::now());
                covered = place_covered();
            }
            near = covered && pointer_zone(near);
            if (covered, near) != (was_covered, was_near) {
                rep.cover(covered, near);
            }
            std::thread::sleep(Duration::from_millis(if covered { 80 } else { 250 }));
        }
    });
}

/// The rectangle of the primary monitor (left, top, right, bottom) in physical pixels.
fn primary_rect() -> Option<(i32, i32, i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
    unsafe {
        let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        GetMonitorInfoW(mon, &mut info).as_bool().then_some((info.rcMonitor.left, info.rcMonitor.top, info.rcMonitor.right, info.rcMonitor.bottom))
    }
}

/// True when the pointer is where the island asks to be shown: at the very top of the primary screen, near its
/// middle; once shown, a wider and taller area keeps it there.
fn pointer_zone(already_near: bool) -> bool {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::HiDpi::GetDpiForSystem;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let Some((l, t, r, _)) = primary_rect() else { return false };
    let mut p = POINT::default();
    if unsafe { GetCursorPos(&mut p) }.is_err() {
        return false;
    }
    let k = unsafe { GetDpiForSystem() } as f64 / 96.0;
    let (cx, dx, dy) = ((l + r) as f64 / 2.0, p.x as f64, (p.y - t) as f64);
    let (half, depth) = if already_near { (330.0 * k, 130.0 * k) } else { (230.0 * k, 3.0 * k) };
    (dx - cx).abs() <= half && dy >= -2.0 && dy <= depth
}

/// What `place_covered` looks for, and whether it found it.
struct Search {
    place: cover::Rect,
    found: bool,
}

/// Whether a window of some application is over the island's place on the primary monitor.
fn place_covered() -> bool {
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::HiDpi::GetDpiForSystem;
    use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
    let Some((l, t, r, b)) = primary_rect() else { return false };
    let scale = f64::from(unsafe { GetDpiForSystem() }) / 96.0;
    let screen = cover::Rect { left: l.into(), top: t.into(), right: r.into(), bottom: b.into() };
    let mut search = Search { place: cover::place(screen, scale), found: false };
    // SAFETY: the callback only runs during the call, while `search` lives; the error returned is the callback stopping
    unsafe {
        let _ = EnumWindows(Some(look), LPARAM(std::ptr::addr_of_mut!(search) as isize));
    }
    search.found
}

/// One top-level window: stops the search when it is in the way. What is cheap to ask is asked first.
unsafe extern "system" fn look(hwnd: windows::Win32::Foundation::HWND, search: windows::Win32::Foundation::LPARAM) -> windows::core::BOOL {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetLayeredWindowAttributes, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible, GWL_EXSTYLE, LWA_ALPHA, WS_EX_LAYERED, WS_EX_TRANSPARENT};
    // SAFETY: `search` is the `Search` the caller of `EnumWindows` holds; every pointer passed on is to a local of the
    // size said
    unsafe {
        let search = &mut *(search.0 as *mut Search);
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return true.into();
        }
        let rect = |r: RECT| cover::Rect { left: r.left.into(), top: r.top.into(), right: r.right.into(), bottom: r.bottom.into() };
        let mut outer = RECT::default();
        if GetWindowRect(hwnd, &mut outer).is_err() || !rect(outer).overlaps(&search.place) {
            return true.into();
        }
        // without the invisible border a window has around it, when the compositor says
        let mut frame = RECT::default();
        let at = if DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, std::ptr::addr_of_mut!(frame).cast(), std::mem::size_of::<RECT>() as u32).is_ok() { frame } else { outer };
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, std::ptr::addr_of_mut!(cloaked).cast(), 4);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut class).max(0) as usize;
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let mut clear = false;
        if style & WS_EX_LAYERED.0 != 0 {
            let (mut alpha, mut flags) = (255u8, Default::default());
            clear = GetLayeredWindowAttributes(hwnd, None, Some(&mut alpha), Some(&mut flags)).is_ok() && flags.0 & LWA_ALPHA.0 != 0 && alpha == 0;
        }
        let seen = cover::Seen {
            at: rect(at),
            shown: cloaked == 0,
            furniture: pid == std::process::id() || is_shell_class(&String::from_utf16_lossy(&class[..n])),
            ghost: clear || style & WS_EX_TRANSPARENT.0 != 0,
        };
        search.found = seen.is_over(&search.place);
        (!search.found).into()
    }
}

/// Windows that are the desktop or the shell, never an application covering the screen.
pub fn is_shell_class(class: &str) -> bool {
    matches!(class, "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "NotifyIconOverflowWindow" | "TopLevelWindowForOverflowXamlIsland" | "XamlExplorerHostIslandWindow")
}

// ---- other applications' notifications ---------------------------------------------------------------

/// The database where Windows keeps the toasts of every application (what the notification centre lists).
pub fn notification_db() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    let p = std::path::Path::new(&base).join(r"Microsoft\Windows\Notifications\wpndatabase.db");
    p.is_file().then_some(p)
}

/// Windows has no way for a plain program to be handed other applications' toasts (the listener API needs a
/// packaged app), so the island reads the database Windows keeps them in: a copy of it every second and a half
/// while it changes, and every toast that is new since the last look is shown as a notification. The Windows
/// banner still shows as well; turning banners off for an application (the toast still goes to the centre)
/// leaves the island as the only one.
fn toasts(rep: Reporter) {
    use island_ui::model::{Incoming, Input};
    use rusqlite::Connection;
    std::thread::spawn(move || {
        let Some(db) = notification_db() else {
            log::warn!("no notification database: other applications' notifications will not show");
            return;
        };
        let tmp = std::env::temp_dir().join("open-island-wpn");
        let _ = std::fs::create_dir_all(&tmp);
        let stamp = |ext: &str| {
            let mut p = db.clone().into_os_string();
            p.push(ext);
            std::fs::metadata(p).ok().map(|m| (m.len(), m.modified().ok()))
        };
        let (mut seen_stamp, mut last_id): (Option<_>, Option<i64>) = (None, None);
        loop {
            std::thread::sleep(Duration::from_millis(1500));
            let now_stamp = (stamp(""), stamp("-wal"));
            if Some(&now_stamp) == seen_stamp.as_ref() {
                continue;
            }
            // read a copy: Windows keeps the original open
            for ext in ["", "-wal", "-shm"] {
                let (mut from, mut to) = (db.clone().into_os_string(), tmp.join("wpndatabase.db").into_os_string());
                from.push(ext);
                to.push(ext);
                if std::fs::copy(&from, &to).is_err() && ext.is_empty() {
                    continue;
                }
            }
            let Ok(conn) = Connection::open(tmp.join("wpndatabase.db")) else { continue };
            let Ok(max) = conn.query_row("SELECT COALESCE(MAX(Id), 0) FROM Notification", [], |r| r.get::<_, i64>(0)) else { continue };
            seen_stamp = Some(now_stamp);
            let Some(after) = last_id else {
                last_id = Some(max); // what is already there is old news
                log::info!("watching the notification database (last toast {max})");
                continue;
            };
            if max <= after {
                last_id = Some(max); // the database was cleared
                continue;
            }
            let Ok(mut q) = conn.prepare("SELECT n.Id, n.Payload, COALESCE(h.PrimaryId, '') FROM Notification n LEFT JOIN NotificationHandler h ON n.HandlerId = h.RecordId WHERE n.Id > ?1 AND n.Type = 'toast' ORDER BY n.Id LIMIT 20") else { continue };
            let rows: Vec<(i64, Vec<u8>, String)> = q.query_map([after], |r| Ok((r.get(0)?, r.get::<_, Vec<u8>>(1).unwrap_or_default(), r.get(2)?))).map(|it| it.flatten().collect()).unwrap_or_default();
            last_id = Some(max);
            for (id, payload, aumid) in rows {
                let xml = super::parse::decode_payload(&payload);
                let Some(t) = super::parse::parse_toast(&xml) else { continue };
                let app = t.attribution.clone().filter(|_| matches!(super::parse::app_name_from_aumid(&aumid).as_str(), "Chrome" | "Edge" | "Firefox")).unwrap_or_else(|| super::parse::app_name_from_aumid(&aumid));
                let image = t.image.as_deref().and_then(|p| std::fs::read(p).ok()).and_then(|b| super::art::decode(&b));
                rep.input(Input::Notification(Incoming { id: 4_000_000 + (id % 1_000_000) as u32, app, summary: t.title, body: t.body, image, desktop_entry: aumid, ..Default::default() }));
            }
        }
    });
}

// ---- privacy ------------------------------------------------------------------------------------------

/// The apps the capability store says are using it right now: an app's `LastUsedTimeStop` is 0 (or older than
/// `LastUsedTimeStart`) while it runs. Names come from the key (`parse::consent_app`); true when it has a process.
pub fn capability_users(capability: &str) -> Vec<(String, bool)> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let base = format!(r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\{capability}");
    let Ok(root) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(&base) else { return vec![] };
    let in_use = |k: &RegKey| {
        let (start, stop): (u64, u64) = (k.get_value("LastUsedTimeStart").unwrap_or(0), k.get_value("LastUsedTimeStop").unwrap_or(0));
        start > 0 && start > stop
    };
    let mut users: Vec<(String, bool)> = root.enum_keys().flatten().filter(|n| n != "NonPackaged").filter(|n| root.open_subkey(n).is_ok_and(|k| in_use(&k))).map(|n| super::parse::consent_app(&n)).collect();
    if let Ok(np) = root.open_subkey("NonPackaged") {
        users.extend(np.enum_keys().flatten().filter(|n| np.open_subkey(n).is_ok_and(|k| in_use(&k))).map(|n| super::parse::consent_app(&n)));
    }
    users.sort();
    users.dedup();
    users
}

pub fn capability_in_use(capability: &str) -> bool {
    !capability_users(capability).is_empty()
}

fn privacy(rep: Reporter) {
    std::thread::spawn(move || {
        let mut last = Privacy::default();
        loop {
            let now = Privacy { mic: capability_in_use("microphone"), camera: capability_in_use("webcam"), screen: capability_in_use("graphicsCaptureProgrammatic") };
            if now != last {
                last = now;
                // who it is, for the system page: a program with a process can be quit from there
                let users = |cap: &str| -> Vec<(String, Vec<u32>)> { capability_users(cap).into_iter().map(|(n, has_proc)| { let pids = if has_proc { super::culprit::pids_named(&n) } else { vec![] }; (n, pids) }).collect() };
                let mic: Vec<island_ui::model::MicUser> = if now.mic { users("microphone").into_iter().map(|(name, pids)| island_ui::model::MicUser { stream: 0, name, pid: pids.first().copied(), muted: false }).collect() } else { vec![] };
                let cam = if now.camera { users("webcam") } else { vec![] };
                rep.patch(move |d| {
                    d.privacy = now;
                    d.mic_users = mic;
                    d.camera_users = cam;
                });
            }
            std::thread::sleep(Duration::from_secs(3));
        }
    });
}

// ---- volume -------------------------------------------------------------------------------------------

type Endpoint = windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;

/// The volume and mute of the default speakers and of the default microphone, when there are such devices.
fn default_devices(enumerator: &windows::Win32::Media::Audio::IMMDeviceEnumerator) -> (Option<Endpoint>, Option<Endpoint>) {
    use windows::Win32::Media::Audio::{eCapture, eCommunications, eMultimedia, eRender};
    use windows::Win32::System::Com::CLSCTX_ALL;
    // SAFETY: calls on a live COM object
    unsafe {
        let of = |flow, role| enumerator.GetDefaultAudioEndpoint(flow, role).and_then(|d| d.Activate::<Endpoint>(CLSCTX_ALL, None)).ok();
        (of(eRender, eMultimedia), of(eCapture, eCommunications))
    }
}

/// Reads the volume four times a second (and at once after a command). The enumerator is made once and the default
/// devices are asked for again once a second: making both at every reading was most of what this thread did.
fn volume(rep: Reporter, rx: Receiver<Command>) {
    use std::sync::mpsc::RecvTimeoutError;
    use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
    std::thread::spawn(move || {
        // SAFETY: once on this thread, before any other COM call
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        // SAFETY: a plain COM object, made on this thread and used only here
        let Ok(enumerator) = (unsafe { CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) }) else {
            log::warn!("no audio device enumerator: no volume bar");
            return;
        };
        let (mut ep, mut mic) = default_devices(&enumerator);
        let mut found_at = std::time::Instant::now();
        let mut last: Option<(f32, bool)> = None;
        let mut last_mic: Option<bool> = None;
        loop {
            // commands first, so a slider drag feels direct; then read the state (the device can change)
            let first = match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(c) => Some(c),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            if found_at.elapsed() >= Duration::from_secs(1) {
                (ep, mic) = default_devices(&enumerator);
                found_at = std::time::Instant::now();
            }
            for c in first.into_iter().chain(rx.try_iter()) {
                // SAFETY: calls on live COM objects
                unsafe {
                    match (c, ep.as_ref(), mic.as_ref()) {
                        (Command::SetMic(muted), _, Some(mic)) => {
                            let to = muted.unwrap_or_else(|| !mic.GetMute().map(|m| m.as_bool()).unwrap_or(false));
                            let _ = mic.SetMute(to, std::ptr::null());
                        }
                        (Command::SetVolume(v), Some(ep), _) => {
                            let _ = ep.SetMasterVolumeLevelScalar(v.clamp(0.0, 1.0), std::ptr::null());
                        }
                        (Command::ToggleMute, Some(ep), _) => {
                            let muted = ep.GetMute().map(|m| m.as_bool()).unwrap_or(false);
                            let _ = ep.SetMute(!muted, std::ptr::null());
                        }
                        _ => {}
                    }
                }
            }
            // SAFETY: calls on live COM objects
            let now = ep.as_ref().and_then(|ep| unsafe { Some((ep.GetMasterVolumeLevelScalar().ok()?, ep.GetMute().map(|m| m.as_bool()).unwrap_or(false))) });
            if let Some(now) = now {
                if Some(now) != last {
                    if last.is_some() {
                        rep.input(Input::Osd { kind: OsdKind::Volume, level: now.0, muted: now.1 });
                    }
                    last = Some(now);
                    rep.patch(move |d| {
                        d.volume = Some(now.0);
                        d.muted = now.1;
                    });
                }
            }
            // SAFETY: a call on a live COM object
            let muted = mic.as_ref().and_then(|m| unsafe { m.GetMute() }.ok()).map(|m| m.as_bool());
            if muted != last_mic {
                last_mic = muted;
                rep.patch(move |d| d.mic_muted = muted);
            }
            // a device that stopped answering is looked for again at once
            if ep.is_some() && now.is_none() {
                (ep, mic) = default_devices(&enumerator);
            }
        }
    });
}

// ---- media (SMTC) -------------------------------------------------------------------------------------

fn media(rep: Reporter, rx: Receiver<Command>) {
    use windows::Media::MediaPlaybackAutoRepeatMode as Repeats;
    use windows::Media::Control::{GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager, GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let Ok(manager) = Manager::RequestAsync().and_then(|op| op.join()) else {
            log::warn!("no media session manager: the player is unavailable");
            return;
        };
        let mut selected: Option<String> = None;
        let mut last_track = String::new();
        let mut art_cache: Option<island_ui::model::Image> = None;
        let mut last_sent: Option<Media> = None;
        // a command that cut the wait short
        let mut woken: Option<Command> = None;
        let wait = |most: u64| match rx.recv_timeout(Duration::from_millis(most)) {
            Ok(c) => Some(c),
            Err(e) => {
                if e == std::sync::mpsc::RecvTimeoutError::Disconnected {
                    std::thread::sleep(Duration::from_millis(most));
                }
                None
            }
        };
        loop {
            let sessions: Vec<Session> = manager.GetSessions().map(|v| v.into_iter().collect()).unwrap_or_default();
            let id_of = |s: &Session| s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
            let chosen = selected.as_ref().and_then(|id| sessions.iter().find(|s| id_of(s) == *id).cloned()).or_else(|| manager.GetCurrentSession().ok());
            let mut acted = false;
            while let Some(c) = woken.take().or_else(|| rx.try_recv().ok()) {
                let Some(s) = chosen.as_ref() else { continue };
                acted = true;
                match c {
                    Command::MediaShuffle(on) => {
                        let _ = s.TryChangeShuffleActiveAsync(on).and_then(|o| o.join());
                        // what the player now says goes to the island even if nothing changed: it may have refused
                        last_sent = None;
                    }
                    Command::MediaLoop(mode) => {
                        let to = match mode {
                            LoopMode::Off => Repeats::None,
                            LoopMode::All => Repeats::List,
                            LoopMode::One => Repeats::Track,
                        };
                        let _ = s.TryChangeAutoRepeatModeAsync(to).and_then(|o| o.join());
                        last_sent = None;
                    }
                    Command::MediaPlayPause => drop(s.TryTogglePlayPauseAsync().and_then(|o| o.join())),
                    Command::MediaNext => drop(s.TrySkipNextAsync().and_then(|o| o.join())),
                    Command::MediaPrevious => drop(s.TrySkipPreviousAsync().and_then(|o| o.join())),
                    Command::MediaSeek { position_ms } => drop(s.TryChangePlaybackPositionAsync((position_ms * 10_000.0) as i64).and_then(|o| o.join())),
                    Command::MediaCyclePlayer => {
                        let ids: Vec<String> = sessions.iter().map(id_of).collect();
                        let at = chosen.as_ref().and_then(|c| ids.iter().position(|i| *i == id_of(c))).unwrap_or(0);
                        selected = ids.get((at + 1) % ids.len().max(1)).cloned();
                    }
                    _ => {}
                }
            }
            let Some(s) = chosen else {
                if last_sent.take().is_some() {
                    rep.patch(|d| d.media = None);
                }
                woken = wait(1500);
                continue;
            };
            if acted {
                // a player needs a moment to take it in
                std::thread::sleep(Duration::from_millis(150));
            }
            if let (Ok(props), Ok(info), Ok(tl)) = (s.TryGetMediaPropertiesAsync().and_then(|o| o.join()), s.GetPlaybackInfo(), s.GetTimelineProperties()) {
                let (title, artist) = (props.Title().map(|t| t.to_string()).unwrap_or_default(), props.Artist().map(|t| t.to_string()).unwrap_or_default());
                let track_id = format!("{}|{title}|{artist}", id_of(&s));
                if track_id != last_track {
                    last_track = track_id.clone();
                    art_cache = thumbnail(&props);
                }
                let playing = info.PlaybackStatus().is_ok_and(|st| st == Status::Playing);
                let now = chrono::Utc::now().timestamp_millis();
                let ticks = |t: windows::Foundation::TimeSpan| t.Duration as f64 / 10_000.0;
                let m = Media {
                    track_id,
                    title,
                    artist,
                    playing,
                    position_ms: tl.Position().map(ticks).unwrap_or(0.0),
                    sampled_at_ms: now,
                    length_ms: tl.EndTime().map(ticks).unwrap_or(0.0),
                    art: art_cache.clone(),
                    can_seek: info.Controls().and_then(|c| c.IsPlaybackPositionEnabled()).unwrap_or(false),
                    shuffle: info.Controls().and_then(|c| c.IsShuffleEnabled()).unwrap_or(false).then(|| info.IsShuffleActive().and_then(|v| v.Value()).ok()).flatten(),
                    repeat: info.Controls().and_then(|c| c.IsRepeatEnabled()).unwrap_or(false).then(|| info.AutoRepeatMode().and_then(|v| v.Value()).ok()).flatten().map(|m| match m {
                        Repeats::List => LoopMode::All,
                        Repeats::Track => LoopMode::One,
                        _ => LoopMode::Off,
                    }),
                    players: sessions.len() as u32,
                    player: id_of(&s).trim_end_matches(".exe").rsplit(['.', '!']).next().unwrap_or("").to_string(),
                };
                let worth = last_sent.as_ref().is_none_or(|l| {
                    let expected = if l.playing { l.position_ms + (now - l.sampled_at_ms) as f64 } else { l.position_ms };
                    l.track_id != m.track_id || l.playing != m.playing || l.length_ms != m.length_ms || l.players != m.players || l.shuffle != m.shuffle || l.repeat != m.repeat || l.art.as_ref().map(|a| a.id) != m.art.as_ref().map(|a| a.id) || (m.position_ms - expected).abs() > 1500.0
                });
                if worth {
                    last_sent = Some(m.clone());
                    rep.patch(move |d| d.media = Some(m));
                }
            }
            woken = wait(1000);
        }
    });
}

/// The cover of the current track, decoded.
fn thumbnail(props: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties) -> Option<island_ui::model::Image> {
    use windows::Storage::Streams::{DataReader, IRandomAccessStreamReference};
    let reference: IRandomAccessStreamReference = props.Thumbnail().ok()?;
    let stream = reference.OpenReadAsync().ok()?.join().ok()?;
    let size = stream.Size().ok()? as u32;
    if size == 0 || size > 8 * 1024 * 1024 {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size).ok()?.join().ok()?;
    let mut bytes = vec![0u8; size as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    art::decode(&bytes)
}

// ---- the window's shape (no transparency) ---------------------------------------------------------------

/// Cuts a window to the union of rounded rectangles (pixels, relative to the window): the island is black, so an
/// opaque window clipped to its shape looks like a transparent one, and the clicks outside it go to what is below.
pub fn set_window_shape(hwnd: isize, rects: &[(i32, i32, i32, i32, i32)], repaint: bool) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{CombineRgn, CreateRoundRectRgn, DeleteObject, SetWindowRgn, RGN_OR};
    unsafe {
        let hwnd = HWND(hwnd as *mut _);
        // no shapes: the whole window again
        let Some(&(x, y, w, h, r)) = rects.first() else {
            SetWindowRgn(hwnd, None, repaint);
            return;
        };
        let mut region = CreateRoundRectRgn(x, y, x + w + 1, y + h + 1, r * 2, r * 2);
        for &(x, y, w, h, r) in &rects[1..] {
            let part = CreateRoundRectRgn(x, y, x + w + 1, y + h + 1, r * 2, r * 2);
            CombineRgn(Some(region), Some(region), Some(part), RGN_OR);
            let _ = DeleteObject(part.into());
        }
        // the system owns the region from here on
        if SetWindowRgn(hwnd, Some(region), repaint) == 0 {
            let _ = DeleteObject(region.into());
            region = Default::default();
        }
        let _ = region;
    }
}
