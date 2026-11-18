//! Parsers for the system page: CPU load per core, memory, network rates, load average (Linux /proc formats).
//! Pure functions over the file contents, so they are unit-tested without a system.

/// Cumulative CPU time of one core (or the whole CPU): (busy jiffies, total jiffies).
pub type CpuTimes = (u64, u64);

/// "cpu  10 0 20 100 ..." lines -> the total first, then one entry per core.
pub fn parse_proc_stat(text: &str) -> Vec<CpuTimes> {
    text.lines()
        .filter(|l| l.starts_with("cpu"))
        .filter_map(|l| {
            let v: Vec<u64> = l.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
            if v.len() < 4 {
                return None;
            }
            let idle = v[3] + v.get(4).copied().unwrap_or(0); // idle + iowait
            let total: u64 = v.iter().take(8).sum();
            Some((total.saturating_sub(idle), total))
        })
        .collect()
}

/// Usage in percent between two samples of the same CPU (0 when nothing elapsed).
pub fn usage_percent(prev: CpuTimes, now: CpuTimes) -> f32 {
    let (db, dt) = (now.0.saturating_sub(prev.0), now.1.saturating_sub(prev.1));
    if dt == 0 {
        0.0
    } else {
        (db as f32 / dt as f32 * 100.0).clamp(0.0, 100.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Memory {
    pub total_mb: f64,
    pub used_mb: f64,
    pub swap_used_mb: f64,
}

/// /proc/meminfo: used = total - available.
pub fn parse_meminfo(text: &str) -> Option<Memory> {
    let kb = |key: &str| -> Option<f64> {
        text.lines().find_map(|l| l.strip_prefix(key)?.trim_start_matches(':').split_whitespace().next()?.parse::<f64>().ok())
    };
    let (total, avail) = (kb("MemTotal")?, kb("MemAvailable")?);
    let swap = kb("SwapTotal").unwrap_or(0.0) - kb("SwapFree").unwrap_or(0.0);
    Some(Memory { total_mb: total / 1024.0, used_mb: (total - avail).max(0.0) / 1024.0, swap_used_mb: swap.max(0.0) / 1024.0 })
}

/// /proc/net/dev: total (received, sent) bytes over the real interfaces (not lo, veth, docker, bridges).
pub fn parse_net_dev(text: &str) -> (u64, u64) {
    let (mut rx, mut tx) = (0u64, 0u64);
    for l in text.lines().skip(2) {
        let Some((name, rest)) = l.split_once(':') else { continue };
        let name = name.trim();
        if name == "lo" || ["veth", "docker", "br-", "virbr", "tun", "tap"].iter().any(|p| name.starts_with(p)) {
            continue;
        }
        let v: Vec<u64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        if v.len() >= 9 {
            rx += v[0];
            tx += v[8];
        }
    }
    (rx, tx)
}

pub fn parse_loadavg(text: &str) -> Option<[f32; 3]> {
    let mut it = text.split_whitespace().take(3).map(|x| x.parse::<f32>().ok());
    Some([it.next()??, it.next()??, it.next()??])
}

/// `nvidia-smi --query-gpu=pstate,power.draw,temperature.gpu,utilization.gpu --format=csv,noheader,nounits`
pub fn parse_nvidia_smi(text: &str) -> Option<(String, f32, f32, f32)> {
    let f: Vec<&str> = text.lines().next()?.split(',').map(str::trim).collect();
    if f.len() < 4 || !f[0].starts_with('P') {
        return None;
    }
    Some((f[0].to_string(), f[1].parse().ok()?, f[2].parse().ok()?, f[3].parse().ok()?))
}

/// `df -Pk /` -> (used MB, total MB).
pub fn parse_df(text: &str) -> Option<(f64, f64)> {
    let f: Vec<&str> = text.lines().nth(1)?.split_whitespace().collect();
    let (total, used) = (f.get(1)?.parse::<f64>().ok()?, f.get(2)?.parse::<f64>().ok()?);
    Some((used / 1024.0, total / 1024.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &str = "cpu  100 0 100 700 100 0 0 0 0 0\ncpu0 50 0 50 300 0 0 0 0 0 0\ncpu1 50 0 50 400 100 0 0 0 0 0\nintr 1\n";

    #[test]
    fn reads_cpu_times_and_usage() {
        let t = parse_proc_stat(STAT);
        assert_eq!(t.len(), 3);
        assert_eq!(t[0], (200, 1000), "busy = total - idle - iowait");
        let later = [(300, 1200), (150, 500), (150, 700)];
        assert_eq!(usage_percent(t[0], later[0]), 50.0);
        assert_eq!(usage_percent(t[1], later[1]).round(), 50.0);
        assert_eq!(usage_percent(t[0], t[0]), 0.0, "nothing elapsed");
        assert_eq!(usage_percent((500, 1000), (400, 1100)), 0.0, "a counter going backwards");
        assert!(parse_proc_stat("garbage").is_empty());
    }

    #[test]
    fn reads_memory() {
        let m = parse_meminfo("MemTotal:       16384000 kB\nMemFree: 1000 kB\nMemAvailable:    8192000 kB\nSwapTotal: 2048000 kB\nSwapFree: 1024000 kB\n").unwrap();
        assert_eq!(m.total_mb, 16000.0);
        assert_eq!(m.used_mb, 8000.0);
        assert_eq!(m.swap_used_mb, 1000.0);
        assert!(parse_meminfo("MemTotal: 1 kB\n").is_none(), "no MemAvailable");
    }

    #[test]
    fn sums_real_interfaces_only() {
        let text = "Inter-|   Receive |  Transmit\n face |bytes packets errs drop fifo frame compressed multicast|bytes packets\n    lo: 999 1 0 0 0 0 0 0 999 1 0 0 0 0 0 0\n wlan0: 1000 5 0 0 0 0 0 0 200 3 0 0 0 0 0 0\nveth1: 5000 1 0 0 0 0 0 0 5000 1 0 0 0 0 0 0\n enp3s0: 10 1 0 0 0 0 0 0 20 1 0 0 0 0 0 0\n";
        assert_eq!(parse_net_dev(text), (1010, 220));
    }

    #[test]
    fn reads_the_small_files() {
        assert_eq!(parse_loadavg("0.52 0.40 0.31 1/500 1234\n"), Some([0.52, 0.40, 0.31]));
        assert_eq!(parse_loadavg("x"), None);
        assert_eq!(parse_nvidia_smi("P8, 4.25, 41, 3\n"), Some(("P8".into(), 4.25, 41.0, 3.0)));
        assert_eq!(parse_nvidia_smi("No devices were found"), None);
        assert_eq!(parse_df("Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/nvme0n1p2 478150000 311296000 166854000 66% /\n"), Some((304000.0, 466943.359375)));
    }
}
