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
    /// Where it takes place (a room, a building); empty when it is not said or is only a video-call address.
    pub place: String,
}

/// The part of a calendar entry's LOCATION worth showing: a room or a building, not a link, and short.
pub fn place_of(location: &str) -> String {
    let l = location.trim();
    let first = l.split(['\n', ';']).next().unwrap_or("").trim();
    if first.is_empty() || first.contains("://") || first.to_lowercase().contains("teams.microsoft") || first.to_lowercase().starts_with("microsoft teams") || first.to_lowercase().contains("zoom.us") {
        return String::new();
    }
    // "Aula 3, Polo Fibonacci, Largo Pontecorvo 3, Pisa": the room and the building are enough
    let parts: Vec<&str> = first.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
    parts.iter().take(2).copied().collect::<Vec<_>>().join(", ").chars().take(40).collect()
}

/// What happens in [start, end): all-day events first, then everything by start time (a reminder is a point in time).
pub fn build(events: &[Event], reminders: &[(i64, String)], start_ms: i64, end_ms: i64) -> Vec<Item> {
    let mut out: Vec<Item> = crate::calendar::events_between(events, start_ms, end_ms)
        .into_iter()
        .map(|e| Item { at_ms: e.start_ms, end_ms: e.end_ms, title: e.title.clone(), kind: Kind::Event, all_day: e.all_day, color: e.color, url: e.url.clone().filter(|u| !u.is_empty()), place: place_of(&e.location) })
        .collect();
    out.extend(reminders.iter().filter(|(at, _)| *at >= start_ms && *at < end_ms).map(|(at, text)| Item {
        at_ms: *at,
        end_ms: *at,
        title: text.clone(),
        kind: Kind::Reminder,
        all_day: false,
        color: None,
        url: None,
        place: String::new(),
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

/// When to leave for the first thing of the day that has to be gone to: the first timed event with no video-call link
/// (a lecture hall, not a meeting on the laptop). Returns the moment to leave and the event.
pub fn leave_plan(items: &[Item], commute_ms: i64, margin_ms: i64) -> Option<(i64, &Item)> {
    if commute_ms <= 0 {
        return None;
    }
    items.iter().find(|i| i.kind == Kind::Event && !i.all_day && i.url.is_none()).map(|i| (i.at_ms - commute_ms - margin_ms, i))
}

/// The free time between two events when it is at least `min_ms` (a gap to study in), else None.
pub fn gap_between(prev: &Item, next: &Item, min_ms: i64) -> Option<i64> {
    if prev.kind != Kind::Event || next.kind != Kind::Event || prev.all_day || next.all_day {
        return None;
    }
    let gap = next.at_ms - prev.end_ms;
    (gap >= min_ms).then_some(gap)
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

    #[test]
    fn the_first_event_to_go_to_sets_the_time_to_leave() {
        let mut online = ev("call", 8 * 60 * MIN, 9 * 60 * MIN, false);
        online.url = Some("https://meet".into());
        let events = [online, ev("holiday", 0, 1440 * MIN, true), ev("Reti", 9 * 60 * MIN + 30 * MIN, 11 * 60 * MIN, false), ev("Lab", 14 * 60 * MIN, 16 * 60 * MIN, false)];
        let items = build(&events, &[(8 * 60 * MIN, "tea".into())], 0, 1440 * MIN);
        let (leave, item) = leave_plan(&items, 75 * MIN, 10 * MIN).unwrap();
        assert_eq!(item.title, "Reti", "an online call, an all-day event and a reminder are not trips");
        assert_eq!(leave, 9 * 60 * MIN + 30 * MIN - 85 * MIN, "start minus commute minus margin");
        assert!(leave_plan(&items, 0, 10 * MIN).is_none(), "no commute set, no plan");
        assert!(leave_plan(&[], 75 * MIN, 0).is_none());
    }

    #[test]
    fn only_a_real_pause_between_two_events_is_a_gap() {
        let (a, b, c) = (ev("a", 0, 60 * MIN, false), ev("b", 70 * MIN, 120 * MIN, false), ev("c", 180 * MIN, 240 * MIN, false));
        let items = build(&[a, b, c], &[], 0, 1440 * MIN);
        assert_eq!(gap_between(&items[0], &items[1], 45 * MIN), None, "ten minutes is a walk, not a gap");
        assert_eq!(gap_between(&items[1], &items[2], 45 * MIN), Some(60 * MIN));
        let rem = build(&[], &[(10, "x".into())], 0, 1440 * MIN);
        assert_eq!(gap_between(&items[1], &rem[0], 1), None, "a reminder is not an event");
    }

    #[test]
    fn the_place_of_an_event_is_the_room_not_the_link_or_the_whole_address() {
        assert_eq!(place_of("Aula 3, Polo Fibonacci, Largo Bruno Pontecorvo 3, Pisa"), "Aula 3, Polo Fibonacci");
        assert_eq!(place_of("  Aula Magna  "), "Aula Magna");
        assert_eq!(place_of("https://teams.microsoft.com/l/meetup-join/abc"), "");
        assert_eq!(place_of("Microsoft Teams Meeting"), "");
        assert_eq!(place_of("Zoom: https://us02web.zoom.us/j/123"), "");
        assert_eq!(place_of(""), "");
        assert_eq!(place_of("Aula 1; Aula 2"), "Aula 1", "the first of several");
        assert!(place_of(&"x".repeat(100)).chars().count() <= 40);
    }
}

