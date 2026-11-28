//! Every page and face draws, fits the window, and survives odd time steps.

use super::*;

#[test]
fn every_page_fits_in_the_fixed_surface() {
    // platforms that cannot resize their window smoothly (Windows) keep one window of MAX_SURFACE
    let mut mx = (0.0f64, 0.0f64);
    for page in [Page::Home, Page::Clip, Page::Timer, Page::Meeting, Page::Media, Page::Qs, Page::Notifs, Page::Settings, Page::Options, Page::Audio, Page::Today, Page::Send, Page::Activity, Page::Map, Page::Weather, Page::Cal, Page::Sys, Page::Focus, Page::Op, Page::Plan, Page::Shelf] {
        let mut i = Island::new(Config::default());
        i.data = data();
        // the shelf, the pins and the reminders waiting for a place, all full
        i.restore_shelf((0..40).map(|n| format!("/tmp/a-file-with-a-long-name-{n}.pdf")).collect());
        i.restore_places(vec![], (0..20).map(|n| island_core::places::Waiting { place: "home".into(), leaving: false, text: format!("thing {n}"), armed: false }).collect());
        // the Plan page at its fullest
        i.tick(T0, 8.0);
        for n in 0..12 {
            run_bar(&mut i, &format!("pin note {n}"));
        }
        for n in 0..8 {
            run_bar(&mut i, &format!("esame {} jan Corso{n} #c{n} 9cfu {}", 20 + n, if n % 2 == 0 { "~20h" } else { "" }));
            run_bar(&mut i, &format!("log 30m c{n}"));
        }
        i.notif = None;
        i.push(T0, Input::OpenPage(page));
        for _ in 0..400 {
            let t = i.now + 8;
            let fr = i.tick(t, 8.0);
            i.push(t, Input::OpenPage(page));
            mx = (mx.0.max(fr.surface.0), mx.1.max(fr.surface.1));
        }
    }
    assert!(mx.0 <= MAX_SURFACE.0 && mx.1 <= MAX_SURFACE.1, "a page needs {mx:?}, more than MAX_SURFACE {MAX_SURFACE:?}");
}

#[test]
fn every_face_draws_without_panicking_and_stays_finite() {
    let f = fonts();
    for page in [Page::Home, Page::Cal, Page::Qs, Page::Sys, Page::Notifs, Page::Media, Page::Timer, Page::Op, Page::Meeting] {
        let mut i = Island::new(Config::default());
        i.data = data();
        i.data.events = vec![Event { id: "e".into(), title: "Meeting".into(), start_ms: T0 + 5 * 60_000, end_ms: T0 + 3_600_000, url: Some("https://x".into()), ..Default::default() }];
        i.push(T0, Input::StartTimer(60));
        i.push(T0, Input::Progress("10 x".into()));
        i.push(T0, Input::Notification(Incoming { id: 1, app: "a".into(), summary: "s".into(), body: "b".into(), ..Default::default() }));
        i.push(T0, Input::OpenPage(page));
        for _ in 0..300 {
            let t = i.now + 8;
            let fr = i.tick(t, 8.0);
            assert!(fr.surface.0.is_finite() && fr.surface.1.is_finite() && fr.surface.0 < 2000.0 && fr.surface.1 < 1500.0, "{page:?}: {fr:?}");
            i.push(t, Input::OpenPage(page));
            let mut scene = Scene::new();
            i.draw(&mut scene, &f, fr.surface, 1.75);
        }
    }
}

#[test]
fn survives_absurd_time_steps() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::OpenPage(Page::Cal));
    for dt in [-1e9, f64::NAN, f64::INFINITY, 1e12, 0.0] {
        let fr = i.tick(i.now + 8, dt);
        assert!(fr.surface.0.is_finite() && fr.surface.1.is_finite(), "dt {dt}");
    }
    run(&mut i, 3000);
    let fr = i.tick(i.now + 8, 8.0);
    assert!(fr.surface.1 > 100.0, "the calendar opened: {fr:?}");
}

#[test]
fn every_new_face_draws() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    let reply = vec![NotifAction { key: "inline-reply".into(), label: "Reply".into() }];
    i.push(T0, Input::Notification(Incoming { actions: reply, progress: Some(40), ..incoming(1) }));
    i.activate(Action::NotifAction("inline-reply".into()));
    i.push(i.now, Input::Key(KeyInput::Text("hi".into())));
    i.data = data();
    i.data.media.as_mut().unwrap().players = 2;
    for _ in 0..200 {
        let fr = i.tick(i.now + 8, 8.0);
        let mut scene = Scene::new();
        i.draw(&mut scene, &f, fr.surface, 2.0);
    }
    assert_eq!(i.current_face(), Face::Reply);
}

#[test]
fn the_new_pages_draw() {
    let f = fonts();
    for page in [Page::Clip, Page::Focus, Page::Qs, Page::Sys] {
        let mut i = Island::new(Config::default());
        i.data = data();
        i.data.clipboard = vec!["a".into(); 7];
        i.data.focus = Some(Focus { active: true, end_epoch: Some(T0 / 1000 + 600) });
        i.data.system = Some(SystemStats { cores: vec![10.0; 12], ..Default::default() });
        for _ in 0..200 {
            let t = i.now + 8;
            let fr = i.tick(t, 8.0);
            i.push(t, Input::OpenPage(page));
            let mut scene = Scene::new();
            i.draw(&mut scene, &f, fr.surface, 1.0);
            assert!(fr.surface.0 < 1500.0 && fr.surface.1 < 1200.0 && fr.surface.1.is_finite(), "{page:?}");
        }
    }
}

#[test]
fn the_outline_follows_the_pill_and_its_bubble() {
    let mut i = Island::new(Config::default());
    draw_frames(&mut i, 100);
    let r = i.shape_rects();
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].2, r[0].3, r[0].4), (150.0, 28.0, 14.0), "a 150 x 28 pill with full-round ends");
    i.data = data();
    i.cfg.progress_drop = false;
    i.push(T0, Input::StartTimer(60));
    draw_frames(&mut i, 300);
    let r = i.shape_rects();
    assert_eq!(r.len(), 2, "the bubble is part of the outline");
    assert!(r[1].0 > r[0].0 + r[0].2, "to the right of the pill, past the gap");
}

#[test]
fn a_resting_line_asks_for_its_next_frame_only_when_it_moves() {
    let mut i = Island::new(Config::default());
    run(&mut i, 3000);
    let t = i.now + 8;
    i.marquee_wake.set(Some(t + 400));
    let f = i.tick(t, 8.0);
    assert!(!f.animating && f.next_ms == 400, "{f:?}");
    // woken at the end of the pause: it slides, at sixty frames a second and not at the screen's own rate
    i.marquee_wake.set(Some(t + 400));
    let f = i.tick(t + 400, 8.0);
    assert!(!f.animating && f.next_ms == MARQUEE_STEP_MS, "{f:?}");
    assert!(i.tick(t + 412, 8.0).next_ms > 100, "no line drawn: nothing to wake for");
}
