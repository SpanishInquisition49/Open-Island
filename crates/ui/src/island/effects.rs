//! Part of `Island` (see `mod.rs`): the short animations that belong to one event. The drop beside the pill that goes on
//! to become the card of what ended, the drops of the notifications held while quiet that gather into their summary, the
//! digits of a one-time code on their way into the pill, the drop that leaves when files have gone, and the pill that
//! catches a dragged file. Drawn in `faces/effects.rs`.
//!
//! Each is started by an event, is a closed form of the time since then and ends by itself: nothing here runs at rest.

use super::*;
use island_core::droplet::{Curve, DROP, GAP};

/// Where a card comes from when not from the pill itself.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Arrival {
    /// The notification whose card this is.
    pub card: u32,
    /// The middle of the drop it starts as, from the middle of the pill's bottom edge.
    pub from: (f64, f64),
    /// It starts that much later (something else plays first).
    pub delay_ms: i64,
    /// The colour the drop carries until it opens.
    pub tint: Option<(u8, u8, u8)>,
    /// Bad news: the card trembles as it opens.
    pub shake: bool,
}

/// The most held notifications that come out as drops of their own.
pub(crate) const BURST_MAX: usize = 6;
/// Each drop comes out this long after the one before.
const BURST_STAGGER: i64 = 50;
/// When the drops start to gather, how long that takes, and when the summary's card takes over.
const GATHER_AT: i64 = 850;
const GATHER_MS: i64 = 220;
pub(crate) const BURST_MS: i64 = GATHER_AT + GATHER_MS;
const BURST_OUT: Curve = Curve::new(0.45, 0.25);
/// The room between two drops of a burst.
const BURST_SPREAD: f64 = 36.0;
/// Where the drops gather, below the pill's bottom: the middle of a drop that already hangs loose.
const GATHERED: f64 = GAP + DROP / 2.0 + 4.0;
const BURST_BLEND: f64 = 20.0;

/// One held notification on its way out.
#[derive(Clone)]
pub(crate) struct Held {
    pub app: String,
    pub icon: Option<Image>,
}

pub(crate) struct Burst {
    pub born: i64,
    pub drops: Vec<Held>,
}

/// A drop of a burst at one instant: its middle from the middle of the pill's bottom edge, its width, how much it still
/// pulls on the pill, and how visible what it says is.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BurstDrop {
    pub x: f64,
    pub y: f64,
    pub d: f64,
    pub blend: f64,
    pub content: f64,
}

fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Burst {
    /// Where the `n`-th drop is `age` ms after the start, in a pill `pill_h` tall.
    pub fn place(&self, n: usize, age: i64, pill_h: f64) -> BurstDrop {
        let out = BURST_OUT.progress((age - n as i64 * BURST_STAGGER) as f64 / 1000.0);
        let slot = (n as f64 - (self.drops.len() as f64 - 1.0) / 2.0) * BURST_SPREAD;
        let inside = -pill_h.min(DROP + 2.0) / 2.0;
        let gather = smooth(0.0, 1.0, (age - GATHER_AT) as f64 / GATHER_MS as f64);
        let spread = 14.0 + (DROP + 2.0 - 14.0) * out.min(1.0);
        BurstDrop {
            x: slot * out * (1.0 - gather),
            y: inside + (GATHERED - inside) * out,
            d: spread + (DROP - spread) * gather,
            blend: BURST_BLEND * (1.0 - smooth(0.5, 0.95, out)),
            content: smooth(0.6, 1.0, out) * (1.0 - smooth(0.0, 0.6, gather)),
        }
    }
}

/// How long after the one before each digit sets off, how long its way takes, and how long the pill says it is copied.
const FLIGHT_STAGGER: i64 = 35;
const FLIGHT_MS: i64 = 420;
const SAID_MS: i64 = 900;

/// The digits of a code going from its card into the pill.
pub(crate) struct Flight {
    pub born: i64,
    /// The code as the card writes it (with its space).
    pub text: String,
    /// The left end of the code and the middle of its line, in the pill's coordinates.
    pub from: (f64, f64),
}

impl Flight {
    fn characters(&self) -> i64 {
        self.text.chars().count() as i64
    }

    /// When the last digit is in.
    fn landed(&self) -> i64 {
        (self.characters() - 1).max(0) * FLIGHT_STAGGER + FLIGHT_MS
    }

    pub fn end(&self) -> i64 {
        self.born + self.landed() + SAID_MS
    }

    /// How far the `n`-th character has gone: 0 still on the card, 1 in the pill.
    pub fn way(&self, n: usize, now: i64) -> f64 {
        ((now - self.born - n as i64 * FLIGHT_STAGGER) as f64 / FLIGHT_MS as f64).clamp(0.0, 1.0)
    }

    /// How visible the pill's "Copied" is.
    pub fn said(&self, now: i64) -> f64 {
        let age = now - self.born;
        let from = self.landed() - 120;
        smooth(0.0, 150.0, (age - from) as f64).min(smooth(0.0, 250.0, (self.end() - now) as f64))
    }
}

/// How long the drop that leaves after a send takes to be gone, and when it sets off (after the card has started).
pub(crate) const SHOT_MS: i64 = 750;
const SHOT_AFTER_MS: i64 = 150;
/// The room it needs past the pill's right end.
pub(crate) const SHOT_ROOM: f64 = 96.0;

/// How long the pill takes to swallow a file, and to give up on one that went away.
pub(crate) const GULP_MS: i64 = 560;
const RETRACT_MS: i64 = 250;
/// A drag that says nothing for this long is forgotten (a compositor that never said it left).
const CATCH_STALE_MS: i64 = 60_000;
/// How far the drop that reaches for the file goes below the pill.
pub(crate) const REACH: f64 = 46.0;
/// How far from the pill's ends the reaching drop stays: clear of the round corners.
const REACH_INSET: f64 = 48.0;
/// The room around the pill that takes the drag.
const CATCH_MARGIN: f64 = 54.0;

/// A file dragged over the island.
pub(crate) struct Catch {
    /// The pointer, in surface coordinates.
    at: (f64, f64),
    /// The drop that reaches for it, in the pill's coordinates.
    pub tongue: (f64, f64),
    /// When it was let go, and when it went away.
    pub dropped: Option<i64>,
    left: Option<i64>,
    heard: i64,
}

impl Catch {
    /// How much of the reaching drop is left: 1 while it follows the pointer, 0 swallowed or gone.
    pub fn out(&self, now: i64) -> f64 {
        match (self.dropped, self.left) {
            (Some(at), _) => 1.0 - smooth(0.0, GULP_MS as f64 * 0.65, (now - at) as f64),
            (None, Some(at)) => 1.0 - smooth(0.0, RETRACT_MS as f64, (now - at) as f64),
            (None, None) => 1.0,
        }
    }

    /// The pill swells as the file goes down: 0..1 and back.
    pub fn swell(&self, now: i64) -> f64 {
        let Some(at) = self.dropped else { return 0.0 };
        let t = (now - at) as f64;
        smooth(200.0, 300.0, t) * (1.0 - smooth(330.0, 440.0, t))
    }
}

#[derive(Default)]
pub(crate) struct Effects {
    /// Cards still to be made that start somewhere of their own.
    pub arrivals: Vec<Arrival>,
    pub burst: Option<Burst>,
    pub flight: Option<Flight>,
    /// When the drop that says files have gone sets off.
    pub shot: Option<i64>,
    pub catch: Option<Catch>,
    /// A device that connected without saying its battery, and until when it is worth waiting for it.
    pub awaiting_battery: Option<(String, i64)>,
}

impl Effects {
    pub fn take_arrival(&mut self, card: u32) -> Option<Arrival> {
        let at = self.arrivals.iter().position(|a| a.card == card)?;
        Some(self.arrivals.remove(at))
    }

    /// Something here needs a frame at every refresh.
    pub fn moving(&self) -> bool {
        !self.arrivals.is_empty() || self.burst.is_some() || self.flight.is_some() || self.shot.is_some() || self.catch.is_some()
    }
}

impl Island {
    /// A file drag is over the island (`Some`: where the pointer is, in surface coordinates) or has left it.
    pub(super) fn file_drag(&mut self, at: Option<(f64, f64)>) {
        let now = self.now;
        match (at, self.fx.catch.as_mut()) {
            (Some(at), Some(c)) if c.dropped.is_none() => (c.at, c.left, c.heard) = (at, None, now),
            // a new one, also over the end of the last one's meal
            (Some(at), _) => self.fx.catch = Some(Catch { at, tongue: (self.spring.w.pos / 2.0, self.spring.h.pos / 2.0), dropped: None, left: None, heard: now }),
            (None, Some(c)) if c.dropped.is_none() => {
                c.left.get_or_insert(now);
            }
            (None, _) => {}
        }
    }

    /// The file was let go over the island: down it goes.
    pub(super) fn file_dropped(&mut self) {
        let now = self.now;
        if let Some(c) = self.fx.catch.as_mut() {
            (c.dropped, c.left) = (Some(now), None);
        }
    }

    /// Ends what has run its course, and moves the reaching drop towards the pointer.
    pub(super) fn update_effects(&mut self, dt_ms: f64) {
        let now = self.now;
        if self.fx.burst.as_ref().is_some_and(|b| now - b.born >= BURST_MS) {
            self.fx.burst = None;
        }
        if self.fx.flight.as_ref().is_some_and(|f| now >= f.end()) {
            self.fx.flight = None;
        }
        if self.fx.shot.is_some_and(|at| now - at >= SHOT_MS) {
            self.fx.shot = None;
        }
        let (w, h, origin) = (self.spring.w.pos, self.spring.h.pos, self.pill_origin);
        let over = self.fx.catch.as_ref().is_some_and(|c| {
            c.dropped.is_some_and(|at| now - at >= GULP_MS) || c.left.is_some_and(|at| now - at >= RETRACT_MS) || (c.dropped.is_none() && now - c.heard >= CATCH_STALE_MS)
        });
        if over {
            self.fx.catch = None;
        }
        if let Some(c) = self.fx.catch.as_mut() {
            let target = if c.dropped.is_some() || c.left.is_some() {
                (w / 2.0, h / 2.0)
            } else {
                // as far as it reaches: between the ends, some way below
                ((c.at.0 - origin.0).clamp(REACH_INSET, (w - REACH_INSET).max(REACH_INSET)), (c.at.1 - origin.1).clamp(h - 8.0, h + REACH))
            };
            let k = 1.0 - (-dt_ms.clamp(0.0, 100.0) / 70.0).exp();
            c.tongue = (c.tongue.0 + (target.0 - c.tongue.0) * k, c.tongue.1 + (target.1 - c.tongue.1) * k);
        }
    }

    /// The area that takes a file drag, in surface coordinates, while one is over the island.
    pub(super) fn catch_rect(&self) -> Option<(f64, f64, f64, f64)> {
        self.fx.catch.as_ref().filter(|c| c.dropped.is_none())?;
        let (x, y, w, h) = self.pill_rect();
        Some((x - CATCH_MARGIN, y, w + 2.0 * CATCH_MARGIN, h + REACH + 30.0))
    }

    /// A send went through: a drop leaves the pill.
    pub(super) fn shoot(&mut self) {
        self.fx.shot = Some(self.now + SHOT_AFTER_MS);
    }

    /// What was held while quiet comes out drop by drop, and the card `summary` forms where the drops gather. Only when
    /// no other card is on screen: the drops would cross it.
    pub(super) fn burst(&mut self, summary: u32, drops: Vec<Held>) {
        if drops.is_empty() || !self.cards.stack.is_empty() || self.notif.as_ref().is_none_or(|n| n.id != summary) {
            return;
        }
        self.fx.burst = Some(Burst { born: self.now, drops });
        self.fx.arrivals.push(Arrival { card: summary, from: (0.0, GATHERED), delay_ms: BURST_MS, tint: None, shake: false });
    }

    /// The code on the card of the notification `id` was copied: its digits go into the pill.
    pub(super) fn fly_code(&mut self, id: u32, code: &str) {
        let Some(from) = self.cards.live().find(|c| c.n.id == id).and_then(|c| c.code_at) else { return };
        self.fx.flight = Some(Flight { born: self.now, text: island_core::codes::display(code), from });
    }
}
