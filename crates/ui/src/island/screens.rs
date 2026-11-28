//! The cards of a second screen: what to do with one just connected, and whether to keep a choice that switched one off.

use super::*;
use island_core::screens::{Mode, Screen};

const SCREEN_ID: u32 = events::EVENT_ID + 40;
const ASK_MS: i64 = 20_000;

impl Island {
    /// What the platform sees. A card comes when a screen beyond the machine's own appears, or when asked for.
    pub(super) fn screens_seen(&mut self, all: Vec<Screen>, asked: bool) {
        let new: Vec<&Screen> = all.iter().filter(|s| !s.internal && !self.externals.contains(&s.name)).collect();
        let shown = if asked { all.iter().find(|s| !s.internal) } else { new.first().copied() };
        let own = all.iter().any(|s| s.internal);
        match shown {
            Some(s) => {
                let body = if s.label.is_empty() { s.name.clone() } else { format!("{} · {}", s.label, s.name) };
                // the choices are about the laptop's panel and the other one: a desktop has nothing to choose
                let actions = if own {
                    [("extend", "Extend"), ("external", "External"), ("internal", "Laptop")].iter().map(|(k, l)| events::action(format!("open-island:screens:{k}"), l)).collect()
                } else {
                    vec![]
                };
                self.announce(Incoming { id: SCREEN_ID, summary: "Screen connected".into(), body, icon: Some("device-desktop".into()), actions, expire_ms: Some(ASK_MS), ..Default::default() });
            }
            None if asked => self.say("No other screen", "Nothing but this one is connected"),
            None => {}
        }
        self.externals = all.into_iter().filter(|s| !s.internal).map(|s| s.name).collect();
    }

    /// A screen has just been switched off: the choice stays only if it is confirmed (the platform undoes it otherwise).
    pub(super) fn screens_changed(&mut self) {
        let actions = vec![events::action("open-island:screens:keep".into(), "Keep"), events::action("open-island:screens:undo".into(), "Undo")];
        self.announce(Incoming { id: SCREEN_ID, summary: "Keep the screens like this?".into(), body: format!("Back as before in {} s", crate::model::SCREEN_CONFIRM_MS / 1000), icon: Some("device-desktop".into()), actions, expire_ms: Some(crate::model::SCREEN_CONFIRM_MS), ..Default::default() });
    }

    /// A button of a screen card.
    pub(super) fn screens_action(&mut self, what: &str) {
        self.pending.push(match what {
            "extend" => Command::SetScreens(Mode::Extend),
            "external" => Command::SetScreens(Mode::ExternalOnly),
            "internal" => Command::SetScreens(Mode::InternalOnly),
            "keep" => Command::KeepScreens,
            _ => Command::UndoScreens,
        });
    }
}
