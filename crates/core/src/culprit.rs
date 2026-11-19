//! Which program is hogging the machine: from two readings of /proc, the processes that used the most CPU in between,
//! grouped by program (a browser is dozens of processes), and the one holding the most memory.

use std::collections::HashMap;

/// One process as /proc/<pid>/stat says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub name: String,
    /// utime + stime, in clock ticks
    pub ticks: u64,
    pub rss_pages: u64,
}

/// `pid (name) S ppid ... utime stime ... rss ...`: the name may hold spaces and brackets, so it is cut at the last `)`.
pub fn parse_stat(line: &str) -> Option<Proc> {
    let open = line.find('(')?;
    let close = line.rfind(')')?;
    let pid = line[..open].trim().parse().ok()?;
    let name = line[open + 1..close].to_string();
    let rest: Vec<&str> = line[close + 1..].split_whitespace().collect();
    // rest[0] is the state (field 3): utime is field 14, stime 15, rss 24
    let ppid: u32 = rest.get(1)?.parse().ok()?;
    if ppid == 2 || pid == 2 {
        return None; // kernel threads
    }
    Some(Proc { pid, name, ticks: rest.get(11)?.parse::<u64>().ok()? + rest.get(12)?.parse::<u64>().ok()?, rss_pages: rest.get(21)?.parse().ok()? })
}

/// Programs the island never offers to stop: the ones the session lives on.
pub fn is_protected(name: &str) -> bool {
    const KEEP: &[&str] = &["systemd", "niri", "noctalia", "Xwayland", "Xorg", "sway", "Hyprland", "kwin_wayland", "gnome-shell", "pipewire", "wireplumber", "dbus-daemon", "open-island", "login", "sddm", "gdm", "greetd", "init"];
    KEEP.iter().any(|k| name == *k || name.starts_with(&format!("{k}-")))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub name: String,
    pub pids: Vec<u32>,
    /// In cores: 1.0 is one core fully busy, 3.4 is three and a bit.
    pub cores: f64,
    pub rss_mb: f64,
}

/// CPU use by program between two readings, busiest first. `tick_secs` is how long the gap was, `hz` the clock ticks
/// per second (100 on Linux).
pub fn by_cpu(before: &[Proc], now: &[Proc], tick_secs: f64, hz: f64, page_kb: f64) -> Vec<Group> {
    let old: HashMap<u32, u64> = before.iter().map(|p| (p.pid, p.ticks)).collect();
    let mut groups: HashMap<&str, Group> = HashMap::new();
    for p in now {
        let used = p.ticks.saturating_sub(old.get(&p.pid).copied().unwrap_or(p.ticks)) as f64 / hz / tick_secs.max(0.001);
        let g = groups.entry(&p.name).or_insert_with(|| Group { name: p.name.clone(), pids: vec![], cores: 0.0, rss_mb: 0.0 });
        g.pids.push(p.pid);
        g.cores += used;
        g.rss_mb += p.rss_pages as f64 * page_kb / 1024.0;
    }
    let mut v: Vec<Group> = groups.into_values().collect();
    v.sort_by(|a, b| b.cores.total_cmp(&a.cores).then(a.name.cmp(&b.name)));
    v
}

/// Memory held by program, biggest first.
pub fn by_memory(now: &[Proc], page_kb: f64) -> Vec<Group> {
    let mut v = by_cpu(now, now, 1.0, 100.0, page_kb);
    v.sort_by(|a, b| b.rss_mb.total_cmp(&a.rss_mb).then(a.name.cmp(&b.name)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, name: &str, ticks: u64, rss: u64) -> Proc {
        Proc { pid, name: name.into(), ticks, rss_pages: rss }
    }

    #[test]
    fn a_stat_line_with_a_tricky_name_is_read() {
        let l = "4242 (Web Content (x)) S 1 4242 4242 0 -1 4194560 1000 0 0 0 150 50 0 0 20 0 5 0 100 1000000 25600 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0";
        let p = parse_stat(l).unwrap();
        assert_eq!((p.pid, p.name.as_str(), p.ticks, p.rss_pages), (4242, "Web Content (x)", 200, 25600));
        assert!(parse_stat("2 (kthreadd) S 0 0 0 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0").is_none(), "kernel thread");
        assert!(parse_stat("garbage").is_none());
    }

    #[test]
    fn a_browser_is_one_program_and_the_busiest_comes_first() {
        let before = [proc(1, "firefox", 1000, 100), proc(2, "firefox", 500, 100), proc(3, "make", 0, 10)];
        let now = [proc(1, "firefox", 1300, 100), proc(2, "firefox", 800, 100), proc(3, "make", 100, 10), proc(4, "new", 50, 1)];
        // 5 seconds: firefox used 600 ticks = 6 s = 1.2 cores; make 1 s = 0.2 cores; a process that just appeared has no history
        let g = by_cpu(&before, &now, 5.0, 100.0, 4.0);
        assert_eq!(g[0].name, "firefox");
        assert!((g[0].cores - 1.2).abs() < 1e-9 && g[0].pids == vec![1, 2]);
        assert!((g[1].cores - 0.2).abs() < 1e-9 && g[1].name == "make");
        assert_eq!(g[2].cores, 0.0);
        assert!((g[0].rss_mb - 200.0 * 4.0 / 1024.0).abs() < 1e-9);
    }

    #[test]
    fn memory_is_by_program_too() {
        let now = [proc(1, "a", 0, 1000), proc(2, "b", 0, 5000), proc(3, "a", 0, 1500)];
        let g = by_memory(&now, 4.0);
        assert_eq!(g[0].name, "b");
        assert_eq!(g[1].pids, vec![1, 3]);
    }

    #[test]
    fn the_session_is_protected() {
        assert!(is_protected("niri") && is_protected("systemd-logind") && is_protected("open-island"));
        assert!(!is_protected("firefox") && !is_protected("nirimaster"));
    }
}
