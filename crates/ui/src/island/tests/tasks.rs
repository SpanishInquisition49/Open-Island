//! The to-do list, pinned notes and the search.

use super::*;

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
