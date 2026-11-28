//! What the weather leaves on the pill: snow that piles up at the bottom and melts, and drops that stay on its glass
//! and run down. Pure, like the sky: how much there is and where it lies follow from the time.

use crate::sky::{self, chance};

/// How long snow takes to pile as high as it gets, and to melt away.
pub const SNOW_RISE_MS: i64 = 24_000;
pub const SNOW_MELT_MS: i64 = 20_000;
/// The same for the glass: to get wet, and to dry.
pub const WET_RISE_MS: i64 = 5_000;
pub const WET_DRY_MS: i64 = 8_000;
/// How high the snow gets: below what is written in the pill.
pub const SNOW_HIGH: f64 = 5.5;
/// The levels the snow goes through: between two it does not change, and nothing has to be drawn again.
const SNOW_STEPS: f64 = 32.0;
/// The width the counts below are for.
const PER_WIDTH: f64 = 150.0;

/// An amount, 0 to 1, that grows while its weather lasts and goes down after.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pile {
    /// What there was when it last turned, and when that was.
    had: f64,
    since: i64,
    pub rising: bool,
}

impl Pile {
    /// How much there is at `now`, taking `rise_ms` to fill and `fall_ms` to empty.
    pub fn level(&self, now: i64, rise_ms: i64, fall_ms: i64) -> f64 {
        let ms = (now - self.since).max(0) as f64;
        let change = if self.rising { ms / rise_ms as f64 } else { -ms / fall_ms as f64 };
        (self.had + change).clamp(0.0, 1.0)
    }

    /// Its weather starts or stops: it goes the other way from where it is.
    pub fn turn(&mut self, rising: bool, now: i64, rise_ms: i64, fall_ms: i64) {
        if rising != self.rising {
            *self = Pile { had: self.level(now, rise_ms, fall_ms), since: now, rising };
        }
    }

    /// It is still on its way up or down.
    pub fn changing(&self, now: i64, rise_ms: i64, fall_ms: i64) -> bool {
        let level = self.level(now, rise_ms, fall_ms);
        if self.rising {
            level < 1.0
        } else {
            level > 0.0
        }
    }
}

/// The top of the snow at the bottom of a pill `w` by `h`, from left to right: uneven, and always the same for the
/// same level. Empty while there is none to see.
pub fn bank(level: f64, w: f64, h: f64) -> Vec<(f64, f64)> {
    let level = (level.clamp(0.0, 1.0) * SNOW_STEPS).round() / SNOW_STEPS;
    if level <= 0.0 || w <= 0.0 || h <= 0.0 {
        return vec![];
    }
    // heaps of two sizes, one over the other
    let heaps = |x: f64, apart: f64, salt: u64| {
        let at = x / apart;
        let (i, t) = (at.floor() as u64, at.fract());
        let t = t * t * (3.0 - 2.0 * t);
        chance(i, salt) * (1.0 - t) + chance(i + 1, salt) * t
    };
    let steps = (w / 3.0).ceil() as u32;
    (0..=steps)
        .map(|n| {
            let x = w * f64::from(n) / f64::from(steps);
            let uneven = 0.55 + 0.3 * heaps(x, 23.0, 41) + 0.15 * heaps(x, 8.0, 42);
            (x, h - level * SNOW_HIGH * uneven)
        })
        .collect()
}

/// A drop on the glass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bead {
    pub x: f64,
    pub y: f64,
    pub radius: f64,
    pub alpha: f64,
    /// Where it has run down from, while it runs.
    pub trail: Option<f64>,
}

/// The time the drops on the glass are at, `since_ms` into the rain: it goes while the rain is seen to move, and stays
/// where the last stretch of it ended while it does not (on battery).
pub fn glass_time(since_ms: i64, on_power: bool) -> i64 {
    sky::stopped_at(since_ms, on_power)
}

/// The drops on a pill `w` by `h`, as wet as `level`, at `t_ms`: each forms, hangs there, runs down and is followed by
/// another elsewhere. As the glass dries they slide off one after the other.
pub fn beads(level: f64, t_ms: i64, w: f64, h: f64) -> Vec<Bead> {
    if level <= 0.0 || w <= 0.0 || h <= 0.0 {
        return vec![];
    }
    let many = ((7.0 * w / PER_WIDTH).round() as u64).clamp(2, 24);
    (0..many)
        .filter_map(|i| {
            // each waits for its share of wet
            let here = ((level - 0.85 * chance(i, 51)) / 0.15).clamp(0.0, 1.0);
            if here <= 0.0 {
                return None;
            }
            let period = 7000.0 + 6000.0 * chance(i, 52);
            let at = t_ms.max(0) as f64 / period + chance(i, 53);
            let (turn, p) = (at.floor() as u64, at.fract());
            // along the pill, each in its own stretch
            let x = (i as f64 + 0.15 + 0.7 * chance(i, turn.wrapping_add(54))) / many as f64 * w;
            let top = 3.0 + chance(i, turn.wrapping_add(55)) * h * 0.45;
            let radius = (1.0 + 0.9 * chance(i, 56)) * (p / 0.12).clamp(0.35, 1.0);
            let run = ((p - 0.72) / 0.28).max(0.0);
            // a drying glass lets go of it
            let y = top + run * run * (h + radius - top) + (1.0 - here) * 7.0;
            Some(Bead { x, y, radius, alpha: here, trail: (run > 0.0).then_some(top) })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pile_grows_while_its_weather_lasts_and_goes_down_from_where_it_was() {
        let mut p = Pile::default();
        assert_eq!(p.level(5_000, 1000, 2000), 0.0);
        assert!(!p.changing(5_000, 1000, 2000));
        p.turn(true, 10_000, 1000, 2000);
        assert!((p.level(10_500, 1000, 2000) - 0.5).abs() < 1e-9 && p.changing(10_500, 1000, 2000));
        assert!((p.level(60_000, 1000, 2000) - 1.0).abs() < 1e-9 && !p.changing(60_000, 1000, 2000));
        // stopped half way: it goes down from there, at its own pace
        let mut half = Pile::default();
        half.turn(true, 0, 1000, 2000);
        half.turn(false, 500, 1000, 2000);
        assert!((half.level(500, 1000, 2000) - 0.5).abs() < 1e-9 && (half.level(1000, 1000, 2000) - 0.25).abs() < 1e-9);
        assert_eq!(half.level(9_000, 1000, 2000), 0.0);
        // saying the same again changes nothing
        let before = half;
        half.turn(false, 9_000, 1000, 2000);
        assert_eq!(half, before);
    }

    #[test]
    fn the_snow_lies_at_the_bottom_and_stays_below_the_writing() {
        assert!(bank(0.0, 150.0, 28.0).is_empty() && bank(0.01, 150.0, 28.0).is_empty());
        let (some, full) = (bank(0.5, 150.0, 28.0), bank(1.0, 150.0, 28.0));
        assert_eq!((some[0].0, some[some.len() - 1].0), (0.0, 150.0));
        assert!(full.iter().all(|p| p.1 >= 28.0 - SNOW_HIGH && p.1 < 28.0 - 2.0), "{full:?}");
        assert!(some.iter().zip(&full).all(|(a, b)| a.1 > b.1), "higher as it piles");
        assert!(full.iter().any(|p| (p.1 - full[0].1).abs() > 0.5), "not flat");
        assert_eq!(bank(0.500, 150.0, 28.0), bank(0.505, 150.0, 28.0), "in steps");
    }

    #[test]
    fn drops_form_hang_run_down_and_leave_as_the_glass_dries() {
        let wet = beads(1.0, 20_000, 200.0, 28.0);
        assert!(wet.len() >= 6);
        assert_eq!(wet, beads(1.0, 20_000, 200.0, 28.0));
        assert_ne!(wet, beads(1.0, 22_000, 200.0, 28.0));
        assert!(beads(0.0, 20_000, 200.0, 28.0).is_empty());
        assert!(beads(0.4, 20_000, 200.0, 28.0).len() < wet.len());
        let mut ran = false;
        for t in (0..60_000).step_by(90) {
            for b in beads(1.0, t, 200.0, 28.0) {
                assert!((0.0..=200.0).contains(&b.x) && (0.0..=38.0).contains(&b.y) && b.radius > 0.0 && b.radius <= 2.0, "{b:?}");
                ran |= b.trail.is_some_and(|from| from < b.y);
            }
        }
        assert!(ran);
        assert_eq!(glass_time(2_000, false), 2_000);
        assert_eq!(glass_time(60_000, false), glass_time(100_000, false), "still while the rain is");
        assert_eq!(glass_time(60_000, true), 60_000);
    }
}
