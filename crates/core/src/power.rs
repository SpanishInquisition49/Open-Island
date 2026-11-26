//! Will the battery last the day? From how fast it drains, where the day ends (the last lecture and the way home) and
//! the level the machine is allowed to reach, this says what charge the day needs and when the battery would run out.
//! The drain is learned from the machine's own use (percent over time on battery), never guessed from nothing.

/// Percent per hour from samples (epoch ms, percent) taken on battery, oldest first. Needs at least `min_span_ms` between the
/// first and the last and a drop of at least 2 points (the battery reports whole percents), else None.
pub fn drain_per_hour(samples: &[(i64, f32)], min_span_ms: i64) -> Option<f64> {
    let (first, last) = (samples.first()?, samples.last()?);
    let span = last.0 - first.0;
    let drop = (first.1 - last.1) as f64;
    (span >= min_span_ms && drop >= 2.0).then(|| drop / (span as f64 / 3_600_000.0))
}

/// A rate worth keeping, mixed into what was learned before so one bad hour (a compile, a video call) does not rule.
pub fn blend(old: Option<f64>, new: f64) -> f64 {
    match old {
        Some(o) if o > 0.0 => 0.7 * o + 0.3 * new,
        _ => new,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plan {
    /// The charge the machine needs when it leaves, to reach `home_at` with `reserve` left.
    pub need_pct: f64,
    /// When the battery would reach the reserve at this rate, starting from the current level.
    pub empty_at_ms: i64,
    /// The current level is below what the day needs.
    pub short: bool,
}

/// `percent` now, `rate` percent per hour, `depart_ms` when it goes on battery for the day (now, if it already is),
/// `home_at_ms` when the day ends, `reserve` the level to keep.
pub fn plan(percent: f64, rate: f64, now_ms: i64, depart_ms: i64, home_at_ms: i64, reserve: f64) -> Option<Plan> {
    if rate <= 0.0 || home_at_ms <= depart_ms {
        return None;
    }
    let hours = (home_at_ms - depart_ms) as f64 / 3_600_000.0;
    let need = (reserve + rate * hours).min(100.0);
    let empty_at = now_ms.max(depart_ms) + ((percent - reserve).max(0.0) / rate * 3_600_000.0) as i64;
    Some(Plan { need_pct: need, empty_at_ms: empty_at, short: percent + 0.5 < need })
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 3_600_000;

    #[test]
    fn the_drain_is_learned_from_enough_time_and_enough_drop() {
        let s = [(0, 80.0), (30 * 60_000, 74.0), (H, 68.0)];
        assert_eq!(drain_per_hour(&s, 10 * 60_000), Some(12.0));
        assert_eq!(drain_per_hour(&[(0, 80.0), (5 * 60_000, 76.0)], 10 * 60_000), None, "too short");
        assert_eq!(drain_per_hour(&[(0, 80.0), (H, 79.0)], 10 * 60_000), None, "one point is rounding");
        assert_eq!(drain_per_hour(&[], 1), None);
        assert_eq!(drain_per_hour(&[(0, 50.0), (H, 60.0)], 1), None, "charging is not a drain");
        assert_eq!(blend(None, 12.0), 12.0);
        assert!((blend(Some(10.0), 20.0) - 13.0).abs() < 1e-9);
    }

    #[test]
    fn the_day_needs_a_charge_and_the_battery_runs_out_at_a_time() {
        // 12 %/h, on battery from 09:00 until 19:00, keep 15 %
        let p = plan(60.0, 12.0, 9 * H, 9 * H, 19 * H, 15.0).unwrap();
        assert!((p.need_pct - 100.0).abs() < 1e-9, "15 + 12 x 10 h is more than a full battery: capped");
        assert!(p.short);
        assert_eq!(p.empty_at_ms, 9 * H + (45.0 / 12.0 * H as f64) as i64, "45 points at 12 an hour: 3 h 45 min");
        // a shorter day is fine
        let q = plan(60.0, 12.0, 9 * H, 9 * H, 13 * H, 15.0).unwrap();
        assert!((q.need_pct - 63.0).abs() < 1e-9 && q.short, "63 needed, 60 there");
        let r = plan(70.0, 12.0, 9 * H, 9 * H, 13 * H, 15.0).unwrap();
        assert!(!r.short);
        assert!(plan(60.0, 0.0, 0, 0, H, 15.0).is_none() && plan(60.0, 12.0, 0, H, H, 15.0).is_none());
    }
}
