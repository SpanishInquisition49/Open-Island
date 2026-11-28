//! Taking back what was removed.

use super::*;

/// The key of the Undo button on the card that says something went.
fn undo_key(i: &Island) -> (u32, String) {
    let cards = i.notif.iter().chain(i.below.iter().map(|b| &b.n));
    cards.filter_map(|n| n.actions.iter().find(|a| a.label == "Undo").map(|a| (n.id, a.key.clone()))).next().expect("a card with Undo")
}

fn undo(i: &mut Island) {
    let (id, key) = undo_key(i);
    i.activate(Action::OnCard(id, Box::new(Action::NotifAction(key.clone()))));
    assert!(i.notif.iter().all(|n| n.actions.iter().all(|a| a.key != key)), "the card goes once pressed");
}

#[test]
fn a_cross_pressed_by_mistake_is_taken_back_where_it_was() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    run_bar(&mut i, "exam 15 dec Algorithms ~20h");
    run_bar(&mut i, "exam 20 dec Networks");
    let deadlines = i.deadlines.clone();
    i.activate(Action::DeadlineDone(0));
    assert_eq!(i.deadlines.len(), 1);
    assert_eq!(card(&i), Some(("Removed from the plan".into(), "Algorithms".into())));
    undo(&mut i);
    assert_eq!(i.deadlines, deadlines);

    for p in ["one", "two", "three"] {
        i.pin(p);
    }
    i.notif = None;
    let pins = i.pins().to_vec();
    i.activate(Action::PinRemove(1));
    undo(&mut i);
    assert_eq!(i.pins(), pins);
    run_bar(&mut i, "unpin all");
    i.push(i.now, Input::PinRemove("all".into()));
    assert!(i.pins().is_empty());
}

#[test]
fn only_the_last_removal_is_taken_back_and_only_once() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    i.push(i.now, Input::ShelveQuietly(vec!["/tmp/a".into(), "/tmp/b".into(), "/tmp/c".into()]));
    i.activate(Action::ShelfRemove(0));
    let (_, first) = undo_key(&i);
    i.activate(Action::ShelfClear);
    assert!(i.shelf().is_empty());
    assert_eq!(card(&i), Some(("Shelf cleared".into(), "2 files".into())));
    let (_, last) = undo_key(&i);
    assert_ne!(first, last);
    assert!(i.event_action(&first) && i.shelf().is_empty(), "the card of an earlier removal brings back nothing");
    assert!(i.event_action(&last));
    assert_eq!(i.shelf(), ["/tmp/b", "/tmp/c"]);
    i.activate(Action::ShelfRemove(1));
    let (_, key) = undo_key(&i);
    i.push(i.now, Input::ShelveQuietly(vec!["/tmp/c".into()]));
    assert!(i.event_action(&key));
    assert_eq!(i.shelf(), ["/tmp/c", "/tmp/b"], "one set down again meanwhile is not there twice");
    assert!(i.event_action(&key));
    assert_eq!(i.shelf().len(), 2);
}

#[test]
fn a_task_taken_back_goes_to_the_server_again() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    run_bar(&mut i, "todo report");
    run_bar(&mut i, "todo milk");
    let uid = i.todos()[0].uid.clone();
    i.take_commands();
    i.activate(Action::TodoRemove(0));
    assert!(i.take_commands().contains(&Command::DeleteTask { uid: uid.clone() }));
    undo(&mut i);
    assert_eq!(i.todos().iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), ["report", "milk"]);
    assert!(i.take_commands().iter().any(|c| matches!(c, Command::PushTask { task, done: false } if task.uid == uid && !task.synced)));
    // a list read before the server has it again does not drop it
    i.push(i.now, Input::TasksRemote(vec![]));
    assert_eq!(i.todos().len(), 2);
}

#[test]
fn a_reminder_of_a_place_comes_back_and_a_full_list_is_said() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    i.data.config.home_pos = Some((43.7, 10.4));
    run_bar(&mut i, "remind me when i get home buy milk");
    let waiting = i.places_saved().1.to_vec();
    i.activate(Action::WaitingRemove(0));
    assert!(i.places_saved().1.is_empty());
    let (_, key) = undo_key(&i);
    for n in 0..20 {
        run_bar(&mut i, &format!("remind me when i get home thing {n}"));
    }
    assert!(i.event_action(&key));
    assert_eq!(i.places_saved().1.len(), 20);
    assert!(i.places_saved().1.iter().all(|w| w.text != waiting[0].text));
    assert!(i.history.iter().all(|(n, _)| n.app != "open-island"), "the island's own words are no history");
}
