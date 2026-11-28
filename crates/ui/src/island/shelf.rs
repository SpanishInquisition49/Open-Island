//! Part of `Island` (see `mod.rs`): the shelf, files set down on the island to be picked up again somewhere else.

use super::*;

/// The most files it keeps.
const MAX: usize = 12;
/// How far the pointer moves, button held, before a press on a file is a drag.
const DRAG_AFTER: f64 = 6.0;

impl Island {
    /// Sets files down on the shelf and shows it.
    pub(super) fn shelve(&mut self, paths: Vec<String>) {
        if self.keep_on_shelf(paths) {
            self.activate(Action::Open(Page::Shelf));
            self.expand_until = self.now + 8000;
        }
    }

    /// Puts files on the shelf, the newest first (one already there only moves up); false when there was none.
    pub(super) fn keep_on_shelf(&mut self, paths: Vec<String>) -> bool {
        let paths: Vec<String> = paths.into_iter().filter(|p| !p.is_empty()).collect();
        let any = !paths.is_empty();
        for path in paths.into_iter().rev() {
            self.shelf.retain(|p| *p != path);
            self.shelf.insert(0, path);
        }
        self.shelf.truncate(MAX);
        any
    }

    /// The pointer moved with a file of the shelf pressed: far enough, and it leaves with the pointer.
    pub(super) fn shelf_drag(&mut self, n: usize, at: (f64, f64)) -> Option<Vec<Command>> {
        let (dx, dy) = (at.0 - self.shelf_pressed.0, at.1 - self.shelf_pressed.1);
        if dx.hypot(dy) < DRAG_AFTER {
            return None;
        }
        self.dragging = None;
        let path = self.shelf.get(n)?.clone();
        Some(vec![Command::DragFiles(vec![path])])
    }

    pub fn shelf(&self) -> &[String] {
        &self.shelf
    }

    pub fn restore_shelf(&mut self, shelf: Vec<String>) {
        self.shelf = shelf;
        self.shelf.truncate(MAX);
    }
}
