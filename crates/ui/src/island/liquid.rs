//! Part of `Island` (see `mod.rs`): the motion of the two other liquid things. The small pills beside the pill leave it
//! on a neck and go back into it (which of them there are is decided elsewhere, as before); and a drop hangs at the
//! left of the pill, filling with the progress of a long job or draining with a countdown. Drawn in `faces/liquid.rs`.
//!
//! When the island opens, neither of them rides the edge that moves away (it would stick out of the growing shape):
//! each stays where it is and the pill grows over it. They split off again once the pill is back at rest.

use super::*;
use island_core::droplet::Curve;

const BUBBLE_IN: Curve = Curve::new(0.55, 0.18);
const BUBBLE_OUT: Curve = Curve::new(0.32, 0.0);
const DROP_IN: Curve = Curve::new(0.6, 0.25);
const DROP_OUT: Curve = Curve::new(0.35, 0.0);
/// How long the drop's level takes to reach a new value, and its surface to settle after that.
const LEVEL_MS: i64 = 600;
pub(crate) const WAVE_MS: i64 = 1500;
/// A drop with nothing to measure rises and falls once in this time, between these two levels.
const SLOSH_MS: f64 = 2400.0;
const SLOSH: (f64, f64) = (0.25, 0.75);
/// The time between two of its frames: it moves slowly, and goes on for as long as a download does.
pub(crate) const BUSY_STEP_MS: u32 = 33;
/// A change of level smaller than this is simply there: a countdown creeping on needs no frames.
const STEP: f64 = 0.02;
/// The hanging drop's width and the room between it and the pill.
pub(crate) const DROP_D: f64 = 26.0;
pub(crate) const DROP_GAP: f64 = 4.0;
/// The height of everything beside the pill: the pill's own at rest.
pub(crate) const REST_H: f64 = 28.0;
/// The pill counts as at rest within this much of that height (its spring overshoots a little).
const REST_SLACK: f64 = 6.0;
/// A swallowed thing is gone once it is this far inside the pill's end (clear of the rounded corner).
const SWALLOWED: f64 = 20.0;
/// The room it takes beside the pill, overshoot included.
pub(crate) const DROP_ROOM: f64 = DROP_D + DROP_GAP + 4.0;

/// One small pill on its way out of the main one, resting, or on its way back.
pub(crate) struct Bubble {
    pub side: Side,
    born: i64,
    /// When it started back, and how far out it was.
    left: Option<(i64, f64)>,
    /// The island is opening over it: its left edge, from the middle of the surface, where it stays.
    pub held: Option<f64>,
}

impl Bubble {
    /// 0 inside the pill, 1 in its place (a little over on the way).
    pub fn out(&self, now: i64) -> f64 {
        match self.left {
            Some((at, from)) => from * (1.0 - BUBBLE_OUT.progress((now - at) as f64 / 1000.0)),
            None => BUBBLE_IN.progress((now - self.born) as f64 / 1000.0),
        }
    }

    fn moving(&self, now: i64) -> bool {
        self.left.is_some() || now - self.born < 900
    }

    /// Turns it round to come out again from where it is.
    fn come_back(&mut self, now: i64) {
        let k = self.out(now);
        let mut age = 0;
        while age < 900 && BUBBLE_IN.progress(age as f64 / 1000.0) < k {
            age += 8;
        }
        (self.born, self.left, self.held) = (now - age, None, None);
    }
}

/// What the hanging drop is filling for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Source {
    /// The running operation with this id.
    Job(String),
    /// The countdown in the pill.
    Timer,
}

/// What there is to hang: for what, how full (None: it cannot be said), in which colour, and the page a click opens.
type Filling = (Source, Option<f64>, (u8, u8, u8), Page);

/// The hanging drop.
pub(crate) struct Fill {
    pub source: Source,
    born: i64,
    left: Option<i64>,
    from: f64,
    to: f64,
    pub changed: i64,
    /// There is nothing to measure: the liquid sloshes.
    pub busy: bool,
    pub tint: (u8, u8, u8),
    pub page: Page,
    /// The island is opening over it: its centre, from the middle of the surface, where it stays.
    pub held: Option<f64>,
    /// Where it was last drawn, in surface coordinates.
    pub rect: Option<(f64, f64, f64, f64)>,
}

impl Fill {
    /// 0 inside the pill, 1 hanging.
    pub fn out(&self, now: i64) -> f64 {
        let k = DROP_IN.progress((now - self.born) as f64 / 1000.0);
        self.left.map_or(k, |at| k * (1.0 - DROP_OUT.progress((now - at) as f64 / 1000.0)))
    }

    /// How far a swallowed one has gone towards the inside of the pill.
    pub fn sunk(&self, now: i64) -> f64 {
        self.left.map_or(0.0, |at| DROP_OUT.progress((now - at) as f64 / 1000.0))
    }

    /// How full it looks.
    pub fn level(&self, now: i64) -> f64 {
        if self.busy {
            let swing = (std::f64::consts::TAU * (now - self.born) as f64 / SLOSH_MS).sin();
            let (mid, half) = ((SLOSH.0 + SLOSH.1) / 2.0, (SLOSH.1 - SLOSH.0) / 2.0);
            return mid + half * swing;
        }
        let t = ((now - self.changed) as f64 / LEVEL_MS as f64).clamp(0.0, 1.0);
        self.from + (self.to - self.from) * (1.0 - (1.0 - t).powi(3))
    }

    fn moving(&self, now: i64) -> bool {
        self.left.is_some() || now - self.born < 900 || now - self.changed < WAVE_MS
    }
}

impl Island {
    /// The pill is a pill, here to stay: what hangs beside it may come out.
    fn at_rest(&self) -> bool {
        self.shown.is_compact() && self.spring.h.pos < REST_H + REST_SLACK
    }

    /// Where the n-th small pill is, in the pill's coordinates, while the pill is `w` wide: (left edge, width).
    pub(crate) fn bubble_place(&self, n: usize, w: f64) -> (f64, f64) {
        let b = &self.bubbles[n];
        if let Some(held) = b.held {
            // where it was on the screen, sinking into the end of the pill that is coming over it
            let there = held + (w + self.bubble_slot()) / 2.0;
            let sunk = b.left.map_or(0.0, |(at, _)| BUBBLE_OUT.progress((self.now - at) as f64 / 1000.0));
            return (there + (w - BUBBLE_W - SWALLOWED - there).min(0.0) * sunk, BUBBLE_W);
        }
        // it starts as a round drop inside the end it leaves and spreads once it is loose
        let out = b.out(self.now);
        let home = w + BUBBLE_GAP + n as f64 * (BUBBLE_W + BUBBLE_GAP);
        let start = home - BUBBLE_GAP - REST_H;
        let t = ((out - 0.45) / 0.55).clamp(0.0, 1.0);
        let width = REST_H + (BUBBLE_W - REST_H) * (t * t * (3.0 - 2.0 * t));
        (start + (home - start) * out.max(0.0), width)
    }

    /// Gives every small pill that should show a place, and sends the others back.
    pub(super) fn sync_bubbles(&mut self) {
        let (now, w) = (self.now, self.spring.w.pos);
        if !self.want.is_compact() {
            // opening: each stays where it is, all at once and before the pill starts to grow
            let slot = self.bubble_slot();
            let places: Vec<f64> = (0..self.bubbles.len()).map(|n| self.bubble_place(n, w).0 - (w + slot) / 2.0).collect();
            for (b, place) in self.bubbles.iter_mut().zip(places).filter(|(b, _)| b.held.is_none()) {
                (b.held, b.left) = (Some(place), Some((now, b.out(now))));
            }
        } else if self.at_rest() {
            let wanted = self.sides_shown();
            for (i, side) in wanted.iter().enumerate() {
                match self.bubbles.get_mut(i) {
                    Some(b) => {
                        b.side = *side;
                        if b.left.is_some() {
                            b.come_back(now);
                        }
                    }
                    None => self.bubbles.push(Bubble { side: *side, born: now, left: None, held: None }),
                }
            }
            for b in self.bubbles.iter_mut().skip(wanted.len()).filter(|b| b.left.is_none()) {
                b.left = Some((now, b.out(now)));
            }
        }
        while let Some(last) = self.bubbles.len().checked_sub(1) {
            let b = &self.bubbles[last];
            let (x, width) = self.bubble_place(last, w);
            let swallowed = b.held.is_some() && x + width <= w - SWALLOWED + 0.5;
            if b.left.is_none() || (b.out(now) >= 0.02 && !swallowed) {
                break;
            }
            self.bubbles.pop();
        }
    }

    /// The width the small pills take beside the pill as they are now.
    pub(crate) fn bubble_slot(&self) -> f64 {
        self.bubbles.iter().map(|b| b.out(self.now).clamp(0.0, 1.0)).sum::<f64>() * (BUBBLE_W + BUBBLE_GAP)
    }

    pub(super) fn bubbles_moving(&self) -> bool {
        self.bubbles.iter().any(|b| b.moving(self.now))
    }

    /// What the hanging drop shows: how far along, in which colour, and the page a click opens.
    fn fill_source(&self) -> Option<Filling> {
        if let Some(op) = self.ops.running().into_iter().find(|o| o.frac.is_some()) {
            return Some((Source::Job(op.id.clone()), Some(op.frac.unwrap_or(0.0).clamp(0.0, 1.0)), op.tint.unwrap_or((0x0a, 0x84, 0xff)), Page::Op));
        }
        let countdown = self.timer.as_ref().filter(|t| t.mode != TimerMode::Stopwatch && t.total_ms > 0).and_then(|t| {
            // full at the start and empty at the end, as the ring of a card drains: what is left is what shows
            let left = (t.end_ms? - self.now).max(0);
            Some((Source::Timer, Some((left as f64 / t.total_ms as f64).clamp(0.0, 1.0)), (0xff, 0x9f, 0x0a), Page::Timer))
        });
        // what has something to measure comes first; then whatever is at work without being able to say how far it is
        countdown.or_else(|| self.ops.running().into_iter().find(|o| o.busy).map(|op| (Source::Job(op.id.clone()), None, op.tint.unwrap_or((0x0a, 0x84, 0xff)), Page::Op)))
    }

    /// Hangs the drop when there is something to fill it with and the pill is at rest.
    pub(super) fn sync_fill(&mut self) {
        let now = self.now;
        let opening = !self.want.is_compact();
        let source = self.fill_source().filter(|_| self.cfg.progress_drop && !opening && self.at_rest());
        // (on the way back to rest there is nothing to hang yet, and nothing to send away)
        if !opening && !self.at_rest() && self.fill.as_ref().is_none_or(|f| f.left.is_some()) {
            if self.fill.as_ref().is_some_and(|f| f.out(now) < 0.02) {
                self.fill = None;
            }
            return;
        }
        let (w, slot) = (self.spring.w.pos, self.bubble_slot());
        match (source, self.fill.as_mut()) {
            (Some((source, level, tint, page)), Some(f)) if f.left.is_none() => {
                (f.source, f.tint, f.page) = (source, tint, page);
                if f.busy && level.is_some() {
                    // it can be measured now: the level goes there from where the liquid happens to be
                    (f.from, f.changed) = (f.level(now), now);
                    f.to = f.from;
                }
                f.busy = level.is_none();
                let level = level.unwrap_or(f.to);
                if (level - f.to).abs() > STEP {
                    (f.from, f.to, f.changed) = (f.level(now), level, now);
                } else if now - f.changed >= LEVEL_MS {
                    (f.from, f.to) = (level, level);
                }
            }
            // a new one, also in place of one on its way back
            (Some((source, level, tint, page)), _) => self.fill = Some(Fill { source, born: now, left: None, from: 0.0, to: level.unwrap_or(0.0), changed: now, busy: level.is_none(), tint, page, held: None, rect: None }),
            (None, Some(f)) => {
                if f.left.is_none() {
                    // it leaves with the liquid as it is
                    if f.busy {
                        (f.to, f.busy) = (f.level(now), false);
                        f.from = f.to;
                    }
                    (f.left, f.rect) = (Some(now), None);
                    // the pill opens over it: it stays where it hangs
                    f.held = opening.then_some(-DROP_GAP - DROP_D / 2.0 - (w + slot) / 2.0);
                }
                if f.out(now) < 0.02 {
                    self.fill = None;
                }
            }
            (None, None) => {}
        }
    }

    /// What the drop was filling for has ended and `card` says how: when that card is the next to show, the drop itself
    /// goes on to become it, in the colour of the end.
    pub(super) fn hand_drop(&mut self, source: &Source, card: u32, tint: (u8, u8, u8), shake: bool) {
        let hangs = self.fill.as_ref().is_some_and(|f| f.source == *source && f.left.is_none() && f.held.is_none() && f.out(self.now) > 0.9);
        // behind other cards, or in the queue, it could not get there
        if !hangs || !self.cards.stack.is_empty() || self.notif.as_ref().is_none_or(|n| n.id != card) {
            return;
        }
        let from = (-self.spring.w.pos / 2.0 - DROP_GAP - DROP_D / 2.0, -self.spring.h.pos / 2.0);
        self.fx.arrivals.push(effects::Arrival { card, from, delay_ms: 0, tint: Some(tint), shake });
        self.fill = None;
    }

    pub(super) fn fill_moving(&self) -> bool {
        self.fill.as_ref().is_some_and(|f| f.moving(self.now))
    }

    /// A drop that sloshes is hanging (it needs its own frames).
    pub(super) fn fill_sloshing(&self) -> bool {
        self.fill.as_ref().is_some_and(|f| f.busy && f.left.is_none() && f.held.is_none())
    }
}
