//! The weather inside the pill: the sun, the moon and stars, clouds, fog, or what falls, and where each is at a moment.
//! Pure and without memory: the place of everything is a function of the time, so nothing has to be stepped and a frame
//! can be drawn at any moment.
//!
//! What falls moves all the time. The rest is there far more often, so it stands still and only moves for a moment now
//! and then (the sun shines, the stars twinkle, clouds and fog drift on): a still picture costs nothing to keep.

/// How often, on battery, the weather moves for a while, and for how long.
const BATTERY_EVERY_MS: i64 = 120_000;
const BATTERY_FOR_MS: i64 = 6_000;
/// A thunderstorm lights up once in each stretch this long, at a moment of its own, and softly.
const FLASH_EVERY_MS: i64 = 9_000;
const FLASH_FOR_MS: i64 = 360;
const FLASH_MOST: f64 = 0.16;
/// How often the sun shines for a moment, on power, and for how long.
const SHINE_EVERY_MS: i64 = 30_000;
const SHINE_FOR_MS: i64 = 5_000;
/// The rays of the sun: it turns by one of their spacings as it shines, and so ends as it began.
pub const RAYS: u32 = 8;
/// How far clouds and fog go in one moment of drift, in pixels.
const DRIFT: f64 = 18.0;
/// A new moon (6 January 2000, 18:14 UTC) and the mean time from one to the next. The real ones stray from the mean by
/// some hours, which a moon a dozen pixels wide does not show.
const NEW_MOON_MS: i64 = 947_182_440_000;
const MONTH_MS: f64 = 29.530_588_853 * 86_400_000.0;
/// The width the counts below are for.
const PER_WIDTH: f64 = 150.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Sun,
    /// A clear night: the moon and the stars.
    Night,
    /// Some cloud, with the sun by day or the moon by night.
    Partly,
    Overcast,
    Fog,
    Drizzle,
    Rain,
    Snow,
    Storm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sky {
    pub kind: Kind,
    /// 1 light, 2 steady, 3 heavy.
    pub strength: u8,
    pub day: bool,
}

impl Kind {
    /// It stands still but for a moment now and then.
    pub fn stands(self) -> bool {
        matches!(self, Kind::Sun | Kind::Night | Kind::Partly | Kind::Overcast | Kind::Fog)
    }
}

/// The sky of a WMO weather code, by day or by night. None for a code that means nothing here.
pub fn of(code: i32, day: bool) -> Option<Sky> {
    use Kind::*;
    let (kind, strength) = match code {
        0 if day => (Sun, 2),
        1 if day => (Sun, 1),
        0 => (Night, 2),
        1 => (Night, 1),
        2 => (Partly, 1),
        3 => (Overcast, 2),
        45 | 48 => (Fog, 2),
        51 | 56 => (Drizzle, 1),
        53 | 57 => (Drizzle, 2),
        55 => (Drizzle, 3),
        61 | 66 | 80 => (Rain, 1),
        63 | 81 => (Rain, 2),
        65 | 67 | 82 => (Rain, 3),
        71 | 77 | 85 => (Snow, 1),
        73 => (Snow, 2),
        75 | 86 => (Snow, 3),
        95..=98 => (Storm, 2),
        99 => (Storm, 3),
        _ => return None,
    };
    Some(Sky { kind, strength, day })
}

/// A sky by the name a script asks for it with (`msg sky rain`).
pub fn named(name: &str) -> Option<Sky> {
    if matches!(name, "night" | "moon") {
        return of(0, false);
    }
    let code = match name {
        "sun" => 0,
        "partly" => 2,
        "cloudy" => 3,
        "fog" => 45,
        "drizzle" => 53,
        "rain" => 63,
        "heavy" => 65,
        "snow" => 73,
        "storm" => 95,
        _ => return None,
    };
    of(code, true)
}

/// Whether what falls moves now, `since_ms` after it began: always on power, on battery only now and then.
pub fn moves(since_ms: i64, on_power: bool) -> bool {
    on_power || since_ms.rem_euclid(BATTERY_EVERY_MS) < BATTERY_FOR_MS
}

/// Where in the time since it began what falls is seen to be, `since_ms` after: there while it moves, and where it
/// last stopped while it does not.
pub fn stopped_at(since_ms: i64, on_power: bool) -> i64 {
    if moves(since_ms, on_power) {
        since_ms
    } else {
        since_ms - since_ms.rem_euclid(BATTERY_EVERY_MS) + BATTERY_FOR_MS
    }
}

/// How far into a moment of shine the sun is, 0 to 1, `since_ms` after it came out (it shines as it does, and then
/// every so often: less often on battery). None while it stands still.
pub fn shine(since_ms: i64, on_power: bool) -> Option<f64> {
    let in_it = since_ms.rem_euclid(if on_power { SHINE_EVERY_MS } else { BATTERY_EVERY_MS });
    (in_it < SHINE_FOR_MS).then(|| in_it as f64 / SHINE_FOR_MS as f64)
}

/// The sun at a moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sun {
    /// How far its rays have turned, in radians.
    pub turn: f64,
    /// How bright it is beyond its rest, 0 to 1.
    pub glow: f64,
    /// Where the gleam that crosses the pill is, 0 to 1, while there is one.
    pub gleam: Option<f64>,
}

/// The sun `shine` into a moment of shine, or at rest (None).
pub fn sun(shine: Option<f64>) -> Sun {
    let Some(p) = shine else { return Sun { turn: 0.0, glow: 0.0, gleam: None } };
    let p = p.clamp(0.0, 1.0);
    // slow at both ends
    let eased = p * p * (3.0 - 2.0 * p);
    let gleam = ((p - 0.15) / 0.4 >= 0.0 && p <= 0.55).then(|| (p - 0.15) / 0.4);
    Sun { turn: eased * std::f64::consts::TAU / f64::from(RAYS), glow: (p * std::f64::consts::PI).sin(), gleam }
}

/// Where the moon is in its month at a moment (epoch ms): 0 new, 0.25 first quarter, 0.5 full, 0.75 last quarter.
pub fn moon_phase(utc_ms: i64) -> f64 {
    ((utc_ms - NEW_MOON_MS) as f64 / MONTH_MS).rem_euclid(1.0)
}

/// How much of the moon is lit at a phase, 0 to 1.
pub fn moon_light(phase: f64) -> f64 {
    (1.0 - (phase * std::f64::consts::TAU).cos()) / 2.0
}

/// The outline of the lit part of a moon of radius `r` around (0, 0), as seen from the northern hemisphere (it grows
/// from the right); from the southern one it is the other way round. The edge between light and dark is half an
/// ellipse, as wide as the phase says.
pub fn moon_outline(phase: f64, r: f64, south: bool) -> Vec<(f64, f64)> {
    const STEPS: i32 = 24;
    let waxing = phase.rem_euclid(1.0) < 0.5;
    // the same shape grows and wanes, mirrored
    let side = if waxing != south { 1.0 } else { -1.0 };
    let from_new = if waxing { phase.rem_euclid(1.0) } else { 1.0 - phase.rem_euclid(1.0) };
    let edge = (from_new * std::f64::consts::TAU).cos();
    let at = |k: i32| {
        let angle = std::f64::consts::PI * (f64::from(k) / f64::from(STEPS) - 0.5);
        (r * angle.cos(), r * angle.sin())
    };
    // down the lit rim from the top, and back up along the edge of the shadow
    let rim = (0..=STEPS).map(|k| (side * at(k).0, at(k).1));
    let shadow = (0..=STEPS).rev().map(|k| (side * edge * at(k).0, at(k).1));
    rim.chain(shadow).collect()
}

/// The stars of a clear night over a pill `w` by `h`: each in its own place, dim or bright. While the sky shines
/// (`shine` as for the sun) they twinkle, and one falls across.
pub fn stars(sky: Sky, shine: Option<f64>, w: f64, h: f64) -> Vec<Piece> {
    let night = sky.kind == Kind::Night || (sky.kind == Kind::Partly && !sky.day);
    if !night || w <= 0.0 || h <= 0.0 {
        return vec![];
    }
    let many = ((f64::from(4 + 4 * u32::from(sky.strength)) * w / PER_WIDTH).round() as u64).clamp(3, 40);
    // how much they stray from their rest: nothing as the shine starts and ends
    let stir = shine.map_or(0.0, |p| (p.clamp(0.0, 1.0) * std::f64::consts::PI).sin());
    let mut out: Vec<Piece> = (0..many)
        .map(|i| {
            // spread along the pill, each within its own stretch, so that they do not bunch
            let x = (i as f64 + 0.15 + 0.7 * chance(i, 11)) / many as f64 * w;
            let y = 3.0 + chance(i, 12) * (h - 6.0).max(0.0);
            let (size, rest) = (0.5 + 0.7 * chance(i, 13), 0.3 + 0.45 * chance(i, 14));
            let twinkle = shine.map_or(0.0, |p| (p * std::f64::consts::TAU * (2.0 + 3.0 * chance(i, 15)) + chance(i, 16) * 6.0).sin());
            Piece::Dot { x, y, radius: size * (1.0 + 0.35 * stir * twinkle), alpha: (rest + 0.35 * stir * twinkle).clamp(0.05, 1.0) }
        })
        .collect();
    if let Some(p) = shine.filter(|p| (0.3..0.5).contains(p)) {
        let at = (p - 0.3) / 0.2;
        let head = (w * (0.55 + 0.4 * at), 2.0 + (h - 4.0) * at);
        let tail = (head.0 - 14.0, head.1 - 14.0 * (h - 4.0) / (w * 0.4));
        out.push(Piece::Streak { from: tail, to: head, alpha: 0.9 * (at * std::f64::consts::PI).sin() });
    }
    out
}

/// How far the clouds and the fog have drifted, `since_ms` after they came: one more in each moment of shine, so that
/// they stand still in between.
pub fn drift(since_ms: i64, on_power: bool) -> f64 {
    let done = since_ms.div_euclid(if on_power { SHINE_EVERY_MS } else { BATTERY_EVERY_MS }) as f64;
    match shine(since_ms, on_power) {
        Some(p) => done + p * p * (3.0 - 2.0 * p),
        None => done + 1.0,
    }
}

/// How long into a sky `drift` steps are done and it rests, at the pace of that power: where to count from when the
/// pace changes, so that nothing jumps.
pub fn rested(drift: f64, on_power: bool) -> i64 {
    let every = if on_power { SHINE_EVERY_MS } else { BATTERY_EVERY_MS };
    match drift.round() as i64 {
        ..=0 => 0,
        steps => (steps - 1) * every + SHINE_FOR_MS,
    }
}

/// A cloud: where its middle is, how big it is (at 1 it is some 26 pixels wide) and how thick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cloud {
    pub x: f64,
    pub y: f64,
    pub size: f64,
    pub alpha: f64,
}

/// The clouds over a pill `w` by `h` after `drift`. Under a partly cloudy sky one stands before the sun or the moon,
/// when its disc is shown (`disc` says where), and a faint one passes further off.
pub fn clouds(sky: Sky, drift: f64, w: f64, h: f64, disc: Option<f64>) -> Vec<Cloud> {
    if w <= 0.0 || h <= 0.0 {
        return vec![];
    }
    // across the pill and round again, out of sight where it comes back
    let across = |start: f64, speed: f64, size: f64| {
        let span = w + 40.0 * size;
        (start * span + drift * DRIFT * speed).rem_euclid(span) - 20.0 * size
    };
    match sky.kind {
        Kind::Partly => {
            let mut out = vec![Cloud { x: across(0.72, 0.6, 0.75), y: h * 0.4, size: 0.75, alpha: 0.2 }];
            // it moves a little to and fro, and never leaves the disc bare or hidden
            out.extend(disc.map(|x| Cloud { x: x + 6.0 + 2.5 * (drift * 1.9).sin(), y: h / 2.0 + 3.5, size: 0.72, alpha: 0.9 }));
            out
        }
        Kind::Overcast => {
            let many = ((w / 55.0).ceil() as u64).clamp(2, 8);
            (0..many)
                .map(|i| {
                    let size = 0.85 + 0.5 * chance(i, 22);
                    let x = across((i as f64 + 0.6 * chance(i, 23)) / many as f64, 0.6 + 0.6 * chance(i, 21), size);
                    Cloud { x, y: h * (0.3 + 0.45 * chance(i, 24)), size, alpha: 0.14 + 0.12 * chance(i, 25) }
                })
                .collect()
        }
        _ => vec![],
    }
}

/// A bank of fog: its left end and middle line, its length and thickness.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bank {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub alpha: f64,
}

/// The fog over a pill `w` by `h` after `drift`: banks one above the other, each going the other way from the next.
pub fn fog(drift: f64, w: f64, h: f64) -> Vec<Bank> {
    if w <= 0.0 || h <= 0.0 {
        return vec![];
    }
    (0..4u64)
        .map(|i| {
            let width = w * (0.45 + 0.35 * chance(i, 31));
            let way = if i % 2 == 0 { 1.0 } else { -1.0 };
            let span = w + width;
            let x = (chance(i, 32) * span + way * drift * DRIFT * (0.7 + 0.5 * chance(i, 33))).rem_euclid(span) - width;
            Bank { x, y: h * (0.14 + 0.24 * i as f64), width, height: 4.0 + 3.0 * chance(i, 34), alpha: 0.2 + 0.14 * chance(i, 35) }
        })
        .collect()
}

/// Something to draw, in the pill's own coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Piece {
    /// A streak of rain, from its tail to its head.
    Streak { from: (f64, f64), to: (f64, f64), alpha: f64 },
    /// A flake, or a droplet thrown up where a drop landed.
    Dot { x: f64, y: f64, radius: f64, alpha: f64 },
}

/// A number in 0..1 that looks random and is always the same for the same pair.
pub(crate) fn chance(a: u64, b: u64) -> f64 {
    let mut z = a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_add(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
}

/// How many are in the air over a pill `w` wide.
fn count(sky: Sky, w: f64) -> u64 {
    let s = u64::from(sky.strength);
    let base = match sky.kind {
        Kind::Drizzle => 6 + 4 * s,
        Kind::Rain | Kind::Storm => 3 + 4 * s,
        Kind::Snow => 3 + 3 * s,
        _ => return 0,
    };
    ((base as f64 * w / PER_WIDTH).round() as u64).clamp(1, 80)
}

/// Everything in the air at `t_ms` over a pill `w` by `h`.
pub fn falling(sky: Sky, t_ms: i64, w: f64, h: f64) -> Vec<Piece> {
    if w <= 0.0 || h <= 0.0 || sky.kind.stands() {
        return vec![];
    }
    let t = t_ms.max(0) as f64;
    let mut out = vec![];
    for i in 0..count(sky, w) {
        // each has its own pace and starts at its own moment; every fall is from a new place
        let (a, b) = (chance(i, 1), chance(i, 2));
        let period = match sky.kind {
            Kind::Drizzle => 900.0 + 400.0 * a,
            Kind::Rain | Kind::Storm => 520.0 + 260.0 * a,
            Kind::Snow => 2600.0 + 1600.0 * a,
            _ => return out,
        };
        let at = t / period + b;
        let (fall, progress) = (at.floor() as u64, at.fract());
        let from = chance(i, fall.wrapping_add(3));
        match sky.kind {
            Kind::Snow => {
                let sway = (t / 620.0 + b * std::f64::consts::TAU).sin() * 3.0;
                let radius = 0.8 + 1.1 * a;
                out.push(Piece::Dot { x: from * w + sway, y: progress * (h + 2.0 * radius) - radius, radius, alpha: 0.45 + 0.4 * a });
            }
            kind => {
                let (length, slant, alpha) = if kind == Kind::Drizzle { (2.5, 0.08, 0.45) } else { (5.0 + 3.0 * a, 0.22, 0.6) };
                // the last stretch of the fall is the landing: a drop of rain throws two droplets up
                let landing = 0.82;
                if progress < landing || kind == Kind::Drizzle {
                    let reach = if kind == Kind::Drizzle { 1.0 } else { landing };
                    let y = progress / reach * (h + length);
                    let x = from * (w + h * slant) - y * slant;
                    out.push(Piece::Streak { from: (x + length * slant, y - length), to: (x, y), alpha });
                } else {
                    let p = (progress - landing) / (1.0 - landing);
                    let x = from * (w + h * slant) - (h + length) * slant;
                    for side in [-1.0, 1.0] {
                        out.push(Piece::Dot { x: x + side * (1.0 + 3.5 * p), y: h - 1.0 - 4.0 * p * (1.0 - p) * 2.0, radius: 0.9 * (1.0 - 0.5 * p), alpha: alpha * (1.0 - p) });
                    }
                }
            }
        }
    }
    out
}

/// How lit the sky is at `t_ms`, 0 for no flash: one soft flash in each stretch of a thunderstorm.
pub fn flash(sky: Sky, t_ms: i64) -> f64 {
    if sky.kind != Kind::Storm {
        return 0.0;
    }
    let stretch = t_ms.div_euclid(FLASH_EVERY_MS);
    let starts = (chance(stretch as u64, 7) * (FLASH_EVERY_MS - FLASH_FOR_MS) as f64) as i64;
    let in_it = t_ms.rem_euclid(FLASH_EVERY_MS) - starts;
    if !(0..FLASH_FOR_MS).contains(&in_it) {
        return 0.0;
    }
    // up quickly, down slowly
    let p = in_it as f64 / FLASH_FOR_MS as f64;
    FLASH_MOST * if p < 0.2 { p / 0.2 } else { (1.0 - p) / 0.8 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_weather_has_its_sky() {
        let sky = |code, day| of(code, day).map(|s| (s.kind, s.strength));
        for unknown in [-1, 4, 100] {
            assert_eq!(of(unknown, true), None, "{unknown}");
        }
        assert_eq!((sky(0, true), sky(1, true)), (Some((Kind::Sun, 2)), Some((Kind::Sun, 1))));
        assert_eq!((sky(0, false), sky(1, false)), (Some((Kind::Night, 2)), Some((Kind::Night, 1))));
        assert_eq!(of(2, false), Some(Sky { kind: Kind::Partly, strength: 1, day: false }));
        assert_eq!(sky(3, true).map(|s| s.0), Some(Kind::Overcast));
        assert_eq!((sky(45, true), sky(48, false)), (Some((Kind::Fog, 2)), Some((Kind::Fog, 2))));
        assert_eq!(sky(51, false), Some((Kind::Drizzle, 1)));
        assert_eq!(sky(65, true), Some((Kind::Rain, 3)));
        assert_eq!(sky(75, true), Some((Kind::Snow, 3)));
        assert_eq!(sky(95, false).map(|s| s.0), Some(Kind::Storm));
        assert_eq!(named("night"), of(0, false));
        assert_eq!(named("moon"), of(0, false));
        for (name, code) in [("sun", 0), ("partly", 2), ("cloudy", 3), ("fog", 45), ("snow", 73)] {
            assert_eq!(named(name), of(code, true), "{name}");
        }
        assert_eq!(named("hail"), None);
    }

    #[test]
    fn clouds_and_fog_drift_a_step_now_and_then_and_stand_still_between() {
        assert_eq!(drift(0, true), 0.0);
        assert!((drift(2_500, true) - 0.5).abs() < 1e-9);
        assert_eq!((drift(5_000, true), drift(29_999, true), drift(30_000, true)), (1.0, 1.0, 1.0), "no jump between one step and the next");
        assert_eq!((drift(35_000, true), drift(60_000, false)), (2.0, 1.0));
        for on_power in [false, true] {
            assert_eq!((drift(rested(7.0, on_power), on_power), rested(0.0, on_power)), (7.0, 0));
        }
        let overcast = named("cloudy").unwrap();
        let still = clouds(overcast, 1.0, 200.0, 28.0, None);
        assert!(still.len() >= 3 && clouds(overcast, 1.5, 200.0, 28.0, None) != still);
        for step in 0..400 {
            let d = f64::from(step) / 7.0;
            for c in clouds(overcast, d, 200.0, 28.0, None) {
                assert!((-20.0 * c.size..=200.0 + 20.0 * c.size).contains(&c.x) && (0.0..=1.0).contains(&c.alpha), "{c:?}");
            }
            for b in fog(d, 200.0, 28.0) {
                assert!((-b.width..=200.0).contains(&b.x) && (0.0..=28.0).contains(&b.y), "{b:?}");
            }
        }
        assert_ne!(fog(1.0, 200.0, 28.0), fog(1.2, 200.0, 28.0));
        // partly cloudy: one before the disc when there is one, by day and by night; stars only by night
        let (day, night) = (of(2, true).unwrap(), of(2, false).unwrap());
        assert_eq!(clouds(day, 0.0, 150.0, 28.0, Some(17.0)).len(), 2);
        assert_eq!(clouds(day, 0.0, 150.0, 28.0, None).len(), 1);
        assert!(clouds(day, 3.3, 150.0, 28.0, Some(17.0))[1].x > 17.0);
        assert!(stars(day, None, 150.0, 28.0).is_empty() && !stars(night, None, 150.0, 28.0).is_empty());
        assert!(stars(night, None, 150.0, 28.0).len() < stars(named("night").unwrap(), None, 150.0, 28.0).len());
        assert!(clouds(named("sun").unwrap(), 0.0, 150.0, 28.0, Some(17.0)).is_empty() && falling(overcast, 0, 150.0, 28.0).is_empty());
    }

    /// Eclipses tell when the moon really was new (the sun's) or full (its own).
    #[test]
    fn the_moons_phase_agrees_with_the_eclipses() {
        let at = |y: i64, m: i64, d: i64, h: i64| (crate::calendar::days_from_civil(y, m, d) * 24 + h) * 3_600_000;
        let off = |phase: f64, from: f64| (phase - from).rem_euclid(1.0).min((from - phase).rem_euclid(1.0));
        for (y, m, d, h, name) in [(2024, 4, 8, 18, "the sun's, April 2024"), (2026, 2, 17, 12, "the sun's, February 2026"), (2026, 8, 12, 18, "the sun's, August 2026")] {
            let p = moon_phase(at(y, m, d, h));
            assert!(off(p, 0.0) < 0.025, "{name}: {p}");
            assert!(moon_light(p) < 0.01, "{name}");
        }
        for (y, m, d, h, name) in [(2025, 3, 14, 7, "the moon's, March 2025"), (2025, 9, 7, 18, "the moon's, September 2025"), (2026, 3, 3, 12, "the moon's, March 2026"), (2026, 8, 28, 4, "the moon's, August 2026")] {
            let p = moon_phase(at(y, m, d, h));
            assert!(off(p, 0.5) < 0.025, "{name}: {p}");
            assert!(moon_light(p) > 0.99, "{name}");
        }
        assert!((moon_light(0.25) - 0.5).abs() < 1e-9);
        assert!((0.0..1.0).contains(&moon_phase(0)), "also before the new moon counted from");
    }

    /// The area inside an outline.
    fn area(points: &[(f64, f64)]) -> f64 {
        let twice: f64 = points.iter().zip(points.iter().cycle().skip(1)).map(|(a, b)| a.0 * b.1 - b.0 * a.1).sum();
        twice.abs() / 2.0
    }

    #[test]
    fn the_moon_is_lit_as_much_as_its_phase_says_and_on_the_right_side() {
        let disc = std::f64::consts::PI * 36.0;
        for phase in [0.0, 0.1, 0.25, 0.4, 0.5, 0.6, 0.75, 0.9] {
            let lit = area(&moon_outline(phase, 6.0, false)) / disc;
            assert!((lit - moon_light(phase)).abs() < 0.02, "{phase}: {lit} for {}", moon_light(phase));
        }
        let middle = |phase, south| {
            let o = moon_outline(phase, 6.0, south);
            o.iter().map(|p| p.0).sum::<f64>() / o.len() as f64
        };
        assert!(middle(0.15, false) > 1.0, "a young moon is lit on the right in the north");
        assert!(middle(0.85, false) < -1.0, "an old one on the left");
        assert!(middle(0.15, true) < -1.0 && middle(0.85, true) > 1.0, "the other way round in the south");
    }

    #[test]
    fn the_stars_stay_put_twinkle_while_they_shine_and_one_falls() {
        let night = named("night").unwrap();
        let rest = stars(night, None, 200.0, 28.0);
        assert_eq!(rest, stars(night, None, 200.0, 28.0));
        assert!(rest.len() > 8 && rest.iter().all(|p| matches!(p, Piece::Dot { x, y, .. } if (0.0..=200.0).contains(x) && (0.0..=28.0).contains(y))));
        assert!(stars(night, Some(0.7), 200.0, 28.0) != rest, "they twinkle");
        for end in [0.0, 1.0] {
            let there = stars(night, Some(end), 200.0, 28.0);
            assert_eq!(there.len(), rest.len());
            for pair in there.iter().zip(&rest) {
                match pair {
                    (Piece::Dot { alpha: a, radius: r, .. }, Piece::Dot { alpha: b, radius: s, .. }) => assert!((a - b).abs() < 1e-9 && (r - s).abs() < 1e-9, "at {end} as at rest"),
                    _ => panic!("only stars at the ends"),
                }
            }
        }
        let falling_one = stars(night, Some(0.4), 200.0, 28.0);
        assert_eq!(falling_one.len(), rest.len() + 1);
        assert!(matches!(falling_one.last(), Some(Piece::Streak { .. })));
        assert!(stars(named("sun").unwrap(), None, 200.0, 28.0).is_empty());
        assert!(falling(night, 1000, 200.0, 28.0).is_empty());
    }

    #[test]
    fn the_sun_shines_now_and_then_and_ends_as_it_began() {
        assert_eq!(shine(0, true), Some(0.0));
        assert!(shine(4_999, true).is_some_and(|p| p > 0.99) && shine(5_000, true).is_none() && shine(29_999, true).is_none());
        assert!(shine(30_000, true).is_some() && shine(30_000, false).is_none() && shine(120_000, false).is_some());
        let rest = sun(None);
        let (start, end) = (sun(Some(0.0)), sun(Some(1.0)));
        assert_eq!((start.turn, start.gleam), (rest.turn, None));
        assert!(start.glow.abs() < 1e-9 && end.glow.abs() < 1e-9 && end.gleam.is_none());
        // one spacing of its rays on, they are where they were
        assert!((end.turn - std::f64::consts::TAU / f64::from(RAYS)).abs() < 1e-9);
        let middle = sun(Some(0.5));
        assert!(middle.glow > 0.99 && middle.turn > 0.0 && middle.gleam.is_some_and(|g| (0.0..=1.0).contains(&g)));
        assert!(falling(named("sun").unwrap(), 1000, 150.0, 28.0).is_empty());
    }

    fn place(p: &Piece) -> (f64, f64) {
        match *p {
            Piece::Streak { to, .. } => to,
            Piece::Dot { x, y, .. } => (x, y),
        }
    }

    #[test]
    fn what_falls_stays_about_the_pill_and_moves_down() {
        for name in ["drizzle", "rain", "heavy", "snow", "storm"] {
            let sky = named(name).unwrap();
            for t in (0..20_000).step_by(37) {
                for p in falling(sky, t, 200.0, 28.0) {
                    let (x, y) = place(&p);
                    assert!((-12.0..=212.0).contains(&x) && (-8.0..=40.0).contains(&y), "{name} at {t}: {p:?}");
                    let alpha = match p {
                        Piece::Streak { alpha, .. } | Piece::Dot { alpha, .. } => alpha,
                    };
                    assert!((0.0..=1.0).contains(&alpha));
                }
            }
            assert_eq!(falling(sky, 1234, 200.0, 28.0), falling(sky, 1234, 200.0, 28.0), "the same at the same moment");
            assert_ne!(falling(sky, 1234, 200.0, 28.0), falling(sky, 1270, 200.0, 28.0), "and moving");
        }
        // one flake, a moment later, is lower
        let snow = named("snow").unwrap();
        let down = (0..40).filter(|n| place(&falling(snow, n * 50 + 50, 200.0, 28.0)[0]).1 > place(&falling(snow, n * 50, 200.0, 28.0)[0]).1).count();
        assert!(down >= 38, "{down} of 40: only a new fall goes back to the top");
    }

    #[test]
    fn heavier_weather_and_a_wider_pill_have_more_in_the_air() {
        let rain = |strength| Sky { kind: Kind::Rain, strength, day: true };
        assert!(count(rain(1), 150.0) < count(rain(2), 150.0) && count(rain(2), 150.0) < count(rain(3), 150.0));
        assert!(count(rain(2), 150.0) < count(rain(2), 300.0));
        assert!(falling(rain(2), 0, 0.0, 28.0).is_empty());
        assert!(count(rain(3), 100_000.0) <= 80);
    }

    #[test]
    fn on_battery_it_moves_only_now_and_then() {
        assert!(moves(0, false) && moves(5_999, false) && !moves(6_000, false) && !moves(119_999, false) && moves(120_000, false));
        assert!(moves(60_000, true));
    }

    #[test]
    fn a_storm_flashes_softly_once_in_a_while_and_nothing_else_does() {
        let storm = named("storm").unwrap();
        for stretch in 0..20 {
            let lit: Vec<f64> = (0..FLASH_EVERY_MS).step_by(10).map(|t| flash(storm, stretch * FLASH_EVERY_MS + t)).filter(|a| *a > 0.0).collect();
            assert!(!lit.is_empty() && lit.len() as i64 * 10 <= FLASH_FOR_MS, "one short flash: {}", lit.len());
            assert!(lit.iter().all(|a| *a <= FLASH_MOST));
        }
        assert!((0..60_000).step_by(10).all(|t| flash(named("rain").unwrap(), t) == 0.0));
    }
}
