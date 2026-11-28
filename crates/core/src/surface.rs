//! The edges of the pill as the surface of a liquid. Points along each edge pull on their neighbours, are drawn back to
//! rest and slowly lose their motion; the sound gives them pushes, low notes towards the middle and high ones towards
//! the ends, and so does a drop that leaves or comes back. What follows (waves that run, meet, turn back at the ends and die down) is
//! not drawn up beforehand.

use crate::sky::chance;

/// The farthest an edge goes out of the pill, at the bottom and at the top (there is little between the pill and the top
/// of the screen).
pub const SWELL: f64 = 7.0;
pub const CREST: f64 = 3.5;
/// The farthest it goes in, so that it stays clear of what is written.
const DENT: f64 = 2.5;
const TOP_DENT: f64 = 1.5;
/// The points that move along an edge; the ones past both ends stay put.
const POINTS: usize = 48;
/// The longest step of the motion taken at once.
const STEP_MS: f64 = 4.0;
/// How hard a point is pulled towards its neighbours (waves run some sixty points a second) and back to rest (about
/// two swings a second).
const TENSION: f64 = 3600.0;
const SPRING: f64 = 150.0;
/// How fast motion is lost, and how much faster for the small ripples than for the broad waves.
const DAMPING: f64 = 4.5;
const VISCOSITY: f64 = 45.0;
/// The push of a note that has just struck, and the steady one of the loudness.
const KICK: f64 = 150.0;
const PRESSURE: f64 = 150.0;
/// The bands heard: where along the edge each one pushes, how broadly, and how hard.
const BANDS: [(f64, f64, f64); 8] = [(0.50, 5.0, 1.0), (0.37, 4.5, 0.9), (0.65, 4.0, 0.8), (0.26, 3.5, 0.75), (0.76, 3.2, 0.7), (0.17, 3.0, 0.65), (0.86, 2.8, 0.6), (0.11, 2.8, 0.55)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
}

/// One edge: how far out each point is, and how fast it goes.
#[derive(Clone, Debug)]
struct Line {
    out: [f64; POINTS],
    speed: [f64; POINTS],
}

impl Default for Line {
    fn default() -> Self {
        Line { out: [0.0; POINTS], speed: [0.0; POINTS] }
    }
}

impl Line {
    /// A push around `at` (0 to 1 along the edge), `broad` points wide. Part of it is taken back all along the edge, as a
    /// liquid that rises in one place sinks in the others.
    fn push(&mut self, at: f64, broad: f64, strength: f64) {
        let centre = at * (POINTS - 1) as f64;
        let shape: [f64; POINTS] = std::array::from_fn(|i| (-((i as f64 - centre) / broad).powi(2)).exp());
        let mean = shape.iter().sum::<f64>() / POINTS as f64;
        for (speed, s) in self.speed.iter_mut().zip(shape) {
            *speed += strength * (s - 0.85 * mean);
        }
    }

    fn step(&mut self, seconds: f64, pressure: f64) {
        let (out, speed) = (self.out, self.speed);
        for i in 0..POINTS {
            let beside = |of: &[f64; POINTS]| if i > 0 { of[i - 1] } else { 0.0 } + if i + 1 < POINTS { of[i + 1] } else { 0.0 } - 2.0 * of[i];
            let along = (std::f64::consts::PI * (i + 1) as f64 / (POINTS + 1) as f64).sin();
            let pull = TENSION * beside(&out) - SPRING * out[i] - DAMPING * speed[i] + VISCOSITY * beside(&speed) + pressure * along;
            // the new speed moves the point: this keeps the swinging from growing by itself
            self.speed[i] += pull * seconds;
            self.out[i] += self.speed[i] * seconds;
        }
    }

    /// How far out the edge is at `along`, smoothly between the points.
    fn at(&self, along: f64) -> f64 {
        // 0 and POINTS + 1 are the ends that stay put
        let x = along.clamp(0.0, 1.0) * (POINTS + 1) as f64;
        let i = (x.floor() as isize).min(POINTS as isize);
        let t = x - i as f64;
        let point = |n: isize| if (1..=POINTS as isize).contains(&n) { self.out[(n - 1) as usize] } else { 0.0 };
        let (a, b, c, d) = (point(i - 1), point(i), point(i + 1), point(i + 2));
        b + 0.5 * t * (c - a + t * (2.0 * a - 5.0 * b + 4.0 * c - d + t * (3.0 * (b - c) + d - a)))
    }

    fn quiet(&self) -> bool {
        self.out.iter().all(|o| o.abs() < 0.08) && self.speed.iter().all(|s| s.abs() < 1.0)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Surface {
    top: Line,
    bottom: Line,
    /// The level of each band of late: a push comes from standing above it.
    usual: [f32; 8],
    loudness: f64,
    heard: u64,
}

impl Surface {
    /// A new set of levels of the sound, low bands first.
    pub fn hear(&mut self, levels: &[f32]) {
        self.heard = self.heard.wrapping_add(1);
        let mut sum = 0.0;
        for (n, ((&level, usual), (at, broad, weight))) in levels.iter().zip(self.usual.iter_mut()).zip(BANDS).enumerate() {
            let struck = f64::from(level - *usual - 0.04).max(0.0);
            *usual += (level - *usual) * 0.15;
            sum += f64::from(level);
            if struck > 0.0 {
                // never twice in the very same spot
                let here = at + 0.07 * (chance(self.heard, n as u64 + 71) - 0.5);
                self.bottom.push(here, broad, KICK * weight * struck);
                self.top.push(1.0 - here, broad, 0.6 * KICK * weight * struck);
            }
        }
        self.loudness = sum / levels.len().clamp(1, BANDS.len()) as f64;
    }

    /// Something hits the bottom edge, or lets go of it, around `at` (0 to 1 along it) over `broad` of its length:
    /// outwards when `speed` is above zero. The top edge feels it too, less and the other way.
    pub fn strike(&mut self, at: f64, broad: f64, speed: f64) {
        let broad = (broad * POINTS as f64).clamp(2.5, POINTS as f64 / 4.0);
        self.bottom.push(at.clamp(0.0, 1.0), broad, speed);
        self.top.push(at.clamp(0.0, 1.0), broad * 1.3, -0.3 * speed);
    }

    /// Moves on by `ms`.
    pub fn step(&mut self, ms: f64) {
        let mut left = ms.clamp(0.0, 100.0);
        while left > 0.0 {
            let now = left.min(STEP_MS);
            left -= now;
            // the loudness is forgotten soon after nothing more is heard
            self.loudness *= (-now / 220.0).exp();
            self.top.step(now / 1000.0, 0.5 * PRESSURE * self.loudness);
            self.bottom.step(now / 1000.0, PRESSURE * self.loudness);
        }
        if self.still() {
            (self.top, self.bottom, self.loudness) = Default::default();
        }
    }

    /// Nothing moves, nor is about to.
    pub fn still(&self) -> bool {
        self.loudness < 0.02 && self.top.quiet() && self.bottom.quiet()
    }

    /// How far out of the pill an edge is at `along` (0 to 1), in pixels; negative: in. It nears its limits softly.
    pub fn out(&self, edge: Edge, along: f64) -> f64 {
        let (line, most, least) = match edge {
            Edge::Top => (&self.top, CREST, TOP_DENT),
            Edge::Bottom => (&self.bottom, SWELL, DENT),
        };
        let o = line.at(along);
        let limit = if o >= 0.0 { most } else { least };
        limit * (o / limit).tanh()
    }

    /// Where an edge of a pill `w` by `h` with corners of `radius` is at `x`: at rest, and now.
    pub fn edge_at(&self, edge: Edge, x: f64, w: f64, h: f64, radius: f64) -> (f64, f64) {
        // the corners curve, and stay: the moving part starts on them
        let from_corner = (radius - x.min(w - x)).clamp(0.0, radius);
        let round = (radius * radius - from_corner * from_corner).max(0.0).sqrt();
        let (rest, way) = match edge {
            Edge::Top => (radius - round, -1.0),
            Edge::Bottom => (h - radius + round, 1.0),
        };
        let (start, straight) = (radius * 0.6, w - 2.0 * radius * 0.6);
        let along = (x - start) / straight;
        if straight <= 8.0 || !(0.0..=1.0).contains(&along) {
            return (rest, rest);
        }
        (rest, rest + way * self.out(edge, along))
    }

    /// The whole outline of the pill as it is now: along the top from left to right, then back along the bottom.
    pub fn outline(&self, w: f64, h: f64, radius: f64) -> Vec<(f64, f64)> {
        if w <= 0.0 || h <= 0.0 {
            return vec![];
        }
        let radius = radius.clamp(0.0, (w / 2.0).min(h / 2.0));
        let start = radius * 0.6;
        // closer together on the corners, by their angle, so that they stay round
        const ON_A_CORNER: u32 = 12;
        let reach = if radius > 0.0 { (1.0 - start / radius).acos() } else { 0.0 };
        let corner: Vec<f64> = (0..ON_A_CORNER).map(|n| radius * (1.0 - (reach * f64::from(n) / f64::from(ON_A_CORNER)).cos())).collect();
        let steps = ((w - 2.0 * start).max(0.0) / 3.0).ceil().max(1.0) as u32;
        let mut xs = corner.clone();
        xs.extend((0..=steps).map(|n| start + (w - 2.0 * start) * f64::from(n) / f64::from(steps)));
        xs.extend(corner.iter().rev().map(|x| w - x));
        // the ends of a pill whose corners do not reach its middle are straight sides
        let top = xs.iter().map(|&x| (x, self.edge_at(Edge::Top, x, w, h, radius).1));
        let bottom = xs.iter().rev().map(|&x| (x, self.edge_at(Edge::Bottom, x, w, h, radius).1));
        top.chain(bottom).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KICKED: [f32; 8] = [0.9, 0.85, 0.3, 0.2, 0.2, 0.1, 0.1, 0.1];

    /// The farthest out and in the bottom edge gets over `ms`, a frame at a time.
    fn reach(s: &mut Surface, ms: u32) -> (f64, f64) {
        let (mut most, mut least) = (0.0f64, 0.0f64);
        for _ in 0..ms / 33 {
            s.step(33.0);
            for n in 0..=40 {
                let o = s.out(Edge::Bottom, f64::from(n) / 40.0);
                (most, least) = (most.max(o), least.min(o));
            }
        }
        (most, least)
    }

    #[test]
    fn a_note_that_strikes_pushes_the_edge_which_swings_and_comes_to_rest() {
        let mut s = Surface::default();
        assert!(s.still());
        s.hear(&KICKED);
        assert!(!s.still());
        let (out, inward) = reach(&mut s, 600);
        println!("one beat: out {out:.2} in {inward:.2}");
        assert!(out > 3.0 && out <= SWELL, "{out}");
        assert!((-DENT..-0.4).contains(&inward), "it sinks as well as rises: {inward}");
        let later = reach(&mut s, 400);
        assert!(later.0 < out, "{later:?}");
        reach(&mut s, 3_000);
        assert!(s.still());
        assert_eq!((s.out(Edge::Bottom, 0.5), s.out(Edge::Top, 0.5)), (0.0, 0.0));
    }

    #[test]
    fn a_drop_that_leaves_or_comes_back_ripples_it_from_there() {
        for speed in [-220.0, 220.0] {
            let mut s = Surface::default();
            s.strike(0.5, 0.1, speed);
            assert!(!s.still());
            s.step(60.0);
            let here = s.out(Edge::Bottom, 0.5);
            assert!(here * speed > 0.0 && here.abs() > 1.2, "{here}");
            assert!(s.out(Edge::Bottom, 0.05).abs() < here.abs() / 2.0, "from there");
            for _ in 0..120 {
                s.step(33.0);
            }
            assert!(s.still());
        }
        // also from off the end of the edge
        let mut s = Surface::default();
        s.strike(7.0, 5.0, 200.0);
        s.step(100.0);
        assert!(s.out(Edge::Bottom, 0.9).is_finite() && !s.still());
    }

    #[test]
    fn it_never_leaves_its_limits_nor_runs_wild_however_loud_and_long() {
        let mut s = Surface::default();
        for n in 0..3000 {
            let on = if n % 4 < 2 { 1.0 } else { 0.0 };
            s.hear(&[on; 8]);
            s.step(if n % 100 == 0 { 5_000.0 } else { 33.0 });
            for edge in [Edge::Top, Edge::Bottom] {
                for k in 0..=20 {
                    let o = s.out(edge, f64::from(k) / 20.0);
                    let (most, least) = if edge == Edge::Top { (CREST, TOP_DENT) } else { (SWELL, DENT) };
                    assert!(o.is_finite() && o <= most && o >= -least, "{o}");
                }
            }
        }
    }

    #[test]
    fn low_notes_push_in_the_middle_high_ones_towards_the_ends_and_the_wave_runs() {
        let one = |band: usize| {
            let mut s = Surface::default();
            let mut levels = [0.0f32; 8];
            levels[band] = 1.0;
            s.hear(&levels);
            s.step(33.0);
            s
        };
        let highest = |s: &Surface| (0..=100).map(|n| f64::from(n) / 100.0).max_by(|a, b| s.out(Edge::Bottom, *a).total_cmp(&s.out(Edge::Bottom, *b))).unwrap();
        assert!((highest(&one(0)) - 0.5).abs() < 0.1);
        let high = one(7);
        assert!(highest(&high) < 0.25, "{}", highest(&high));
        // further along, a moment later
        let mut running = high.clone();
        running.step(100.0);
        running.step(100.0);
        assert!(highest(&running) > highest(&high) + 0.12, "{} then {}", highest(&high), highest(&running));
    }

    #[test]
    fn the_outline_is_the_pill_at_rest_and_leaves_it_only_along_the_straight() {
        let calm = Surface::default();
        let o = calm.outline(200.0, 28.0, 14.0);
        assert!(o.len() > 100 && o.iter().all(|&(x, y)| (0.0..=200.0).contains(&x) && (0.0..=28.0).contains(&y)));
        for &(x, y) in &o {
            // on a circle at the ends, on a side in between
            let from_end = x.min(200.0 - x);
            if from_end < 14.0 {
                assert!(((from_end - 14.0).hypot(y - 14.0) - 14.0).abs() < 1e-6, "{x} {y}");
            } else {
                assert!(y.abs() < 1e-9 || (y - 28.0).abs() < 1e-9, "{x} {y}");
            }
        }
        let mut s = Surface::default();
        s.hear(&KICKED);
        s.step(100.0);
        let moved = s.outline(200.0, 28.0, 14.0);
        assert_eq!(moved.len(), o.len());
        assert_ne!(moved, o);
        assert_eq!((moved[0], s.edge_at(Edge::Bottom, 2.0, 200.0, 28.0, 14.0).0), (o[0], s.edge_at(Edge::Bottom, 2.0, 200.0, 28.0, 14.0).1), "the ends stay");
        assert!(calm.outline(0.0, 28.0, 14.0).is_empty() && !s.outline(10.0, 28.0, 14.0).is_empty());
    }
}
