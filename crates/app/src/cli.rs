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
pub fn shelf(args: &[String]) -> i32 {
    match send_line(args) {
        Ok(line) if crate::services::ipc::send(&format!("shelf{}", line.trim_start_matches("send"))) => 0,
        Ok(_) => {
            eprintln!("open-island is not running");
            1
        }
        Err(e) => {
            eprintln!("open-island: {}", e.replace("open-island send", "open-island shelf"));
            2
        }
    }
}

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
            "--id" => Some("id"),
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
    let spawn = |program: &str, args: &[String]| {
        let mut builder = CommandBuilder::new(program);
        builder.args(args);
        if let Ok(dir) = std::env::current_dir() {
            builder.cwd(dir);
        }
        pair.slave.spawn_command(builder)
    };
    let mut child = match spawn(prog, &cmd[1..]) {
        Ok(c) => c,
        Err(first) => {
            // Windows: batch files (npm, yarn...) and shell built-ins (dir, echo) are not programs: cmd runs them
            let fallback = cfg!(windows).then(|| spawn("cmd.exe", &[["/C".to_string()].as_slice(), cmd].concat()).ok()).flatten();
            match fallback {
                Some(c) => c,
                None => {
                    eprintln!("open-island progress run: cannot start {prog}: {first}");
                    return 127;
                }
            }
        }
    };
    drop(pair.slave);
    let Ok(mut reader) = pair.master.try_clone_reader() else { return 1 };
    send("progress", &format!("? {label}"));
    // The output is read on a thread: on Windows the pseudo-console keeps the pipe open after the command has ended,
    // so a blocking read would never see the end.
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let (mut pct, mut sent_pct, mut sent_at) = (None::<f64>, None::<f64>, Instant::now());
    let mut out = std::io::stdout();
    let mut ended: Option<Instant> = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(chunk) => {
                let _ = out.write_all(&chunk);
                let _ = out.flush();
                if let Some(p) = last_percent(&chunk) {
                    pct = Some(p);
                }
                if pct != sent_pct && sent_at.elapsed() >= Duration::from_millis(800) {
                    sent_pct = pct;
                    sent_at = Instant::now();
                    send("progress", &format!("{} {label}", pct.map_or("?".to_string(), |p| format!("{p:.0}"))));
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // the command is over and nothing more came for a moment: what is left is the pipe being held open
                if matches!(child.try_wait(), Ok(Some(_))) && ended.get_or_insert_with(Instant::now).elapsed() > Duration::from_millis(300) {
                    break;
                }
            }
        }
    }
    drop(pair.master);
    let code = child.wait().map(|s| s.exit_code() as i32).unwrap_or(1);
    if code == 0 {
        send("progress-done", &label);
    } else {
        send("progress-fail", &format!("{label} failed"));
    }
    code
}

const CARD_USAGE: &str = "usage: open-island ok|warn|fail|info <title> [options]   |   open-island notify <title> [body] [options]
  -d, --detail <text>      the grey text under the title        -s, --seconds <n>   0.5 to 60
  --stay                   until clicked (10 minutes at most)   --id <name>         a newer card of this name takes its place
  --open <url|file>        adds a button that opens it          --button <label>    that button's label
  --yes <label> [--no <label>]   asks, and waits: exit 0 for yes, 1 for no, 5 when nobody answers
  --icon <name>   --color <name|#rrggbb>   --sound <file>   --preset <name> (card_presets in the settings)";

/// The JSON of a `card` message from the arguments of `open-island <verb> ...`. `piped` is what came on standard input,
/// whose first line is the title when no argument gives one.
pub fn card_spec(verb: &str, args: &[String], piped: Option<&str>) -> Result<serde_json::Value, String> {
    let mut v = serde_json::Map::new();
    if verb != "notify" {
        v.insert("kind".into(), serde_json::json!(verb));
    }
    let mut words = vec![];
    let mut args = args.iter();
    while let Some(a) = args.next() {
        let key = match a.as_str() {
            "--" => {
                words.extend(args.by_ref().cloned());
                break;
            }
            "--stay" => {
                v.insert("stay".into(), serde_json::json!(true));
                continue;
            }
            "-d" | "--detail" => "detail",
            "-s" | "--seconds" => "seconds",
            "--id" => "id",
            "--open" => "open",
            "--button" => "button",
            "--yes" => "yes",
            "--no" => "no",
            "--icon" => "icon",
            "--color" | "--colour" => "color",
            "--sound" => "sound",
            "--preset" => "preset",
            f if f.starts_with('-') && f.len() > 1 => return Err(format!("unknown option {f}")),
            _ => {
                words.push(a.clone());
                continue;
            }
        };
        let value = args.next().ok_or_else(|| format!("{a} needs a value"))?;
        let value = match key {
            "seconds" => {
                let n: f64 = value.parse().ok().filter(|n| (0.5..=60.0).contains(n)).ok_or_else(|| format!("{a} is a number from 0.5 to 60, not {value}"))?;
                serde_json::json!(n)
            }
            // the island runs somewhere else: a file is named from here
            "open" | "sound" => serde_json::json!(std::fs::canonicalize(value).map_or_else(|_| value.clone(), |p| p.to_string_lossy().into_owned())),
            "color" if island_core::progress::parse_color(value).is_none() => return Err(format!("{value} is not a colour (a name or #rrggbb)")),
            "icon" if !island_ui::is_known_icon(value) => return Err(format!("the island has no icon named {value}")),
            _ => serde_json::json!(value),
        };
        v.insert(key.into(), value);
    }
    if words.len() > if verb == "notify" { usize::MAX } else { 1 } {
        return Err(format!("one title, in quotes (the text under it goes in -d): {}", words.join(" ")));
    }
    let mut words = words.into_iter();
    if let Some(title) = words.next() {
        v.insert("title".into(), serde_json::json!(title));
        let body: Vec<String> = words.collect();
        if !body.is_empty() && !v.contains_key("detail") {
            v.insert("detail".into(), serde_json::json!(body.join(" ")));
        }
    } else if let Some(text) = piped {
        let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
        if let Some(title) = lines.next() {
            v.insert("title".into(), serde_json::json!(title));
            let rest = lines.collect::<Vec<_>>().join(" ");
            if !rest.is_empty() && !v.contains_key("detail") {
                v.insert("detail".into(), serde_json::json!(rest));
            }
        }
    }
    if !v.contains_key("title") {
        return Err("a card needs a title".into());
    }
    if v.contains_key("no") && !v.contains_key("yes") {
        return Err("--no goes with --yes".into());
    }
    if v.contains_key("yes") && v.contains_key("open") {
        return Err("a question (--yes) has no --open button".into());
    }
    Ok(serde_json::Value::Object(v))
}

/// The exit status for what the island answered to a question.
fn answer_status(answer: &str) -> i32 {
    match answer {
        "yes" => 0,
        "no" => 1,
        _ => 5,
    }
}

/// `open-island ok|warn|fail|info|notify ...`: a card from a script. Only a clear "yes" exits 0 for a question, so
/// that `... --yes Merge && gh pr merge` never goes ahead by mistake.
pub fn card(verb: &str, args: &[String]) -> i32 {
    use std::io::{IsTerminal, Read};
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{CARD_USAGE}");
        return 0;
    }
    let mut spec = card_spec(verb, args, None);
    // no title among the arguments: it may come from a pipe
    if spec.as_ref().is_err_and(|e| e.starts_with("a card needs")) && !std::io::stdin().is_terminal() {
        let mut text = String::new();
        let _ = std::io::stdin().take(4096).read_to_string(&mut text);
        spec = card_spec(verb, args, Some(&text));
    }
    let spec = match spec {
        Ok(s) => s,
        Err(e) => {
            eprintln!("open-island: {e}\n{CARD_USAGE}");
            return 2;
        }
    };
    let line = format!("card {spec}");
    let status = if spec.get("yes").is_some() { ipc::ask(&line).map(|a| answer_status(&a)) } else { ipc::send(&line).then_some(0) };
    status.unwrap_or_else(|| {
        eprintln!("open-island is not running");
        3
    })
}

/// A command's running time as a card says it: "3.1s", "3m 12s", "1h 05m".
fn took(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..=9 => format!("{:.1}s", d.as_secs_f64()),
        10..=59 => format!("{s}s"),
        60..=3599 => format!("{}m {:02}s", s / 60, s % 60),
        _ => format!("{}h {:02}m", s / 3600, s % 3600 / 60),
    }
}

/// The card for a command that ended with `code`.
fn ran_spec(command: &[String], code: i32, time: Duration) -> serde_json::Value {
    let mut line = command.join(" ");
    // only the command is cut, so that the outcome after it always shows
    if line.chars().count() > 80 {
        line = line.chars().take(79).collect::<String>() + "…";
    }
    let (kind, title) = if code == 0 { ("ok", format!("{line} finished")) } else { ("fail", format!("{line} failed (exit {code})")) };
    serde_json::json!({"kind": kind, "title": title, "detail": took(time)})
}

/// `open-island run [--] <command>...`: runs the command as it is (same terminal, same input), then a card says how it
/// went and how long it took. Exits with the command's own status.
pub fn run_and_tell(args: &[String]) -> i32 {
    let command = if args.first().is_some_and(|a| a == "--") { &args[1..] } else { args };
    let Some(program) = command.first().filter(|p| !matches!(p.as_str(), "-h" | "--help")) else {
        eprintln!("usage: open-island run [--] <command> [args]...");
        return 2;
    };
    let started = Instant::now();
    let code = match std::process::Command::new(program).args(&command[1..]).status() {
        Ok(status) => {
            #[cfg(unix)]
            let signal = std::os::unix::process::ExitStatusExt::signal(&status).map(|s| 128 + s);
            #[cfg(not(unix))]
            let signal = None;
            status.code().or(signal).unwrap_or(1)
        }
        Err(e) => {
            eprintln!("open-island run: cannot start {program}: {e}");
            127
        }
    };
    let _ = ipc::send(&format!("card {}", ran_spec(command, code, started.elapsed())));
    code
}

/// `open-island dismiss [--id <name>]`: takes the first card away, or the one of that name.
pub fn dismiss(args: &[String]) -> i32 {
    let line = match args {
        [] => "dismiss".to_string(),
        [flag, name] if flag == "--id" => format!("dismiss {name}"),
        _ => {
            eprintln!("usage: open-island dismiss [--id <name>]");
            return 2;
        }
    };
    if !ipc::send(&line) {
        eprintln!("open-island is not running");
        return 3;
    }
    0
}

/// `open-island pin <text>` (or the text on stdin: `echo 10.0.0.5 | open-island pin`): a note on the Today page, a click there copies it.
pub fn pin(args: &[String]) -> i32 {
    use std::io::{IsTerminal, Read};
    let mut text = args.join(" ");
    if text.trim().is_empty() && !std::io::stdin().is_terminal() {
        let _ = std::io::stdin().take(4096).read_to_string(&mut text);
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        eprintln!("usage: open-island pin <text>   (or pipe the text in)   ·   open-island unpin [text|all]");
        return 2;
    }
    if !ipc::send(&format!("pin {text}")) {
        eprintln!("open-island is not running");
        return 1;
    }
    0
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

    fn spec(verb: &str, a: &[&str]) -> Result<serde_json::Value, String> {
        card_spec(verb, &a.iter().map(|x| x.to_string()).collect::<Vec<_>>(), None)
    }

    #[test]
    fn a_cards_arguments_become_its_message() {
        assert_eq!(spec("ok", &["Build finished", "-d", "246 passed", "-s", "8"]).unwrap(), serde_json::json!({"kind": "ok", "title": "Build finished", "detail": "246 passed", "seconds": 8.0}));
        assert_eq!(spec("notify", &["Build done", "all", "passed"]).unwrap(), serde_json::json!({"title": "Build done", "detail": "all passed"}), "the older form still works");
        let q = spec("warn", &["Merge?", "--yes", "Merge", "--no", "Later", "--id", "pr"]).unwrap();
        assert_eq!((q["yes"].as_str(), q["no"].as_str(), q["id"].as_str()), (Some("Merge"), Some("Later"), Some("pr")));
        assert_eq!(spec("info", &["--stay", "--", "-v is a title"]).unwrap()["title"], "-v is a title");
        assert!(spec("ok", &["Build", "finished"]).unwrap_err().contains("quotes"));
        for bad in [&["t", "-s", "90"][..], &["t", "--bogus"], &["t", "--id"], &["t", "--no", "n"], &["t", "--yes", "y", "--open", "u"], &["t", "--color", "mauve"], &["t", "--icon", "no-such"], &[]] {
            assert!(spec("ok", bad).is_err(), "{bad:?}");
        }
        let piped = card_spec("fail", &[], Some("\nDeploy failed\nstaging: timed out\nafter 30s\n")).unwrap();
        assert_eq!((piped["title"].as_str(), piped["detail"].as_str()), (Some("Deploy failed"), Some("staging: timed out after 30s")));
    }

    #[test]
    fn only_a_yes_is_a_success() {
        assert_eq!([answer_status("yes"), answer_status("no"), answer_status("none"), answer_status("")], [0, 1, 5, 5]);
    }

    #[test]
    fn a_command_ends_in_a_card() {
        let words = |s: &str| s.split(' ').map(str::to_string).collect::<Vec<_>>();
        assert_eq!(ran_spec(&words("make test"), 0, Duration::from_millis(3140)), serde_json::json!({"kind": "ok", "title": "make test finished", "detail": "3.1s"}));
        let f = ran_spec(&words(&"x".repeat(200)), 2, Duration::from_secs(192));
        assert!(f["title"].as_str().unwrap().ends_with("… failed (exit 2)"));
        assert_eq!((f["kind"].as_str(), f["detail"].as_str()), (Some("fail"), Some("3m 12s")));
        assert_eq!(took(Duration::from_secs(3900)), "1h 05m");
        assert_eq!(run_and_tell(&words("-- sh -c false")), 1);
        assert_eq!(run_and_tell(&words("definitely-not-a-program-xyz")), 127);
        assert_eq!(run_and_tell(&[]), 2);
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
