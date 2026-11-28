//! Part of `Island` (see `mod.rs`): on the day of a birthday in the calendars, a card with the names and a burst of
//! confetti. Whose it is and where the confetti flies are in `island_core::birthday`.

use super::*;
use island_core::birthday;
use island_core::timeline::Kind;

/// The hours between which the card comes (minutes from midnight).
const FROM_MIN: i64 = 8 * 60;
const UNTIL_MIN: i64 = 23 * 60 + 30;
/// How often the day is looked through.
const LOOK_EVERY_MS: i64 = 10_000;
/// The card has dropped out this long after it is there: the confetti bursts then.
pub(crate) const BURST_AFTER_MS: i64 = 350;

impl Island {
    /// Today's birthdays: the names in the titles of the all-day events, each once.
    pub(crate) fn birthdays_today(&self) -> Vec<String> {
        let mut names: Vec<String> = vec![];
        for item in self.today_left().into_iter().filter(|i| i.all_day && i.kind == Kind::Event) {
            let Some(name) = birthday::who(&item.title) else { continue };
            // one that names nobody is said by its title
            let name = if name.is_empty() { item.title.trim().to_string() } else { name };
            if !names.iter().any(|n| n.to_lowercase() == name.to_lowercase()) {
                names.push(name);
            }
        }
        names
    }

    /// Once a day, at the first moment from eight when a calendar has a birthday and the island is not being quiet.
    pub(super) fn check_birthdays(&mut self, now: i64) {
        if !self.cfg.birthday_cards || (now - self.birthday_looked).abs() < LOOK_EVERY_MS {
            return;
        }
        self.birthday_looked = now;
        let (day, local_min) = (self.day_number(now), ((now / 1000 + self.data.utc_offset_secs as i64).rem_euclid(86_400)) / 60);
        // a card held back while quiet would come out later without its confetti: it waits
        if self.birthday_day == day || !(FROM_MIN..UNTIL_MIN).contains(&local_min) || self.quiet() {
            return;
        }
        let names = self.birthdays_today();
        if !names.is_empty() {
            self.birthday_day = day;
            self.celebrate(&names);
        }
    }

    /// The card for these names, with its confetti.
    pub(super) fn celebrate(&mut self, names: &[String]) {
        let summary = match names.len() {
            0 => return,
            1 => "Birthday today".to_string(),
            n => format!("{n} birthdays today"),
        };
        self.next_own_id += 1;
        let id = self.next_own_id;
        let card = Incoming { id, app: "open-island".into(), summary, body: names.join("\n"), icon: Some("calendar".into()), expire_ms: Some(30_000), full: true, ..Default::default() };
        self.push(self.now, Input::Notification(card));
        self.fx.confetti = Some(effects::Confetti { card: id, born: None, asked: self.now });
    }

    /// The last day the birthday card came (kept across restarts by the platform layer).
    pub fn birthday_day(&self) -> i64 {
        self.birthday_day
    }

    pub fn restore_birthday_day(&mut self, day: i64) {
        self.birthday_day = day;
    }
}
