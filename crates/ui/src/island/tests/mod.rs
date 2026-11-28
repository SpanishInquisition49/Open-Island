//! Tests of the island through its inputs, one file a topic; what several of them use is here.

mod activities;
mod auth;
mod bar;
mod calendar;
mod cards;
mod drawing;
mod effects;
mod files;
mod media;
mod navigation;
mod notifications;
mod phone;
mod quiet;
mod recaps;
mod settings;
mod sky;
mod study;
mod system;
mod tasks;
mod timers;
mod travel;
mod tuck;
mod undo;

use super::*;
use crate::text::Fonts;
use island_core::calendar::Event;
use vello::Scene;
use island_core::pomodoro::Phase;

const T0: i64 = 1_800_000_000_000;

pub(super) fn fonts() -> Fonts {
    Fonts::from_db(&fontdb::Database::new())
}

fn run(i: &mut Island, ms: i64) {
    let mut t = i.now.max(T0);
    let end = t + ms;
    while t < end {
        t += 8;
        i.tick(t, 8.0);
    }
}

#[allow(clippy::field_reassign_with_default)]
fn data() -> Data {
    let mut d = Data::default();
    d.battery = Some(Battery { percent: 50.0, charging: false, on_ac: true });
    d.media = Some(Media { track_id: "a".into(), title: "Song".into(), artist: "Artist".into(), playing: true, length_ms: 200_000.0, sampled_at_ms: T0, can_seek: true, ..Default::default() });
    d
}

fn incoming(id: u32) -> Incoming {
    Incoming { id, app: "chat".into(), summary: "Anna".into(), body: "Are you coming?".into(), ..Default::default() }
}

fn closes(i: &mut Island) -> Vec<(u32, CloseReason)> {
    i.take_commands()
        .into_iter()
        .filter_map(|c| if let Command::NotificationClosed { id, reason } = c { Some((id, reason)) } else { None })
        .collect()
}

fn type_line(i: &mut Island, text: &str) {
    i.push(i.now, Input::Key(KeyInput::Text(text.into())));
}

fn type_text(i: &mut Island, t: &str) {
    i.push(i.now, Input::Key(KeyInput::Text(t.into())));
}

fn draw_frames(i: &mut Island, n: usize) -> (f64, f64) {
    let f = fonts();
    let mut surface = (0.0, 0.0);
    for _ in 0..n {
        let fr = i.tick(i.now + 8, 8.0);
        let mut scene = Scene::new();
        i.draw(&mut scene, &f, fr.surface, 1.0);
        surface = fr.surface;
    }
    surface
}

/// Presses the middle of the first drawn control with this action.
fn press(i: &mut Island, action: Action) -> bool {
    // a card's controls say which card they are on
    let is = |a: &Action| *a == action || matches!(a, Action::OnCard(_, inner) if **inner == action);
    let Some(h) = i.hits.iter().find(|h| is(&h.action)).map(|h| h.rect) else { return false };
    let cmds = i.pointer_down(i.now, h.center().x + i.pill_origin.0, h.center().y + i.pill_origin.1);
    i.pending.extend(cmds);
    true
}

fn card(i: &Island) -> Option<(String, String)> {
    i.notif.as_ref().map(|n| (n.summary.clone(), n.body.clone()))
}

fn type_in_bar(i: &mut Island, text: &str) {
    i.push(i.now.max(T0), Input::OpenBar);
    for c in text.chars() {
        i.push(i.now, Input::Key(KeyInput::Text(c.to_string())));
    }
}

fn commuter() -> Island {
    // T0 is Friday 08:00 UTC; lectures: Reti 09:30-11:00 today, a video call, and Reti again tomorrow at 09:30
    let mut i = Island::new(Config { commute_minutes: 60, travel_margin_ms: 0, meeting_alerts: false, ..Config::default() });
    let ev = |id: &str, at_min: i64, len: i64| Event { id: id.into(), title: id.into(), start_ms: T0 + at_min * 60_000, end_ms: T0 + (at_min + len) * 60_000, location: "Aula 3".into(), ..Default::default() };
    let call = Event { url: Some("https://meet.example/x".into()), ..ev("Call", 30, 30) };
    i.data.events = vec![call, ev("Reti", 90, 90), ev("Lab", 300, 120), ev("Reti domani", 24 * 60 + 90, 90)];
    i
}

fn on_battery(i: &mut Island, percent: f32) {
    i.data.battery = Some(Battery { percent, charging: false, on_ac: false });
}

fn run_bar(i: &mut Island, line: &str) {
    type_in_bar(i, line);
    i.push(i.now, Input::Key(KeyInput::Enter));
    i.notif = None;
}

/// Ticks and draws, as the platform does: the card's motion and its place come from the drawing.
fn card_frames(i: &mut Island, f: &Fonts, ms: i64) -> crate::island::Frame {
    let mut fr = i.tick(i.now.max(T0), 8.0);
    for _ in 0..(ms / 8).max(1) {
        fr = i.tick(i.now + 8, 8.0);
        i.draw(&mut Scene::new(), f, fr.surface, 1.0);
    }
    fr
}

/// The titles of the cards that have a place on screen, top to bottom.
fn up(i: &Island) -> Vec<&str> {
    i.notif.iter().chain(i.below.iter().map(|b| &b.n)).map(|n| n.summary.as_str()).collect()
}
