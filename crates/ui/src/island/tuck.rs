//! Part of `Island` (see `mod.rs`): out of the way of a window. While one is over the island's place the island slides
//! up out of sight, and the pointer at the top edge there brings it back. It also comes out by itself for what it has
//! to show (a card, the volume bar, a password request, a page or the bar asked for) and leaves again afterwards.

use super::*;

/// How long the island stays after nothing holds it out any more.
const LEAVE_MS: i64 = 700;
/// How long the slide takes, either way.
const SLIDE_MS: f64 = 220.0;
/// The strip at the very top that calls the island back.
const REVEAL_H: f64 = 3.0;

#[derive(Default)]
pub(crate) struct Tuck {
    /// A window is over the island's place.
    covered: bool,
    /// The platform says the pointer is asking for it (one whose window cannot be pointed at while it is hidden).
    called: bool,
    /// Since when nothing has held it out.
    free_since: Option<i64>,
    /// How far it is gone: 0 in its place, 1 out of sight.
    amount: f64,
}

impl Island {
    /// What the platform sees: a window over the island's place, and the pointer calling for the island.
    pub fn set_cover(&mut self, covered: bool, called: bool) {
        (self.tuck.covered, self.tuck.called) = (covered, called);
    }

    /// Whether something keeps the island in its place, window or no window.
    fn held_out(&self) -> bool {
        !self.tuck.covered
            || !self.cfg.hide_when_covered
            || self.tuck.called
            || self.pointer.is_some()
            // anything but the resting pill: a page, a bar, a card, the volume, a request, a file on its way in
            || !self.shown.is_compact()
            || !self.want.is_compact()
            || self.has_card()
            || self.wants_keyboard()
            || self.auth_motion().is_some()
    }

    /// Once a frame.
    pub(super) fn tuck_tick(&mut self, now: i64, dt_ms: f64) {
        let held = self.held_out();
        let t = &mut self.tuck;
        let step = dt_ms.clamp(0.0, 50.0) / SLIDE_MS;
        if held {
            t.free_since = None;
            t.amount = (t.amount - step).max(0.0);
        } else if now - *t.free_since.get_or_insert(now) >= LEAVE_MS {
            t.amount = (t.amount + step).min(1.0);
        }
    }

    /// It is on its way out or back.
    pub(super) fn tuck_sliding(&self) -> bool {
        match (self.held_out(), self.tuck.free_since) {
            (true, _) => self.tuck.amount > 0.0,
            (false, since) => self.tuck.amount < 1.0 && since.is_some_and(|since| self.now - since >= LEAVE_MS),
        }
    }

    /// How long until it starts to leave, when it is about to.
    pub(super) fn tuck_due_ms(&self) -> Option<u32> {
        let since = self.tuck.free_since.filter(|_| self.tuck.amount < 1.0)?;
        Some((since + LEAVE_MS - self.now).max(0) as u32)
    }

    /// Nothing of it is on screen: there is nothing to draw, and a platform may hide its window.
    pub fn out_of_sight(&self) -> bool {
        self.tuck.amount >= 1.0
    }

    /// How far up the drawing is moved, in logical pixels: enough for the pill to be gone when it has fully left.
    pub(crate) fn tuck_rise(&self) -> f64 {
        let t = self.tuck.amount;
        // slow at both ends
        let eased = t * t * (3.0 - 2.0 * t);
        eased * (TOP_MARGIN + self.spring.h.pos + 2.0)
    }

    /// The parts that take the pointer, given those of the island in its place.
    pub(super) fn tuck_input(&self, in_place: Vec<(f64, f64, f64, f64)>) -> Vec<(f64, f64, f64, f64)> {
        if !self.tuck.covered || !self.cfg.hide_when_covered {
            return in_place;
        }
        let (x, y, w, _) = self.pill_rect();
        if self.tuck.amount >= 0.5 {
            return vec![(x, 0.0, w, REVEAL_H)];
        }
        // the pointer that called it from the edge is not lost on the way down to the pill
        let mut v = in_place;
        v.push((x, 0.0, w, y + 1.0));
        v
    }
}
