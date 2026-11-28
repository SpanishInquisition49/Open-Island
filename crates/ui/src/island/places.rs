//! Part of `Island` (see `mod.rs`): reminders tied to a place. The rules are in `island_core::places`; here are the
//! places the person saved, what the position says, and the card.

use super::*;
use crate::model::PosSource;
use island_core::places::{Waiting, ROUGH_M};

/// A position older than this says nothing about now.
const FRESH_MS: i64 = 15 * 60_000;
/// How often a position is asked while something waits for a place.
const ASK_MS: i64 = 2 * 60_000;
/// The most reminders waiting, and the most places saved.
const MAX: usize = 20;

impl Island {
    /// Where a place is: home from the settings, the others from the saved list.
    fn place_at(&self, name: &str) -> Option<(f64, f64)> {
        let saved = self.places.iter().find(|p| p.0 == name).map(|p| (p.1, p.2));
        saved.or_else(|| if name == "home" { self.data.config.home_pos } else { None })
    }

    /// The position now, when it is recent and found by the machine (not the one typed in the settings).
    fn fix(&self) -> Option<(f64, f64, f64)> {
        let p = self.data.position.as_ref().filter(|p| p.source != PosSource::Settings && self.now - p.at_ms <= FRESH_MS)?;
        Some((p.lat, p.lon, p.accuracy_m))
    }

    pub(super) fn remind_at(&mut self, place: String, leaving: bool, text: String) {
        let Some(at) = self.place_at(&place) else {
            let how = if place == "home" { "Say where home is first: home <address>".to_string() } else { format!("When you are there, save it: place {place}") };
            self.say(&format!("Where is {place}?"), &how);
            return;
        };
        if self.at_places.len() >= MAX {
            self.say("Too many reminders waiting", "Take one off on the Today page first");
            return;
        }
        let mut waiting = Waiting { place, leaving, text: text.chars().take(120).collect(), armed: false };
        // where one is now decides what counts as the next arrival
        let fix = self.fix();
        if let Some((lat, lon, accuracy)) = fix {
            waiting.sees(island_core::map::distance_m((lat, lon), at), accuracy);
        }
        let when = waiting.when();
        self.at_places.push(waiting);
        self.place_asked_at = 0;
        match fix {
            _ if !self.cfg.follow_position => self.say("Reminder kept, position is off", "Switch on Follow my position in the options"),
            Some((_, _, accuracy)) if accuracy > ROUGH_M => self.say("Position too rough for this", &format!("It is sure within {} only: turn on the Wi-Fi position", island_core::map::fmt_distance(accuracy))),
            _ => self.say("Reminder set", &when),
        }
    }

    pub(super) fn set_place(&mut self, name: String, forget: bool) {
        if forget {
            let before = self.places.len();
            self.places.retain(|p| p.0 != name);
            self.at_places.retain(|w| w.place != name || (name == "home" && self.data.config.home_pos.is_some()));
            let said = if self.places.len() < before { "Place forgotten" } else { "No such place" };
            self.say(said, &name);
            return;
        }
        match self.fix() {
            Some((lat, lon, accuracy)) if accuracy <= ROUGH_M => {
                self.places.retain(|p| p.0 != name);
                self.places.push((name.clone(), lat, lon));
                let extra = self.places.len().saturating_sub(MAX);
                self.places.drain(..extra);
                self.say("Place saved", &name);
            }
            Some((_, _, accuracy)) => self.say("Position too rough to save", &format!("It is sure within {} only", island_core::map::fmt_distance(accuracy))),
            None => {
                self.pending.push(Command::RefreshPosition);
                self.say("No position yet", "Asked for one: try again in a moment");
            }
        }
    }

    /// While something waits for a place: keeps the position fresh, and tells what is due.
    pub(super) fn check_places(&mut self, now: i64) {
        if self.at_places.is_empty() {
            return;
        }
        if self.cfg.follow_position && now - self.place_asked_at >= ASK_MS {
            self.place_asked_at = now;
            self.pending.push(Command::RefreshPosition);
        }
        let Some((lat, lon, accuracy)) = self.fix() else { return };
        let places: Vec<Option<(f64, f64)>> = self.at_places.iter().map(|w| self.place_at(&w.place)).collect();
        let mut due = vec![];
        let mut n = 0;
        self.at_places.retain_mut(|w| {
            let at = places[n];
            n += 1;
            let ring = at.is_some_and(|at| w.sees(island_core::map::distance_m((lat, lon), at), accuracy));
            if ring {
                due.push(w.clone());
            }
            !ring
        });
        for w in due {
            let how = match (w.leaving, w.place.as_str()) {
                (false, "home") => "You got home".to_string(),
                (false, place) => format!("You got to {place}"),
                (true, "home") => "You left home".to_string(),
                (true, place) => format!("You left {place}"),
            };
            self.next_own_id += 1;
            let card = Incoming { id: self.next_own_id, app: "Reminder".into(), summary: w.text.clone(), body: how, icon: Some("map-pin".into()), expire_ms: Some(20_000), ..Default::default() };
            self.push(now, Input::Notification(card));
            self.pending.push(Command::PlaySound);
        }
    }

    pub fn places_saved(&self) -> (&[(String, f64, f64)], &[Waiting]) {
        (&self.places, &self.at_places)
    }

    pub fn restore_places(&mut self, places: Vec<(String, f64, f64)>, waiting: Vec<Waiting>) {
        self.places = places;
        self.at_places = waiting;
    }
}
