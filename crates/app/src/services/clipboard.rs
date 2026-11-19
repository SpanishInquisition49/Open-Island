//! Clipboard history on Wayland (wl-clipboard): text only, newest first, never password-manager entries.
//! Kept in memory only: copied text is not written to disk.

use super::clipboard_history as history;
use super::Reporter;
use std::io::{Read, Write};
use std::process::{Command, Stdio};

/// Entries a password manager marks as secret must never be kept.
pub fn is_secret(types: &str) -> bool {
    types.lines().any(|t| t.trim().eq_ignore_ascii_case("x-kde-passwordManagerHint"))
}

pub fn restore(text: &str) {
    if let Ok(mut c) = Command::new("wl-copy").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        if let Some(mut i) = c.stdin.take() {
            let _ = i.write_all(text.as_bytes());
        }
        std::thread::spawn(move || {
            let _ = c.wait();
        });
    }
}

/// Copies a one-time code and takes it off the clipboard again after thirty seconds, when it is still there.
pub fn copy_code(code: &str) {
    super::clipboard_history::ignore(code);
    restore(code);
    let code = code.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(30));
        let now = Command::new("wl-paste").args(["--no-newline", "--type", "text"]).stderr(Stdio::null()).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        if now == code {
            let _ = Command::new("wl-copy").arg("--clear").status();
        }
    });
}

pub fn start(rep: Reporter) {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return;
    }
    std::thread::spawn(move || {
        // wl-paste runs the command on every change with the text on its stdin; a NUL ends each entry
        let child = Command::new("wl-paste").args(["--type", "text", "--watch", "sh", "-c", "cat; printf '\\0'"]).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
        let Ok(mut child) = child else { return log::info!("wl-paste not found: no clipboard history") };
        let Some(mut out) = child.stdout.take() else { return };
        history::init(&rep);
        let (mut buf, mut chunk) = (Vec::<u8>::new(), [0u8; 4096]);
        while let Ok(n) = out.read(&mut chunk) {
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            while let Some(end) = buf.iter().position(|b| *b == 0) {
                let entry: Vec<u8> = buf.drain(..=end).collect();
                let text = String::from_utf8_lossy(&entry[..entry.len() - 1]).into_owned();
                let types = Command::new("wl-paste").arg("--list-types").stderr(Stdio::null()).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
                if is_secret(&types) {
                    continue;
                }
                history::record(&rep, &text);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_manager_entries_are_secret() {
        assert!(is_secret("text/plain\nx-kde-passwordManagerHint\n"));
        assert!(!is_secret("text/plain\ntext/html\n"));
    }
}
