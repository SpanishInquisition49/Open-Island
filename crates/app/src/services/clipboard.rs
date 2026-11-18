//! Clipboard history on Wayland (wl-clipboard): text only, newest first, never password-manager entries.
//! Kept in memory only: copied text is not written to disk.

use super::Reporter;
use std::io::{Read, Write};
use std::process::{Command, Stdio};

const MAX: usize = 25;
const MAX_CHARS: usize = 4000;

/// Adds an entry to the history: newest first, no duplicates, no blank text, capped.
pub fn push(history: &mut Vec<String>, text: &str) -> bool {
    let text: String = text.chars().take(MAX_CHARS).collect();
    if text.trim().is_empty() {
        return false;
    }
    history.retain(|t| *t != text);
    history.insert(0, text);
    history.truncate(MAX);
    true
}

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

pub fn start(rep: Reporter) {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return;
    }
    std::thread::spawn(move || {
        // wl-paste runs the command on every change with the text on its stdin; a NUL ends each entry
        let child = Command::new("wl-paste").args(["--type", "text", "--watch", "sh", "-c", "cat; printf '\\0'"]).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
        let Ok(mut child) = child else { return log::info!("wl-paste not found: no clipboard history") };
        let Some(mut out) = child.stdout.take() else { return };
        let (mut history, mut buf, mut chunk) = (Vec::<String>::new(), Vec::<u8>::new(), [0u8; 4096]);
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
                if push(&mut history, &text) {
                    let h = history.clone();
                    rep.patch(move |d| d.clipboard = h);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_first_without_duplicates() {
        let mut h = vec![];
        assert!(push(&mut h, "a"));
        assert!(push(&mut h, "b"));
        assert!(push(&mut h, "a"));
        assert_eq!(h, vec!["a", "b"]);
        assert!(!push(&mut h, "   \n"));
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn caps_the_list_and_the_length() {
        let mut h = vec![];
        for i in 0..40 {
            push(&mut h, &i.to_string());
        }
        assert_eq!(h.len(), MAX);
        assert_eq!(h[0], "39");
        push(&mut h, &"x".repeat(10_000));
        assert_eq!(h[0].chars().count(), MAX_CHARS);
    }

    #[test]
    fn password_manager_entries_are_secret() {
        assert!(is_secret("text/plain\nx-kde-passwordManagerHint\n"));
        assert!(!is_secret("text/plain\ntext/html\n"));
    }
}
