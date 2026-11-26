//! Part of `Island` (see `mod.rs`): the life of the notification card that drops out of the pill. Which card is up, when
//! the one before has gone, the countdown that waits while the pointer is on it. The motion is in `island_core::droplet`,
//! the drawing in `faces/card.rs`.

use super::*;
use island_core::droplet::Animator;

/// The widest a card gets.
pub(crate) const CARD_MAX_W: f64 = 520.0;
/// The room the surface keeps under the pill while a card is up: the gap, the tallest card and its overshoot, and the
/// small drop that counts the waiting ones.
pub(crate) const CARD_ZONE_H: f64 = 176.0;
/// A pointer resting on a card holds it this long after it last moved.
const HOLD_MS: i64 = 5000;
/// A card the pointer leaves stays at least this much longer.
const AFTER_HOLD_MS: i64 = 1500;

pub(crate) struct Card {
    pub n: Incoming,
    /// How many messages of one conversation it stands for.
    pub group: u32,
    /// When it starts to come out of the pill (after the one before has gone back in).
    pub born: i64,
    /// Made when first drawn: the sizes need the fonts.
    pub anim: Option<Animator>,
    /// When what it says was last replaced in place.
    pub changed_at: i64,
    /// How long the countdown has been held: it starts that much later.
    pub held_ms: i64,
}

impl Card {
    /// Seconds since it was born.
    pub fn age(&self, now: i64) -> f64 {
        (now - self.born) as f64 / 1000.0
    }

    fn moving(&self, now: i64) -> bool {
        now < self.born || now - self.changed_at < 250 || self.anim.as_ref().is_none_or(|a| self.age(now) < a.end().max(a.cue() + 0.5))
    }
}

#[derive(Default)]
pub(crate) struct Cards {
    /// The card on screen.
    pub live: Option<Card>,
    /// The one on its way back into the pill.
    pub gone: Option<Card>,
    /// Where the open card was last drawn: (x, y, width, height, radius) in surface coordinates.
    pub rect: Option<(f64, f64, f64, f64, f64)>,
    pointer_moved_at: i64,
    held: bool,
    ticked_at: i64,
}

/// Whether two notifications would be drawn the same.
fn same_words(a: &Incoming, b: &Incoming) -> bool {
    a.id == b.id && a.app == b.app && a.summary == b.summary && a.body == b.body
}

impl Island {
    /// A notification card is on screen (coming, up or going).
    pub fn has_card(&self) -> bool {
        self.cards.live.is_some() || self.cards.gone.is_some()
    }

    pub(crate) fn over_card(&self) -> bool {
        match (self.pointer, self.cards.rect) {
            (Some((x, y)), Some((cx, cy, cw, ch, _))) => x >= cx && x <= cx + cw && y >= cy && y <= cy + ch,
            _ => false,
        }
    }

    pub(super) fn card_pointer_moved(&mut self) {
        self.cards.pointer_moved_at = self.now;
    }

    /// Sends the card on screen back into the pill.
    pub(super) fn leave_card(&mut self) {
        let Some(mut c) = self.cards.live.take() else { return };
        self.cards.rect = None;
        self.cards.held = false;
        let age = c.age(self.now);
        // one that never showed has nothing to take back
        if let Some(a) = c.anim.as_mut().filter(|_| age > 0.0) {
            a.exit(age, self.spring.h.pos);
            self.cards.gone = Some(c);
        }
    }

    /// While the pointer rests on the card its countdown waits, and a card it leaves gets a moment more.
    pub(super) fn hold_card(&mut self) {
        let step = (self.now - self.cards.ticked_at).clamp(0, 1000);
        self.cards.ticked_at = self.now;
        let over = self.over_card();
        // a pointer that came down from an open page keeps it open, or the card would move away from under it
        if over && self.now < self.hover_left + HOVER_GRACE_MS {
            self.hover_left = self.now;
        }
        let held = self.notif.is_some() && over && self.now - self.cards.pointer_moved_at < HOLD_MS;
        if held {
            self.notif_until += step;
            if let Some(c) = self.cards.live.as_mut() {
                c.held_ms += step;
            }
        } else if self.cards.held && self.notif.is_some() {
            self.notif_until = self.notif_until.max(self.now + AFTER_HOLD_MS);
        }
        self.cards.held = held;
    }

    /// Makes the card agree with the notification that is current.
    pub(super) fn sync_card(&mut self) {
        let now = self.now;
        if self.cards.gone.as_ref().is_some_and(|c| c.anim.as_ref().is_none_or(|a| c.age(now) >= a.end())) {
            self.cards.gone = None;
        }
        // an inline reply is typed in the pill: the card goes back in meanwhile
        let Some(n) = self.notif.as_ref().filter(|_| self.reply.is_none()) else {
            self.leave_card();
            return;
        };
        let waiting = self.queue.iter().find(|q| q.app == n.app).map_or(0, |q| q.group_count);
        let group = self.notif_group.max(waiting);
        match self.cards.live.as_mut() {
            // an update, or another message of the same conversation: the card changes where it is
            Some(c) => {
                if !same_words(&c.n, n) || c.group != group {
                    c.changed_at = now;
                }
                c.n = n.clone();
                c.group = group;
            }
            None => {
                // the one before goes first
                let born = self.cards.gone.as_ref().and_then(|c| c.anim.as_ref().map(|a| c.born + (a.end() * 1000.0) as i64)).map_or(now, |t| t.max(now));
                self.notif_until += born - now;
                self.cards.live = Some(Card { n: n.clone(), group, born, anim: None, changed_at: i64::MIN / 2, held_ms: 0 });
            }
        }
    }

    /// Whether a card needs a frame at every refresh.
    pub(super) fn card_moving(&self) -> bool {
        self.cards.gone.is_some() || self.cards.live.as_ref().is_some_and(|c| c.moving(self.now))
    }

    /// How much of its time the card on screen has left (1 as its words appear, 0 when it goes).
    pub(crate) fn card_time_left(&self) -> f64 {
        let Some(c) = &self.cards.live else { return 0.0 };
        let start = c.born + c.anim.as_ref().map_or(0, |a| (a.cue() * 1000.0) as i64) + c.held_ms;
        let total = (self.notif_until - start).max(1);
        ((self.notif_until - self.now) as f64 / total as f64).clamp(0.0, 1.0)
    }
}
