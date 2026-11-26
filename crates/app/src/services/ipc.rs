//! The local control socket: `open-island msg <event> [payload]` (progress bars from scripts, peek, timers).
//! One text line per message; works the same on Linux, macOS (unix socket) and Windows (named pipe).

use super::Reporter;
use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions};
use island_ui::model::Input;
use std::io::{BufRead, BufReader, Write};

/// The socket's name; OPEN_ISLAND_SOCKET adds a suffix so a second island (a test run) can live next to the first.
fn socket_name() -> String {
    // a test must never reach the island the user is running
    if cfg!(test) {
        return format!("open-island-test-{}.sock", std::process::id());
    }
    match std::env::var("OPEN_ISLAND_SOCKET") {
        Ok(suffix) if !suffix.is_empty() => format!("open-island-{suffix}.sock"),
        _ => "open-island.sock".to_string(),
    }
}

fn page(name: &str) -> Option<island_core::face::Page> {
    island_core::face::page_by_name(name)
}

/// Settings from a script: `font <family or file>`, `calendar add <url>`, `calendar remove <n>` (1-based),
/// `location <lat> <lon>`, `sound <file>`. They go through the same code as the settings menu.
fn parse_command(line: &str) -> Option<island_ui::model::Command> {
    use island_ui::model::Command;
    let line = line.trim();
    let (event, payload) = line.split_once(char::is_whitespace).map_or((line, ""), |(a, b)| (a, b.trim()));
    Some(match event {
        "refresh" => Command::RefreshCalendars,
        "ocr" => Command::Ocr,
        "font" => Command::SetFont(payload.to_string()),
        // `set <option> <value>`: the same as the options page
        "set" => {
            use island_ui::model::OptionValue;
            let (key, value) = payload.split_once(char::is_whitespace).map_or((payload, ""), |(a, b)| (a, b.trim()));
            let value = if crate::config::BOOL_OPTIONS.contains(&key) {
                OptionValue::Bool(match value.to_ascii_lowercase().as_str() {
                    "on" | "true" | "yes" | "1" => true,
                    "off" | "false" | "no" | "0" => false,
                    _ => return None,
                })
            } else if key == "evening_recap_time" || key == "morning_recap_time" {
                OptionValue::Text(value.to_string())
            } else if crate::config::NUM_OPTIONS.iter().any(|(k, ..)| *k == key) {
                OptionValue::Num(value.parse().ok()?)
            } else if matches!(key, "timer_presets" | "screen" | "travel_mode" | "map_tile_url" | "map_api_key") {
                OptionValue::Text(value.to_string())
            } else {
                return None;
            };
            Command::SetOption { key: key.to_string(), value }
        }
        "sound" => Command::SetNotificationSound(payload.to_string()),
        // `home 43.7085, 10.4036` or `home Via Roma 1, Pisa`; `train pisa centrale 17:12 verso lucca`, `train off`
        "home" if !payload.is_empty() => Command::SetHome(payload.to_string()),
        "train" if matches!(payload.to_lowercase().as_str(), "off" | "unpin") => Command::UnpinTrain,
        "train" if island_core::train::parse_pin_query(payload).is_some() => Command::PinTrain(payload.to_string()),
        "calendar" => match payload.split_once(char::is_whitespace) {
            Some(("add", url)) if island_ui::is_calendar_source(url) => Command::AddCalendar(url.trim().to_string()),
            Some(("remove", n)) => Command::RemoveCalendar(n.trim().parse::<usize>().ok()?.checked_sub(1)?),
            _ => return None,
        },
        _ => return None,
    })
}

pub(crate) fn parse(line: &str) -> Option<Input> {
    let line = line.trim();
    let (event, payload) = line.split_once(char::is_whitespace).map_or((line, ""), |(a, b)| (a, b.trim()));
    Some(match event {
        // a notification from a script on any platform: `notify <summary> | <body>`
        "notify" if !payload.is_empty() => {
            static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(2_000_000);
            let (summary, body) = payload.split_once(" | ").unwrap_or((payload, ""));
            Input::Notification(island_ui::model::Incoming {
                id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                app: "open-island".into(),
                summary: summary.trim().into(),
                body: body.trim().into(),
                ..Default::default()
            })
        }
        // `card <json>`: what `open-island ok ...` sends; `dismiss [name]`: takes a card away
        "card" => Input::Notification(super::cards::post(payload)?.0.card),
        "dismiss" if payload.is_empty() => Input::DismissCard,
        "dismiss" => Input::NotificationClosed(super::cards::take(payload)?),
        "hits" => Input::DumpHits,
        // `send <path>[<tab><path>...]`: files to send to a device on the network
        "send" if !payload.is_empty() => Input::SendFiles(payload.split('\t').map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect()),
        // `key enter|escape|backspace|left|right|up|down|home|end|tab` or `key text <what to type>`: for tests, the
        // same input a keyboard gives when the island holds it
        "key" => {
            use island_ui::model::KeyInput as K;
            Input::Key(match payload.split_once(' ').map_or((payload, ""), |(a, b)| (a, b)) {
                ("enter", _) => K::Enter,
                ("escape", _) => K::Escape,
                ("backspace", _) => K::Backspace,
                ("delete", _) => K::Delete,
                ("left", _) => K::Left,
                ("right", _) => K::Right,
                ("up", _) => K::Up,
                ("down", _) => K::Down,
                ("home", _) => K::Home,
                ("end", _) => K::End,
                ("tab", _) => K::NextPage,
                ("text", t) if !t.is_empty() => K::Text(t.to_string()),
                _ => return None,
            })
        }
        "click" => {
            let mut it = payload.split_whitespace().filter_map(|v| v.parse::<f64>().ok());
            Input::Click(it.next()?, it.next()?)
        }
        "peek" => Input::Peek,
        "reply" => Input::StartReply,
        "bar" | "command" => Input::OpenBar,
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
        "activity" => {
            // "<run|done|fail|clear> <json>"
            use island_core::progress::{parse_color, Activity, State};
            let (state, json) = payload.split_once(char::is_whitespace).map_or((payload, ""), |(a, b)| (a, b.trim()));
            let state = match state {
                "run" => State::Run,
                "done" => State::Done,
                "fail" => State::Fail,
                "clear" => {
                    let v: serde_json::Value = if json.is_empty() { serde_json::json!({}) } else { serde_json::from_str(json).ok()? };
                    return Some(Input::ActivityClear(v.get("id").and_then(|x| x.as_str()).unwrap_or("").trim().chars().take(80).collect()));
                }
                _ => return None,
            };
            let v: serde_json::Value = if json.is_empty() { serde_json::json!({}) } else { serde_json::from_str(json).ok()? };
            let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").trim().chars().take(80).collect::<String>();
            Input::Activity(
                state,
                Activity {
                    id: s("id"),
                    label: s("label"),
                    detail: s("detail"),
                    icon: Some(s("icon")).filter(|i| island_ui::is_known_icon(i)),
                    tint: parse_color(&s("color")),
                    frac: v.get("progress").and_then(|x| x.as_f64()).filter(|x| x.is_finite()).map(|x| x.clamp(0.0, 100.0) / 100.0),
                    file: None,
                },
            )
        }
        "cmd-start" => {
            // "<id> <seconds> <label...>"
            let mut it = payload.splitn(3, ' ');
            let (id, secs, label) = (it.next()?, it.next()?.parse().ok()?, it.next().unwrap_or("").trim());
            if id.is_empty() || label.is_empty() {
                return None;
            }
            Input::CommandStart { id: id.into(), label: label.into(), after_secs: secs }
        }
        "cmd-end" => {
            let mut it = payload.split_whitespace();
            Input::CommandEnd { id: it.next()?.into(), code: it.next()?.parse().ok()? }
        }
        // "25m", "25m tea", "tea 25m"? no: the duration first, as in the command bar; the rest names it
        "timer" => match island_core::bar::parse(&format!("timer {payload}"), 0) {
            Some(island_core::bar::Cmd::Timer { secs, label }) if label.is_empty() => Input::StartTimer(secs),
            Some(island_core::bar::Cmd::Timer { secs, label }) => Input::StartNamedTimer(secs, label),
            _ => Input::StartTimer(island_core::duration::parse(payload).unwrap_or(300)),
        },
        // `todo <text>` or `todo add <text>`: something for the list on the Today page
        "do" if !payload.is_empty() => Input::RunLine(payload.to_string()),
        "find" if !payload.is_empty() => Input::Find(payload.to_string()),
        "todo" if !payload.is_empty() => Input::TodoAdd(payload.strip_prefix("add ").unwrap_or(payload).into()),
        // `pin <text>`: a note that stays on the Today page; `unpin [text|all]` takes it off
        "pin" if !payload.is_empty() => Input::PinAdd(payload.into()),
        "unpin" => Input::PinRemove(payload.into()),
        "pomodoro" => Input::StartPomodoro,
        "stopwatch" => Input::StartStopwatch,
        // "timer-cancel" ends the main timer; "timer-cancel tea" the ones named tea
        "timer-cancel" => {
            if payload.is_empty() {
                Input::CancelTimer
            } else {
                Input::CancelTimerNamed(payload.into())
            }
        }
        "page" => match payload {
            "next" => Input::CyclePage(1),
            "prev" | "previous" => Input::CyclePage(-1),
            p => Input::OpenPage(page(p)?),
        },
        _ => return None,
    })
}

/// The shells with a command running (their pid is the id), so that a shell that dies mid-command does not leave
/// its pill on for ever.
static LIVE: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn track(input: &Input) {
    let mut live = LIVE.lock().unwrap();
    match input {
        Input::CommandStart { id, .. } if !live.contains(id) => live.push(id.clone()),
        Input::CommandEnd { id, .. } | Input::CommandGone(id) => live.retain(|l| l != id),
        _ => {}
    }
}

#[cfg(target_os = "linux")]
fn watch_shells(rep: Reporter) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(5));
        let gone: Vec<String> = LIVE.lock().unwrap().iter().filter(|id| id.parse::<u32>().is_ok() && !std::path::Path::new(&format!("/proc/{id}")).exists()).cloned().collect();
        for id in gone {
            LIVE.lock().unwrap().retain(|l| *l != id);
            rep.input(Input::CommandGone(id));
        }
    });
}

pub fn start(rep: Reporter) {
    #[cfg(target_os = "linux")]
    watch_shells(rep.clone());
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
                    if let Some(spec) = line.trim().strip_prefix("watch ") {
                        super::watchpid::start(&rep, spec);
                    } else if let Some(json) = line.trim().strip_prefix("card ") {
                        match super::cards::post(json) {
                            Some((posted, sound)) => {
                                if let Some(file) = sound {
                                    super::sound::play(&file);
                                }
                                rep.input(Input::Notification(posted.card));
                                // a question: the script waits on this connection for what was pressed
                                if let Some(answer) = posted.answer {
                                    let word = match answer.recv_timeout(super::cards::WAIT_LIMIT) {
                                        Ok(super::cards::Answer::Yes) => "yes",
                                        Ok(super::cards::Answer::No) => "no",
                                        Err(_) => "none",
                                    };
                                    let _ = reader.get_mut().write_all(format!("{word}\n").as_bytes());
                                }
                            }
                            None => log::warn!("a card with no title was asked for"),
                        }
                    // a name the island does not know is nothing to take away, not a mistake
                    } else if line.trim().starts_with("dismiss ") {
                        if let Some(input) = parse(&line) {
                            rep.input(input);
                        }
                    } else if let Some(input) = parse(&line) {
                        track(&input);
                        rep.input(input);
                    } else if let Some(cmd) = parse_command(&line) {
                        rep.exec(cmd);
                    } else {
                        log::warn!("unknown message: {}", line.trim());
                    }
                    line.clear();
                }
            });
        }
    });
}

/// Sends a message that is answered with one line, and waits for it. None when no island is running.
pub fn ask(line: &str) -> Option<String> {
    let name = socket_name().to_ns_name::<GenericNamespaced>().ok()?;
    let mut conn = interprocess::local_socket::Stream::connect(name).ok()?;
    conn.write_all(format!("{line}\n").as_bytes()).ok()?;
    let mut answer = String::new();
    BufReader::new(conn).read_line(&mut answer).ok()?;
    Some(answer.trim().to_string())
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

    /// The whole way over a real socket: a question goes out, the island's button press comes back to the asker.
    #[test]
    fn a_question_is_answered_over_the_socket() {
        use crate::services::{cards, Msg};
        use island_ui::model::{CloseReason, Command};
        use std::time::Duration;
        let (tx, rx) = std::sync::mpsc::channel();
        start(Reporter { tx, waker: std::sync::Arc::new(|| {}) });
        // (other tests of this process talk to the same socket meanwhile: only this test's cards count)
        let next = || loop {
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(Msg::Input(Input::Notification(n))) if ["Merge?", "named"].contains(&n.summary.as_str()) => break Input::Notification(n),
                Ok(Msg::Input(i @ (Input::NotificationClosed(_) | Input::DismissCard))) => break i,
                Ok(_) => {}
                Err(e) => panic!("an input: {e}"),
            }
        };
        for (press, expected) in [(Some("yes"), "yes"), (Some("no"), "no"), (None, "none")] {
            let asker = std::thread::spawn(|| ask(r#"card {"title":"Merge?","yes":"Merge","no":"Later","id":"pr"}"#));
            let Input::Notification(card) = next() else { panic!("the card") };
            if let Some(key) = press {
                assert!(cards::on_command(&Command::InvokeAction { id: card.id, key: key.into() }));
            }
            assert!(cards::on_command(&Command::NotificationClosed { id: card.id, reason: CloseReason::Dismissed }));
            assert_eq!(asker.join().unwrap().as_deref(), Some(expected));
        }
        assert!(send(r#"card {"title":"named","id":"x"}"#));
        let Input::Notification(card) = next() else { panic!("the card") };
        assert!(send("dismiss x"));
        assert!(matches!(next(), Input::NotificationClosed(id) if id == card.id));
        assert!(send("dismiss nothing") && send("dismiss"));
        assert!(matches!(next(), Input::DismissCard), "the unknown name said nothing");
    }

    #[test]
    fn parses_messages() {
        assert!(matches!(parse("peek"), Some(Input::Peek)));
        match parse("notify Build done | all 120 tests passed") {
            Some(Input::Notification(n)) => assert_eq!((n.summary.as_str(), n.body.as_str(), n.app.as_str()), ("Build done", "all 120 tests passed", "open-island")),
            _ => panic!("a notification"),
        }
        match parse("notify only a title") {
            Some(Input::Notification(n)) => assert_eq!((n.summary.as_str(), n.body.as_str()), ("only a title", "")),
            _ => panic!("a notification"),
        }
        assert!(parse("notify").is_none(), "nothing to show");
        assert!(matches!(parse("reply"), Some(Input::StartReply)));
        assert!(matches!(parse("pin ssh 10.0.0.5"), Some(Input::PinAdd(t)) if t == "ssh 10.0.0.5"));
        assert!(parse("pin").is_none());
        assert!(matches!(parse("unpin all"), Some(Input::PinRemove(t)) if t == "all"));
        assert!(matches!(parse("unpin"), Some(Input::PinRemove(t)) if t.is_empty()));
        assert!(matches!(parse("dnd on"), Some(Input::SetDnd(true))));
        assert!(parse("dnd maybe").is_none());
        assert!(matches!(parse("progress 42 Copying"), Some(Input::Progress(p)) if p == "42 Copying"));
        assert!(matches!(parse("cmd-start 99 10 cargo build -j8"), Some(Input::CommandStart { id, label, after_secs: 10 }) if id == "99" && label == "cargo build -j8"));
        assert!(matches!(parse(r#"activity run {"label":"CI","detail":"building","icon":"git-branch","color":"orange","progress":40}"#),
            Some(Input::Activity(island_core::progress::State::Run, a)) if a.label == "CI" && a.detail == "building" && a.icon.as_deref() == Some("git-branch") && a.tint == Some((0xff, 0x9f, 0x0a)) && a.frac == Some(0.4)));
        assert!(matches!(parse(r#"activity fail {"label":"CI","detail":"2 failed"}"#), Some(Input::Activity(island_core::progress::State::Fail, a)) if a.detail == "2 failed" && a.icon.is_none() && a.tint.is_none()));
        assert!(matches!(parse("activity clear"), Some(Input::ActivityClear(id)) if id.is_empty()));
        assert!(matches!(parse_command("home 43.7, 10.4"), Some(island_ui::model::Command::SetHome(h)) if h == "43.7, 10.4"));
        assert!(matches!(parse_command("train pisa centrale 17:12"), Some(island_ui::model::Command::PinTrain(_))));
        assert!(matches!(parse_command("train off"), Some(island_ui::model::Command::UnpinTrain)));
        assert!(parse_command("train pisa centrale").is_none() && parse_command("home").is_none());
        assert!(matches!(parse("timer 25m tea time"), Some(Input::StartNamedTimer(1500, l)) if l == "tea time"));
        assert!(matches!(parse("timer-cancel tea"), Some(Input::CancelTimerNamed(l)) if l == "tea"));
        assert!(matches!(parse("timer-cancel"), Some(Input::CancelTimer)));
        assert!(matches!(parse(r#"activity run {"id":"s1","label":"Claude"}"#), Some(Input::Activity(island_core::progress::State::Run, a)) if a.id == "s1"));
        assert!(matches!(parse("key enter"), Some(Input::Key(island_ui::model::KeyInput::Enter))));
        assert!(matches!(parse("key text 10m tea"), Some(Input::Key(island_ui::model::KeyInput::Text(t))) if t == "10m tea"));
        assert!(parse("key wobble").is_none() && parse("key text").is_none());
        assert!(parse("activity wobble {}").is_none() && parse("activity run {broken").is_none());
        let opt = |l: &str| parse_command(l);
        assert_eq!(opt("set lyrics off"), Some(island_ui::model::Command::SetOption { key: "lyrics".into(), value: island_ui::model::OptionValue::Bool(false) }));
        assert_eq!(opt("set notification_seconds 8"), Some(island_ui::model::Command::SetOption { key: "notification_seconds".into(), value: island_ui::model::OptionValue::Num(8.0) }));
        assert_eq!(opt("set timer_presets 5m, 20m"), Some(island_ui::model::Command::SetOption { key: "timer_presets".into(), value: island_ui::model::OptionValue::Text("5m, 20m".into()) }));
        assert!(opt("set lyrics maybe").is_none() && opt("set nonsense on").is_none() && opt("set osd_seconds fast").is_none());
        assert!(matches!(parse("cmd-end 99 130"), Some(Input::CommandEnd { id, code: 130 }) if id == "99"));
        assert!(parse("cmd-start 99 ten make").is_none() && parse("cmd-start 99 5").is_none() && parse("cmd-end 99").is_none());
        assert!(matches!(parse("timer 25m"), Some(Input::StartTimer(1500))));
        assert!(matches!(parse("send /a b/c.txt\t/d.txt"), Some(Input::SendFiles(f)) if f == vec!["/a b/c.txt".to_string(), "/d.txt".to_string()]));
        assert!(parse("send").is_none());
        assert!(matches!(parse("page cal"), Some(Input::OpenPage(_))));
        assert!(matches!(parse("page next"), Some(Input::CyclePage(1))));
        assert!(matches!(parse("page prev"), Some(Input::CyclePage(-1))));
        assert!(matches!(parse("open qs"), Some(Input::OpenKeyboard(Some(_)))));
        assert!(matches!(parse("open"), Some(Input::OpenKeyboard(None))));
        assert!(parse("open nowhere").is_none());
        assert!(parse("page nope").is_none());
        assert!(parse("garbage").is_none());
        use island_ui::model::Command;
        assert_eq!(parse_command("refresh"), Some(Command::RefreshCalendars));
        assert_eq!(parse_command("font Inter"), Some(Command::SetFont("Inter".into())));
        assert_eq!(parse_command("calendar remove 2"), Some(Command::RemoveCalendar(1)));
        assert_eq!(parse_command("calendar remove 0"), None);
        assert_eq!(parse_command("calendar add webcal://x.org/a/b.ics"), Some(Command::AddCalendar("webcal://x.org/a/b.ics".into())));
        assert_eq!(parse_command("calendar add nonsense"), None);
        assert_eq!(parse_command("nothing"), None);
    }
}
