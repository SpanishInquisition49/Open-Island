//! Samples the numbers of the system page (Linux): CPU per core, temperature, frequency, memory, GPU,
//! battery draw, network rates, disk. Runs only while the page is open (see `SYSTEM_WANTED`).

use super::Reporter;
use island_core::sysmon::{self, CpuTimes};
use island_ui::model::{Gpu, SystemStats};
use std::process::Command;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

fn read(p: &str) -> Option<String> {
    std::fs::read_to_string(p).ok()
}

fn num(p: impl AsRef<std::path::Path>) -> Option<f64> {
    std::fs::read_to_string(p).ok()?.trim().parse().ok()
}

fn cpu_temp() -> Option<f32> {
    for e in std::fs::read_dir("/sys/class/hwmon").ok()?.flatten() {
        let name = read(e.path().join("name").to_str()?).unwrap_or_default();
        if ["k10temp", "coretemp", "zenpower", "cpu_thermal"].contains(&name.trim()) {
            if let Some(t) = num(e.path().join("temp1_input")) {
                return Some(t as f32 / 1000.0);
            }
        }
    }
    None
}

fn cpu_ghz() -> Option<f32> {
    let mut sum = 0.0;
    let mut n = 0;
    for e in std::fs::read_dir("/sys/devices/system/cpu").ok()?.flatten() {
        if let Some(k) = num(e.path().join("cpufreq/scaling_cur_freq")) {
            sum += k;
            n += 1;
        }
    }
    (n > 0).then(|| (sum / n as f64 / 1e6) as f32)
}

/// (watts, minutes to empty/full) from the first battery.
pub(super) fn battery() -> (Option<f32>, Option<u64>) {
    let Ok(rd) = std::fs::read_dir("/sys/class/power_supply") else { return (None, None) };
    for e in rd.flatten() {
        let p = e.path();
        if read(p.join("type").to_str().unwrap_or("")).unwrap_or_default().trim() != "Battery" {
            continue;
        }
        let status = read(p.join("status").to_str().unwrap_or("")).unwrap_or_default();
        let charging = status.trim() == "Charging";
        let (mut now, mut full, mut power) = (num(p.join("energy_now")), num(p.join("energy_full")), num(p.join("power_now")));
        // some batteries count charge (µAh, µA) instead of energy: times the voltage it is the same
        if now.is_none() {
            if let Some(volt) = num(p.join("voltage_now")).map(|v| v / 1e6) {
                let energy = |name: &str| num(p.join(name)).map(|c| c * volt);
                (now, full, power) = (energy("charge_now"), energy("charge_full"), energy("current_now").map(f64::abs));
            }
        }
        return (power.map(|w| (w / 1e6) as f32), island_core::system::battery_eta(now, full, power, charging));
    }
    (None, None)
}

pub fn start(rep: Reporter) {
    std::thread::spawn(move || {
        let mut prev_cpu: Vec<CpuTimes> = vec![];
        let mut prev_net: Option<((u64, u64), Instant)> = None;
        let mut gpu: Option<Gpu> = None;
        let mut disk: Option<(f64, f64)> = None;
        let (mut gpu_at, mut disk_at) = (None::<Instant>, None::<Instant>);
        loop {
            if !super::SYSTEM_WANTED.load(Ordering::Relaxed) {
                prev_cpu.clear();
                prev_net = None;
                std::thread::sleep(Duration::from_millis(400));
                continue;
            }
            let cpu_now = sysmon::parse_proc_stat(&read("/proc/stat").unwrap_or_default());
            let (mut total, mut cores) = (0.0, vec![]);
            if prev_cpu.len() == cpu_now.len() && !cpu_now.is_empty() {
                total = sysmon::usage_percent(prev_cpu[0], cpu_now[0]);
                cores = (1..cpu_now.len()).map(|i| sysmon::usage_percent(prev_cpu[i], cpu_now[i])).collect();
            }
            prev_cpu = cpu_now;
            let net = sysmon::parse_net_dev(&read("/proc/net/dev").unwrap_or_default());
            let now = Instant::now();
            let (rx, tx) = match prev_net {
                Some((p, t)) => {
                    let dt = now.duration_since(t).as_secs_f64().max(0.001);
                    (net.0.saturating_sub(p.0) as f64 / dt, net.1.saturating_sub(p.1) as f64 / dt)
                }
                None => (0.0, 0.0),
            };
            prev_net = Some((net, now));
            if gpu_at.is_none_or(|t| t.elapsed() > Duration::from_secs(6)) {
                gpu_at = Some(Instant::now());
                gpu = Command::new("nvidia-smi")
                    .args(["--query-gpu=pstate,power.draw,temperature.gpu,utilization.gpu", "--format=csv,noheader,nounits"])
                    .stderr(std::process::Stdio::null())
                    .output()
                    .ok()
                    .and_then(|o| sysmon::parse_nvidia_smi(&String::from_utf8_lossy(&o.stdout)))
                    .map(|(pstate, watts, temp_c, util)| Gpu { pstate, watts, temp_c, util });
            }
            if disk_at.is_none_or(|t| t.elapsed() > Duration::from_secs(30)) {
                disk_at = Some(Instant::now());
                disk = Command::new("df").args(["-Pk", "/"]).output().ok().and_then(|o| sysmon::parse_df(&String::from_utf8_lossy(&o.stdout)));
            }
            let (battery_watts, battery_eta_min) = battery();
            let stats = SystemStats {
                cpu_percent: total,
                cpu_temp_c: cpu_temp(),
                cpu_ghz: cpu_ghz(),
                cores,
                memory: sysmon::parse_meminfo(&read("/proc/meminfo").unwrap_or_default()),
                gpu: gpu.clone(),
                battery_watts,
                battery_eta_min,
                net_rx: rx,
                net_tx: tx,
                disk,
                load: sysmon::parse_loadavg(&read("/proc/loadavg").unwrap_or_default()),
            };
            rep.patch(move |d| d.system = Some(stats));
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}
