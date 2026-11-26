//! The iPhone as `tether` (https://github.com/zackb/tether) reports it: what the Bluetooth link carries, the message
//! conversations and the messages in one. The island reads the text the `tether` command prints, so these are parsers
//! for that text. Anything they do not understand is skipped, never guessed.

/// What `tether --bt-connection` says.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Link {
    pub present: bool,
    pub messages: bool,
    pub contacts: bool,
    /// The low-energy link that carries notifications.
    pub notifications: bool,
    pub calls: bool,
    pub battery: Option<u8>,
}

fn yes(v: &str) -> bool {
    v.trim().eq_ignore_ascii_case("yes")
}

pub fn parse_link(text: &str) -> Link {
    let mut l = Link::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        match k.trim() {
            "Device present" => l.present = yes(v),
            "Messages (MAP)" => l.messages = yes(v),
            "Contacts (PBAP)" => l.contacts = yes(v),
            "Notifications" => l.notifications = yes(v),
            "Calls (HFP)" => l.calls = yes(v),
            "Battery" => l.battery = v.trim().trim_end_matches('%').parse().ok().filter(|b| *b <= 100),
            _ => {}
        }
    }
    l
}

/// `tether --bt-phone-audio`: whether the iPhone's music and sounds play on this computer (true) or stay on the phone.
pub fn parse_audio(text: &str) -> Option<bool> {
    let t = text.lines().next()?.to_lowercase();
    if t.contains("this computer") {
        Some(true)
    } else if t.contains("plays on") || t.contains("stays on") || t.contains("iphone") {
        Some(false)
    } else {
        None
    }
}

/// One conversation.
#[derive(Clone, Debug, PartialEq)]
pub struct Thread {
    pub name: String,
    /// What to give `--bt-messages` / `--bt-send`: `sender:iliad` or `tel:+39...`.
    pub key: String,
    /// `2026-10-26 09:23`
    pub when: String,
    pub preview: String,
}

/// `YYYY-MM-DD HH:MM` at the start of `s`, when there is one.
fn stamp(s: &str) -> Option<&str> {
    let b = s.as_bytes();
    let ok = b.len() >= 16 && b[..16].iter().enumerate().all(|(i, c)| match i {
        4 | 7 => *c == b'-',
        10 => *c == b' ',
        13 => *c == b':',
        _ => c.is_ascii_digit(),
    });
    ok.then(|| &s[..16])
}

/// `  name      2026-10-26 09:23   preview` followed by `      sender:name` (or `tel:`), newest first.
pub fn parse_threads(text: &str) -> Vec<Thread> {
    let mut out: Vec<Thread> = vec![];
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("sender:") || t.starts_with("tel:") {
            if let Some(last) = out.last_mut().filter(|l| l.key.is_empty()) {
                last.key = t.to_string();
            }
            continue;
        }
        // the date is the only fixed thing: the name is what comes before it, the preview after
        let Some(at) = (0..t.len()).filter(|i| t.is_char_boundary(*i)).find(|i| stamp(&t[*i..]).is_some() && *i > 0 && t[..*i].ends_with(' ')) else { continue };
        let name = t[..at].trim().to_string();
        let when = t[at..at + 16].to_string();
        let preview = t[at + 16..].trim().trim_end_matches("...").trim().to_string();
        if !name.is_empty() {
            out.push(Thread { name, key: String::new(), when, preview });
        }
    }
    out.retain(|t| !t.key.is_empty());
    out
}

/// One message of a conversation.
#[derive(Clone, Debug, PartialEq)]
pub struct Msg {
    pub when: String,
    /// Written by the user (`me`), else by the other side (`them`).
    pub mine: bool,
    pub text: String,
}

/// `2026-08-26 21:37  them     text` with the text going on over the next lines until the next stamp.
pub fn parse_messages(text: &str) -> Vec<Msg> {
    let mut out: Vec<Msg> = vec![];
    for line in text.lines() {
        if let Some(when) = stamp(line) {
            let rest = line[16..].trim_start();
            let (who, body) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            if who == "them" || who == "me" {
                out.push(Msg { when: when.to_string(), mine: who == "me", text: body.trim().to_string() });
                continue;
            }
        }
        if let Some(last) = out.last_mut() {
            if !line.trim().is_empty() {
                if !last.text.is_empty() {
                    last.text.push(' ');
                }
                last.text.push_str(line.trim());
            }
        }
    }
    out
}

/// A call on the iPhone, as `tether --bt-calls` lists it: `state  number  name` and the call's path on the next line.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    /// What tether says ("ringing", "active", ...): kept as it is.
    pub state: String,
    pub number: String,
    pub name: String,
}

impl Call {
    /// Somebody is calling and nobody has answered.
    pub fn is_ringing(&self) -> bool {
        let s = self.state.to_lowercase();
        s.contains("ring") || s.contains("incoming") || s.contains("alert") || s.contains("waiting")
    }
}

pub fn parse_calls(text: &str) -> Vec<Call> {
    let mut out = vec![];
    for line in text.lines() {
        // the call's path is the line under it
        if line.starts_with("  ") || line.trim().is_empty() || line.starts_with("No calls") || line.starts_with("Could not") {
            continue;
        }
        let mut f = line.split_whitespace();
        let (Some(state), Some(number)) = (f.next(), f.next()) else { continue };
        let name = f.collect::<Vec<_>>().join(" ");
        out.push(Call { state: state.to_string(), number: number.to_string(), name });
    }
    out
}

/// One contact with its numbers, as `tether --bt-contacts` lists them.
#[derive(Clone, Debug, PartialEq)]
pub struct Contact {
    pub name: String,
    pub numbers: Vec<String>,
}

/// `  Name` (two spaces) and under it each number with six.
pub fn parse_contacts(text: &str) -> Vec<Contact> {
    let mut out: Vec<Contact> = vec![];
    for line in text.lines() {
        let indent = line.len() - line.trim_start().len();
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        match indent {
            2..=4 => out.push(Contact { name: t.to_string(), numbers: vec![] }),
            n if n >= 5 => {
                // an address is written with its kind (`tel:+39...`); only telephone numbers can be written to
                let number = match t.split_once(':') {
                    Some(("tel", n)) => Some(n.trim()),
                    Some(_) => None,
                    None => Some(t),
                };
                if let (Some(c), Some(n)) = (out.last_mut(), number.filter(|n| !n.is_empty())) {
                    c.numbers.push(n.to_string());
                }
            }
            _ => {}
        }
    }
    out.retain(|c| !c.numbers.is_empty());
    out
}

/// The key of a conversation with this number (`tel:+39...`), numbers cleaned of spaces and dashes.
pub fn key_for_number(number: &str) -> String {
    format!("tel:{}", number.chars().filter(|c| c.is_ascii_digit() || *c == '+').collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_link_is_read_with_its_battery() {
        let t = "Device present:   yes\nBR/EDR:           yes\nLE:               no\nMessages (MAP):   yes\nContacts (PBAP):  yes\nNotifications:    no\nCalls (HFP):      no\nBattery:          80%\n\n  Connected. Waiting";
        assert_eq!(parse_link(t), Link { present: true, messages: true, contacts: true, notifications: false, calls: false, battery: Some(80) });
        assert_eq!(parse_link("Tether: not running"), Link::default());
        assert_eq!(parse_link("Battery: 400%").battery, None);
    }

    #[test]
    fn conversations_are_read_with_their_keys() {
        let t = "  iliad                    2026-10-26 09:23   Ciao, la tua offerta iliad a 9,99€ al mese ...\n      sender:iliad\n  Enrica                   2026-10-03 17:06   Se vuoi possiamo chiedere anche a Giovanni ch...\n      tel:+393472935666\n  +393938320663            2026-09-03 15:59   Ciao Sono il corriere\n      tel:+393938320663\n";
        let v = parse_threads(t);
        assert_eq!(v.len(), 3);
        assert_eq!((v[0].name.as_str(), v[0].key.as_str(), v[0].when.as_str()), ("iliad", "sender:iliad", "2026-10-26 09:23"));
        assert_eq!(v[0].preview, "Ciao, la tua offerta iliad a 9,99€ al mese");
        assert_eq!((v[1].name.as_str(), v[1].key.as_str()), ("Enrica", "tel:+393472935666"));
        assert_eq!(v[2].name, "+393938320663");
        assert!(parse_threads("No conversations yet. Check 'tether --bt-connection'.").is_empty());
        assert!(parse_threads("  lonely  2026-10-03 17:06 text\n").is_empty(), "no key, no way to open it");
    }

    #[test]
    fn messages_keep_their_side_and_run_over_lines() {
        let t = "2026-08-26 21:37  them     Ciao, la tua offerta\nE come ogni mese, paghi\n2026-08-27 16:14  me       Grazie\n";
        let v = parse_messages(t);
        assert_eq!(v.len(), 2);
        assert!(!v[0].mine && v[0].text == "Ciao, la tua offerta E come ogni mese, paghi");
        assert!(v[1].mine && v[1].text == "Grazie" && v[1].when == "2026-08-27 16:14");
        assert!(parse_messages("No messages in x").is_empty());
    }

    #[test]
    fn calls_and_contacts_are_read_as_tether_prints_them() {
        let calls = "ringing      +393471234567            Enrica\n  /org/bluez/hci0/dev_CC/telephony1/call1\nactive       withheld                 \n  /org/bluez/hci0/dev_CC/telephony1/call2\n";
        let v = parse_calls(calls);
        assert_eq!(v.len(), 2);
        assert!(v[0].is_ringing() && v[0].number == "+393471234567" && v[0].name == "Enrica");
        assert!(!v[1].is_ringing() && v[1].name.is_empty());
        assert!(parse_calls("No calls. Check 'tether --bt-connection'.").is_empty());
        let c = parse_contacts("  Enrica Rossi\n      tel:+39 347 123 4567\n      tel:+39 06 111\n      mailto:e@x.it\n  Senza numero\n  Solo mail\n      mailto:a@b.it\n  Luca\n      3331112222\n");
        assert_eq!(c.len(), 2, "a contact with no number cannot be written to");
        assert_eq!((c[0].name.as_str(), c[0].numbers.len()), ("Enrica Rossi", 2));
        assert_eq!(key_for_number("+39 347-123 4567"), "tel:+393471234567");
        assert!(parse_contacts("No contacts match zz").is_empty());
    }

    #[test]
    fn where_the_phone_audio_plays_is_read() {
        assert_eq!(parse_audio("The iPhone's audio plays on this computer.\nRemembered choice: auto"), Some(true));
        assert_eq!(parse_audio("The iPhone's audio plays on the iPhone.\nRemembered choice: phone"), Some(false));
        assert_eq!(parse_audio(""), None);
        assert_eq!(parse_audio("Could not reach the daemon."), None);
    }
}
