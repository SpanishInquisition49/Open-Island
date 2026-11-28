//! The calendar and the Today page: events, meetings, reminders and heads-ups.

use super::*;

#[test]
fn the_meeting_heads_up_fires_once_even_across_a_restart() {
    let ev = Event { id: "m".into(), title: "Class".into(), start_ms: T0 + 8 * 60_000, end_ms: T0 + 3_600_000, url: Some("https://meet.example/x".into()), ..Default::default() };
    let mut i = Island::new(Config::default());
    i.data.events = vec![ev.clone()];
    i.tick(T0, 8.0);
    let card = i.notif.clone().expect("a card");
    assert_eq!((card.summary.as_str(), card.actions[0].label.as_str()), ("Class", "Join"));
    i.pending.clear();
    i.activate(Action::OnCard(card.id, Box::new(Action::NotifOpen)));
    assert!(!i.pending.iter().any(|c| matches!(c, Command::OpenUrl(_))), "a click on the card does not join a call");
    run(&mut i, 3000);
    assert!(i.notif.is_none(), "not announced twice");
    let mut j = Island::new(Config::default());
    j.restore_announced(i.announced_keys());
    j.data.events = vec![ev];
    j.tick(T0 + 100, 8.0);
    assert!(j.notif.is_none(), "a restart does not announce it again");
}

#[test]
fn a_reminder_comes_back_as_a_notification_and_survives_a_restart() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenBar);
    type_line(&mut i, "remind 2m Call Anna");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert_eq!(i.reminders().len(), 1);
    let saved = i.reminders().to_vec();
    run(&mut i, 5000);
    assert_eq!(i.reminders().len(), 1, "not yet");
    // a new run of the app reads it back
    let mut j = Island::new(Config::default());
    j.restore_reminders(saved);
    run(&mut j, 130_000);
    assert!(j.reminders().is_empty(), "it fired");
    assert!(j.history.iter().any(|(n, _)| n.summary == "Call Anna" && n.app == "Reminder"));
}

#[test]
fn the_calendar_always_opens_on_today() {
    let mut i = Island::new(Config::default());
    i.data.utc_offset_secs = 3600;
    // reached by a swipe: the page is set without the Open action
    i.page = Some(Page::Cal);
    i.expand_until = T0 + 5000;
    run(&mut i, 400);
    let c = crate::clock::civil(i.now, 3600);
    assert_eq!((i.cal.year, i.cal.month, i.cal.day), (c.year, c.month, c.day));
    // browsing to another month, closing and opening again returns to today
    i.activate(Action::CalNext);
    assert_ne!((i.cal.year, i.cal.month), (c.year, c.month));
    i.page = None;
    i.expand_until = 0;
    run(&mut i, 3000);
    i.page = Some(Page::Cal);
    i.expand_until = i.now + 5000;
    run(&mut i, 400);
    assert_eq!((i.cal.year, i.cal.month, i.cal.day), (c.year, c.month, c.day));
}

#[test]
fn opening_the_calendar_with_stale_events_asks_for_a_refresh_once() {
    let mut i = Island::new(Config::default());
    i.data.config.calendars = vec![CalendarView { name: "A".into(), detail: "a.org".into(), color: (1, 2, 3) }];
    i.data.events_synced_at = T0 - 600_000;
    i.push(T0, Input::OpenPage(Page::Cal));
    run(&mut i, 300);
    let asks = |i: &mut Island| i.take_commands().iter().filter(|c| **c == Command::RefreshCalendars).count();
    assert_eq!(asks(&mut i), 1, "stale: read again");
    i.page = None;
    i.expand_until = 0;
    run(&mut i, 3000);
    i.push(i.now, Input::OpenPage(Page::Cal));
    run(&mut i, 300);
    assert_eq!(asks(&mut i), 0, "not again within 30 seconds");
    // fresh events and no calendars never ask
    let mut j = Island::new(Config::default());
    j.data.config.calendars = i.data.config.calendars.clone();
    j.data.events_synced_at = T0 - 1000;
    j.push(T0, Input::OpenPage(Page::Cal));
    run(&mut j, 300);
    assert_eq!(asks(&mut j), 0);
    let mut k = Island::new(Config::default());
    k.push(T0, Input::OpenPage(Page::Cal));
    run(&mut k, 300);
    assert_eq!(asks(&mut k), 0, "nothing to read");
}

#[test]
fn the_refresh_button_asks_at_once() {
    let mut i = Island::new(Config::default());
    assert_eq!(i.activate(Action::Cmd(Command::RefreshCalendars)), vec![Command::RefreshCalendars]);
    assert!(i.note.is_some());
}

/// An island on the calendar page, with a calendar and the stretch always read: two months around T0.
fn on_the_calendar() -> Island {
    let mut i = Island::new(Config::default());
    i.data.config.calendars = vec![CalendarView { name: "A".into(), detail: "a.org".into(), color: (1, 2, 3) }];
    i.data.events_synced_at = T0;
    i.data.events_span = (T0 - 62 * 86_400_000, T0 + 62 * 86_400_000);
    i.push(T0, Input::OpenPage(Page::Cal));
    run(&mut i, 300);
    i
}

fn months_asked(i: &mut Island) -> Vec<(i32, u32)> {
    i.take_commands().into_iter().filter_map(|c| if let Command::LoadCalendarMonth { year, month } = c { Some((year, month)) } else { None }).collect()
}

fn back(i: &mut Island, months: i32) -> (i32, u32) {
    for _ in 0..months {
        i.activate(Action::CalPrev);
    }
    i.expand_until = i.now + 5000;
    run(i, 100);
    (i.cal.year, i.cal.month)
}

#[test]
fn a_month_past_what_is_read_is_asked_for_once_and_shown_when_it_comes() {
    let mut i = on_the_calendar();
    assert_eq!(months_asked(&mut i), [], "this month is always at hand");
    let (y, m) = back(&mut i, 6);
    let asked = months_asked(&mut i);
    assert_eq!(asked.last(), Some(&(y, m)));
    assert_eq!(asked.iter().filter(|a| **a == (y, m)).count(), 1);
    assert_eq!(i.no_events_text(), "Reading this month…");
    run(&mut i, 500);
    assert_eq!(months_asked(&mut i), [], "not again while the answer is awaited");
    let start = i.local_midnight_ms(y, m, 1) + 10 * 3_600_000;
    let old = Event { id: "old".into(), title: "Old lesson".into(), start_ms: start, end_ms: start + 3_600_000, ..Default::default() };
    i.data.months = vec![MonthEvents { year: y, month: m, events: vec![old], read_at: i.now, complete: true }];
    run(&mut i, 500);
    assert_eq!(months_asked(&mut i), []);
    assert_eq!(i.day_events().iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["Old lesson"]);
    assert!(i.month_marks()[1].is_some(), "and its day has a dot");
    // a refresh marks it old: it is asked for again, and stays on screen meanwhile
    i.data.months[0].read_at = 0;
    run(&mut i, 200);
    assert_eq!(months_asked(&mut i), [(y, m)]);
    assert_eq!(i.day_events().len(), 1);
}

#[test]
fn a_month_ahead_of_what_is_read_is_asked_for_too() {
    let mut i = on_the_calendar();
    for _ in 0..5 {
        i.activate(Action::CalNext);
    }
    i.expand_until = i.now + 5000;
    run(&mut i, 100);
    assert_eq!(months_asked(&mut i).last(), Some(&(i.cal.year, i.cal.month)));
}

#[test]
fn a_month_that_could_not_be_read_is_tried_again_after_a_minute() {
    let mut i = on_the_calendar();
    let (y, m) = back(&mut i, 6);
    months_asked(&mut i);
    i.data.months = vec![MonthEvents { year: y, month: m, events: vec![], read_at: i.now, complete: false }];
    run(&mut i, 500);
    assert_eq!((months_asked(&mut i), i.no_events_text()), (vec![], "Could not read the calendars"));
    i.now += 61_000;
    i.expand_until = i.now + 5000;
    run(&mut i, 100);
    assert_eq!(months_asked(&mut i), [(y, m)]);
}

#[test]
fn a_month_partly_read_shows_both_parts_without_doubles() {
    let mut i = on_the_calendar();
    // the month holding the old edge of the stretch
    let edge = i.data.events_span.0;
    let c = crate::clock::civil(edge, 0);
    i.cal = Cal { year: c.year, month: c.month, day: 1 };
    let ev = |title: &str, start: i64| Event { id: title.into(), title: title.into(), start_ms: start, end_ms: start + 1000, ..Default::default() };
    let (before, after) = (ev("before", edge - 86_400_000), ev("after", edge + 3_600_000));
    i.data.events = vec![after.clone()];
    i.data.months = vec![MonthEvents { year: c.year, month: c.month, events: vec![before, after], read_at: i.now, complete: true }];
    assert_eq!(i.calendar_events().iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["before", "after"]);
}

#[test]
fn days_before_a_change_of_clocks_keep_their_own_hours() {
    /// Two hours ahead until 25 October 2026 01:00 UTC, one after.
    fn zone(ms: i64) -> i32 {
        if ms < 1_792_890_000_000 { 7200 } else { 3600 }
    }
    let mut i = Island::new(Config::default());
    i.data.utc_offset_secs = 3600;
    i.data.zone = Some(zone);
    let midnight = i.local_midnight_ms(2026, 10, 10);
    assert_eq!(midnight, crate::clock::midnight_ms(2026, 10, 10, 7200));
    i.data.events = vec![Event { id: "d".into(), title: "Holiday".into(), start_ms: midnight, end_ms: midnight + 86_400_000, all_day: true, ..Default::default() }];
    let on = |i: &mut Island, day: u32| {
        i.cal = Cal { year: 2026, month: 10, day };
        i.day_events().len()
    };
    assert_eq!((on(&mut i, 9), on(&mut i, 10), on(&mut i, 11)), (0, 1, 0), "an all-day event stays on its day");
    assert_eq!(i.local_midnight_ms(2026, 10, 26) - i.local_midnight_ms(2026, 10, 25), 25 * 3_600_000);
}

#[test]
fn a_search_puts_what_is_to_come_before_what_is_over() {
    let mut i = Island::new(Config::default());
    i.now = T0;
    let ev = |title: &str, hours: i64| Event { id: title.into(), title: title.into(), start_ms: T0 + hours * 3_600_000, end_ms: T0 + hours * 3_600_000 + 1000, ..Default::default() };
    i.data.events = vec![ev("Reti 1", -96), ev("Reti 2", -72), ev("Reti 3", -48), ev("Reti 4", -24), ev("Reti 5", 24), ev("Reti 6", 48)];
    i.start_find("reti".into());
    assert_eq!(i.found.hits.iter().map(|h| h.title.as_str()).collect::<Vec<_>>(), ["Reti 5", "Reti 6", "Reti 4", "Reti 3", "Reti 2"]);
}

#[test]
fn the_today_page_lists_events_and_reminders_with_a_now_line() {
    let mut i = Island::new(Config::default());
    let t = crate::clock::civil(T0, 0);
    let at = |h: i64| crate::clock::midnight_ms(t.year, t.month, t.day, 0) + h * 3_600_000;
    let now = at(10);
    let ev = |title: &str, s: i64, e: i64| Event { id: title.into(), title: title.into(), start_ms: at(s), end_ms: at(e), all_day: false, url: None, color: None, calendar: "c".into(), location: String::new() };
    i.data.events = vec![ev("early", 1, 2), ev("later", 16, 17)];
    i.reminders = vec![Reminder { due_ms: at(14), text: "call".into(), ..Default::default() }];
    i.push(now, Input::OpenPage(Page::Today));
    i.now = now;
    let (items, now_at, _) = i.today_view();
    assert_eq!(items.iter().map(|x| x.title.as_str()).collect::<Vec<_>>(), ["early", "call", "later"]);
    assert_eq!(now_at, 1, "the now line is after the finished event");
    assert!(i.pages().windows(2).any(|w| w == [Page::Home, Page::Today]), "the dock shows it after the home page");
    draw_frames(&mut i, 200);
    assert_eq!(i.current_face(), Face::Today);
}

#[test]
fn the_pill_reminds_earlier_than_the_card_when_asked() {
    let mut i = Island::new(Config { meeting_lead_ms: 10 * 60_000, meeting_pill_ms: 45 * 60_000, ..Default::default() });
    i.data.events = vec![Event { id: "e".into(), title: "Seminar".into(), start_ms: T0 + 30 * 60_000, end_ms: T0 + 60 * 60_000, all_day: false, url: None, color: None, calendar: "c".into(), location: String::new() }];
    i.push(T0, Input::OpenKeyboard(None));
    i.now = T0;
    assert!(i.soon().is_some(), "30 min away: inside the 45 min of the pill");
    assert!(i.announcer.new_heads_up(&i.data.events, T0, i.cfg.meeting_lead_ms).is_none(), "but outside the 10 min of the card");
    i.cfg.meeting_pill_ms = 20 * 60_000;
    assert!(i.soon().is_none(), "a shorter pill lead hides it");
}

#[test]
fn rain_or_a_low_battery_before_an_event_comes_as_one_card() {
    let mut i = Island::new(Config::default());
    i.data.events = vec![Event { id: "e".into(), title: "Lecture".into(), start_ms: T0 + 30 * 60_000, end_ms: T0 + 90 * 60_000, ..Default::default() }];
    i.data.battery = Some(Battery { percent: 21.0, charging: false, on_ac: false });
    run(&mut i, 1000);
    let card = i.notif.clone().expect("a card");
    assert_eq!(card.summary, "Lecture in 30 min");
    assert_eq!(card.body, "battery 21%: bring the charger");
    i.notif = None;
    run(&mut i, 3000);
    assert!(i.notif.is_none(), "once");
    let mut j = Island::new(Config::default());
    j.data.events = i.data.events.clone();
    j.data.battery = Some(Battery { percent: 90.0, charging: false, on_ac: false });
    run(&mut j, 2000);
    assert!(j.notif.is_none(), "nothing to say, nothing said");
}

#[test]
fn today_shows_the_time_to_leave_and_the_free_time_between_lectures() {
    let mut i = commuter();
    i.tick(T0, 8.0);
    let (items, _, _) = i.today_view();
    assert_eq!(i.today_gaps(&items), 1, "Reti ends 11:00, Lab starts 13:00; the call and Reti are only 30 minutes apart");
    let with_gap = i.today_height();
    i.data.events.retain(|e| e.title != "Lab");
    assert_eq!(with_gap - i.today_height(), 28.0 + 18.0, "the Lab row and the gap row are gone");
}

#[test]
fn the_files_of_the_next_lecture_are_asked_for_shown_on_today_and_opened() {
    use island_core::materials::Material;
    let mut i = Island::new(Config::default());
    // 09:00 and 15:00 lectures, now 08:00
    let ev = |id: &str, title: &str, h: i64| Event { id: id.into(), title: title.into(), start_ms: T0 + h * 3_600_000, end_ms: T0 + (h + 1) * 3_600_000, ..Default::default() };
    i.data.events = vec![ev("a", "Reti di calcolatori", 1), ev("b", "Lab", 7)];
    i.tick(T0, 8.0);
    let cmds = i.take_commands();
    assert!(cmds.contains(&Command::FindMaterials("Reti di calcolatori".into())), "{cmds:?}");
    assert!(!cmds.iter().any(|c| matches!(c, Command::FindMaterials(t) if t == "Lab")), "a three-letter title has nothing to look for");
    i.tick(T0 + 60_000, 8.0);
    assert!(!i.take_commands().iter().any(|c| matches!(c, Command::FindMaterials(_))), "not again within ten minutes");
    let plain = i.today_height();
    i.data.materials.insert(island_core::materials::fold("Reti di calcolatori"), (T0, vec![Material { path: "/home/u/Reti/slides-03.pdf".into(), name: "slides-03.pdf".into(), modified_ms: T0 - 86_400_000 }]));
    assert_eq!(i.today_height(), plain + 22.0 + 28.0, "a heading and a row");
    i.push(T0, Input::OpenPage(Page::Today));
    draw_frames(&mut i, 200);
    assert!(i.hits.iter().any(|h| h.action == Action::OpenFile("/home/u/Reti/slides-03.pdf".into())));
    assert_eq!(i.activate(Action::OpenFile("/home/u/Reti/slides-03.pdf".into())), vec![Command::OpenFile("/home/u/Reti/slides-03.pdf".into())]);
    // a card line for the lecture
    let line = i.files_line(&i.today_left()).unwrap();
    assert_eq!(line, "Files for Reti: slides-03.pdf");
    // ten minutes later it is looked up again
    i.take_commands();
    i.tick(T0 + 11 * 60_000, 8.0);
    assert!(i.take_commands().contains(&Command::FindMaterials("Reti di calcolatori".into())));
}

#[test]
fn a_reminder_that_repeats_is_set_again_when_it_rings() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0); // Friday 15 January 2027, 08:00
    run_bar(&mut i, "remind every monday 9 gym");
    assert_eq!(i.reminders().len(), 1);
    let first = i.reminders()[0].due_ms;
    assert_eq!(first, T0 + 3 * 86_400_000 + 3_600_000, "Monday at nine");
    i.tick(first + 1000, 8.0);
    assert_eq!(i.reminders().len(), 1, "it rang and is set again");
    assert_eq!(i.reminders()[0].due_ms, first + 7 * 86_400_000, "the next Monday, same time");
    run_bar(&mut i, "remind every day 8:30 vitamins");
    let daily = i.reminders().iter().find(|r| r.text == "vitamins").unwrap().due_ms;
    assert!(daily > i.now && daily - i.now <= 86_400_000, "today's time is gone, so tomorrow");
}

#[test]
fn an_event_from_the_bar_shows_at_once_goes_to_the_calendar_and_flags_an_overlap() {
    let mut i = Island::new(Config::default());
    i.data.events = vec![Event { id: "l".into(), title: "Algoritmi".into(), start_ms: T0 + 2 * 3_600_000, end_ms: T0 + 4 * 3_600_000, ..Default::default() }];
    i.tick(T0, 8.0); // 08:00
    type_in_bar(&mut i, "event 9-10:30 Office hours");
    i.push(i.now, Input::Key(KeyInput::Enter));
    let cmds = i.take_commands();
    assert!(cmds.iter().any(|c| matches!(c, Command::CreateEvent { title, start_ms, end_ms, .. } if title == "Office hours" && *start_ms == T0 + 3_600_000 && *end_ms == T0 + 3_600_000 + 5_400_000)));
    assert!(i.data.events.iter().any(|e| e.title == "Office hours"), "shown before the server answers");
    let n = i.notif.take();
    assert!(n.is_some_and(|n| n.summary.contains("overlaps") && n.body.contains("Algoritmi")));
    run_bar(&mut i, "event 15 Dentist");
    assert!(i.notif.as_ref().is_none_or(|n| !n.summary.contains("overlaps")));
}

#[test]
fn five_minutes_before_the_end_of_an_event_a_card_says_what_comes_next() {
    let mut i = Island::new(Config { meeting_alerts: false, ..Config::default() });
    i.tick(T0, 8.0); // 08:00
    let h = 3_600_000;
    let ev = |t: &str, a: i64, b: i64, place: &str| Event { id: t.into(), title: t.into(), start_ms: a, end_ms: b, location: place.into(), ..Default::default() };
    // a lecture entered twice, a short note, then a gap and another lecture
    i.data.events = vec![ev("Reti", T0, T0 + 2 * h, ""), ev("Reti bis", T0, T0 + 2 * h, ""), ev("Water", T0 + h, T0 + h + 600_000, ""), ev("Algoritmi", T0 + 3 * h, T0 + 5 * h, "Aula 3, Polo Fibonacci, Pisa")];
    i.tick(T0 + h + 6 * 60_000, 8.0);
    assert!(i.notif.is_none(), "a short entry ends without a word");
    i.tick(T0 + 2 * h - 6 * 60_000, 8.0);
    assert!(i.notif.is_none(), "not yet");
    i.tick(T0 + 2 * h - 5 * 60_000, 8.0);
    let card = i.notif.take().expect("five minutes before");
    assert_eq!(card.summary, "Reti ends in 5 min");
    assert!(card.body.starts_with("Free until 11:00, then Algoritmi · ") && card.body.contains("Aula 3"), "{}", card.body);
    i.tick(T0 + 2 * h - 60_000, 8.0);
    assert!(i.notif.is_none(), "once, also for the one entered twice");
    i.tick(T0 + 5 * h - 60_000, 8.0);
    assert_eq!(i.notif.take().map(|c| (c.summary, c.body)), Some(("Algoritmi ends in 1 min".into(), "Nothing else today".into())));
    let mut off = Island::new(Config { end_alerts: false, meeting_alerts: false, ..Config::default() });
    off.data.events = vec![ev("Reti", T0, T0 + 2 * h, "")];
    off.tick(T0 + 2 * h - 60_000, 8.0);
    assert!(off.notif.is_none());
}

#[test]
fn a_whole_day_event_has_no_hours_and_blocks_nothing() {
    let mut i = Island::new(Config::default());
    i.data.events = vec![Event { id: "l".into(), title: "Algoritmi".into(), start_ms: T0 + 2 * 3_600_000, end_ms: T0 + 4 * 3_600_000, ..Default::default() }];
    i.tick(T0, 8.0);
    type_in_bar(&mut i, "event today all day Conference");
    i.push(i.now, Input::Key(KeyInput::Enter));
    let cmds = i.take_commands();
    assert!(cmds.iter().any(|c| matches!(c, Command::CreateEvent { title, whole_day: Some(_), .. } if title == "Conference")));
    assert!(i.data.events.iter().any(|e| e.title == "Conference" && e.all_day));
    assert!(i.notif.take().is_some_and(|n| n.summary == "Event added" && n.body.contains("all day")));
}

#[test]
fn an_event_is_moved_keeping_its_length_and_cancelled() {
    let mut i = Island::new(Config::default());
    i.data.events = vec![
        Event { id: "lab-1@x".into(), title: "Lab".into(), start_ms: T0 + 3_600_000, end_ms: T0 + 3 * 3_600_000, ..Default::default() },
        Event { id: "dent-1@y".into(), title: "Dentist".into(), start_ms: T0 + 5 * 3_600_000, end_ms: T0 + 6 * 3_600_000, ..Default::default() },
    ];
    i.tick(T0, 8.0);
    run_bar(&mut i, "move lab to tomorrow 10");
    let cmds = i.take_commands();
    let Some(Command::MoveEvent { id, start_ms, end_ms, .. }) = cmds.first() else { panic!("{cmds:?}") };
    assert_eq!(id, "lab-1@x");
    assert_eq!(end_ms - start_ms, 2 * 3_600_000, "the length stays");
    assert!(*start_ms > T0 + 12 * 3_600_000);
    run_bar(&mut i, "cancel dentist");
    assert!(matches!(i.take_commands().as_slice(), [Command::CancelEvent { id, .. }] if id == "dent-1@y"));
    assert!(!i.data.events.iter().any(|e| e.title == "Dentist"));
    run_bar(&mut i, "cancel nothing like this");
    assert!(i.take_commands().is_empty());
}

#[test]
fn a_reminder_can_be_tied_to_the_next_event() {
    let mut i = Island::new(Config::default());
    i.data.events = vec![
        Event { id: "a".into(), title: "Lab".into(), start_ms: T0 + 3_600_000, end_ms: T0 + 2 * 3_600_000, ..Default::default() },
        Event { id: "b".into(), title: "Lab".into(), start_ms: T0 + 86_400_000, end_ms: T0 + 86_400_000 + 3_600_000, ..Default::default() },
    ];
    i.tick(T0, 8.0);
    run_bar(&mut i, "remind 10m before lab");
    assert_eq!(i.reminders.len(), 1);
    assert_eq!(i.reminders[0].due_ms, T0 + 3_600_000 - 600_000, "the nearest one, ten minutes before");
    assert!(i.reminders[0].text.contains("Lab"));
    run_bar(&mut i, "remind 2h before lab");
    assert_eq!(i.reminders.len(), 2, "too late for the first, so the next day's");
    assert_eq!(i.reminders[1].due_ms, T0 + 86_400_000 - 7_200_000);
    i.notif = None;
    run_bar(&mut i, "remind 5m before chemistry");
    assert_eq!(i.reminders.len(), 2);
}

#[test]
fn a_reminder_card_offers_to_ring_again_later() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0); // 08:00
    i.reminders = vec![Reminder { due_ms: T0 + 1000, text: "Call mom".into(), ..Default::default() }];
    i.tick(T0 + 2000, 8.0);
    let n = i.notif.clone().expect("the reminder rings");
    let keys: Vec<&str> = n.actions.iter().map(|a| a.key.as_str()).collect();
    assert_eq!(keys, ["open-island:snooze:1h", "open-island:snooze:tonight", "open-island:snooze:tomorrow"]);
    i.activate(Action::NotifAction("open-island:snooze:tonight".into()));
    assert_eq!(i.reminders.len(), 1);
    assert_eq!(i.reminders[0].text, "Call mom");
    assert_eq!(i.reminders[0].due_ms, i.day_start_ms(i.today_day()) + 20 * 3_600_000);
}
