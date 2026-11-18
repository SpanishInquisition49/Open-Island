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
    fn wraps_a_command_and_keeps_its_exit_code() {
        // no island is running: reporting is a no-op, the command's own result is what counts
        assert_eq!(run(&["--".into(), "true".into()]), 0);
        assert_eq!(run(&["-l".into(), "x".into(), "--".into(), "sh".into(), "-c".into(), "exit 3".into()]), 3);
        assert_eq!(run(&["--".into(), "definitely-not-a-program-xyz".into()]), 127);
        assert_eq!(run(&[]), 2);
    }
}
