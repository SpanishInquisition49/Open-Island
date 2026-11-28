//! Services that work the same on Windows and macOS (through cross-platform crates): the system page, the
//! battery, the clipboard history. Linux has its own versions (they read /proc, sysfs and wl-clipboard).

use super::Reporter;
use island_ui::model::{Battery, SystemStats};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// CPU, memory, network, disk and temperature, sampled only while the system page is open.
pub fn start_system(rep: Reporter) {
    std::thread::spawn(move || {
        // sysinfo 0.40 reports a platform that cannot be read as an error: then the page stays empty
        let (Ok(mut sys), Ok(mut nets), Ok(mut disks), Ok(mut comps)) = (sysinfo::System::new(), sysinfo::Networks::new_with_refreshed_list(), sysinfo::Disks::new_with_refreshed_list(), sysinfo::Components::new_with_refreshed_list()) else {
            log::warn!("the system page needs sysinfo, which cannot read this system");
            return;
        };
        let mut last_net: Option<((u64, u64), Instant)> = None;
        let mut disk_at: Option<Instant> = None;
        let mut disk: Option<(f64, f64)> = None;
        loop {
            if !super::SYSTEM_WANTED.load(Ordering::Relaxed) {
                last_net = None;
                super::signal::wait_until(&super::SYSTEM_WANTED);
                continue;
            }
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            nets.refresh(true);
            let total = nets.iter().fold((0u64, 0u64), |a, (_, n)| (a.0 + n.total_received(), a.1 + n.total_transmitted()));
            let now = Instant::now();
            let (rx, tx) = match last_net {
                Some((p, t)) => {
                    let dt = now.duration_since(t).as_secs_f64().max(0.001);
                    (total.0.saturating_sub(p.0) as f64 / dt, total.1.saturating_sub(p.1) as f64 / dt)
                }
                None => (0.0, 0.0),
            };
            last_net = Some((total, now));
            if disk_at.is_none_or(|t| t.elapsed() > Duration::from_secs(30)) {
                disk_at = Some(Instant::now());
                disks.refresh(true);
                // the system drive: the biggest one mounted at a root
                disk = disks.iter().filter(|d| d.mount_point() == std::path::Path::new("/") || d.mount_point().to_string_lossy().ends_with(":\\")).max_by_key(|d| d.total_space()).map(|d| {
                    let (total, free) = (d.total_space() as f64, d.available_space() as f64);
                    ((total - free) / 1_048_576.0, total / 1_048_576.0)
                });
            }
            comps.refresh(true);
            let cpu_temp = comps.iter().filter(|c| { let l = c.label().to_lowercase(); l.contains("cpu") || l.contains("package") || l.contains("core") }).filter_map(|c| c.temperature()).fold(None, |m: Option<f32>, t| Some(m.map_or(t, |x| x.max(t))));
            let cpus = sys.cpus();
            let mem = island_core::sysmon::Memory { total_mb: sys.total_memory() as f64 / 1_048_576.0, used_mb: sys.used_memory() as f64 / 1_048_576.0, swap_used_mb: sys.used_swap() as f64 / 1_048_576.0 };
            let stats = SystemStats {
                cpu_percent: sys.global_cpu_usage(),
                cpu_temp_c: cpu_temp,
                cpu_ghz: (!cpus.is_empty()).then(|| cpus.iter().map(|c| c.frequency() as f32).sum::<f32>() / cpus.len() as f32 / 1000.0),
                cores: cpus.iter().map(|c| c.usage()).collect(),
                memory: Some(mem),
                gpu: None,
                battery_watts: battery_now().and_then(|b| b.1),
                battery_eta_min: battery_now().and_then(|b| b.2),
                net_rx: rx,
                net_tx: tx,
                disk,
                load: None,
            };
            rep.patch(move |d| d.system = Some(stats));
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

/// (state, watts, minutes to empty/full) of the first battery.
fn battery_now() -> Option<(Battery, Option<f32>, Option<u64>)> {
    use starship_battery::units::{power::watt, ratio::percent, time::minute};
    use starship_battery::State;
    let manager = starship_battery::Manager::new().ok()?;
    let b = manager.batteries().ok()?.flatten().next()?;
    let charging = matches!(b.state(), State::Charging);
    let on_ac = !matches!(b.state(), State::Discharging | State::Empty);
    let eta = if charging { b.time_to_full() } else { b.time_to_empty() }.map(|t| t.get::<minute>().round() as u64);
    Some((Battery { percent: b.state_of_charge().get::<percent>(), charging, on_ac }, Some(b.energy_rate().get::<watt>()), eta))
}

pub fn start_battery(rep: Reporter) {
    std::thread::spawn(move || loop {
        let now = battery_now();
        let eta = now.and_then(|(b, _, eta)| eta.map(|m| (m, b.charging)));
        let b = now.map(|b| b.0);
        rep.patch(move |d| {
            d.battery = b;
            d.battery_eta = eta;
        });
        std::thread::sleep(Duration::from_secs(10));
    });
}

/// Clipboard history by polling the text on the clipboard: newest first, in memory only. Windows and macOS give no
/// way to tell a password manager's entry from another one (there is no x-kde-passwordManagerHint), so entries a
/// manager clears quickly may still appear: only the entries the user pins are written to disk.
pub fn start_clipboard(rep: Reporter) {
    std::thread::spawn(move || {
        let Ok(mut cb) = arboard::Clipboard::new() else { return };
        super::clipboard_history::init(&rep);
        let mut last = String::new();
        loop {
            if let Ok(text) = cb.get_text() {
                if text != last {
                    last = text.clone();
                    super::clipboard_history::record(&rep, &text);
                }
            }
            std::thread::sleep(Duration::from_millis(700));
        }
    });
}

/// Copies a one-time code and clears the clipboard after thirty seconds when it still holds it.
pub fn copy_code(code: &str) {
    super::clipboard_history::ignore(code);
    restore_clipboard(code);
    let code = code.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(30));
        if let Ok(mut cb) = arboard::Clipboard::new() {
            if cb.get_text().is_ok_and(|t| t == code) {
                let _ = cb.clear();
            }
        }
    });
}

pub fn restore_clipboard(text: &str) {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        let _ = cb.set_text(text.to_string());
    }
}
