//! Deadlines, exams, the study plan and the study hours.

use super::*;

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
fn studying_takes_time_off_the_work_and_the_plan_spreads_what_is_left() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    run_bar(&mut i, "esame 25 jan Algoritmi #algoritmi ~10h");
    let today = i.today_day();
    assert_eq!(i.deadlines()[0].est_min, 600);
    assert_eq!(i.study_on(today), vec![("Algoritmi".to_string(), 60)], "ten days left: an hour a day");
    run_bar(&mut i, "log 90m algoritmi");
    assert_eq!((i.deadlines()[0].est_min, i.deadlines()[0].done_min), (600, 90), "the hours stay, what is done is counted");
    assert_eq!(i.study_log(), [(today, "algoritmi".to_string(), 90)]);
    assert_eq!(i.studied_this_week(), vec![("algoritmi".to_string(), 90)]);
    assert_eq!(i.study_line(today), None, "more than today's hour is done: nothing more is asked today");
    assert_eq!(i.study_on(today + 1), vec![("Algoritmi".to_string(), 60)], "the nine other days share the 8h30 left");
    let (title, body) = i.agenda_card("Tomorrow", &[]).expect("the plan alone is worth a card");
    assert!(title.starts_with("Tomorrow") && body.contains("Study 1h"), "{title} · {body}");
    run_bar(&mut i, "log 30m chemistry");
    assert_eq!(i.deadlines()[0].done_min, 90, "a course with no work leaves it alone");
}

#[test]
fn study_counts_for_an_exam_whether_its_hours_are_said_before_or_after() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    run_bar(&mut i, "exam 25 jan mpp");
    type_in_bar(&mut i, "log 4h MPP");
    i.push(i.now, Input::Key(KeyInput::Enter));
    let card = i.notif.take().expect("a card says where it went");
    assert!(card.body.contains("mpp") && card.body.contains("4h"), "{}", card.body);
    assert_eq!(i.deadlines()[0].done_min, 240);
    assert_eq!((i.plan_view().deadlines[0].done_min, i.plan_view().deadlines[0].left_min), (240, 0));
    // the hours come later, as a total: what was studied is part of it
    run_bar(&mut i, "exam 25 jan mpp ~20h");
    let row = &i.plan_view().deadlines[0];
    assert_eq!((row.est_min, row.done_min, row.left_min), (1200, 240, 960));
    // moved or renamed from the page, it keeps what was done
    i.activate(Action::DeadlineEdit(0));
    i.bar.as_mut().unwrap().text = "exam 2027-01-28 MPP orale ~20h".into();
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert_eq!(i.deadlines()[0].done_min, 240);
    // logged before the exam was written down
    run_bar(&mut i, "log 90m reti");
    run_bar(&mut i, "exam 30 jan Reti ~10h");
    assert_eq!(i.deadlines()[1].done_min, 90, "{:?}", i.deadlines());
    type_in_bar(&mut i, "log 1h chimica");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.notif.take().is_some_and(|n| n.body.contains("No exam or task")));
}

#[test]
fn study_logged_for_an_exam_saved_without_hours_is_counted_at_the_next_start() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    let today = i.today_day();
    let saved = |text: &str, day, est_min| Deadline { day, text: text.into(), est_min, ..Default::default() };
    // as an earlier version left them: no count of what was done; hours already reduced where there were any
    i.restore_deadlines(vec![saved("mpp", today + 40, 0), saved("reti", today + 50, 300)]);
    i.restore_study_log(vec![(today, "MPP".into(), 240), (today, "reti".into(), 60)]);
    assert_eq!(i.deadlines().iter().map(|d| d.done_min).collect::<Vec<_>>(), [240, 0]);
}

#[test]
fn exams_with_credits_and_no_hours_share_the_free_time() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    let today = i.today_day();
    run_bar(&mut i, "exam 25 jan AE 12cfu");
    run_bar(&mut i, "exam 25 jan ap 6 cfu");
    run_bar(&mut i, "exam 25 jan mpp");
    let on = |i: &Island, d: i64| i.study_on(today + d);
    assert_eq!(on(&i, 0), [("AE".to_string(), 315), ("ap".to_string(), 165)], "eight hours, two to one, in quarters");
    let view = i.plan_view();
    assert!(view.today.iter().all(|r| r.suggested));
    assert_eq!(view.split.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["AE", "ap"], "the one without credits has no weight");
    assert!(view.week.iter().all(|d| !d.heavy()), "a suggestion fills the day without making it heavy: {:?}", view.week);
    assert!(i.study_line(today).is_some_and(|l| l.contains("AE 5h15")));
    // studied for one of them: its part of today goes down
    type_in_bar(&mut i, "log 2h AE");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.notif.take().is_some_and(|n| n.body == "AE: 2h studied"));
    assert_eq!(on(&i, 0), [("AE".to_string(), 195), ("ap".to_string(), 165)]);
    // hours said for one: they are planned, and the other one gets what is still free
    run_bar(&mut i, "exam 25 jan AE ~22h");
    assert_eq!((i.deadlines().iter().find(|d| d.text == "AE").map(|d| d.cfu)), Some(12), "the credits stay");
    let view = i.plan_view();
    assert_eq!(view.today.iter().map(|r| (r.what.as_str(), r.suggested)).collect::<Vec<_>>(), [("AE", false), ("ap", true)]);
    i.activate(Action::DeadlineEdit(i.deadlines().iter().position(|d| d.text == "AE").unwrap()));
    assert_eq!(i.bar.as_ref().unwrap().text, "exam 2027-01-25 AE 12cfu ~22h");
}

#[test]
fn the_plan_never_asks_for_more_time_than_there_is() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    let today = i.today_day();
    let h = 3_600_000;
    let ev = |id: &str, from: i64, hours: i64| Event { id: id.into(), title: id.into(), start_ms: from, end_ms: from + hours * h, ..Default::default() };
    i.data.events = vec![ev("a", i.day_start_ms(today) + 9 * h, 3), ev("b", i.day_start_ms(today + 1) + 9 * h, 10), ev("late", i.day_start_ms(today) + 22 * h, 1)];
    for line in ["exam 17 jan AE ~40h", "exam 19 jan ap ~30h", "exam 20 jan mpp 6cfu", "todo Report due tomorrow ~9h"] {
        run_bar(&mut i, line);
    }
    let asked = |i: &Island, d: i64| i.study_on(today + d).iter().map(|(_, m)| m).sum::<u32>();
    // 08:00: the study hours, 9 to 17, less the three of events in them (the one at 22 is outside)
    assert_eq!(asked(&i, 0), 300);
    assert_eq!(asked(&i, 1), 0, "ten hours of events");
    assert!((0..7).all(|d| asked(&i, d) <= 480), "{:?}", (0..7).map(|d| asked(&i, d)).collect::<Vec<_>>());
    assert!(i.plan_view().deadlines.iter().any(|d| d.short_min > 0), "and what does not fit is told");
    // 16:00: one hour to the end of the study hours
    i.tick(i.day_start_ms(today) + 16 * h, 8.0);
    assert_eq!(asked(&i, 0), 60);
    i.tick(i.day_start_ms(today) + 17 * h + 60_000, 8.0);
    assert_eq!((asked(&i, 0), i.plan_view().room_today), (0, 0));
}

#[test]
fn only_what_falls_in_a_days_study_hours_counts() {
    use island_core::plan::StudyHours;
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0); // Friday 15 January, 08:00
    let today = i.today_day();
    let h = 3_600_000;
    let at = |i: &Island, day: i64, hour: i64| i.day_start_ms(day) + hour * h;
    let ev = |id: &str, a: i64, b: i64| Event { id: id.into(), title: id.into(), start_ms: a, end_ms: b, ..Default::default() };
    // today: an hour before the study hours, one across their end, a whole evening after them
    i.data.events = vec![ev("run", at(&i, today, 7), at(&i, today, 8)), ev("lecture", at(&i, today, 16), at(&i, today, 18)), ev("dinner", at(&i, today, 19), at(&i, today, 23))];
    run_bar(&mut i, "esame 25 jan Reti ~100h");
    let first = i.week_loads()[0].clone();
    assert_eq!((first.event_min, first.events, first.study_min), (60, 1, 420), "one hour of the lecture is inside, the rest is free: {first:?}");
    // Friday from 15 to 20: the lecture and an hour of the dinner are inside now
    let friday = island_core::calendar::weekday_of_days(today) as usize;
    i.cfg.study_week[friday] = StudyHours { on: true, from: 15 * 60, to: 20 * 60, pause: None };
    let first = i.week_loads()[0].clone();
    assert_eq!((first.event_min, first.study_min), (180, 120), "{first:?}");
    assert_eq!(i.plan_view().room_today, 120);
    // the study hours not begun yet: all of them are ahead; half way through only what is left
    i.tick(at(&i, today, 19), 8.0);
    assert_eq!(i.plan_view().room_today, 0, "the last hour is the dinner's");
    // a day off asks nothing, and is not offered as the lighter day to move work to
    i.cfg.study_week[(friday + 1) % 7].on = false;
    assert_eq!(i.study_on(today + 1), vec![]);
    assert_eq!(i.week_loads()[1].study_min, 0);
    assert!(!i.study_on(today + 2).is_empty());
}

#[test]
fn the_plan_leaves_the_lunch_pause_free() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0); // 08:00
    let today = i.today_day();
    let h = 3_600_000;
    let day = island_core::plan::StudyHours { on: true, from: 9 * 60, to: 18 * 60, pause: Some((13 * 60, 14 * 60)) };
    i.cfg.study_week = [day; 7];
    // an appointment over lunch takes nothing, the part of a lecture that runs into it neither
    let at = i.day_start_ms(today);
    i.data.events = vec![Event { id: "l".into(), title: "Lecture".into(), start_ms: at + 12 * h, end_ms: at + 14 * h, ..Default::default() }];
    run_bar(&mut i, "esame 25 jan Reti ~100h");
    let first = i.week_loads()[0].clone();
    assert_eq!((first.event_min, first.study_min), (60, 420), "{first:?}");
    // at 13:30 only the afternoon is ahead
    i.tick(at + 13 * h + h / 2, 8.0);
    assert_eq!(i.plan_view().room_today, 240);
    i.tick(at + 12 * h, 8.0);
    assert_eq!(i.plan_view().room_today, 240, "the rest of the morning is the lecture's");
}

#[test]
fn the_study_hours_are_edited_on_the_plan_page_and_kept() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    i.data = data();
    i.tick(T0, 8.0);
    let open = |i: &mut Island| {
        for _ in 0..200 {
            let t = i.now + 8;
            i.push(t, Input::OpenPage(Page::Plan));
            let fr = i.tick(t, 8.0);
            let mut scene = Scene::new();
            i.draw(&mut scene, &f, fr.surface, 1.0);
        }
    };
    open(&mut i);
    let plan_page = i.hits.iter().filter(|h| matches!(h.action, Action::PlanHours(true))).count();
    assert_eq!(plan_page, 1, "a button opens the editor");
    i.activate(Action::PlanHours(true));
    open(&mut i);
    assert_eq!(i.hits.iter().filter(|h| matches!(h.action, Action::StudyStep(..))).count(), 28, "four buttons a day");
    let kept = |cmds: Vec<Command>| match cmds.as_slice() {
        [Command::SetOption { key, value: OptionValue::Text(t) }] if key == "study_hours" => t.clone(),
        other => panic!("{other:?}"),
    };
    assert!(kept(i.activate(Action::StudyStep(0, island_core::plan::Edge::Start, true))).starts_with("09:30-17:00,09:00-17:00"));
    assert!(kept(i.activate(Action::StudyCopyWeekdays)).starts_with("09:30-17:00,09:30-17:00,09:30-17:00,09:30-17:00,09:30-17:00,09:00-17:00"));
    assert!(kept(i.activate(Action::StudyDayToggle(6))).ends_with(",off 09:00-17:00"));
    open(&mut i);
    assert_eq!(i.hits.iter().filter(|h| matches!(h.action, Action::StudyStep(..))).count(), 24, "a day off has none");
    // a lunch pause on Monday: the day is two stretches, each with its two times, and the page is taller
    let before = i.plan_height();
    assert!(kept(i.activate(Action::StudyPause(0))).starts_with("09:30-13:00+14:00-17:00,09:30-17:00"));
    open(&mut i);
    assert_eq!(i.hits.iter().filter(|h| matches!(h.action, Action::StudyStep(..))).count(), 28);
    assert!(i.plan_height() > before);
    assert!(kept(i.activate(Action::StudyStep(0, island_core::plan::Edge::PauseEnd, true))).starts_with("09:30-13:00+14:30-17:00,"));
    // leaving the page leaves the editor
    for _ in 0..200 {
        let t = i.now + 8;
        i.push(t, Input::OpenPage(Page::Today));
        i.tick(t, 8.0);
    }
    open(&mut i);
    assert!(i.hits.iter().any(|h| matches!(h.action, Action::PlanHours(true))));
}

#[test]
fn the_clock_pill_counts_down_to_the_nearest_deadline_when_it_is_near() {
    let mut i = Island::new(Config::default());
    run(&mut i, 2000);
    assert_eq!(i.countdown(), None);
    let plain = i.face_size(Face::Idle).0;
    run_bar(&mut i, "exam 30 jan Algoritmi");
    run_bar(&mut i, "exam 28 feb Reti");
    assert_eq!(i.countdown(), Some(("Algoritmi · 15 d".to_string(), false)));
    assert!(i.face_size(Face::Idle).0 > plain + 80.0, "the pill makes room");
    run_bar(&mut i, "exam 17 jan Un esame dal nome lunghissimo");
    assert_eq!(i.countdown(), Some(("Un esame dal no… · 2 d".to_string(), true)));
    i.cfg.countdown_days = 1;
    assert_eq!(i.countdown(), None);
    i.cfg.countdown_days = 0;
    assert_eq!((i.countdown(), i.face_size(Face::Idle).0), (None, plain));
}

#[test]
fn too_long_on_a_distracting_app_in_study_hours_gets_a_card() {
    let mut i = Island::new(Config { break_minutes: 0, deep_work_minutes: 0, ..Config::default() });
    i.tick(T0, 8.0); // 08:00, study hours from 9 to 17
    let min = 60_000;
    let at = |i: &mut Island, m: i64| {
        i.notif = None;
        for s in 0..=1 {
            i.tick(T0 + m * min + s * 1100, 8.0);
        }
        i.notif.as_ref().map(|n| (n.summary.clone(), n.body.clone()))
    };
    let cmds = i.activate(Action::ToggleDistraction("Telegram".into()));
    assert!(matches!(cmds.as_slice(), [Command::SetOption { key, value: OptionValue::Text(t) }] if key == "distraction_apps" && t == "Telegram"));
    i.push(T0 + min, Input::Focused { app: "org.telegram.desktop".into(), title: String::new() });
    assert_eq!(at(&mut i, 59), None, "before the study hours");
    assert_eq!(at(&mut i, 70), None, "ten minutes of them so far");
    assert_eq!(at(&mut i, 80), Some(("20 min on Telegram".into(), "These are study hours, until 17:00".into())));
    assert_eq!(at(&mut i, 90), None, "told once");
    assert!(at(&mut i, 101).is_some(), "and again after as long");
    // another app for a good while: the count starts over
    i.push(T0 + 102 * min, Input::Focused { app: "code".into(), title: String::new() });
    assert_eq!(at(&mut i, 130), None);
    i.push(T0 + 131 * min, Input::Focused { app: "org.telegram.desktop".into(), title: String::new() });
    assert_eq!(at(&mut i, 140), None);
    assert!(at(&mut i, 153).is_some());
    // the mark off, no more
    assert!(!i.activate(Action::ToggleDistraction("telegram".into())).is_empty());
    assert!(i.cfg.distraction_apps.is_empty());
    assert_eq!(at(&mut i, 200), None);
}

#[test]
fn the_days_of_the_countdown_are_chosen() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    // forty days away: beyond the three weeks it starts with
    run_bar(&mut i, "exam 24 feb Algorithms");
    assert!(i.countdown().is_none());
    i.take_commands();
    run_bar(&mut i, "countdown 40");
    assert_eq!(i.countdown().map(|c| c.0), Some("Algorithms · 40 d".into()));
    assert!(i.take_commands().contains(&Command::SetOption { key: "countdown_days".into(), value: OptionValue::Num(40.0) }), "kept for the next start");
    i.activate(Action::Step("countdown_days", -1));
    assert_eq!(i.cfg.countdown_days, 39, "a day at a time");
    assert!(i.countdown().is_none());
    run_bar(&mut i, "countdown off");
    assert_eq!(i.cfg.countdown_days, 0);
}

#[test]
fn an_exam_has_one_day_which_can_be_moved() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0); // 15 January
    run_bar(&mut i, "esame 25 jan Algoritmi #algoritmi ~10h");
    let first = i.deadlines()[0].day;
    run_bar(&mut i, "exam 30 jan algoritmi");
    assert_eq!(i.deadlines().len(), 1, "the same name is the same exam: {:?}", i.deadlines());
    let d = &i.deadlines()[0];
    assert_eq!((d.day, d.est_min, d.course.as_str()), (first + 5, 600, "algoritmi"), "moved, and what was not said again stays");
    run_bar(&mut i, "exam 2 feb");
    run_bar(&mut i, "exam 9 feb");
    assert_eq!(i.deadlines().len(), 3, "two without a name are two exams");
}

#[test]
fn a_click_on_a_deadline_opens_the_line_that_changes_it_and_running_it_replaces_it() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    run_bar(&mut i, "esame 25 jan Reti #reti_di_calcolatori ~10h30");
    i.activate(Action::DeadlineEdit(0));
    let bar = i.bar.clone().expect("the bar is open");
    assert_eq!(bar.text, "exam 2027-01-25 Reti #reti_di_calcolatori ~10h30");
    assert_eq!(bar.cursor, "exam 2027-01-25".len(), "ready to change the day");
    // a new name and no hours: the line says everything
    i.bar.as_mut().unwrap().text = "exam 2027-01-28 Reti, secondo appello".into();
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert_eq!(i.deadlines().len(), 1, "{:?}", i.deadlines());
    assert_eq!((i.deadlines()[0].text.as_str(), i.deadlines()[0].est_min, i.deadlines()[0].course.as_str()), ("Reti, secondo appello", 0, ""));
    i.notif = None;
    i.activate(Action::DeadlineAdd);
    assert_eq!(i.bar.as_ref().map(|b| (b.text.as_str(), b.replaces.clone())), Some(("exam ", None)));
}

#[test]
fn the_plan_gives_less_study_to_a_day_of_lectures_and_never_counts_work_twice() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    let today = i.today_day();
    let h = 3_600_000;
    // tomorrow six of the eight hours go in lectures
    let from = i.day_start_ms(today + 1) + 9 * h;
    // the same hours entered twice, and one event inside them: six hours are taken, not fourteen
    let lecture = |id: &str, a: i64, b: i64| Event { id: id.into(), title: "Lezioni".into(), start_ms: a, end_ms: b, ..Default::default() };
    i.data.events = vec![lecture("l", from, from + 6 * h), lecture("m", from, from + 6 * h), lecture("n", from + h, from + 3 * h)];
    run_bar(&mut i, "esame 19 jan Algoritmi ~13h"); // four days: 8 + 2 + 8 + 8 hours free
    let on = |i: &Island, d: i64| i.study_on(today + d).iter().map(|(_, m)| m).sum::<u32>();
    assert_eq!([on(&i, 0), on(&i, 1), on(&i, 2), on(&i, 3)], [240, 60, 240, 240]);
    assert_eq!((0..7).map(|d| on(&i, d)).sum::<u32>(), 13 * 60);
    i.cfg.study_week = [island_core::plan::StudyHours { on: true, from: 9 * 60, to: 15 * 60, pause: None }; 7];
    assert_eq!(on(&i, 1), 0, "a shorter day leaves nothing next to six hours of lectures");
    let view = i.plan_view();
    assert_eq!((view.deadlines.len(), view.deadlines[0].left_min, view.deadlines[0].short_min), (1, 780, 0));
    i.cfg.study_week = [island_core::plan::StudyHours { on: true, from: 9 * 60, to: 11 * 60, pause: None }; 7];
    assert!(i.plan_view().deadlines[0].short_min > 0, "six hours free before the exam are not enough for thirteen");
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
fn the_plan_and_find_pages_draw_and_the_plan_flags_a_heavy_day() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    i.data = data();
    i.tick(T0, 8.0);
    let h = 3_600_000;
    // tomorrow: seven hours of lectures, from nine
    i.data.events = (0..4).map(|k| Event { id: format!("e{k}"), title: format!("Lecture {k}"), start_ms: T0 + 25 * h + k * 2 * h, end_ms: T0 + 25 * h + k * 2 * h + 3_600_000 * 2 - 600_000, ..Default::default() }).collect();
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

const MINUTE: i64 = 60_000;

#[test]
fn play_on_todays_study_starts_a_quiet_session_that_counts_its_own_time() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    run_bar(&mut i, "exam 25 jan Algoritmi #ae ~10h");
    i.notif = None;
    i.activate(Action::Open(Page::Plan));
    draw_frames(&mut i, 60);
    assert!(press(&mut i, Action::StudyStart(0)), "the row of today has its play button");
    assert_eq!(i.session().map(|t| t.label.as_str()), Some("ae"), "logged under the course");
    i.tick(T0 + MINUTE, 8.0);
    assert_eq!(i.auto_quiet_reason(), Some("study"));
    assert!(!i.quiet_blocks_headsup(), "a lecture about to start and the distraction nudge still come");
    i.push(i.now, Input::Notification(incoming(7)));
    assert!(card(&i).is_none_or(|c| c.0 != "Anna"), "held while the focus runs");
    i.activate(Action::Open(Page::Plan));
    draw_frames(&mut i, 60);
    assert!(!i.hits.iter().any(|h| h.action == Action::StudyStart(0)), "the running row offers to stop instead");
    assert!(i.hits.iter().any(|h| h.action == Action::TimerCancel));
    let end = i.timer.as_ref().and_then(|t| t.end_ms).expect("a focus has an end");
    i.tick(end + 100, 8.0);
    i.tick(end + 200, 8.0);
    assert_eq!(i.deadlines()[0].done_min, 25);
    assert_eq!(i.auto_quiet_reason(), None, "the break is not quiet");
    assert!(i.session().is_some(), "the cycle goes on");
}

#[test]
fn a_session_left_early_counts_what_ran_of_it() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    run_bar(&mut i, "exam 25 jan Reti ~10h");
    // a false start counts for nothing
    run_bar(&mut i, "pomodoro Reti");
    i.tick(i.now + 4 * MINUTE, 8.0);
    i.activate(Action::TimerCancel);
    assert!(i.study_log().is_empty());
    // stopped after twelve minutes
    run_bar(&mut i, "pomodoro Reti");
    i.tick(i.now + 12 * MINUTE, 8.0);
    i.activate(Action::TimerCancel);
    assert_eq!(i.deadlines()[0].done_min, 12);
    // skipped to the break
    run_bar(&mut i, "pomodoro Reti");
    i.tick(i.now + 7 * MINUTE, 8.0);
    i.activate(Action::PomodoroSkip);
    i.tick(i.now + 100, 8.0);
    assert_eq!(i.deadlines()[0].done_min, 19);
    // the break is not study, and neither stopping it nor replacing it adds anything
    i.tick(i.now + 3 * MINUTE, 8.0);
    run_bar(&mut i, "pomodoro Reti");
    assert_eq!(i.deadlines()[0].done_min, 19);
    // another one takes its place: the first is counted
    i.tick(i.now + 6 * MINUTE, 8.0);
    i.push(i.now, Input::StartStopwatch);
    assert_eq!(i.deadlines()[0].done_min, 25);
}

#[test]
fn a_plain_pomodoro_is_not_a_session_and_do_not_disturb_can_be_switched_off_during_one() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    i.push(i.now, Input::StartPomodoro);
    i.tick(i.now + MINUTE, 8.0);
    assert_eq!(i.auto_quiet_reason(), None);
    i.tick(i.now + 10 * MINUTE, 8.0);
    i.activate(Action::TimerCancel);
    assert!(i.study_log().is_empty(), "stopped, with nothing it was for");
    run_bar(&mut i, "pomodoro Reti");
    i.tick(i.now + MINUTE, 8.0);
    assert!(i.quiet());
    i.activate(Action::ToggleDnd);
    i.tick(i.now + 100, 8.0);
    assert!(!i.quiet(), "off means off");
}

#[test]
fn the_row_that_was_pressed_is_the_one_that_gets_the_time() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    run_bar(&mut i, "todo report due mon #reti ~5h");
    run_bar(&mut i, "exam 25 jan Reti orale #reti ~10h");
    let rows = i.plan_view().today;
    assert_eq!(rows.iter().map(|r| (r.what.as_str(), r.session.as_str())).collect::<Vec<_>>(), [("report", "reti"), ("Reti orale", "Reti orale")], "the course leads to the task, due first: the exam goes by its name");
    i.activate(Action::StudyStart(1));
    i.tick(i.now + 25 * MINUTE + 100, 8.0);
    assert_eq!((i.deadlines()[0].done_min, i.todos[0].est_min), (25, 300));
}

#[test]
fn the_plan_says_where_the_last_days_lead_for_each_exam_with_hours() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0); // Friday 15 January 2027; study days Monday to Friday and the weekend by default
    run_bar(&mut i, "exam 29 jan AE #ae ~20h");
    run_bar(&mut i, "exam 29 jan Reti 6cfu");
    let view = i.plan_view();
    let pace = view.deadlines.iter().find(|d| d.name == "AE").and_then(|d| d.pace).expect("hours and a day: a pace");
    assert_eq!((pace.now_per_day, pace.percent, pace.short_min), (0, 0, 1200));
    assert!(view.deadlines.iter().find(|d| d.name == "Reti").is_some_and(|d| d.pace.is_none()), "no hours, nothing to measure against");
    let plain = i.plan_height();
    // 3h in a week of measure, 13 study days after today
    run_bar(&mut i, "log 3h ae");
    let pace = i.plan_view().deadlines[0].pace.expect("still to do");
    assert_eq!((pace.now_per_day, pace.needed_per_day, pace.percent, pace.short_min), (26, 80, 42, 690));
    let (line, _) = crate::faces::pace_line(&pace);
    assert_eq!(line, "11h30 short: 42% by then · 26m a day, 1h20 needed");
    assert!(i.plan_height() >= plain);
    draw_frames(&mut i, 5);
    run_bar(&mut i, "log 17h ae");
    assert!(i.plan_view().deadlines[0].pace.is_none(), "all done");
}
