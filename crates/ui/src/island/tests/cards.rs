//! The cards under the pill: how they drop and stack, scripts' cards and the island's own.

use super::*;

/// A card from an app of its own, so that none joins another.
fn from_app(id: u32) -> Incoming {
    Incoming { app: format!("app{id}"), ..incoming(id) }
}

/// The ids of the cards that stay, top to bottom.
fn stacked(i: &Island) -> Vec<u32> {
    i.cards.live().map(|c| c.n.id).collect()
}

fn scripted(n: u32) -> Incoming {
    Incoming { id: SCRIPT_ID_BASE + n, app: "open-island".into(), summary: format!("s{n}"), kind: Some(island_core::notify::Kind::Ok), ..Default::default() }
}

fn question(n: u32) -> Incoming {
    let b = |k: &str| NotifAction { key: k.into(), label: k.into() };
    Incoming { stay: true, actions: vec![b("no"), b("yes")], ..scripted(n) }
}

/// The ids of the cards that have a place on screen, top to bottom.
fn placed(i: &Island) -> Vec<u32> {
    i.notif.iter().map(|n| n.id).chain(i.below.iter().map(|b| b.n.id)).collect()
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

#[test]
fn a_scripts_cards_never_fold_into_one_another() {
    let mut i = Island::new(Config::default());
    for n in 1..=5 {
        i.push(T0, Input::Notification(Incoming { summary: "same".into(), ..scripted(n) }));
    }
    assert_eq!((placed(&i).len(), i.waiting.len()), (STACK_MAX, 2), "three up, both others wait their turn");
}

#[test]
fn a_newer_card_of_the_same_id_takes_the_place_of_a_waiting_one() {
    let mut i = Island::new(Config::default());
    for n in 1..=4 {
        i.push(T0, Input::Notification(scripted(n)));
    }
    i.push(T0, Input::Notification(Incoming { summary: "newer".into(), ..scripted(4) }));
    assert_eq!(i.waiting.len(), 1);
    assert_eq!(i.waiting[0].summary, "newer");
    // and on screen it keeps its place, with its own time
    i.push(T0, Input::Notification(Incoming { expire_ms: Some(20_000), ..scripted(2) }));
    assert_eq!(placed(&i).len(), STACK_MAX);
    assert_eq!(i.below.iter().find(|b| b.n.id == SCRIPT_ID_BASE + 2).unwrap().until, T0 + 20_000);
}

#[test]
fn a_card_that_stays_outlasts_the_others_but_gives_way_to_one_waiting() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Notification(Incoming { stay: true, ..scripted(1) }));
    run(&mut i, 60_000);
    assert_eq!(placed(&i), [SCRIPT_ID_BASE + 1], "still there after a minute");
    i.push(i.now, Input::Notification(question(2)));
    i.push(i.now, Input::Notification(question(3)));
    i.push(i.now, Input::Notification(scripted(4)));
    run(&mut i, 200);
    assert_eq!(placed(&i), [2, 3, 4].map(|n| SCRIPT_ID_BASE + n), "the one that asks nothing makes room; questions do not");
}

#[test]
fn a_held_back_card_tells_whoever_asked_and_a_question_shows_anyway() {
    let mut i = Island::new(Config::default());
    i.data.dnd = true;
    i.push(T0, Input::Notification(scripted(1)));
    assert!(placed(&i).is_empty());
    assert!(i.pending.contains(&Command::NotificationClosed { id: SCRIPT_ID_BASE + 1, reason: CloseReason::Expired }));
    i.push(T0, Input::Notification(question(2)));
    assert_eq!(placed(&i), [SCRIPT_ID_BASE + 2]);
}

#[test]
fn a_question_is_answered_by_its_buttons_only() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Notification(question(1)));
    run(&mut i, 100);
    i.activate(Action::OnCard(SCRIPT_ID_BASE + 1, Box::new(Action::NotifOpen)));
    assert_eq!(placed(&i), [SCRIPT_ID_BASE + 1], "a click on its body does nothing");
    i.pending.clear();
    let cmds = i.activate(Action::OnCard(SCRIPT_ID_BASE + 1, Box::new(Action::NotifAction("yes".into()))));
    let all: Vec<Command> = i.pending.iter().cloned().chain(cmds).collect();
    assert_eq!(all, [Command::InvokeAction { id: SCRIPT_ID_BASE + 1, key: "yes".into() }, Command::NotificationClosed { id: SCRIPT_ID_BASE + 1, reason: CloseReason::Dismissed }], "the answer comes before the card is gone");
    assert!(placed(&i).is_empty());
}

#[test]
fn a_card_can_be_dismissed_from_outside() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Notification(scripted(1)));
    i.push(T0, Input::DismissCard);
    assert!(placed(&i).is_empty());
    assert!(i.pending.contains(&Command::NotificationClosed { id: SCRIPT_ID_BASE + 1, reason: CloseReason::Dismissed }));
}

#[test]
fn only_the_islands_own_cards_can_have_it_open_something() {
    let mut i = Island::new(Config::default());
    let key = "open-island:file:/etc/passwd".to_string();
    i.push(T0, Input::Notification(Incoming { id: 7, app: "open-island".into(), summary: "x".into(), actions: vec![NotifAction { key: key.clone(), label: "Open".into() }], ..Default::default() }));
    let mut cmds = i.activate(Action::OnCard(7, Box::new(Action::NotifAction(key))));
    cmds.extend(i.pending.clone());
    assert!(!cmds.iter().any(|c| matches!(c, Command::OpenFile(_))), "{cmds:?}");
    i.push(i.now, Input::Notification(Incoming { id: 8, app: "x".into(), summary: "y".into(), actions: vec![NotifAction { key: "open-island:open:https://example.org".into(), label: "Open".into() }], ..Default::default() }));
    let mut cmds = i.activate(Action::OnCard(8, Box::new(Action::NotifOpen)));
    cmds.extend(i.pending.clone());
    assert!(!cmds.iter().any(|c| matches!(c, Command::OpenUrl(_))), "nor by a click on its body: {cmds:?}");
}

#[test]
fn a_value_of_the_bar_drops_as_a_card_that_copies_it_again() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::RunLine("12*7".into()));
    let card = i.notif.clone().expect("the result");
    assert!(card.summary.contains("84"), "{}", card.summary);
    i.pending.clear();
    i.activate(Action::OnCard(card.id, Box::new(Action::NotifOpen)));
    assert!(i.pending.contains(&Command::RestoreClipboard("84".into())), "{:?}", i.pending);
}

#[test]
fn a_copied_link_drops_a_card_that_opens_it() {
    let mut i = Island::new(Config::default());
    i.data.clipboard = vec!["hello".into()];
    i.tick(T0, 8.0);
    i.data.clipboard.insert(0, "just words".into());
    i.tick(T0 + 8, 8.0);
    assert!(up(&i).is_empty(), "nothing to open");
    i.data.clipboard.insert(0, "https://example.org/a".into());
    i.tick(T0 + 16, 8.0);
    assert_eq!(up(&i), ["Link copied"]);
    let card = i.notif.clone().unwrap();
    i.pending.clear();
    i.activate(Action::OnCard(card.id, Box::new(Action::NotifOpen)));
    assert!(i.pending.contains(&Command::OpenUrl("https://example.org/a".into())), "{:?}", i.pending);
    // not while quiet, nor when switched off
    i.data.dnd = true;
    i.data.clipboard.insert(0, "https://example.org/b".into());
    i.tick(T0 + 24, 8.0);
    assert!(up(&i).is_empty());
    i.data.dnd = false;
    i.cfg.link_cards = false;
    i.data.clipboard.insert(0, "https://example.org/c".into());
    i.tick(T0 + 32, 8.0);
    assert!(up(&i).is_empty());
}
