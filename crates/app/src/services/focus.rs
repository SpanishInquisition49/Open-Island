//! The study mode, if `~/.local/bin/focus` exists: `focus on|off` blocks distracting sites in /etc/hosts. The
//! island only reads its state (the marker in /etc/hosts and the end time in $XDG_RUNTIME_DIR/focus-end, every
//! 5 s) and starts or stops it through the script, which asks for the admin password itself.

use super::Reporter;
use island_ui::model::Focus;
use std::process::{Command, Stdio};
use std::time::Duration;

const MARKER: &str = "# >>> focus (managed, do not edit) >>>";

pub fn script() -> Option<std::path::PathBuf> {
    let p = std::path::PathBuf::from(std::env::var_os("HOME")?).join(".local/bin/focus");
    p.exists().then_some(p)
}

pub fn parse_state(hosts: &str, end_file: Option<&str>) -> Focus {
    Focus { active: hosts.contains(MARKER), end_epoch: end_file.and_then(|s| s.trim().parse().ok()) }
}

fn read_state() -> Focus {
    let hosts = std::fs::read_to_string("/etc/hosts").unwrap_or_default();
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    parse_state(&hosts, std::fs::read_to_string(format!("{dir}/focus-end")).ok().as_deref())
}

pub fn run(script: &std::path::Path, args: &[&str]) {
    let _ = Command::new(script).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

/// Polls the state; returns the script's path so the command router can start and stop it.
pub fn start(rep: Reporter) -> Option<std::path::PathBuf> {
    let script = script()?;
    std::thread::spawn(move || loop {
        let f = read_state();
        rep.patch(move |d| d.focus = Some(f));
        std::thread::sleep(Duration::from_secs(5));
    });
    Some(script)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_state_files() {
        let on = parse_state("127.0.0.1 localhost\n# >>> focus (managed, do not edit) >>>\n0.0.0.0 x.com\n", Some("1800000000\n"));
        assert_eq!(on, Focus { active: true, end_epoch: Some(1_800_000_000) });
        let off = parse_state("127.0.0.1 localhost\n", None);
        assert_eq!(off, Focus { active: false, end_epoch: None });
        assert_eq!(parse_state("", Some("not a number")).end_epoch, None);
    }
}
