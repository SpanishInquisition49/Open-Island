//! A notification card as liquid: a drop forms in the island, falls on a neck that pinches off, spreads into a
//! capsule below it and, on the way out, gathers round again and is swallowed.
//!
//! A port of `Spring.swift`, `Droplet.swift` and the animator of `Island.swift` from drip
//! (<https://github.com/jrnxf/drip>, MIT License, Copyright (c) 2026 Colby Thomas): the same springs, the same smooth
//! union of signed distance fields traced where it crosses zero, the same sizes. What differs: the drop comes out of
//! the island's pill instead of a screen notch, so it starts hidden inside it and melts in.
//!
//! Coordinates are logical pixels with x = 0 at the middle of the pill and y growing down from its **bottom** edge.
//! Times are seconds since the card was born. Everything is a closed form of the time: no state steps with the frames.

use std::collections::HashMap;
use std::f64::consts::PI;

/// The round drop a capsule starts and ends as.
pub const DROP: f64 = 26.0;
/// The drop's size when it starts to form.
pub const SEED: f64 = 12.0;
/// How far apart the drop and the pill still pull on each other (the neck pinches off at a third of it).
pub const BLEND: f64 = 24.0;
/// The room between the pill's bottom and the first open capsule.
pub const GAP: f64 = 12.0;
/// The room between two cards of the stack.
pub const STACK_GAP: f64 = 10.0;
/// How far open a capsule is when its text starts to fade in.
pub const TEXT_OPEN: f64 = 0.8;
/// Each wrapped line fades in this long after the line above it.
pub const LINE_STAGGER: f64 = 0.03;
/// How long the content takes to fade out on the exit.
pub const FADE_OUT: f64 = 0.12;

/// A spring described by its perceptual duration and bounce (`CASpringAnimation`'s, as drip uses it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Curve {
    pub duration: f64,
    pub bounce: f64,
}

impl Curve {
    pub const fn new(duration: f64, bounce: f64) -> Self {
        Self { duration, bounce }
    }

    /// 0 at the start, 1 at rest (a bouncy one passes 1 on the way), `t` seconds after it started.
    pub fn progress(self, t: f64) -> f64 {
        if t.is_nan() || t <= 0.0 {
            return 0.0;
        }
        if !t.is_finite() {
            return 1.0;
        }
        let natural = 2.0 * PI / self.duration;
        let ratio = 1.0 - self.bounce.clamp(0.0, 0.99);
        if ratio >= 1.0 {
            return 1.0 - (-natural * t).exp() * (1.0 + natural * t);
        }
        let damped = natural * (1.0 - ratio * ratio).sqrt();
        let decay = (-ratio * natural * t).exp();
        1.0 - decay * ((damped * t).cos() + ratio * natural / damped * (damped * t).sin())
    }
}

const FALL: Curve = Curve::new(0.62, 0.2);
const FEED: Curve = Curve::new(0.3, 0.0);
const MELT_IN: Curve = Curve::new(0.08, 0.0);
const LET_GO: Curve = Curve::new(0.35, 0.0);
const GROW: Curve = Curve::new(0.5, 0.15);
const GATHER: Curve = Curve::new(0.22, 0.0);
const GATHER_DELAY: f64 = 0.03;
const RISE: Curve = Curve::new(0.4, 0.0);
const RESIZE: Curve = Curve::new(0.4, 0.1);
/// How a card of the stack glides into a gap.
const SETTLE: Curve = Curve::new(0.45, 0.12);
/// How long after the drop starts to rise it is gone for good.
const SWALLOWED_AFTER: f64 = 0.45;

/// A value that springs from target to target: a new target adds a spring to those still running, so an
/// interrupted motion keeps its velocity.
#[derive(Clone, Debug)]
struct Value {
    from: f64,
    target: f64,
    springs: Vec<(f64, Curve, f64)>, // (change, curve, start)
}

impl Value {
    fn new(v: f64) -> Self {
        Self { from: v, target: v, springs: Vec::new() }
    }

    fn animate(&mut self, to: f64, curve: Curve, at: f64) {
        let change = to - self.target;
        if change != 0.0 {
            self.springs.push((change, curve, at));
            self.target = to;
        }
    }

    fn at(&self, t: f64) -> f64 {
        self.from + self.springs.iter().map(|(d, c, start)| d * c.progress(t - start)).sum::<f64>()
    }
}

/// The sizes of the open capsule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub w: f64,
    pub h: f64,
    /// The largest corner radius: a card of two lines is round at the ends, a taller one keeps corners of 30.
    pub corner: f64,
}

/// The card at one instant.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    /// Its top edge, below the pill's bottom (negative while inside the pill).
    pub top: f64,
    pub w: f64,
    pub h: f64,
    pub radius: f64,
    /// 0 a round drop, 1 the open capsule.
    pub opened: f64,
    /// How strongly the drop and the pill melt together; 0 keeps them apart.
    pub blend: f64,
    /// The width the content is laid out for (it springs along when the card changes size).
    pub content_w: f64,
}

/// What a drop comes out of and goes back into: the pill, or the card above it in the stack. Where it rests.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub bottom: f64,
    pub height: f64,
}

impl Anchor {
    /// A pill `height` tall (its bottom is the origin).
    pub fn pill(height: f64) -> Self {
        Self { bottom: 0.0, height }
    }

    /// Where the middle of a drop is that hides just inside the bottom edge.
    fn inside(self, room: f64) -> f64 {
        self.bottom - self.height.min(room) / 2.0
    }
}

#[derive(Clone, Debug)]
pub struct Animator {
    size: Size,
    /// Where the open capsule's top is heading.
    top: f64,
    /// When the gathered drop starts to rise on the way out.
    rise: Option<f64>,
    centre: Value,
    w: Value,
    h: Value,
    opened: Value,
    blend: Value,
    content_w: Value,
    /// When the content starts to fade in.
    cue: f64,
    left_at: Option<f64>,
    end: f64,
}

/// The first time from `start`, in 1 ms steps and within 2 s, at which `done` holds.
fn first_time(start: f64, done: impl Fn(f64) -> bool) -> f64 {
    let mut t = start;
    while t < start + 2.0 && !done(t) {
        t += 0.001;
    }
    t
}

impl Animator {
    /// A card born at time 0 inside `from`, which falls and opens to `size` with its top at `top`.
    pub fn enter(size: Size, from: Anchor, top: f64) -> Self {
        let start = from.inside(DROP + 2.0);
        let end_centre = top + size.h / 2.0;
        let mut centre = Value::new(start);
        let (mut w, mut h) = (Value::new(SEED), Value::new(SEED));
        let (mut opened, mut blend) = (Value::new(0.0), Value::new(0.0));
        w.animate(DROP, FEED, 0.0);
        h.animate(DROP, FEED, 0.0);
        centre.animate(end_centre, FALL, 0.0);
        blend.animate(BLEND, MELT_IN, 0.0);
        // the neck pinches off when the drop is a third of the blend below what it hangs from; only then does it spread
        let loose = first_time(0.0, |t| start + (end_centre - start) * FALL.progress(t) - DROP / 2.0 >= from.bottom + BLEND / 3.0);
        blend.animate(0.0, LET_GO, loose);
        w.animate(size.w, GROW, loose);
        h.animate(size.h, GROW, loose);
        opened.animate(1.0, GROW, loose);
        let cue = first_time(loose, |t| GROW.progress(t - loose) >= TEXT_OPEN);
        Self { size, top, rise: None, centre, w, h, opened, blend, content_w: Value::new(size.w), cue, left_at: None, end: loose + 0.8 }
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// When the icon and the first line start to fade in.
    pub fn cue(&self) -> f64 {
        self.cue
    }

    pub fn left_at(&self) -> Option<f64> {
        self.left_at
    }

    /// When nothing moves any more (after an exit: when the card is gone).
    pub fn end(&self) -> f64 {
        self.end
    }

    /// Where the open capsule's top is heading.
    pub fn top(&self) -> f64 {
        self.top
    }

    /// When the drop of a card on its way out starts to rise: until then it keeps its place in the stack.
    pub fn rise_at(&self) -> Option<f64> {
        self.rise
    }

    /// Glides the open capsule so that its top is at `top`: the stack closing a gap or making room.
    pub fn move_to(&mut self, top: f64, at: f64) {
        if top == self.top || self.left_at.is_some() {
            return;
        }
        self.top = top;
        self.centre.animate(top + self.size.h / 2.0, SETTLE, at);
        self.end = self.end.max(at + 0.8);
    }

    /// Springs the open capsule to a new size, its top held where it is.
    pub fn resize(&mut self, size: Size, at: f64) {
        if size == self.size || self.left_at.is_some() {
            return;
        }
        self.size = size;
        self.centre.animate(self.top + size.h / 2.0, RESIZE, at);
        self.w.animate(size.w, RESIZE, at);
        self.h.animate(size.h, RESIZE, at);
        self.content_w.animate(size.w, RESIZE, at);
        self.end = self.end.max(at + 0.7);
    }

    /// Gathers the capsule into a drop, which then rises into `into`.
    pub fn exit(&mut self, at: f64, into: Anchor) {
        if self.left_at.is_some() {
            return;
        }
        self.left_at = Some(at);
        let gather = at + GATHER_DELAY;
        self.w.animate(DROP, GATHER, gather);
        self.h.animate(DROP, GATHER, gather);
        self.opened.animate(0.0, GATHER, gather);
        let (w, h) = (self.w.clone(), self.h.clone());
        let rise = first_time(gather, |t| (w.at(t) - DROP).abs().max((h.at(t) - DROP).abs()) <= 2.0);
        self.blend.animate(BLEND, Curve::new(0.3, 0.0), rise);
        // aimed past its resting place, so that it is swallowed at speed instead of easing in
        self.centre.animate(into.inside(DROP + 4.0), RISE, rise);
        self.blend.animate(0.0, Curve::new(0.25, 0.0), rise + 0.2);
        self.rise = Some(rise);
        self.end = rise + SWALLOWED_AFTER;
    }

    /// Aims a drop on its way out somewhere else: what it was going into moved, or left too.
    pub fn redirect(&mut self, into: Anchor, at: f64) {
        let Some(rise) = self.rise else { return };
        let at = at.max(rise);
        self.centre.animate(into.inside(DROP + 4.0), RISE, at);
        self.end = self.end.max(at + SWALLOWED_AFTER);
    }

    pub fn shape(&self, t: f64) -> Shape {
        let opened = self.opened.at(t);
        // a moving drop stretches along its path and keeps its area; the capsule stays rigid
        let step = 1.0 / 240.0;
        let speed = (self.centre.at(t + step) - self.centre.at(t - step)).abs() / (2.0 * step);
        let stretch = 1.0 + (speed / 3000.0).min(0.07) * (1.0 - opened).max(0.0);
        let h = (self.h.at(t) * stretch).max(1.0);
        let w = (self.w.at(t) / stretch).max(1.0);
        let radius = (w / 2.0).min(h / 2.0).min(self.size.corner);
        Shape { top: self.centre.at(t) - h / 2.0, w, h, radius, opened, blend: self.blend.at(t).max(0.0), content_w: self.content_w.at(t) }
    }

    /// How visible a part of the content is that comes `delay` after the cue and takes `over` to fade in.
    pub fn content_opacity(&self, t: f64, delay: f64, over: f64) -> f64 {
        let k = ((t - self.cue - delay) / over).clamp(0.0, 1.0);
        let shown = 1.0 - (1.0 - k).powi(3);
        match self.left_at {
            Some(at) => shown.min(1.0 - ((t - at) / FADE_OUT).clamp(0.0, 1.0)),
            None => shown,
        }
    }
}

/// A rounded box.
#[derive(Clone, Copy, Debug)]
pub struct Blob {
    pub cx: f64,
    pub cy: f64,
    pub half_w: f64,
    pub half_h: f64,
    pub radius: f64,
}

impl Blob {
    /// Signed distance, negative inside.
    pub fn distance(&self, x: f64, y: f64) -> f64 {
        let r = self.radius.min(self.half_w).min(self.half_h);
        let qx = (x - self.cx).abs() - self.half_w + r;
        let qy = (y - self.cy).abs() - self.half_h + r;
        qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
    }
}

/// The cubic smooth minimum: a plain union far apart, a fillet of about `blend` close up.
pub fn smooth_min(a: f64, b: f64, blend: f64) -> f64 {
    if blend <= 0.0 {
        return a.min(b);
    }
    let h = (blend - (a - b).abs()).max(0.0) / blend;
    a.min(b) - h * h * h * blend / 6.0
}

/// The outline of `anchor` (the pill, or the card above) melted with the drop, as closed loops: only the part near the
/// drop is traced, from a line inside the anchor down; the anchor drawn on its own covers the cut. None when the two
/// are too far apart to melt (the drop is then a plain rounded rectangle).
pub fn neck(anchor: &Blob, s: &Shape, cell: f64) -> Option<Vec<Vec<(f64, f64)>>> {
    let floor = anchor.cy + anchor.half_h;
    if s.blend <= 0.01 || s.top >= floor + s.blend || s.top + s.h <= floor - s.blend || cell <= 0.0 {
        return None;
    }
    let drop = Blob { cx: 0.0, cy: s.top + s.h / 2.0, half_w: s.w / 2.0, half_h: s.h / 2.0, radius: s.radius };
    // just inside the anchor, unless the melt reaches its round ends: then from above everything the drop changes
    let inside = floor - 4.0;
    let straight = anchor.half_w - anchor.radius.min(anchor.half_w).min(anchor.half_h);
    let top = if s.w / 2.0 + s.blend > straight { (s.top - s.blend - 1.0).min(inside).max(anchor.cy - anchor.half_h) } else { inside };
    let half = s.w / 2.0 + s.blend + 2.0 * cell;
    let bottom = (s.top + s.h).max(floor) + s.blend + 2.0 * cell;
    let columns = (2.0 * half / cell).ceil() as usize + 1;
    let rows = ((bottom - top) / cell).ceil() as usize + 1;
    let x0 = -((columns - 1) as f64) * cell / 2.0;
    // the border is held outside, so every loop closes
    let mut values = vec![1.0; columns * rows];
    for row in 1..rows.saturating_sub(1) {
        let y = top + row as f64 * cell;
        for column in 1..columns - 1 {
            let x = x0 + column as f64 * cell;
            values[row * columns + column] = smooth_min(anchor.distance(x, y), drop.distance(x, y), s.blend);
        }
    }
    Some(loops(&values, columns, rows).into_iter().map(|l| l.into_iter().map(|(x, y)| (x0 + x * cell, top + y * cell)).collect()).collect())
}

/// The same melt sideways: a drop leaving the right end of `anchor`. Both are in coordinates with y = 0 at their
/// common middle line, and `s` runs along x: `s.top` is its left edge, `s.h` its width, `s.w` its height.
pub fn neck_sideways(anchor: &Blob, s: &Shape, cell: f64) -> Option<Vec<Vec<(f64, f64)>>> {
    // the same problem lying down: swap the axes, solve it upright, swap them back
    let upright = Blob { cx: anchor.cy, cy: anchor.cx, half_w: anchor.half_h, half_h: anchor.half_w, radius: anchor.radius };
    Some(neck(&upright, s, cell)?.into_iter().map(|l| l.into_iter().map(|(x, y)| (y, x)).collect()).collect())
}

/// A grid edge: its first corner and its direction.
type Edge = (usize, usize, bool); // (row, column, vertical)

/// Marching squares: the zero crossings of a sampled field as closed loops, in grid units. The border must be outside.
fn loops(values: &[f64], columns: usize, rows: usize) -> Vec<Vec<(f64, f64)>> {
    let value = |column: usize, row: usize| values[row * columns + column];
    let mut links: HashMap<Edge, Vec<Edge>> = HashMap::new();
    let mut order: Vec<Edge> = Vec::new();
    let mut link = |a: Edge, b: Edge| {
        for (from, to) in [(a, b), (b, a)] {
            let list = links.entry(from).or_default();
            if list.is_empty() {
                order.push(from);
            }
            list.push(to);
        }
    };
    for row in 0..rows.saturating_sub(1) {
        for column in 0..columns.saturating_sub(1) {
            let corners = [value(column, row), value(column + 1, row), value(column + 1, row + 1), value(column, row + 1)];
            let index = corners.iter().enumerate().filter(|(_, c)| **c < 0.0).fold(0, |i, (bit, _)| i | 1 << bit);
            if index == 0 || index == 15 {
                continue;
            }
            let (top, right) = ((row, column, false), (row, column + 1, true));
            let (bottom, left) = ((row + 1, column, false), (row, column, true));
            match index {
                1 | 14 => link(left, top),
                2 | 13 => link(top, right),
                3 | 12 => link(left, right),
                4 | 11 => link(right, bottom),
                6 | 9 => link(top, bottom),
                7 | 8 => link(left, bottom),
                // a saddle: the middle of the cell decides whether the two insides connect
                _ => {
                    if (index == 5) == (corners.iter().sum::<f64>() < 0.0) {
                        link(left, bottom);
                        link(top, right);
                    } else {
                        link(left, top);
                        link(right, bottom);
                    }
                }
            }
        }
    }
    let point = |(row, column, vertical): Edge| {
        let a = value(column, row);
        let b = if vertical { value(column, row + 1) } else { value(column + 1, row) };
        let t = a / (a - b);
        if vertical { (column as f64, row as f64 + t) } else { (column as f64 + t, row as f64) }
    };
    let mut visited = std::collections::HashSet::new();
    let mut out = Vec::new();
    for start in order {
        let (mut current, mut previous) = (start, None);
        let mut one = Vec::new();
        while visited.insert(current) {
            one.push(point(current));
            let Some(next) = links[&current].iter().copied().find(|n| Some(*n) != previous && !visited.contains(n)) else { break };
            previous = Some(current);
            current = next;
        }
        if one.len() > 2 {
            out.push(one);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drop_leaving_sideways_melts_into_the_end_it_leaves() {
        // a pill 150 x 28 from x = 0, a drop just past its right end
        let pill = Blob { cx: 75.0, cy: 0.0, half_w: 75.0, half_h: 14.0, radius: 14.0 };
        let drop = Shape { top: 154.0, w: 28.0, h: 28.0, radius: 14.0, opened: 0.0, blend: 18.0, content_w: 28.0 };
        let loops = neck_sideways(&pill, &drop, 0.7).expect("close enough to melt");
        let points: Vec<(f64, f64)> = loops.concat();
        let right = points.iter().map(|p| p.0).fold(f64::MIN, f64::max);
        assert!(right > 178.0 && right < 190.0, "it reaches round the drop: {right}");
        assert!(points.iter().all(|p| p.1.abs() < 24.0), "and stays about the pill's height");
        assert!(loops.len() == 1, "one body while joined");
        assert!(neck_sideways(&pill, &Shape { top: 200.0, ..drop }, 0.7).is_none(), "too far to pull on each other");
    }

    const SIZE: Size = Size { w: 300.0, h: 52.0, corner: f64::INFINITY };
    const PILL: Blob = Blob { cx: 0.0, cy: -14.0, half_w: 75.0, half_h: 14.0, radius: 14.0 };

    #[test]
    fn a_spring_starts_at_zero_ends_at_one_and_bounces_only_when_asked() {
        for c in [FALL, FEED, GROW, RISE] {
            assert_eq!(c.progress(-1.0), 0.0);
            assert_eq!(c.progress(0.0), 0.0);
            assert!((c.progress(3.0) - 1.0).abs() < 1e-3, "{c:?}");
            assert_eq!(c.progress(f64::INFINITY), 1.0);
            assert_eq!(c.progress(f64::NAN), 0.0);
            let peak = (0..1000).map(|i| c.progress(i as f64 / 500.0)).fold(0.0, f64::max);
            assert_eq!(peak > 1.001, c.bounce > 0.0, "{c:?} peaks at {peak}");
        }
    }

    #[test]
    fn the_drop_starts_inside_the_pill_and_ends_as_the_open_capsule_below_it() {
        let a = Animator::enter(SIZE, Anchor::pill(28.0), GAP);
        let s = a.shape(0.0);
        assert!(s.top >= -28.0 && s.top + s.h <= 0.0, "hidden whole inside the pill: {s:?}");
        let s = a.shape(a.end() + 1.0);
        assert!((s.top - GAP).abs() < 0.1 && (s.w - SIZE.w).abs() < 0.1 && (s.h - SIZE.h).abs() < 0.1, "{s:?}");
        assert!(s.blend < 0.01 && (s.opened - 1.0).abs() < 1e-3);
        assert!(a.cue() > 0.1 && a.cue() < a.end(), "the text waits for the capsule to open: {}", a.cue());
        assert_eq!(a.content_opacity(0.0, 0.0, 0.2), 0.0);
        assert_eq!(a.content_opacity(a.end(), 0.0, 0.2), 1.0);
    }

    #[test]
    fn leaving_gathers_the_capsule_and_hides_it_in_the_pill() {
        let mut a = Animator::enter(SIZE, Anchor::pill(28.0), GAP);
        a.exit(3.0, Anchor::pill(28.0));
        assert!(a.end() > 3.0 && a.end() < 4.5, "{}", a.end());
        let s = a.shape(a.end());
        assert!(s.top + s.h < 2.0, "inside the pill again: {s:?}");
        assert!((s.w - DROP).abs() < 1.0);
        assert_eq!(a.content_opacity(3.0 + FADE_OUT + 0.01, 0.0, 0.2), 0.0);
    }

    #[test]
    fn leaving_halfway_in_does_not_jump() {
        let mut a = Animator::enter(SIZE, Anchor::pill(28.0), GAP);
        let before = a.shape(0.25);
        a.exit(0.25, Anchor::pill(28.0));
        let after = a.shape(0.2501);
        assert!((after.top - before.top).abs() < 0.5 && (after.w - before.w).abs() < 0.5, "{before:?} {after:?}");
        assert!(a.shape(a.end()).top + a.shape(a.end()).h < 2.0);
    }

    #[test]
    fn resizing_keeps_the_top_still() {
        let mut a = Animator::enter(SIZE, Anchor::pill(28.0), GAP);
        a.resize(Size { w: 420.0, h: 90.0, corner: 30.0 }, 2.0);
        for t in [2.05, 2.2, 3.5] {
            assert!((a.shape(t).top - GAP).abs() < 0.2, "{:?}", a.shape(t));
        }
        let s = a.shape(4.0);
        assert!((s.w - 420.0).abs() < 0.1 && (s.content_w - 420.0).abs() < 0.1 && s.radius == 30.0);
    }

    #[test]
    fn the_neck_joins_pill_and_drop_and_pinches_off() {
        let shape = |top: f64, blend: f64| Shape { top, w: DROP, h: DROP, radius: DROP / 2.0, opened: 0.0, blend, content_w: 0.0 };
        let joined = neck(&PILL, &shape(3.0, BLEND), 1.0).expect("close enough to melt");
        assert_eq!(joined.len(), 1, "one body");
        let apart = neck(&PILL, &shape(20.0, BLEND), 1.0).expect("still within the blend");
        assert_eq!(apart.len(), 2, "pinched off: the pill and the drop");
        assert!(neck(&PILL, &shape(40.0, BLEND), 1.0).is_none(), "too far to melt");
        assert!(neck(&PILL, &shape(3.0, 0.0), 1.0).is_none());
        for (x, y) in joined.into_iter().flatten() {
            assert!(x.is_finite() && y.is_finite() && x.abs() < 100.0 && (-5.0..60.0).contains(&y), "{x} {y}");
        }
    }

    #[test]
    fn a_wide_drop_against_a_narrow_pill_is_traced_without_a_step() {
        let s = Shape { top: -10.0, w: 140.0, h: 30.0, radius: 15.0, opened: 0.5, blend: BLEND, content_w: 0.0 };
        let l = neck(&PILL, &s, 1.0).unwrap();
        assert!(l.iter().flatten().any(|(_, y)| *y < -20.0), "traced from the pill's top");
    }

    #[test]
    fn a_second_card_drops_out_of_the_first_and_goes_back_into_it() {
        let first = Anchor { bottom: GAP + SIZE.h, height: SIZE.h };
        let top = first.bottom + STACK_GAP;
        let mut a = Animator::enter(SIZE, first, top);
        let s = a.shape(0.0);
        assert!(s.top >= GAP && s.top + s.h <= first.bottom, "hidden inside the card above: {s:?}");
        assert!((a.shape(a.end()).top - top).abs() < 0.2);
        let card = Blob { cx: 0.0, cy: GAP + SIZE.h / 2.0, half_w: SIZE.w / 2.0, half_h: SIZE.h / 2.0, radius: SIZE.h / 2.0 };
        assert!((0..200).any(|i| neck(&card, &a.shape(i as f64 * 0.004), 1.0).is_some_and(|l| l.len() == 1)), "a neck joins them on the way");
        a.exit(3.0, first);
        assert!(a.rise_at().is_some_and(|r| r > 3.0 && r < a.end()));
        let s = a.shape(a.end());
        assert!(s.top + s.h < first.bottom + 2.0 && s.top > GAP, "swallowed by the card above: {s:?}");
    }

    #[test]
    fn the_stack_closes_a_gap_and_a_rising_drop_follows_what_it_goes_into() {
        let mut a = Animator::enter(SIZE, Anchor::pill(28.0), 80.0);
        a.move_to(GAP, 2.0);
        assert_eq!(a.top(), GAP);
        assert!((a.shape(a.end()).top - GAP).abs() < 0.2);
        let mut b = Animator::enter(SIZE, Anchor { bottom: 64.0, height: 52.0 }, 74.0);
        b.exit(2.0, Anchor { bottom: 64.0, height: 52.0 });
        b.redirect(Anchor::pill(28.0), 2.05);
        let s = b.shape(b.end());
        assert!(s.top + s.h < 2.0, "it went on into the pill: {s:?}");
        b.move_to(500.0, 2.1);
        assert!(b.top() < 100.0, "a card on its way out is not moved about");
    }
}
