//! The local control socket: `open-island msg <event> [payload]` (progress bars from scripts, peek, timers).
//! One text line per message; works the same on Linux, macOS (unix socket) and Windows (named pipe).

use super::Reporter;
use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions};
use island_ui::model::Input;
use std::io::{BufRead, BufReader, Write};

/// The socket's name; OPEN_ISLAND_SOCKET adds a suffix so a second island (a test run) can live next to the first.
fn socket_name() -> String {
    match std::env::var("OPEN_ISLAND_SOCKET") {
        Ok(suffix) if !suffix.is_empty() => format!("open-island-{suffix}.sock"),
        _ => "open-island.sock".to_string(),
    }
}

fn page(name: &str) -> Option<island_core::face::Page> {
    use island_core::face::Page;
    Some(match name {
        "home" => Page::Home,
        "cal" | "calendar" => Page::Cal,
        "qs" => Page::Qs,
        "sys" | "system" => Page::Sys,
        "notifs" | "notifications" => Page::Notifs,
        "media" => Page::Media,
        "clip" | "clipboard" => Page::Clip,
        "focus" => Page::Focus,
        "timer" => Page::Timer,
        _ => return None,
    })
}

fn parse(line: &str) -> Option<Input> {
    let line = line.trim();
    let (event, payload) = line.split_once(char::is_whitespace).map_or((line, ""), |(a, b)| (a, b.trim()));
    Some(match event {
        "peek" => Input::Peek,
        "reply" => Input::StartReply,
        "open" => Input::OpenKeyboard(match payload {
            "" => None,
            p => Some(page(p)?),
        }),
        "dnd" => match payload {
            "on" => Input::SetDnd(true),
            "off" => Input::SetDnd(false),
            _ => return None,
        },
        "progress" => Input::Progress(payload.into()),
        "progress-done" => Input::ProgressDone(payload.into()),
        "progress-fail" => Input::ProgressFail(payload.into()),
        "progress-clear" => Input::ProgressClear,
        "timer" => Input::StartTimer(island_core::duration::parse(payload).unwrap_or(300)),
        "pomodoro" => Input::StartPomodoro,
        "stopwatch" => Input::StartStopwatch,
        "timer-cancel" => Input::CancelTimer,
        "page" => Input::OpenPage(page(payload)?),
        _ => return None,
    })
}

pub fn start(rep: Reporter) {
    let Ok(name) = socket_name().to_ns_name::<GenericNamespaced>() else { return };
    let listener = match ListenerOptions::new().name(name).create_sync() {
        Ok(l) => l,
        Err(e) => {
            log::warn!("control socket unavailable (another island running?): {e}");
            return;
        }
    };
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let rep = rep.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(conn);
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 0 {
                    if let Some(input) = parse(&line) {
                        rep.input(input);
                    } else {
                        log::warn!("unknown message: {}", line.trim());
                    }
                    line.clear();
                }
            });
        }
    });
}

/// Sends one message to a running island (the `msg` subcommand). Returns false when none is running.
pub fn send(line: &str) -> bool {
    let Ok(name) = socket_name().to_ns_name::<GenericNamespaced>() else { return false };
    let Ok(mut conn) = interprocess::local_socket::Stream::connect(name) else { return false };
    conn.write_all(format!("{line}\n").as_bytes()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_messages() {
        assert!(matches!(parse("peek"), Some(Input::Peek)));
        assert!(matches!(parse("reply"), Some(Input::StartReply)));
        assert!(matches!(parse("dnd on"), Some(Input::SetDnd(true))));
        assert!(parse("dnd maybe").is_none());
        assert!(matches!(parse("progress 42 Copying"), Some(Input::Progress(p)) if p == "42 Copying"));
        assert!(matches!(parse("timer 25m"), Some(Input::StartTimer(1500))));
        assert!(matches!(parse("page cal"), Some(Input::OpenPage(_))));
        assert!(matches!(parse("open qs"), Some(Input::OpenKeyboard(Some(_)))));
        assert!(matches!(parse("open"), Some(Input::OpenKeyboard(None))));
        assert!(parse("open nowhere").is_none());
        assert!(parse("page nope").is_none());
        assert!(parse("garbage").is_none());
    }
}
