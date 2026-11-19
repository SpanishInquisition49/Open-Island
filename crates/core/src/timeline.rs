//! The day as one list: calendar events and reminders in time order, with the place of "now" in it.

use crate::calendar::Event;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Event,
    Reminder,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub at_ms: i64,
    pub end_ms: i64,
    pub title: String,
    pub kind: Kind,
    pub all_day: bool,
    pub color: Option<(u8, u8, u8)>,
    pub url: Option<String>,
}

/// What happens in [start, end): all-day events first, then everything by start time (a reminder is a point in time).
pub fn build(events: &[Event], reminders: &[(i64, String)], start_ms: i64, end_ms: i64) -> Vec<Item> {
    let mut out: Vec<Item> = crate::calendar::events_between(events, start_ms, end_ms)
        .into_iter()
        .map(|e| Item { at_ms: e.start_ms, end_ms: e.end_ms, title: e.title.clone(), kind: Kind::Event, all_day: e.all_day, color: e.color, url: e.url.clone().filter(|u| !u.is_empty()) })
        .collect();
    out.extend(reminders.iter().filter(|(at, _)| *at >= start_ms && *at < end_ms).map(|(at, text)| Item {
        at_ms: *at,
        end_ms: *at,
        title: text.clone(),
        kind: Kind::Reminder,
        all_day: false,
        color: None,
        url: None,
    }));
    out.sort_by(|a, b| b.all_day.cmp(&a.all_day).then(a.at_ms.cmp(&b.at_ms)).then(a.end_ms.cmp(&b.end_ms)));
    out
}

/// Where the "now" line goes: before the first timed item that has not ended. All-day items stay above it.
pub fn now_index(items: &[Item], now_ms: i64) -> usize {
    items.iter().position(|i| !i.all_day && i.end_ms.max(i.at_ms) > now_ms).unwrap_or(items.len())
}

/// The next timed item that has not started, and how many minutes away it is (rounded up).
pub fn next_up(items: &[Item], now_ms: i64) -> Option<(&Item, i64)> {
    items.iter().find(|i| !i.all_day && i.at_ms > now_ms).map(|i| (i, (i.at_ms - now_ms + 59_999) / 60_000))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000;

    fn ev(title: &str, start: i64, end: i64, all_day: bool) -> Event {
        Event { id: title.into(), title: title.into(), start_ms: start, end_ms: end, all_day, url: Some(String::new()), color: None, calendar: "c".into(), location: String::new() }
    }

    #[test]
    fn events_and_reminders_are_in_time_order_with_all_day_first() {
        let events = [ev("lunch", 120 * MIN, 150 * MIN, false), ev("holiday", 0, 1440 * MIN, true), ev("call", 60 * MIN, 90 * MIN, false)];
        let items = build(&events, &[(100 * MIN, "tea".into()), (5000 * MIN, "other day".into())], 0, 1440 * MIN);
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["holiday", "call", "tea", "lunch"]);
        assert_eq!(items[2].kind, Kind::Reminder);
        assert_eq!(items[1].url, None, "an empty link is no link");
    }

    #[test]
    fn the_now_line_sits_before_what_is_still_to_come() {
        let events = [ev("holiday", 0, 1440 * MIN, true), ev("done", 10 * MIN, 20 * MIN, false), ev("now", 55 * MIN, 70 * MIN, false), ev("later", 90 * MIN, 100 * MIN, false)];
        let items = build(&events, &[], 0, 1440 * MIN);
        assert_eq!(now_index(&items, 60 * MIN), 2, "after the all-day and the finished one, before the one in progress");
        assert_eq!(now_index(&items, 200 * MIN), 4, "everything is behind");
        assert_eq!(now_index(&items, 0), 1, "an all-day event is never behind the line");
    }

    #[test]
    fn next_up_skips_what_has_started() {
        let events = [ev("now", 55 * MIN, 70 * MIN, false), ev("later", 90 * MIN, 100 * MIN, false)];
        let items = build(&events, &[], 0, 1440 * MIN);
        let (i, m) = next_up(&items, 60 * MIN + 1).unwrap();
        assert_eq!((i.title.as_str(), m), ("later", 30));
        assert!(next_up(&items, 95 * MIN).is_none());
    }
}
