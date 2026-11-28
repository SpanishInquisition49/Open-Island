//! Other programs' notifications: their life, their answers and the history.

use super::*;

fn message(id: u32, app: &str, from: &str, text: &str) -> Incoming {
    Incoming { id, app: app.into(), summary: from.into(), body: text.into(), ..Default::default() }
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
