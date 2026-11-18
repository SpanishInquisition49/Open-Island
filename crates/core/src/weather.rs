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

    #[test]
    fn rain() {
        let s = |o, mm| Slot { offset_min: o, mm };
        assert_eq!(rain_soon(&[], 60), None);
        assert_eq!(rain_soon(&[s(0, 0.0), s(15, 0.0), s(30, 0.4)], 60), Some(30));
        assert_eq!(rain_soon(&[s(0, 0.5), s(15, 0.5)], 60), None, "already raining");
        assert_eq!(rain_soon(&[s(0, 0.0), s(75, 1.0)], 60), None, "beyond the horizon");
    }
}
