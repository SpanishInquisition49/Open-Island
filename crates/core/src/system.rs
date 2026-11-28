//! System page numbers: battery estimate and byte/rate formatting.

/// Minutes until empty (discharging) or full (charging), from energy (µWh) and power (µW); None when the
/// draw is unknown or too small to say.
pub fn battery_eta(energy_now: Option<f64>, energy_full: Option<f64>, power_now: Option<f64>, charging: bool) -> Option<u64> {
    let (e, p) = (energy_now?, power_now?);
    if p < 100_000.0 {
        return None; // under 0.1 W
    }
    let remaining = if charging { energy_full.unwrap_or(e) - e } else { e };
    if remaining <= 0.0 {
        return None;
    }
    Some((remaining / p * 60.0 + 0.5).floor() as u64)
}

/// What a battery card says under its state: "Full in 1h10", "About 3h20 left". `eta` counts only when it was worked
/// out for the way the battery is going now. Times are rounded to five minutes: they are estimates.
pub fn battery_line(percent: f32, charging: bool, on_ac: bool, eta: Option<(u64, bool)>) -> String {
    let time = eta.filter(|(m, up)| *up == charging && (1..=48 * 60).contains(m)).map(|(m, _)| crate::task::fmt_minutes((m.max(5) as u32 + 2) / 5 * 5));
    match (charging, on_ac, time) {
        (true, _, Some(t)) => format!("Full in {t}"),
        (false, false, Some(t)) => format!("About {t} left"),
        (true, ..) => "Plugged in".into(),
        (false, true, _) if percent >= 95.0 => "Full".into(),
        (false, true, _) => "Plugged in, not charging".into(),
        (false, false, None) => "Running on battery".into(),
    }
}

/// Bytes per second -> "820 B/s", "12 KB/s", "1.4 MB/s".
pub fn fmt_rate(bps: f64) -> String {
    if bps < 1024.0 {
        format!("{} B/s", bps.floor() as u64)
    } else if bps < 1024.0 * 1024.0 {
        format!("{} KB/s", (bps / 1024.0 + 0.5).floor() as u64)
    } else {
        format!("{:.1} MB/s", bps / (1024.0 * 1024.0))
    }
}

/// Megabytes -> "7.8 GB" / "512 MB".
pub fn fmt_mb(mb: f64) -> String {
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{} MB", (mb + 0.5).floor() as u64)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_battery_says_how_long_in_the_way_it_is_going() {
        use super::battery_line;
        assert_eq!(battery_line(60.0, true, true, Some((68, true))), "Full in 1h10");
        assert_eq!(battery_line(60.0, false, false, Some((201, false))), "About 3h20 left");
        assert_eq!(battery_line(60.0, true, true, Some((201, false))), "Plugged in", "an estimate from before it was plugged in");
        assert_eq!(battery_line(100.0, false, true, None), "Full");
        assert_eq!(battery_line(79.0, false, true, None), "Plugged in, not charging");
        assert_eq!(battery_line(12.0, false, false, None), "Running on battery");
        assert_eq!(battery_line(12.0, false, false, Some((1, false))), "About 5m left");
        assert_eq!(battery_line(12.0, false, false, Some((100_000, false))), "Running on battery", "nonsense is not shown");
    }

    use super::*;

    #[test]
    fn eta() {
        assert_eq!(battery_eta(Some(30e6), Some(50e6), Some(10e6), false), Some(180));
        assert_eq!(battery_eta(Some(30e6), Some(50e6), Some(10e6), true), Some(120));
        assert_eq!(battery_eta(Some(30e6), None, Some(50_000.0), false), None, "too little draw");
        assert_eq!(battery_eta(None, None, Some(1e7), false), None);
        assert_eq!(battery_eta(Some(50e6), Some(50e6), Some(1e7), true), None, "already full");
    }

    #[test]
    fn formats() {
        assert_eq!(fmt_rate(820.0), "820 B/s");
        assert_eq!(fmt_rate(12.0 * 1024.0), "12 KB/s");
        assert_eq!(fmt_rate(1.4 * 1024.0 * 1024.0), "1.4 MB/s");
        assert_eq!(fmt_mb(512.0), "512 MB");
        assert_eq!(fmt_mb(7987.0), "7.8 GB");
    }
}
