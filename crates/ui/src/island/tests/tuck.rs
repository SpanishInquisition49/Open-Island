//! Out of the way of a window over the island's place.

use super::*;

const SECOND: i64 = 1000;

/// A resting island that has been drawn, with a window over its place from now on.
fn covered() -> Island {
    let mut i = Island::new(Config::default());
    run(&mut i, SECOND);
    draw_frames(&mut i, 2);
    i.set_cover(true, false);
    i
}

fn gone(i: &mut Island) {
    run(i, 2 * SECOND);
    assert!(i.out_of_sight());
}

#[test]
fn the_island_leaves_shortly_after_a_window_comes_over_it_and_returns_when_it_goes() {
    let mut i = covered();
    run(&mut i, 400);
    assert!(!i.out_of_sight() && i.tuck_rise() == 0.0, "not at once");
    gone(&mut i);
    let f = fonts();
    let fr = i.tick(i.now + 8, 8.0);
    let mut scene = Scene::new();
    i.draw(&mut scene, &f, fr.surface, 1.0);
    assert!(scene.encoding().is_empty(), "nothing is drawn");
    assert!(i.shape_rects().is_empty() && !fr.animating && fr.next_ms >= 500, "no frames to speak of: {}", fr.next_ms);
    // only the strip at the very top calls it back
    let input = i.input_rects();
    assert_eq!(input.len(), 1);
    assert_eq!((input[0].1, input[0].3), (0.0, 3.0));
    i.set_cover(false, false);
    run(&mut i, 400);
    assert!(!i.out_of_sight() && i.tuck_rise() == 0.0);
}

#[test]
fn the_pointer_at_the_top_brings_it_back_and_it_stays_while_the_pointer_does() {
    let mut i = covered();
    gone(&mut i);
    draw_frames(&mut i, 1);
    let (x, _, w, _) = i.input_rects()[0];
    i.pointer_moved(i.now, x + w / 2.0, 1.0);
    run(&mut i, 400);
    assert_eq!(i.tuck_rise(), 0.0, "back in its place");
    // the way from the edge down to the pill takes the pointer too
    let (_, py, _, _) = i.pill_rect();
    assert!(i.input_rects().iter().any(|r| r.1 == 0.0 && r.3 >= py), "{:?}", i.input_rects());
    run(&mut i, 5 * SECOND);
    assert!(!i.out_of_sight(), "as long as the pointer is there");
    i.pointer_left(i.now);
    run(&mut i, 400);
    assert!(!i.out_of_sight(), "a moment of grace");
    gone(&mut i);
}

#[test]
fn a_platform_that_watches_the_pointer_itself_can_call_it() {
    let mut i = covered();
    gone(&mut i);
    i.set_cover(true, true);
    run(&mut i, 400);
    assert_eq!(i.tuck_rise(), 0.0);
    i.set_cover(true, false);
    gone(&mut i);
}

#[test]
fn what_it_has_to_show_comes_out_and_then_it_leaves_again() {
    let mut i = covered();
    gone(&mut i);
    i.push(i.now, Input::Notification(incoming(1)));
    run(&mut i, 600);
    assert!(i.has_card() && i.tuck_rise() == 0.0, "a notification shows");
    run(&mut i, 8 * SECOND);
    assert!(!i.has_card());
    gone(&mut i);
    // the volume bar, the command bar asked for from the keyboard, a page asked for
    i.push(i.now, Input::Osd { kind: OsdKind::Volume, level: 0.5, muted: false });
    run(&mut i, 600);
    assert_eq!(i.tuck_rise(), 0.0, "the volume");
    gone_after(&mut i, 6);
    i.push(i.now, Input::OpenBar);
    run(&mut i, 600);
    assert_eq!(i.tuck_rise(), 0.0, "the bar");
    i.push(i.now, Input::Key(KeyInput::Escape));
    gone_after(&mut i, 6);
    i.push(i.now, Input::OpenPage(Page::Cal));
    run(&mut i, 600);
    assert_eq!(i.tuck_rise(), 0.0, "a page");
    gone_after(&mut i, 12);
}

fn gone_after(i: &mut Island, seconds: i64) {
    run(i, seconds * SECOND);
    assert!(i.out_of_sight(), "left again");
}

#[test]
fn a_resting_activity_hides_with_it_and_the_sound_is_not_read_for_bars_nobody_sees() {
    let mut i = Island::new(Config::default());
    i.data = data();
    run(&mut i, SECOND);
    assert!(i.wants_spectrum());
    i.set_cover(true, false);
    gone(&mut i);
    assert!(!i.wants_spectrum());
}

#[test]
fn switched_off_it_stays_where_it_is() {
    let mut i = Island::new(Config { hide_when_covered: false, ..Config::default() });
    i.set_cover(true, false);
    run(&mut i, 3 * SECOND);
    assert!(!i.out_of_sight() && i.tuck_rise() == 0.0);
    assert_eq!(i.input_rects().len(), 1, "no strip, no bridge");
}
