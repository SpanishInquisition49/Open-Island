//! The iPhone, through `tether` (https://github.com/zackb/tether): its battery, the Bluetooth link, the message
//! conversations, and replying. The island only runs the `tether` command (arguments are never passed through a shell)
//! and reads what it prints; nothing is sent anywhere but through the phone's own Bluetooth link. Without `tether` or
//! its daemon the Phone page is not offered.

use super::Reporter;
use island_core::phone;
use island_ui::model::{Command, Incoming, Input, Phone, PhoneCall};
use std::collections::HashMap;
use std::process::{Command as Process, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Seconds between looks at the connection and the conversations; calls are looked at every `TICK` while call control is on.
const FULL_EVERY: u32 = 5;
const TICK: u64 = 4;

/// When each conversation last had a message, as of the previous look (None before the first look: nothing is news then).
static SEEN: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
/// Calls already announced (state and number), so a ringing call is one card.
static RANG: Mutex<Vec<String>> = Mutex::new(Vec::new());
static CALLS_ON: AtomicBool = AtomicBool::new(false);
/// The battery warning was given and the charge has not come back up since.
static LOW_TOLD: AtomicBool = AtomicBool::new(false);
static CARD: AtomicU32 = AtomicU32::new(3_100_000);

/// Where `tether` is, if it is installed.
fn binary() -> Option<std::path::PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join("tether")).find(|p| p.is_file())
}

/// Runs `tether` with these arguments and gives what it printed; None when it fails (exit status), or takes over 8 seconds.
fn run(args: &[&str]) -> Option<String> {
    let mut child = Process::new(binary()?).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let began = Instant::now();
    let status = loop {
        match child.try_wait().ok()? {
            Some(st) => break st,
            None if began.elapsed() > Duration::from_secs(8) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(40)),
        }
    };
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    status.success().then_some(out)
}

/// A conversation key as the thread list writes it.
fn valid_key(key: &str) -> bool {
    (key.starts_with("sender:") || key.starts_with("tel:")) && !key.contains(['\n', '\r']) && key.len() < 120
}

/// `tether` reads its command line for options: a reply that starts with a dash (`--bt-hangup`) must not become one.
fn safe_text(text: &str) -> String {
    let t = text.trim();
    let dashes = t.chars().take_while(|c| *c == '-').count();
    format!("{}{}", "\u{2212}".repeat(dashes), &t[dashes..])
}

/// A number to dial: digits and a leading plus, nothing else.
fn valid_number(n: &str) -> bool {
    let d = n.strip_prefix('+').unwrap_or(n);
    !d.is_empty() && d.len() <= 20 && d.chars().all(|c| c.is_ascii_digit())
}

fn card(rep: &Reporter, summary: String, body: String, secs: u64) {
    rep.input(Input::Notification(Incoming { id: CARD.fetch_add(1, Ordering::Relaxed), app: "iPhone".into(), summary, body, expire_ms: Some(secs as i64 * 1000), ..Default::default() }));
}

/// A card for each conversation that has a message it did not have at the last look.
fn announce_messages(rep: &Reporter, threads: &[phone::Thread], quiet: bool) {
    let Ok(mut seen) = SEEN.lock() else { return };
    let first = seen.is_none();
    let map = seen.get_or_insert_with(HashMap::new);
    for t in threads {
        let news = map.get(&t.key).is_none_or(|w| *w < t.when);
        if news && !first && !quiet {
            card(rep, t.name.clone(), t.preview.clone(), 8);
        }
        map.insert(t.key.clone(), t.when.clone());
    }
}

fn announce_calls(rep: &Reporter, calls: &[phone::Call]) {
    let Ok(mut rang) = RANG.lock() else { return };
    let now: Vec<String> = calls.iter().filter(|c| c.is_ringing()).map(|c| format!("{}{}", c.state, c.number)).collect();
    for (c, key) in calls.iter().filter(|c| c.is_ringing()).zip(&now) {
        if !rang.contains(key) {
            let who = if c.name.is_empty() { c.number.clone() } else { c.name.clone() };
            card(rep, format!("Call · {who}"), "Open the iPhone page to answer".into(), 20);
            rep.exec(Command::PlaySound);
        }
    }
    *rang = now;
}

fn refresh_calls(rep: &Reporter) {
    let calls = run(&["--bt-calls"]).map(|t| phone::parse_calls(&t)).unwrap_or_default();
    announce_calls(rep, &calls);
    rep.patch(move |d| {
        if let Some(p) = d.phone.as_mut() {
            p.calls = calls;
        }
    });
}

/// Reads the link and the conversations; `quiet` for the look after our own reply, whose message is not news.
fn refresh(rep: &Reporter, quiet: bool) {
    let Some(text) = run(&["--bt-connection"]) else {
        rep.patch(|d| d.phone = None);
        return;
    };
    let link = phone::parse_link(&text);
    if !link.present {
        rep.patch(|d| d.phone = None);
        return;
    }
    CALLS_ON.store(link.calls, Ordering::Relaxed);
    let threads = if link.messages { run(&["--bt-threads"]).map(|t| phone::parse_threads(&t)).unwrap_or_default() } else { vec![] };
    announce_messages(rep, &threads, quiet);
    let calls = if link.calls { run(&["--bt-calls"]).map(|t| phone::parse_calls(&t)).unwrap_or_default() } else { vec![] };
    announce_calls(rep, &calls);
    // the phone's battery: one card at 20%, again only after it has been above 30% (the island's battery alerts switch applies)
    if let Some(b) = link.battery {
        if b > 30 {
            LOW_TOLD.store(false, Ordering::Relaxed);
        } else if b <= 20 && crate::config::load().ui.battery_alerts && !LOW_TOLD.swap(true, Ordering::Relaxed) {
            card(rep, format!("iPhone battery low · {b}%"), "Plug it in soon".into(), 8);
        }
    }
    let audio_here = run(&["--bt-phone-audio"]).and_then(|t| phone::parse_audio(&t));
    let at_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
    rep.patch(move |d| {
        let (messages, contacts) = d.phone.as_ref().map_or((None, None), |p| (p.messages.clone(), p.contacts.clone()));
        d.phone = Some(Phone { link, threads, messages, calls, contacts, audio_here, at_ms });
    });
}

fn load_messages(rep: &Reporter, key: &str) {
    if !valid_key(key) {
        return;
    }
    let msgs = run(&["--bt-messages", key]).map(|t| phone::parse_messages(&t)).unwrap_or_default();
    let key = key.to_string();
    rep.patch(move |d| {
        if let Some(p) = d.phone.as_mut() {
            p.messages = Some((key, msgs));
        }
    });
}

pub fn start(rep: Reporter) {
    std::thread::spawn(move || {
        let mut tick = 0u32;
        loop {
            if binary().is_some() {
                if tick.is_multiple_of(FULL_EVERY) {
                    refresh(&rep, false);
                } else if CALLS_ON.load(Ordering::Relaxed) {
                    refresh_calls(&rep);
                }
            }
            std::thread::sleep(Duration::from_secs(TICK));
            tick = tick.wrapping_add(1);
        }
    });
}

pub fn command(rep: &Reporter, c: &Command) {
    let rep = rep.clone();
    match c.clone() {
        Command::PhoneOpen(key) => {
            std::thread::spawn(move || load_messages(&rep, &key));
        }
        Command::PhoneSend { key, text } => {
            std::thread::spawn(move || {
                let text = text.trim().to_string();
                if !valid_key(&key) || text.is_empty() {
                    return;
                }
                let ok = run(&["--bt-send", &key, &safe_text(&text)]).is_some();
                log::info!("reply to {key}: {}", if ok { "sent" } else { "failed" });
                if !ok {
                    card(&rep, "Reply not sent".into(), "tether could not send it (is the iPhone in range?)".into(), 6);
                }
                load_messages(&rep, &key);
                refresh(&rep, true);
            });
        }
        Command::PhoneSearch(q) => {
            std::thread::spawn(move || {
                let found = run(&["--bt-contacts", &safe_text(&q)]).map(|t| phone::parse_contacts(&t)).unwrap_or_default();
                rep.patch(move |d| {
                    if let Some(p) = d.phone.as_mut() {
                        p.contacts = Some(found);
                    }
                });
            });
        }
        Command::PhoneAudio(here) => {
            std::thread::spawn(move || {
                if run(&["--bt-phone-audio", if here { "here" } else { "phone" }]).is_none() {
                    card(&rep, "Could not change where the audio plays".into(), "tether refused or could not be reached".into(), 5);
                }
                std::thread::sleep(Duration::from_millis(600));
                refresh(&rep, true);
            });
        }
        Command::PhoneCall(call) => {
            std::thread::spawn(move || {
                let ok = match &call {
                    PhoneCall::Answer => run(&["--bt-answer"]).is_some(),
                    PhoneCall::Hangup => run(&["--bt-hangup"]).is_some(),
                    PhoneCall::Dial(n) if valid_number(n) => run(&["--bt-call", n]).is_some(),
                    PhoneCall::Dial(_) => false,
                    PhoneCall::Enable(on) => run(&["--bt-calls-enable", if *on { "on" } else { "off" }]).is_some(),
                };
                if !ok {
                    card(&rep, "The call did not go through".into(), "tether refused or could not be reached".into(), 5);
                }
                // the setting is read back by the next full look; calls at once
                std::thread::sleep(Duration::from_millis(600));
                refresh(&rep, true);
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_never_becomes_an_option() {
        assert_eq!(safe_text("  ciao  "), "ciao");
        assert_eq!(safe_text("--bt-hangup"), "\u{2212}\u{2212}bt-hangup");
        assert_eq!(safe_text("-5 gradi"), "\u{2212}5 gradi");
        assert_eq!(safe_text("a-b"), "a-b");
    }

    #[test]
    fn only_plain_numbers_are_dialled() {
        assert!(valid_number("+393471234567") && valid_number("112"));
        assert!(!valid_number("") && !valid_number("+") && !valid_number("--bt-hangup") && !valid_number("12 34") && !valid_number(&"9".repeat(30)));
    }

    #[test]
    fn only_keys_from_the_thread_list_are_passed_on() {
        assert!(valid_key("sender:iliad") && valid_key("tel:+393472935666"));
        assert!(!valid_key("--bt-hangup") && !valid_key("") && !valid_key("tel:1\n2"));
        assert!(!valid_key(&format!("tel:{}", "9".repeat(200))));
    }
}
