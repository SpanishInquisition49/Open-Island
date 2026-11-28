//! The morning and evening cards.

use super::*;

fn evening_island() -> Island {
    // T0 is Friday 2027-01-15 08:00 UTC; the recap is at 18:30
    let mut i = Island::new(Config { evening_recap_min: 18 * 60 + 30, ..Config::default() });
    let ev = |id: &str, h: i64| Event { id: id.into(), title: id.into(), start_ms: T0 + (16 + h) * 3_600_000, end_ms: T0 + (17 + h) * 3_600_000, ..Default::default() };
    i.data.events = vec![ev("Reti", 1), ev("Lab", 6)];
    i
}

fn morning_island() -> Island {
    // 07:30, from 07:00 on; Friday 2027-01-15, events today at 09:00 and 13:00 (T0 is 08:00 UTC)
    let mut i = Island::new(Config { morning_recap_min: 7 * 60, meeting_alerts: false, ..Config::default() });
    let ev = |id: &str, h: i64| Event { id: id.into(), title: id.into(), start_ms: T0 + h * 3_600_000, end_ms: T0 + (h + 1) * 3_600_000, ..Default::default() };
    i.data.events = vec![ev("Reti", 1), ev("Lab", 5)];
    i
}

#[test]
fn tomorrow_shows_when_today_is_over() {
    let mut i = Island::new(Config::default());
    let ev = |id: &str, at: i64| Event { id: id.into(), title: id.into(), start_ms: T0 + at * 3_600_000, end_ms: T0 + at * 3_600_000 + 3_600_000, ..Default::default() };
    i.data.events = vec![ev("Lesson", 26), ev("Lab", 28)];
    i.tick(T0, 8.0);
    assert_eq!(i.tomorrow_view().len(), 2, "nothing today: the start of tomorrow");
    i.data.events.push(ev("Gym", 4));
    i.tick(T0, 8.0);
    assert!(i.tomorrow_view().is_empty(), "something is still ahead today");
    i.tick(T0 + 8 * 3_600_000, 8.0);
    assert_eq!(i.tomorrow_view().len(), 2, "today is over");
}

#[test]
fn the_evening_card_comes_once_at_the_chosen_time_with_tomorrow_in_it() {
    let mut i = evening_island();
    i.restore_todos(vec![Todo { text: "buy milk".into(), ..Default::default() }]);
    assert_eq!(i.weekday_at(T0), 4, "a Friday");
    i.tick(T0 + 10 * 3_600_000, 8.0); // 18:00
    assert!(card(&i).is_none(), "not yet");
    i.tick(T0 + 10 * 3_600_000 + 31 * 60_000, 8.0); // 18:31
    let (head, body) = card(&i).expect("the card");
    assert_eq!(head, "Tomorrow · 2 events, 1 to do");
    assert!(body.starts_with("01:00 Reti") && body.contains("06:00 Lab"), "{body}");
    assert_eq!(i.evening_day(), i.day_number(T0));
    i.notif = None;
    i.tick(T0 + 11 * 3_600_000, 8.0);
    assert!(card(&i).is_none(), "once a day");
}

#[test]
fn the_evening_card_skips_weekends_unless_asked_and_a_restart_does_not_repeat_it() {
    let mut i = evening_island();
    // Saturday 18:40
    let sat = T0 + 24 * 3_600_000 + 10 * 3_600_000 + 40 * 60_000;
    assert_eq!(i.weekday_at(sat), 5);
    i.tick(sat, 8.0);
    assert!(card(&i).is_none(), "weekend");
    i.cfg.evening_weekends = true;
    i.restore_todos(vec![Todo { text: "laundry".into(), ..Default::default() }]);
    i.tick(sat + 1000, 8.0);
    assert!(card(&i).is_some(), "asked for weekends");
    let mut j = evening_island();
    j.restore_evening_day(j.day_number(T0));
    j.tick(T0 + 11 * 3_600_000, 8.0);
    assert!(card(&j).is_none(), "shown before the restart");
    let mut late = evening_island();
    late.tick(T0 + 15 * 3_600_000 + 45 * 60_000, 8.0); // 23:45
    assert!(card(&late).is_none(), "too late to be useful");
}

#[test]
fn nothing_tomorrow_means_no_card_and_the_bar_can_ask_for_it_any_time() {
    let mut i = Island::new(Config { evening_recap_min: 18 * 60, ..Config::default() });
    i.tick(T0 + 10 * 3_600_000 + 5 * 60_000, 8.0);
    assert_eq!(i.evening_day(), i64::MIN, "the calendars may still be loading: wait");
    i.tick(T0 + 10 * 3_600_000 + 5 * 60_000 + 31_000, 8.0);
    assert!(card(&i).is_none(), "no noise when there is nothing to say");
    assert_eq!(i.evening_day(), i.day_number(T0), "and it does not ask again every frame");
    i.push(i.now, Input::OpenBar);
    for c in "recap".chars() {
        i.push(i.now, Input::Key(KeyInput::Text(c.to_string())));
    }
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert_eq!(card(&i).map(|c| c.0), Some("Nothing planned tomorrow".to_string()));
    i.notif = None;
    i.push(i.now, Input::TodoAdd("call Anna".into()));
    i.notif = None;
    assert!(i.show_evening());
    assert_eq!(card(&i).map(|c| c.0), Some("Tomorrow · 1 to do".to_string()));
}

#[test]
fn the_time_option_steps_from_off_to_the_evening() {
    let mut i = Island::new(Config::default());
    assert_eq!(crate::options::fmt_num(crate::options::get_num(&i.cfg, "evening_recap_time"), "time"), "off");
    i.activate(Action::Step("evening_recap_time", 1));
    assert_eq!(i.cfg.evening_recap_min, 18 * 60, "the first step lands on 18:00");
    i.activate(Action::Step("evening_recap_time", 1));
    assert_eq!(i.cfg.evening_recap_min, 18 * 60 + 15);
    for _ in 0..2 {
        i.activate(Action::Step("evening_recap_time", -1));
    }
    i.activate(Action::Step("evening_recap_time", -1));
    assert_eq!(i.cfg.evening_recap_min, 17 * 60 + 30);
}

#[test]
fn the_morning_card_shows_what_is_left_of_today_on_first_use() {
    let mut i = morning_island();
    i.restore_todos(vec![Todo { text: "buy milk".into(), ..Default::default() }]);
    i.tick(T0 - 3_600_000 - 30 * 60_000, 8.0); // 06:30: before the time
    assert!(card(&i).is_none());
    i.tick(T0 + 30 * 60_000, 8.0); // 08:30: the machine is opened late, still the morning
    let (head, body) = card(&i).expect("the card");
    assert_eq!(head, "Today · 2 events, 1 to do");
    assert!(body.starts_with("09:00 Reti") && body.contains("13:00 Lab"), "{body}");
    i.notif = None;
    i.tick(T0 + 3_600_000, 8.0);
    assert!(card(&i).is_none(), "once a day");
    assert_eq!(i.morning_day(), i.day_number(T0));
}

#[test]
fn the_morning_card_leaves_out_what_is_over_waits_for_the_calendars_and_ends_at_two() {
    let mut i = morning_island();
    i.tick(T0 + 3 * 3_600_000, 8.0); // 11:00: Reti (09:00) is over
    assert_eq!(card(&i).map(|c| c.0), Some("Today · 1 event".to_string()), "only Lab is left");
    // no events yet (the calendars are still loading): wait, do not mark the day
    let mut j = Island::new(Config { morning_recap_min: 7 * 60, ..Config::default() });
    j.restore_todos(vec![Todo { text: "x".into(), ..Default::default() }]);
    j.tick(T0, 8.0);
    assert!(card(&j).is_none(), "too early to know");
    assert_eq!(j.morning_day(), i64::MIN);
    j.data.events = vec![Event { id: "e".into(), title: "Late".into(), start_ms: T0 + 3_600_000, end_ms: T0 + 7_200_000, ..Default::default() }];
    j.tick(T0 + 1000, 8.0);
    assert_eq!(card(&j).map(|c| c.0), Some("Today · 1 event, 1 to do".to_string()), "the events arrived");
    let mut k = morning_island();
    k.tick(T0 + 6 * 3_600_000 + 1000, 8.0); // 14:00:01
    assert!(card(&k).is_none(), "after two it is no longer the morning");
    let mut w = morning_island();
    w.tick(T0 + 24 * 3_600_000 + 3_600_000, 8.0); // Saturday 09:00
    assert!(card(&w).is_none(), "weekend");
    w.push(w.now, Input::OpenBar);
    for c in "agenda".chars() {
        w.push(w.now, Input::Key(KeyInput::Text(c.to_string())));
    }
    w.push(w.now, Input::Key(KeyInput::Enter));
    assert_eq!(card(&w).map(|c| c.0), Some("Nothing left today".to_string()), "asked for by hand: the day has nothing");
}

#[test]
fn the_morning_time_option_starts_at_eight() {
    let mut i = Island::new(Config::default());
    i.activate(Action::Step("morning_recap_time", 1));
    assert_eq!(i.cfg.morning_recap_min, 8 * 60);
}

#[test]
fn the_cards_say_when_to_leave_and_a_card_comes_at_that_moment() {
    let mut i = commuter();
    i.tick(T0, 8.0);
    let (_, body) = i.evening_card().expect("tomorrow has a lecture");
    assert!(body.starts_with("Leave 08:30\n09:30 Reti domani"), "{body}");
    let (_, morning) = i.morning_card().expect("today has lectures");
    assert!(morning.starts_with("Leave 08:30\n"), "the video call is not a trip, Reti is: {morning}");
    assert!(card(&i).is_none());
    i.tick(T0 + 25 * 60_000, 8.0);
    assert!(card(&i).is_none(), "not yet");
    i.tick(T0 + 31 * 60_000, 8.0);
    let (head, text) = card(&i).expect("time to leave");
    assert_eq!(head, "Time to leave");
    assert_eq!(text, "Reti at 09:30 · Aula 3");
    assert!(i.pending.contains(&Command::PlaySound));
    i.notif = None;
    i.tick(T0 + 40 * 60_000, 8.0);
    assert!(card(&i).is_none(), "once");
    let mut late = commuter();
    late.tick(T0 + 80 * 60_000, 8.0); // 09:20: the PC is opened when it is too late to leave on time
    assert!(card(&late).is_none(), "a card for a moment long gone is noise");
    let mut none = commuter();
    none.cfg.commute_minutes = 0;
    none.tick(T0 + 31 * 60_000, 8.0);
    assert!(card(&none).is_none() && none.evening_card().unwrap().1.starts_with("09:30"), "no commute set: nothing about leaving");
}

#[test]
fn the_cards_end_with_the_weather_of_their_day() {
    use island_core::weather::Hour;
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0); // Friday 15 January 2027, 08:00 UTC
    i.restore_todos(vec![Todo { text: "buy milk".into(), ..Default::default() }]);
    let midnight = |d: i64| T0 - 8 * 3_600_000 + d * 86_400_000;
    let hour = |d: i64, h: i64, t: f32, rain: u8| Hour { time_ms: midnight(d) + h * 3_600_000, temp_c: t, code: 3, is_day: true, rain_prob: rain, mm: 0.0 };
    // today dry and 6 to 12 degrees, tomorrow warmer with rain from 16:00
    let mut hours = vec![];
    for h in 0..24 {
        hours.push(hour(0, h, 6.0 + (h - 7).clamp(0, 6) as f32, 5));
        hours.push(hour(1, h, 10.0 + (h - 7).clamp(0, 5) as f32, if h >= 16 { 80 } else { 5 }));
    }
    i.data.weather = Some(Weather { temp_c: 8.0, feels_c: 7.0, code: 3, day: true, high_c: 12.0, low_c: 6.0, rain_slots: vec![], hours });
    let (_, today) = i.morning_card().expect("a card");
    assert!(today.ends_with("6–12°, dry") || today.ends_with("7–12°, dry") || today.contains("°, dry"), "today: {today}");
    let (head, tomorrow) = i.evening_card().expect("a card");
    assert!(head.ends_with("rain from 16:00"), "rain is in the headline, where the card is read first: {head}");
    assert!(tomorrow.ends_with("rain from 16:00"), "tomorrow: {tomorrow}");
    assert!(!i.morning_card().unwrap().0.contains("rain"), "a dry day has nothing in its headline");
    i.data.weather = None;
    assert!(!i.evening_card().unwrap().1.contains('°'), "without a forecast, nothing about the weather");
}

#[test]
fn the_evening_card_leads_with_the_train_home_when_it_is_late_or_cancelled() {
    let mut i = evening_island();
    // 18:31 local is T0 + 10 h 31 min
    let now = T0 + 10 * 3_600_000 + 31 * 60_000;
    i.now = now;
    let pin = island_core::train::TrainPin { time: "18:50".into(), number: "4177".into(), ..Default::default() };
    i.data.train = Some(pin);
    let st = |d: Option<i32>, c: bool, at: i64| island_core::train::TrainStatus { delay_min: d, cancelled: c, at_ms: at, ..Default::default() };
    i.data.train_status = Some(st(Some(12), false, now - 60_000));
    assert_eq!(i.train_tonight().as_deref(), Some("18:50 train +12 min"));
    assert!(i.evening_card().unwrap().1.starts_with("18:50 train +12 min"), "first on the card");
    i.data.train_status = Some(st(None, true, now - 60_000));
    assert_eq!(i.train_tonight().as_deref(), Some("18:50 train cancelled"));
    i.data.train_status = Some(st(Some(1), false, now - 60_000));
    assert!(i.train_tonight().is_none(), "on time is not news");
    i.data.train_status = Some(st(Some(12), false, now - 30 * 60_000));
    assert!(i.train_tonight().is_none(), "an old reading is not trusted");
    i.data.train_status = Some(st(Some(12), false, now - 60_000));
    i.now = T0 + 11 * 3_600_000;
    assert!(i.train_tonight().is_none(), "it has left");
}

#[test]
fn the_cards_say_what_to_bring_for_the_courses_of_the_day() {
    let mut i = evening_island(); // tomorrow: events "Reti" and "Lab"
    i.tick(T0 + 10 * 3_600_000, 8.0); // 18:00 of the day before
    i.run_bar(island_core::bar::Cmd::Bring { text: "laptop".into(), course: "reti".into() });
    i.run_bar(island_core::bar::Cmd::Bring { text: "charger".into(), course: "chimica".into() });
    i.run_bar(island_core::bar::Cmd::Bring { text: "umbrella".into(), course: String::new() });
    assert_eq!(i.bring_list().len(), 3);
    let (_, body) = i.evening_card().expect("a card");
    assert!(body.contains("Bring laptop, umbrella"), "{body}");
    assert!(!body.contains("charger"), "no chemistry tomorrow: {body}");
    // without events a thing for no course alone still makes a card
    let mut j = Island::new(Config::default());
    assert!(j.evening_card().is_none());
    j.add_bring("umbrella", "");
    assert_eq!(j.evening_card().map(|c| c.1), Some("Bring umbrella".to_string()));
    j.remove_bring("all");
    assert!(j.bring_list().is_empty());
    // taking one off by a word of its course
    i.run_bar(island_core::bar::Cmd::Unbring("chim".into()));
    assert_eq!(i.bring_list().len(), 2);
}

#[test]
fn the_day_says_whether_the_battery_will_do_and_the_card_comes_once() {
    // T0 = Friday 08:00 UTC; lectures at 09:00 and 15:00 (an hour each), a 60 minute way home: back at 17:00
    let mut i = Island::new(Config { commute_minutes: 60, battery_alerts: true, meeting_alerts: false, ..Config::default() });
    let ev = |id: &str, h: i64| Event { id: id.into(), title: id.into(), start_ms: T0 + (1 + h) * 3_600_000, end_ms: T0 + (2 + h) * 3_600_000, ..Default::default() };
    i.data.events = vec![ev("Reti", 0), ev("Lab", 6)];
    i.restore_drain(Some(10.0));
    on_battery(&mut i, 50.0);
    i.tick(T0 + 3_600_000, 8.0); // 09:00, the first lecture has started
    let d = i.power_today().expect("a plan");
    // from 09:00 to 17:00 is 8 h at 10 %/h with 15 reserve: 95 %
    assert!(d.plan.short && (d.plan.need_pct - 95.0).abs() < 1.0, "{:?}", d.plan);
    let line = i.power_line(false, &i.today_left()).expect("a line for the morning card");
    assert!(line.starts_with("Battery 50%: the day needs ~95%"), "{line}");
    let card1 = i.notif.as_ref().map(|n| n.summary.clone());
    assert_eq!(card1.as_deref(), Some("The battery will not last the day"));
    i.notif = None;
    i.tick(T0 + 3_600_000 + 5_000, 8.0);
    assert!(i.notif.is_none(), "once a day");
    // plenty of charge: no card, no line
    let mut j = Island::new(Config { commute_minutes: 60, meeting_alerts: false, ..Config::default() });
    j.data.events = i.data.events.clone();
    j.restore_drain(Some(3.0));
    on_battery(&mut j, 90.0);
    j.tick(T0 + 3_600_000, 8.0);
    assert!(j.power_line(false, &j.today_left()).is_none() && j.notif.is_none());
    // nothing learned yet: nothing said
    let mut k = Island::new(Config { meeting_alerts: false, ..Config::default() });
    k.data.events = i.data.events.clone();
    on_battery(&mut k, 10.0);
    k.tick(T0 + 3_600_000, 8.0);
    assert!(k.power_today().is_none());
}

#[test]
fn the_days_agenda_is_shown_whole() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    // eight tasks left behind: the card names every one
    let yesterday = i.today_day() - 1;
    for n in 0..8 {
        i.todos.push(Todo { uid: format!("t{n}"), text: format!("task number {n} with a name long enough to matter"), due: Some(yesterday), ..Default::default() });
    }
    let (card, _) = i.with_leftovers(i.agenda_card("Today", &[]), true);
    let (_, body) = card.expect("a card");
    for n in 0..8 {
        assert!(body.contains(&format!("task number {n} ")), "{n} missing: {body}");
    }
    assert!(i.show_morning());
    let frame = card_frames(&mut i, &f, 200);
    let c = i.cards.live().next().expect("the card");
    let (_, y, _, h, _) = c.rect.expect("drawn");
    assert!(h > 150.0, "the card grew to fit its lines: {h}");
    assert!(frame.surface.1 >= y + h, "and the surface has room for it: {} < {}", frame.surface.1, y + h);
    assert_eq!(i.notif_until - c.born, 45_000);
}
