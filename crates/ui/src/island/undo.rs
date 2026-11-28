//! Part of `Island` (see `mod.rs`): taking back the last thing removed, by a cross, a menu or a line, from the card
//! that says it went.

use super::events::{action, EVENT_ID};
use super::*;
use island_core::places::Waiting;

/// The card of a removal takes the place of the one before: only the last can be taken back.
const UNDO_ID: u32 = EVENT_ID + 6;
/// How long it stays.
const UNDO_MS: i64 = 8000;
/// The most of each kind the island keeps.
const MOST_TASKS: usize = 30;
const MOST_DEADLINES: usize = 20;
const MOST_PINS: usize = 8;
const MOST_SHELF: usize = 12;
const MOST_WAITING: usize = 20;

/// What was removed, each with the position it had.
pub(crate) enum Removed {
    Task(usize, Todo),
    Deadline(usize, Deadline),
    Pins(Vec<(usize, String)>),
    Shelf(Vec<(usize, String)>),
    Waiting(usize, Waiting),
}

/// Puts what is not there again back where it was; false when the list has no room left for it.
fn put_back<T: PartialEq>(list: &mut Vec<T>, items: Vec<(usize, T)>, most: usize) -> bool {
    let missing: Vec<(usize, T)> = items.into_iter().filter(|(_, x)| !list.contains(x)).collect();
    if list.len() + missing.len() > most {
        return false;
    }
    // in the order they had, so that each position counts the ones before it
    for (at, x) in missing {
        list.insert(at.min(list.len()), x);
    }
    true
}

impl Island {
    /// Says that something went, with the button that brings it back.
    pub(super) fn removed(&mut self, what: Removed, summary: &str, body: &str) {
        self.undo_count += 1;
        let key = format!("open-island:undo:{}", self.undo_count);
        self.undo = Some(what);
        self.announce(Incoming { id: UNDO_ID, summary: summary.into(), body: body.into(), icon: Some("trash".into()), actions: vec![action(key, "Undo")], expire_ms: Some(UNDO_MS), ..Default::default() });
    }

    /// The Undo of the card number `count`. A card of an earlier removal brings back nothing.
    pub(super) fn undo_removal(&mut self, count: &str) {
        if count.parse() != Ok(self.undo_count) {
            return;
        }
        let Some(what) = self.undo.take() else { return };
        let back = match what {
            Removed::Task(at, task) => self.restore_task(at, task),
            Removed::Deadline(at, d) => {
                let back = put_back(&mut self.deadlines, vec![(at, d)], MOST_DEADLINES);
                self.deadlines.sort_by_key(|d| d.day);
                back
            }
            Removed::Pins(items) => put_back(&mut self.pins, items, MOST_PINS),
            Removed::Shelf(items) => put_back(&mut self.shelf, items, MOST_SHELF),
            Removed::Waiting(at, w) => put_back(&mut self.at_places, vec![(at, w)], MOST_WAITING),
        };
        if !back {
            self.say("No room to put it back", "The list filled up meanwhile");
        }
    }

    /// A task comes back here and on the server, which may have been told to delete it.
    fn restore_task(&mut self, at: usize, mut task: Todo) -> bool {
        if self.todos.iter().any(|t| t.uid == task.uid) {
            return true;
        }
        if self.todos.len() >= MOST_TASKS {
            return false;
        }
        self.tasks_done.retain(|uid| *uid != task.uid);
        // not on the server until it says so: a list read meanwhile must not drop it
        task.synced = false;
        self.pending.push(Command::PushTask { task: task.clone(), done: false });
        self.todos.insert(at.min(self.todos.len()), task);
        true
    }
}
