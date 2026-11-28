//! The settings and options pages.

use super::*;

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
            // switches and steppers go two to a row, so rows are what crowds a page
            let rows = toggles(tab).len().div_ceil(2) + steppers(tab).len().div_ceil(2) + text_rows(tab);
            assert!(!keys.is_empty() && rows <= 7, "{name}: {rows} rows");
            assert!(!tab_blurb(tab).is_empty(), "{name} has no blurb");
        }
    }
    // the tabs fit the page: names of at most six letters
    assert!(TABS.iter().all(|t| t.len() <= 6));
}

#[test]
fn the_settings_page_chooses_how_a_face_answers_a_password_request() {
    use crate::model::{AuthFaceUse, FaceChoice};
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    i.activate(Action::Open(Page::Settings));
    draw_frames(&mut i, 200);
    assert!(!i.hits.iter().any(|h| matches!(h.action, Action::FaceChoice(_))), "nothing to choose while the island asks no passwords");
    let plain = i.settings_content_height();
    i.data.config.face = Some(FaceChoice { chosen: AuthFaceUse::Ask, note: "Enter on the empty field".into() });
    assert!(i.settings_content_height() > plain);
    i.activate(Action::Open(Page::Settings));
    draw_frames(&mut i, 200);
    i.take_commands();
    assert!(press(&mut i, Action::FaceChoice(AuthFaceUse::Auto)));
    assert_eq!(i.take_commands(), vec![Command::SetOption { key: "polkit_face".into(), value: OptionValue::Text("auto".into()) }]);
    assert_eq!(i.data.config.face.as_ref().map(|f| f.chosen), Some(AuthFaceUse::Auto), "lit at once");
    draw_frames(&mut i, 5);
    assert!(press(&mut i, Action::FaceChoice(AuthFaceUse::No)));
    assert_eq!(i.take_commands(), vec![Command::SetOption { key: "polkit_face".into(), value: OptionValue::Text("off".into()) }]);
}

