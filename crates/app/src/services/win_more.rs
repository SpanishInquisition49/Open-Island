//! More Windows services: the Wi-Fi and Bluetooth switches (the radio API), where the attention is (the window in
//! front, for the Activity page), going to the window of the app behind a notification, and the position
//! (the location API, used by `location.rs`).
//!
//! None of this has been run on a Windows machine yet: it is type-checked with
//! `cargo xwin check --target x86_64-pc-windows-msvc`.

use super::Reporter;
use island_ui::model::{Command, Input};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::Duration;

static RADIO_TX: Mutex<Option<Sender<Command>>> = Mutex::new(None);

pub fn start(rep: Reporter) {
    let (tx, rx) = channel();
    if let Ok(mut t) = RADIO_TX.lock() {
        *t = Some(tx);
    }
    radios(rep.clone(), rx);
    attention(rep);
}

/// The commands of this module; anything else is nobody's on Windows.
pub fn command(c: Command) {
    match c {
        Command::ToggleWifi | Command::ToggleBluetooth | Command::NextProfile | Command::RefreshQuick => {
            if let Some(t) = RADIO_TX.lock().ok().and_then(|t| t.clone()) {
                let _ = t.send(c);
            }
        }
        Command::FocusApp { desktop_entry, app } => {
            std::thread::spawn(move || focus_app(&desktop_entry, &app));
        }
        _ => {}
    }
}

// ---- Wi-Fi and Bluetooth --------------------------------------------------------------------------------

fn radio(kind: windows::Devices::Radios::RadioKind) -> Option<windows::Devices::Radios::Radio> {
    use windows::Devices::Radios::Radio;
    Radio::GetRadiosAsync().ok()?.join().ok()?.into_iter().find(|r| r.Kind().is_ok_and(|k| k == kind))
}

fn is_on(kind: windows::Devices::Radios::RadioKind) -> Option<bool> {
    use windows::Devices::Radios::RadioState;
    radio(kind)?.State().ok().map(|s| s == RadioState::On)
}

fn flip(kind: windows::Devices::Radios::RadioKind) {
    use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioState};
    // the first call may show the system's permission question; later ones are instant
    if !Radio::RequestAccessAsync().and_then(|o| o.join()).is_ok_and(|s| s == RadioAccessStatus::Allowed) {
        log::warn!("the radios may not be switched (access denied in the privacy settings)");
        return;
    }
    let Some(r) = radio(kind) else { return };
    let to = if r.State().is_ok_and(|s| s == RadioState::On) { RadioState::Off } else { RadioState::On };
    if let Err(e) = r.SetStateAsync(to).and_then(|o| o.join()) {
        log::warn!("cannot switch the radio: {e}");
    }
}

/// The switches on the quick-settings page: read every 3 s while the page is open (30 s otherwise) and right after a toggle.
fn radios(rep: Reporter, rx: Receiver<Command>) {
    use windows::Devices::Radios::RadioKind;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let mut first = true;
        let mut last: Option<(Option<bool>, Option<bool>)> = None;
        let mut last_profile: Option<Option<&'static str>> = None;
        loop {
            let got = if first {
                first = false;
                Ok(Command::RefreshQuick)
            } else {
                rx.recv_timeout(Duration::from_secs(if super::QUICK_WANTED.load(std::sync::atomic::Ordering::Relaxed) { 3 } else { 30 }))
            };
            match got {
                Ok(Command::ToggleWifi) => flip(RadioKind::WiFi),
                Ok(Command::ToggleBluetooth) => flip(RadioKind::Bluetooth),
                Ok(Command::NextProfile) => next_power_profile(),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                _ => {}
            }
            let profile = power_profile();
            if last_profile != Some(profile) {
                last_profile = Some(profile);
                rep.patch(move |d| d.profile = profile.map(String::from));
            }
            let now = (is_on(RadioKind::WiFi), is_on(RadioKind::Bluetooth));
            if last != Some(now) {
                last = Some(now);
                rep.patch(move |d| {
                    d.wifi = now.0;
                    d.bluetooth = now.1;
                });
            }
        }
    });
}

// ---- power plan -----------------------------------------------------------------------------------------

fn powercfg(args: &[&str]) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let o = std::process::Command::new("powercfg").args(args).creation_flags(0x0800_0000).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// The plan in force, as one of the island's profile names (None: a custom plan, or no powercfg).
fn power_profile() -> Option<&'static str> {
    powercfg(&["/getactivescheme"]).and_then(|t| super::parse::win_power_profile(&t))
}

/// The same cycle as on Linux: performance, balanced, power saver. A plan Windows does not offer on this machine
/// (modern laptops hide High performance) makes `/setactive` fail, and the next one is tried.
fn next_power_profile() {
    const ORDER: [&str; 3] = ["performance", "balanced", "powersave"];
    let at = power_profile().and_then(|p| ORDER.iter().position(|o| *o == p)).unwrap_or(ORDER.len() - 1);
    for step in 1..=ORDER.len() {
        let name = ORDER[(at + step) % ORDER.len()];
        if let Some(guid) = super::parse::win_power_guid(name) {
            if powercfg(&["/setactive", guid]).is_some() {
                return;
            }
        }
    }
}

// ---- windows ------------------------------------------------------------------------------------------

/// The program a process runs, without the folder and the `.exe`.
pub fn process_name(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        ok.then(|| std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().into_owned())).flatten()
    }
}

fn title_of(hwnd: windows::Win32::Foundation::HWND) -> String {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;
    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) }.max(0) as usize;
    String::from_utf16_lossy(&buf[..n])
}

fn class_of(hwnd: windows::Win32::Foundation::HWND) -> String {
    use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) }.max(0) as usize;
    String::from_utf16_lossy(&buf[..n])
}

fn pid_of(hwnd: windows::Win32::Foundation::HWND) -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

/// Where the attention is, for the Activity page: the program and the title of the window in front, reported when
/// they change (checked once a second). The island's own window and the desktop do not count.
fn attention(rep: Reporter) {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    std::thread::spawn(move || {
        let mut last: Option<(String, String)> = None;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let hwnd = unsafe { GetForegroundWindow() };
            if hwnd.0.is_null() || pid_of(hwnd) == std::process::id() || super::windows::is_shell_class(&class_of(hwnd)) {
                continue;
            }
            let Some(app) = process_name(pid_of(hwnd)) else { continue };
            let now = (app, title_of(hwnd));
            if last.as_ref() != Some(&now) {
                last = Some(now.clone());
                log::debug!("focus: {} / {}", now.0, now.1);
                rep.input(Input::Focused { app: now.0, title: now.1 });
            }
        }
    });
}

/// The windows an application could be behind: visible, with a title, not tool windows.
fn app_windows() -> Vec<(isize, super::windows_focus::Win)> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowLongW, IsWindowVisible, GWL_EXSTYLE, WS_EX_TOOLWINDOW};
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let out = &mut *(lparam.0 as *mut Vec<(isize, super::windows_focus::Win)>);
        if IsWindowVisible(hwnd).as_bool() && (GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0) == 0 {
            let title = title_of(hwnd);
            if !title.is_empty() && pid_of(hwnd) != std::process::id() {
                if let Some(app) = process_name(pid_of(hwnd)) {
                    out.push((hwnd.0 as isize, super::windows_focus::Win { id: (hwnd.0 as isize).to_string(), app_id: app, title }));
                }
            }
        }
        BOOL(1)
    }
    let mut v: Vec<(isize, super::windows_focus::Win)> = vec![];
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut v as *mut _ as isize));
    }
    v
}

/// Brings the window of the app behind a notification to the front. A notification carries the app's user-model id
/// (`Telegram.TelegramDesktop`, `Chrome`) and its display name; they are matched against the programs' file names.
pub fn focus_app(desktop_entry: &str, app: &str) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE};
    let wins = app_windows();
    let list: Vec<_> = wins.iter().map(|(_, w)| w.clone()).collect();
    let Some(w) = super::windows_focus::pick(&list, desktop_entry, app) else {
        log::debug!("no window for {desktop_entry} / {app}");
        return;
    };
    let Some(&(h, _)) = wins.iter().find(|(_, x)| x.id == w.id) else { return };
    unsafe {
        let hwnd = HWND(h as *mut _);
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

// ---- position -----------------------------------------------------------------------------------------

/// A fix from Windows' location service (Wi-Fi, GPS, IP, whatever the machine has), with its accuracy in metres:
/// None when location is off in the privacy settings or the machine has no way to tell.
pub fn position() -> Option<(f64, f64, f64)> {
    use windows::Devices::Geolocation::Geolocator;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let g = Geolocator::new().ok()?;
    let p = g.GetGeopositionAsync().ok()?.join().ok()?;
    let c = p.Coordinate().ok()?;
    let pt = c.Point().ok()?.Position().ok()?;
    Some((pt.Latitude, pt.Longitude, c.Accuracy().unwrap_or(5000.0)))
}
