//! Do not disturb, the quiet the island keeps by itself, and what it tells afterwards.

use super::*;

fn count_cards(i: &Island) -> usize {
    i.notif.is_some() as usize + i.waiting.len()
}

#[test]
fn notifications_are_held_during_a_calendar_event_and_come_back_after() {
    let mut i = Island::new(Config::default());
    i.data.events = vec![Event { id: "e".into(), title: "Lesson".into(), start_ms: T0 + 60_000, end_ms: T0 + 3_600_000, ..Default::default() }];
    let n = |id: u32| Input::Notification(Incoming { id, app: "Chat".into(), summary: format!("m{id}"), ..Default::default() });
    run(&mut i, 5000);
    i.push(i.now, n(1));
    assert_eq!(count_cards(&i), 1, "before the event: shown");
    run(&mut i, 20_000);
    run(&mut i, 61_000 - 25_000);
    assert_eq!(i.auto_quiet_reason(), Some("meeting"));
    assert!(i.take_commands().contains(&Command::SetDnd(true)), "the daemon is told, so it stays silent too");
    i.push(i.now, n(2));
    assert_eq!(count_cards(&i), 0, "during the event: held back");
    assert_eq!(i.history.len(), 2, "but kept in the history");
    i.push(i.now, Input::Notification(Incoming { id: 3, critical: true, ..Default::default() }));
    assert_eq!(count_cards(&i), 1, "a critical one still gets through");
    run(&mut i, 3_600_000);
    assert_eq!(i.auto_quiet_reason(), None);
    assert!(i.take_commands().contains(&Command::SetDnd(false)));
}

#[test]
fn the_microphone_and_a_screen_share_hold_notifications_unless_switched_off() {
    let mut i = Island::new(Config::default());
    i.data.privacy.mic = true;
    run(&mut i, 2000);
    assert_eq!(i.auto_quiet_reason(), Some("microphone"));
    i.data.privacy.screen = true;
    run(&mut i, 2000);
    assert_eq!(i.auto_quiet_reason(), Some("screen share"));
    i.data.privacy = Default::default();
    run(&mut i, 2000);
    assert!(!i.quiet());
    let mut j = Island::new(Config { quiet_on_mic: false, quiet_on_screen_share: false, quiet_in_meetings: false, ..Config::default() });
    j.data.privacy.mic = true;
    j.data.privacy.screen = true;
    j.data.events = vec![Event { id: "e".into(), start_ms: 0, end_ms: i64::MAX / 2, ..Default::default() }];
    run(&mut j, 2000);
    assert!(!j.quiet(), "all three can be switched off");
}

#[test]
fn an_all_day_or_very_long_event_does_not_silence_the_island() {
    let mut i = Island::new(Config::default());
    i.data.events = vec![
        Event { id: "a".into(), start_ms: 0, end_ms: i64::MAX / 2, all_day: true, ..Default::default() },
        Event { id: "b".into(), start_ms: 0, end_ms: i64::MAX / 2, ..Default::default() },
    ];
    run(&mut i, 2000);
    assert!(!i.quiet());
}

#[test]
fn what_was_held_back_comes_as_one_recap_that_opens_the_list() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::SetDnd(true));
    let n = |id: u32, app: &str| Input::Notification(Incoming { id, app: app.into(), summary: format!("m{id}"), ..Default::default() });
    run(&mut i, 1000);
    for (id, app) in [(1, "Telegram"), (2, "Telegram"), (3, "Mail"), (4, "Telegram")] {
        i.push(i.now, n(id, app));
    }
    assert!(i.notif.is_none(), "held");
    i.push(i.now, Input::SetDnd(false));
    run(&mut i, 1000);
    let card = i.notif.clone().expect("the recap");
    assert_eq!((card.summary.as_str(), card.body.as_str()), ("4 held back", "Telegram 3 · Mail"));
    assert_eq!(i.history.len(), 4, "the recap is not a notification of its own");
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::NotifOpen), "a click on it");
    assert!(i.take_commands().iter().all(|c| !matches!(c, Command::FocusApp { .. })), "does not look for a window");
    run(&mut i, 600);
    assert_eq!(i.current_face(), Face::Notifs);
}

#[test]
fn coming_back_after_a_while_shows_what_came_in() {
    let mut i = Island::new(Config::default());
    let n = |id: u32, app: &str| Input::Notification(Incoming { id, app: app.into(), summary: format!("m{id}"), ..Default::default() });
    run(&mut i, 1000);
    i.push(i.now, n(1, "Telegram"));
    i.push(i.now, n(2, "Mail"));
    i.push(i.now, Input::NotificationClosed(1));
    i.push(i.now, Input::NotificationClosed(2));
    run(&mut i, 40_000);
    i.push(i.now, Input::Returned { away_ms: 30_000 });
    assert!(i.notif.is_none(), "half a minute is a pause, not an absence");
    i.push(i.now, Input::Returned { away_ms: 120_000 });
    let card = i.notif.clone().expect("the recap");
    assert_eq!((card.summary.as_str(), card.body.as_str()), ("2 while you were away", "Mail · Telegram"));
    i.cfg.away_recap = false;
    i.notif = None;
    i.push(i.now, Input::Returned { away_ms: 120_000 });
    assert!(i.notif.is_none(), "switched off");
}

#[test]
fn nothing_held_means_no_recap_and_a_reminder_gets_through_a_meeting() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::SetDnd(true));
    run(&mut i, 1000);
    i.push(i.now, Input::SetDnd(false));
    run(&mut i, 1000);
    assert!(i.notif.is_none(), "quiet but nothing came: no card");
    i.push(i.now, Input::SetDnd(true));
    i.restore_reminders(vec![Reminder { due_ms: i.now + 2000, text: "Call".into(), ..Default::default() }]);
    run(&mut i, 3000);
    assert_eq!(i.notif.as_ref().map(|n| n.summary.as_str()), Some("Call"), "a reminder is not held back");
}

#[test]
fn a_study_focus_holds_notifications_too() {
    let mut i = Island::new(Config::default());
    i.data.focus = Some(Focus { active: true, end_epoch: None });
    run(&mut i, 2000);
    assert_eq!(i.auto_quiet_reason(), Some("focus"));
}

#[test]
fn dnd_from_a_script_and_spectrum_levels() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::SetDnd(true));
    assert!(i.data.dnd);
    assert_eq!(i.take_commands(), vec![Command::SetDnd(true)]);
    assert_eq!(i.dnd_changed(), Some((true, None)));
    assert_eq!(i.dnd_changed(), None);
    i.data = data();
    run(&mut i, 600);
    assert!(i.wants_spectrum(), "music plays on mains power");
    i.data.battery = Some(Battery { percent: 50.0, charging: false, on_ac: false });
    assert!(!i.wants_spectrum(), "not on battery");
    i.push(i.now, Input::Spectrum(vec![0.5; 8]));
    assert_eq!(i.spectrum.len(), 8);
}

#[test]
fn pressing_a_lit_dnd_turns_it_off_also_when_the_island_lit_it_by_itself() {
    let mut i = Island::new(Config::default());
    let share = |i: &mut Island, on: bool| {
        i.data.privacy.screen = on;
        run(i, 500);
    };
    share(&mut i, true);
    assert!(i.quiet(), "lit by the screen share");
    i.take_commands();
    let told = i.activate(Action::ToggleDnd);
    assert!(!i.quiet() && !i.data.dnd, "off, and nothing switched on behind it");
    assert_eq!(told, [Command::SetDnd(false)], "the daemon plays sounds again");
    run(&mut i, 2000);
    assert!(!i.quiet(), "and it stays off while the same reason lasts");
    i.push(i.now, Input::Notification(Incoming { id: 1, app: "Chat".into(), summary: "m".into(), ..Default::default() }));
    assert!(i.notif.is_some(), "shown");
    share(&mut i, false);
    assert!(!i.quiet(), "nothing is left behind when the reason ends");
    share(&mut i, true);
    assert_eq!(i.auto_quiet_reason(), Some("screen share"), "the next time it holds them again");
}

#[test]
fn turning_dnd_off_silences_every_reason_there_is_but_not_the_next_one() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::SetDnd(true));
    i.data.privacy.screen = true;
    i.data.privacy.mic = true;
    run(&mut i, 500);
    i.activate(Action::ToggleDnd);
    run(&mut i, 500);
    assert!(!i.quiet(), "the manual one, the share and the call behind it: all off");
    // one that was not there when it was switched off does hold
    i.data.focus = Some(Focus { active: true, ..Default::default() });
    run(&mut i, 500);
    assert_eq!(i.auto_quiet_reason(), Some("focus"));
    // and by hand it can be switched on at any time, which forgets what was silenced
    i.activate(Action::ToggleDnd);
    i.data.focus = None;
    run(&mut i, 500);
    i.activate(Action::ToggleDnd);
    assert!(i.quiet() && i.data.dnd);
    i.activate(Action::ToggleDnd);
    assert!(!i.quiet());
}

#[test]
fn dnd_typed_in_the_bar_or_sent_from_outside_switches_it() {
    let mut i = Island::new(Config::default());
    run(&mut i, 100);
    i.push(i.now, Input::RunLine("dnd on".into()));
    assert!(i.data.dnd && i.quiet(), "on");
    assert_eq!(i.dnd_changed(), Some((true, None)), "and written down");
    i.take_commands();
    i.push(i.now, Input::RunLine("dnd off".into()));
    assert!(!i.data.dnd && !i.quiet(), "off");
    assert_eq!(i.dnd_changed(), Some((false, None)));
    assert!(i.take_commands().contains(&Command::SetDnd(false)));
    // over an automatic reason too
    for off in [Input::RunLine("dnd off".into()), Input::SetDnd(false)] {
        let mut i = Island::new(Config::default());
        i.data.privacy.screen = true;
        run(&mut i, 500);
        i.push(i.now, off);
        run(&mut i, 500);
        assert!(!i.quiet(), "asked off, it is off");
    }
}

#[test]
fn twenty_minutes_on_work_apps_hold_the_notifications_until_the_person_leaves() {
    let mut i = Island::new(Config::default());
    let n = |id: u32| Input::Notification(Incoming { id, app: "Telegram".into(), summary: format!("m{id}"), ..Default::default() });
    run(&mut i, 1000);
    i.push(i.now, Input::Focused { app: "code".into(), title: "main.rs".into() });
    run(&mut i, 10 * 60_000);
    i.push(i.now, Input::Focused { app: "org.gnome.Evince".into(), title: "paper.pdf".into() });
    run(&mut i, 5 * 60_000);
    i.push(i.now, n(1));
    assert!(i.notif.is_some(), "fifteen minutes is not yet deep work");
    i.notif = None;
    run(&mut i, 6 * 60_000);
    assert_eq!(i.auto_quiet_reason(), Some("deep work"));
    i.push(i.now, n(2));
    assert!(i.notif.is_none(), "held");
    i.push(i.now, Input::Focused { app: "zen".into(), title: "news".into() });
    run(&mut i, 64_000);
    assert_eq!(i.auto_quiet_reason(), None, "a long detour ends it");
    assert!(i.notif.as_ref().is_some_and(|c| c.summary.contains("held back")), "and the recap comes");
    i.cfg.deep_work_minutes = 0;
    i.notif = None;
    i.push(i.now, Input::Focused { app: "code".into(), title: "x".into() });
    run(&mut i, 30 * 60_000);
    assert_eq!(i.auto_quiet_reason(), None, "switched off");
}

#[test]
fn a_do_not_disturb_with_a_length_ends_by_itself() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0); // 08:00
    run_bar(&mut i, "dnd 1h");
    i.tick(i.now + 8, 8.0);
    assert!(i.quiet() && i.dnd_until == Some(T0 + 3_600_000), "{:?}", i.dnd_until);
    assert_eq!(i.dnd_changed(), Some((true, Some(T0 + 3_600_000))), "kept with its end");
    i.tick(T0 + 3_600_000 - 1000, 8.0);
    assert!(i.quiet());
    i.tick(T0 + 3_600_000, 8.0);
    assert!(!i.quiet() && i.dnd_until.is_none());
    assert_eq!(i.dnd_changed(), Some((false, None)));
    // pressing the button ends it, and one switched on by hand has no end
    run_bar(&mut i, "dnd 2h");
    i.activate(Action::ToggleDnd);
    assert!(!i.quiet() && i.dnd_until.is_none());
    run_bar(&mut i, "dnd 2h");
    run_bar(&mut i, "dnd on");
    assert!(i.dnd_until.is_none() && i.data.dnd);
    run_bar(&mut i, "dnd off");
    // until the lecture ends: the one that is on
    let h = 3_600_000;
    run_bar(&mut i, "dnd until class ends");
    assert!(!i.data.dnd, "nothing is on: nothing to wait for");
    i.data.events = vec![Event { id: "l".into(), title: "Reti".into(), start_ms: i.now - h, end_ms: i.now + h, ..Default::default() }];
    let end = i.now + h;
    run_bar(&mut i, "dnd until class ends");
    assert_eq!((i.data.dnd, i.dnd_until), (true, Some(end)));
    // a restart brings the end back
    let mut next = Island::new(Config::default());
    next.restore_dnd_until(end);
    next.tick(T0, 8.0);
    assert!(next.quiet() && next.dnd_changed().is_none());
    next.tick(end + 1, 8.0);
    assert!(!next.data.dnd);
}
