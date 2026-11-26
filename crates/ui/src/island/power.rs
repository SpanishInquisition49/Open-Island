//! Will the battery last the day? The drain is learned from the machine's own use, the day is the events that are left and the
//! way home (`commute_minutes`). See `island_core::power` for the arithmetic.

use super::*;
use island_core::power;
use island_core::timeline::{Item, Kind};

/// Percent samples kept for the drain, newest last (one a minute, an hour and a half).
const KEEP_MS: i64 = 90 * 60_000;

/// What the plan for a day says.
pub(crate) struct DayPower {
    pub plan: power::Plan,
    pub percent: f64,
    pub home_at: i64,
    pub depart: i64,
}

impl Island {
    /// One sample a minute while on battery; what was charged is forgotten (a drain is measured only on battery).
    pub(super) fn sample_battery(&mut self, now: i64) {
        let Some(b) = self.data.battery else { return };
        if b.on_ac || b.charging {
            self.battery_log.clear();
            self.drain_live = None;
            return;
        }
        if self.battery_log.last().is_none_or(|(t, _)| now - t >= 60_000) {
            self.battery_log.push((now, b.percent));
            self.battery_log.retain(|(t, _)| now - t <= KEEP_MS);
        }
        self.drain_live = power::drain_per_hour(&self.battery_log, 10 * 60_000);
        if let Some(r) = self.drain_live {
            if now - self.drain_blended_at >= 15 * 60_000 {
                self.drain_saved = Some(power::blend(self.drain_saved, r));
                self.drain_blended_at = now;
            }
        }
    }

    /// Percent per hour: what is being measured now, else what was learned before.
    pub fn drain_rate(&self) -> Option<f64> {
        self.drain_live.or(self.drain_saved)
    }

    /// What was learned, to be kept between runs.
    pub fn drain_saved(&self) -> Option<f64> {
        self.drain_saved
    }

    pub fn restore_drain(&mut self, rate: Option<f64>) {
        self.drain_saved = rate.filter(|r| r.is_finite() && *r > 0.0 && *r < 200.0);
    }

    /// The timed events of a list: (first start, last end).
    fn day_span(items: &[Item]) -> Option<(i64, i64)> {
        let timed = || items.iter().filter(|i| i.kind == Kind::Event && !i.all_day);
        Some((timed().map(|i| i.at_ms).min()?, timed().map(|i| i.end_ms.max(i.at_ms)).max()?))
    }

    /// The plan for what is left of today, when the drain is known and there is a timed event ahead.
    pub(crate) fn power_today(&self) -> Option<DayPower> {
        let b = self.data.battery?;
        let rate = self.drain_rate()?;
        let items = self.today_left();
        let (first, last_end) = Self::day_span(&items)?;
        let commute = self.cfg.commute_minutes.max(0) * 60_000;
        let home_at = last_end + commute;
        // already on battery: from now; plugged in: from when it has to be left
        let depart = if b.on_ac || b.charging { (first - commute - self.cfg.travel_margin_ms).max(self.now) } else { self.now };
        let percent = b.percent as f64;
        let plan = power::plan(percent, rate, self.now, depart, home_at, self.cfg.low_battery as f64)?;
        Some(DayPower { plan, percent, home_at, depart })
    }

    /// A line for the cards: the morning one when the battery will not do, the evening one with the charge to leave with.
    pub(crate) fn power_line(&self, tomorrow: bool, items: &[Item]) -> Option<String> {
        let off = self.data.utc_offset_secs;
        let hhmm = |ms: i64| crate::clock::civil(ms, off).hhmm();
        if tomorrow {
            let rate = self.drain_rate()?;
            let (first, last_end) = Self::day_span(items)?;
            let commute = self.cfg.commute_minutes.max(0) * 60_000;
            let p = power::plan(0.0, rate, first, first - commute, last_end + commute, self.cfg.low_battery as f64)?;
            // a short day on a full battery is not news
            return (p.need_pct >= 40.0).then(|| format!("Charge to {}% before you leave", (p.need_pct / 5.0).ceil() as i64 * 5));
        }
        let d = self.power_today()?;
        d.plan.short.then(|| format!("Battery {}%: the day needs ~{}% (home ~{})", d.percent.round(), d.plan.need_pct.ceil(), hhmm(d.home_at)))
    }

    /// One card a day when the battery will not last, and one when it is about to be unplugged too low.
    pub(super) fn power_card(&mut self, now: i64) {
        if !self.cfg.battery_alerts {
            return;
        }
        let Some(d) = self.power_today() else { return };
        if !d.plan.short {
            return;
        }
        let day = self.day_number(now);
        let off = self.data.utc_offset_secs;
        let hhmm = |ms: i64| crate::clock::civil(ms, off).hhmm();
        let on_battery = self.data.battery.is_some_and(|b| !b.on_ac && !b.charging);
        let (kind, summary, body) = if on_battery {
            (0, "The battery will not last the day", format!("{}% now · the day needs ~{}% (home ~{}) · empty about {}", d.percent.round(), d.plan.need_pct.ceil(), hhmm(d.home_at), hhmm(d.plan.empty_at_ms)))
        } else if d.depart - now <= 30 * 60_000 {
            (1, "Charge a little more", format!("{}% now · the day needs ~{}% when you leave", d.percent.round(), d.plan.need_pct.ceil()))
        } else {
            return;
        };
        if self.power_told[kind] == day {
            return;
        }
        self.power_told[kind] = day;
        self.next_own_id += 1;
        let n = Incoming { id: self.next_own_id, app: "open-island".into(), summary: summary.into(), body, expire_ms: Some(12_000), ..Default::default() };
        self.push(now, Input::Notification(n));
        self.pending.push(Command::PlaySound);
    }
}
