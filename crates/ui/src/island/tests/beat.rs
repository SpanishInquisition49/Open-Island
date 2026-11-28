//! The edges of the pill that move with the music.

use super::*;
use island_core::surface::{CREST, SWELL};

fn listening(cfg: Config) -> Island {
    let mut i = Island::new(cfg);
    i.data = data();
    run(&mut i, 3000);
    i
}

fn hear(i: &mut Island, low: f32) {
    let mut bands = vec![0.1; 8];
    (bands[0], bands[1]) = (low, low);
    i.push(i.now, Input::Spectrum(bands));
}

#[test]
fn a_beat_moves_the_edges_of_the_small_pill_which_come_to_rest_after() {
    // the top one stays on the screen
    const { assert!(CREST < crate::island::TOP_MARGIN) };
    let mut i = listening(Config::default());
    assert_eq!(i.current_face(), Face::MediaMini);
    assert!(i.rippling().is_none());
    let tall = i.tick(i.now + 8, 8.0).surface.1;
    // the surface has room for the edge, with or without a beat
    let room = crate::island::TOP_MARGIN + i.spring.h.pos + SWELL;
    assert!(tall >= room, "{tall} {room}");
    hear(&mut i, 0.9);
    let fr = i.tick(i.now + 33, 33.0);
    assert!(i.rippling().is_some() && fr.next_ms <= 40 && !fr.animating, "{}", fr.next_ms);
    assert_eq!(fr.surface.1, tall);
    // no more sound heard: it goes on by itself for a moment, and stops
    run(&mut i, 300);
    assert!(i.rippling().is_some());
    run(&mut i, 4000);
    let fr = i.tick(i.now + 8, 8.0);
    assert!(i.rippling().is_none() && fr.surface.1 == tall, "the surface stays as it is between beats");
}

#[test]
fn paused_music_settles_and_gives_its_room_back() {
    let mut i = listening(Config::default());
    hear(&mut i, 0.9);
    let tall = i.tick(i.now + 33, 33.0).surface.1;
    i.data.media.as_mut().unwrap().playing = false;
    hear(&mut i, 0.9);
    assert!(i.tick(i.now + 33, 33.0).surface.1 == tall && i.rippling().is_some(), "still swinging");
    run(&mut i, 4000);
    let fr = i.tick(i.now + 8, 8.0);
    assert!(i.rippling().is_none() && fr.surface.1 < tall);
}

#[test]
fn the_edges_stay_put_when_switched_off_or_open() {
    let mut off = listening(Config { pill_pulse: false, ..Config::default() });
    hear(&mut off, 0.9);
    off.tick(off.now + 33, 33.0);
    assert!(off.rippling().is_none());
    let mut open = listening(Config::default());
    hear(&mut open, 0.9);
    open.push(open.now, Input::OpenPage(Page::Cal));
    run(&mut open, 800);
    hear(&mut open, 0.9);
    open.tick(open.now + 33, 33.0);
    assert!(open.rippling().is_none());
}

#[test]
fn the_pill_ripples_as_a_card_lets_go_of_it_and_comes_back() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    run(&mut i, 2000);
    let room = |i: &Island, fr: &Frame| assert!(i.rippling().is_none() || fr.surface.1 >= crate::island::TOP_MARGIN + i.spring.h.pos + SWELL, "{:?}", fr.surface);
    i.push(i.now, Input::Notification(incoming(1)));
    let mut rippled = false;
    for _ in 0..75 {
        let fr = card_frames(&mut i, &f, 8);
        rippled |= i.rippling().is_some();
        room(&i, &fr);
    }
    assert!(rippled, "as the drop lets go");
    card_frames(&mut i, &f, 3000);
    assert!(i.rippling().is_none() && i.has_card(), "at rest while the card stays");
    i.push(i.now, Input::DismissCard);
    let mut after = vec![];
    for _ in 0..500 {
        let fr = card_frames(&mut i, &f, 8);
        room(&i, &fr);
        after.push((i.rippling().is_some(), i.has_card(), fr.next_ms));
    }
    let back = after.iter().position(|a| a.0).expect("as the drop comes back");
    assert!(back > 15, "not before it is there: {back}");
    assert!(after.iter().any(|a| a.0 && !a.1 && a.2 <= 16), "it goes on, smoothly, after the card is gone");
    assert!(!after.last().unwrap().0);
}

#[test]
fn drops_leave_the_edge_alone_when_switched_off() {
    let f = fonts();
    let mut i = Island::new(Config { card_ripples: false, ..Config::default() });
    run(&mut i, 2000);
    i.push(i.now, Input::Notification(incoming(1)));
    for n in 0..600 {
        card_frames(&mut i, &f, 8);
        assert!(i.rippling().is_none());
        if n == 300 {
            i.push(i.now, Input::DismissCard);
        }
    }
}
