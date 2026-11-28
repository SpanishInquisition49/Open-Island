//! The map, trips, trains, places and the weather.

use super::*;

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
fn a_reminder_waits_for_a_place_and_rings_on_arrival_and_departure() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    run_bar(&mut i, "remind me when i get home buy milk");
    assert!(i.places_saved().1.is_empty(), "home is not set yet: nothing to wait for");
    i.data.config.home_pos = Some((43.7000, 10.4000));
    let fix = |i: &mut Island, lat: f64, acc: f64| {
        i.data.position = Some(Position { lat, lon: 10.4000, accuracy_m: acc, at_ms: i.now, source: PosSource::Geoclue });
        // only what this reading brings (cards stack, so the history says it best)
        let before = i.history.len();
        i.tick(i.now + 1100, 8.0);
        i.tick(i.now + 8, 8.0);
        let new = i.history.len() - before;
        i.history.iter().take(new).find(|(c, _)| c.app == "Reminder").map(|(c, _)| (c.summary.clone(), c.body.clone()))
    };
    fix(&mut i, 43.7500, 30.0);
    run_bar(&mut i, "remind me when i get home buy milk");
    assert_eq!(i.places_saved().1.len(), 1);
    i.take_commands();
    assert_eq!(fix(&mut i, 43.7200, 30.0), None, "on the way");
    assert!(i.take_commands().contains(&Command::RefreshPosition) || i.place_asked_at > 0, "the position is asked while something waits");
    assert_eq!(fix(&mut i, 43.7001, 30.0), Some(("buy milk".into(), "You got home".into())));
    assert!(i.places_saved().1.is_empty(), "told once");
    // a place saved where one stands, then leaving it
    i.tick(i.now + 8, 8.0);
    run_bar(&mut i, "place uni");
    assert_eq!(i.places_saved().0.len(), 1);
    run_bar(&mut i, "remind me when i leave uni call sam");
    assert_eq!(fix(&mut i, 43.7001, 30.0), None);
    assert_eq!(fix(&mut i, 43.7100, 25_000.0), None, "too rough a reading");
    assert_eq!(fix(&mut i, 43.7100, 30.0), Some(("call sam".into(), "You left uni".into())));
    // unknown places and rough positions are said at once
    run_bar(&mut i, "remind me when i get to mars wave");
    assert!(i.places_saved().1.is_empty());
    i.data.position = None;
    run_bar(&mut i, "place gym");
    assert_eq!(i.places_saved().0.len(), 1, "no position: nothing saved");
    run_bar(&mut i, "unplace uni");
    assert!(i.places_saved().0.is_empty());
}

#[test]
fn a_place_reminder_too_many_is_refused_aloud_and_none_is_lost() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    i.data.config.home_pos = Some((43.7, 10.4));
    for n in 0..20 {
        run_bar(&mut i, &format!("remind me when i get home thing {n}"));
    }
    type_in_bar(&mut i, "remind me when i get home one too many");
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert_eq!(i.places_saved().1.len(), 20);
    assert!(i.notif.iter().chain(i.below.iter().map(|b| &b.n)).any(|n| n.summary == "Too many reminders waiting"));
}
