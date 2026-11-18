//! Local time from an epoch and a UTC offset (the platform layer provides the offset; no time-zone
//! database is needed for the island's clock).

pub struct Civil {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// 0 = Monday .. 6 = Sunday
    pub weekday: u32,
}

/// Days since 1970-01-01 -> (year, month, day) (Howard Hinnant's algorithm).
pub fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((y + if m <= 2 { 1 } else { 0 }) as i32, m, d)
}

/// (year, month, day) -> days since 1970-01-01.
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = y as i64 - if m <= 2 { 1 } else { 0 };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil(epoch_ms: i64, utc_offset_secs: i32) -> Civil {
    let secs = epoch_ms.div_euclid(1000) + utc_offset_secs as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    Civil {
        year,
        month,
        day,
        hour: (rem / 3600) as u32,
        minute: ((rem % 3600) / 60) as u32,
        second: (rem % 60) as u32,
        weekday: (days + 3).rem_euclid(7) as u32, // 1970-01-01 was a Thursday
    }
}

/// Epoch ms of local midnight of the given date.
pub fn midnight_ms(y: i32, m: u32, d: u32, utc_offset_secs: i32) -> i64 {
    (days_from_civil(y, m, d) * 86_400 - utc_offset_secs as i64) * 1000
}

pub const WEEKDAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
pub const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November",
    "December",
];

impl Civil {
    pub fn hhmm(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    /// "Wednesday, 18 November"
    pub fn long_date(&self) -> String {
        format!("{}, {} {}", WEEKDAYS[self.weekday as usize], self.day, MONTHS[self.month as usize - 1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_known_dates() {
        let c = civil(0, 0);
        assert_eq!((c.year, c.month, c.day, c.hour, c.weekday), (1970, 1, 1, 0, 3));
        // 2026-11-18 16:45:30 UTC+1 (Wednesday)
        let ms = (days_from_civil(2026, 11, 18) * 86_400 + 15 * 3600 + 45 * 60 + 30) * 1000;
        let c = civil(ms, 3600);
        assert_eq!((c.year, c.month, c.day, c.hour, c.minute, c.second), (2026, 11, 18, 16, 45, 30));
        assert_eq!(c.weekday, 2);
        assert_eq!(c.hhmm(), "16:45");
        assert_eq!(c.long_date(), "Wednesday, 18 November");
    }

    #[test]
    fn round_trips_and_negative_offsets() {
        for (y, m, d) in [(2000, 2, 29), (1999, 12, 31), (2024, 3, 1), (2100, 3, 1), (1969, 12, 31)] {
            assert_eq!(civil_from_days(days_from_civil(y, m, d)), (y, m, d));
        }
        let ms = midnight_ms(2026, 11, 18, -5 * 3600);
        let c = civil(ms, -5 * 3600);
        assert_eq!((c.day, c.hour, c.minute), (18, 0, 0));
        assert_eq!(civil(-1000, 0).hour, 23, "before the epoch");
    }
}
