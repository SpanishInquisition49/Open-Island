//! Part of `Island` (see `mod.rs`): what happens is announced by a card that drops under the pill. The pill shows how
//! things are; a timer running out, a coming event, rain, a device, the power, the end of a long job, a computed value
//! or a copied link are events, and each is a card of the island's own, stacked with the others.

use super::*;
use island_core::clipact;
use island_core::notify::Kind;

/// Ids of the cards that take the place of an older one of the same sort (a second plug-in replaces the first).
const EVENT_ID: u32 = OWN_ID_BASE + 900_000;
const POWER_ID: u32 = EVENT_ID + 1;
const DEVICE_ID: u32 = EVENT_ID + 2;
const RAIN_ID: u32 = EVENT_ID + 3;
const LINK_ID: u32 = EVENT_ID + 4;

/// The actions of the island's cards that a click on the card itself carries out too.
const BODY_ACTIONS: [&str; 3] = ["open-island:copy:", "open-island:open:", "open-island:file:"];

fn action(key: String, label: &str) -> NotifAction {
    NotifAction { key, label: label.into() }
}

impl Island {
    fn next_card_id(&mut self) -> u32 {
        self.next_own_id += 1;
        self.next_own_id
    }

    fn announce(&mut self, card: Incoming) {
        self.push(self.now, Input::Notification(Incoming { app: "open-island".into(), ..card }));
    }

    /// A countdown or a phase ran out. `again` is the name of a countdown that can be given one more minute.
    pub(super) fn timer_card(&mut self, summary: &str, body: String, again: Option<String>) {
        let id = self.next_card_id();
        let actions = again.map(|label| vec![action(format!("open-island:timer:{label}"), "+1 min")]).unwrap_or_default();
        self.announce(Incoming { id, summary: summary.into(), body, icon: Some("bell-ringing".into()), tint: Some(Kind::Warn.color()), actions, expire_ms: Some(10_000), ..Default::default() });
    }

    pub(super) fn power_event_card(&mut self, ev: PowerEvent) {
        let percent = self.data.battery.map(|b| b.percent.round().clamp(0.0, 100.0) as u8);
        let (summary, icon, tint) = match ev {
            PowerEvent::Plugged => ("Charging", "battery-charging", Kind::Ok.color()),
            PowerEvent::Unplugged => ("On battery", "battery", (255, 255, 255)),
            PowerEvent::Low => ("Low battery", "battery-1", Kind::Fail.color()),
        };
        let body = percent.map_or(String::new(), |p| format!("{p}%"));
        self.announce(Incoming { id: POWER_ID, summary: summary.into(), body, icon: Some(icon.into()), tint: Some(tint), progress: percent, expire_ms: Some(if ev == PowerEvent::Low { 8000 } else { 3500 }), ..Default::default() });
    }

    pub(super) fn device_card(&mut self, d: &Device, low: bool) {
        let percent = d.battery.map(|b| b.min(100));
        let state = if low { "Low battery" } else { "Connected" };
        let body = percent.map_or(state.to_string(), |p| format!("{state} · {p}%"));
        let tint = if low || percent.is_some_and(|p| p <= 20) { Kind::Fail.color() } else { Kind::Info.color() };
        self.announce(Incoming { id: DEVICE_ID, summary: d.name.clone(), body, icon: Some(d.kind.icon().into()), tint: Some(tint), progress: percent, expire_ms: Some(if low { 8000 } else { 3500 }), ..Default::default() });
    }

    pub(super) fn rain_card(&mut self, minutes: i32) {
        self.announce(Incoming { id: RAIN_ID, summary: format!("Rain in about {minutes} min"), icon: Some("umbrella".into()), tint: Some(Kind::Info.color()), expire_ms: Some(8000), ..Default::default() });
    }

    /// An event of the calendar is about to start.
    pub(super) fn meeting_card(&mut self, e: &calendar::Event) {
        let id = self.next_card_id();
        let place = e.location.trim();
        let when = self.until_text(e);
        let body = if place.is_empty() || place.starts_with("http") { when } else { format!("{when} · {place}") };
        let actions = e.url.as_deref().filter(|u| !u.is_empty()).map(|u| vec![action(format!("open-island:join:{u}"), "Join")]).unwrap_or_default();
        self.announce(Incoming { id, summary: e.title.clone(), body, icon: Some("calendar-event".into()), tint: Some(Kind::Info.color()), actions, expire_ms: Some(10_000), ..Default::default() });
    }

    /// The card for a long job that has just ended, when one has: its bubble goes, the card says how it went.
    pub(super) fn announce_finished(&mut self) {
        while let Some(op) = self.ops.take_finished() {
            let failed = op.state == progress::State::Fail;
            let summary = if op.label.is_empty() { if failed { "Failed" } else { "Done" }.to_string() } else { op.label };
            let mut actions = vec![];
            if let Some(file) = op.file.filter(|_| !failed) {
                if let Some(folder) = std::path::Path::new(&file).parent().and_then(|p| p.to_str()).filter(|p| !p.is_empty()) {
                    actions.push(action(format!("open-island:file:{file}"), "Open"));
                    actions.push(action(format!("open-island:file:{folder}"), "Show in folder"));
                }
            }
            let ms = if failed || !actions.is_empty() { 10_000 } else { 5000 };
            let id = self.next_card_id();
            self.announce(Incoming { id, summary, body: op.detail, kind: Some(if failed { Kind::Fail } else { Kind::Ok }), tint: op.tint.filter(|_| !failed), icon: op.icon, actions, expire_ms: Some(ms), ..Default::default() });
        }
    }

    /// What the command bar worked out: already on the clipboard, and a click copies it again.
    pub(super) fn value_card(&mut self, shown: &str, value: &str) {
        let id = self.next_card_id();
        self.announce(Incoming { id, summary: shown.into(), body: "Copied · click to copy again".into(), icon: Some("clipboard-text".into()), tint: Some(Kind::Info.color()), actions: vec![action(format!("open-island:copy:{value}"), "Copy")], expire_ms: Some(8000), ..Default::default() });
    }

    /// A copied link, address or e-mail offers to open it. Not while quiet (what was copied is on the screen), nor while
    /// the clipboard page is open (the copy was made there).
    pub(super) fn link_card(&mut self, copied: &str) {
        if !self.cfg.link_cards || self.quiet() || self.page == Some(Page::Clip) {
            return;
        }
        let Some(act) = clipact::classify(copied) else { return };
        let (summary, icon, button) = match act.kind {
            clipact::Kind::Link => ("Link copied", "external-link", "Open"),
            clipact::Kind::Email => ("Address copied", "mail", "Write"),
            clipact::Kind::Place => ("Place copied", "map-pin", "Map"),
        };
        let one_line: String = copied.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(90).collect();
        self.announce(Incoming { id: LINK_ID, summary: summary.into(), body: one_line, icon: Some(icon.into()), tint: Some(Kind::Info.color()), actions: vec![action(format!("open-island:open:{}", act.target), button)], expire_ms: Some(6000), ..Default::default() });
    }

    /// What a click on one of these cards does besides closing it, when its first button is one a click may press.
    pub(super) fn body_action(n: &Incoming) -> Option<String> {
        let key = &n.actions.first()?.key;
        (is_own(n) && BODY_ACTIONS.iter().any(|a| key.starts_with(a))).then(|| key.clone())
    }

    /// The buttons of these cards. Returns false for a key that is not one of theirs.
    pub(super) fn event_action(&mut self, key: &str) -> bool {
        let Some(rest) = key.strip_prefix("open-island:") else { return false };
        let Some((verb, what)) = rest.split_once(':') else { return false };
        match verb {
            "timer" => self.start_named_timer(60, what.to_string()),
            "join" | "open" => self.pending.push(Command::OpenUrl(what.to_string())),
            "file" => self.pending.push(Command::OpenFile(what.to_string())),
            "copy" => self.pending.push(Command::RestoreClipboard(what.to_string())),
            _ => return false,
        }
        true
    }
}
