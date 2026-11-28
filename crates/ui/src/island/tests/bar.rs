//! The command bar.

use super::*;

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
fn the_bar_finds_emoji_and_symbols_and_copies_the_chosen_one() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    let plain = {
        type_in_bar(&mut i, "10m tea");
        i.face_size(Face::Bar).1
    };
    i.bar = None;
    type_in_bar(&mut i, "emoji fuoco");
    assert_eq!(i.picks()[0].text, "🔥");
    assert!(i.face_size(Face::Bar).1 > plain, "room for the results");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.take_commands().contains(&Command::RestoreClipboard("🔥".into())));
    // Tab moves to the next one, Shift+Tab back round to the last
    i.notif = None;
    type_in_bar(&mut i, "symbol freccia");
    let found = i.picks();
    i.push(i.now, Input::Key(KeyInput::NextPage));
    assert_eq!(i.bar.as_ref().map(|b| b.pick), Some(1));
    i.push(i.now, Input::Key(KeyInput::PrevPage));
    i.push(i.now, Input::Key(KeyInput::PrevPage));
    assert_eq!(i.bar.as_ref().map(|b| b.pick), Some(found.len() - 1));
    // typing starts from the first again
    i.push(i.now, Input::Key(KeyInput::Backspace));
    assert_eq!(i.bar.as_ref().map(|b| b.pick), Some(0));
    // a click copies that one
    draw_frames(&mut i, 60);
    i.take_commands();
    let want = i.picks()[2].text.to_string();
    assert!(press(&mut i, Action::BarPick(2)));
    assert!(i.bar.is_none());
    assert!(i.take_commands().contains(&Command::RestoreClipboard(want)));
    // nothing found: nothing copied
    type_in_bar(&mut i, "emoji qwertyzz");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(!i.take_commands().iter().any(|c| matches!(c, Command::RestoreClipboard(_))));
}
