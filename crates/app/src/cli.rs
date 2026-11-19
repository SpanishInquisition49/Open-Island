//! `open-island progress ...`: report a long operation to the running island, or wrap a command and report
//! its progress. The command runs on a pseudo-terminal, so rsync, curl, pacman... still draw their own
//! progress, and the last "NN%" they print is what the island shows. Not for commands that ask questions
//! (the input is not forwarded): authenticate first (`sudo -v`).

use crate::services::ipc;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

/// The last "42%" / "12.5 %" in a chunk of terminal output (0..=100), or None.
pub fn last_percent(chunk: &[u8]) -> Option<f64> {
    let mut found = None;
    for (i, b) in chunk.iter().enumerate() {
        if *b != b'%' {
            continue;
        }
        let mut j = i;
        while j > 0 && chunk[j - 1] == b' ' {
            j -= 1;
        }
        let end = j;
        while j > 0 && (chunk[j - 1].is_ascii_digit() || chunk[j - 1] == b'.') {
            j -= 1;
        }
        if j == end {
            continue;
        }
        if let Ok(v) = std::str::from_utf8(&chunk[j..end]).unwrap_or("").parse::<f64>() {
            if (0.0..=100.0).contains(&v) {
                found = Some(v);
            }
        }
    }
    found
}

/// `autostart on|off|status`: start the island with the user's session. Windows: the Run key; macOS: a LaunchAgent;
/// Linux: a systemd user unit.
pub fn autostart(action: Option<&str>) -> i32 {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("cannot find the program's own path: {e}");
            return 1;
        }
    };
    let result = match action {
        Some("on") => autostart_set(&exe, true).map(|()| "will start with your session".to_string()),
        Some("off") => autostart_set(&exe, false).map(|()| "will not start with your session".to_string()),
        Some("status") => Ok(if autostart_enabled() { "starts with your session" } else { "does not start with your session" }.to_string()),
        _ => {
            eprintln!("usage: open-island autostart on|off|status");
            return 2;
        }
    };
    match result {
        Ok(m) => {
            println!("open-island {m}");
            0
        }
        Err(e) => {
            eprintln!("open-island: {e}");
            1
        }
    }
}

#[cfg(target_os = "windows")]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(target_os = "windows")]
fn autostart_set(exe: &std::path::Path, on: bool) -> Result<(), String> {
    use winreg::{enums::*, RegKey};
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY).map_err(|e| e.to_string())?;
    if on {
        key.set_value("open-island", &format!("\"{}\"", exe.display())).map_err(|e| e.to_string())
    } else {
        key.delete_value("open-island").or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) }).map_err(|e| e.to_string())
    }
}

#[cfg(target_os = "windows")]
fn autostart_enabled() -> bool {
    use winreg::{enums::*, RegKey};
    RegKey::predef(HKEY_CURRENT_USER).open_subkey(RUN_KEY).and_then(|k| k.get_value::<String, _>("open-island")).is_ok()
}

#[cfg(target_os = "macos")]
fn plist_path() -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(std::env::var_os("HOME")?).join("Library/LaunchAgents/org.open-island.plist"))
}

#[cfg(target_os = "macos")]
fn autostart_set(exe: &std::path::Path, on: bool) -> Result<(), String> {
    let path = plist_path().ok_or("no home directory")?;
    if on {
        let plist = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>org.open-island</string><key>ProgramArguments</key><array><string>{}</string></array><key>RunAtLoad</key><true/><key>KeepAlive</key><true/></dict></plist>\n", exe.display());
        std::fs::create_dir_all(path.parent().ok_or("no directory")?).map_err(|e| e.to_string())?;
        std::fs::write(&path, plist).map_err(|e| e.to_string())
    } else {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

#[cfg(target_os = "macos")]
fn autostart_enabled() -> bool {
    plist_path().is_some_and(|p| p.exists())
}

#[cfg(target_os = "linux")]
fn unit_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(base.join("systemd/user/open-island.service"))
}

#[cfg(target_os = "linux")]
fn autostart_set(exe: &std::path::Path, on: bool) -> Result<(), String> {
    let path = unit_path().ok_or("no configuration directory")?;
    let systemctl = |args: &[&str]| std::process::Command::new("systemctl").arg("--user").args(args).status().map(|s| s.success()).unwrap_or(false);
    if on {
        let unit = format!("[Unit]\nDescription=open-island, a dynamic island for the desktop\nPartOf=graphical-session.target\nAfter=graphical-session.target\nConditionEnvironment=WAYLAND_DISPLAY\n\n[Service]\nExecStart={}\nRestart=on-failure\nRestartSec=2\n\n[Install]\nWantedBy=graphical-session.target\n", exe.display());
        std::fs::create_dir_all(path.parent().ok_or("no directory")?).map_err(|e| e.to_string())?;
        std::fs::write(&path, unit).map_err(|e| e.to_string())?;
        // enabling is enough: it starts with the next session (a running island is not started twice)
        if systemctl(&["enable", "open-island.service"]) { Ok(()) } else { Err("the unit is written but `systemctl --user enable open-island` failed".into()) }
    } else {
        let _ = systemctl(&["disable", "open-island.service"]);
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

#[cfg(target_os = "linux")]
fn autostart_enabled() -> bool {
    unit_path().is_some_and(|p| p.exists())
}

fn send(event: &str, payload: &str) {
    let _ = ipc::send(&if payload.is_empty() { event.to_string() } else { format!("{event} {payload}") });
}

/// `progress <set|done|fail|clear|run> ...`; returns the process exit code.
pub fn progress(args: &[String]) -> i32 {
    let rest = |from: usize| args.get(from..).unwrap_or(&[]).join(" ");
    match args.first().map(String::as_str) {
        Some("set") => {
            send("progress", &rest(1));
            0
        }
        Some(e @ ("done" | "fail" | "clear")) => {
            send(&format!("progress-{e}"), &rest(1));
            0
        }
        Some("run") => run(&args[1..]),
        _ => {
            eprintln!("usage: open-island progress set <percent|?> [label] | done [label] | fail [label] | clear | run [-l label] -- command...");
            2
        }
    }
}

/// `activity run|done|fail|clear [label] [--detail text] [--icon name] [--color name|#hex] [--progress N]`: the
/// status of something a script watches (a build, a deploy, a CI run), with its own icon, colour and status line.
/// The control-socket line for `open-island send`: the files by absolute path, separated by tabs.
pub fn send_line(args: &[String]) -> Result<String, String> {
    if args.is_empty() {
        return Err("usage: open-island send <file>...".into());
    }
    let mut paths = vec![];
    for a in args {
        let p = std::fs::canonicalize(a).map_err(|e| format!("{a}: {e}"))?;
        if !p.is_file() {
            return Err(format!("{a}: not a file"));
        }
        let s = p.to_string_lossy().into_owned();
        if s.contains(['\t', '\n']) {
            return Err(format!("{a}: a tab or a new line in the name"));
        }
        paths.push(s);
    }
    Ok(format!("send {}", paths.join("\t")))
}

/// `open-island send <file>...`: the island lists the devices on the network (LocalSend) and sends the files to the one you pick.
pub fn send_files(args: &[String]) -> i32 {
    match send_line(args) {
        Ok(line) => {
            if !ipc::send(&line) {
                eprintln!("open-island is not running");
                return 1;
            }
            0
        }
        Err(e) => {
            eprintln!("open-island send: {e}");
            2
        }
    }
}

pub fn activity(args: &[String]) -> i32 {
    let Some(state) = args.first().map(String::as_str).filter(|s| matches!(*s, "run" | "done" | "fail" | "clear")) else {
        eprintln!("usage: open-island activity run|done|fail|clear [label] [--detail text] [--icon name] [--color red|orange|green|blue|#rrggbb] [--progress 0-100]");
        return 2;
    };
    match activity_line(state, &args[1..]) {
        Ok(line) => {
            if !ipc::send(&line) {
                eprintln!("open-island is not running");
                return 1;
            }
            0
        }
        Err(e) => {
            eprintln!("open-island activity: {e}");
            2
        }
    }
}

fn activity_line(state: &str, args: &[String]) -> Result<String, String> {
    let mut v = serde_json::Map::new();
    let mut label = vec![];
    let mut i = 0;
    while i < args.len() {
        let key = match args[i].as_str() {
            "--detail" | "--text" => Some("detail"),
            "--icon" => Some("icon"),
            "--color" | "--colour" => Some("color"),
            "--progress" => Some("progress"),
            "--label" => Some("label"),
            f if f.starts_with("--") => return Err(format!("unknown option {f}")),
            _ => None,
        };
        match key {
            Some(k) => {
                let val = args.get(i + 1).ok_or_else(|| format!("{} needs a value", args[i]))?;
                if k == "progress" {
                    let n: f64 = val.trim_end_matches('%').parse().map_err(|_| format!("progress is a number 0-100, not {val}"))?;
                    v.insert(k.into(), serde_json::json!(n));
                } else {
                    v.insert(k.into(), serde_json::json!(val));
                }
                i += 2;
            }
            None => {
                label.push(args[i].clone());
                i += 1;
            }
        }
    }
    if !label.is_empty() {
        v.insert("label".into(), serde_json::json!(label.join(" ")));
    }
    Ok(format!("activity {state} {}", serde_json::Value::Object(v)))
}

fn run(args: &[String]) -> i32 {
    let mut label = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-l" | "--label" if i + 1 < args.len() => {
                label = Some(args[i + 1].clone());
                i += 2;
            }
            "--" => {
                i += 1;
                break;
            }
            _ => break,
        }
    }
    let cmd = &args[i..];
    let Some(prog) = cmd.first() else {
        eprintln!("open-island progress run: no command given");
        return 2;
    };
    let label = label.unwrap_or_else(|| std::path::Path::new(prog).file_name().map_or(prog.clone(), |n| n.to_string_lossy().into_owned()));
    let size = PtySize { rows: std::env::var("LINES").ok().and_then(|v| v.parse().ok()).unwrap_or(30), cols: std::env::var("COLUMNS").ok().and_then(|v| v.parse().ok()).unwrap_or(120), pixel_width: 0, pixel_height: 0 };
    let pair = match native_pty_system().openpty(size) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("open-island progress run: no pseudo-terminal: {e}");
            return 1;
        }
    };
    let mut builder = CommandBuilder::new(prog);
    builder.args(&cmd[1..]);
    if let Ok(dir) = std::env::current_dir() {
        builder.cwd(dir);
    }
    let mut child = match pair.slave.spawn_command(builder) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("open-island progress run: cannot start {prog}: {e}");
            return 127;
        }
    };
    drop(pair.slave);
    let Ok(mut reader) = pair.master.try_clone_reader() else { return 1 };
    send("progress", &format!("? {label}"));
    let (mut pct, mut sent_pct, mut sent_at) = (None::<f64>, None::<f64>, Instant::now());
    let (mut out, mut buf) = (std::io::stdout(), [0u8; 4096]);
    while let Ok(n) = reader.read(&mut buf) {
        if n == 0 {
            break;
        }
        let _ = out.write_all(&buf[..n]);
        let _ = out.flush();
        if let Some(p) = last_percent(&buf[..n]) {
            pct = Some(p);
        }
        if pct != sent_pct && sent_at.elapsed() >= Duration::from_millis(800) {
            sent_pct = pct;
            sent_at = Instant::now();
            send("progress", &format!("{} {label}", pct.map_or("?".to_string(), |p| format!("{p:.0}"))));
        }
    }
    let code = child.wait().map(|s| s.exit_code() as i32).unwrap_or(1);
    if code == 0 {
        send("progress-done", &label);
    } else {
        send("progress-fail", &format!("{label} failed"));
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_names_the_files_by_absolute_path_and_refuses_what_is_not_a_file() {
        let dir = std::env::temp_dir().join(format!("oi-cli-send-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.txt");
        std::fs::write(&f, "x").unwrap();
        let line = send_line(&[f.to_string_lossy().into_owned()]).unwrap();
        assert_eq!(line, format!("send {}", std::fs::canonicalize(&f).unwrap().to_string_lossy()));
        assert!(send_line(&[]).is_err());
        assert!(send_line(&[dir.to_string_lossy().into_owned()]).unwrap_err().contains("not a file"));
        assert!(send_line(&["/definitely/not/here".into()]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_the_last_percentage_in_terminal_output() {
        assert_eq!(last_percent(b"copying  42%"), Some(42.0));
        assert_eq!(last_percent(b"\r 10%\r 55 %\r 99.5%"), Some(99.5), "the latest wins");
        assert_eq!(last_percent(b"1.2GB  37%  3.4MB/s 0:01:00"), Some(37.0), "rsync and curl lines");
        assert_eq!(last_percent(b"(12/40) installing foo ... [###   ]"), None);
        assert_eq!(last_percent(b"100% 250%"), Some(100.0), "250% is not a progress");
        assert_eq!(last_percent(b"% 50"), None);
        assert_eq!(last_percent(b""), None);
    }

    #[test]
    fn builds_the_activity_message() {
        let a = |s: &[&str]| activity_line("run", &s.iter().map(|x| x.to_string()).collect::<Vec<_>>());
        assert_eq!(a(&["CI", "main", "--detail", "building", "--icon", "git-branch", "--progress", "40%"]).unwrap(), r#"activity run {"detail":"building","icon":"git-branch","label":"CI main","progress":40.0}"#);
        assert_eq!(a(&[]).unwrap(), "activity run {}");
        assert!(a(&["--bogus"]).is_err() && a(&["--icon"]).is_err() && a(&["--progress", "lots"]).is_err());
    }

    #[test]
    fn wraps_a_command_and_keeps_its_exit_code() {
        // no island is running: reporting is a no-op, the command's own result is what counts
        assert_eq!(run(&["--".into(), "true".into()]), 0);
        assert_eq!(run(&["-l".into(), "x".into(), "--".into(), "sh".into(), "-c".into(), "exit 3".into()]), 3);
        assert_eq!(run(&["--".into(), "definitely-not-a-program-xyz".into()]), 127);
        assert_eq!(run(&[]), 2);
    }
}
