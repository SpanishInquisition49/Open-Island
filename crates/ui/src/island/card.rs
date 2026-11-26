//! Part of `Island` (see `mod.rs`): the life of the notification cards under the pill. Which cards are up, when the next
//! one drops in, the countdowns that wait while the pointer is on the stack. The motion is in `island_core::droplet`,
//! the drawing and the places in the stack in `faces/card.rs`.

use super::*;
use island_core::droplet::{Anchor, Animator, GAP, STACK_GAP};

/// The most cards on screen at once; the rest wait their turn.
pub(crate) const STACK_MAX: usize = 3;
/// The widest a card gets.
pub(crate) const CARD_MAX_W: f64 = 520.0;
/// The tallest a card gets.
const CARD_MAX_H: f64 = 118.0;
/// The least time between two cards dropping in: the one before has landed and spread.
const SPACING_MS: i64 = 700;
/// How long the cards below take to close a gap, from the moment the drop above them starts to rise.
const SETTLE_MS: i64 = 450;
/// How long the dot of a card that waits keeps pinging before it rests (frames cost power).
pub(crate) const PING_FOR_MS: i64 = 12_000;
/// A pointer resting on the stack holds it this long after it last moved.
const HOLD_MS: i64 = 5000;
/// How long after it appears, or changes, a card with something of its own that moves needs frames.
const LIVELY_MS: i64 = 2000;
/// A card the pointer leaves stays at least this much longer.
const AFTER_HOLD_MS: i64 = 1500;

/// The room the surface keeps under the pill for `cards` cards: their places, the overshoot, and the small drop that
/// counts the waiting ones. `taller` is how much the cards shown whole stand above the usual tallest.
pub(crate) fn zone_height(cards: usize, taller: f64) -> f64 {
    GAP + cards.max(1) as f64 * (CARD_MAX_H + STACK_GAP) + taller + 46.0
}

/// A notification that has a place on screen.
pub(crate) struct Shown {
    pub n: Incoming,
    /// When it goes.
    pub until: i64,
    /// How many messages of one conversation it stands for.
    pub group: u32,
}

/// A card as it is drawn, from the drop that forms to the one that is swallowed.
pub(crate) struct Card {
    /// Its own name in the stack (a notification's id can come again while its card is still leaving).
    pub key: u64,
    pub n: Incoming,
    pub group: u32,
    /// When it starts to come out (after the one before has landed).
    pub born: i64,
    /// Made when first drawn: the sizes need the fonts.
    pub anim: Option<Animator>,
    /// The card it comes out of or goes back into; None for the pill.
    pub anchor: Option<u64>,
    /// It is on its way out.
    pub leaving: bool,
    /// When what it says was last replaced in place.
    pub changed_at: i64,
    /// How long the countdown has been held: it starts that much later.
    pub held_ms: i64,
    /// Where the open card was last drawn: (x, y, width, height, radius) in surface coordinates.
    pub rect: Option<(f64, f64, f64, f64, f64)>,
    /// It starts somewhere of its own instead of inside what is above it.
    pub arrival: Option<effects::Arrival>,
    /// Where its one-time code was last drawn: the left end and the middle of the line, in the pill's coordinates.
    pub code_at: Option<(f64, f64)>,
}

impl Card {
    /// Seconds since it was born.
    pub fn age(&self, now: i64) -> f64 {
        (now - self.born) as f64 / 1000.0
    }

    /// It waits for the user and asks nothing: a dot where the countdown would be.
    pub fn waits(&self) -> bool {
        self.n.stay && self.n.actions.is_empty()
    }

    fn moving(&self, now: i64) -> bool {
        // a battery ring fills, a code rolls in: both a little after the words, and again when they are replaced
        let lively = (now - self.born < LIVELY_MS || now - self.changed_at < LIVELY_MS) && (self.n.ring.is_some() || notif_code(&self.n).is_some());
        self.leaving || lively || now < self.born || now - self.changed_at < 250 || self.anim.as_ref().is_none_or(|a| self.age(now) < a.end().max(a.cue() + 0.5))
    }
}

#[derive(Default)]
pub(crate) struct Cards {
    /// Top to bottom, the ones on their way out included.
    pub stack: Vec<Card>,
    keys: u64,
    pointer_moved_at: i64,
    held: bool,
    ticked_at: i64,
}

impl Cards {
    /// The cards that are not leaving, top to bottom.
    pub fn live(&self) -> impl Iterator<Item = &Card> {
        self.stack.iter().filter(|c| !c.leaving)
    }

    /// Whether something on a card moves slowly: a countdown draining, or a waiting dot that still pings.
    pub fn any_ticking(&self, now: i64) -> bool {
        self.live().any(|c| !c.n.stay || (c.waits() && now - c.born.max(c.changed_at) < PING_FOR_MS))
    }

    /// How much the drawn cards stand above the usual tallest, all together.
    pub fn taller(&self) -> f64 {
        self.stack.iter().filter_map(|c| c.anim.as_ref()).map(|a| (a.size().h - CARD_MAX_H).max(0.0)).sum()
    }

    /// Where the cards that take the pointer are.
    pub fn rects(&self) -> impl Iterator<Item = (f64, f64, f64, f64, f64)> + '_ {
        self.live().filter_map(|c| c.rect)
    }

    /// Where the card above the one at `index` that stays rests, and its name: what the one at `index` comes out of or
    /// goes back into. The pill when there is none.
    pub fn anchor_above(&self, index: usize, pill_h: f64) -> (Option<u64>, Anchor) {
        self.stack[..index]
            .iter()
            .rev()
            .filter(|c| !c.leaving)
            .find_map(|c| c.anim.as_ref().map(|a| (Some(c.key), Anchor { bottom: a.top() + a.size().h, height: a.size().h })))
            .unwrap_or((None, Anchor::pill(pill_h)))
    }

    /// Sends the drops on their way into `card` to `into` instead.
    pub fn redirect_into(&mut self, card: u64, key: Option<u64>, into: Anchor, now: i64) {
        for c in self.stack.iter_mut().filter(|c| c.leaving && c.anchor == Some(card)) {
            let age = c.age(now);
            if let Some(a) = c.anim.as_mut() {
                a.redirect(into, age);
                c.anchor = key;
            }
        }
    }

    /// Starts the card at `index` on its way out. Returns false when it never showed and is simply dropped.
    fn leave(&mut self, index: usize, now: i64, pill_h: f64) -> bool {
        let (above, into) = self.anchor_above(index, pill_h);
        let c = &mut self.stack[index];
        let age = c.age(now);
        let Some(a) = c.anim.as_mut().filter(|_| age > 0.0) else {
            self.stack.remove(index);
            return false;
        };
        a.exit(age, into);
        c.leaving = true;
        c.anchor = above;
        c.rect = None;
        let key = c.key;
        self.redirect_into(key, above, into, now);
        true
    }
}

/// Whether two notifications would be drawn the same.
fn same_words(a: &Incoming, b: &Incoming) -> bool {
    a.id == b.id && a.app == b.app && a.summary == b.summary && a.body == b.body && a.kind == b.kind && a.tint == b.tint && a.icon == b.icon && a.stay == b.stay && a.actions == b.actions && a.ring == b.ring
}

impl Island {
    /// A notification card is on screen (coming, up or going).
    pub fn has_card(&self) -> bool {
        !self.cards.stack.is_empty()
    }

    pub(crate) fn over_card(&self) -> bool {
        self.card_under_pointer().is_some()
    }

    /// The notification whose card the pointer is on.
    pub(super) fn card_under_pointer(&self) -> Option<u32> {
        let (x, y) = self.pointer?;
        self.cards.live().find(|c| c.rect.is_some_and(|(cx, cy, cw, ch, _)| x >= cx && x <= cx + cw && y >= cy && y <= cy + ch)).map(|c| c.n.id)
    }

    pub(super) fn card_pointer_moved(&mut self) {
        self.cards.pointer_moved_at = self.now;
    }

    /// The card of the notification `old` now stands for `new`: it changes where it is.
    pub(super) fn rename_card(&mut self, old: u32, new: u32) {
        let now = self.now;
        if let Some(c) = self.cards.stack.iter_mut().find(|c| !c.leaving && c.n.id == old) {
            c.n.id = new;
            c.changed_at = now;
        }
    }

    /// While the pointer rests on the stack every countdown waits, and the cards it leaves get a moment more.
    pub(super) fn hold_cards(&mut self) {
        let step = (self.now - self.cards.ticked_at).clamp(0, 1000);
        self.cards.ticked_at = self.now;
        let over = self.over_card();
        // a pointer that came down from an open page keeps it open, or the cards would move away from under it
        if over && self.now < self.hover_left + HOVER_GRACE_MS {
            self.hover_left = self.now;
        }
        let held = over && self.now - self.cards.pointer_moved_at < HOLD_MS;
        let released = self.cards.held && !held;
        let at_least = self.now + AFTER_HOLD_MS;
        if held {
            self.notif_until += step;
            self.below.iter_mut().for_each(|b| b.until += step);
            self.cards.stack.iter_mut().filter(|c| !c.leaving).for_each(|c| c.held_ms += step);
        } else if released {
            self.notif_until = self.notif_until.max(at_least);
            self.below.iter_mut().for_each(|b| b.until = b.until.max(at_least));
        }
        self.cards.held = held;
    }

    /// Makes the drawn cards agree with the notifications that have a place.
    pub(super) fn sync_cards(&mut self) {
        let (now, pill_h) = (self.now, self.spring.h.pos);
        self.cards.stack.retain(|c| !c.leaving || c.anim.as_ref().is_some_and(|a| c.age(now) < a.end()));
        // an inline reply is typed in the pill: its card goes back in meanwhile
        let replying = self.reply.as_ref().map(|r| r.id);
        let front = self.notif.iter().map(|n| (n, self.notif_group));
        let wanted: Vec<(Incoming, u32)> = front.chain(self.below.iter().map(|b| (&b.n, b.group))).filter(|(n, _)| replying != Some(n.id)).map(|(n, g)| (n.clone(), g)).collect();
        // from the bottom up, so that each goes into what stays above it
        for i in (0..self.cards.stack.len()).rev() {
            let c = &self.cards.stack[i];
            if !c.leaving && !wanted.iter().any(|(n, _)| n.id == c.n.id) {
                self.cards.leave(i, now, pill_h);
            }
        }
        // nothing drops in while the stack closes a gap, or the card it comes out of would not be there yet
        let settled_at = self.cards.stack.iter().filter(|c| c.leaving).filter_map(|c| c.anim.as_ref()?.rise_at().map(|r| c.born + (r * 1000.0) as i64 + SETTLE_MS)).max();
        if let Some(at) = settled_at {
            let late: Vec<(u32, i64)> = self.cards.stack.iter_mut().filter(|c| !c.leaving && c.born > now && c.born < at).map(|c| (c.n.id, at - std::mem::replace(&mut c.born, at))).collect();
            late.into_iter().for_each(|(id, by)| self.delay_card(id, by));
        }
        for (n, own_group) in &wanted {
            // the messages of its app that wait are counted on the lowest card of that app
            let lowest = wanted.iter().rev().find(|(o, _)| o.app == n.app).is_some_and(|(o, _)| o.id == n.id);
            let waiting = if lowest { self.queue.iter().find(|q| q.app == n.app).map_or(0, |q| q.group_count) } else { 0 };
            let group = (*own_group).max(waiting);
            if let Some(c) = self.cards.stack.iter_mut().find(|c| !c.leaving && c.n.id == n.id) {
                // an update, or another message of the same conversation: the card changes where it is
                if !same_words(&c.n, n) || c.group != group {
                    c.changed_at = now;
                }
                c.n = n.clone();
                c.group = group;
                continue;
            }
            // one at a time
            let after_last = self.cards.live().last().map(|c| c.born + SPACING_MS);
            let arrival = self.fx.take_arrival(n.id);
            let born = after_last.into_iter().chain(settled_at).chain(arrival.map(|a| now + a.delay_ms)).fold(now, i64::max);
            // its time starts when it shows
            self.delay_card(n.id, born - now);
            self.cards.keys += 1;
            self.cards.stack.push(Card { key: self.cards.keys, n: n.clone(), group, born, anim: None, anchor: None, leaving: false, changed_at: i64::MIN / 2, held_ms: 0, rect: None, arrival, code_at: None });
        }
        // whatever did not get a card now never will
        self.fx.arrivals.clear();
    }

    /// Gives the notification `id` that much more time on screen.
    fn delay_card(&mut self, id: u32, by: i64) {
        if self.notif.as_ref().is_some_and(|n| n.id == id) {
            self.notif_until += by;
        } else if let Some(b) = self.below.iter_mut().find(|b| b.n.id == id) {
            b.until += by;
        }
    }

    /// Whether a card needs a frame at every refresh.
    pub(super) fn card_moving(&self) -> bool {
        self.cards.stack.iter().any(|c| c.moving(self.now))
    }

    /// How much of its time a card has left (1 as its words appear, 0 when it goes).
    pub(crate) fn card_time_left(&self, c: &Card) -> f64 {
        let until = match &self.notif {
            Some(n) if n.id == c.n.id => self.notif_until,
            _ => self.below.iter().find(|b| b.n.id == c.n.id).map_or(0, |b| b.until),
        };
        let start = c.born + c.anim.as_ref().map_or(0, |a| (a.cue() * 1000.0) as i64) + c.held_ms;
        ((until - self.now) as f64 / (until - start).max(1) as f64).clamp(0.0, 1.0)
    }
}
