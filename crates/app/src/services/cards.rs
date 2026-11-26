//! The cards a script asks for (`open-island ok ...`, `run`, the agent hook), after drip's command line: what each one
//! says, the name that lets a newer card take an older one's place (`--id`), and who waits for an answer.
//!
//! The island itself only draws them. A click on a button comes back as a command, which ends here: it answers the
//! script that waits on the control socket, or opens what the card offered.

use crate::config::CardPreset;
use island_core::notify::Kind;
use island_core::progress::parse_color;
use island_ui::island::SCRIPT_ID_BASE;
use island_ui::model::{Command, Incoming, NotifAction};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Mutex;

/// The longest a script is kept waiting: a little over the ten minutes a card can stay.
pub const WAIT_LIMIT: std::time::Duration = std::time::Duration::from_secs(620);
/// The longest button label, in characters (drip's).
const LABEL_MAX: usize = 24;

/// What became of a question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Yes,
    No,
}

#[derive(Default)]
struct Entry {
    open: Option<String>,
    waiter: Option<SyncSender<Answer>>,
}

/// The cards that were asked for: names to numbers, and what is pending on each.
#[derive(Default)]
pub struct Registry {
    names: HashMap<String, u32>,
    entries: HashMap<u32, Entry>,
    next: u32,
}

/// A new card and, when it asks a question, where its answer will come.
pub struct Posted {
    pub card: Incoming,
    pub answer: Option<Receiver<Answer>>,
}

impl Registry {
    /// The card that `spec` (the JSON the command line sends) describes, or None when it says nothing.
    pub fn post(&mut self, spec: &Value, presets: &[(String, CardPreset)]) -> Option<Posted> {
        let text = |k: &str, max: usize| spec.get(k).and_then(Value::as_str).map(|t| t.trim().chars().take(max).collect::<String>()).filter(|t| !t.is_empty());
        let preset = text("preset", 40).and_then(|name| presets.iter().find(|(n, _)| *n == name).map(|(_, p)| p.clone())).unwrap_or_default();
        let title = text("title", 300)?;
        let kind = text("kind", 20).and_then(|k| Kind::parse(&k)).or(preset.kind).unwrap_or(Kind::Info);
        let seconds = spec.get("seconds").and_then(Value::as_f64).or(preset.seconds).filter(|s| s.is_finite() && *s > 0.0);
        let yes = text("yes", LABEL_MAX);
        let no = yes.as_ref().and(text("no", LABEL_MAX));
        // a question has no button that opens something: its buttons are its answers
        let open = if yes.is_none() { text("open", 2000) } else { None };
        // a card that stays, or a question, given a time counts it down instead of waiting
        let stay = (yes.is_some() || spec.get("stay").and_then(Value::as_bool).unwrap_or(false)) && seconds.is_none();

        let mut actions = vec![];
        if let Some(label) = no {
            actions.push(NotifAction { key: "no".into(), label });
        }
        if let Some(label) = yes.clone() {
            actions.push(NotifAction { key: "yes".into(), label });
        }
        if open.is_some() {
            actions.push(NotifAction { key: "open".into(), label: text("button", LABEL_MAX).unwrap_or_else(|| "Open".into()) });
        }

        let name = text("id", 200);
        let id = match name.as_ref().and_then(|n| self.names.get(n)) {
            Some(id) => *id,
            None => {
                self.next = (self.next + 1) % 1_000_000;
                SCRIPT_ID_BASE + self.next
            }
        };
        if let Some(name) = name {
            self.names.insert(name, id);
        }
        let (waiter, answer) = match yes {
            Some(_) => {
                let (tx, rx) = sync_channel(1);
                (Some(tx), Some(rx))
            }
            None => (None, None),
        };
        // (an older card of this name that was waiting hears that nobody answered: its sender is dropped here)
        self.entries.insert(id, Entry { open, waiter });
        let card = Incoming {
            id,
            app: "open-island".into(),
            summary: title,
            body: text("detail", 600).unwrap_or_default(),
            kind: Some(kind),
            tint: text("color", 20).and_then(|c| parse_color(&c)).or(preset.color),
            icon: text("icon", 40).or(preset.icon).filter(|i| island_ui::is_known_icon(i)),
            stay,
            expire_ms: seconds.map(|s| (s * 1000.0) as i64),
            actions,
            desktop_entry: text("app", 100).unwrap_or_default(),
            ..Default::default()
        };
        Some(Posted { card, answer })
    }

    /// A button of the card `id` was pressed: whoever waits hears it. Returns what there is to open.
    pub fn pressed(&mut self, id: u32, key: &str) -> Option<String> {
        let entry = self.entries.get_mut(&id)?;
        let answer = match key {
            "yes" => Answer::Yes,
            "no" => Answer::No,
            "open" => return entry.open.clone(),
            _ => return None,
        };
        if let Some(tx) = entry.waiter.take() {
            let _ = tx.send(answer);
        }
        None
    }

    /// The card is gone: a script still waiting on it hears nothing more.
    pub fn closed(&mut self, id: u32) {
        self.entries.remove(&id);
    }

    /// The number of the card named `name`, which is to be taken away.
    pub fn take(&mut self, name: &str) -> Option<u32> {
        let id = *self.names.get(name)?;
        self.entries.remove(&id).map(|_| id)
    }
}

static REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);

fn registry<T>(f: impl FnOnce(&mut Registry) -> T) -> T {
    let mut guard = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Registry::default))
}

/// A card from the JSON of a `card` message, with the sound it asks for.
pub fn post(json: &str) -> Option<(Posted, Option<String>)> {
    let spec: Value = serde_json::from_str(json).ok()?;
    let presets = crate::config::load().card_presets;
    let sound = spec.get("sound").and_then(Value::as_str).map(crate::config::expand_home).or_else(|| {
        let name = spec.get("preset").and_then(Value::as_str)?;
        presets.iter().find(|(n, _)| n == name)?.1.sound.clone()
    });
    Some((registry(|r| r.post(&spec, &presets))?, sound))
}

pub fn take(name: &str) -> Option<u32> {
    registry(|r| r.take(name))
}

/// What the island does about a script's card ends here: true when the command was only for the card and goes no
/// further.
pub fn on_command(c: &Command) -> bool {
    let ours = |id: u32| (SCRIPT_ID_BASE..SCRIPT_ID_BASE + 1_000_000).contains(&id);
    match c {
        Command::InvokeAction { id, key } if ours(*id) => {
            if let Some(target) = registry(|r| r.pressed(*id, key)) {
                open(&target);
            }
            true
        }
        Command::NotificationClosed { id, .. } | Command::CloseNotification(id) if ours(*id) => {
            registry(|r| r.closed(*id));
            true
        }
        _ => false,
    }
}

/// Opens an address, a file or an app with whatever the system opens it with.
fn open(target: &str) {
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let program = "xdg-open";
    // a leading dash would be read as an option
    let target = if target.starts_with('-') { format!("./{target}") } else { target.to_string() };
    match std::process::Command::new(program).arg(target).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
        Ok(mut child) => drop(std::thread::spawn(move || child.wait())),
        Err(e) => log::warn!("cannot open: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use island_ui::model::CloseReason;
    use serde_json::json;

    fn post_in(r: &mut Registry, v: Value) -> Posted {
        r.post(&v, &[("deploy".into(), CardPreset { icon: Some("bolt".into()), color: Some((1, 2, 3)), seconds: Some(8.0), ..Default::default() })]).expect("a card")
    }

    #[test]
    fn a_card_says_what_was_asked() {
        let mut r = Registry::default();
        let p = post_in(&mut r, json!({"kind": "fail", "title": " Deploy failed ", "detail": "exit 1", "seconds": 8}));
        assert_eq!((p.card.summary.as_str(), p.card.body.as_str(), p.card.kind, p.card.stay, p.card.expire_ms), ("Deploy failed", "exit 1", Some(Kind::Fail), false, Some(8000)));
        assert!(p.answer.is_none() && p.card.actions.is_empty());
        assert!(r.post(&json!({"kind": "ok"}), &[]).is_none() && r.post(&json!({"title": "  "}), &[]).is_none(), "a card has a title");
        let plain = post_in(&mut r, json!({"title": "x"}));
        assert_eq!(plain.card.kind, Some(Kind::Info));
        assert_ne!(plain.card.id, p.card.id);
    }

    #[test]
    fn a_question_waits_for_one_of_its_buttons() {
        let mut r = Registry::default();
        let p = post_in(&mut r, json!({"title": "Merge?", "yes": "Merge", "no": "Later", "open": "https://x"}));
        let keys: Vec<&str> = p.card.actions.iter().map(|a| a.key.as_str()).collect();
        assert_eq!(keys, ["no", "yes"], "no on the left, yes on the right, and nothing to open");
        assert!(p.card.stay);
        let rx = p.answer.unwrap();
        assert!(rx.try_recv().is_err(), "not answered yet");
        r.pressed(p.card.id, "yes");
        r.closed(p.card.id);
        assert_eq!(rx.try_recv(), Ok(Answer::Yes));

        let q = post_in(&mut r, json!({"title": "Again?", "yes": "Yes", "seconds": 5}));
        assert!(!q.card.stay, "given a time, it counts it down");
        let rx = q.answer.unwrap();
        r.closed(q.card.id);
        assert_eq!(rx.recv(), Err(std::sync::mpsc::RecvError), "gone unanswered");
        assert!(post_in(&mut r, json!({"title": "t", "no": "No"})).card.actions.is_empty(), "no 'no' without a 'yes'");
    }

    #[test]
    fn a_name_keeps_the_place_of_the_card_before() {
        let mut r = Registry::default();
        let first = post_in(&mut r, json!({"title": "Deploying", "id": "deploy", "yes": "Stop"}));
        let rx = first.answer.unwrap();
        let second = post_in(&mut r, json!({"title": "Deployed", "id": "deploy", "kind": "ok"}));
        assert_eq!(second.card.id, first.card.id);
        assert!(rx.recv().is_err(), "the replaced question is not answered");
        assert_eq!(r.take("deploy"), Some(second.card.id));
        assert_eq!(r.take("deploy"), None, "only once");
        assert_eq!(r.take("nothing"), None);
    }

    #[test]
    fn a_button_opens_what_the_card_offers_and_presets_fill_the_gaps() {
        let mut r = Registry::default();
        let p = post_in(&mut r, json!({"title": "Report ready", "open": "./report.html", "button": "View report"}));
        assert_eq!(p.card.actions[0].label, "View report");
        assert_eq!(r.pressed(p.card.id, "open").as_deref(), Some("./report.html"));
        let d = post_in(&mut r, json!({"title": "Shipped", "preset": "deploy"}));
        assert_eq!((d.card.icon.as_deref(), d.card.tint, d.card.expire_ms), (Some("bolt"), Some((1, 2, 3)), Some(8000)));
        let own = post_in(&mut r, json!({"title": "Shipped", "preset": "deploy", "color": "#0a84ff", "icon": "no-such-icon"}));
        assert_eq!(own.card.tint, Some((0x0a, 0x84, 0xff)));
        assert_eq!(own.card.icon, None, "an icon the island does not have is no icon");
    }

    #[test]
    fn only_a_scripts_cards_end_here() {
        assert!(!on_command(&Command::NotificationClosed { id: 7, reason: CloseReason::Expired }));
        assert!(on_command(&Command::NotificationClosed { id: SCRIPT_ID_BASE + 999_999, reason: CloseReason::Expired }));
    }
}
