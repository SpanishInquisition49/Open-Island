//! Getting around: the dock, the grid, the keyboard, hovering and clicking.

use super::*;

/// Rests the pointer on the pill (`bubble` = on the side bubble) and lets the intent delay pass.
fn hover(i: &mut Island, bubble: bool) {
    let (px, py, _, ph) = i.pill_rect();
    let x = if bubble { px + i.spring.w.pos + BUBBLE_GAP + BUBBLE_W / 2.0 } else { px + i.spring.w.pos / 2.0 };
    i.pointer_moved(i.now, x, py + ph / 2.0);
    draw_frames(i, 60);
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
