//! Countdowns, the Pomodoro and the focus.

use super::*;

#[test]
fn a_timer_counts_down_ends_and_announces() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::StartTimer(1));
    run(&mut i, 500);
    assert!(i.timer.is_some());
    run(&mut i, 1500);
    assert!(i.timer.is_none(), "a one-second timer is over");
    assert!(i.pending.contains(&Command::PlaySound));
    run(&mut i, 100);
    assert_eq!((i.current_face(), up(&i)), (Face::Idle, vec!["Time's up"]), "a card drops, the pill stays");
    run(&mut i, 11_000);
    assert!(up(&i).is_empty(), "the card goes away on its own");
}

#[test]
fn pomodoro_goes_on_to_the_break() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::StartPomodoro);
    i.activate(Action::PomodoroSkip);
    run(&mut i, 100);
    let t = i.timer.as_ref().expect("the break timer");
    assert_eq!(t.phase, Phase::Break);
    assert_eq!(t.total_ms, 5 * 60_000);
}

#[test]
fn focus_shows_its_minutes_and_a_bubble_beside_music() {
    let mut i = Island::new(Config::default());
    i.data.focus = Some(Focus { active: true, end_epoch: Some(T0 / 1000 + 23 * 60 - 5) });
    run(&mut i, 600);
    assert_eq!(i.current_face(), Face::FocusMini);
    assert_eq!(i.focus_minutes_left(), Some(23));
    i.data = data();
    i.data.focus = Some(Focus { active: true, end_epoch: None });
    run(&mut i, 600);
    assert_eq!(i.current_face(), Face::MediaMini);
    assert_eq!(i.shown_side, Some(Side::Focus));
    assert_eq!(i.focus_minutes_left(), None);
    i.data.focus = Some(Focus { active: false, end_epoch: None });
    assert!(!i.focus_active());
}

#[test]
fn countdowns_run_side_by_side_and_the_soonest_is_the_main_one() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::StartTimer(600));
    i.push(T0, Input::StartTimer(60));
    assert_eq!(i.timer.as_ref().map(|t| t.total_ms), Some(60_000), "the one that ends first is the main one");
    assert_eq!(i.extra_timers.len(), 1);
    assert_eq!(i.extra_timers[0].total_ms, 600_000, "the long one runs beside it, it is not replaced");
    run(&mut i, 61_000);
    assert!(i.pending.contains(&Command::PlaySound), "the short one rang");
    assert_eq!(i.timer.as_ref().map(|t| t.total_ms), Some(600_000), "the next countdown takes its place");
    assert!(i.extra_timers.is_empty());
    i.pending.clear();
    i.push(i.now, Input::CancelTimer);
    assert!(i.timer.is_none() && i.extra_timers.is_empty());
}

#[test]
fn a_pomodoro_keeps_the_main_place_and_the_countdown_rings_beside_it() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::StartTimer(30));
    i.push(T0, Input::StartPomodoro);
    assert_eq!(i.timer.as_ref().map(|t| t.mode), Some(TimerMode::Pomodoro), "the Pomodoro is the main timer");
    assert_eq!(i.extra_timers.len(), 1, "the countdown that was running was kept");
    i.push(i.now, Input::StartTimer(10));
    assert_eq!(i.timer.as_ref().map(|t| t.mode), Some(TimerMode::Pomodoro), "a new countdown does not push the Pomodoro out");
    assert_eq!(i.extra_timers.len(), 2);
    assert_eq!(i.extra_timers[0].total_ms, 10_000, "soonest first");
    run(&mut i, 11_000);
    assert!(i.pending.contains(&Command::PlaySound));
    assert_eq!(i.extra_timers.len(), 1, "the ten-second one rang beside the Pomodoro");
    assert_eq!(i.timer.as_ref().map(|t| t.mode), Some(TimerMode::Pomodoro));
}

#[test]
fn an_extra_countdown_can_be_cancelled_and_has_room_on_the_timer_page() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::StartTimer(60));
    let alone = Face::TimerFull.size().1;
    i.push(i.now, Input::StartTimer(600));
    i.push(i.now, Input::StartTimer(900));
    assert_eq!(i.extra_timers.len(), 2);
    i.push(i.now, Input::OpenPage(Page::Timer));
    run(&mut i, 1200);
    let fr = i.tick(i.now + 8, 8.0);
    assert!(fr.surface.1 >= alone + 2.0 * 28.0 - 1.0, "two rows more than the timer alone: {fr:?}");
    i.activate(Action::TimerCancelExtra(0));
    assert_eq!(i.extra_timers.len(), 1);
    assert_eq!(i.extra_timers[0].total_ms, 900_000);
}

#[test]
fn until_class_starts_a_timer_that_ends_five_minutes_before() {
    let mut i = commuter();
    i.tick(T0, 8.0);
    type_in_bar(&mut i, "until class");
    i.push(i.now, Input::Key(KeyInput::Enter));
    let t = i.timer.as_ref().expect("a timer");
    assert_eq!(t.label, "Before Call");
    assert_eq!(t.total_ms, 25 * 60_000, "the call is at 08:30, the timer ends at 08:25");
    i.push(i.now, Input::CancelTimer);
    i.notif = None;
    i.tick(T0 + 28 * 60_000, 8.0);
    type_in_bar(&mut i, "until class");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.timer.is_none(), "with less than six minutes left there is nothing to count down to");
    assert_eq!(card(&i).map(|c| c.0), Some("Almost time".to_string()));
}

#[test]
fn a_countdown_that_ran_out_can_have_another_minute() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::StartNamedTimer(1, "tea".into()));
    run(&mut i, 2200);
    let card = i.notif.clone().expect("time's up");
    assert_eq!((card.body.as_str(), card.actions[0].label.as_str()), ("tea", "+1 min"));
    i.activate(Action::OnCard(card.id, Box::new(Action::NotifAction(card.actions[0].key.clone()))));
    let t = i.timer.as_ref().expect("a minute more");
    assert_eq!((t.total_ms, t.label.as_str()), (60_000, "tea"));
}
