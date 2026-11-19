//! Which program is hogging the machine (Linux): watches the CPU and the memory, and when one program is the cause
//! of a sustained load, says which. It also freezes, resumes and stops programs when asked.

use super::Reporter;
use island_core::culprit::{self as c, Proc};
use island_ui::model::{Command, Culprit};
use std::sync::Mutex;
use std::time::Duration;

const PERIOD: Duration = Duration::from_secs(5);
/// Samples in a row above the threshold before a program is named (3 x 5 s).
const SUSTAINED: u32 = 3;
const HZ: f64 = 100.0;

/// What the island froze: name and pids, so they can be resumed (even after a restart: see `restore`).
static PAUSED: Mutex<Vec<(String, Vec<u32>)>> = Mutex::new(Vec::new());

fn read_procs() -> Vec<Proc> {
    let Ok(rd) = std::fs::read_dir("/proc") else { return vec![] };
    rd.flatten()
        .filter(|e| e.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit()))
        .filter_map(|e| c::parse_stat(&std::fs::read_to_string(e.path().join("stat")).ok()?))
        .collect()
}

/// (busy, total) ticks of all the CPUs, from the first line of /proc/stat.
fn cpu_ticks() -> Option<(u64, u64)> {
    let s = std::fs::read_to_string("/proc/stat").ok()?;
    let v: Vec<u64> = s.lines().next()?.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
    let total: u64 = v.iter().take(8).sum();
    let idle = v.get(3).copied().unwrap_or(0) + v.get(4).copied().unwrap_or(0);
    Some((total.saturating_sub(idle), total))
}

/// Memory still available, as a fraction of all of it.
fn mem_free_fraction() -> Option<f64> {
    let s = std::fs::read_to_string("/proc/meminfo").ok()?;
    let get = |k: &str| s.lines().find_map(|l| l.strip_prefix(k)?.trim().trim_end_matches("kB").trim().parse::<f64>().ok());
    Some(get("MemAvailable:")? / get("MemTotal:")?.max(1.0))
}

fn cores() -> f64 {
    std::thread::available_parallelism().map_or(1.0, |n| n.get() as f64)
}

fn signal(pids: &[u32], sig: i32) {
    for p in pids {
        // SAFETY: kill(2) with a pid we were given from /proc and a plain signal number
        unsafe {
            libc::kill(*p as i32, sig);
        }
    }
}

fn paused_names() -> Vec<String> {
    PAUSED.lock().map(|p| p.iter().map(|(n, _)| n.clone()).collect()).unwrap_or_default()
}

fn save_paused() {
    let Some(f) = super::state_file("paused-procs.json") else { return };
    let v: Vec<(String, Vec<u32>)> = PAUSED.lock().map(|p| p.clone()).unwrap_or_default();
    if v.is_empty() {
        let _ = std::fs::remove_file(f);
    } else if let Ok(j) = serde_json::to_string(&v) {
        let _ = std::fs::write(f, j);
    }
}

/// Programs frozen by an island that has since ended must not stay frozen: wake them (when the pid still belongs to that program).
fn restore() {
    let Some(f) = super::state_file("paused-procs.json") else { return };
    if let Some(v) = std::fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str::<Vec<(String, Vec<u32>)>>(&s).ok()) {
        let procs = read_procs();
        for (name, pids) in v {
            let same: Vec<u32> = pids.into_iter().filter(|p| procs.iter().any(|x| x.pid == *p && x.name == name)).collect();
            signal(&same, libc::SIGCONT);
        }
    }
    let _ = std::fs::remove_file(f);
}

pub fn command(rep: &Reporter, cmd: &Command) {
    match cmd {
        Command::PauseProcs { name, pids } => {
            if c::is_protected(name) {
                return;
            }
            signal(pids, libc::SIGSTOP);
            if let Ok(mut p) = PAUSED.lock() {
                p.retain(|(n, _)| n != name);
                p.push((name.clone(), pids.clone()));
            }
            save_paused();
        }
        Command::ResumeProcs(name) => {
            let pids = PAUSED.lock().ok().and_then(|p| p.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone())).unwrap_or_default();
            signal(&pids, libc::SIGCONT);
            if let Ok(mut p) = PAUSED.lock() {
                p.retain(|(n, _)| n != name);
            }
            save_paused();
        }
        Command::QuitProcs(pids) => {
            // only what is not the session: the pids are checked against the protected names again here
            let procs = read_procs();
            let ok: Vec<u32> = pids.iter().copied().filter(|p| procs.iter().any(|x| x.pid == *p && !c::is_protected(&x.name))).collect();
            signal(&ok, libc::SIGTERM);
            // a frozen program only hears SIGTERM when it is awake
            signal(&ok, libc::SIGCONT);
        }
        _ => return,
    }
    let names = paused_names();
    rep.patch(move |d| d.paused = names);
}

/// The program to blame for a load, if one clearly is.
pub fn blame(before: &[Proc], now: &[Proc], secs: f64, cpu_frac: f64, ncpu: f64, mem_free: f64) -> Option<Culprit> {
    let page_kb = 4.0;
    if mem_free < 0.08 {
        let top = c::by_memory(now, page_kb).into_iter().next()?;
        return Some(Culprit { protected: c::is_protected(&top.name), name: top.name, pids: top.pids, cores: 0.0, mem_mb: top.rss_mb as f32, memory: true });
    }
    if cpu_frac < 0.85 {
        return None;
    }
    let top = c::by_cpu(before, now, secs, HZ, page_kb).into_iter().next()?;
    // it must be a real share of what the machine is doing, not one of many
    (top.cores >= 1.0 && top.cores >= 0.4 * cpu_frac * ncpu).then(|| Culprit { protected: c::is_protected(&top.name), name: top.name, pids: top.pids, cores: top.cores as f32, mem_mb: top.rss_mb as f32, memory: false })
}

/// Who is using the microphone and the camera, read every two seconds while the system page is open.
fn watch_privacy(rep: Reporter) {
    std::thread::spawn(move || {
        let (mut last_mic, mut last_cam) = (Vec::new(), Vec::new());
        loop {
            if !super::SYSTEM_WANTED.load(std::sync::atomic::Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(700));
                continue;
            }
            let run = |a: &[&str]| std::process::Command::new("pactl").args(a).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
            let mic = super::linux::mic_users(&run(&["-f", "json", "list", "source-outputs"]), &run(&["-f", "json", "list", "sources"]));
            let cam = super::linux::camera_users();
            if mic != last_mic || cam != last_cam {
                (last_mic, last_cam) = (mic.clone(), cam.clone());
                rep.patch(move |d| {
                    d.mic_users = mic;
                    d.camera_users = cam;
                });
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

pub fn start(rep: Reporter) {
    watch_privacy(rep.clone());
    restore();
    std::thread::spawn(move || {
        let ncpu = cores();
        let (mut last_ticks, mut last_procs, mut hot, mut shown) = (cpu_ticks(), Vec::<Proc>::new(), 0u32, false);
        loop {
            std::thread::sleep(PERIOD);
            let now_ticks = cpu_ticks();
            let cpu_frac = match (last_ticks, now_ticks) {
                (Some((b0, t0)), Some((b1, t1))) if t1 > t0 => (b1 - b0) as f64 / (t1 - t0) as f64,
                _ => 0.0,
            };
            last_ticks = now_ticks;
            let mem_free = mem_free_fraction().unwrap_or(1.0);
            let busy = cpu_frac >= 0.6 || mem_free < 0.08;
            if !busy {
                hot = 0;
                last_procs.clear();
                if shown {
                    shown = false;
                    rep.patch(|d| d.culprit = None);
                }
                continue;
            }
            let procs = read_procs();
            hot = if cpu_frac >= 0.85 || mem_free < 0.08 { hot + 1 } else { 0 };
            let found = if hot >= SUSTAINED && !last_procs.is_empty() { blame(&last_procs, &procs, PERIOD.as_secs_f64(), cpu_frac, ncpu, mem_free) } else { None };
            last_procs = procs;
            if let Some(f) = &found {
                log::debug!("culprit: {} ({:.1} cores, {:.0} MB, memory {})", f.name, f.cores, f.mem_mb, f.memory);
            }
            if found.is_some() || shown {
                shown = found.is_some();
                rep.patch(move |d| d.culprit = found);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, name: &str, ticks: u64, rss: u64) -> Proc {
        Proc { pid, name: name.into(), ticks, rss_pages: rss }
    }

    fn state_of(pid: u32) -> char {
        let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
        s[s.rfind(')').unwrap() + 2..].chars().next().unwrap()
    }

    #[test]
    fn a_program_can_be_frozen_and_woken() {
        let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        signal(&[pid], libc::SIGSTOP);
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(state_of(pid), 'T', "stopped");
        signal(&[pid], libc::SIGCONT);
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(state_of(pid), 'S', "running again");
        signal(&[pid], libc::SIGTERM);
        assert!(!child.wait().unwrap().success());
    }

    #[test]
    fn a_program_using_most_of_a_loaded_machine_is_named() {
        let before = [p(1, "ffmpeg", 0, 1000), p(2, "bash", 0, 10)];
        let now = [p(1, "ffmpeg", 2000, 1000), p(2, "bash", 10, 10)];
        // 5 s: 20 s of CPU = 4 cores, on an 8-core machine at 90%
        let b = blame(&before, &now, 5.0, 0.9, 8.0, 0.5).unwrap();
        assert_eq!((b.name.as_str(), b.pids.clone(), b.memory, b.protected), ("ffmpeg", vec![1], false, false));
        assert!((b.cores - 4.0).abs() < 1e-3);
    }

    #[test]
    fn nobody_is_blamed_for_a_spread_out_load_or_a_quiet_machine() {
        let before: Vec<Proc> = (0..8).map(|i| p(i, "w", 0, 1)).collect();
        let now: Vec<Proc> = (0..8).map(|i| p(i, &format!("w{i}"), 500, 1)).collect();
        assert!(blame(&before, &now, 5.0, 0.9, 8.0, 0.5).is_none(), "eight programs at one core each: nobody stands out");
        assert!(blame(&before, &now, 5.0, 0.3, 8.0, 0.5).is_none(), "machine not loaded");
    }

    #[test]
    fn memory_shortage_blames_the_biggest_holder_and_the_session_is_protected() {
        let now = [p(1, "niri", 0, 900_000), p(2, "chrome", 0, 800_000)];
        let b = blame(&now, &now, 5.0, 0.1, 8.0, 0.03).unwrap();
        assert!(b.memory && b.protected && b.name == "niri");
    }
}
