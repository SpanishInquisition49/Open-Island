//! Calendar maths (no time zones: callers pass local dates) and the meeting heads-up.

use std::collections::HashSet;

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date (year, month 1..=12, day) of a day count since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

/// 0 = Monday .. 6 = Sunday for a day count since 1970-01-01.
pub fn weekday_of_days(days: i64) -> i64 {
    (days + 3).rem_euclid(7)
}

pub fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        2 if is_leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Day of the week, 0 = Monday .. 6 = Sunday.
pub fn weekday(y: i32, m: u32, d: u32) -> u32 {
    // Sakamoto's algorithm gives 0 = Sunday
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let yy = if m < 3 { y - 1 } else { y };
    let sun0 = (yy + yy / 4 - yy / 100 + yy / 400 + T[(m - 1) as usize] + d as i32).rem_euclid(7);
    ((sun0 + 6) % 7) as u32
}

/// Weeks of a month, Monday first: rows of 7 slots holding the day number or None.
pub fn month_grid(y: i32, m: u32) -> Vec<[Option<u32>; 7]> {
    let mut weeks = vec![];
    let mut row = [None; 7];
    let mut col = weekday(y, m, 1) as usize;
    for d in 1..=days_in_month(y, m) {
        row[col] = Some(d);
        col += 1;
        if col == 7 {
            weeks.push(row);
            row = [None; 7];
            col = 0;
        }
    }
    if col > 0 {
        weeks.push(row);
    }
    weeks
}

/// Month shifted by `delta` months: (year, month 1..=12).
pub fn add_months(y: i32, m: u32, delta: i32) -> (i32, u32) {
    let idx = y * 12 + (m as i32 - 1) + delta;
    (idx.div_euclid(12), idx.rem_euclid(12) as u32 + 1)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Event {
    pub id: String,
    pub title: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub all_day: bool,
    pub url: Option<String>,
    pub color: Option<(u8, u8, u8)>,
    /// The name of the calendar the event belongs to.
    pub calendar: String,
}

/// Events overlapping [start, end), all-day ones first, then by start time.
pub fn events_between(events: &[Event], start_ms: i64, end_ms: i64) -> Vec<&Event> {
    let mut out: Vec<&Event> = events.iter().filter(|e| e.start_ms < end_ms && e.end_ms > start_ms).collect();
    out.sort_by(|a, b| b.all_day.cmp(&a.all_day).then(a.start_ms.cmp(&b.start_ms)));
    out
}

/// Next timed event that has not ended (ongoing ones count).
pub fn next_event(events: &[Event], now_ms: i64) -> Option<&Event> {
    events.iter().filter(|e| !e.all_day && e.end_ms > now_ms).min_by_key(|e| e.start_ms)
}

/// Event starting within the heads-up lead time (or started less than a minute ago).
pub fn soon(events: &[Event], now_ms: i64, lead_ms: i64) -> Option<&Event> {
    let e = next_event(events, now_ms)?;
    let d = e.start_ms - now_ms;
    (d <= lead_ms && d > -60_000).then_some(e)
}

/// Remembers which meetings were announced. A meeting is announced once, when it enters the
/// heads-up window. `keys()` / `from_keys()` let the caller persist them so a restart does not
/// announce the same meeting again.
#[derive(Default)]
pub struct Announcer {
    seen: HashSet<String>,
}

impl Announcer {
    pub fn from_keys(keys: impl IntoIterator<Item = String>) -> Self {
        Self { seen: keys.into_iter().collect() }
    }

    pub fn keys(&self) -> Vec<String> {
        let mut k: Vec<String> = self.seen.iter().cloned().collect();
        k.sort();
        k
    }

    /// The event to announce now, once; None when nothing is new. Keys of events that started more than a
    /// day ago are forgotten.
    pub fn new_heads_up<'a>(&mut self, events: &'a [Event], now_ms: i64, lead_ms: i64) -> Option<&'a Event> {
        let e = soon(events, now_ms, lead_ms)?;
        let key = format!("{}:{}", e.id, e.start_ms);
        if !self.seen.insert(key) {
            return None;
        }
        let cutoff = now_ms - 86_400_000;
        self.seen.retain(|k| k.rsplit(':').next().and_then(|s| s.parse::<i64>().ok()).is_none_or(|s| s > cutoff));
        Some(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert!(is_leap(2024) && !is_leap(1900) && is_leap(2000));
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 11), 30);
        assert_eq!(weekday(2026, 11, 18), 2); // Wednesday
        assert_eq!(weekday(2024, 1, 1), 0); // Monday
        assert_eq!(weekday(2023, 10, 1), 6); // Sunday
        assert_eq!(add_months(2026, 11, 2), (2027, 1));
        assert_eq!(add_months(2026, 1, -1), (2025, 12));
        assert_eq!(add_months(2026, 12, 0), (2026, 12));
    }

    #[test]
    fn grid() {
        let g = month_grid(2026, 11); // starts on a Sunday
        assert_eq!(g[0], [None, None, None, None, None, None, Some(1)]);
        assert_eq!(g.len(), 6);
        assert_eq!(g[5][0], Some(30));
        assert_eq!(g[5][1], None);
        let feb = month_grid(2027, 2); // starts on a Monday, 28 days: exactly four weeks
        assert_eq!(feb.len(), 4);
        assert_eq!(feb[0][0], Some(1));
        assert_eq!(feb[3][6], Some(28));
    }

    fn ev(id: &str, start: i64, end: i64) -> Event {
        Event { id: id.into(), start_ms: start, end_ms: end, ..Default::default() }
    }

    #[test]
    fn next_and_soon() {
        let evs = [ev("old", 0, 100), ev("b", 5000, 6000), ev("a", 2000, 3000)];
        assert_eq!(next_event(&evs, 150).unwrap().id, "a");
        assert_eq!(next_event(&evs, 2500).unwrap().id, "a", "ongoing counts");
        assert_eq!(soon(&evs, 150, 1000), None);
        assert_eq!(soon(&evs, 1200, 1000).unwrap().id, "a");
        assert_eq!(soon(&evs, 2000 + 61_000, 1000).map(|e| e.id.as_str()), None, "a started >1 min ago is no longer soon");
    }

    #[test]
    fn announces_once_and_survives_restart() {
        let evs = [ev("m", 1_000_000, 2_000_000)];
        let mut a = Announcer::default();
        assert!(a.new_heads_up(&evs, 0, 600_000).is_none(), "not yet in the window");
        assert!(a.new_heads_up(&evs, 400_000, 600_000).is_some());
        assert!(a.new_heads_up(&evs, 410_000, 600_000).is_none(), "once");
        assert!(a.new_heads_up(&evs, 950_000, 600_000).is_none(), "not again at one minute");
        let mut b = Announcer::from_keys(a.keys());
        assert!(b.new_heads_up(&evs, 420_000, 600_000).is_none(), "a restart does not repeat it");
    }

    #[test]
    fn between_orders_all_day_first() {
        let mut allday = ev("d", 0, 10_000);
        allday.all_day = true;
        let evs = [ev("t", 100, 200), allday];
        let out = events_between(&evs, 0, 1000);
        assert_eq!(out[0].id, "d");
        assert_eq!(events_between(&evs, 5000, 6000).len(), 1);
    }
}
