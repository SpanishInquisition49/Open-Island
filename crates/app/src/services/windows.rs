//! Windows services: the media session (SMTC), the master volume, the microphone / camera / screen-capture
//! indicators (the privacy registry), fullscreen detection, keep awake and sounds.
//!
//! None of this has been run on a Windows machine yet: it is written against the `windows` crate and
//! type-checked with `cargo check --target x86_64-pc-windows-msvc`.

use super::{art, Reporter};
use island_ui::model::{Command, Input, Media, OsdKind, Privacy};
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
    brightness(rep.clone());
    keep_awake(rep, krx);
}

/// Routes the commands this platform handles itself.
pub fn command(c: Command) {
    let send = |tx: &Mutex<Option<Sender<Command>>>, c: Command| {
        if let Some(t) = tx.lock().unwrap().as_ref() {
            let _ = t.send(c);
        }
    };
    match c {
        Command::MediaPlayPause | Command::MediaNext | Command::MediaPrevious | Command::MediaSeek { .. } | Command::MediaCyclePlayer => send(&MEDIA_TX, c),
        Command::SetVolume(_) | Command::ToggleMute => send(&VOLUME_TX, c),
        Command::ToggleKeepAwake => {
            if let Some(t) = KEEP_AWAKE_TX.lock().unwrap().as_ref() {
                let _ = t.send(true);
            }
        }
        Command::SetBrightness(v) => {
            let pct = (v.clamp(0.0, 1.0) * 100.0).round() as i32;
            std::thread::spawn(move || drop(powershell(&format!("(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods) | Invoke-CimMethod -MethodName WmiSetBrightness -Arguments @{{Timeout=0;Brightness={pct}}}"))));
        }
        Command::PlaySound => play_default_sound(),
        _ => {}
    }
}

// ---- sounds -------------------------------------------------------------------------------------------

fn powershell(script: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", script]).creation_flags(0x0800_0000).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The panel brightness (laptops and tablets answer WMI; desktop monitors do not, and then the slider stays hidden).
fn brightness(rep: Reporter) {
    std::thread::spawn(move || {
        let mut last = None;
        loop {
            let v = powershell("(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness | Select-Object -First 1).CurrentBrightness").and_then(|t| super::parse::win_brightness(&t));
            if v != last {
                last = v;
                rep.patch(move |d| d.brightness = v);
            }
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    });
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
        let mut on = false;
        for _ in rx {
            on = !on;
            unsafe {
                SetThreadExecutionState(if on { ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED } else { ES_CONTINUOUS });
            }
            rep.patch(move |d| d.keep_awake = on);
        }
    });
}

// ---- fullscreen ---------------------------------------------------------------------------------------

fn fullscreen(rep: Reporter) {
    use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN};
    std::thread::spawn(move || {
        let mut last = false;
        loop {
            // the shell's own "a full-screen application is running" answer (games, video, presentations)
            let now = unsafe { SHQueryUserNotificationState() }.is_ok_and(|s| s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE);
            if now != last {
                last = now;
                rep.fullscreen(now);
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

// ---- privacy ------------------------------------------------------------------------------------------

/// True when the capability store says an application is using it right now: an app's `LastUsedTimeStop` is 0
/// (or older than `LastUsedTimeStart`) while it runs.
pub fn capability_in_use(capability: &str) -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let base = format!(r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\{capability}");
    let Ok(root) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(&base) else { return false };
    let in_use = |k: &RegKey| {
        let (start, stop): (u64, u64) = (k.get_value("LastUsedTimeStart").unwrap_or(0), k.get_value("LastUsedTimeStop").unwrap_or(0));
        start > 0 && start > stop
    };
    let mut keys: Vec<RegKey> = root.enum_keys().flatten().filter_map(|n| root.open_subkey(n).ok()).collect();
    if let Ok(np) = root.open_subkey("NonPackaged") {
        keys.extend(np.enum_keys().flatten().filter_map(|n| np.open_subkey(n).ok()));
    }
    keys.iter().any(in_use)
}

fn privacy(rep: Reporter) {
    std::thread::spawn(move || {
        let mut last = Privacy::default();
        loop {
            let now = Privacy { mic: capability_in_use("microphone"), camera: capability_in_use("webcam"), screen: capability_in_use("graphicsCaptureProgrammatic") };
            if now != last {
                last = now;
                rep.patch(move |d| d.privacy = now);
            }
            std::thread::sleep(Duration::from_secs(3));
        }
    });
}

// ---- volume -------------------------------------------------------------------------------------------

fn endpoint() -> windows::core::Result<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume> {
    use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
        device.Activate(CLSCTX_ALL, None)
    }
}

fn volume(rep: Reporter, rx: Receiver<Command>) {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let mut last: Option<(f32, bool)> = None;
        loop {
            // commands first, so a slider drag feels direct; then read the state (the device can change)
            while let Ok(c) = rx.try_recv() {
                if let Ok(ep) = endpoint() {
                    unsafe {
                        match c {
                            Command::SetVolume(v) => {
                                let _ = ep.SetMasterVolumeLevelScalar(v.clamp(0.0, 1.0), std::ptr::null());
                            }
                            Command::ToggleMute => {
                                let muted = ep.GetMute().map(|m| m.as_bool()).unwrap_or(false);
                                let _ = ep.SetMute(!muted, std::ptr::null());
                            }
                            _ => {}
                        }
                    }
                }
            }
            if let Ok(ep) = endpoint() {
                let now = unsafe { (ep.GetMasterVolumeLevelScalar().unwrap_or(0.0), ep.GetMute().map(|m| m.as_bool()).unwrap_or(false)) };
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
            std::thread::sleep(Duration::from_millis(250));
        }
    });
}

// ---- media (SMTC) -------------------------------------------------------------------------------------

fn media(rep: Reporter, rx: Receiver<Command>) {
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
        loop {
            let sessions: Vec<Session> = manager.GetSessions().map(|v| v.into_iter().collect()).unwrap_or_default();
            let id_of = |s: &Session| s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
            let chosen = selected.as_ref().and_then(|id| sessions.iter().find(|s| id_of(s) == *id).map(|s| s.clone())).or_else(|| manager.GetCurrentSession().ok());
            while let Ok(c) = rx.try_recv() {
                let Some(s) = chosen.as_ref() else { continue };
                match c {
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
                std::thread::sleep(Duration::from_millis(1500));
                continue;
            };
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
                    players: sessions.len() as u32,
                    player: id_of(&s).trim_end_matches(".exe").rsplit(['.', '!']).next().unwrap_or("").to_string(),
                };
                let worth = last_sent.as_ref().is_none_or(|l| {
                    let expected = if l.playing { l.position_ms + (now - l.sampled_at_ms) as f64 } else { l.position_ms };
                    l.track_id != m.track_id || l.playing != m.playing || l.length_ms != m.length_ms || l.players != m.players || l.art.as_ref().map(|a| a.id) != m.art.as_ref().map(|a| a.id) || (m.position_ms - expected).abs() > 1500.0
                });
                if worth {
                    last_sent = Some(m.clone());
                    rep.patch(move |d| d.media = Some(m));
                }
            }
            std::thread::sleep(Duration::from_millis(1000));
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
pub fn set_window_shape(hwnd: isize, rects: &[(i32, i32, i32, i32, i32)]) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{CombineRgn, CreateRoundRectRgn, DeleteObject, SetWindowRgn, RGN_OR};
    let Some(&(x, y, w, h, r)) = rects.first() else { return };
    unsafe {
        let hwnd = HWND(hwnd as *mut _);
        let mut region = CreateRoundRectRgn(x, y, x + w + 1, y + h + 1, r * 2, r * 2);
        for &(x, y, w, h, r) in &rects[1..] {
            let part = CreateRoundRectRgn(x, y, x + w + 1, y + h + 1, r * 2, r * 2);
            CombineRgn(Some(region), Some(region), Some(part), RGN_OR);
            let _ = DeleteObject(part.into());
        }
        // the system owns the region from here on
        if SetWindowRgn(hwnd, Some(region), true) == 0 {
            let _ = DeleteObject(region.into());
            region = Default::default();
        }
        let _ = region;
    }
}
