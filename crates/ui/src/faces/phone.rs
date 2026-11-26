//! The Phone page: the iPhone's battery and what the link carries, its calls (answer, hang up), its message
//! conversations, a reply, and a new message to somebody found among the contacts.
//! The data comes from the `tether` command (see `services/phone.rs`).

use super::*;

const THREAD_ROWS: usize = 4;
const MSG_ROWS: usize = 4;
const CONTACT_ROWS: usize = 5;
const CALL_ROWS: usize = 2;

/// "2026-10-26 09:23" -> "26/10 09:23"
fn short_when(when: &str) -> String {
    match (when.get(8..10), when.get(5..7), when.get(11..16)) {
        (Some(d), Some(m), Some(t)) => format!("{d}/{m} {t}"),
        _ => when.to_string(),
    }
}

impl Island {
    fn phone_present(&self) -> bool {
        self.data.phone.as_ref().is_some_and(|p| p.link.present)
    }

    fn phone_call_rows(&self) -> usize {
        self.data.phone.as_ref().map_or(0, |p| p.calls.len().min(CALL_ROWS))
    }

    pub(crate) fn phone_height(&self) -> f64 {
        let head = 14.0 + 34.0;
        let Some(ph) = self.data.phone.as_ref().filter(|_| self.phone_present()) else { return head + 70.0 };
        if self.phone_search {
            let n = ph.contacts.as_ref().map_or(1, |c| c.len().clamp(1, CONTACT_ROWS));
            return head + 6.0 + n as f64 * 40.0 + 10.0;
        }
        match &self.phone_open {
            Some(_) => head + 4.0 + MSG_ROWS as f64 * 32.0 + 8.0 + 34.0 + 10.0,
            None => head + self.phone_call_rows() as f64 * 42.0 + 38.0 + 4.0 + ph.threads.len().clamp(1, THREAD_ROWS) as f64 * 46.0 + 22.0 + if ph.link.calls { 0.0 } else { 30.0 } + if ph.audio_here.is_some() { 30.0 } else { 0.0 } + 6.0,
        }
    }

    /// A round button with an icon.
    fn call_button(&self, p: P, cx: f64, cy: f64, icon: &str, colour: Color, action: Action) {
        let r = rect(cx - 15.0, cy - 15.0, 30.0, 30.0);
        p.rrect(r, 15.0, if p.hovered(r) { with_alpha(colour, 0.85) } else { colour });
        p.icon(icon, cx, cy, 15.0, theme::ON_ACCENT);
        p.hit(r, action);
    }

    pub(super) fn face_phone(&self, p: P, w: f64, _h: f64) {
        let Some(ph) = self.data.phone.as_ref().filter(|_| self.phone_present()) else {
            self.page_header(p, w, "Phone");
            p.text("No iPhone connected", w / 2.0, 14.0 + 34.0 + 18.0, 12.0, SEMI, theme::FG, Align::Center, (w - 40.0) as f32);
            p.text("Pair it with tether (github.com/zackb/tether), then it shows here", w / 2.0, 14.0 + 34.0 + 40.0, 10.5, REG, theme::DIM, Align::Center, (w - 40.0) as f32);
            return;
        };
        let battery = ph.link.battery.map(|b| format!("{b}%")).unwrap_or_default();
        if self.phone_search {
            return self.phone_contacts_view(p, w, ph);
        }
        match &self.phone_open {
            None => {
                let hy = self.page_header(p, w, "iPhone");
                if !battery.is_empty() {
                    let low = ph.link.battery.is_some_and(|b| b <= 20);
                    p.text(&battery, w - 18.0, hy, 12.0, SEMI, if low { theme::RED } else { theme::DIM }, Align::Right, 60.0);
                }
                let mut y = 14.0 + 34.0;
                // calls first: a ringing phone is what matters
                for (n, c) in ph.calls.iter().take(CALL_ROWS).enumerate() {
                    p.stagger(1 + n);
                    let row = rect(10.0, y + 2.0, w - 20.0, 38.0);
                    let ring = c.is_ringing();
                    p.rrect(row, 13.0, with_alpha(if ring { theme::GREEN } else { theme::ACCENT }, 0.18));
                    p.icon("phone", 30.0, row.center().y, 15.0, if ring { theme::GREEN } else { theme::ACCENT });
                    let who = if c.name.is_empty() { c.number.clone() } else { c.name.clone() };
                    p.text(&who, 48.0, row.center().y - 7.0, 12.0, SEMI, theme::FG, Align::Left, (w - 48.0 - 100.0) as f32);
                    p.text(&c.state, 48.0, row.center().y + 8.0, 10.0, REG, theme::DIM, Align::Left, 120.0);
                    if ring {
                        self.call_button(p, row.x1 - 62.0, row.center().y, "phone", theme::GREEN, Action::PhoneCallDo(PhoneCall::Answer));
                    }
                    self.call_button(p, row.x1 - 24.0, row.center().y, "phone-off", theme::RED, Action::PhoneCallDo(PhoneCall::Hangup));
                    y += 42.0;
                }
                // a new message: a name from the contacts
                let bar = rect(14.0, y + 4.0, w - 28.0, 30.0);
                match self.edit.as_ref().filter(|e| e.field == Field::PhoneSearch) {
                    Some(e) => self.edit_row(p, bar, e, "Name of a contact, Enter looks"),
                    None => {
                        p.rrect(bar, 15.0, if p.hovered(bar) { theme::HOT_CHIP } else { theme::CHIP });
                        p.icon("search", bar.x0 + 18.0, bar.center().y, 13.0, theme::ACCENT);
                        p.text("New message…", bar.x0 + 34.0, bar.center().y, 12.0, REG, theme::DIM, Align::Left, 200.0);
                        p.hit(bar, Action::EditField(Field::PhoneSearch));
                    }
                }
                y += 38.0 + 4.0;
                if ph.threads.is_empty() {
                    p.stagger(3);
                    let t = if ph.link.messages { "No conversations yet" } else { "Messages are off: turn on Show Message Notifications on the iPhone" };
                    p.text(t, w / 2.0, y + 22.0, 12.0, REG, theme::DIM, Align::Center, (w - 40.0) as f32);
                    y += 46.0;
                }
                for (n, t) in ph.threads.iter().take(THREAD_ROWS).enumerate() {
                    p.stagger(3 + n);
                    let row = rect(8.0, y, w - 16.0, 44.0);
                    if p.hovered(row) {
                        p.rrect(row, 10.0, theme::CHIP);
                    }
                    p.icon("message-reply", 28.0, y + 22.0, 15.0, theme::ACCENT);
                    p.text(&t.name, 48.0, y + 15.0, 12.0, SEMI, theme::FG, Align::Left, (w - 48.0 - 100.0) as f32);
                    p.text(&short_when(&t.when), w - 18.0, y + 15.0, 10.0, REG, theme::DIM, Align::Right, 90.0);
                    p.text(&t.preview, 48.0, y + 32.0, 11.0, REG, theme::DIM, Align::Left, (w - 48.0 - 18.0) as f32);
                    p.hit(row, Action::PhoneThread(n));
                    y += 46.0;
                }
                let l = &ph.link;
                let on = |b: bool| if b { "on" } else { "off" };
                p.text(&format!("Messages {} · Contacts {} · Notifications {}", on(l.messages), on(l.contacts), on(l.notifications)), w / 2.0, y + 8.0, 10.0, REG, theme::DIM, Align::Center, (w - 30.0) as f32);
                let mut cy = y + 22.0;
                if let Some(here) = ph.audio_here {
                    let chip = rect(14.0, cy, w - 28.0, 26.0);
                    p.rrect(chip, 13.0, if p.hovered(chip) { theme::HOT_CHIP } else { theme::CHIP });
                    p.icon("music", chip.x0 + 18.0, chip.center().y, 12.0, theme::ACCENT);
                    p.text(if here { "Phone audio plays here · keep it on the phone" } else { "Phone audio stays on the phone · play it here" }, chip.x0 + 32.0, chip.center().y, 11.0, REG, theme::FG, Align::Left, (chip.width() - 44.0) as f32);
                    p.hit(chip, Action::PhoneAudioDo(!here));
                    cy += 30.0;
                }
                if !l.calls {
                    let chip = rect(14.0, cy, w - 28.0, 26.0);
                    p.rrect(chip, 13.0, if p.hovered(chip) { theme::HOT_CHIP } else { theme::CHIP });
                    p.icon("phone", chip.x0 + 18.0, chip.center().y, 12.0, theme::ACCENT);
                    p.text("Calls are off · turn on to answer from here", chip.x0 + 32.0, chip.center().y, 11.0, REG, theme::FG, Align::Left, (chip.width() - 44.0) as f32);
                    p.hit(chip, Action::PhoneCallDo(PhoneCall::Enable(true)));
                }
            }
            Some(key) => {
                let name = ph.threads.iter().find(|t| &t.key == key).map(|t| t.name.clone()).unwrap_or_else(|| key.strip_prefix("tel:").unwrap_or(key).to_string());
                p.stagger(0);
                let cy = 14.0 + 17.0;
                p.icon_button(14.0 + 14.0, cy, 28.0, "arrow-left", 15.0, Action::PhoneList);
                p.text(&name, 14.0 + 28.0 + 8.0, cy, 14.0, SEMI, theme::FG, Align::Left, (w - 120.0) as f32);
                // a call to the number, when call control is on
                if let (Some(number), true) = (key.strip_prefix("tel:"), ph.link.calls) {
                    p.icon_button(w - 28.0, cy, 28.0, "phone", 15.0, Action::PhoneCallDo(PhoneCall::Dial(number.to_string())));
                }
                let mut y = 14.0 + 34.0 + 4.0;
                match ph.messages.as_ref().filter(|(k, _)| k == key) {
                    None => {
                        p.text("Reading the messages…", w / 2.0, y + 50.0, 12.0, REG, theme::DIM, Align::Center, (w - 40.0) as f32);
                    }
                    Some((_, msgs)) => {
                        if msgs.is_empty() {
                            p.text("No messages yet, write the first one", w / 2.0, y + 50.0, 12.0, REG, theme::DIM, Align::Center, (w - 40.0) as f32);
                        }
                        let shown: Vec<_> = msgs.iter().rev().take(MSG_ROWS).rev().collect();
                        y += (MSG_ROWS - shown.len()) as f64 * 32.0;
                        for (n, m) in shown.into_iter().enumerate() {
                            p.stagger(1 + n);
                            let tw = (p.fonts.measure(&m.text, 12.0, REG) as f64 + 22.0).clamp(60.0, w - 90.0);
                            let r = if m.mine { rect(w - 14.0 - tw, y + 2.0, tw, 27.0) } else { rect(14.0, y + 2.0, tw, 27.0) };
                            p.rrect(r, 13.0, if m.mine { theme::ACCENT } else { theme::CHIP });
                            p.text(&m.text, r.x0 + 11.0, r.center().y, 12.0, REG, if m.mine { theme::ON_ACCENT } else { theme::FG }, Align::Left, (tw - 18.0) as f32);
                            y += 32.0;
                        }
                    }
                }
                let reply = rect(14.0, 14.0 + 34.0 + 4.0 + MSG_ROWS as f64 * 32.0 + 8.0, w - 28.0, 30.0);
                match self.edit.as_ref().filter(|e| e.field == Field::PhoneReply) {
                    Some(e) => self.edit_row(p, reply, e, "Write a reply, Enter sends"),
                    None => {
                        p.rrect(reply, 15.0, if p.hovered(reply) { theme::HOT_CHIP } else { theme::CHIP });
                        p.icon("message-reply", reply.x0 + 18.0, reply.center().y, 13.0, theme::ACCENT);
                        p.text("Reply", reply.x0 + 34.0, reply.center().y, 12.0, SEMI, theme::ACCENT, Align::Left, 120.0);
                        p.hit(reply, Action::EditField(Field::PhoneReply));
                    }
                }
            }
        }
    }

    /// The contacts a search found: a click writes to the first number of one.
    fn phone_contacts_view(&self, p: P, w: f64, ph: &Phone) {
        p.stagger(0);
        let cy = 14.0 + 17.0;
        p.icon_button(14.0 + 14.0, cy, 28.0, "arrow-left", 15.0, Action::PhoneList);
        p.text("Write to…", 14.0 + 28.0 + 8.0, cy, 14.0, SEMI, theme::FG, Align::Left, (w - 120.0) as f32);
        let mut y = 14.0 + 34.0 + 6.0;
        match &ph.contacts {
            None => {
                p.text("Looking…", w / 2.0, y + 20.0, 12.0, REG, theme::DIM, Align::Center, (w - 40.0) as f32);
            }
            Some(c) if c.is_empty() => {
                p.text("Nobody found with that name", w / 2.0, y + 20.0, 12.0, REG, theme::DIM, Align::Center, (w - 40.0) as f32);
            }
            Some(list) => {
                for (n, c) in list.iter().take(CONTACT_ROWS).enumerate() {
                    p.stagger(1 + n);
                    let row = rect(8.0, y, w - 16.0, 38.0);
                    if p.hovered(row) {
                        p.rrect(row, 10.0, theme::CHIP);
                    }
                    p.icon("message-reply", 28.0, row.center().y, 15.0, theme::ACCENT);
                    p.text(&c.name, 48.0, row.center().y - 7.0, 12.0, SEMI, theme::FG, Align::Left, (w - 70.0) as f32);
                    p.text(c.numbers.first().map(String::as_str).unwrap_or(""), 48.0, row.center().y + 8.0, 10.5, REG, theme::DIM, Align::Left, (w - 70.0) as f32);
                    p.hit(row, Action::PhoneContact(n));
                    y += 40.0;
                }
            }
        }
    }
}
