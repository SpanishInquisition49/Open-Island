//! `open-island watch <pid|program> [label]`: tells you when a process that is already running ends, without having
//! started it through the shell hook (a build in another terminal, a download, a render).
//! While it runs it shows as a pill like a long shell command; when it is gone, a card with how long it took.
//! The exit code of a process that is not our child cannot be known, so it never says "failed".

use super::Reporter;
use island_core::culprit::Proc;
use island_ui::model::{Incoming, Input};
use std::time::{Duration, Instant};

/// What to watch: one process by number, or every process of a program by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Pid(u32),
    Name(String),
}

/// `1234`, `1234 render`, `firefox`, `"my program" label` is not special: the first word is the target, the rest the label.
pub fn parse(spec: &str) -> Option<(Target, String)> {
    let spec = spec.trim();
    let (first, label) = spec.split_once(char::is_whitespace).map_or((spec, ""), |(a, b)| (a, b.trim()));
    if first.is_empty() {
        return None;
    }
    let target = match first.parse::<u32>() {
        Ok(pid) if pid > 0 => Target::Pid(pid),
        _ => Target::Name(first.strip_suffix(".exe").unwrap_or(first).to_string()),
    };
    Some((target, label.to_string()))
}

/// The pids of `procs` the target stands for. Linux cuts program names at 15 letters, so a long name matches on its start.
pub fn matching(target: &Target, procs: &[Proc]) -> Vec<(u32, String)> {
    procs
        .iter()
        .filter(|p| match target {
            Target::Pid(n) => p.pid == *n,
            Target::Name(n) => {
                let (n, name) = (n.to_lowercase(), p.name.to_lowercase());
                name == n || (name.chars().count() >= 15 && n.starts_with(&name))
            }
        })
        .map(|p| (p.pid, p.name.clone()))
        .collect()
}

/// Ids of these cards, apart from the ones scripts and the system use.
fn card_id() -> u32 {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(3_000_000);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

pub fn start(rep: &Reporter, spec: &str) {
    let Some((target, label)) = parse(spec) else { return };
    let found = matching(&target, &super::culprit::processes());
    let say = |summary: String, body: String| {
        rep.input(Input::Notification(Incoming { id: card_id(), app: "open-island".into(), summary, body, expire_ms: Some(5000), ..Default::default() }));
    };
    if found.is_empty() {
        let what = match &target {
            Target::Pid(n) => format!("no process {n}"),
            Target::Name(n) => format!("{n} is not running"),
        };
        say("Nothing to watch".into(), what);
        return;
    }
    let name = found[0].1.clone();
    let label = if label.is_empty() { name } else { label };
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    let id = format!("watch-{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let pids: Vec<u32> = found.iter().map(|f| f.0).collect();
    rep.input(Input::CommandStart { id: id.clone(), label: label.clone(), after_secs: 0 });
    let rep = rep.clone();
    std::thread::spawn(move || {
        let began = Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let alive = super::culprit::processes().iter().any(|p| pids.contains(&p.pid));
            if !alive {
                break;
            }
        }
        rep.input(Input::CommandGone(id));
        let took = island_core::duration::fmt_took(began.elapsed().as_millis() as i64);
        rep.input(Input::Notification(Incoming { id: card_id(), app: "open-island".into(), summary: format!("{label} is done"), body: format!("ended after {took}"), expire_ms: Some(10_000), ..Default::default() }));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, name: &str) -> Proc {
        Proc { pid, name: name.into(), ticks: 0, rss_pages: 0 }
    }

    #[test]
    fn a_number_is_a_process_and_a_word_is_a_program() {
        assert_eq!(parse("1234"), Some((Target::Pid(1234), String::new())));
        assert_eq!(parse("1234 render of the thesis"), Some((Target::Pid(1234), "render of the thesis".into())));
        assert_eq!(parse("firefox"), Some((Target::Name("firefox".into()), String::new())));
        assert_eq!(parse("ffmpeg.exe encode"), Some((Target::Name("ffmpeg".into()), "encode".into())));
        assert_eq!(parse("0"), Some((Target::Name("0".into()), String::new())), "pid 0 is not a process");
        assert_eq!(parse("  "), None);
    }

    #[test]
    fn a_program_is_all_its_processes_and_long_names_match_their_cut_form() {
        let procs = [proc(1, "ffmpeg"), proc(2, "FFmpeg"), proc(3, "make"), proc(4, "chrome_crashpad_")];
        assert_eq!(matching(&Target::Name("ffmpeg".into()), &procs).len(), 2);
        assert_eq!(matching(&Target::Pid(3), &procs), vec![(3, "make".to_string())]);
        assert!(matching(&Target::Pid(99), &procs).is_empty());
        assert_eq!(matching(&Target::Name("chrome_crashpad_handler".into()), &procs).len(), 1, "comm is cut at 15 letters");
        assert!(matching(&Target::Name("mak".into()), &procs).is_empty(), "a part of a short name is not a match");
    }
}
