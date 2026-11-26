use super::*;
use crate::text::Fonts;
use island_core::calendar::Event;
use vello::Scene;
use island_core::pomodoro::Phase;

const T0: i64 = 1_800_000_000_000;

fn fonts() -> Fonts {
    Fonts::from_db(&fontdb::Database::new())
}

fn run(i: &mut Island, ms: i64) {
    let mut t = i.now.max(T0);
    let end = t + ms;
    while t < end {
        t += 8;
        i.tick(t, 8.0);
    }
}

#[allow(clippy::field_reassign_with_default)]
fn data() -> Data {
    let mut d = Data::default();
    d.battery = Some(Battery { percent: 50.0, charging: false, on_ac: true });
    d.media = Some(Media { track_id: "a".into(), title: "Song".into(), artist: "Artist".into(), playing: true, length_ms: 200_000.0, sampled_at_ms: T0, can_seek: true, ..Default::default() });
    d
}

#[test]
fn every_page_fits_in_the_fixed_surface() {
    // platforms that cannot resize their window smoothly (Windows) keep one window of MAX_SURFACE
    let mut mx = (0.0f64, 0.0f64);
    for page in [Page::Home, Page::Clip, Page::Timer, Page::Meeting, Page::Media, Page::Qs, Page::Notifs, Page::Settings, Page::Options, Page::Audio, Page::Today, Page::Send, Page::Activity, Page::Map, Page::Weather, Page::Cal, Page::Sys, Page::Focus, Page::Op] {
        let mut i = Island::new(Config::default());
        i.data = data();
        i.push(T0, Input::OpenPage(page));
        for _ in 0..400 {
            let t = i.now + 8;
            let fr = i.tick(t, 8.0);
            i.push(t, Input::OpenPage(page));
            mx = (mx.0.max(fr.surface.0), mx.1.max(fr.surface.1));
        }
    }
    assert!(mx.0 <= MAX_SURFACE.0 && mx.1 <= MAX_SURFACE.1, "a page needs {mx:?}, more than MAX_SURFACE {MAX_SURFACE:?}");
}

#[test]
fn every_face_draws_without_panicking_and_stays_finite() {
    let f = fonts();
    for page in [Page::Home, Page::Cal, Page::Qs, Page::Sys, Page::Notifs, Page::Media, Page::Timer, Page::Op, Page::Meeting] {
        let mut i = Island::new(Config::default());
        i.data = data();
        i.data.events = vec![Event { id: "e".into(), title: "Meeting".into(), start_ms: T0 + 5 * 60_000, end_ms: T0 + 3_600_000, url: Some("https://x".into()), ..Default::default() }];
        i.push(T0, Input::StartTimer(60));
        i.push(T0, Input::Progress("10 x".into()));
        i.push(T0, Input::Notification(Incoming { id: 1, app: "a".into(), summary: "s".into(), body: "b".into(), ..Default::default() }));
        i.push(T0, Input::OpenPage(page));
        for _ in 0..300 {
            let t = i.now + 8;
            let fr = i.tick(t, 8.0);
            assert!(fr.surface.0.is_finite() && fr.surface.1.is_finite() && fr.surface.0 < 2000.0 && fr.surface.1 < 1500.0, "{page:?}: {fr:?}");
            i.push(t, Input::OpenPage(page));
            let mut scene = Scene::new();
            i.draw(&mut scene, &f, fr.surface, 1.75);
        }
    }
}

#[test]
fn survives_absurd_time_steps() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenPage(Page::Cal));
    for dt in [-1e9, f64::NAN, f64::INFINITY, 1e12, 0.0] {
        let fr = i.tick(i.now + 8, dt);
        assert!(fr.surface.0.is_finite() && fr.surface.1.is_finite(), "dt {dt}");
    }
    run(&mut i, 3000);
    let fr = i.tick(i.now + 8, 8.0);
    assert!(fr.surface.1 > 100.0, "the calendar opened: {fr:?}");
}

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
    assert_eq!(i.current_face(), Face::TimerDone);
    run(&mut i, 7000);
    assert_eq!(i.current_face(), Face::Idle, "the card goes away on its own");
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
fn progress_flashes_once_then_becomes_a_bubble() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Progress("20 Backup".into()));
    run(&mut i, 600);
    assert_eq!(i.current_face(), Face::Progress);
    run(&mut i, 4000);
    assert_eq!(i.current_face(), Face::Idle);
    assert_eq!(i.shown_side, Some(Side::Op));
    i.push(i.now, Input::ProgressDone("done".into()));
    run(&mut i, 600);
    assert_eq!(i.current_face(), Face::Progress);
    run(&mut i, 6000);
    assert!(i.ops.op.is_none());
}

#[test]
fn a_shell_command_becomes_a_pill_only_after_its_threshold_and_ends_green_or_red() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::CommandStart { id: "1".into(), label: "make".into(), after_secs: 10 });
    run(&mut i, 5000);
    assert!(i.ops.op.is_none(), "a short command never shows");
    i.push(i.now, Input::CommandEnd { id: "1".into(), code: 0 });
    run(&mut i, 500);
    assert!(i.ops.op.is_none(), "and says nothing when it ends");

    let t = i.now;
    i.push(t, Input::CommandStart { id: "2".into(), label: "cargo build".into(), after_secs: 10 });
    run(&mut i, 11_000);
    let op = i.ops.op.clone().expect("past the threshold");
    assert_eq!((op.label.as_str(), op.state), ("cargo build", progress::State::Run));
    run(&mut i, 100_000);
    assert!(matches!(&i.ops.op, Some(o) if o.state == progress::State::Run), "refreshed, not dropped as stale after two minutes");
    i.push(i.now, Input::CommandEnd { id: "2".into(), code: 0 });
    let op = i.ops.op.clone().unwrap();
    assert_eq!(op.state, progress::State::Done);
    assert!(op.label.starts_with("cargo build · 1m 5"), "{}", op.label);

    i.push(i.now, Input::ProgressClear);
    let t = i.now;
    i.push(t, Input::CommandStart { id: "3".into(), label: "pytest".into(), after_secs: 2 });
    run(&mut i, 3000);
    i.push(i.now, Input::CommandEnd { id: "3".into(), code: 2 });
    let op = i.ops.op.clone().unwrap();
    assert_eq!(op.state, progress::State::Fail);
    assert!(op.label.starts_with("pytest · exit 2 · "), "{}", op.label);
}

#[test]
fn a_second_command_waits_for_the_pill_to_be_free() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::CommandStart { id: "a".into(), label: "one".into(), after_secs: 1 });
    run(&mut i, 2000);
    i.push(i.now, Input::CommandStart { id: "b".into(), label: "two".into(), after_secs: 1 });
    run(&mut i, 3000);
    assert_eq!(i.ops.op.as_ref().unwrap().label, "one", "the first keeps the pill");
    i.push(i.now, Input::CommandEnd { id: "b".into(), code: 0 });
    assert_eq!(i.ops.op.as_ref().unwrap().label, "one", "an unseen command ends silently");
}

#[test]
fn notifications_queue_and_do_not_disturb() {
    let mut i = Island::new(Config::default());
    let n = |id: u32, app: &str, crit: bool| Input::Notification(Incoming { id, app: app.into(), summary: format!("n{id}"), critical: crit, ..Default::default() });
    for (id, app) in [(1, "a"), (2, "b"), (3, "c"), (4, "d")] {
        i.push(T0, n(id, app, false));
    }
    assert_eq!((i.below.len(), i.queue.len()), (2, 1), "three have a card, the fourth waits");
    run(&mut i, 600);
    assert!(i.has_card() && i.current_face() == Face::Idle, "the cards drop under the pill, which stays as it is");
    assert_eq!(i.notif.as_ref().unwrap().id, 1);
    run(&mut i, 4500);
    assert_eq!(i.notif.as_ref().map(|n| n.id), Some(2), "the next one moves up");
    assert!(i.below.iter().any(|b| b.n.id == 4) && i.waiting.is_empty(), "and the one that waited has its place");
    i.data.dnd = true;
    run(&mut i, 10_000);
    i.push(i.now, n(5, "e", false));
    assert!(i.notif.is_none(), "do not disturb hides it");
    i.push(i.now, n(6, "f", true));
    assert_eq!(i.notif.as_ref().map(|n| n.id), Some(6), "but not a critical one");
    assert_eq!(i.history.len(), 6, "everything lands in the history");
}

#[test]
fn the_meeting_heads_up_fires_once_even_across_a_restart() {
    let ev = Event { id: "m".into(), title: "Class".into(), start_ms: T0 + 8 * 60_000, end_ms: T0 + 3_600_000, ..Default::default() };
    let mut i = Island::new(Config::default());
    i.data.events = vec![ev.clone()];
    i.tick(T0, 8.0);
    assert!(i.meeting.is_some() && i.meeting_until > T0);
    i.meeting_until = 0;
    run(&mut i, 3000);
    assert!(i.meeting_until == 0, "not announced twice");
    let mut j = Island::new(Config::default());
    j.restore_announced(i.announced_keys());
    j.data.events = vec![ev];
    j.tick(T0 + 100, 8.0);
    assert!(j.meeting.is_none(), "a restart does not announce it again");
}

#[test]
fn low_battery_and_plugging_raise_one_event_each() {
    let mut i = Island::new(Config::default());
    i.data.battery = Some(Battery { percent: 80.0, charging: false, on_ac: true });
    i.tick(T0, 8.0);
    i.data.battery = Some(Battery { percent: 80.0, charging: false, on_ac: false });
    i.tick(T0 + 8, 8.0);
    assert_eq!(i.power, Some(PowerEvent::Unplugged));
    i.power_until = 0;
    i.data.battery = Some(Battery { percent: 10.0, charging: false, on_ac: false });
    i.tick(T0 + 16, 8.0);
    assert_eq!(i.power, Some(PowerEvent::Low));
    i.power = None;
    i.tick(T0 + 24, 8.0);
    assert_eq!(i.power, None, "low battery is announced once");
}

#[test]
fn clicks_hit_the_buttons_that_were_drawn() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenPage(Page::Home));
    let mut surface = (0.0, 0.0);
    for _ in 0..200 {
        let fr = i.tick(i.now + 8, 8.0);
        i.push(i.now, Input::OpenPage(Page::Home));
        surface = fr.surface;
        let mut scene = Scene::new();
        i.draw(&mut scene, &f, fr.surface, 1.0);
    }
    let hit = i.hits.iter().find(|h| h.action == Action::TimerStart(300)).expect("the 5 min preset is drawn");
    let (x, y) = (hit.rect.center().x + i.pill_origin.0, hit.rect.center().y + i.pill_origin.1);
    assert!(x < surface.0 && y < surface.1);
    i.pointer_down(i.now, x, y);
    assert!(i.timer.is_some(), "clicking the preset starts the timer");
}

fn incoming(id: u32) -> Incoming {
    Incoming { id, app: "chat".into(), summary: "Anna".into(), body: "Are you coming?".into(), ..Default::default() }
}

fn closes(i: &mut Island) -> Vec<(u32, CloseReason)> {
    i.take_commands()
        .into_iter()
        .filter_map(|c| if let Command::NotificationClosed { id, reason } = c { Some((id, reason)) } else { None })
        .collect()
}

#[test]
fn an_expired_notification_tells_its_sender() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Notification(incoming(7)));
    run(&mut i, 6500);
    assert_eq!(closes(&mut i), vec![(7, CloseReason::Expired)]);
    assert!(i.notif.is_none());
}

#[test]
fn a_dismissed_notification_says_dismissed_and_a_sender_close_says_nothing() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Notification(incoming(1)));
    i.activate(Action::Dismiss);
    assert_eq!(closes(&mut i), vec![(1, CloseReason::Dismissed)]);
    i.push(i.now, Input::Notification(incoming(2)));
    i.push(i.now, Input::NotificationClosed(2));
    assert!(closes(&mut i).is_empty(), "closed by the sender: no signal back");
    assert!(i.notif.is_none());
}

#[test]
fn the_senders_timeout_is_honoured() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Notification(Incoming { expire_ms: Some(12_000), ..incoming(1) }));
    run(&mut i, 9000);
    assert!(i.notif.is_some(), "stays for the 12 s it asked for");
    run(&mut i, 4000);
    assert!(i.notif.is_none());
    i.push(i.now, Input::Notification(Incoming { expire_ms: Some(0), ..incoming(2) }));
    run(&mut i, 20_000);
    assert!(i.notif.is_some(), "a persistent one waits for the user (up to 30 s on screen)");
    run(&mut i, 12_000);
    assert!(i.notif.is_none());
}

#[test]
fn an_action_invokes_it_and_closes_unless_resident() {
    let mut i = Island::new(Config::default());
    let act = vec![NotifAction { key: "open".into(), label: "Open".into() }];
    i.push(T0, Input::Notification(Incoming { actions: act.clone(), ..incoming(1) }));
    i.activate(Action::NotifAction("open".into()));
    let cmds = i.take_commands();
    assert_eq!(cmds[0], Command::InvokeAction { id: 1, key: "open".into() });
    assert_eq!(cmds[1], Command::NotificationClosed { id: 1, reason: CloseReason::Dismissed });
    i.push(i.now, Input::Notification(Incoming { actions: act, resident: true, ..incoming(2) }));
    i.activate(Action::NotifAction("open".into()));
    let cmds = i.take_commands();
    assert_eq!(cmds.len(), 1, "a resident notification is not closed by its action");
}

#[test]
fn inline_reply_types_edits_and_sends() {
    let mut i = Island::new(Config::default());
    let reply = vec![NotifAction { key: "inline-reply".into(), label: "Write back".into() }];
    i.push(T0, Input::Notification(Incoming { actions: reply, ..incoming(5) }));
    i.activate(Action::NotifAction("inline-reply".into()));
    assert!(i.wants_keyboard());
    assert_eq!(i.reply.as_ref().unwrap().placeholder, "Write back");
    let key = |i: &mut Island, k: KeyInput| i.push(i.now, Input::Key(k));
    key(&mut i, KeyInput::Text("Hello wörld".into()));
    for _ in 0..5 {
        key(&mut i, KeyInput::Left);
    }
    key(&mut i, KeyInput::Backspace); // removes the space
    key(&mut i, KeyInput::Text(", ".into()));
    key(&mut i, KeyInput::End);
    key(&mut i, KeyInput::Text("!\n\u{7}".into())); // control characters are dropped
    assert_eq!(i.reply.as_ref().unwrap().text, "Hello, wörld!");
    key(&mut i, KeyInput::Home);
    key(&mut i, KeyInput::Delete);
    assert_eq!(i.reply.as_ref().unwrap().text, "ello, wörld!");
    key(&mut i, KeyInput::Enter);
    assert!(!i.wants_keyboard());
    let cmds = i.take_commands();
    assert_eq!(cmds[0], Command::NotificationReplied { id: 5, text: "ello, wörld!".into() });
    assert_eq!(cmds[1], Command::NotificationClosed { id: 5, reason: CloseReason::Dismissed });
    assert!(i.notif.is_none());
}

#[test]
fn escape_and_idle_cancel_a_reply_without_sending() {
    let mut i = Island::new(Config::default());
    let reply = vec![NotifAction { key: "inline-reply".into(), label: String::new() }];
    i.push(T0, Input::Notification(Incoming { actions: reply.clone(), ..incoming(1) }));
    i.activate(Action::NotifAction("inline-reply".into()));
    i.push(i.now, Input::Key(KeyInput::Text("x".into())));
    i.push(i.now, Input::Key(KeyInput::Escape));
    assert!(i.take_commands().iter().all(|c| !matches!(c, Command::NotificationReplied { .. })));
    i.push(i.now, Input::Notification(Incoming { actions: reply, ..incoming(2) }));
    i.activate(Action::NotifAction("inline-reply".into()));
    assert_eq!(i.reply.as_ref().unwrap().placeholder, "Reply", "a default placeholder");
    run(&mut i, 21_000);
    assert!(i.reply.is_none(), "an abandoned reply times out");
    assert!(i.take_commands().iter().all(|c| !matches!(c, Command::NotificationReplied { .. })));
}

#[test]
fn every_new_face_draws() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    let reply = vec![NotifAction { key: "inline-reply".into(), label: "Reply".into() }];
    i.push(T0, Input::Notification(Incoming { actions: reply, progress: Some(40), ..incoming(1) }));
    i.activate(Action::NotifAction("inline-reply".into()));
    i.push(i.now, Input::Key(KeyInput::Text("hi".into())));
    i.data = data();
    i.data.media.as_mut().unwrap().players = 2;
    for _ in 0..200 {
        let fr = i.tick(i.now + 8, 8.0);
        let mut scene = Scene::new();
        i.draw(&mut scene, &f, fr.surface, 2.0);
    }
    assert_eq!(i.current_face(), Face::Reply);
}

fn type_line(i: &mut Island, text: &str) {
    i.push(i.now, Input::Key(KeyInput::Text(text.into())));
}

#[test]
fn the_command_bar_starts_a_timer() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenBar);
    run(&mut i, 600);
    assert_eq!(i.current_face(), Face::Bar);
    assert!(i.wants_keyboard());
    type_line(&mut i, "10m tea");
    assert!(matches!(i.bar_cmd(), Some(island_core::bar::Cmd::Timer { secs: 600, .. })));
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.bar.is_none() && i.timer.is_some(), "the bar closes and the timer runs");
    assert!(!i.wants_keyboard());
}

#[test]
fn the_command_bar_stays_open_on_what_it_does_not_understand_and_escape_closes_it() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenBar);
    type_line(&mut i, "blah blah");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.bar.is_some(), "nothing happened, the preview says why");
    assert!(i.timer.is_none() && i.take_commands().is_empty());
    i.push(i.now, Input::Key(KeyInput::Escape));
    assert!(i.bar.is_none());
    i.push(i.now, Input::OpenBar);
    run(&mut i, 31_000);
    assert!(i.bar.is_none(), "an idle bar closes itself");
}

#[test]
fn a_sum_is_copied_to_the_clipboard() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenBar);
    type_line(&mut i, "5 km in mi");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.take_commands().contains(&Command::RestoreClipboard("3.106855961 mi".into())), "the result with its unit, not the whole line");
    i.push(T0, Input::OpenBar);
    type_line(&mut i, "12*7");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.take_commands().contains(&Command::RestoreClipboard("84".into())));
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
fn the_do_not_disturb_and_focus_words_reach_the_platform() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenBar);
    type_line(&mut i, "dnd on");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.take_commands().contains(&Command::SetDnd(true)));
    i.push(T0, Input::OpenBar);
    type_line(&mut i, "focus 45");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.take_commands().contains(&Command::StartFocus(45)));
}

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
fn snoozing_a_notification_brings_it_back_in_ten_minutes() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Notification(Incoming { id: 7, app: "Mail".into(), summary: "Invoice".into(), ..Default::default() }));
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::Snooze), "the bell is drawn on the card");
    assert!(i.notif.is_none());
    assert!(i.take_commands().iter().any(|c| matches!(c, Command::NotificationClosed { id: 7, .. })));
    run(&mut i, 9 * 60_000);
    assert!(i.notif.is_none(), "not yet");
    run(&mut i, 61_000);
    assert_eq!(i.notif.as_ref().map(|n| n.summary.as_str()), Some("Mail: Invoice"), "and then it is back");
    draw_frames(&mut i, 200);
    assert!(!press(&mut i, Action::Snooze), "a reminder's own card has no snooze");
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
fn a_script_activity_shows_with_its_own_icon_colour_and_words() {
    let mut i = Island::new(Config::default());
    let a = island_core::progress::Activity { id: String::new(), label: "CI main".into(), detail: "building".into(), icon: Some("git-branch".into()), tint: Some((255, 159, 10)), frac: None };
    i.push(T0, Input::Activity(progress::State::Run, a.clone()));
    run(&mut i, 600);
    assert_eq!(i.current_face(), Face::Progress);
    run(&mut i, 4000);
    assert_eq!(i.shown_side, Some(Side::Op), "then a bubble");
    i.push(i.now, Input::Activity(progress::State::Fail, island_core::progress::Activity { id: String::new(), label: "CI main".into(), detail: "2 tests failed".into(), ..Default::default() }));
    run(&mut i, 600);
    let o = i.ops.op.clone().unwrap();
    assert_eq!((o.state, o.detail.as_str(), o.icon.as_deref()), (progress::State::Fail, "2 tests failed", Some("git-branch")));
    // it draws without trouble in every state
    let f = fonts();
    let mut scene = Scene::new();
    let fr = i.tick(i.now + 8, 8.0);
    i.draw(&mut scene, &f, fr.surface, 1.0);
}

fn headphones(battery: Option<u8>) -> Device {
    Device { id: "88:C9".into(), name: "Sony XM5".into(), kind: island_core::bluetooth::Kind::Headphones, battery }
}

#[test]
fn a_connecting_device_gets_a_card_and_a_low_battery_warns_once_until_it_recovers() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::DeviceConnected(headphones(Some(80))));
    run(&mut i, 600);
    assert_eq!(i.current_face(), Face::Device);
    run(&mut i, 4000);
    assert_ne!(i.current_face(), Face::Device, "it goes away by itself");
    i.data.devices = vec![headphones(Some(12))];
    i.push(i.now, Input::DeviceLow(headphones(Some(12))));
    run(&mut i, 600);
    assert_eq!(i.current_face(), Face::Device);
    assert!(i.device_ev.as_ref().is_some_and(|(_, low)| *low));
    run(&mut i, 7000);
    i.push(i.now, Input::DeviceLow(headphones(Some(11))));
    run(&mut i, 600);
    assert_ne!(i.current_face(), Face::Device, "told once");
    // charged again, then low again: a new warning
    i.data.devices = vec![headphones(Some(90))];
    run(&mut i, 1000);
    i.push(i.now, Input::DeviceLow(headphones(Some(10))));
    run(&mut i, 600);
    assert_eq!(i.current_face(), Face::Device);
}

#[test]
fn the_system_page_lists_connected_devices_and_grows_for_them() {
    let mut i = Island::new(Config::default());
    i.data.system = Some(Default::default());
    let before = i.sys_rows();
    i.data.devices = vec![headphones(Some(50)), Device { id: "m".into(), name: "MX".into(), kind: island_core::bluetooth::Kind::Mouse, battery: None }];
    assert_eq!(i.sys_rows(), before + 2);
    i.push(T0, Input::OpenPage(Page::Sys));
    draw_frames(&mut i, 300);
}

#[test]
fn the_quick_settings_page_lists_the_devices_and_their_battery() {
    let mut i = Island::new(Config::default());
    let h0 = i.qs_layout().height;
    i.data.devices = vec![headphones(Some(85))];
    assert_eq!(i.qs_layout().height, h0 + 10.0 + 32.0, "one row");
    i.data.devices = vec![headphones(Some(85)), headphones(Some(12)), headphones(None), headphones(Some(5))];
    assert_eq!(i.qs_layout().height, h0 + 10.0 + 3.0 * 32.0 + 2.0 * 6.0, "three at most");
    i.push(T0, Input::OpenPage(Page::Qs));
    draw_frames(&mut i, 300);
    i.data.devices.clear();
    assert_eq!(i.qs_layout().height, h0, "and nothing when none is connected");
}

#[test]
fn the_home_page_is_left_as_it_was() {
    let mut i = Island::new(Config::default());
    let h0 = i.face_size(Face::IdleHover).1;
    i.data.devices = vec![headphones(Some(85))];
    assert_eq!(i.face_size(Face::IdleHover).1, h0);
}

#[test]
fn device_icons_exist_in_the_bundled_font() {
    use island_core::bluetooth::Kind::*;
    for k in [Earbuds, Headphones, Headset, Keyboard, Mouse, Gamepad, Other] {
        assert!(crate::is_known_icon(k.icon()), "{}", k.icon());
    }
}

fn forecast(now: i64, rain_at_h: Option<i64>) -> Weather {
    let hours = (0..48)
        .map(|k| {
            let t = (now / 3_600_000 - 12 + k) * 3_600_000;
            let wet = rain_at_h.is_some_and(|h| t >= now + h * 3_600_000 && t < now + (h + 2) * 3_600_000);
            island_core::weather::Hour { time_ms: t, temp_c: 12.0, code: if wet { 61 } else { 2 }, is_day: true, rain_prob: if wet { 80 } else { 0 }, mm: if wet { 1.0 } else { 0.0 } }
        })
        .collect();
    Weather { temp_c: 12.0, feels_c: 10.0, code: 2, day: true, high_c: 15.0, low_c: 7.0, rain_slots: (0..8).map(|k| island_core::weather::Slot { offset_min: k * 15, mm: 0.0 }).collect(), hours }
}

#[test]
fn the_weather_page_exists_once_there_is_a_forecast_and_the_home_weather_opens_it() {
    let mut i = Island::new(Config::default());
    assert!(!i.pages().contains(&Page::Weather), "no forecast, no page");
    i.data.weather = Some(forecast(T0, None));
    let pages = i.pages();
    let at = pages.iter().position(|p| *p == Page::Weather).unwrap();
    assert_eq!(pages[at - 1], Page::Cal, "after the calendar");
    i.push(T0, Input::OpenPage(Page::Weather));
    draw_frames(&mut i, 300);
    assert_eq!(i.current_face(), Face::Weather);
    i.push(i.now, Input::OpenPage(Page::Home));
    draw_frames(&mut i, 300);
    assert!(press(&mut i, Action::Open(Page::Weather)), "the weather block on the home page is a button");
    draw_frames(&mut i, 300);
    assert_eq!(i.current_face(), Face::Weather);
}

#[test]
fn rain_is_told_in_a_way_that_fits_the_distance() {
    use island_core::weather::Outlook::*;
    let mut i = Island::new(Config::default());
    i.data.utc_offset_secs = 0;
    i.now = 14 * 3_600_000; // 14:00
    assert_eq!(i.rain_text(Rain { in_min: 25 }, false).as_deref(), Some("Rain in 25 min"));
    assert_eq!(i.rain_text(Rain { in_min: 180 }, false).as_deref(), Some("Rain in 3 h"));
    assert_eq!(i.rain_text(Rain { in_min: 180 }, true).as_deref(), Some("Rain around 17:00 · in 3 h"));
    assert_eq!(i.rain_text(Rain { in_min: 25 }, true).as_deref(), Some("Rain in 25 min"), "near rain needs no clock time");
    assert_eq!(i.rain_text(Raining { clears_in_min: Some(40) }, true).as_deref(), Some("Raining · clears in 40 min"));
    assert_eq!(i.rain_text(Raining { clears_in_min: None }, false).as_deref(), Some("Raining now"));
    assert_eq!(i.rain_text(Dry, true), None);
    assert_eq!(Island::span_text(1), "1 min");
    assert_eq!(Island::span_text(95), "1 h 40 min", "rounded to ten minutes");
}

#[test]
fn the_outlook_follows_the_forecast_and_home_only_cares_when_it_is_near() {
    use island_core::weather::{worth_showing_at_home, Outlook};
    let mut i = Island::new(Config::default());
    i.now = T0;
    assert_eq!(i.rain_outlook(), None, "no forecast");
    i.data.weather = Some(forecast(T0, None));
    assert_eq!(i.rain_outlook(), Some(Outlook::Dry));
    i.data.weather = Some(forecast(T0, Some(3)));
    let o = i.rain_outlook().unwrap();
    assert!(matches!(o, Outlook::Rain { in_min } if (170..=190).contains(&in_min)), "{o:?}");
    assert!(worth_showing_at_home(o));
    i.data.weather = Some(forecast(T0, Some(9)));
    assert!(!worth_showing_at_home(i.rain_outlook().unwrap()), "nine hours away is not a home page matter");
}

#[test]
fn a_level_that_arrives_in_steps_slides_instead_of_jumping() {
    let mut i = Island::new(Config::default());
    i.data.volume = Some(0.30);
    run(&mut i, 100);
    assert!((i.volume_shown - 0.30).abs() < 1e-6, "the first reading is not slid to");
    i.data.volume = Some(0.60);
    let fr = i.tick(i.now + 8, 8.0);
    assert!(i.volume_shown > 0.30 && i.volume_shown < 0.40, "one frame moves a little: {}", i.volume_shown);
    assert!(fr.animating, "and frames keep coming at the display rate until it arrives");
    let mut shown = vec![i.volume_shown];
    for _ in 0..80 {
        i.tick(i.now + 8, 8.0);
        shown.push(i.volume_shown);
    }
    assert!(shown.windows(2).all(|w| w[1] >= w[0]), "monotonic, no overshoot");
    assert!((*shown.last().unwrap() - 0.60).abs() < 1e-6, "it lands on the level");
}

#[test]
fn a_slider_under_the_pointer_follows_it_exactly() {
    let mut i = Island::new(Config::default());
    i.data.volume = Some(0.30);
    run(&mut i, 100);
    i.dragging = Some(Action::VolumeBar);
    i.data.volume = Some(0.90);
    i.tick(i.now + 8, 8.0);
    assert!((i.volume_shown - 0.90).abs() < 1e-6, "no lag behind the finger");
    i.dragging = None;
    i.data.brightness = Some(0.5);
    i.tick(i.now + 8, 8.0);
    assert!(i.brightness_shown <= 0.5);
}

#[test]
fn the_volume_bar_starts_from_the_level_it_had() {
    let mut i = Island::new(Config::default());
    i.data.volume = Some(0.50);
    run(&mut i, 100);
    i.push(i.now, Input::Osd { kind: OsdKind::Volume, level: 0.56, muted: false });
    assert!((i.osd_shown - 0.50).abs() < 1e-6, "it starts where the bar last was");
    draw_frames(&mut i, 60);
    assert!((i.osd_shown - 0.56).abs() < 0.001, "and slides to the new one: {}", i.osd_shown);
    // muting slides to empty
    i.push(i.now, Input::Osd { kind: OsdKind::Volume, level: 0.56, muted: true });
    draw_frames(&mut i, 80);
    assert!(i.osd_shown < 0.01);
}

#[test]
fn the_options_page_flips_switches_and_steps_numbers() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenPage(Page::Options));
    draw_frames(&mut i, 300);
    assert_eq!(i.current_face(), Face::Options);
    // a switch on the first tab
    assert!(press(&mut i, Action::Toggle("clock_in_pill")));
    assert!(!i.cfg.clock_in_pill, "the island follows at once");
    assert_eq!(i.take_commands(), vec![Command::SetOption { key: "clock_in_pill".into(), value: OptionValue::Bool(false) }]);
    // another tab
    assert!(press(&mut i, Action::OptionsTab(1)));
    draw_frames(&mut i, 300);
    assert!(press(&mut i, Action::Step("notification_seconds", 1)));
    assert_eq!(i.cfg.notification_ms, 6000);
    assert_eq!(i.take_commands(), vec![Command::SetOption { key: "notification_seconds".into(), value: OptionValue::Num(6.0) }]);
    assert!(press(&mut i, Action::Step("osd_seconds", -1)));
    assert_eq!(i.cfg.osd_ms, 1500);
    // the ends of the range hold
    i.cfg.notification_ms = 30_000;
    i.activate(Action::Step("notification_seconds", 1));
    assert_eq!(i.cfg.notification_ms, 30_000);
    // the page changes height with the tab
    assert_ne!(i.face_size(Face::Options).1, { i.activate(Action::OptionsTab(2)); i.face_size(Face::Options).1 });
}

#[test]
fn options_that_switch_a_behaviour_off_do_so() {
    let n = |id: u32| Input::Notification(Incoming { id, app: "a".into(), summary: "s".into(), ..Default::default() });
    // no cards, but the history keeps them
    let mut i = Island::new(Config { show_notifications: false, ..Config::default() });
    i.push(T0, n(1));
    assert!(i.notif.is_none() && i.history.len() == 1);
    // no volume bar
    let mut j = Island::new(Config { show_osd: false, ..Config::default() });
    j.push(T0, Input::Osd { kind: OsdKind::Volume, level: 0.5, muted: false });
    run(&mut j, 600);
    assert_ne!(j.current_face(), Face::Osd);
    let mut k = Island::new(Config::default());
    k.push(T0, Input::Osd { kind: OsdKind::Volume, level: 0.5, muted: false });
    run(&mut k, 600);
    assert_eq!(k.current_face(), Face::Osd, "and with it on, it shows");
}

#[test]
fn scrolling_over_the_small_pill_changes_the_volume_and_the_middle_button_mutes() {
    let mut i = Island::new(Config::default());
    i.data.volume = Some(0.50);
    run(&mut i, 600);
    assert!(i.current_face().is_compact());
    let cmds = i.scroll(i.now, 0.0, 2.0);
    assert!(matches!(cmds.as_slice(), [Command::SetVolume(v)] if (*v - 0.54).abs() < 1e-6), "{cmds:?}");
    let cmds = i.scroll(i.now, 0.0, -50.0);
    assert!(matches!(cmds.as_slice(), [Command::SetVolume(v)] if *v == 0.0), "down to zero, no further");
    assert_eq!(i.middle_click(i.now), vec![Command::ToggleMute]);
    // not while a page is open: there the middle button does nothing
    i.push(i.now, Input::OpenPage(Page::Cal));
    run(&mut i, 600);
    assert!(i.middle_click(i.now).is_empty());
}

#[test]
fn a_sideways_swipe_over_the_pill_skips_a_track_once() {
    let mut i = Island::new(Config::default());
    i.data = data();
    run(&mut i, 600);
    assert_eq!(i.current_face(), Face::MediaMini);
    assert!(i.scroll(i.now, 1.0, 0.0).is_empty(), "a small movement is added up, not acted on");
    assert!(i.scroll(i.now, 1.0, 0.0).is_empty());
    assert_eq!(i.scroll(i.now, 1.5, 0.0), vec![Command::MediaNext], "past the threshold: next");
    assert!(i.scroll(i.now + 100, 5.0, 0.0).is_empty(), "the rest of the same swipe does nothing");
    i.now += 800;
    assert_eq!(i.scroll(i.now, -4.0, 0.0), vec![Command::MediaPrevious], "the other way: previous");
    // without music a sideways swipe does nothing
    let mut j = Island::new(Config::default());
    run(&mut j, 600);
    assert!(j.scroll(j.now, 9.0, 0.0).is_empty());
}

#[test]
fn an_app_can_be_hidden_from_the_history_and_rules_are_listed_and_removed() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Notification(Incoming { id: 1, app: "Zen Browser".into(), summary: "unseen".into(), ..Default::default() }));
    i.activate(Action::Dismiss); // the card first, then the page behind it
    i.take_commands();
    i.push(i.now, Input::OpenPage(Page::Notifs));
    draw_frames(&mut i, 300);
    assert!(press(&mut i, Action::MuteApp(0)), "a bell on the row");
    assert_eq!(i.take_commands(), vec![Command::AddFilter { app: "Zen Browser".into(), content: String::new(), hide: true }]);
    // the confirmation card away, then the rules tab
    i.activate(Action::Dismiss);
    i.take_commands();
    i.data.config.filters = vec![FilterView { app: "Zen Browser".into(), content: String::new(), show: false, sound: false }, FilterView { app: "Telegram".into(), content: String::new(), show: true, sound: false }];
    i.push(i.now, Input::OpenPage(Page::Options));
    i.activate(Action::OptionsTab(crate::options::RULES_TAB));
    draw_frames(&mut i, 300);
    assert!(press(&mut i, Action::RemoveRule(1)));
    assert_eq!(i.take_commands(), vec![Command::RemoveFilter(1)]);
    // typing an app name into the add row
    i.activate(Action::EditField(Field::RuleSilence));
    i.push(i.now, Input::Key(KeyInput::Text("Mail".into())));
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert_eq!(i.take_commands(), vec![Command::AddFilter { app: "Mail".into(), content: String::new(), hide: false }]);
    // an empty name adds nothing
    i.activate(Action::EditField(Field::RuleHide));
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.take_commands().is_empty());
    assert_eq!(i.face_size(Face::Options).1, 14.0 + 34.0 + 40.0 + 4.0 * 34.0 + 14.0 + DOCK_H, "two rules and two add rows, and the dock");
}

fn media_page(playing: bool) -> Island {
    let mut i = Island::new(Config::default());
    i.data = data();
    i.data.media.as_mut().unwrap().playing = playing;
    i.push(T0, Input::OpenPage(Page::Media));
    draw_frames(&mut i, 300);
    assert!(matches!(i.current_face(), Face::MediaFull | Face::MediaLyric));
    i
}

#[test]
fn the_play_pause_button_answers_at_once_and_the_icons_cross_fade() {
    let mut i = media_page(false);
    assert_eq!((i.play_anim, i.playing_shown()), (0.0, false));
    assert!(press(&mut i, Action::Cmd(Command::MediaPlayPause)));
    assert_eq!(i.take_commands(), vec![Command::MediaPlayPause], "the player is asked once");
    assert!(i.playing_shown(), "it shows playing before the player has said so");
    assert!(i.disc_press > 0.9, "and the disc gives under the press");
    i.tick(i.now + 8, 8.0);
    assert!(i.play_anim > 0.0 && i.play_anim < 0.5, "one frame in: part of the way ({})", i.play_anim);
    draw_frames(&mut i, 40);
    assert!(i.play_anim > 0.99, "and then the pause icon is fully there");
    assert!(i.disc_press < 0.05, "the press has died away");
    // the player agrees: nothing changes, the override is dropped
    i.data.media.as_mut().unwrap().playing = true;
    draw_frames(&mut i, 5);
    assert!(i.play_override.is_none() && i.playing_shown());
}

#[test]
fn a_press_the_player_never_confirms_is_taken_back() {
    let mut i = media_page(true);
    assert!(press(&mut i, Action::Cmd(Command::MediaPlayPause)));
    assert!(!i.playing_shown());
    draw_frames(&mut i, 40);
    assert!(i.play_anim < 0.1, "shown as paused: {}", i.play_anim);
    i.now += 2000; // the player still says playing
    draw_frames(&mut i, 40);
    assert!(i.playing_shown(), "back to what the player says");
    assert!(i.play_anim > 0.9);
}

#[test]
fn a_skip_button_nudges_its_icon_and_comes_back() {
    let mut i = media_page(true);
    assert!(press(&mut i, Action::Cmd(Command::MediaNext)));
    assert_eq!(i.take_commands(), vec![Command::MediaNext]);
    assert!(i.skip_press > 0.9, "{}", i.skip_press);
    draw_frames(&mut i, 120);
    assert!(i.skip_press.abs() < 0.01, "back in place: {}", i.skip_press);
    assert!(press(&mut i, Action::Cmd(Command::MediaPrevious)));
    assert!(i.skip_press < -0.9, "the other way for previous: {}", i.skip_press);
}

#[test]
fn the_disc_swells_under_the_pointer() {
    let mut i = media_page(true);
    let disc = i.hits.iter().find(|h| h.action == Action::Cmd(Command::MediaPlayPause)).unwrap().rect;
    assert_eq!(i.disc_hover, 0.0);
    i.pointer_moved(i.now, disc.center().x + i.pill_origin.0, disc.center().y + i.pill_origin.1);
    draw_frames(&mut i, 30);
    assert!(i.disc_hover > 0.95, "{}", i.disc_hover);
    i.pointer_left(i.now);
    draw_frames(&mut i, 30);
    assert!(i.disc_hover < 0.05);
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
fn quick_settings_toggles_answer_at_once() {
    let mut i = Island::new(Config::default());
    i.data.wifi = Some(true);
    i.data.bluetooth = Some(false);
    assert_eq!(i.activate(Action::Cmd(Command::ToggleWifi)), vec![Command::ToggleWifi]);
    assert_eq!(i.data.wifi, Some(false));
    i.activate(Action::Cmd(Command::ToggleBluetooth));
    assert_eq!(i.data.bluetooth, Some(true));
    i.activate(Action::Cmd(Command::ToggleKeepAwake));
    assert!(i.data.keep_awake);
    i.data.wifi = None;
    i.activate(Action::Cmd(Command::ToggleWifi));
    assert_eq!(i.data.wifi, None, "unknown stays unknown until the next sample");
    i.data.night_light = Some(false);
    i.activate(Action::Cmd(Command::ToggleNightLight));
    assert_eq!(i.data.night_light, Some(true));
    i.data.night_light = None;
    i.activate(Action::Cmd(Command::ToggleNightLight));
    assert_eq!(i.data.night_light, None, "no wlsunset: the chip stays hidden");
}

#[test]
fn clipboard_rows_copy_back_and_show_a_check() {
    let mut i = Island::new(Config::default());
    i.data.clipboard = vec!["one".into(), "two".into()];
    assert_eq!(i.activate(Action::ClipRestore(1)), vec![Command::RestoreClipboard("two".into())]);
    assert!(i.copied.is_some_and(|(j, until)| j == 1 && until > i.now));
    assert!(i.activate(Action::ClipRestore(9)).is_empty());
}

#[test]
fn dnd_from_a_script_and_spectrum_levels() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::SetDnd(true));
    assert!(i.data.dnd);
    assert_eq!(i.take_commands(), vec![Command::SetDnd(true)]);
    assert_eq!(i.dnd_changed(), Some(true));
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
fn the_new_pages_draw() {
    let f = fonts();
    for page in [Page::Clip, Page::Focus, Page::Qs, Page::Sys] {
        let mut i = Island::new(Config::default());
        i.data = data();
        i.data.clipboard = vec!["a".into(); 7];
        i.data.focus = Some(Focus { active: true, end_epoch: Some(T0 / 1000 + 600) });
        i.data.system = Some(SystemStats { cores: vec![10.0; 12], ..Default::default() });
        for _ in 0..200 {
            let t = i.now + 8;
            let fr = i.tick(t, 8.0);
            i.push(t, Input::OpenPage(page));
            let mut scene = Scene::new();
            i.draw(&mut scene, &f, fr.surface, 1.0);
            assert!(fr.surface.0 < 1500.0 && fr.surface.1 < 1200.0 && fr.surface.1.is_finite(), "{page:?}");
        }
    }
}

#[test]
fn the_keyboard_reaches_the_controls() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenKeyboard(Some(Page::Home)));
    assert!(i.wants_keyboard());
    for _ in 0..200 {
        let fr = i.tick(i.now + 8, 8.0);
        let mut scene = Scene::new();
        i.draw(&mut scene, &f, fr.surface, 1.0);
    }
    let nav = i.nav_hits();
    assert!(nav.len() >= 5, "the preset chips and the calendar button: {}", nav.len());
    let key = |i: &mut Island, k: KeyInput| i.push(i.now, Input::Key(k));
    key(&mut i, KeyInput::Right);
    assert_eq!(i.kb.unwrap().index, 1);
    key(&mut i, KeyInput::Left);
    key(&mut i, KeyInput::Left);
    assert_eq!(i.kb.unwrap().index, nav.len() - 1, "wraps around");
    let target = nav.iter().position(|ix| i.hits[*ix].action == Action::TimerStart(300)).expect("the 5 min chip is drawn");
    for _ in 0..(target + 1) % nav.len() {
        key(&mut i, KeyInput::Right);
    }
    key(&mut i, KeyInput::Enter);
    assert!(i.timer.is_some(), "Enter on the 5 min chip starts the timer");
    key(&mut i, KeyInput::Escape);
    assert!(i.kb.is_none() && !i.wants_keyboard());
}

#[test]
fn the_keyboard_lets_go_when_idle() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenKeyboard(None));
    run(&mut i, 10_000);
    assert!(i.kb.is_none(), "an abandoned keyboard session ends");
}

fn type_text(i: &mut Island, t: &str) {
    i.push(i.now, Input::Key(KeyInput::Text(t.into())));
}

#[test]
fn the_settings_menu_adds_a_calendar_and_checks_what_you_type() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenPage(Page::Settings));
    i.activate(Action::EditField(Field::Calendar));
    assert!(i.wants_keyboard());
    type_text(&mut i, "not a calendar");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.take_commands().is_empty(), "rejected");
    assert!(i.note.is_some() && i.edit.is_some(), "the field stays open with a hint");
    i.push(i.now, Input::Key(KeyInput::Home));
    for _ in 0..14 {
        i.push(i.now, Input::Key(KeyInput::Delete));
    }
    type_text(&mut i, "webcal://p01-caldav.icloud.com/published/2/abc");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert_eq!(i.take_commands(), vec![Command::AddCalendar("webcal://p01-caldav.icloud.com/published/2/abc".into())]);
    assert!(i.edit.is_none() && !i.wants_keyboard());
}

#[test]
fn coordinates_font_and_sound_are_validated_and_escape_cancels() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenPage(Page::Settings));
    i.data.config.latitude = Some(43.7);
    i.activate(Action::EditField(Field::Latitude));
    assert_eq!(i.edit.as_ref().unwrap().text, "43.7", "starts from the current value");
    i.push(i.now, Input::Key(KeyInput::Home));
    i.push(i.now, Input::Key(KeyInput::Delete));
    type_text(&mut i, "9");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.take_commands().is_empty(), "93.7 is not a latitude");
    i.push(i.now, Input::Key(KeyInput::Escape));
    assert!(i.edit.is_none());
    i.activate(Action::EditField(Field::Longitude));
    type_text(&mut i, "10,4017");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert_eq!(i.take_commands(), vec![Command::SetLongitude(10.4017)], "a decimal comma is accepted");
    i.activate(Action::EditField(Field::Font));
    type_text(&mut i, "Inter");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert_eq!(i.take_commands(), vec![Command::SetFont("Inter".into())]);
    i.activate(Action::EditField(Field::Sound));
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert_eq!(i.take_commands(), vec![Command::SetNotificationSound(String::new())], "empty clears it");
    assert_eq!(i.activate(Action::RemoveCalendar(2)), vec![Command::RemoveCalendar(2)]);
}

#[test]
fn leaving_the_settings_page_drops_an_open_edit_and_the_page_draws_with_an_edit() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    i.data.config.calendars = vec![CalendarView { name: "A".into(), detail: "a.org".into(), color: (10, 132, 255) }; 6];
    i.push(T0, Input::OpenPage(Page::Settings));
    i.activate(Action::EditField(Field::Font));
    type_text(&mut i, &"long ".repeat(30));
    for _ in 0..200 {
        let fr = i.tick(i.now + 8, 8.0);
        i.push(i.now, Input::OpenPage(Page::Settings));
        let mut scene = Scene::new();
        i.draw(&mut scene, &f, fr.surface, 1.0);
        assert!(fr.surface.1 < 1000.0);
    }
    assert_eq!(i.current_face(), Face::Settings);
    i.push(i.now, Input::Key(KeyInput::Escape));
    i.activate(Action::EditField(Field::Font));
    i.activate(Action::Back);
    run(&mut i, 600);
    assert!(i.edit.is_none(), "the edit does not outlive its page");
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

fn draw_frames(i: &mut Island, n: usize) -> (f64, f64) {
    let f = fonts();
    let mut surface = (0.0, 0.0);
    for _ in 0..n {
        let fr = i.tick(i.now + 8, 8.0);
        let mut scene = Scene::new();
        i.draw(&mut scene, &f, fr.surface, 1.0);
        surface = fr.surface;
    }
    surface
}

#[test]
fn pages_carry_a_dock_that_jumps_straight_to_a_page() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenPage(Page::Qs));
    draw_frames(&mut i, 200);
    let gotos: Vec<Page> = i.hits.iter().filter_map(|h| if let Action::Goto(p) = h.action { Some(p) } else { None }).collect();
    assert_eq!(gotos, vec![Page::Home, Page::Today, Page::Map, Page::Cal, Page::Qs, Page::Sys, Page::Activity, Page::Grid], "Home and six more, in the order of the pages, then the grid");
    let home = i.hits.iter().find(|h| h.action == Action::Goto(Page::Sys)).unwrap().rect;
    i.pointer_down(i.now, home.center().x + i.pill_origin.0, home.center().y + i.pill_origin.1);
    assert_eq!(i.page, Some(Page::Sys));
    assert!(i.page_stack.is_empty(), "the dock does not add to the back history");
    draw_frames(&mut i, 200);
    assert_eq!(i.current_face(), Face::Sys);
    // a card without a page has no dock
    let mut j = Island::new(Config::default());
    j.push(T0, Input::StartTimer(60));
    j.push(j.now, Input::Notification(Incoming { id: 1, ..Default::default() }));
    draw_frames(&mut j, 200);
    assert!(!j.hits.iter().any(|h| matches!(h.action, Action::Goto(_))));
}

#[test]
fn the_dock_lists_live_activities_first_and_grows_the_face() {
    let mut i = Island::new(Config::default());
    i.data = data();
    i.push(T0, Input::StartTimer(600));
    assert_eq!(i.pages()[..2], [Page::Media, Page::Timer]);
    assert_eq!(i.face_size(Face::Qs).1, i.qs_layout().height + DOCK_H, "room for the dock");
    assert_eq!(i.face_size(Face::Idle).1, 28.0, "the compact pill has none");
}

#[test]
fn back_returns_to_the_page_that_led_here() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenPage(Page::Home));
    i.activate(Action::Open(Page::Cal));
    i.activate(Action::Open(Page::Settings));
    assert_eq!(i.page, Some(Page::Settings));
    i.activate(Action::Back);
    assert_eq!(i.page, Some(Page::Cal));
    i.activate(Action::Back);
    assert_eq!(i.page, Some(Page::Home));
    i.activate(Action::Back);
    assert_eq!(i.page, None, "nothing left: the island goes back to what it shows by default");
}

#[test]
fn next_and_previous_page_wrap_and_the_content_slides_the_right_way() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenPage(Page::Home));
    draw_frames(&mut i, 150);
    i.push(i.now, Input::CyclePage(1));
    assert_eq!(i.page, Some(Page::Today));
    i.tick(i.now + 8, 8.0);
    assert_eq!(i.slide, 1.0, "forward slides in from the right");
    draw_frames(&mut i, 150);
    i.push(i.now, Input::CyclePage(-1));
    i.push(i.now, Input::CyclePage(-1));
    assert_eq!(i.page, Some(Page::Settings), "wraps around to the last page");
    i.tick(i.now + 8, 8.0);
    assert_eq!(i.slide, 1.0, "Home -> Settings is still forward in the dock's order");
    draw_frames(&mut i, 150);
    i.push(i.now, Input::CyclePage(-1));
    i.tick(i.now + 8, 8.0);
    assert_eq!(i.slide, -1.0);
}

#[test]
fn tab_changes_page_in_keyboard_mode() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenKeyboard(Some(Page::Home)));
    draw_frames(&mut i, 150);
    i.push(i.now, Input::Key(KeyInput::NextPage));
    assert_eq!(i.page, Some(Page::Today));
    i.push(i.now, Input::Key(KeyInput::PrevPage));
    i.push(i.now, Input::Key(KeyInput::PrevPage));
    assert_eq!(i.page, Some(Page::Settings));
    assert_eq!(i.kb.unwrap().index, 0, "the focus ring starts again on the new page");
}

/// Rests the pointer on the pill (`bubble` = on the side bubble) and lets the intent delay pass.
fn hover(i: &mut Island, bubble: bool) {
    let (px, py, _, ph) = i.pill_rect();
    let x = if bubble { px + i.spring.w.pos + BUBBLE_GAP + BUBBLE_W / 2.0 } else { px + i.spring.w.pos / 2.0 };
    i.pointer_moved(i.now, x, py + ph / 2.0);
    draw_frames(i, 60);
}

#[test]
fn hovering_the_timer_bubble_opens_the_timer_not_the_music() {
    let mut i = Island::new(Config::default());
    i.data = data();
    i.push(T0, Input::StartTimer(600));
    draw_frames(&mut i, 200);
    assert_eq!((i.current_face(), i.shown_side), (Face::MediaMini, Some(Side::Timer)));
    hover(&mut i, true);
    assert_eq!(i.page, Some(Page::Timer));
    assert_eq!(i.current_face(), Face::TimerFull, "the bubble's activity opens");
}

#[test]
fn hovering_the_pill_opens_its_own_activity() {
    let mut i = Island::new(Config::default());
    i.data = data();
    i.push(T0, Input::StartTimer(600));
    draw_frames(&mut i, 200);
    hover(&mut i, false);
    assert_eq!(i.page, Some(Page::Media));
    assert_eq!(i.current_face(), Face::MediaFull);
}

#[test]
fn the_pointer_crossing_from_the_pill_to_the_bubble_before_it_opens_picks_the_bubble() {
    let mut i = Island::new(Config::default());
    i.data = data();
    i.push(T0, Input::StartTimer(600));
    draw_frames(&mut i, 200);
    let (px, py, _, ph) = i.pill_rect();
    i.pointer_moved(i.now, px + 20.0, py + ph / 2.0);
    let bx = px + i.spring.w.pos + BUBBLE_GAP + BUBBLE_W / 2.0;
    i.pointer_moved(i.now + 20, bx, py + ph / 2.0);
    draw_frames(&mut i, 60);
    assert_eq!(i.page, Some(Page::Timer));
    // leaving forgets it: the next visit starts from the pill again
    i.pointer_left(i.now);
    i.expand_until = 0;
    draw_frames(&mut i, 200);
    hover(&mut i, false);
    assert_eq!(i.page, Some(Page::Media));
}

#[test]
fn a_lone_timer_or_meeting_pill_opens_itself_and_an_idle_pill_opens_home() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::StartTimer(600));
    draw_frames(&mut i, 200);
    hover(&mut i, false);
    assert_eq!(i.current_face(), Face::TimerFull);
    let mut j = Island::new(Config::default());
    draw_frames(&mut j, 100);
    hover(&mut j, false);
    assert_eq!(j.current_face(), Face::IdleHover);
}

#[test]
fn a_click_on_the_pill_and_a_click_on_the_bubble_open_different_pages() {
    let mut i = Island::new(Config::default());
    i.data = data();
    i.push(T0, Input::StartTimer(600));
    draw_frames(&mut i, 200);
    let (px, py, _, ph) = i.pill_rect();
    let y = py + ph / 2.0;
    // the left part of the pill used to be covered by the bubble's hit region
    i.pointer_down(i.now, px + 20.0, y);
    assert_eq!(i.page, Some(Page::Media), "a click on the pill opens the music");
    i.activate(Action::Back);
    i.page = None;
    i.expand_until = 0;
    draw_frames(&mut i, 300);
    let (px, py, _, ph) = i.pill_rect();
    i.pointer_down(i.now, px + i.spring.w.pos + BUBBLE_GAP + BUBBLE_W / 2.0, py + ph / 2.0);
    assert_eq!(i.page, Some(Page::Timer), "a click on the bubble opens the timer");
    for h in &i.hits {
        assert!(h.rect.x1 <= i.spring.w.pos + BUBBLE_GAP + BUBBLE_W + 1.0, "no hit outside the group");
    }
}

#[test]
fn adding_an_icloud_account_asks_for_the_apple_id_then_the_app_password() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenPage(Page::Settings));
    i.activate(Action::EditField(Field::ICloudUser));
    type_text(&mut i, "not an email");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.take_commands().is_empty() && i.edit.as_ref().unwrap().field == Field::ICloudUser, "an Apple ID is an email address");
    i.push(i.now, Input::Key(KeyInput::Home));
    for _ in 0..12 {
        i.push(i.now, Input::Key(KeyInput::Delete));
    }
    type_text(&mut i, "me@icloud.com");
    i.push(i.now, Input::Key(KeyInput::Enter));
    let e = i.edit.as_ref().expect("the password field opens");
    assert_eq!(e.field, Field::ICloudPassword);
    assert!(e.secret(), "the password is drawn as dots");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.take_commands().is_empty() && i.edit.is_some(), "an empty password is refused");
    type_text(&mut i, "abcd-efgh-ijkl-mnop");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert_eq!(i.take_commands(), vec![Command::AddICloud { user: "me@icloud.com".into(), password: "abcd-efgh-ijkl-mnop".into() }]);
    assert!(i.edit.is_none());
    // escape in the middle forgets the Apple ID
    i.activate(Action::EditField(Field::ICloudUser));
    type_text(&mut i, "a@b.co");
    i.push(i.now, Input::Key(KeyInput::Enter));
    i.push(i.now, Input::Key(KeyInput::Escape));
    i.activate(Action::EditField(Field::ICloudPassword));
    type_text(&mut i, "x");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.take_commands().is_empty(), "no Apple ID left: nothing is sent");
}

#[test]
fn calendars_can_be_renamed_and_recoloured() {
    let mut i = Island::new(Config::default());
    i.data.config.calendars = vec![CalendarView { name: "Uni".into(), detail: "x.org".into(), color: (255, 69, 58) }];
    i.push(T0, Input::OpenPage(Page::Settings));
    i.activate(Action::EditField(Field::CalendarName(0)));
    assert_eq!(i.edit.as_ref().unwrap().text, "Uni", "starts from the current name");
    for _ in 0..3 {
        i.push(i.now, Input::Key(KeyInput::Backspace));
    }
    type_text(&mut i, "Lezioni");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert_eq!(i.take_commands(), vec![Command::RenameCalendar { index: 0, name: "Lezioni".into() }]);
    i.activate(Action::PickColor(0));
    assert_eq!(i.picking, Some(0));
    i.activate(Action::PickColor(0));
    assert_eq!(i.picking, None, "a second click closes the choice");
    i.activate(Action::PickColor(0));
    assert_eq!(i.activate(Action::SetColor(0, (48, 209, 88))), vec![Command::SetCalendarColor { index: 0, color: (48, 209, 88) }]);
    assert_eq!(i.picking, None);
}

#[test]
fn the_settings_page_draws_calendars_pickers_and_secret_fields() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    i.data.config.calendars = (0..3).map(|k| CalendarView { name: if k == 1 { String::new() } else { format!("Cal {k}") }, detail: "iCloud · me@icloud.com".into(), color: (10, 132, 255) }).collect();
    i.push(T0, Input::OpenPage(Page::Settings));
    i.activate(Action::PickColor(1));
    for _ in 0..200 {
        let fr = i.tick(i.now + 8, 8.0);
        i.push(i.now, Input::OpenPage(Page::Settings));
        let mut scene = Scene::new();
        i.draw(&mut scene, &f, fr.surface, 1.0);
        assert!(fr.surface.1 < 1000.0);
    }
    assert!(i.hits.iter().filter(|h| matches!(h.action, Action::SetColor(1, _))).count() == 8, "eight swatches");
    i.picking = None;
    i.activate(Action::EditField(Field::ICloudUser));
    type_text(&mut i, "me@icloud.com");
    i.push(i.now, Input::Key(KeyInput::Enter));
    type_text(&mut i, "secret");
    draw_frames(&mut i, 5);
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

#[test]
fn the_lyrics_scroll_one_line_but_a_jump_just_lands() {
    let mut i = Island::new(Config { lyrics_in_pill: true, ..Config::default() });
    i.data = data();
    let lrc: String = (1..=40).map(|n| format!("[{:02}:{:02}.00] line {n}\n", n * 3 / 60, n * 3 % 60)).collect();
    i.data.lyrics_lrc = Some(lrc);
    i.data.media.as_mut().unwrap().position_ms = 10_000.0;
    i.data.media.as_mut().unwrap().sampled_at_ms = T0;
    run(&mut i, 600);
    let idx = i.lyric_idx;
    // the next line comes up: a scroll in progress
    i.data.media.as_mut().unwrap().position_ms = 13_000.0;
    i.data.media.as_mut().unwrap().sampled_at_ms = i.now;
    run(&mut i, 16);
    assert_eq!(i.lyric_idx, idx + 1);
    assert!(i.lyric_at > 0, "one line on is animated");
    // the island wakes after a long pause (the screen was locked): thirty lines on, no scroll through them
    i.now += 1000;
    i.data.media.as_mut().unwrap().position_ms = 100_000.0;
    i.data.media.as_mut().unwrap().sampled_at_ms = i.now;
    run(&mut i, 16);
    assert!(i.lyric_idx > idx + 20);
    assert!(i.lyric_at < 0, "a jump has no animation to play");
    assert!(i.lyric_strip().is_some_and(|s| s.iter().any(|(_, slot)| *slot == 0.0)), "the current line sits in its place");
}

#[test]
fn the_pill_can_show_the_line_being_sung() {
    let f = fonts();
    let mut i = Island::new(Config { lyrics_in_pill: true, ..Config::default() });
    i.data = data();
    i.data.lyrics_lrc = Some("[00:01.00] First line\n[00:30.00] Second line".into());
    i.data.media.as_mut().unwrap().position_ms = 5000.0;
    i.data.media.as_mut().unwrap().sampled_at_ms = T0;
    run(&mut i, 600);
    assert_eq!(i.current_face(), Face::MediaMini);
    assert_eq!(i.lyric_now().map(|(t, _)| t).as_deref(), Some("First line"));
    let mut scene = Scene::new();
    let fr = i.tick(i.now + 8, 8.0);
    i.draw(&mut scene, &f, fr.surface, 1.0);
    // time moves on past the second line: the entrance restarts from nothing
    i.data.media.as_mut().unwrap().position_ms = 29_900.0;
    i.data.media.as_mut().unwrap().sampled_at_ms = i.now;
    run(&mut i, 200);
    let (t, e) = i.lyric_now().unwrap();
    assert_eq!(t, "Second line");
    assert!(e < 1.0, "still coming in");
    run(&mut i, 600);
    assert_eq!(i.lyric_now().unwrap().1, 1.0);
    // a blank line (an instrumental gap) shows nothing
    let mut j = Island::new(Config::default());
    j.data = data();
    j.data.lyrics_lrc = Some("[00:01.00] \n[00:30.00] x".into());
    j.data.media.as_mut().unwrap().position_ms = 5000.0;
    j.data.media.as_mut().unwrap().sampled_at_ms = T0;
    run(&mut j, 600);
    assert!(j.lyric_now().is_none());
}

#[test]
fn the_outline_follows_the_pill_and_its_bubble() {
    let mut i = Island::new(Config::default());
    draw_frames(&mut i, 100);
    let r = i.shape_rects();
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].2, r[0].3, r[0].4), (150.0, 28.0, 14.0), "a 150 x 28 pill with full-round ends");
    i.data = data();
    i.push(T0, Input::StartTimer(60));
    draw_frames(&mut i, 300);
    let r = i.shape_rects();
    assert_eq!(r.len(), 2, "the bubble is part of the outline");
    assert!(r[1].0 > r[0].0 + r[0].2, "to the right of the pill, past the gap");
}

#[test]
fn quick_settings_hide_what_the_machine_does_not_have() {
    let mut i = Island::new(Config::default());
    // a bare system: DND and keep awake only, no brightness, no outputs
    assert_eq!(i.qs_chips().len(), 2);
    let bare = i.qs_layout();
    assert!(bare.bri_cy.is_none() && bare.out_top.is_none());
    i.data = data();
    i.data.wifi = Some(true);
    i.data.bluetooth = Some(false);
    i.data.profile = Some("balanced".into());
    i.data.night_light = Some(false);
    i.data.brightness = Some(0.5);
    i.data.sinks = vec![Sink { name: "a".into(), label: "A".into() }];
    let full = i.qs_layout();
    assert_eq!(i.qs_chips().len(), 6);
    assert!(full.height > bare.height + 44.0 + 36.0, "two chip rows, a brightness row and the output row");
    assert!(full.bri_cy.is_some() && full.out_top.is_some());
    // the page draws at every size without escaping its own height
    let f = fonts();
    for (page_data, label) in [(Data::default(), "bare"), (i.data.clone(), "full")] {
        let mut j = Island::new(Config::default());
        j.data = page_data;
        j.push(T0, Input::OpenPage(Page::Qs));
        for _ in 0..150 {
            let fr = j.tick(j.now + 8, 8.0);
            let mut scene = Scene::new();
            j.draw(&mut scene, &f, fr.surface, 1.0);
        }
        assert_eq!(j.current_face(), Face::Qs, "{label}");
    }
}

/// Presses the middle of the first drawn control with this action.
fn press(i: &mut Island, action: Action) -> bool {
    // a card's controls say which card they are on
    let is = |a: &Action| *a == action || matches!(a, Action::OnCard(_, inner) if **inner == action);
    let Some(h) = i.hits.iter().find(|h| is(&h.action)).map(|h| h.rect) else { return false };
    let cmds = i.pointer_down(i.now, h.center().x + i.pill_origin.0, h.center().y + i.pill_origin.1);
    i.pending.extend(cmds);
    true
}

#[test]
fn the_x_on_a_notification_in_the_list_removes_only_that_one() {
    let mut i = Island::new(Config::default());
    for n in 0..3u32 {
        i.history.push((Incoming { id: n, app: "a".into(), summary: format!("n{n}"), ..Default::default() }, T0));
    }
    i.push(T0, Input::OpenPage(Page::Notifs));
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::HistoryRemove(1)), "the x of the second row is drawn");
    assert_eq!(i.history.iter().map(|(n, _)| n.summary.as_str()).collect::<Vec<_>>(), ["n0", "n2"]);
    draw_frames(&mut i, 10);
    assert!(press(&mut i, Action::HistoryRemove(0)));
    assert_eq!(i.history.len(), 1, "and the first one");
}

#[test]
fn the_clipboard_page_pins_and_forgets_an_entry() {
    let mut i = Island::new(Config::default());
    i.data.clipboard = vec!["one".into(), "two".into(), "three".into()];
    i.push(T0, Input::OpenPage(Page::Clip));
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::ClipPin(1)), "a star on each row");
    assert!(i.take_commands().contains(&Command::PinClipboard("two".into())));
    draw_frames(&mut i, 10);
    assert!(press(&mut i, Action::ClipForget(2)), "and an x");
    assert!(i.take_commands().contains(&Command::ForgetClipboard("three".into())));
    i.data.clipboard = (0..12).map(|n| n.to_string()).collect();
    draw_frames(&mut i, 200);
    assert!(i.hits.iter().any(|h| h.action == Action::ClipPin(CLIP_ROWS - 1)));
    assert!(!i.hits.iter().any(|h| h.action == Action::ClipPin(CLIP_ROWS)), "only the first rows are listed");
}

#[test]
fn the_sound_page_picks_devices_and_sets_the_volume_of_an_app() {
    let mut i = Island::new(Config::default());
    let sink = |n: &str| Sink { name: n.into(), label: n.into() };
    i.data.sinks = vec![sink("speaker"), sink("bluez_output.x")];
    i.data.sink = Some("speaker".into());
    i.data.sources = vec![sink("mic")];
    i.data.source = Some("mic".into());
    i.data.streams = vec![AppStream { id: 7, name: "Spotify".into(), volume: 60, muted: false }];
    i.push(T0, Input::OpenPage(Page::Audio));
    draw_frames(&mut i, 200);
    assert_eq!(i.current_face(), Face::Audio);
    assert!(press(&mut i, Action::Cmd(Command::SetSink("bluez_output.x".into()))), "an output row");
    assert!(i.take_commands().contains(&Command::SetSink("bluez_output.x".into())));
    draw_frames(&mut i, 10);
    assert!(press(&mut i, Action::Cmd(Command::SetStreamVolume { id: 7, percent: 70 })), "plus raises by ten");
    draw_frames(&mut i, 10);
    assert!(press(&mut i, Action::Cmd(Command::SetStreamVolume { id: 7, percent: 50 })), "minus lowers by ten");
    draw_frames(&mut i, 10);
    assert!(press(&mut i, Action::Cmd(Command::ToggleStreamMute(7))));
    let (_, h) = draw_frames(&mut i, 200);
    assert!(h >= i.audio_height(), "the page is as tall as its rows");
}

#[test]
fn the_output_row_of_quick_settings_opens_the_sound_page() {
    let mut i = Island::new(Config::default());
    i.data.sinks = vec![Sink { name: "a".into(), label: "A".into() }, Sink { name: "b".into(), label: "B".into() }];
    i.data.sink = Some("a".into());
    i.push(T0, Input::OpenPage(Page::Qs));
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::Open(Page::Audio)));
    draw_frames(&mut i, 200);
    assert_eq!(i.current_face(), Face::Audio);
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
fn files_to_send_open_a_page_of_devices_and_a_click_sends_them() {
    let mut i = Island::new(Config::default());
    let peer = |a: &str| island_core::localsend::Peer { alias: a.into(), ip: "10.0.0.2".into(), port: 53317, https: true, model: "Pixel 8".into(), kind: "mobile".into(), taildrop: false };
    i.push(T0, Input::SendFiles(vec![]));
    assert!(i.take_commands().is_empty(), "no files, no page");
    i.push(T0, Input::SendFiles(vec!["/tmp/a.txt".into(), "/tmp/b.txt".into()]));
    assert!(i.take_commands().contains(&Command::ScanPeers), "the network is scanned at once");
    i.data.peers_scanning = true;
    draw_frames(&mut i, 200);
    assert_eq!(i.current_face(), Face::Send);
    i.data.peers = vec![peer("Pixel"), peer("Laptop")];
    i.data.peers_scanning = false;
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::SendTo(1)));
    let cmds = i.take_commands();
    assert!(cmds.contains(&Command::SendFiles { peer: peer("Laptop"), paths: vec!["/tmp/a.txt".into(), "/tmp/b.txt".into()] }));
    assert!(i.send_files.is_empty(), "the files went");
    draw_frames(&mut i, 300);
    assert_ne!(i.current_face(), Face::Send, "the page closes once the files are on their way");
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
fn a_card_with_a_one_time_code_copies_it_on_a_click() {
    let mut i = Island::new(Config::default());
    let n = |id: u32, summary: &str| Input::Notification(Incoming { id, app: "Bank".into(), summary: summary.into(), body: "Do not share it".into(), ..Default::default() });
    i.push(T0, n(1, "Your verification code is 482913"));
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::CopyCode("482913".into())), "the whole card is the button");
    assert_eq!(i.take_commands(), vec![Command::CopyCode("482913".into())]);
    assert!(i.code_copied.is_some(), "the card says it is copied");
    i.push(i.now, Input::NotificationClosed(1));
    i.push(i.now, n(2, "Meeting at 10:30"));
    draw_frames(&mut i, 200);
    assert!(!press(&mut i, Action::CopyCode("1030".into())));
    assert!(press(&mut i, Action::NotifOpen), "a card without a code still opens its sender");
}

#[test]
fn a_copied_link_or_mail_has_its_own_button_on_the_clipboard_page() {
    let mut i = Island::new(Config::default());
    i.data.clipboard = vec!["plain words".into(), "https://example.org/x".into(), "me@uni.it".into(), "#ff8000".into()];
    i.push(T0, Input::OpenPage(Page::Clip));
    draw_frames(&mut i, 200);
    assert!(!press(&mut i, Action::ClipAct(0)), "nothing to do with plain words");
    assert!(!press(&mut i, Action::ClipAct(3)), "a colour is only shown");
    assert!(press(&mut i, Action::ClipAct(1)));
    assert_eq!(i.take_commands(), vec![Command::OpenUrl("https://example.org/x".into())]);
    draw_frames(&mut i, 10);
    assert!(press(&mut i, Action::ClipAct(2)));
    assert_eq!(i.take_commands(), vec![Command::OpenUrl("mailto:me@uni.it".into())]);
}

#[test]
fn a_program_that_hogs_the_machine_gets_a_card_once_and_buttons_on_the_system_page() {
    let mut i = Island::new(Config::default());
    let hog = Culprit { name: "ffmpeg".into(), pids: vec![11, 12], cores: 3.4, mem_mb: 900.0, memory: false, protected: false };
    run(&mut i, 1000);
    i.data.culprit = Some(hog.clone());
    run(&mut i, 6000);
    let card = i.notif.clone().expect("a card");
    assert_eq!((card.summary.as_str(), card.body.as_str()), ("ffmpeg is using 3.4 cores", "Tap to pause or quit it"));
    i.notif = None;
    run(&mut i, 6000);
    assert!(i.notif.is_none(), "not again for the same program within ten minutes");
    i.push(i.now, Input::OpenPage(Page::Sys));
    i.data.system = Some(SystemStats::default());
    i.data.paused = vec!["chrome".into()];
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::Cmd(Command::PauseProcs { name: "ffmpeg".into(), pids: vec![11, 12] })));
    assert!(i.take_commands().contains(&Command::PauseProcs { name: "ffmpeg".into(), pids: vec![11, 12] }));
    draw_frames(&mut i, 10);
    assert!(press(&mut i, Action::Cmd(Command::QuitProcs(vec![11, 12]))));
    draw_frames(&mut i, 10);
    assert!(press(&mut i, Action::Cmd(Command::ResumeProcs("chrome".into()))), "what was paused can be resumed");
    i.data.culprit = Some(Culprit { name: "niri".into(), protected: true, ..hog });
    draw_frames(&mut i, 200);
    assert!(!i.hits.iter().any(|h| matches!(&h.action, Action::Cmd(Command::QuitProcs(_)))), "the session cannot be quit from here");
}

#[test]
fn a_broken_network_says_where_and_a_click_reconnects() {
    use island_core::netstate::Health;
    let mut i = Island::new(Config::default());
    run(&mut i, 1000);
    i.data.net = Health::NoInternet;
    run(&mut i, 6000);
    let card = i.notif.clone().expect("a card");
    assert_eq!((card.summary.as_str(), card.body.as_str()), ("No internet", "The router answers: the problem is beyond it · tap to reconnect"));
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::NotifOpen));
    assert!(i.take_commands().contains(&Command::ReconnectNetwork));
    i.notif = None;
    i.data.net = Health::Online;
    run(&mut i, 1000);
    assert_eq!(i.notif.as_ref().map(|n| n.summary.as_str()), Some("Back online"));
    assert_eq!(i.icon_count(false), 0, "no warning icon once it works");
    i.data.net = Health::RouterDown;
    assert_eq!(i.icon_count(false), 1, "a warning icon while it does not");
}

#[test]
fn an_event_with_a_place_gets_a_trip_and_a_time_to_leave() {
    use island_core::travel::{Mode, Trip};
    let mut i = Island::new(Config::default());
    let ev = |id: &str, loc: &str, start: i64| Event { id: id.into(), title: "Seminar".into(), start_ms: start, end_ms: start + 3_600_000, location: loc.into(), ..Default::default() };
    i.data.events = vec![ev("call", "https://teams.microsoft.com/x", T0 + 3 * 3_600_000), ev("far", "Polo Fibonacci, Pisa", T0 + 5 * 3_600_000), ev("near", "Largo Pontecorvo 3, Pisa", T0 + 90 * 60_000)];
    i.push(T0, Input::OpenPage(Page::Today));
    i.now = T0;
    assert_eq!(i.trip_event().map(|e| e.id.as_str()), Some("near"), "a link is no place, and the nearest comes first");
    run(&mut i, 1000);
    let cmds = i.take_commands();
    assert!(cmds.iter().any(|c| matches!(c, Command::PlanTrip { event_id, mode: Mode::Car, margin_secs: 300, .. } if event_id == "near")), "asked for the way by car");
    run(&mut i, 1000);
    assert!(!i.take_commands().iter().any(|c| matches!(c, Command::PlanTrip { .. })), "not asked again within ten minutes");
    i.push(i.now, Input::OpenPage(Page::Today));
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::TravelMode(Mode::Transit)));
    assert!(i.take_commands().contains(&Command::SetOption { key: "travel_mode".into(), value: OptionValue::Text("transit".into()) }));
    run(&mut i, 1000);
    assert!(i.take_commands().iter().any(|c| matches!(c, Command::PlanTrip { mode: Mode::Transit, .. })), "the other way is asked for at once");
    // the way is known: leave in two minutes
    i.data.trips = vec![TripInfo { event_id: "near".into(), mode: Mode::Transit, trip: Some(Trip { secs: 1500, meters: None, leave_ms: Some(i.now + 2 * 60_000), transfers: Some(1) }), error: None, at_ms: i.now, dest: None }];
    run(&mut i, 1000);
    let card = i.notif.clone().expect("a card");
    assert_eq!(card.summary, "Leave in 2 min for Seminar");
    assert!(card.body.starts_with("transit · 25 min"));
    i.notif = None;
    run(&mut i, 3000);
    assert!(i.notif.is_none(), "told once");
    i.cfg.travel_times = false;
    assert!(i.trip_event().is_none(), "switched off");
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
fn a_long_stretch_on_one_app_gets_a_break_nudge_once_per_period() {
    let mut i = Island::new(Config { break_minutes: 30, ..Default::default() });
    run(&mut i, 1000);
    i.push(i.now, Input::Focused { app: "zen".into(), title: "x".into() });
    run(&mut i, 29 * 60_000);
    assert!(i.notif.is_none());
    run(&mut i, 63_000);
    let card = i.notif.clone().expect("a nudge");
    assert!(card.summary.starts_with("30 minutes on Zen") || card.summary.starts_with("31 minutes on Zen"), "{}", card.summary);
    i.notif = None;
    run(&mut i, 10 * 60_000);
    assert!(i.notif.is_none(), "not again within the period");
}

#[test]
fn the_activity_page_shows_the_day_and_the_last_half_hour() {
    let mut i = Island::new(Config::default());
    run(&mut i, 1000);
    i.push(i.now, Input::Focused { app: "code".into(), title: "main.rs".into() });
    run(&mut i, 4 * 60_000);
    i.push(i.now, Input::Notification(Incoming { id: 1, app: "Telegram".into(), summary: "Hi".into(), ..Default::default() }));
    i.push(i.now, Input::Focused { app: "zen".into(), title: "Docs".into() });
    run(&mut i, 60_000);
    i.data.clipboard = vec!["first".into()];
    run(&mut i, 3000);
    i.data.clipboard = vec!["second thing".into(), "first".into()];
    run(&mut i, 3000);
    let rows = i.rewind_rows();
    assert_eq!(rows[0].text, "second thing");
    assert_eq!(rows[0].copy.as_deref(), Some("second thing"));
    assert!(rows.iter().any(|r| r.text == "Telegram: Hi") && rows.iter().any(|r| r.text == "Code: main.rs") && rows.iter().any(|r| r.text == "Zen: Docs"));
    i.push(i.now, Input::OpenPage(Page::Activity));
    draw_frames(&mut i, 200);
    assert_eq!(i.current_face(), Face::Activity);
    assert!(press(&mut i, Action::ActivityTab(1)));
    draw_frames(&mut i, 50);
    assert!(press(&mut i, Action::RewindCopy(0)));
    assert!(i.take_commands().contains(&Command::RestoreClipboard("second thing".into())));
    let (day, totals) = i.attention_export();
    assert!(day > 0 && totals.iter().any(|(a, ms)| a == "Code" && *ms >= 4 * 60_000));
    assert!(i.pages().windows(2).any(|w| w == [Page::Sys, Page::Activity]), "the dock shows it after the system page");
}

#[test]
fn the_system_page_lists_who_uses_the_microphone_and_the_camera() {
    let mut i = Island::new(Config::default());
    i.data.system = Some(SystemStats::default());
    i.data.mic_users = vec![MicUser { stream: 7, name: "Zen".into(), pid: Some(100), muted: false }];
    i.data.camera_users = vec![("zoom".into(), vec![200, 201]), ("pipewire".into(), vec![5])];
    i.push(T0, Input::OpenPage(Page::Sys));
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::Cmd(Command::MuteMicStream { stream: 7, mute: true })));
    assert!(i.take_commands().contains(&Command::MuteMicStream { stream: 7, mute: true }));
    draw_frames(&mut i, 10);
    assert!(press(&mut i, Action::Cmd(Command::QuitProcs(vec![200, 201]))), "the camera's program can be quit");
    assert!(!i.hits.iter().any(|h| h.action == Action::Cmd(Command::QuitProcs(vec![5]))), "the session cannot");
    i.data.mic_users[0].muted = true;
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::Cmd(Command::MuteMicStream { stream: 7, mute: false })), "and it can be unmuted");
}

#[test]
fn a_player_with_no_artist_draws_the_media_page_in_place() {
    // Microsoft Teams in a browser is an MPRIS player with a long title, no artist, no art and a 7 s length
    let mut i = Island::new(Config::default());
    i.data.media = Some(Media { track_id: "z".into(), title: "Teams and Channels | 26 - decorators and OOP in Python | Microsoft Teams".into(), artist: String::new(), playing: false, position_ms: 0.0, sampled_at_ms: T0, length_ms: 7000.0, can_seek: true, players: 3, player: "Zen".into(), ..Default::default() });
    i.push(T0, Input::OpenPage(Page::Media));
    draw_frames(&mut i, 300);
    assert_eq!(i.current_face(), Face::MediaFull);
    let seek = i.hits.iter().find(|h| h.action == Action::SeekBar).expect("the seek bar").rect;
    assert!(seek.y0 < 70.0 && seek.x1 < 420.0, "the bar sits under the title, inside the island: {seek:?}");
    // and the picture: an empty text must not displace what is drawn after it
    let f = fonts();
    let mut with_empty = Scene::new();
    let mut without = Scene::new();
    i.draw(&mut with_empty, &f, (420.0, 200.0), 1.0);
    i.data.media.as_mut().unwrap().artist = " ".into();
    i.draw(&mut without, &f, (420.0, 200.0), 1.0);
    let _ = (with_empty, without);
}

#[test]
fn the_map_page_asks_for_a_position_and_the_tiles_in_view_and_offers_what_to_do() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenPage(Page::Map));
    run(&mut i, 1000);
    assert!(i.take_commands().contains(&Command::RefreshPosition), "no position yet: ask for one");
    assert!(i.map_view().is_none());
    draw_frames(&mut i, 50);
    assert_eq!(i.current_face(), Face::Map);
    i.data.position = Some(Position { lat: 43.7255, lon: 10.3908, accuracy_m: 300.0, at_ms: i.now, source: PosSource::Geoclue });
    i.push(i.now, Input::OpenPage(Page::Map));
    run(&mut i, 500);
    let cmds = i.take_commands();
    let loads: Vec<&Command> = cmds.iter().filter(|c| matches!(c, Command::LoadTile { .. })).collect();
    assert!(loads.len() >= 2 && loads.len() <= 9, "the tiles of the view: {}", loads.len());
    assert!(matches!(loads[0], Command::LoadTile { z: 16, template, .. } if template.contains("{z}")), "close, for a precise fix");
    run(&mut i, 500);
    assert!(!i.take_commands().iter().any(|c| matches!(c, Command::LoadTile { .. })), "each tile is asked for once");
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::MapZoom(1)));
    run(&mut i, 300);
    assert!(i.take_commands().iter().any(|c| matches!(c, Command::LoadTile { z: 17, .. })), "zooming asks for the new tiles");
    draw_frames(&mut i, 50);
    assert!(press(&mut i, Action::MapCopy));
    assert!(i.take_commands().contains(&Command::RestoreClipboard("43.725500, 10.390800".into())));
    run(&mut i, 4000); // the card that says it is copied goes away
    i.push(i.now, Input::OpenPage(Page::Map));
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::MapOpen));
    assert!(i.take_commands().iter().any(|c| matches!(c, Command::OpenUrl(u) if u.starts_with("https://www.openstreetmap.org/?mlat=43.7255"))));
    // the next event with a known place: both on the map, and a way to get directions
    let ev = Event { id: "e".into(), title: "Seminar".into(), start_ms: i.now + 3_600_000, end_ms: i.now + 7_200_000, location: "Via Roma 1, Pisa".into(), ..Default::default() };
    i.data.events = vec![ev];
    i.data.trips = vec![TripInfo { event_id: "e".into(), mode: island_core::travel::Mode::Car, trip: None, error: None, at_ms: i.now, dest: Some((43.7167, 10.4017)) }];
    i.map_zoom_delta = 0;
    run(&mut i, 4000);
    let v = i.map_view().unwrap();
    assert!(v.dest.is_some() && v.zoom >= 12 && v.zoom <= 16, "fits both: {}", v.zoom);
    i.push(i.now, Input::OpenPage(Page::Map));
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::MapRoute));
    assert!(i.take_commands().iter().any(|c| matches!(c, Command::OpenUrl(u) if u.contains("directions?engine=fossgis_osrm_car"))));
    assert!(i.pages().windows(2).any(|w| w == [Page::Today, Page::Map]), "the dock has it after Today");
}

#[test]
fn the_map_provider_and_its_key_are_set_from_the_island_and_the_key_stays_out_of_sight() {
    let mut i = Island::new(Config::default());
    i.data.position = Some(Position { lat: 43.7, lon: 10.4, accuracy_m: 100.0, at_ms: T0, source: PosSource::Geoclue });
    i.push(T0, Input::OpenPage(Page::Settings));
    draw_frames(&mut i, 100);
    i.activate(Action::EditField(Field::MapTiles));
    assert!(i.edit.is_some_and(|e| !e.secret()), "the URL is shown");
    i.edit = None;
    // a URL that is no tile URL is refused, with a word on it
    i.activate(Action::EditField(Field::MapTiles));
    i.edit.as_mut().unwrap().text = "http://x/{z}".into();
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.edit.is_some() && i.cfg.map_tile_url == island_core::map::DEFAULT_TILES, "refused, still editing");
    i.edit.as_mut().unwrap().text = "https://basemaps.cartocdn.com/rastertiles/dark_all/{z}/{x}/{y}.png?key={key}".into();
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.cfg.map_tile_url.contains("?key={key}"));
    assert!(i.take_commands().iter().any(|c| matches!(c, Command::SetOption { key, value: OptionValue::Text(t) } if key == "map_tile_url" && t.contains("{key}"))));
    // no key yet: no tile is asked for, and the page says why
    i.push(i.now, Input::OpenPage(Page::Map));
    run(&mut i, 500);
    assert!(!i.take_commands().iter().any(|c| matches!(c, Command::LoadTile { .. })), "a key is needed first");
    i.activate(Action::EditField(Field::MapKey));
    assert!(i.edit.as_ref().is_some_and(|e| e.secret()), "the key is drawn as dots");
    i.edit.as_mut().unwrap().text = "abc123".into();
    i.push(i.now, Input::Key(KeyInput::Enter));
    let cmds = i.take_commands();
    assert!(cmds.contains(&Command::SetOption { key: "map_api_key".into(), value: OptionValue::Text("abc123".into()) }));
    assert!(i.data.config.map_key_set);
    i.push(i.now, Input::OpenPage(Page::Map));
    run(&mut i, 500);
    assert!(i.take_commands().iter().any(|c| matches!(c, Command::LoadTile { template, .. } if template.contains("{key}"))), "with the key, tiles are asked for (the key is added where the tiles are fetched)");
    draw_frames(&mut i, 100);
    i.activate(Action::EditField(Field::MapKey));
    i.edit.as_mut().unwrap().text = "two words".into();
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.edit.is_some(), "a key with a space is refused");
}

#[test]
fn the_text_rows_of_the_options_page_can_be_edited() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenPage(Page::Options));
    draw_frames(&mut i, 200);
    for (field, key, typed, value) in [
        (Field::Screen, "screen", "eDP-1", "eDP-1"),
        (Field::TimerPresets, "timer_presets", "5m, 20m", "5m, 20m"),
        (Field::MapTiles, "map_tile_url", "esri-dark", "https://server.arcgisonline.com/ArcGIS/rest/services/Canvas/World_Dark_Gray_Base/MapServer/tile/{z}/{y}/{x}"),
    ] {
        // the map tiles are under Places, the other two under General
        i.activate(Action::OptionsTab(if field == Field::MapTiles { 4 } else { 0 }));
        draw_frames(&mut i, 60);
        assert!(press(&mut i, Action::EditField(field)), "{key}: the row is a button");
        // time passes with the field open: it must stay open on this page
        run(&mut i, 600);
        assert!(i.edit.is_some(), "{key}: the field stays open on the options page");
        type_text(&mut i, typed);
        i.push(i.now, Input::Key(KeyInput::Enter));
        assert!(i.take_commands().contains(&Command::SetOption { key: key.into(), value: OptionValue::Text(value.into()) }), "{key}");
        assert!(i.edit.is_none());
        draw_frames(&mut i, 20);
    }
}

#[test]
fn the_map_is_dragged_not_clicked_and_zooms_with_the_wheel() {
    let mut i = Island::new(Config::default());
    i.data.position = Some(Position { lat: 43.7255, lon: 10.3908, accuracy_m: 100.0, at_ms: T0, source: PosSource::Geoclue });
    i.push(T0, Input::OpenPage(Page::Map));
    run(&mut i, 500);
    i.take_commands();
    draw_frames(&mut i, 200);
    let area = i.hits.iter().find(|h| h.action == Action::MapPan).expect("the map is a drag area").rect;
    let (ox, oy) = i.pill_origin;
    let (x, y) = (area.center().x + ox - 40.0, area.center().y + oy + 30.0);
    // a click on the map does nothing: no page in the browser
    let cmds = i.pointer_down(i.now, x, y);
    i.pointer_up(i.now);
    assert!(cmds.is_empty() && !i.take_commands().iter().any(|c| matches!(c, Command::OpenUrl(_))), "a click on the map opens nothing");
    let before = i.map_view().unwrap();
    assert!(!i.map_is_panned());
    // drag it 120 px to the right: what is west of here comes into view
    i.pointer_down(i.now, x, y);
    i.pointer_moved(i.now, x + 60.0, y);
    i.pointer_moved(i.now, x + 120.0, y + 20.0);
    i.pointer_up(i.now);
    let after = i.map_view().unwrap();
    assert!(i.map_is_panned() && after.zoom == before.zoom, "the zoom is kept while dragging");
    assert!(after.center.1 < before.center.1 - 0.001, "the centre moved west: {:?} -> {:?}", before.center, after.center);
    assert!(after.center.0 > before.center.0, "and north, because the drag went down");
    assert_eq!(after.me, before.me, "the position itself did not move");
    // the tiles around the new centre are asked for
    run(&mut i, 300);
    assert!(i.take_commands().iter().any(|c| matches!(c, Command::LoadTile { .. })));
    // the wheel zooms the dragged map; a button brings it back
    i.scroll(i.now, 0.0, 2.0);
    assert_eq!(i.map_view().unwrap().zoom, before.zoom + 2);
    draw_frames(&mut i, 100);
    assert!(press(&mut i, Action::MapRecenter), "a button appears once the map is away");
    assert!(!i.map_is_panned() && i.map_view().unwrap().center == before.center && i.map_view().unwrap().zoom == before.zoom);
}

#[test]
fn a_place_is_searched_on_the_map_picked_and_routed() {
    use island_core::travel::{Mode, Place, Segment, Trip};
    let mut i = Island::new(Config::default());
    i.data.position = Some(Position { lat: 43.7255, lon: 10.3908, accuracy_m: 60.0, at_ms: T0, source: PosSource::Geoclue });
    i.push(T0, Input::OpenPage(Page::Map));
    run(&mut i, 300);
    i.take_commands();
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::EditField(Field::MapSearch)), "a search button on the map");
    run(&mut i, 600);
    assert!(i.edit.is_some(), "the field stays open on the map page");
    type_text(&mut i, "stazione");
    i.push(i.now, Input::Key(KeyInput::Enter));
    let cmds = i.take_commands();
    assert!(cmds.iter().any(|c| matches!(c, Command::SearchPlaces { query, near: Some((la, _)) } if query == "stazione" && (*la - 43.7255).abs() < 1e-9)), "searched near here");
    assert!(i.data.places_searching && i.data.places.as_ref().is_some_and(|p| p.is_empty()));
    // the answer arrives
    let station = Place { name: "Pisa Centrale".into(), detail: "Piazza della Stazione".into(), lat: 43.7079, lon: 10.3984 };
    i.data.places = Some(vec![station.clone(), Place { name: "Stazione San Rossore".into(), detail: "Pisa".into(), lat: 43.7311, lon: 10.3697 }]);
    i.data.places_searching = false;
    i.push(i.now, Input::OpenPage(Page::Map));
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::MapPick(0)));
    let cmds = i.take_commands();
    assert!(cmds.iter().any(|c| matches!(c, Command::PlanRoute { dest, mode: Mode::Car, from } if dest == &station && (from.0 - 43.7255).abs() < 1e-9)), "the route is asked for from here");
    assert!(i.data.places.is_none() && i.data.route_pending && i.map_dest.as_ref() == Some(&station));
    let v = i.map_view().unwrap();
    assert_eq!(v.dest.as_ref().map(|d| d.1.as_str()), Some("Pisa Centrale"), "the pin is the picked place");
    // the route comes back
    i.data.route = Some(RouteInfo { dest: station.clone(), mode: Mode::Car, trip: Some(Trip { secs: 420, meters: Some(2600), leave_ms: Some(i.now), transfers: None }), error: None, segments: vec![Segment { points: vec![(43.7255, 10.3908), (43.7079, 10.3984)], walk: false }] });
    i.data.route_pending = false;
    i.push(i.now, Input::OpenPage(Page::Map));
    draw_frames(&mut i, 200);
    // another way: the route is asked for again
    assert!(press(&mut i, Action::TravelMode(Mode::Walk)));
    assert!(i.take_commands().iter().any(|c| matches!(c, Command::PlanRoute { mode: Mode::Walk, .. })));
    draw_frames(&mut i, 50);
    assert!(press(&mut i, Action::MapClearDest));
    assert!(i.map_dest.is_none() && i.data.route.is_none());
}

#[test]
fn only_todays_events_get_a_trip() {
    let mut i = Island::new(Config::default());
    let t = crate::clock::civil(T0, 0);
    let day = crate::clock::midnight_ms(t.year, t.month, t.day, 0);
    let ev = |id: &str, start: i64| Event { id: id.into(), title: id.into(), start_ms: start, end_ms: start + 3_600_000, location: "Via Roma 1, Pisa".into(), ..Default::default() };
    i.data.events = vec![ev("tomorrow", day + 26 * 3_600_000), ev("today", day + 22 * 3_600_000)];
    i.now = day + 9 * 3_600_000;
    assert_eq!(i.trip_event().map(|e| e.id.as_str()), Some("today"));
    i.data.events = vec![ev("tomorrow", day + 26 * 3_600_000)];
    assert!(i.trip_event().is_none(), "tomorrow is not today, even when it is within 24 hours");
    i.now = day + 20 * 3_600_000;
    i.data.events = vec![ev("tomorrow-early", day + 30 * 3_600_000)];
    assert!(i.trip_event().is_none());
}

#[test]
fn the_x_on_the_notification_card_skips_to_the_next_one() {
    let mut i = Island::new(Config::default());
    let n = |id: u32, app: &str| Input::Notification(Incoming { id, app: app.into(), summary: format!("n{id}"), ..Default::default() });
    i.push(T0, n(1, "a"));
    i.push(T0, n(2, "b"));
    draw_frames(&mut i, 200);
    assert_eq!(i.notif.as_ref().map(|n| n.id), Some(1));
    assert!(press(&mut i, Action::Dismiss), "the x is drawn on the card");
    assert!(i.take_commands().iter().any(|c| matches!(c, Command::NotificationClosed { id: 1, reason: CloseReason::Dismissed })));
    draw_frames(&mut i, 400);
    assert_eq!(i.notif.as_ref().map(|n| n.id), Some(2), "the next one is shown");
}

#[test]
fn a_button_inside_a_row_beats_the_row_in_every_page() {
    let mut i = Island::new(Config::default());
    i.data.config.calendars = vec![CalendarView { name: "A".into(), detail: "a.org".into(), color: (1, 2, 3) }];
    i.push(T0, Input::OpenPage(Page::Settings));
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::RemoveCalendar(0)), "the calendar's x");
    assert!(i.take_commands().contains(&Command::RemoveCalendar(0)));
    assert!(i.edit.is_none(), "and not the rename field under it");
}

#[test]
fn clicking_a_notification_goes_to_the_app_that_sent_it() {
    let mut i = Island::new(Config::default());
    let tg = Incoming { id: 4, app: "Telegram Desktop".into(), summary: "Anna".into(), desktop_entry: "org.telegram.desktop".into(), actions: vec![NotifAction { key: "default".into(), label: "".into() }], ..Default::default() };
    i.push(T0, Input::Notification(tg));
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::NotifOpen), "the card body is a hit");
    let cmds = i.take_commands();
    assert_eq!(cmds[0], Command::InvokeAction { id: 4, key: "default".into() }, "the sender's own default action first");
    assert_eq!(cmds[1], Command::FocusApp { desktop_entry: "org.telegram.desktop".into(), app: "Telegram Desktop".into() });
    assert!(cmds.iter().any(|c| matches!(c, Command::NotificationClosed { id: 4, .. })), "and the card goes away");
    assert!(i.notif.is_none());
    // a sender without a default action still gets its window raised
    i.push(i.now, Input::Notification(Incoming { id: 5, app: "Zen".into(), summary: "site".into(), desktop_entry: "zen".into(), ..Default::default() }));
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::NotifOpen));
    let cmds = i.take_commands();
    assert_eq!(cmds[0], Command::FocusApp { desktop_entry: "zen".into(), app: "Zen".into() });
    assert!(!cmds.iter().any(|c| matches!(c, Command::InvokeAction { .. })));
}

#[test]
fn clicking_a_row_of_the_history_focuses_its_app_and_removes_it() {
    let mut i = Island::new(Config::default());
    i.restore_history(vec![(Incoming { id: 1, app: "Mail".into(), summary: "a".into(), desktop_entry: "mail".into(), ..Default::default() }, 0), (Incoming { id: 2, app: "Zen".into(), summary: "b".into(), desktop_entry: "zen".into(), ..Default::default() }, 0)]);
    i.push(T0, Input::OpenPage(Page::Notifs));
    draw_frames(&mut i, 200);
    i.activate(Action::HistoryOpen(1));
    assert_eq!(i.take_commands(), vec![Command::FocusApp { desktop_entry: "zen".into(), app: "Zen".into() }]);
    assert_eq!(i.history.len(), 1, "an opened notification is read");
}

#[test]
fn a_single_notification_card_has_an_x_that_dismisses_it() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Notification(Incoming { id: 9, app: "a".into(), summary: "alone".into(), ..Default::default() }));
    draw_frames(&mut i, 200);
    assert!(i.queue.is_empty() && i.notif.is_some(), "nothing else waits");
    assert!(press(&mut i, Action::Dismiss), "the x is there even for a lone notification");
    assert!(i.take_commands().iter().any(|c| matches!(c, Command::NotificationClosed { id: 9, reason: CloseReason::Dismissed })));
    assert!(i.notif.is_none());
    // and one with buttons (an actions card) has it too
    i.push(i.now, Input::Notification(Incoming { id: 10, actions: vec![NotifAction { key: "open".into(), label: "Open".into() }], ..Default::default() }));
    draw_frames(&mut i, 300);
    assert!(press(&mut i, Action::Dismiss));
    assert!(i.notif.is_none());
}

#[test]
fn today_lists_what_is_running_above_the_day() {
    let mut i = Island::new(Config::default());
    let quiet = i.today_height();
    assert!(i.running_now().is_empty());
    i.push(T0, Input::StartTimer(600));
    i.push(T0, Input::Progress("40 Backup".into()));
    let rows = i.running_now();
    assert_eq!(rows.len(), 2, "the timer and the command");
    assert_eq!((rows[0].label.as_str(), rows[0].page), ("Timer", Page::Timer));
    assert_eq!((rows[1].label.as_str(), rows[1].right.as_str()), ("Backup", "40%"));
    assert_eq!(i.today_height(), quiet + 60.0, "each row takes its place on the page");
    i.push(i.now, Input::ProgressDone("done".into()));
    assert_eq!(i.running_now().len(), 1, "a finished command is not running");
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
fn several_agents_show_up_on_today_each_with_its_status() {
    let mut i = Island::new(Config::default());
    let act = |id: &str, label: &str, detail: &str| island_core::progress::Activity { id: id.into(), label: label.into(), detail: detail.into(), icon: Some("bolt".into()), ..Default::default() };
    i.push(T0, Input::Activity(island_core::progress::State::Run, act("s1", "Claude · site", "Edit lib.rs")));
    i.push(T0, Input::Activity(island_core::progress::State::Run, act("s2", "Codex · api", "thinking")));
    i.push(T0, Input::StartNamedTimer(600, "tea".into()));
    let rows: Vec<String> = i.running_now().into_iter().map(|r| r.label).collect();
    assert_eq!(rows, ["tea", "Claude · site · Edit lib.rs", "Codex · api · thinking"]);
    i.push(i.now, Input::CancelTimerNamed("TEA".into()));
    assert!(i.timer.is_none(), "cancelled by name, whatever the case");
    i.push(i.now, Input::ActivityClear("s1".into()));
    assert_eq!(i.running_now().len(), 1);
}

#[test]
fn the_to_do_list_is_added_to_ticked_off_and_shown_on_today() {
    let mut i = Island::new(Config::default());
    let plain = i.today_height();
    i.push(T0, Input::TodoAdd("  buy   milk ".into()));
    i.push(T0, Input::TodoAdd("buy milk".into()));
    i.push(T0, Input::TodoAdd("   ".into()));
    i.push(T0, Input::TodoAdd("call Anna".into()));
    assert_eq!(i.todos().iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), ["buy milk", "call Anna"], "spaces squeezed, no duplicates, no empty ones");
    assert_eq!(i.today_height(), plain + 22.0 + 2.0 * 28.0, "a heading and a row each");
    i.activate(Action::TodoDone(0));
    assert_eq!(i.todos().len(), 1);
    i.activate(Action::TodoDone(9));
    assert_eq!(i.todos().len(), 1, "a stale position does nothing");
    for n in 0..8 {
        i.push(i.now, Input::TodoAdd(format!("t{n}")));
    }
    assert_eq!(i.todo_rows(), 4, "only what fits is drawn");
}

#[test]
fn the_bar_adds_a_todo() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenBar);
    for c in "todo Renew the library card".chars() {
        i.push(i.now, Input::Key(KeyInput::Text(c.to_string())));
    }
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert_eq!(i.todos().iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), ["Renew the library card"]);
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

fn evening_island() -> Island {
    // T0 is Friday 2027-01-15 08:00 UTC; the recap is at 18:30
    let mut i = Island::new(Config { evening_recap_min: 18 * 60 + 30, ..Config::default() });
    let ev = |id: &str, h: i64| Event { id: id.into(), title: id.into(), start_ms: T0 + (16 + h) * 3_600_000, end_ms: T0 + (17 + h) * 3_600_000, ..Default::default() };
    i.data.events = vec![ev("Reti", 1), ev("Lab", 6)];
    i
}

fn card(i: &Island) -> Option<(String, String)> {
    i.notif.as_ref().map(|n| (n.summary.clone(), n.body.clone()))
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

fn morning_island() -> Island {
    // 07:30, from 07:00 on; Friday 2027-01-15, events today at 09:00 and 13:00 (T0 is 08:00 UTC)
    let mut i = Island::new(Config { morning_recap_min: 7 * 60, ..Config::default() });
    let ev = |id: &str, h: i64| Event { id: id.into(), title: id.into(), start_ms: T0 + h * 3_600_000, end_ms: T0 + (h + 1) * 3_600_000, ..Default::default() };
    i.data.events = vec![ev("Reti", 1), ev("Lab", 5)];
    i
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

fn type_in_bar(i: &mut Island, text: &str) {
    i.push(i.now.max(T0), Input::OpenBar);
    for c in text.chars() {
        i.push(i.now, Input::Key(KeyInput::Text(c.to_string())));
    }
}

#[test]
fn the_bar_makes_a_password_that_is_kept_out_of_the_history_and_cleared() {
    let mut i = Island::new(Config::default());
    type_in_bar(&mut i, "pw 24");
    assert!(matches!(i.bar_cmd(), Some(island_core::bar::Cmd::Gen(island_core::quick::Gen::Password(24)))));
    i.push(i.now, Input::Key(KeyInput::Enter));
    let copied: Vec<&String> = i.pending.iter().filter_map(|c| if let Command::CopyCode(t) = c { Some(t) } else { None }).collect();
    assert_eq!(copied.len(), 1, "copied like a verification code, not as an ordinary copy");
    assert_eq!(copied[0].chars().count(), 24);
    assert!(!i.pending.iter().any(|c| matches!(c, Command::RestoreClipboard(_))));
    assert_eq!(card(&i).map(|c| c.0), Some("Password copied".to_string()), "and the card does not show it");
}

#[test]
fn dice_and_dates_and_money_in_the_bar() {
    let mut i = Island::new(Config::default());
    i.data.rates = vec![("USD".into(), 1.1)];
    type_in_bar(&mut i, "20 usd in eur");
    assert!(matches!(i.bar_cmd(), Some(island_core::bar::Cmd::Value(v)) if v == "20 USD = 18.182 EUR"));
    i.push(i.now, Input::Key(KeyInput::Escape));
    i.tick(T0, 8.0);
    type_in_bar(&mut i, "days until 2027-01-20");
    assert!(matches!(i.bar_cmd(), Some(island_core::bar::Cmd::Value(v)) if v == "Wed 20 Jan 2027 = in 5 days"), "T0 is Friday 15 January 2027");
    i.push(i.now, Input::Key(KeyInput::Escape));
    type_in_bar(&mut i, "roll 2d6");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.pending.iter().any(|c| matches!(c, Command::RestoreClipboard(t) if t.parse::<u32>().is_ok_and(|n| (2..=12).contains(&n)))), "the total is copied");
}

#[test]
fn up_and_down_recall_earlier_lines_of_the_bar_and_a_password_is_not_kept() {
    let mut i = Island::new(Config::default());
    for line in ["10m tea", "todo buy milk", "pw 20", "10m tea"] {
        type_in_bar(&mut i, line);
        i.push(i.now, Input::Key(KeyInput::Enter));
        i.notif = None;
    }
    assert_eq!(i.bar_history(), ["todo buy milk", "10m tea"], "no duplicates, the latest last, no password");
    i.push(i.now, Input::OpenBar);
    for c in "12".chars() {
        i.push(i.now, Input::Key(KeyInput::Text(c.to_string())));
    }
    let text = |i: &Island| i.bar.as_ref().map(|b| b.text.clone());
    i.push(i.now, Input::Key(KeyInput::Up));
    assert_eq!(text(&i).as_deref(), Some("10m tea"));
    i.push(i.now, Input::Key(KeyInput::Up));
    assert_eq!(text(&i).as_deref(), Some("todo buy milk"));
    i.push(i.now, Input::Key(KeyInput::Up));
    assert_eq!(text(&i).as_deref(), Some("todo buy milk"), "the oldest stays");
    i.push(i.now, Input::Key(KeyInput::Down));
    i.push(i.now, Input::Key(KeyInput::Down));
    assert_eq!(text(&i).as_deref(), Some("12"), "back to what was being typed");
    assert_eq!(i.bar.as_ref().map(|b| b.cursor), Some(2));
}

fn commuter() -> Island {
    // T0 is Friday 08:00 UTC; lectures: Reti 09:30-11:00 today, a video call, and Reti again tomorrow at 09:30
    let mut i = Island::new(Config { commute_minutes: 60, travel_margin_ms: 0, ..Config::default() });
    let ev = |id: &str, at_min: i64, len: i64| Event { id: id.into(), title: id.into(), start_ms: T0 + at_min * 60_000, end_ms: T0 + (at_min + len) * 60_000, location: "Aula 3".into(), ..Default::default() };
    let call = Event { url: Some("https://meet.example/x".into()), ..ev("Call", 30, 30) };
    i.data.events = vec![call, ev("Reti", 90, 90), ev("Lab", 300, 120), ev("Reti domani", 24 * 60 + 90, 90)];
    i
}

#[test]
fn the_cards_say_when_to_leave_and_a_card_comes_at_that_moment() {
    let mut i = commuter();
    i.tick(T0, 8.0);
    let (_, body) = i.evening_card().expect("tomorrow has a lecture");
    assert!(body.starts_with("Leave 08:30 · 09:30 Reti domani"), "{body}");
    let (_, morning) = i.morning_card().expect("today has lectures");
    assert!(morning.starts_with("Leave 08:30 · "), "the video call is not a trip, Reti is: {morning}");
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
fn a_deadline_counts_down_on_today_and_in_the_cards_and_goes_away_by_itself() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0); // Friday 15 January 2027
    let plain = i.today_height();
    type_in_bar(&mut i, "esame 18 jan Algoritmi");
    assert!(matches!(i.bar_cmd(), Some(island_core::bar::Cmd::Deadline { text, .. }) if text == "Algoritmi"));
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert_eq!(card(&i).map(|c| c.1), Some("Algoritmi · in 3 days".to_string()));
    i.notif = None;
    type_in_bar(&mut i, "deadline 2027-03-01 Tesi");
    i.push(i.now, Input::Key(KeyInput::Enter));
    i.notif = None;
    assert_eq!(i.deadlines().iter().map(|d| d.text.as_str()).collect::<Vec<_>>(), ["Algoritmi", "Tesi"], "soonest first");
    assert_eq!(i.deadline_rows().len(), 1, "Tesi is six weeks away: not worth a row yet");
    assert_eq!(i.today_height(), plain + 22.0 + 28.0);
    let (head, body) = i.evening_card().expect("a deadline is something to say");
    assert_eq!(head, "Tomorrow · 1 deadline");
    assert_eq!(body, "Algoritmi in 3 days");
    i.activate(Action::DeadlineDone(0));
    assert_eq!(i.deadlines().len(), 1);
    // two days after, an old one is dropped; the far one stays
    i.restore_deadlines(vec![Deadline { day: i.day_number(T0) - 3, text: "old".into(), ..Default::default() }, Deadline { day: i.day_number(T0) - 1, text: "yesterday".into(), ..Default::default() }, Deadline { day: i.day_number(T0) + 40, text: "far".into(), ..Default::default() }]);
    i.tick(T0 + 10, 8.0);
    assert_eq!(i.deadlines().iter().map(|d| d.text.as_str()).collect::<Vec<_>>(), ["yesterday", "far"]);
}

#[test]
fn a_daily_todo_comes_back_tomorrow() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    type_in_bar(&mut i, "todo daily bring the charger");
    i.push(i.now, Input::Key(KeyInput::Enter));
    i.notif = None;
    i.push(i.now, Input::TodoAdd("call Anna".into()));
    assert_eq!(i.open_todos().len(), 2);
    i.activate(Action::TodoDone(0));
    assert_eq!(i.todos().len(), 2, "a daily one is not removed");
    assert_eq!(i.open_todos().iter().map(|(_, t)| t.text.as_str()).collect::<Vec<_>>(), ["call Anna"], "but it is out of the way today");
    assert_eq!(i.evening_card().map(|c| c.0), Some("Tomorrow · 1 to do".to_string()));
    i.tick(T0 + 24 * 3_600_000, 8.0);
    assert_eq!(i.open_todos().len(), 2, "tomorrow it is back");
    i.activate(Action::TodoDone(1));
    assert_eq!(i.todos().len(), 1, "an ordinary one is removed, whatever its position");
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
fn the_pinned_train_shows_on_today_and_a_late_or_cancelled_one_says_so_once() {
    use island_core::train::{TrainPin, TrainStatus};
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    let plain = i.today_height();
    i.data.train = Some(TrainPin { from_id: "S06500".into(), from_name: "Pisa Centrale".into(), time: "17:12".into(), number: "18567".into(), category: "REG".into(), to_name: "Lucca".into(), arrival: "17:52".into(), ..Default::default() });
    assert_eq!(i.today_height(), plain + 30.0, "a row for the train");
    i.data.train_status = Some(TrainStatus { delay_min: Some(4), cancelled: false, platform: Some("3".into()), at_ms: T0, next: vec![] });
    i.tick(T0 + 1000, 8.0);
    assert!(card(&i).is_none(), "four minutes is not news");
    i.data.train_status = Some(TrainStatus { delay_min: Some(12), ..i.data.train_status.clone().unwrap() });
    i.tick(T0 + 2000, 8.0);
    let (head, body) = card(&i).expect("a late train");
    assert_eq!(head, "REG 18567: 12 min late");
    assert_eq!(body, "17:12 Pisa Centrale → Lucca · track 3");
    assert!(i.pending.contains(&Command::PlaySound));
    i.notif = None;
    i.data.train_status = Some(TrainStatus { delay_min: Some(13), ..i.data.train_status.clone().unwrap() });
    i.tick(T0 + 3000, 8.0);
    assert!(card(&i).is_none(), "the same news is not said again");
    i.data.train_status = Some(TrainStatus { cancelled: true, ..i.data.train_status.clone().unwrap() });
    i.tick(T0 + 4000, 8.0);
    assert_eq!(card(&i).map(|c| c.0), Some("REG 18567: Cancelled".to_string()));
}

#[test]
fn the_bar_pins_and_unpins_the_train_and_sets_home() {
    let mut i = Island::new(Config::default());
    type_in_bar(&mut i, "treno pisa centrale 17:12 verso lucca");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.pending.contains(&Command::PinTrain("pisa centrale 17:12 verso lucca".into())));
    i.notif = None;
    type_in_bar(&mut i, "home 43.7085, 10.4036");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.pending.contains(&Command::SetHome("43.7085, 10.4036".into())));
    i.data.train = Some(island_core::train::TrainPin { from_id: "S1".into(), number: "1".into(), time: "17:12".into(), ..Default::default() });
    i.notif = None;
    type_in_bar(&mut i, "train off");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.data.train.is_none() && i.pending.contains(&Command::UnpinTrain));
}

#[test]
fn home_and_the_train_are_set_from_the_settings_page() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    i.start_edit(Field::Home);
    for c in "43.7085, 10.4036".chars() {
        i.push(i.now, Input::Key(KeyInput::Text(c.to_string())));
    }
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.pending.contains(&Command::SetHome("43.7085, 10.4036".into())));
    i.notif = None;
    i.start_edit(Field::Train);
    for c in "pisa centrale".chars() {
        i.push(i.now, Input::Key(KeyInput::Text(c.to_string())));
    }
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(!i.pending.iter().any(|c| matches!(c, Command::PinTrain(_))), "no time: not accepted");
    assert!(i.edit.is_some(), "the field stays open with what was typed, and says why");
    for c in " 17:12 verso lucca".chars() {
        i.push(i.now, Input::Key(KeyInput::Text(c.to_string())));
    }
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.pending.contains(&Command::PinTrain("pisa centrale 17:12 verso lucca".into())));
    // editing a pinned train starts from the line that would pin it again; an empty line unpins it
    i.data.train = Some(island_core::train::TrainPin { from_id: "S1".into(), from_name: "Pisa Centrale".into(), to_name: "Lucca".into(), number: "1".into(), time: "17:12".into(), ..Default::default() });
    i.start_edit(Field::Train);
    assert_eq!(i.edit.as_ref().map(|e| e.text.clone()).as_deref(), Some("Pisa Centrale 17:12 verso Lucca"));
    for _ in 0..40 {
        i.push(i.now, Input::Key(KeyInput::Backspace));
    }
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.data.train.is_none() && i.pending.contains(&Command::UnpinTrain));
    i.start_edit(Field::Home);
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.pending.contains(&Command::SetHome(String::new())), "empty removes home");
}

#[test]
fn every_option_is_on_exactly_one_tab_and_no_tab_is_crowded() {
    use crate::options::*;
    let mut seen = std::collections::BTreeSet::new();
    for (tab, name) in TABS.iter().enumerate() {
        let keys: Vec<&str> = toggles(tab).iter().map(|t| t.0).chain(steppers(tab).iter().map(|s| s.0)).chain(text_keys(tab).iter().copied()).collect();
        for k in &keys {
            assert!(seen.insert(*k), "{k} is on two tabs");
        }
        if tab != RULES_TAB {
            assert!(!keys.is_empty() && keys.len() <= 12, "{name}: {} options", keys.len());
            assert!(!tab_blurb(tab).is_empty(), "{name} has no blurb");
        }
    }
    // the tabs fit the page: names of at most six letters
    assert!(TABS.iter().all(|t| t.len() <= 6));
}

#[test]
fn the_map_draws_with_home_inside_and_outside_the_view() {
    let f = fonts();
    for home in [Some((43.7255, 10.3908)), Some((48.85, 2.35)), None] {
        let mut i = Island::new(Config::default());
        i.data = data();
        i.data.config.home_pos = home;
        i.data.position = Some(Position { lat: 43.7255, lon: 10.3908, accuracy_m: 100.0, at_ms: T0, source: PosSource::Geoclue });
        i.push(T0, Input::OpenPage(Page::Map));
        run(&mut i, 600);
        let fr = i.tick(i.now + 8, 8.0);
        let mut scene = Scene::new();
        i.draw(&mut scene, &f, fr.surface, 1.0);
    }
}

#[test]
fn the_home_pin_gives_the_way_home() {
    let mut i = Island::new(Config::default());
    i.data.config.home_pos = Some((43.9643, 10.2215));
    i.data.config.home = "Via Roma 1, Lucca".into();
    // without a position there is no way to give: it says so
    i.push(T0, Input::OpenPage(Page::Map));
    run(&mut i, 400);
    i.activate(Action::MapHome);
    assert_eq!(card(&i).map(|c| c.0), Some("No position yet".to_string()));
    i.notif = None;
    i.data.position = Some(Position { lat: 43.7255, lon: 10.3908, accuracy_m: 100.0, at_ms: T0, source: PosSource::Geoclue });
    let cmds = i.activate(Action::MapHome);
    assert!(matches!(&cmds[..], [Command::PlanRoute { dest, from, .. }] if (dest.lat, dest.lon) == (43.9643, 10.2215) && dest.name == "Via Roma 1, Lucca" && *from == (43.7255, 10.3908)), "{cmds:?}");
    assert!(i.map_dest.is_some() && i.data.route_pending, "the destination is home and the route is on its way");
    i.activate(Action::MapClearDest);
    assert!(i.map_dest.is_none());
    // no home set: the pin is not there to click
    let mut j = Island::new(Config::default());
    j.data.position = i.data.position.clone();
    assert!(j.activate(Action::MapHome).is_empty());
}

#[test]
fn the_home_pin_is_a_button_on_the_map() {
    let mut i = Island::new(Config::default());
    i.data.config.home_pos = Some((43.7285, 10.3935));
    i.data.position = Some(Position { lat: 43.7255, lon: 10.3908, accuracy_m: 100.0, at_ms: T0, source: PosSource::Geoclue });
    i.push(T0, Input::OpenPage(Page::Map));
    run(&mut i, 500);
    draw_frames(&mut i, 100);
    assert!(i.hits.iter().any(|h| h.action == Action::MapHome), "the pin has a hit area");
}

#[test]
fn the_train_is_one_of_the_ways_on_the_map_and_on_the_trip_row() {
    use island_core::travel::Mode;
    let mut i = Island::new(Config::default());
    i.data.position = Some(Position { lat: 43.7255, lon: 10.3908, accuracy_m: 100.0, at_ms: T0, source: PosSource::Geoclue });
    i.data.config.home_pos = Some((43.9643, 10.2215));
    i.push(T0, Input::OpenPage(Page::Map));
    run(&mut i, 500);
    i.activate(Action::MapHome);
    draw_frames(&mut i, 100);
    assert!(i.hits.iter().any(|h| h.action == Action::TravelMode(Mode::Train)), "a button for the train beside car, foot and transit");
    let cmds = i.activate(Action::TravelMode(Mode::Train));
    assert_eq!(i.cfg.travel_mode, Mode::Train);
    assert!(cmds.iter().any(|c| matches!(c, Command::PlanRoute { mode: Mode::Train, .. })), "the way home is asked again by train: {cmds:?}");
}

#[test]
fn a_late_train_shows_what_leaves_next_to_the_same_place() {
    use island_core::train::{NextTrain, TrainPin, TrainStatus};
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    i.data.train = Some(TrainPin { from_id: "S1".into(), number: "1".into(), time: "17:12".into(), headsign: "LUCCA".into(), ..Default::default() });
    let next = vec![NextTrain { time: "17:42".into(), label: "REG 4177".into(), delay_min: None }];
    i.data.train_status = Some(TrainStatus { delay_min: Some(0), next: next.clone(), ..Default::default() });
    let on_time = i.today_height();
    assert_eq!(i.train_next_line(), None, "on time: nothing to say about alternatives");
    i.data.train_status = Some(TrainStatus { delay_min: Some(8), next: next.clone(), ..Default::default() });
    assert_eq!(i.train_next_line().as_deref(), Some("17:42 REG 4177"));
    assert_eq!(i.today_height(), on_time + 16.0, "a line more");
    i.data.train_status = Some(TrainStatus { cancelled: true, next, ..Default::default() });
    assert!(i.train_next_line().is_some(), "cancelled: the same");
    i.data.train_status = Some(TrainStatus { delay_min: Some(8), next: vec![], ..Default::default() });
    assert_eq!(i.train_next_line(), None, "no alternative known: no line");
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

fn message(id: u32, app: &str, from: &str, text: &str) -> Incoming {
    Incoming { id, app: app.into(), summary: from.into(), body: text.into(), ..Default::default() }
}

#[test]
fn messages_of_one_conversation_join_the_card_on_screen_instead_of_queueing() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Notification(message(1, "Telegram", "Anna", "ciao")));
    i.push(i.now, Input::Notification(message(2, "Telegram", "Anna", "ci sei?")));
    i.push(i.now, Input::Notification(message(3, "Telegram", "Anna", "dove sei")));
    let n = i.notif.as_ref().expect("one card");
    assert_eq!((n.id, n.body.as_str()), (3, "dove sei"), "the latest words are on the card");
    assert_eq!(i.notif_group, 3, "and it stands for three messages");
    assert!(i.queue.is_empty() && i.waiting.is_empty(), "nothing waits for its turn");
    let closed: Vec<u32> = i.pending.iter().filter_map(|c| if let Command::NotificationClosed { id, .. } = c { Some(*id) } else { None }).collect();
    assert_eq!(closed, [1, 2], "the earlier ones are closed towards their sender");
    // someone else in the same app has a card of their own, and does not change the count
    i.push(i.now, Input::Notification(message(4, "Telegram", "Marco", "hey")));
    assert_eq!((i.notif.as_ref().map(|n| n.id), i.below.len(), i.notif_group), (Some(3), 1, 3));
    // a critical message of the same conversation is not folded away
    i.push(i.now, Input::Notification(Incoming { critical: true, ..message(5, "Telegram", "Anna", "urgente") }));
    assert_eq!((i.notif.as_ref().map(|n| n.id), i.below.iter().map(|b| b.n.id).collect::<Vec<_>>()), (Some(3), vec![4, 5]), "it has a card like any other, it is not merged");
    // a new card starts again from one
    i.close_notif(CloseReason::Dismissed);
    i.tick(i.now + 100, 8.0);
    i.push(i.now, Input::Notification(message(6, "Signal", "Luca", "x")));
    assert!(i.notif_group >= 1);
}

#[test]
fn the_history_page_shows_a_conversation_as_one_row_and_a_click_takes_all_of_it() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    for (id, from, text) in [(1, "Anna", "uno"), (2, "Anna", "due"), (3, "Anna", "tre"), (4, "Marco", "ciao"), (5, "Anna", "ancora")] {
        i.push(i.now, Input::Notification(message(id, "Telegram", from, text)));
        i.notif = None;
        i.tick(i.now + 1000, 8.0);
    }
    // newest first: Anna (ancora), Marco, Anna x3
    assert_eq!(i.history_groups(), vec![(0, 1), (1, 1), (2, 3)]);
    assert_eq!(i.history.len(), 5);
    i.activate(Action::HistoryRemove(2));
    assert_eq!(i.history.len(), 2, "the three messages of Anna that were together go with one click");
    i.activate(Action::HistoryOpen(1));
    assert_eq!(i.history.len(), 1);
    assert!(i.pending.iter().any(|c| matches!(c, Command::FocusApp { app, .. } if app == "Telegram")), "and the app is opened");
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
fn pinned_notes_stay_on_today_a_click_copies_them_and_unpin_takes_them_off() {
    let mut i = Island::new(Config::default());
    let plain = i.today_height();
    i.push(T0, Input::PinAdd("  ssh   10.0.0.5 ".into()));
    i.push(T0, Input::PinAdd("Aula B3".into()));
    i.push(T0, Input::PinAdd("   ".into()));
    i.push(T0, Input::PinAdd("ssh 10.0.0.5".into()));
    assert_eq!(i.pins(), ["ssh 10.0.0.5", "Aula B3"], "spaces squeezed, the same text moves up instead of repeating");
    assert_eq!(i.today_height(), plain + 22.0 + 2.0 * 28.0, "a heading and a row each");
    let cmds = i.activate(Action::PinCopy(1));
    assert!(matches!(cmds.as_slice(), [Command::RestoreClipboard(t)] if t == "Aula B3"));
    assert!(i.activate(Action::PinCopy(9)).is_empty(), "a stale position does nothing");
    i.activate(Action::PinRemove(0));
    assert_eq!(i.pins(), ["Aula B3"]);
    i.push(i.now, Input::PinAdd("one".into()));
    i.push(i.now, Input::PinRemove("AULA".into()));
    assert_eq!(i.pins(), ["one"], "by a part of the text, any case");
    i.push(i.now, Input::PinRemove(String::new()));
    assert!(i.pins().is_empty(), "no words: the newest");
    for n in 0..12 {
        i.push(i.now, Input::PinAdd(format!("p{n}")));
    }
    assert_eq!(i.pins().len(), 8, "eight are kept");
    i.push(i.now, Input::PinRemove("all".into()));
    assert!(i.pins().is_empty());
    assert_eq!(i.today_height(), plain);
}

#[test]
fn the_command_bar_pins_and_unpins() {
    let mut i = Island::new(Config::default());
    i.run_bar(island_core::bar::Cmd::Pin("ssh pi@10.0.0.9".into()));
    assert_eq!(i.pins(), ["ssh pi@10.0.0.9"]);
    i.run_bar(island_core::bar::Cmd::Unpin("pi@".into()));
    assert!(i.pins().is_empty());
}

#[test]
fn several_small_pills_sit_beside_the_main_one_up_to_the_option() {
    let act = |id: &str, label: &str| island_core::progress::Activity { id: id.into(), label: label.into(), detail: "thinking".into(), ..Default::default() };
    let build = |pills: u32| {
        let mut i = Island::new(Config { mini_pills: pills, ..Config::default() });
        i.data = data(); // music is playing
        i.data.focus = Some(Focus { active: true, end_epoch: None });
        i.push(T0, Input::StartTimer(600));
        i.push(T0, Input::Activity(island_core::progress::State::Run, act("a1", "Claude · site")));
        i.push(T0, Input::Activity(island_core::progress::State::Run, act("a2", "Codex · api")));
        i.push(T0, Input::Activity(island_core::progress::State::Run, act("a3", "Aider · docs")));
        run(&mut i, 5000);
        i
    };
    let i = build(1);
    assert_eq!((i.current_face(), i.sides_shown()), (Face::MediaMini, vec![Side::Timer]), "one: as it always was");
    let i = build(2);
    assert_eq!(i.sides_shown(), [Side::Timer, Side::Focus]);
    let i = build(4);
    assert_eq!(i.sides_shown(), [Side::Timer, Side::Focus, Side::Op, Side::OpAt(0)]);
    let (px, _, pw, _) = i.pill_rect();
    assert!(pw > i.spring.w.pos + 3.0 * BUBBLE_W, "the pill rect holds all the bubbles");
    // each one opens its own page
    let mut i = i;
    let fr = i.tick(i.now + 8, 8.0);
    i.draw(&mut Scene::new(), &fonts(), fr.surface, 1.0);
    let hits: Vec<Action> = i.hits.iter().map(|h| h.action.clone()).collect();
    assert_eq!(hits.iter().filter(|a| matches!(a, Action::Open(Page::Timer | Page::Focus | Page::Op))).count(), 4, "{hits:?}");
    let _ = px;
    // fewer room again: the extra ones go
    let mut i = build(4);
    i.cfg.mini_pills = 1;
    run(&mut i, 600);
    assert_eq!(i.sides_shown(), [Side::Timer]);
}

#[test]
fn the_phone_page_lists_conversations_opens_one_and_sends_a_reply() {
    use island_core::phone::{Link, Msg, Thread};
    let mut i = Island::new(Config::default());
    assert!(!i.pages().contains(&Page::Phone), "no phone, no page");
    let th = |name: &str, key: &str| Thread { name: name.into(), key: key.into(), when: "2026-10-26 09:23".into(), preview: "Ciao".into() };
    i.data.phone = Some(Phone { link: Link { present: true, messages: true, battery: Some(80), ..Default::default() }, threads: vec![th("Enrica", "tel:+39347"), th("iliad", "sender:iliad")], messages: None, at_ms: T0, ..Default::default() });
    assert!(i.pages().contains(&Page::Phone));
    i.push(T0, Input::OpenPage(Page::Phone));
    draw_frames(&mut i, 200);
    assert_eq!(i.current_face(), Face::Phone);
    let cmds = i.activate(Action::PhoneThread(1));
    assert_eq!(cmds, vec![Command::PhoneOpen("sender:iliad".into())]);
    assert_eq!(i.phone_open.as_deref(), Some("sender:iliad"));
    // the messages arrive; the page draws them and a reply is typed and sent
    i.data.phone.as_mut().unwrap().messages = Some(("sender:iliad".into(), vec![Msg { when: "2026-10-26 09:23".into(), mine: false, text: "Ciao".into() }]));
    draw_frames(&mut i, 50);
    // through the Reply button, as a click does, and the field must survive the frames that follow
    i.activate(Action::EditField(Field::PhoneReply));
    draw_frames(&mut i, 50);
    assert!(i.edit.as_ref().is_some_and(|e| e.field == Field::PhoneReply), "the reply field stays open on the Phone page");
    i.push(i.now, Input::Key(KeyInput::Text("va bene".into())));
    i.push(i.now, Input::Key(KeyInput::Enter));
    let sent: Vec<Command> = i.take_commands();
    assert!(sent.contains(&Command::PhoneSend { key: "sender:iliad".into(), text: "va bene".into() }), "{sent:?}");
    i.activate(Action::PhoneList);
    assert!(i.phone_open.is_none());
    // the phone goes away: the page is not offered any more
    i.data.phone = None;
    assert!(!i.pages().contains(&Page::Phone));
}

#[test]
fn the_dock_keeps_what_is_live_and_what_is_used_and_the_grid_has_the_rest() {
    let mut i = Island::new(Config::default());
    let default = i.dock_pages();
    assert_eq!(default.len(), 7, "Home and six: the room the option gives");
    assert!(!default.contains(&Page::Options) && !default.contains(&Page::Clip), "unused pages wait on the grid");
    // pages that get opened move into the dock
    i.page_uses.insert(Page::Clip, 9);
    i.page_uses.insert(Page::Options, 4);
    let d = i.dock_pages();
    assert!(d.contains(&Page::Clip) && d.contains(&Page::Options) && d.len() == 7, "{d:?}");
    assert_eq!(d[0], Page::Home);
    // something live comes first whatever the history says
    i.data.phone = Some(Phone { link: island_core::phone::Link { present: true, ..Default::default() }, ..Default::default() });
    i.push(T0, Input::StartTimer(60));
    assert!(i.dock_pages().contains(&Page::Timer));
    // the page on screen is always there
    i.page = Some(Page::Weather);
    i.data.weather = None;
    let mut j = Island::new(Config { dock_icons: 4, ..Config::default() });
    j.page = Some(Page::Sys);
    assert!(j.dock_pages().contains(&Page::Sys) && j.dock_pages().len() == 4);
}

#[test]
fn the_grid_lists_every_page_in_groups_and_a_click_opens_one() {
    let mut i = Island::new(Config::default());
    let groups = i.grid_groups();
    let names: Vec<&str> = groups.iter().map(|g| g.0).collect();
    assert_eq!(names, ["Day", "Messages", "Device", "Island"], "no live page, no Now group");
    let all: Vec<Page> = groups.iter().flat_map(|g| g.1.clone()).collect();
    assert!(all.contains(&Page::Options) && all.contains(&Page::Audio) && !all.contains(&Page::Grid) && !all.contains(&Page::Phone), "{all:?}");
    i.push(T0, Input::OpenPage(Page::Grid));
    draw_frames(&mut i, 200);
    assert_eq!(i.current_face(), Face::Grid);
    let opens: Vec<Page> = i.hits.iter().filter_map(|h| if let Action::Open(p) = h.action { Some(p) } else { None }).collect();
    assert_eq!(opens.len(), all.len(), "one cell for each page");
    // the bar: `go phone` opens a page, `go all` the grid
    i.run_bar(island_core::bar::Cmd::Go(Page::Cal));
    draw_frames(&mut i, 200);
    assert_eq!(i.current_face(), Face::Cal);
}

#[test]
fn the_phone_page_answers_calls_searches_contacts_and_starts_a_new_conversation() {
    use island_core::phone::{Call, Contact, Link};
    let mut i = Island::new(Config::default());
    i.data.phone = Some(Phone { link: Link { present: true, messages: true, calls: false, ..Default::default() }, ..Default::default() });
    i.push(T0, Input::OpenPage(Page::Phone));
    draw_frames(&mut i, 200);
    let off = i.phone_height();
    // calls are off: the page offers to turn them on
    assert!(i.hits.iter().any(|h| h.action == Action::PhoneCallDo(PhoneCall::Enable(true))));
    assert_eq!(i.activate(Action::PhoneCallDo(PhoneCall::Enable(true))), vec![Command::PhoneCall(PhoneCall::Enable(true))]);
    // a ringing call shows with the two buttons
    i.data.phone = Some(Phone { link: Link { present: true, messages: true, calls: true, ..Default::default() }, calls: vec![Call { state: "ringing".into(), number: "+39347".into(), name: "Enrica".into() }], ..Default::default() });
    draw_frames(&mut i, 200);
    assert!(i.hits.iter().any(|h| h.action == Action::PhoneCallDo(PhoneCall::Answer)) && i.hits.iter().any(|h| h.action == Action::PhoneCallDo(PhoneCall::Hangup)));
    assert!(i.phone_height() > off - 30.0 + 40.0, "room for the call row");
    // a search for a contact: typed, sent as a command, the result opens a conversation with the number
    i.activate(Action::EditField(Field::PhoneSearch));
    draw_frames(&mut i, 20);
    i.push(i.now, Input::Key(KeyInput::Text("enri".into())));
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.take_commands().contains(&Command::PhoneSearch("enri".into())));
    assert!(i.phone_search);
    i.data.phone.as_mut().unwrap().contacts = Some(vec![Contact { name: "Enrica Rossi".into(), numbers: vec!["+39 347 123 4567".into()] }]);
    draw_frames(&mut i, 100);
    assert!(i.hits.iter().any(|h| h.action == Action::PhoneContact(0)));
    let cmds = i.activate(Action::PhoneContact(0));
    assert_eq!(cmds, vec![Command::PhoneOpen("tel:+393471234567".into())]);
    assert!(!i.phone_search && i.phone_open.as_deref() == Some("tel:+393471234567"));
    // in the conversation, a call button when calls are on
    draw_frames(&mut i, 100);
    assert!(i.hits.iter().any(|h| h.action == Action::PhoneCallDo(PhoneCall::Dial("+393471234567".into()))));
}

#[test]
fn a_screenshot_card_offers_the_text_in_the_picture() {
    let shot = |id: u32| Incoming { id, app: "niri".into(), summary: "Screenshot captured".into(), body: "You can paste the image from the clipboard.".into(), ..Default::default() };
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Notification(shot(7)));
    let n = i.notif.clone().expect("a card");
    assert!(n.app == "open-island" && n.actions.iter().any(|a| a.key == "open-island:ocr"), "{:?} {:?}", n.app, n.actions.len());
    assert_eq!(n.summary, "Screenshot captured", "it still says what happened");
    assert!(i.history.iter().any(|(h, _)| h.app == "niri"), "the original stays in the history");
    // its button reads the image, and the card goes
    let cmds = i.activate(Action::NotifAction("open-island:ocr".into()));
    assert_eq!(cmds, vec![Command::Ocr]);
    assert!(i.notif.is_none());
    // switched off: the card is the compositor's own
    let mut j = Island::new(Config { ocr_screenshots: false, ..Config::default() });
    j.push(T0, Input::Notification(shot(8)));
    assert_eq!(j.notif.as_ref().map(|n| n.app.as_str()), Some("niri"));
    // other notifications are not touched, and quiet time keeps the card back as it does for any
    let mut k = Island::new(Config::default());
    k.push(T0, Input::Notification(Incoming { id: 9, app: "Telegram".into(), summary: "Enrica".into(), ..Default::default() }));
    assert_eq!(k.notif.as_ref().map(|n| n.app.as_str()), Some("Telegram"));
}

#[test]
fn the_phone_page_chooses_where_the_phones_audio_plays() {
    use island_core::phone::Link;
    let mut i = Island::new(Config::default());
    i.data.phone = Some(Phone { link: Link { present: true, messages: true, calls: true, ..Default::default() }, audio_here: Some(false), ..Default::default() });
    let without = { let mut j = Island::new(Config::default()); j.data.phone = Some(Phone { link: Link { present: true, messages: true, calls: true, ..Default::default() }, ..Default::default() }); j.phone_height() };
    assert_eq!(i.phone_height(), without + 30.0, "a row for the choice");
    i.push(T0, Input::OpenPage(Page::Phone));
    draw_frames(&mut i, 200);
    assert!(i.hits.iter().any(|h| h.action == Action::PhoneAudioDo(true)), "it offers to play it here");
    assert_eq!(i.activate(Action::PhoneAudioDo(true)), vec![Command::PhoneAudio(true)]);
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

fn on_battery(i: &mut Island, percent: f32) {
    i.data.battery = Some(Battery { percent, charging: false, on_ac: false });
}

#[test]
fn the_drain_is_learned_while_on_battery_and_forgotten_when_plugged_in() {
    let mut i = Island::new(Config::default());
    on_battery(&mut i, 80.0);
    i.sample_battery(T0);
    assert_eq!(i.drain_rate(), None, "one sample says nothing");
    on_battery(&mut i, 76.0);
    i.sample_battery(T0 + 20 * 60_000);
    assert_eq!(i.drain_rate().map(|r| r.round()), Some(12.0), "4 points in 20 minutes");
    // plugged in: the samples are dropped, what was learned stays
    i.data.battery = Some(Battery { percent: 77.0, charging: true, on_ac: true });
    i.sample_battery(T0 + 25 * 60_000);
    assert!(i.battery_log.is_empty() && i.drain_live.is_none());
    assert_eq!(i.drain_rate().map(|r| r.round()), Some(12.0));
    i.restore_drain(Some(f64::NAN));
    assert_eq!(i.drain_saved(), None, "a bad kept value is ignored");
}

#[test]
fn the_day_says_whether_the_battery_will_do_and_the_card_comes_once() {
    // T0 = Friday 08:00 UTC; lectures at 09:00 and 15:00 (an hour each), a 60 minute way home: back at 17:00
    let mut i = Island::new(Config { commute_minutes: 60, battery_alerts: true, ..Config::default() });
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
    let mut j = Island::new(Config { commute_minutes: 60, ..Config::default() });
    j.data.events = i.data.events.clone();
    j.restore_drain(Some(3.0));
    on_battery(&mut j, 90.0);
    j.tick(T0 + 3_600_000, 8.0);
    assert!(j.power_line(false, &j.today_left()).is_none() && j.notif.is_none());
    // nothing learned yet: nothing said
    let mut k = Island::new(Config::default());
    k.data.events = i.data.events.clone();
    on_battery(&mut k, 10.0);
    k.tick(T0 + 3_600_000, 8.0);
    assert!(k.power_today().is_none());
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

// ---- the organizer

fn run_bar(i: &mut Island, line: &str) {
    type_in_bar(i, line);
    i.push(i.now, Input::Key(KeyInput::Enter));
    i.notif = None;
}

#[test]
fn a_task_keeps_its_day_course_and_estimate_and_the_late_come_first() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    run_bar(&mut i, "todo someday maybe");
    run_bar(&mut i, "todo Report due fri #algoritmi ~2h");
    run_bar(&mut i, "todo Old thing due yesterday");
    let open = i.open_todos();
    assert_eq!(open.iter().map(|(_, t)| t.text.as_str()).collect::<Vec<_>>(), ["Old thing", "Report", "someday maybe"], "late first, then by day, then those without one");
    let report = open[1].1;
    assert_eq!((report.course.as_str(), report.est_min), ("algoritmi", 120));
    assert_eq!(report.due, Some(i.today_day() + 7), "the next Friday after Friday is a week away");
    assert!(!report.uid.is_empty());
    assert!(i.take_commands().iter().any(|c| matches!(c, Command::PushTask { task, done: false } if task.text == "Report")), "the server that keeps tasks is told");
}

#[test]
fn ticking_a_weekly_task_rests_it_until_its_day() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0); // a Friday
    run_bar(&mut i, "todo gym every mon and thu");
    assert_eq!(i.open_todos().len(), 0, "the next one is Monday, so it is not for today");
    i.push(T0 + 3 * 86_400_000, Input::OpenBar);
    i.tick(T0 + 3 * 86_400_000, 8.0); // Monday
    assert_eq!(i.open_todos().len(), 1);
    i.take_commands();
    i.activate(Action::TodoDone(0));
    assert_eq!(i.todos().len(), 1, "it stays");
    assert_eq!(i.todos()[0].due, Some(i.today_day() + 3), "the next day is Thursday");
    assert_eq!(i.open_todos().len(), 0);
    assert!(i.take_commands().iter().any(|c| matches!(c, Command::PushTask { done: false, .. })), "moved, not finished");
}

#[test]
fn finishing_a_task_tells_the_server_and_it_does_not_come_back_from_a_late_answer() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    run_bar(&mut i, "todo Pay the fee");
    let uid = i.todos()[0].uid.clone();
    i.take_commands();
    i.activate(Action::TodoDone(0));
    assert!(i.take_commands().iter().any(|c| matches!(c, Command::PushTask { done: true, .. })));
    // the server still shows it open (our update had not arrived when it answered)
    let stale = Todo { uid: uid.clone(), text: "Pay the fee".into(), synced: true, ..Default::default() };
    i.push(i.now, Input::TasksRemote(vec![(stale, false)]));
    assert!(i.todos().is_empty(), "it stays finished");
}

#[test]
fn the_server_brings_tasks_in_updates_them_and_takes_finished_ones_out() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    let t = |uid: &str, text: &str| Todo { uid: uid.into(), text: text.into(), synced: true, ..Default::default() };
    i.push(i.now, Input::TasksRemote(vec![(t("a", "From the phone"), false), (t("b", "Other"), false)]));
    assert_eq!(i.todos().len(), 2);
    run_bar(&mut i, "todo Only here");
    i.push(i.now, Input::TasksRemote(vec![(Todo { due: Some(5), ..t("a", "Renamed on the phone") }, false), (t("b", "Other"), true)]));
    let texts: Vec<&str> = i.todos().iter().map(|t| t.text.as_str()).collect();
    assert_eq!(texts, ["Renamed on the phone", "Only here"], "b was finished there; the one made here and not yet sent is kept");
    assert_eq!(i.todos()[0].due, Some(5));
    i.take_commands();
    i.push(i.now, Input::TasksRemote(vec![]));
    assert_eq!(i.todos().iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), ["Only here"], "one the server no longer has was deleted there");
    assert!(i.take_commands().iter().any(|c| matches!(c, Command::PushTask { task, done: false } if task.text == "Only here")), "and the one it has never seen is sent");
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
fn studying_takes_time_off_the_work_and_the_plan_spreads_what_is_left() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    run_bar(&mut i, "esame 25 jan Algoritmi #algoritmi ~10h");
    let today = i.today_day();
    assert_eq!(i.deadlines()[0].est_min, 600);
    assert_eq!(i.study_on(today), vec![("Algoritmi".to_string(), 60)], "ten days left: an hour a day");
    run_bar(&mut i, "log 90m algoritmi");
    assert_eq!(i.deadlines()[0].est_min, 510);
    assert_eq!(i.study_log(), [(today, "algoritmi".to_string(), 90)]);
    assert_eq!(i.studied_this_week(), vec![("algoritmi".to_string(), 90)]);
    assert!(i.study_line(today).is_some_and(|l| l.starts_with("Study 55m")), "{:?}", i.study_line(today));
    let (title, body) = i.agenda_card("Today", &[]).expect("the plan alone is worth a card");
    assert!(title.starts_with("Today") && body.contains("Study"), "{title} · {body}");
    run_bar(&mut i, "log 30m chemistry");
    assert_eq!(i.deadlines()[0].est_min, 510, "a course with no work leaves it alone");
}

#[test]
fn a_pomodoro_for_a_course_logs_its_focus_time() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    run_bar(&mut i, "pomodoro Reti");
    assert_eq!(i.timer.as_ref().map(|t| t.label.as_str()), Some("Reti"));
    i.tick(T0 + island_core::pomodoro::FOCUS as i64 * 1000 + 100, 8.0);
    assert_eq!(i.study_log().len(), 1);
    assert_eq!((i.study_log()[0].1.as_str(), i.study_log()[0].2), ("Reti", (island_core::pomodoro::FOCUS / 60) as u32));
}

#[test]
fn a_search_shows_what_the_island_knows_then_what_the_notes_and_files_add() {
    let mut i = Island::new(Config::default());
    i.data.events = vec![Event { id: "l".into(), title: "Compilation Techniques".into(), start_ms: T0 + 3_600_000, end_ms: T0 + 7_200_000, ..Default::default() }];
    i.tick(T0, 8.0);
    run_bar(&mut i, "todo Read the compilers book");
    i.pin("compilers slides link");
    i.take_commands();
    run_bar(&mut i, "find compil");
    assert!(i.take_commands().iter().any(|c| matches!(c, Command::Find(q) if q == "compil")));
    assert_eq!(i.found.hits.len(), 3, "the event, the task and the pin");
    assert!(i.found.searching);
    let file = island_core::plan::Hit { kind: island_core::plan::HitKind::Note, title: "compilers: ask about LLVM".into(), detail: "2027-01-10.md".into(), target: "/n/2027-01-10.md".into() };
    i.push(i.now, Input::FoundMore { query: "other".into(), hits: vec![file.clone()] });
    assert_eq!(i.found.hits.len(), 3, "the answer to an old question is dropped");
    i.push(i.now, Input::FoundMore { query: "compil".into(), hits: vec![file] });
    assert_eq!((i.found.hits.len(), i.found.searching), (4, false));
    i.take_commands();
    i.open_hit(3);
    assert!(i.take_commands().iter().any(|c| matches!(c, Command::OpenFile(p) if p == "/n/2027-01-10.md")));
}

#[test]
fn a_note_goes_to_the_notes_folder() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    run_bar(&mut i, "note ask about the exam format");
    assert!(i.take_commands().iter().any(|c| matches!(c, Command::SaveNote(t) if t == "ask about the exam format")));
}

#[test]
fn the_plan_and_find_pages_draw_and_the_plan_flags_a_heavy_day() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    i.data = data();
    i.tick(T0, 8.0);
    let h = 3_600_000;
    // tomorrow: seven hours of lectures
    i.data.events = (0..4).map(|k| Event { id: format!("e{k}"), title: format!("Lecture {k}"), start_ms: T0 + 20 * h + k * 2 * h, end_ms: T0 + 20 * h + k * 2 * h + 3_600_000 * 2 - 600_000, ..Default::default() }).collect();
    run_bar(&mut i, "esame 25 jan Algoritmi ~10h");
    let week = i.week_loads();
    assert_eq!(week.len(), 7);
    assert!(week[1].heavy(), "{:?}", week[1]);
    run_bar(&mut i, "log 45m");
    run_bar(&mut i, "find lecture");
    for page in [Page::Plan, Page::Find] {
        for _ in 0..120 {
            let t = i.now + 8;
            let fr = i.tick(t, 8.0);
            i.push(t, Input::OpenPage(page));
            let mut scene = Scene::new();
            i.draw(&mut scene, &f, fr.surface, 1.0);
            assert!(fr.surface.1.is_finite() && fr.surface.1 < 900.0, "{page:?}");
        }
    }
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
fn a_line_from_outside_runs_like_one_typed_in_the_bar() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    i.push(i.now, Input::RunLine("todo Report due fri ~2h".into()));
    assert!(i.todos.iter().any(|t| t.text == "Report" && t.est_min == 120));
    i.notif = None;
    i.push(i.now, Input::RunLine("blorp zzz".into()));
    assert!(i.notif.take().is_some_and(|n| n.summary == "Not understood"));
}

#[test]
fn a_task_is_moved_edited_and_removed_and_the_server_is_told() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    run_bar(&mut i, "todo Report due fri ~2h");
    i.take_commands();
    run_bar(&mut i, "move report to sat");
    let sat = i.todos[0].due.unwrap();
    assert!(matches!(i.take_commands().as_slice(), [Command::PushTask { done: false, .. }]));
    run_bar(&mut i, "edit report -> due mon ~3h #algoritmi");
    assert!(i.todos[0].due.unwrap() > sat && i.todos[0].est_min == 180 && i.todos[0].course == "algoritmi" && i.todos[0].text == "Report", "only what was named changes");
    i.take_commands();
    run_bar(&mut i, "rm report");
    assert!(i.todos.is_empty());
    let uid = i.tasks_done.last().cloned().unwrap();
    assert!(matches!(i.take_commands().as_slice(), [Command::DeleteTask { uid: u }] if *u == uid));
    i.merge_tasks(vec![(Todo { uid, text: "Report".into(), synced: true, ..Default::default() }, false)]);
    assert!(i.todos.is_empty(), "a late answer from the server does not bring it back");
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
fn a_heavy_day_ahead_is_named_with_a_lighter_day_to_do_the_work_on() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    let today = i.today_day();
    assert!(i.heavy_lines(today).is_empty(), "an empty week is not heavy");
    for t in ["todo Report due in3 ~2h", "todo Lab sheet due in3", "todo Slides due in3"] {
        let t = t.replace("in3", "fri");
        run_bar(&mut i, &t);
    }
    // whatever weekday today is, put the three on one day two days out
    let due = today + 2;
    for t in i.todos.iter_mut() {
        t.due = Some(due);
    }
    let lines = i.heavy_lines(today);
    assert!(lines[0].contains("is heavy") && lines[0].contains("3 due"), "{lines:?}");
    assert!(lines.get(1).is_some_and(|l| l.contains("lighter") && l.contains("Report")), "the one with the most work is the one to move: {lines:?}");
    let (_, body) = i.agenda_card("Today", &[]).unwrap();
    assert!(body.contains("heavy"), "{body}");
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
fn a_row_can_be_put_off_edited_or_removed_and_the_evening_carries_the_rest_over() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    let today = i.today_day();
    run_bar(&mut i, "todo Report due today");
    run_bar(&mut i, "todo Lab sheet due today");
    run_bar(&mut i, "todo Later due fri");
    i.take_commands();
    i.activate(Action::TodoPostpone(0, 7));
    assert_eq!(i.todos[0].due, Some(today + 7));
    assert!(matches!(i.take_commands().as_slice(), [Command::PushTask { .. }]));
    i.activate(Action::TodoEdit(1));
    assert!(i.bar.as_ref().is_some_and(|b| b.text.starts_with("edit Lab sheet -> ")));
    i.bar = None;
    // what is left of today (the first was put off): one task, offered a move to tomorrow
    let (card, actions) = i.with_leftovers(None, false);
    let (summary, body) = card.expect("a card for what is left");
    assert!(summary.contains("unfinished") && body.contains("Lab sheet"), "{summary} · {body}");
    assert_eq!(actions[0].key, "open-island:carry");
    i.own_card_action("open-island:carry", "");
    assert_eq!(i.todos[1].due, Some(today + 1));
    assert!(i.with_leftovers(None, false).0.is_none(), "nothing left");
    i.activate(Action::TodoRemove(1));
    assert!(!i.todos.iter().any(|t| t.text == "Lab sheet"));
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

#[test]
fn the_pill_widens_for_a_long_line_of_lyrics_and_stays_put_for_a_short_one() {
    let mut i = Island::new(Config { lyrics_in_pill: true, ..Config::default() });
    i.data = data();
    let long = "I've been walking down this road for a thousand miles and I still don't know the way home";
    i.data.lyrics_lrc = Some(format!("[00:01.00] ok\n[00:10.00] {long}\n[00:30.00] end\n"));
    i.data.media.as_mut().unwrap().playing = true;
    i.data.media.as_mut().unwrap().position_ms = 2_000.0;
    i.data.media.as_mut().unwrap().sampled_at_ms = T0;
    run(&mut i, 600);
    let base = i.face_size(Face::MediaMini).0;
    assert!((base - Face::MediaMini.size().0).abs() < 80.0, "a short line fits the usual pill: {base}");
    i.data.media.as_mut().unwrap().position_ms = 11_000.0;
    i.data.media.as_mut().unwrap().sampled_at_ms = i.now;
    run(&mut i, 16);
    let wide = i.face_size(Face::MediaMini).0;
    assert!(wide > base + 200.0 && wide <= 640.0, "{base} -> {wide}");
    i.cfg.lyrics_in_pill = false;
    assert_eq!(i.face_size(Face::MediaMini).0, base.min(i.face_size(Face::MediaMini).0), "off: the title's pill");
    assert!(i.face_size(Face::MediaMini).0 < wide);
}

/// Ticks and draws, as the platform does: the card's motion and its place come from the drawing.
fn card_frames(i: &mut Island, f: &Fonts, ms: i64) -> crate::island::Frame {
    let mut fr = i.tick(i.now.max(T0), 8.0);
    for _ in 0..(ms / 8).max(1) {
        fr = i.tick(i.now + 8, 8.0);
        i.draw(&mut Scene::new(), f, fr.surface, 1.0);
    }
    fr
}

#[test]
fn a_card_drops_under_the_pill_takes_the_pointer_and_goes_back_in() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    assert_eq!(i.input_rects().len(), 1);
    i.push(T0, Input::Notification(incoming(1)));
    let fr = card_frames(&mut i, &f, 1500);
    let rects = i.input_rects();
    assert_eq!(rects.len(), 2, "the pill and the card");
    let (pill, card) = (rects[0], rects[1]);
    assert!((card.1 - (pill.1 + pill.3 + 12.0)).abs() < 0.5, "under the pill, past the gap: {pill:?} {card:?}");
    assert!(card.0 >= 0.0 && card.0 + card.2 <= fr.surface.0 && card.1 + card.3 <= fr.surface.1, "{card:?} in {:?}", fr.surface);
    assert!(((card.0 + card.2 / 2.0) - (pill.0 + pill.2 / 2.0)).abs() < 1.0, "centred under it");
    i.activate(Action::Dismiss);
    card_frames(&mut i, &f, 100);
    assert!(i.has_card() && i.input_rects().len() == 1, "still on its way, and no longer takes the pointer");
    card_frames(&mut i, &f, 1500);
    assert!(!i.has_card());
}

/// A card from an app of its own, so that none joins another.
fn from_app(id: u32) -> Incoming {
    Incoming { app: format!("app{id}"), ..incoming(id) }
}

/// The ids of the cards that stay, top to bottom.
fn stacked(i: &Island) -> Vec<u32> {
    i.cards.live().map(|c| c.n.id).collect()
}

#[test]
fn three_cards_stack_one_under_another_and_the_rest_wait() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    for id in 1..=5 {
        i.push(T0, Input::Notification(from_app(id)));
    }
    card_frames(&mut i, &f, 16);
    assert_eq!(stacked(&i), [1, 2, 3]);
    assert_eq!(i.waiting.len(), 2);
    let born: Vec<i64> = i.cards.live().map(|c| c.born).collect();
    assert!(born[1] >= born[0] + 700 && born[2] >= born[1] + 700, "one at a time: {born:?}");
    assert!(i.below[1].until >= born[2] + i.cfg.notification_ms - 16, "its time starts when it shows");
    let fr = card_frames(&mut i, &f, 2600);
    let r = i.input_rects();
    assert_eq!(r.len(), 4, "the pill and three cards");
    for pair in r[1..].windows(2) {
        assert!((pair[1].1 - (pair[0].1 + pair[0].3 + 10.0)).abs() < 1.0, "one below another: {r:?}");
    }
    let last = r[3];
    assert!(last.1 + last.3 <= fr.surface.1 && fr.surface.1 <= MAX_SURFACE.1, "{r:?} in {:?}", fr.surface);
}

#[test]
fn when_a_card_goes_those_below_move_up_and_a_waiting_one_comes() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    for id in 1..=4 {
        i.push(T0, Input::Notification(from_app(id)));
    }
    card_frames(&mut i, &f, 3000);
    let first = i.input_rects()[1];
    // the middle one is closed: the others stay
    i.activate(Action::OnCard(2, Box::new(Action::Dismiss)));
    assert_eq!(closes(&mut i), vec![(2, CloseReason::Dismissed)]);
    card_frames(&mut i, &f, 100);
    let gone = i.cards.stack.iter().find(|c| c.leaving).unwrap();
    let rises = gone.born + (gone.anim.as_ref().unwrap().rise_at().unwrap() * 1000.0) as i64;
    assert!(i.cards.live().last().unwrap().born >= rises + 450, "the new one waits for the stack to close up");
    card_frames(&mut i, &f, 1500);
    assert_eq!(stacked(&i), [1, 3, 4], "the one that waited goes to the bottom");
    assert!(i.waiting.is_empty());
    assert!((i.input_rects()[2].1 - (first.1 + first.3 + 10.0)).abs() < 1.0, "the third moved up: {:?}", i.input_rects());
    // then the first expires and the rest move up again
    card_frames(&mut i, &f, 700);
    assert_eq!(closes(&mut i), vec![(1, CloseReason::Expired)]);
    card_frames(&mut i, &f, 1000);
    assert_eq!(stacked(&i), [3, 4]);
    assert!((i.input_rects()[1].1 - first.1).abs() < 1.0, "{:?}", i.input_rects());
    card_frames(&mut i, &f, 9000);
    assert!(!i.has_card() && i.notif.is_none() && i.below.is_empty());
}

#[test]
fn an_action_on_a_lower_card_is_that_cards_own() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    let act = vec![NotifAction { key: "open".into(), label: "Open".into() }];
    i.push(T0, Input::Notification(from_app(1)));
    i.push(T0, Input::Notification(Incoming { actions: act, ..from_app(2) }));
    card_frames(&mut i, &f, 2000);
    let hit = i.hits.iter().find(|h| h.action == Action::OnCard(2, Box::new(Action::NotifAction("open".into())))).expect("its button").clone();
    let (x, y) = (hit.rect.center().x + i.pill_origin.0, hit.rect.center().y + i.pill_origin.1);
    let cmds = i.pointer_down(i.now, x, y);
    i.pending.extend(cmds);
    let cmds = i.take_commands();
    assert_eq!(cmds[..2], [Command::InvokeAction { id: 2, key: "open".into() }, Command::NotificationClosed { id: 2, reason: CloseReason::Dismissed }]);
    assert_eq!(i.notif.as_ref().map(|n| n.id), Some(1), "the first one stays");
    assert!(i.below.is_empty());
    run(&mut i, 500);
    assert_eq!(i.current_face(), Face::Idle, "a press on a card does not open the pill");
    // a card that stays after what was done on it keeps its place
    i.push(i.now, Input::Notification(Incoming { body: "482913 is your code".into(), summary: "Your verification code".into(), ..from_app(3) }));
    i.activate(Action::OnCard(3, Box::new(Action::CopyCode("482913".into()))));
    assert_eq!((i.notif.as_ref().map(|n| n.id), i.below.iter().map(|b| b.n.id).collect::<Vec<_>>()), (Some(1), vec![3]));
}

#[test]
fn a_reply_to_a_lower_card_takes_only_that_card_into_the_pill() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    let reply = vec![NotifAction { key: "inline-reply".into(), label: "Reply".into() }];
    i.push(T0, Input::Notification(from_app(1)));
    i.push(T0, Input::Notification(Incoming { actions: reply, ..from_app(2) }));
    card_frames(&mut i, &f, 2000);
    i.push(i.now, Input::StartReply);
    card_frames(&mut i, &f, 6000);
    assert_eq!(i.reply.as_ref().map(|r| r.id), Some(2));
    assert!(stacked(&i).is_empty(), "the other ran out of time, the one being answered is in the pill");
    assert_eq!(i.notif.as_ref().map(|n| n.id), Some(2), "and waits");
    i.activate(Action::ReplyCancel);
    assert!(i.notif.is_none());
}

#[test]
fn a_message_of_a_conversation_on_a_lower_card_joins_it() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Notification(from_app(1)));
    i.push(T0, Input::Notification(from_app(2)));
    card_frames(&mut i, &f, 2000);
    let born = i.cards.live().nth(1).unwrap().born;
    i.push(i.now, Input::Notification(Incoming { id: 9, body: "Again".into(), ..from_app(2) }));
    card_frames(&mut i, &f, 500);
    assert_eq!(stacked(&i), [1, 9]);
    let c = i.cards.live().nth(1).unwrap();
    assert_eq!((c.born, c.group), (born, 2), "the same card, now standing for two");
    assert_eq!(closes(&mut i), vec![(2, CloseReason::Expired)]);
}

#[test]
fn a_card_going_into_one_that_goes_too_is_not_left_in_mid_air() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    for id in 1..=2 {
        i.push(T0, Input::Notification(from_app(id)));
    }
    card_frames(&mut i, &f, 2000);
    i.activate(Action::OnCard(2, Box::new(Action::Dismiss)));
    card_frames(&mut i, &f, 40);
    i.activate(Action::Dismiss);
    card_frames(&mut i, &f, 16);
    assert!(i.cards.stack.iter().all(|c| c.leaving && c.anchor.is_none()), "both now go into the pill");
    card_frames(&mut i, &f, 2000);
    assert!(!i.has_card());
}

#[test]
fn an_update_changes_the_card_where_it_is() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Notification(incoming(1)));
    card_frames(&mut i, &f, 1500);
    let (born, width) = (i.cards.live().next().unwrap().born, i.input_rects()[1].2);
    i.push(i.now, Input::Notification(Incoming { body: "A much longer line, which needs a wider card than the one before".into(), ..incoming(1) }));
    card_frames(&mut i, &f, 1000);
    assert_eq!(i.cards.live().next().unwrap().born, born, "the same card");
    assert!(i.cards.stack.len() == 1 && i.input_rects()[1].2 > width + 50.0);
}

#[test]
fn the_pointer_on_a_card_holds_its_countdown_for_a_while() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Notification(incoming(1)));
    card_frames(&mut i, &f, 4000);
    let (x, y, w, h) = i.input_rects()[1];
    i.pointer_moved(i.now, x + w / 2.0, y + h / 2.0);
    let time_left = |i: &Island| i.cards.live().next().map_or(0.0, |c| i.card_time_left(c));
    let left = time_left(&i);
    card_frames(&mut i, &f, 4000);
    assert!(i.notif.is_some(), "held past its five seconds");
    assert!((time_left(&i) - left).abs() < 0.05, "the ring stands still: {left} -> {}", time_left(&i));
    assert!(!i.hovering, "the card is not the pill: no page opens");
    i.pointer_left(i.now);
    card_frames(&mut i, &f, 1000);
    assert!(i.notif.is_some(), "a moment more after the pointer leaves");
    card_frames(&mut i, &f, 1000);
    assert!(i.notif.is_none());
    // a pointer that never moves does not hold a card for ever
    i.push(i.now, Input::Notification(incoming(2)));
    card_frames(&mut i, &f, 1500);
    let (x, y, w, h) = i.input_rects()[1];
    i.pointer_moved(i.now, x + w / 2.0, y + h / 2.0);
    card_frames(&mut i, &f, 12_000);
    assert!(i.notif.is_none());
}

#[test]
fn going_from_an_open_page_down_to_the_card_keeps_the_page_open() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Notification(incoming(1)));
    card_frames(&mut i, &f, 1500);
    let (px, py, pw, ph) = i.input_rects()[0];
    i.pointer_moved(i.now, px + pw / 2.0, py + ph / 2.0);
    card_frames(&mut i, &f, 1000);
    assert_eq!(i.current_face(), Face::IdleHover);
    // off the pill, across the gap, onto the card
    i.pointer_left(i.now);
    card_frames(&mut i, &f, 100);
    let (x, y, w, h) = i.input_rects()[1];
    i.pointer_moved(i.now, x + w / 2.0, y + h / 2.0);
    card_frames(&mut i, &f, 1500);
    assert_eq!(i.current_face(), Face::IdleHover, "the page stays");
    assert!(i.over_card(), "and the card is still under the pointer");
}
