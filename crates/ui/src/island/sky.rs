//! Part of `Island` (see `mod.rs`): when the weather shows inside the pill, and when it moves. What it looks like at a
//! moment is in `island_core::sky`.

use super::*;
use island_core::residue::{self, Pile, SNOW_MELT_MS, SNOW_RISE_MS, WET_DRY_MS, WET_RISE_MS};
use island_core::sky::{self, Kind, Sky};

/// How long a sky asked for with `msg sky` is shown.
const PREVIEW_MS: i64 = 30_000;
/// The time between two frames of it: what falls is quick, the rest slow.
const FALLING_STEP_MS: u32 = 33;
const SLOW_STEP_MS: u32 = 50;
/// The same for what the weather has left, while it changes: drops that slide off, snow that grows or melts.
const DRYING_STEP_MS: u32 = 50;
const PILE_STEP_MS: u32 = 250;

#[derive(Default)]
pub(crate) struct Outside {
    /// The weather's sky, and since when it has been there.
    real: Option<(Sky, i64)>,
    /// One asked for to look at, and until when.
    shown: Option<(Sky, i64)>,
    /// Whether the machine was on power at the last frame.
    powered: bool,
    /// What the weather has left on the pill.
    snow: Pile,
    wet: Pile,
    /// The moment the time of the drops on the glass is counted from.
    glass_since: i64,
}

impl Island {
    /// Shows a sky for a while, whatever the weather and the options say; None goes back to them.
    pub(super) fn preview_sky(&mut self, asked: Option<Sky>) {
        self.outside.shown = asked.map(|s| (s, self.now + PREVIEW_MS));
    }

    /// Once a frame: the weather's sky.
    pub(super) fn sky_tick(&mut self, now: i64) {
        if self.outside.shown.is_some_and(|(_, until)| now >= until) {
            self.outside.shown = None;
        }
        let powered = self.on_power();
        if powered != self.outside.powered {
            // the pace changes: what has drifted stays where it is
            if let Some((s, since)) = self.outside.real.filter(|(s, _)| s.kind.stands()) {
                let drifted = sky::drift(now - since, self.outside.powered);
                self.outside.real = Some((s, now - sky::rested(drifted, powered)));
            }
            self.outside.powered = powered;
        }
        let new = self.data.weather.as_ref().and_then(|w| sky::of(w.code, w.day));
        if new != self.outside.real.map(|(s, _)| s) {
            self.outside.real = new.map(|s| (s, now));
        }
        self.residue_tick(now);
    }

    /// The sky over the pill, whether it is being drawn or not, and since when.
    fn overhead(&self) -> Option<(Sky, i64)> {
        match self.outside.shown {
            Some((s, until)) => Some((s, until - PREVIEW_MS)),
            None => self.outside.real.filter(|_| self.cfg.weather_in_pill),
        }
    }

    /// Snow piles up while it snows and the glass gets wet while it rains; both go back after.
    fn residue_tick(&mut self, now: i64) {
        if !self.cfg.weather_in_pill && self.outside.shown.is_none() {
            (self.outside.snow, self.outside.wet) = Default::default();
            return;
        }
        let over = self.overhead();
        let snowing = over.is_some_and(|(s, _)| s.kind == Kind::Snow);
        let rain = over.filter(|(s, _)| matches!(s.kind, Kind::Drizzle | Kind::Rain | Kind::Storm));
        let powered = self.shines_often();
        match rain {
            Some((_, since)) => self.outside.glass_since = since,
            // it has just stopped: the drops go on from where they were seen
            None if self.outside.wet.rising => self.outside.glass_since = now - residue::glass_time(now - self.outside.glass_since, powered),
            None => {}
        }
        self.outside.snow.turn(snowing, now, SNOW_RISE_MS, SNOW_MELT_MS);
        self.outside.wet.turn(rain.is_some(), now, WET_RISE_MS, WET_DRY_MS);
    }

    /// What the weather has left on the pill, in the small pill on screen: how much snow, how wet the glass, and the
    /// time its drops are at.
    pub(crate) fn residue(&self) -> Option<(f64, f64, i64)> {
        if !self.shown.is_compact() || self.out_of_sight() {
            return None;
        }
        let o = &self.outside;
        let snow = o.snow.level(self.now, SNOW_RISE_MS, SNOW_MELT_MS);
        let wet = o.wet.level(self.now, WET_RISE_MS, WET_DRY_MS);
        let since = self.now - o.glass_since;
        let glass = if o.wet.rising { residue::glass_time(since, self.shines_often()) } else { since };
        (snow > 0.0 || wet > 0.0).then_some((snow, wet, glass))
    }

    /// The sky in the pill now, and since when: only in the small pill and on screen. What falls is there only while it
    /// moves (on battery now and then); the rest is always there.
    fn sky_since(&self) -> Option<(Sky, i64)> {
        if !self.shown.is_compact() || self.out_of_sight() {
            return None;
        }
        if let Some((s, until)) = self.outside.shown {
            return Some((s, until - PREVIEW_MS));
        }
        self.outside.real.filter(|(s, since)| self.cfg.weather_in_pill && (s.kind.stands() || sky::moves(self.now - since, self.on_power())))
    }

    fn on_power(&self) -> bool {
        self.data.battery.is_none_or(|b| b.on_ac)
    }

    /// How far the clouds and the fog of the pill have drifted.
    pub(crate) fn sky_drift(&self) -> f64 {
        self.sky_since().map_or(0.0, |(_, since)| sky::drift(self.now - since, self.shines_often()))
    }

    /// One asked for to look at moves as often as on power.
    fn shines_often(&self) -> bool {
        self.outside.shown.is_some() || self.on_power()
    }

    /// Whether the machine is south of the equator, where the moon is lit from the other side.
    pub(crate) fn south(&self) -> bool {
        self.data.position.as_ref().map(|p| p.lat).or(self.data.config.latitude).is_some_and(|lat| lat < 0.0)
    }

    pub(crate) fn pill_sky(&self) -> Option<Sky> {
        self.sky_since().map(|(s, _)| s)
    }

    /// How far into a moment of movement a still sky of the pill is (the sun shines, the stars twinkle, the clouds
    /// drift); None while it rests.
    pub(crate) fn sky_shine(&self) -> Option<f64> {
        let (_, since) = self.sky_since().filter(|(s, _)| s.kind.stands())?;
        sky::shine(self.now - since, self.shines_often())
    }

    /// While the sky or what it has left is in motion, the time between two frames of it.
    pub(super) fn sky_step(&self) -> Option<u32> {
        let sky = self.pill_sky().and_then(|sky| if !sky.kind.stands() { Some(FALLING_STEP_MS) } else { self.sky_shine().map(|_| SLOW_STEP_MS) });
        let left = self.residue().and_then(|_| {
            let o = &self.outside;
            if !o.wet.rising && o.wet.changing(self.now, WET_RISE_MS, WET_DRY_MS) {
                Some(DRYING_STEP_MS)
            } else if o.wet.changing(self.now, WET_RISE_MS, WET_DRY_MS) || o.snow.changing(self.now, SNOW_RISE_MS, SNOW_MELT_MS) {
                Some(PILE_STEP_MS)
            } else {
                None
            }
        });
        sky.into_iter().chain(left).min()
    }
}
