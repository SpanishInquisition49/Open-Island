//! The short animations of single events (`effects.rs`).

use super::fonts;
use crate::island::effects::{BURST_MS, GULP_MS};
use crate::island::liquid::Source;
use crate::island::*;
use vello::Scene;

const T0: i64 = 1_800_000_000_000;

/// Runs the island for `ms`, drawing every frame as a backend would (the cards are measured as they are drawn).
fn play(i: &mut Island, ms: i64) {
    let f = fonts();
    let mut t = i.now.max(T0);
    let end = t + ms;
    while t < end {
        t += 8;
        let frame = i.tick(t, 8.0);
        assert!(frame.surface.0.is_finite() && frame.surface.1.is_finite(), "{frame:?}");
        i.draw(&mut Scene::new(), &f, (frame.surface.0.max(600.0), frame.surface.1), 1.0);
    }
}

fn island() -> Island {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Peek);
    i.expand_until = 0;
    i
}

fn card(i: &Island) -> &card::Card {
    i.cards.stack.first().expect("a card")
}

#[test]
fn the_drop_of_a_job_that_ends_goes_on_to_be_its_card() {
    let mut i = island();
    i.push(T0, Input::Progress("62 backup".into()));
    play(&mut i, 2000);
    assert_eq!(i.fill.as_ref().map(|f| f.source.clone()), Some(Source::Job(String::new())), "the drop hangs");
    i.push(i.now, Input::ProgressDone("backup".into()));
    assert!(i.fill.is_none(), "it has left its place");
    play(&mut i, 8);
    let from = card(&i).arrival.expect("the card starts as that drop");
    assert!(from.from.0 < -75.0 && from.from.1 < 0.0, "at the left of the pill, level with it: {from:?}");
    assert!(!from.shake && from.tint.is_some());
    let s = card(&i).anim.as_ref().unwrap().shape(0.0);
    assert!((s.x - from.from.0).abs() < 1e-6, "{s:?}");
    play(&mut i, 1500);
    let c = card(&i);
    let s = c.anim.as_ref().unwrap().shape(c.age(i.now));
    assert!(s.x.abs() < 0.5 && s.opened > 0.99, "open under the pill: {s:?}");
}

#[test]
fn a_failed_job_arrives_trembling() {
    let mut i = island();
    i.push(T0, Input::Progress("30 deploy".into()));
    play(&mut i, 2000);
    i.push(i.now, Input::ProgressFail("deploy".into()));
    play(&mut i, 8);
    let from = card(&i).arrival.expect("from the drop");
    assert!(from.shake);
    assert_eq!(from.tint, Some(island_core::notify::Kind::Fail.color()));
}

#[test]
fn the_drop_of_a_countdown_becomes_the_card_that_says_time_is_up() {
    let mut i = island();
    i.push(T0, Input::StartTimer(3));
    play(&mut i, 4000);
    assert!(card(&i).arrival.is_some(), "the ring of the timer came down as the card");
}

#[test]
fn behind_another_card_the_new_one_drops_from_it_as_before() {
    let mut i = island();
    i.push(T0, Input::Progress("62 backup".into()));
    i.push(T0, Input::Notification(Incoming { id: 1, app: "Mail".into(), summary: "hi".into(), expire_ms: Some(20_000), ..Default::default() }));
    play(&mut i, 2000);
    i.push(i.now, Input::ProgressDone("backup".into()));
    play(&mut i, 8);
    assert!(i.cards.stack.len() == 2 && i.cards.stack.iter().all(|c| c.arrival.is_none()));
}

#[test]
fn a_sent_file_leaves_as_a_drop_and_a_failed_send_does_not() {
    let act = |label: &str| island_core::progress::Activity { id: "send".into(), label: label.into(), detail: String::new(), icon: None, tint: None, frac: Some(0.5), file: None, busy: false };
    for (state, label, shot) in [(progress::State::Done, "Sent", true), (progress::State::Fail, "Send failed", false), (progress::State::Done, "Nothing to send", false)] {
        let mut i = island();
        i.push(T0, Input::Activity(progress::State::Run, act("Sending to Pixel")));
        play(&mut i, 2000);
        i.push(i.now, Input::Activity(state, act(label)));
        assert_eq!(i.fx.shot.is_some(), shot, "{label}");
        play(&mut i, 2000);
        assert!(i.fx.shot.is_none(), "gone");
    }
}

#[test]
fn what_was_held_comes_out_as_drops_that_gather_into_the_summary() {
    let mut i = island();
    i.push(T0, Input::SetDnd(true));
    play(&mut i, 500);
    for n in 0..8 {
        i.push(i.now, Input::Notification(Incoming { id: n + 1, app: if n % 2 == 0 { "Telegram" } else { "Mail" }.into(), summary: format!("m{n}"), ..Default::default() }));
    }
    assert!(i.cards.stack.is_empty(), "held");
    i.push(i.now, Input::SetDnd(false));
    play(&mut i, 40);
    let burst = i.fx.burst.as_ref().expect("the drops come out");
    assert_eq!(burst.drops.len(), effects::BURST_MAX);
    assert!(card(&i).born > i.now + BURST_MS / 2, "the card waits for them");
    let (start, apart, end) = (burst.place(0, 0, 28.0), burst.place(0, 700, 28.0), burst.place(0, BURST_MS, 28.0));
    assert!(start.y < 0.0 && start.x.abs() < 1.0, "from inside the pill: {start:?}");
    assert!(apart.x < -60.0 && apart.y > 12.0 && apart.blend < 0.5, "spread out below it: {apart:?}");
    let from = card(&i).arrival.unwrap().from;
    assert!(end.x.abs() < 0.5 && (end.y - from.1).abs() < 0.5 && (end.d - island_core::droplet::DROP).abs() < 0.5, "where the card starts: {end:?} {from:?}");
    play(&mut i, BURST_MS + 100);
    assert!(i.fx.burst.is_none() && card(&i).anim.is_some());
}

#[test]
fn coming_back_to_the_machine_is_told_by_the_card_alone() {
    let mut i = island();
    play(&mut i, 500);
    i.push(i.now, Input::Notification(Incoming { id: 1, app: "Mail".into(), summary: "m".into(), expire_ms: Some(2000), ..Default::default() }));
    play(&mut i, 4000);
    i.push(i.now, Input::Returned { away_ms: 120_000 });
    play(&mut i, 40);
    assert!(i.fx.burst.is_none());
}

#[test]
fn a_copied_code_flies_into_the_pill() {
    let mut i = island();
    i.push(T0, Input::Notification(Incoming { id: 7, app: "Bank".into(), summary: "Code".into(), body: "Your verification code is 482913".into(), ..Default::default() }));
    play(&mut i, 1500);
    let at = card(&i).code_at.expect("the code is on the card");
    let out = i.activate(Action::OnCard(7, Box::new(Action::CopyCode("482913".into()))));
    assert!(matches!(out.first(), Some(Command::CopyCode(c)) if c == "482913"), "{out:?}");
    let f = i.fx.flight.as_ref().expect("in flight");
    assert_eq!((f.text.as_str(), f.from), ("482 913", at));
    assert_eq!(f.way(0, i.now), 0.0);
    assert!(f.way(0, f.end()) == 1.0 && f.way(6, f.end()) == 1.0);
    assert_eq!(f.said(i.now), 0.0);
    assert!(f.said(f.end() - 400) > 0.99);
    play(&mut i, 2500);
    assert!(i.fx.flight.is_none());
}

#[test]
fn a_device_says_its_battery_with_a_ring_also_when_it_comes_late() {
    let headphones = |battery| Device { id: "aa".into(), name: "Nothing Ear".into(), kind: island_core::bluetooth::Kind::Earbuds, battery };
    let mut i = island();
    i.push(T0, Input::DeviceConnected(headphones(Some(85))));
    play(&mut i, 1500);
    assert_eq!((card(&i).n.ring, card(&i).n.progress, card(&i).n.body.as_str()), (Some(85), None, "Connected"));

    let mut i = island();
    i.push(T0, Input::DeviceConnected(headphones(None)));
    play(&mut i, 1500);
    assert_eq!(card(&i).n.ring, None);
    i.data.devices = vec![headphones(Some(30))];
    play(&mut i, 100);
    assert_eq!(i.cards.stack.len(), 1, "the same card");
    assert_eq!(card(&i).n.ring, Some(30));
    assert!(i.fx.awaiting_battery.is_none());
}

#[test]
fn a_dragged_file_is_reached_for_swallowed_and_then_the_devices_show() {
    let mut i = island();
    play(&mut i, 500);
    let pill = i.pill_rect();
    i.push(i.now, Input::FileDrag(Some((pill.0 + 20.0, pill.1 + 20.0))));
    play(&mut i, 600);
    assert_eq!(i.current_face(), Face::Catch);
    let area = *i.input_rects().last().unwrap();
    assert!(area.2 > i.pill_rect().2 + 60.0 && area.3 > i.pill_rect().3 + 40.0, "room to move about: {area:?}");
    // the pointer below the pill: the drop follows it down
    let pill = i.pill_rect();
    i.push(i.now, Input::FileDrag(Some((pill.0 + 40.0, pill.1 + pill.3 + 36.0))));
    play(&mut i, 600);
    let c = i.fx.catch.as_ref().unwrap();
    assert!(c.tongue.1 > pill.3 + 25.0 && (c.tongue.0 - 48.0).abs() < 6.0, "{:?} {pill:?}", c.tongue);
    i.push(i.now, Input::FileDropped);
    i.push(i.now, Input::FileDrag(None));
    i.push(i.now, Input::SendFiles(vec!["/tmp/a.pdf".into()]));
    play(&mut i, GULP_MS - 100);
    assert_eq!(i.current_face(), Face::Catch, "still swallowing, although the page is asked for");
    play(&mut i, 900);
    assert!(i.fx.catch.is_none());
    assert_eq!(i.current_face(), Face::Send);
}

#[test]
fn a_drag_that_goes_away_leaves_nothing_behind() {
    let mut i = island();
    play(&mut i, 500);
    i.push(i.now, Input::FileDrag(Some((100.0, 10.0))));
    play(&mut i, 400);
    i.push(i.now, Input::FileDrag(None));
    play(&mut i, 900);
    assert!(i.fx.catch.is_none());
    assert_eq!(i.current_face(), Face::Idle);
    // nor does one that never said it left
    i.push(i.now, Input::FileDrag(Some((100.0, 10.0))));
    play(&mut i, 100);
    i.now += 61_000;
    play(&mut i, 100);
    assert!(i.fx.catch.is_none());
}

#[test]
fn nothing_of_this_keeps_asking_for_frames_at_rest() {
    let mut i = island();
    play(&mut i, 3000);
    assert!(!i.fx.moving());
    assert!(!run_frame(&mut i), "at rest");
}

fn run_frame(i: &mut Island) -> bool {
    let t = i.now + 8;
    i.tick(t, 8.0).animating
}
