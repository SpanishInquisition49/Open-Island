//! The weather inside the small pill.

use super::*;
use crate::model::{PosSource, Position, Weather};

fn outside(code: i32, on_ac: bool) -> Island {
    let mut i = Island::new(Config::default());
    i.data.weather = Some(Weather { code, day: true, ..Default::default() });
    i.data.battery = Some(Battery { percent: 80.0, charging: false, on_ac });
    run(&mut i, 2000);
    i
}

/// What a frame draws: the shapes, their colours and where they are put.
fn picture(i: &mut Island) -> Vec<u32> {
    let f = fonts();
    let fr = i.tick(i.now + 8, 8.0);
    let mut s = Scene::new();
    i.draw(&mut s, &f, fr.surface, 1.0);
    let e = s.encoding();
    let places = e.transforms.iter().flat_map(|t| t.matrix.iter().chain(&t.translation).map(|v| v.to_bits()));
    e.path_data.iter().chain(&e.draw_data).copied().chain(places).collect()
}

#[test]
fn rain_falls_in_the_resting_pill_at_thirty_frames_a_second() {
    let mut wet = outside(63, true);
    let fr = wet.tick(wet.now + 8, 8.0);
    assert!(wet.pill_sky().is_some() && !fr.animating && fr.next_ms == 33, "{}", fr.next_ms);
    let first = picture(&mut wet);
    run(&mut wet, 100);
    assert_ne!(picture(&mut wet), first, "it moves");
    let mut dry = outside(100, true);
    let fr = dry.tick(dry.now + 8, 8.0);
    assert!(dry.pill_sky().is_none() && fr.next_ms >= 500, "{}", fr.next_ms);
    assert!(picture(&mut dry).len() < first.len());
}

#[test]
fn it_stops_where_nobody_sees_it_and_where_it_is_switched_off() {
    // an open page
    let mut i = outside(63, true);
    i.push(i.now, Input::OpenPage(Page::Cal));
    run(&mut i, 800);
    assert!(i.pill_sky().is_none());
    // slid away from a window
    let mut i = outside(73, true);
    i.set_cover(true, false);
    run(&mut i, 2000);
    let fr = i.tick(i.now + 8, 8.0);
    assert!(i.pill_sky().is_none() && fr.next_ms >= 500);
    // switched off
    let mut i = Island::new(Config { weather_in_pill: false, ..Config::default() });
    i.data.weather = Some(Weather { code: 63, ..Default::default() });
    run(&mut i, 500);
    assert!(i.pill_sky().is_none());
    // the rain stops
    let mut i = outside(63, true);
    i.data.weather = Some(Weather { code: 100, ..Default::default() });
    run(&mut i, 100);
    assert!(i.pill_sky().is_none());
}

#[test]
fn on_battery_it_shows_as_it_begins_and_then_now_and_then() {
    let mut i = outside(63, false);
    assert!(i.pill_sky().is_some(), "as it begins");
    run(&mut i, 8000);
    let fr = i.tick(i.now + 8, 8.0);
    assert!(i.pill_sky().is_none() && fr.next_ms >= 500, "{}", fr.next_ms);
    i.tick(i.now + 114_000, 8.0);
    assert!(i.pill_sky().is_some(), "two minutes on");
}

#[test]
fn a_sky_asked_for_is_shown_for_half_a_minute_whatever_the_weather() {
    let mut i = Island::new(Config { weather_in_pill: false, ..Config::default() });
    i.data.battery = Some(Battery { percent: 20.0, charging: false, on_ac: false });
    run(&mut i, 500);
    i.push(i.now, Input::PreviewSky(island_core::sky::named("snow")));
    run(&mut i, 20_000);
    assert_eq!(i.pill_sky().map(|s| s.kind), Some(island_core::sky::Kind::Snow));
    run(&mut i, 11_000);
    assert!(i.pill_sky().is_none());
    i.push(i.now, Input::PreviewSky(island_core::sky::named("rain")));
    run(&mut i, 100);
    i.push(i.now, Input::PreviewSky(None));
    run(&mut i, 100);
    assert!(i.pill_sky().is_none());
}

#[test]
fn the_sun_stands_in_the_pill_and_only_costs_frames_while_it_shines() {
    let mut i = outside(0, true);
    let fr = i.tick(i.now + 8, 8.0);
    assert_eq!(i.pill_sky().map(|s| s.kind), Some(island_core::sky::Kind::Sun));
    assert!(i.sky_shine().is_some() && fr.next_ms == 50, "it shines as it comes out: {}", fr.next_ms);
    let shining = picture(&mut i);
    run(&mut i, 200);
    assert_ne!(picture(&mut i), shining, "and moves");
    run(&mut i, 6000);
    let fr = i.tick(i.now + 8, 8.0);
    assert!(i.sky_shine().is_none() && fr.next_ms >= 500, "at rest: {}", fr.next_ms);
    assert!(i.pill_sky().is_some(), "still there");
    // two frames at rest are the same picture, so nothing is rendered again
    let still = picture(&mut i);
    run(&mut i, 3000);
    assert_eq!(picture(&mut i), still);
    i.tick(i.now + 21_000, 8.0);
    assert!(i.sky_shine().is_some(), "half a minute on it shines again");
}

#[test]
fn a_clear_night_has_the_moon_and_stars_which_rest_between_twinkles() {
    let mut i = Island::new(Config::default());
    i.data.weather = Some(Weather { code: 0, day: false, ..Default::default() });
    i.data.battery = Some(Battery { percent: 80.0, charging: false, on_ac: true });
    run(&mut i, 500);
    assert_eq!(i.pill_sky().map(|s| s.kind), Some(island_core::sky::Kind::Night));
    assert!(i.sky_shine().is_some(), "they twinkle as they come out");
    run(&mut i, 7000);
    let fr = i.tick(i.now + 8, 8.0);
    assert!(i.sky_shine().is_none() && fr.next_ms >= 500, "{}", fr.next_ms);
    // two moments of rest are the same picture, so nothing is rendered again
    let tonight = picture(&mut i);
    run(&mut i, 3000);
    assert_eq!(picture(&mut i), tonight);
    // the moon changes with the days
    i.tick(i.now + 6 * 86_400_000, 8.0);
    run(&mut i, 7000);
    assert_ne!(picture(&mut i), tonight);
    assert!(!i.south());
    i.data.position = Some(Position { lat: -33.9, lon: 18.4, accuracy_m: 30.0, at_ms: i.now, source: PosSource::Geoclue });
    assert!(i.south());
    // morning comes
    i.data.weather = Some(Weather { code: 0, day: true, ..Default::default() });
    run(&mut i, 100);
    assert_eq!(i.pill_sky().map(|s| s.kind), Some(island_core::sky::Kind::Sun));
}

#[test]
fn clouds_and_fog_stand_in_the_pill_and_drift_on_now_and_then() {
    use island_core::sky::Kind;
    for (code, day, kind) in [(2, true, Kind::Partly), (2, false, Kind::Partly), (3, true, Kind::Overcast), (45, false, Kind::Fog)] {
        let mut i = Island::new(Config::default());
        i.data.weather = Some(Weather { code, day, ..Default::default() });
        run(&mut i, 500);
        assert_eq!(i.pill_sky().map(|s| (s.kind, s.day)), Some((kind, day)));
        let first = picture(&mut i);
        run(&mut i, 7000);
        let fr = i.tick(i.now + 8, 8.0);
        assert!(fr.next_ms >= 500, "{kind:?} at rest: {}", fr.next_ms);
        let rested = picture(&mut i);
        assert!(!rested.is_empty() && (rested != first || code == 2), "{kind:?} moved");
        run(&mut i, 3000);
        assert_eq!(picture(&mut i), rested, "{kind:?} still");
    }
}

#[test]
fn unplugging_does_not_make_the_clouds_jump() {
    let mut i = Island::new(Config::default());
    i.data.weather = Some(Weather { code: 3, day: true, ..Default::default() });
    i.data.battery = Some(Battery { percent: 80.0, charging: false, on_ac: true });
    // long enough for several steps
    run(&mut i, 500);
    i.tick(i.now + 100_000, 8.0);
    run(&mut i, 6000);
    let (steps, rested) = (i.sky_drift(), picture(&mut i));
    assert!(steps >= 3.0, "{steps}");
    i.data.battery = Some(Battery { percent: 80.0, charging: false, on_ac: false });
    run(&mut i, 9000);
    assert_eq!(i.sky_drift(), steps);
    assert_eq!(picture(&mut i), rested);
}

/// The forecast changes under the island.
fn turns(i: &mut Island, code: i32) {
    i.data.weather = Some(Weather { code, day: true, ..Default::default() });
}

#[test]
fn snow_piles_up_while_it_snows_and_melts_after() {
    let mut i = outside(73, true);
    let early = i.residue().map(|r| r.0).unwrap();
    assert!(early > 0.0 && early < 0.2, "{early}");
    run(&mut i, 30_000);
    assert_eq!(i.residue().map(|r| (r.0, r.1)), Some((1.0, 0.0)));
    // overcast now: it stays for a while, going down, with a few frames a second
    turns(&mut i, 100);
    run(&mut i, 10_000);
    let fr = i.tick(i.now + 8, 8.0);
    let half = i.residue().map(|r| r.0).unwrap();
    assert!(half > 0.3 && half < 0.7 && fr.next_ms == 250, "{half} {}", fr.next_ms);
    run(&mut i, 11_000);
    let fr = i.tick(i.now + 8, 8.0);
    assert!(i.residue().is_none() && fr.next_ms >= 500, "{}", fr.next_ms);
}

#[test]
fn rain_leaves_drops_that_slide_off_when_it_stops() {
    let mut i = outside(63, true);
    run(&mut i, 5_000);
    let (_, wet, at) = i.residue().unwrap();
    assert_eq!(wet, 1.0);
    turns(&mut i, 100);
    i.tick(i.now + 8, 8.0);
    let (_, _, then) = i.residue().unwrap();
    assert!((0..100).contains(&(then - at)), "they go on from where they were: {at} {then}");
    let fr = i.tick(i.now + 8, 8.0);
    assert_eq!(fr.next_ms, 50);
    let first = picture(&mut i);
    run(&mut i, 2_000);
    assert!(i.residue().is_some_and(|r| r.1 < 0.8) && picture(&mut i) != first);
    run(&mut i, 7_000);
    assert!(i.residue().is_none());
}

#[test]
fn on_battery_the_snow_and_the_drops_stay_still_between_two_falls() {
    for code in [63, 73] {
        let mut i = outside(code, false);
        run(&mut i, 40_000);
        assert!(i.pill_sky().is_none(), "nothing falls now");
        assert!(i.residue().is_some(), "{code}: but it has left something");
        let fr = i.tick(i.now + 8, 8.0);
        assert!(fr.next_ms >= 500, "{code}: {}", fr.next_ms);
        let still = picture(&mut i);
        run(&mut i, 20_000);
        assert_eq!(picture(&mut i), still, "{code}");
    }
}

#[test]
fn nothing_is_left_on_an_open_page_or_with_the_weather_off() {
    let mut i = outside(73, true);
    run(&mut i, 30_000);
    i.push(i.now, Input::OpenPage(Page::Cal));
    run(&mut i, 800);
    assert!(i.residue().is_none());
    let mut off = Island::new(Config { weather_in_pill: false, ..Config::default() });
    turns(&mut off, 73);
    run(&mut off, 30_000);
    assert!(off.residue().is_none());
    // one asked for to look at leaves its snow too, which goes when it is switched off
    off.push(off.now, Input::PreviewSky(island_core::sky::named("snow")));
    run(&mut off, 10_000);
    assert!(off.residue().is_some());
    off.push(off.now, Input::PreviewSky(None));
    run(&mut off, 100);
    assert!(off.residue().is_none());
}
