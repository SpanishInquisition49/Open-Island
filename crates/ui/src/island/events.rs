//! Part of `Island` (see `mod.rs`): what happens is announced by a card that drops under the pill. The pill shows how
//! things are; a timer running out, a coming event, rain, a device, the power, the end of a long job, a computed value
//! or a copied link are events, and each is a card of the island's own, stacked with the others.

use super::*;
use island_core::clipact;
use island_core::notify::Kind;
use island_core::system;

/// A battery under these is said in orange, and in red.
const RING_LOW: u8 = 40;
const RING_EMPTY: u8 = 20;

/// Ids of the cards that take the place of an older one of the same sort (a second plug-in replaces the first).
pub(super) const EVENT_ID: u32 = OWN_ID_BASE + 900_000;
const POWER_ID: u32 = EVENT_ID + 1;
/// How long after a battery card appears its estimate is taken.
const ETA_AFTER_MS: i64 = 2500;
const DEVICE_ID: u32 = EVENT_ID + 2;
const RAIN_ID: u32 = EVENT_ID + 3;
const LINK_ID: u32 = EVENT_ID + 4;
const END_ID: u32 = EVENT_ID + 5;

/// How long before the end of an event it is said, and how long an event must be, and at most, to be worth it: a lecture,
/// a shift, a meeting; not a reminder entered as a quarter of an hour, nor something that lasts all day.
const END_LEAD_MS: i64 = 5 * 60_000;
const END_MIN_MS: i64 = 30 * 60_000;
const END_MAX_MS: i64 = 5 * 3_600_000;

/// The actions of the island's cards that a click on the card itself carries out too.
const BODY_ACTIONS: [&str; 3] = ["open-island:copy:", "open-island:open:", "open-island:file:"];

pub(super) fn action(key: String, label: &str) -> NotifAction {
    NotifAction { key, label: label.into() }
}

impl Island {
    fn next_card_id(&mut self) -> u32 {
        self.next_own_id += 1;
        self.next_own_id
    }

    pub(super) fn announce(&mut self, card: Incoming) {
        self.push(self.now, Input::Notification(Incoming { app: "open-island".into(), ..card }));
    }

    /// Five minutes before something in the calendar ends, once: what comes next and where, or until when one is free.
    /// Held, like the other heads-ups, while a call or a shared screen must not be interrupted.
    pub(super) fn check_event_end(&mut self, now: i64) {
        if !self.cfg.end_alerts || self.quiet_blocks_headsup() {
            return;
        }
        let ending = self
            .data
            .events
            .iter()
            .filter(|e| !e.all_day && e.start_ms <= now && now < e.end_ms && e.end_ms - now <= END_LEAD_MS)
            .filter(|e| (END_MIN_MS..=END_MAX_MS).contains(&(e.end_ms - e.start_ms)) && !self.ends_told.contains(&e.end_ms))
            .min_by_key(|e| e.end_ms)
            .map(|e| (e.title.clone(), e.end_ms));
        let Some((title, end)) = ending else { return };
        self.ends_told.push(end);
        let extra = self.ends_told.len().saturating_sub(20);
        self.ends_told.drain(..extra);
        let minutes = ((end - now) as f64 / 60_000.0).ceil() as i64;
        let body = self.after(end);
        self.announce(Incoming { id: END_ID, summary: format!("{title} ends in {minutes} min"), body, icon: Some("clock".into()), expire_ms: Some(15_000), ..Default::default() });
    }

    /// What comes after the moment `end`, today.
    fn after(&self, end: i64) -> String {
        let off = self.data.utc_offset_secs;
        let midnight = self.day_start_ms(self.day_number(end) + 1);
        let next = self.data.events.iter().filter(|e| !e.all_day && e.start_ms >= end && e.start_ms < midnight).min_by_key(|e| e.start_ms);
        let Some(e) = next else { return "Nothing else today".into() };
        let place = island_core::timeline::place_of(&e.location);
        let place = if place.is_empty() { place } else { format!(" · {place}") };
        let at = crate::clock::civil(e.start_ms, off).hhmm();
        if e.start_ms - end >= 15 * 60_000 {
            format!("Free until {at}, then {}{place}", e.title)
        } else {
            format!("Next: {} at {at}{place}", e.title)
        }
    }

    /// A countdown or a phase ran out. `again` is the name of a countdown that can be given one more minute.
    pub(super) fn timer_card(&mut self, summary: &str, body: String, again: Option<String>) -> u32 {
        let id = self.next_card_id();
        let actions = again.map(|label| vec![action(format!("open-island:timer:{label}"), "+1 min")]).unwrap_or_default();
        self.announce(Incoming { id, summary: summary.into(), body, icon: Some("bell-ringing".into()), tint: Some(Kind::Warn.color()), actions, expire_ms: Some(10_000), ..Default::default() });
        id
    }

    pub(super) fn power_event_card(&mut self, ev: PowerEvent) {
        let percent = self.data.battery.map(|b| b.percent.round().clamp(0.0, 100.0) as u8);
        let (summary, icon, tint) = match ev {
            PowerEvent::Plugged => ("Charging", "battery-charging", Kind::Ok.color()),
            PowerEvent::Unplugged => ("On battery", "battery", (255, 255, 255)),
            PowerEvent::Low => ("Low battery", "battery-1", Kind::Fail.color()),
        };
        let flow = if ev == PowerEvent::Plugged { Flow::Rising } else { Flow::Falling };
        let body = self.data.battery.map_or(String::new(), |b| system::battery_line(b.percent, b.charging, b.on_ac, None));
        let ms = if ev == PowerEvent::Low { 8000 } else { 6000 };
        // the time to full or to empty follows once the reading has settled
        self.fx.awaiting_eta = Some((self.now + ETA_AFTER_MS, self.now + ms));
        self.announce(Incoming { id: POWER_ID, summary: summary.into(), body, icon: Some(icon.into()), tint: Some(tint), ring: percent, liquid: percent.map(|_| flow), expire_ms: Some(ms), ..Default::default() });
    }

    /// Puts the time to full or to empty under the battery card that is up, when it is known.
    pub(super) fn late_eta(&mut self) {
        let Some((from, until)) = self.fx.awaiting_eta else { return };
        if self.now >= until {
            self.fx.awaiting_eta = None;
            return;
        }
        let Some(b) = self.data.battery else { return };
        if self.now < from || self.data.battery_eta.is_none_or(|(_, up)| up != b.charging) {
            return;
        }
        let line = system::battery_line(b.percent, b.charging, b.on_ac, self.data.battery_eta);
        self.fx.awaiting_eta = None;
        // changed where it is: the card does not start over
        let drawn = self.notif.iter_mut().chain(self.below.iter_mut().map(|b| &mut b.n)).chain(self.cards.stack.iter_mut().map(|c| &mut c.n));
        drawn.filter(|n| n.id == POWER_ID).for_each(|n| n.body = line.clone());
        self.queue.iter_mut().filter(|n| n.id == POWER_ID).for_each(|n| n.body = line.clone());
    }

    pub(super) fn device_card(&mut self, d: &Device, low: bool) {
        let percent = d.battery.map(|b| b.min(100));
        let state = if low { "Low battery" } else { "Connected" };
        let body = state.to_string();
        // the ring round the icon and its number say the battery: the line under the name only says what happened
        let tint = match percent {
            Some(p) if low || p <= RING_EMPTY => Kind::Fail.color(),
            Some(p) if p <= RING_LOW => Kind::Warn.color(),
            Some(_) => Kind::Ok.color(),
            None if low => Kind::Fail.color(),
            None => Kind::Info.color(),
        };
        let ms = if low { 8000 } else if percent.is_some() { 4500 } else { 3500 };
        self.announce(Incoming { id: DEVICE_ID, summary: d.name.clone(), body, icon: Some(d.kind.icon().into()), tint: Some(tint), ring: percent, expire_ms: Some(ms), ..Default::default() });
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

    /// The battery a device did not say when it connected has come: its card again, now with the ring.
    pub(super) fn late_battery(&mut self) {
        let Some((id, until)) = self.fx.awaiting_battery.clone() else { return };
        if let Some(d) = self.data.devices.iter().find(|d| d.id == id && d.battery.is_some()).cloned() {
            self.fx.awaiting_battery = None;
            self.device_card(&d, false);
        } else if self.now >= until {
            self.fx.awaiting_battery = None;
        }
    }

    /// The card for a long job that has just ended, when one has: its bubble goes, the card says how it went.
    pub(super) fn announce_finished(&mut self) {
        while let Some(op) = self.ops.take_finished() {
            let failed = op.state == progress::State::Fail;
            let sent = !failed && op.id == "send" && op.label == "Sent";
            let source = liquid::Source::Job(op.id.clone());
            let colour = if failed { Kind::Fail.color() } else { op.tint.unwrap_or(Kind::Ok.color()) };
            let summary = if op.label.is_empty() { if failed { "Failed" } else { "Done" }.to_string() } else { op.label };
            let mut actions = vec![];
            if let Some(file) = op.file.filter(|_| !failed) {
                // a path ending in a slash is a folder: several files arrived in it
                if file.ends_with('/') {
                    actions.push(action(format!("open-island:file:{file}"), "Show in folder"));
                } else if let Some(folder) = std::path::Path::new(&file).parent().and_then(|p| p.to_str()).filter(|p| !p.is_empty()) {
                    actions.push(action(format!("open-island:file:{file}"), "Open"));
                    actions.push(action(format!("open-island:file:{folder}"), "Show in folder"));
                }
            }
            let ms = if failed || !actions.is_empty() { 10_000 } else { 5000 };
            let id = self.next_card_id();
            self.announce(Incoming { id, summary, body: op.detail, kind: Some(if failed { Kind::Fail } else { Kind::Ok }), tint: op.tint.filter(|_| !failed), icon: op.icon, actions, expire_ms: Some(ms), ..Default::default() });
            self.hand_drop(&source, id, colour, failed);
            if sent {
                self.shoot();
            }
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

    /// Another device offers files (or a message): the card waits for an answer; the platform layer takes it back
    /// when none comes. It shows during quiet times too, since the person is the one sending, from their other device.
    pub(super) fn offer_card(&mut self, offer: u32, summary: String, body: String, message: bool) {
        let (yes, no) = if message { ("Copy", "Ignore") } else { ("Accept", "Decline") };
        let actions = vec![action(format!("open-island:recv-yes:{offer}"), yes), action(format!("open-island:recv-no:{offer}"), no)];
        self.announce(Incoming { id: offer_card(offer), summary, body, icon: Some(if message { "message-reply" } else { "download" }.into()), tint: Some(Kind::Info.color()), actions, stay: true, full: true, ..Default::default() });
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
            "shot-send" => self.push(self.now, Input::SendFiles(vec![what.to_string()])),
            "shot-trash" => self.pending.push(Command::TrashFile(what.to_string())),
            "recv-yes" | "recv-no" => {
                if let Ok(id) = what.parse() {
                    self.pending.push(Command::AnswerOffer { id, accept: verb == "recv-yes" });
                }
            }
            "copy" => self.pending.push(Command::RestoreClipboard(what.to_string())),
            "screens" => self.screens_action(what),
            _ => return false,
        }
        true
    }
}
