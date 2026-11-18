//! Weather helpers: WMO codes and the "rain soon" heads-up.

/// WMO weather code -> glyph name (Tabler icon names).
pub fn glyph(code: i32, day: bool) -> &'static str {
    match code {
        0 => {
            if day {
                "sun"
            } else {
                "moon"
            }
        }
        1 | 2 => "cloud-sun",
        3 => "cloud",
        45 | 48 => "cloud-fog",
        71..=77 | 85 | 86 => "cloud-snow",
        c if c >= 95 => "cloud-storm",
        _ => "cloud-rain",
    }
}

/// Precipitation (mm per 15 min) that counts as rain.
pub const RAIN_MM: f64 = 0.1;

#[derive(Clone, Copy, Debug)]
pub struct Slot {
    pub offset_min: i32,
    pub mm: f64,
}

/// Minutes until rain starts, from 15-minute slots (ascending offsets from now), or None when it is
/// already raining or stays dry within `horizon_min`.
pub fn rain_soon(slots: &[Slot], horizon_min: i32) -> Option<i32> {
    let first = slots.first()?;
    if first.offset_min <= 0 && first.mm >= RAIN_MM {
        return None;
    }
    for s in slots {
        if s.offset_min > horizon_min {
            break;
        }
        if s.offset_min > 0 && s.mm >= RAIN_MM {
            return Some(s.offset_min);
        }
    }
    None
}

/// One hour of the forecast.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hour {
    /// Start of the hour, epoch ms.
    pub time_ms: i64,
    pub temp_c: f32,
    pub code: i32,
    pub is_day: bool,
    /// Chance of rain, 0..=100.
    pub rain_prob: u8,
    pub mm: f32,
}

/// What a code says in words.
pub fn describe(code: i32) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mostly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51..=57 => "Drizzle",
        61 | 63 | 80 | 81 => "Rain",
        65 | 82 => "Heavy rain",
        66 | 67 => "Freezing rain",
        71..=77 | 85 | 86 => "Snow",
        95 => "Thunderstorm",
        96..=99 => "Thunderstorm, hail",
        _ => "Cloudy",
    }
}

/// An hour counts as rainy from this many mm, or from this chance of rain.
pub const RAIN_HOUR_MM: f32 = 0.2;
pub const RAIN_HOUR_PROB: u8 = 70;

fn rainy(h: &Hour) -> bool {
    h.mm >= RAIN_HOUR_MM || h.rain_prob >= RAIN_HOUR_PROB
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outlook {
    /// It rains now; `clears_in_min` when the end is in the forecast.
    Raining { clears_in_min: Option<i32> },
    /// Rain starts in so many minutes.
    Rain { in_min: i32 },
    /// Nothing in the forecast.
    Dry,
}

/// When it rains next: the 15-minute slots tell the next two hours, the hours the rest.
pub fn outlook(slots: &[Slot], hours: &[Hour], now_ms: i64) -> Outlook {
    let min_to = |t: i64| ((t - now_ms) as f64 / 60_000.0).ceil() as i32;
    let slots_end = slots.last().map_or(0, |s| s.offset_min);
    let raining_now = match slots.first() {
        Some(s) => s.mm >= RAIN_MM,
        None => hours.iter().rev().find(|h| h.time_ms <= now_ms).is_some_and(rainy),
    };
    if raining_now {
        let in_slots = slots.iter().find(|s| s.offset_min > 0 && s.mm < RAIN_MM).map(|s| s.offset_min);
        let in_hours = || hours.iter().find(|h| h.time_ms > now_ms + slots_end as i64 * 60_000 && !rainy(h)).map(|h| min_to(h.time_ms));
        return Outlook::Raining { clears_in_min: in_slots.or_else(in_hours) };
    }
    if let Some(s) = slots.iter().find(|s| s.offset_min > 0 && s.mm >= RAIN_MM) {
        return Outlook::Rain { in_min: s.offset_min };
    }
    let after = now_ms + slots_end as i64 * 60_000;
    match hours.iter().find(|h| h.time_ms > after.max(now_ms) && rainy(h)) {
        Some(h) => Outlook::Rain { in_min: min_to(h.time_ms).max(1) },
        None => Outlook::Dry,
    }
}

/// Rain this near (or now) is worth a line on the home page.
pub const HOME_RAIN_HORIZON_MIN: i32 = 360;

pub fn worth_showing_at_home(o: Outlook) -> bool {
    match o {
        Outlook::Raining { .. } => true,
        Outlook::Rain { in_min } => in_min <= HOME_RAIN_HORIZON_MIN,
        Outlook::Dry => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs() {
        assert_eq!(glyph(0, true), "sun");
        assert_eq!(glyph(0, false), "moon");
        assert_eq!(glyph(2, true), "cloud-sun");
        assert_eq!(glyph(3, true), "cloud");
        assert_eq!(glyph(45, true), "cloud-fog");
        assert_eq!(glyph(73, true), "cloud-snow");
        assert_eq!(glyph(96, true), "cloud-storm");
        assert_eq!(glyph(61, true), "cloud-rain");
    }

    fn hour(t_min: i64, mm: f32, prob: u8) -> Hour {
        Hour { time_ms: t_min * 60_000, mm, rain_prob: prob, ..Default::default() }
    }

    #[test]
    fn the_next_rain_comes_from_the_slots_then_from_the_hours() {
        let s = |o, mm| Slot { offset_min: o, mm };
        let now = 100 * 60_000;
        assert_eq!(outlook(&[s(0, 0.0), s(15, 0.3)], &[], now), Outlook::Rain { in_min: 15 });
        // dry for two hours of slots, the hours say 18:00 (in 4 h) will rain
        let slots: Vec<Slot> = (0..8).map(|i| s(i * 15, 0.0)).collect();
        let hours = [hour(60, 0.0, 0), hour(160, 0.0, 10), hour(340, 0.8, 60), hour(400, 1.0, 90)];
        assert_eq!(outlook(&slots, &hours, now), Outlook::Rain { in_min: 240 });
        assert_eq!(outlook(&slots, &[hour(160, 0.0, 10), hour(220, 0.0, 69)], now), Outlook::Dry, "a likely-but-light hour below the line");
        assert_eq!(outlook(&slots, &[hour(220, 0.0, 70)], now), Outlook::Rain { in_min: 120 }, "a high chance counts");
        // the hour that has already started is not "in the future"
        assert_eq!(outlook(&slots, &[hour(60, 2.0, 100)], now), Outlook::Dry);
    }

    #[test]
    fn raining_now_says_when_it_clears() {
        let s = |o, mm| Slot { offset_min: o, mm };
        let now = 0;
        assert_eq!(outlook(&[s(0, 0.5), s(15, 0.4), s(30, 0.0)], &[], now), Outlook::Raining { clears_in_min: Some(30) });
        let wet: Vec<Slot> = (0..8).map(|i| s(i * 15, 0.5)).collect();
        let hours = [hour(120, 1.0, 100), hour(180, 0.0, 20)];
        assert_eq!(outlook(&wet, &hours, now), Outlook::Raining { clears_in_min: Some(180) });
        assert_eq!(outlook(&wet, &[], now), Outlook::Raining { clears_in_min: None });
        // without slots the current hour decides
        assert_eq!(outlook(&[], &[hour(-30, 1.0, 100), hour(30, 0.0, 0)], now), Outlook::Raining { clears_in_min: Some(30) });
    }

    #[test]
    fn home_shows_rain_within_six_hours_only() {
        assert!(worth_showing_at_home(Outlook::Rain { in_min: 360 }));
        assert!(!worth_showing_at_home(Outlook::Rain { in_min: 361 }));
        assert!(worth_showing_at_home(Outlook::Raining { clears_in_min: None }));
        assert!(!worth_showing_at_home(Outlook::Dry));
    }

    #[test]
    fn codes_in_words() {
        assert_eq!(describe(0), "Clear");
        assert_eq!(describe(63), "Rain");
        assert_eq!(describe(95), "Thunderstorm");
        assert_eq!(describe(999), "Cloudy");
    }

    #[test]
    fn rain() {
        let s = |o, mm| Slot { offset_min: o, mm };
        assert_eq!(rain_soon(&[], 60), None);
        assert_eq!(rain_soon(&[s(0, 0.0), s(15, 0.0), s(30, 0.4)], 60), Some(30));
        assert_eq!(rain_soon(&[s(0, 0.5), s(15, 0.5)], 60), None, "already raining");
        assert_eq!(rain_soon(&[s(0, 0.0), s(75, 1.0)], 60), None, "beyond the horizon");
    }
}
