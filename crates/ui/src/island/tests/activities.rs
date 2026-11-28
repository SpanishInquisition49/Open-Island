//! Long jobs, scripts' activities and what hangs beside the pill.

use super::*;

fn run_frame(i: &mut Island, ms: i64) -> crate::island::Frame {
    let mut f = i.tick(i.now, 0.0);
    for _ in 0..(ms / 8).max(1) {
        f = i.tick(i.now + 8, 8.0);
    }
    f
}

#[test]
fn progress_is_a_small_pill_and_its_end_is_a_card() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Progress("20 Backup".into()));
    run(&mut i, 600);
    assert_eq!((i.current_face(), i.shown_side), (Face::Idle, Some(Side::Op)), "no card to say it began");
    assert!(up(&i).is_empty());
    i.push(i.now, Input::ProgressDone("Backup done".into()));
    run(&mut i, 600);
    assert_eq!(up(&i), ["Backup done"]);
    assert!(i.ops.op.is_none(), "the card says it: the small pill goes at once");
    assert_eq!(i.shown_side, None);
}

#[test]
fn a_shell_command_becomes_a_pill_only_after_its_threshold_and_ends_green_or_red() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::CommandStart { id: "1".into(), label: "make".into(), after_secs: 10 });
    run(&mut i, 5000);
    assert!(i.ops.op.is_none(), "a short command never shows");
    i.push(i.now, Input::CommandEnd { id: "1".into(), code: 0 });
    run(&mut i, 500);
    assert!(i.ops.op.is_none(), "and says nothing when it ends");

    let t = i.now;
    i.push(t, Input::CommandStart { id: "2".into(), label: "cargo build".into(), after_secs: 10 });
    run(&mut i, 11_000);
    let op = i.ops.op.clone().expect("past the threshold");
    assert_eq!((op.label.as_str(), op.state), ("cargo build", progress::State::Run));
    run(&mut i, 100_000);
    assert!(matches!(&i.ops.op, Some(o) if o.state == progress::State::Run), "refreshed, not dropped as stale after two minutes");
    i.push(i.now, Input::CommandEnd { id: "2".into(), code: 0 });
    let card = i.notif.clone().expect("how it ended");
    assert_eq!(card.kind, Some(island_core::notify::Kind::Ok));
    assert!(card.summary.starts_with("cargo build · 1m 5"), "{}", card.summary);
    assert!(i.ops.op.is_none());

    i.push(i.now, Input::ProgressClear);
    let t = i.now;
    i.push(t, Input::CommandStart { id: "3".into(), label: "pytest".into(), after_secs: 2 });
    run(&mut i, 3000);
    i.push(i.now, Input::CommandEnd { id: "3".into(), code: 2 });
    let card = i.below.last().map(|b| b.n.clone()).or(i.notif.clone()).unwrap();
    assert_eq!(card.kind, Some(island_core::notify::Kind::Fail));
    assert!(card.summary.starts_with("pytest · exit 2 · "), "{}", card.summary);
}

#[test]
fn a_second_command_waits_for_the_pill_to_be_free() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::CommandStart { id: "a".into(), label: "one".into(), after_secs: 1 });
    run(&mut i, 2000);
    i.push(i.now, Input::CommandStart { id: "b".into(), label: "two".into(), after_secs: 1 });
    run(&mut i, 3000);
    assert_eq!(i.ops.op.as_ref().unwrap().label, "one", "the first keeps the pill");
    i.push(i.now, Input::CommandEnd { id: "b".into(), code: 0 });
    assert_eq!(i.ops.op.as_ref().unwrap().label, "one", "an unseen command ends silently");
}

#[test]
fn a_script_activity_shows_with_its_own_icon_colour_and_words() {
    let mut i = Island::new(Config::default());
    let a = island_core::progress::Activity { id: String::new(), label: "CI main".into(), detail: "building".into(), icon: Some("git-branch".into()), tint: Some((255, 159, 10)), frac: None, file: None, busy: false };
    i.push(T0, Input::Activity(progress::State::Run, a.clone()));
    run(&mut i, 600);
    assert_eq!((i.current_face(), i.shown_side), (Face::Idle, Some(Side::Op)), "a small pill");
    i.push(i.now, Input::Activity(progress::State::Fail, island_core::progress::Activity { id: String::new(), label: "CI main".into(), detail: "2 tests failed".into(), ..Default::default() }));
    run(&mut i, 600);
    let o = i.notif.clone().expect("how it ended");
    assert_eq!((o.kind, o.summary.as_str(), o.body.as_str(), o.icon.as_deref()), (Some(island_core::notify::Kind::Fail), "CI main", "2 tests failed", Some("git-branch")));
    // it draws without trouble in every state
    let f = fonts();
    let mut scene = Scene::new();
    let fr = i.tick(i.now + 8, 8.0);
    i.draw(&mut scene, &f, fr.surface, 1.0);
}

#[test]
fn today_lists_what_is_running_above_the_day() {
    let mut i = Island::new(Config::default());
    let quiet = i.today_height();
    assert!(i.running_now().is_empty());
    i.push(T0, Input::StartTimer(600));
    i.push(T0, Input::Progress("40 Backup".into()));
    let rows = i.running_now();
    assert_eq!(rows.len(), 2, "the timer and the command");
    assert_eq!((rows[0].label.as_str(), rows[0].page), ("Timer", Page::Timer));
    assert_eq!((rows[1].label.as_str(), rows[1].right.as_str()), ("Backup", "40%"));
    assert_eq!(i.today_height(), quiet + 60.0, "each row takes its place on the page");
    i.push(i.now, Input::ProgressDone("done".into()));
    assert_eq!(i.running_now().len(), 1, "a finished command is not running");
}

#[test]
fn several_agents_show_up_on_today_each_with_its_status() {
    let mut i = Island::new(Config::default());
    let act = |id: &str, label: &str, detail: &str| island_core::progress::Activity { id: id.into(), label: label.into(), detail: detail.into(), icon: Some("bolt".into()), ..Default::default() };
    i.push(T0, Input::Activity(island_core::progress::State::Run, act("s1", "Claude · site", "Edit lib.rs")));
    i.push(T0, Input::Activity(island_core::progress::State::Run, act("s2", "Codex · api", "thinking")));
    i.push(T0, Input::StartNamedTimer(600, "tea".into()));
    let rows: Vec<String> = i.running_now().into_iter().map(|r| r.label).collect();
    assert_eq!(rows, ["tea", "Claude · site · Edit lib.rs", "Codex · api · thinking"]);
    i.push(i.now, Input::CancelTimerNamed("TEA".into()));
    assert!(i.timer.is_none(), "cancelled by name, whatever the case");
    i.push(i.now, Input::ActivityClear("s1".into()));
    assert_eq!(i.running_now().len(), 1);
}

#[test]
fn several_small_pills_sit_beside_the_main_one_up_to_the_option() {
    let act = |id: &str, label: &str| island_core::progress::Activity { id: id.into(), label: label.into(), detail: "thinking".into(), ..Default::default() };
    let build = |pills: u32| {
        let mut i = Island::new(Config { mini_pills: pills, ..Config::default() });
        i.data = data(); // music is playing
        i.data.focus = Some(Focus { active: true, end_epoch: None });
        i.push(T0, Input::StartTimer(600));
        i.push(T0, Input::Activity(island_core::progress::State::Run, act("a1", "Claude · site")));
        i.push(T0, Input::Activity(island_core::progress::State::Run, act("a2", "Codex · api")));
        i.push(T0, Input::Activity(island_core::progress::State::Run, act("a3", "Aider · docs")));
        run(&mut i, 5000);
        i
    };
    let i = build(1);
    assert_eq!((i.current_face(), i.sides_shown()), (Face::MediaMini, vec![Side::Timer]), "one: as it always was");
    let i = build(2);
    assert_eq!(i.sides_shown(), [Side::Timer, Side::Focus]);
    let i = build(4);
    assert_eq!(i.sides_shown(), [Side::Timer, Side::Focus, Side::Op, Side::OpAt(0)]);
    let (px, _, pw, _) = i.pill_rect();
    assert!(pw > i.spring.w.pos + 3.0 * BUBBLE_W, "the pill rect holds all the bubbles");
    // each one opens its own page
    let mut i = i;
    let fr = i.tick(i.now + 8, 8.0);
    i.draw(&mut Scene::new(), &fonts(), fr.surface, 1.0);
    let hits: Vec<Action> = i.hits.iter().map(|h| h.action.clone()).collect();
    // (and the drop under the pill, which the countdown fills)
    assert_eq!(hits.iter().filter(|a| matches!(a, Action::Open(Page::Timer | Page::Focus | Page::Op))).count(), 5, "{hits:?}");
    let _ = px;
    // fewer room again: the extra ones go
    let mut i = build(4);
    i.cfg.mini_pills = 1;
    run(&mut i, 600);
    assert_eq!(i.sides_shown(), [Side::Timer]);
}

#[test]
fn a_download_of_unknown_size_hangs_a_drop_that_sloshes_while_data_comes() {
    let mut i = Island::new(Config::default());
    run(&mut i, 3000);
    let dl = |detail: &str, busy: bool| island_core::progress::Activity { id: "dl:a.iso.part".into(), label: "a.iso".into(), detail: detail.into(), icon: Some("download".into()), busy, ..Default::default() };
    i.push(i.now, Input::Activity(progress::State::Run, dl("1.0 MB", false)));
    run(&mut i, 8000);
    assert!(i.fill.is_none(), "nothing arrives yet: no drop");
    i.push(i.now, Input::Activity(progress::State::Run, dl("5.0 MB · 2.0 MB/s", true)));
    run(&mut i, 4000);
    assert!(i.fill.as_ref().is_some_and(|f| f.out(i.now) > 0.9), "the drop hangs");
    let mut levels = vec![];
    let mut asked = vec![];
    for _ in 0..90 {
        let t = i.now + 33;
        asked.push(i.tick(t, 33.0).next_ms);
        levels.push(i.fill.as_ref().unwrap().level(t));
    }
    let (low, high) = levels.iter().fold((1.0f64, 0.0f64), |(a, b), l| (a.min(*l), b.max(*l)));
    assert!(low > 0.1 && high < 0.9 && high - low > 0.3, "it rises and falls, never empty nor full: {low}..{high}");
    assert!(asked.iter().all(|ms| (1..=liquid::BUSY_STEP_MS).contains(ms)), "frames of its own, but not at the screen's rate: {asked:?}");
    // a countdown has something to show: it takes the drop
    i.push(i.now, Input::StartTimer(60));
    run(&mut i, 4000);
    assert_eq!(i.fill.as_ref().map(|f| f.source.clone()), Some(liquid::Source::Timer));
    i.cancel_timer();
    run(&mut i, 4000);
    // stalled: the drop goes back, and no more frames are asked for it
    i.push(i.now, Input::Activity(progress::State::Run, dl("5.0 MB · stalled", false)));
    run(&mut i, 6000);
    assert!(i.fill.is_none());
    // data again, then the end: the drop becomes the card
    i.push(i.now, Input::Activity(progress::State::Run, dl("7.0 MB · 1 MB/s", true)));
    run(&mut i, 4000);
    i.push(i.now, Input::Activity(progress::State::Done, dl("9.0 MB · 20s", false)));
    run(&mut i, 100);
    let card = i.notif.as_ref().map(|n| format!("{} · {}", n.summary, n.body));
    assert!(card.as_ref().is_some_and(|c| c.contains("a.iso")), "{card:?}");
}

#[test]
fn a_finished_download_offers_the_file_and_its_folder() {
    let mut i = Island::new(Config::default());
    let a = island_core::progress::Activity { id: "dl:x.part".into(), label: "x".into(), detail: "12 MB".into(), file: Some("/home/u/Downloads/x".into()), ..Default::default() };
    i.push(T0, Input::Activity(progress::State::Done, a));
    let card = i.notif.clone().expect("a card");
    let labels: Vec<&str> = card.actions.iter().map(|a| a.label.as_str()).collect();
    assert_eq!(labels, ["Open", "Show in folder"]);
    i.pending.clear();
    i.activate(Action::OnCard(card.id, Box::new(Action::NotifAction(card.actions[1].key.clone()))));
    assert!(i.pending.contains(&Command::OpenFile("/home/u/Downloads".into())), "{:?}", i.pending);
    assert!(up(&i).is_empty());
}

#[test]
fn a_small_pill_comes_out_of_the_pill_and_goes_back_in() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Progress("20 Backup".into()));
    run(&mut i, 120);
    let early = i.bubble_slot();
    assert!(early > 0.0 && early < BUBBLE_W, "on its way out: {early}");
    run(&mut i, 1200);
    assert!((i.bubble_slot() - (BUBBLE_W + BUBBLE_GAP)).abs() < 1.0);
    i.push(i.now, Input::ProgressClear);
    let fr = run_frame(&mut i, 80);
    let back = i.bubble_slot();
    assert!(back > 0.0 && back < BUBBLE_W + BUBBLE_GAP, "on its way back: {back}");
    assert!(fr.animating && fr.surface.0 > i.spring.w.pos + BUBBLE_W, "with frames and room for it");
    run(&mut i, 1000);
    assert_eq!((i.bubble_slot(), i.bubbles.len()), (0.0, 0));
}

#[test]
fn a_drop_hangs_beside_the_pill_and_fills_with_the_progress() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Progress("20 Backup".into()));
    card_frames(&mut i, &f, 1600);
    let fill = i.fill.as_ref().expect("a drop");
    assert!((fill.level(i.now) - 0.2).abs() < 0.01);
    let (x, y, w, h) = fill.rect.expect("hanging");
    let (px, py, _, ph) = i.pill_rect();
    assert!(x + w <= px && y >= py && y + h <= py + ph, "at the left, level with the pill: {x} {y} {w} {h} beside {px} {py}");
    assert!(x >= 0.0 && i.input_rects().contains(&(x, y, w, h)), "inside the surface, taking the pointer");
    i.push(i.now, Input::Progress("70 Backup".into()));
    let fr = card_frames(&mut i, &f, 1600);
    assert!((i.fill.as_ref().unwrap().level(i.now) - 0.7).abs() < 0.01);
    assert!(fr.surface.0 >= i.spring.w.pos + 2.0 * w && !fr.animating, "room for it, and no frames once it is still");
    // a click opens the job's page
    i.pointer_moved(i.now, x + w / 2.0, y + h / 2.0);
    assert_eq!(i.hit_at(x + w / 2.0, y + h / 2.0).map(|h| h.action.clone()), Some(Action::Open(Page::Op)));
    // a card drops under the pill and the drop, at the side, stays
    i.push(i.now, Input::Notification(incoming(1)));
    card_frames(&mut i, &f, 600);
    assert!(i.fill.as_ref().is_some_and(|d| d.rect.is_some()) && i.has_card());
    i.cfg.progress_drop = false;
    card_frames(&mut i, &f, 1000);
    assert!(i.fill.is_none());
}

#[test]
fn a_countdown_drains_the_drop() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    i.push(T0, Input::StartTimer(100));
    card_frames(&mut i, &f, 1000);
    let level = |i: &Island| i.fill.as_ref().expect("a drop").level(i.now);
    assert!(level(&i) > 0.96, "full at the start: {}", level(&i));
    run(&mut i, 69_000);
    card_frames(&mut i, &f, 1000);
    assert!((level(&i) - 0.29).abs() < 0.03, "what is left: {}", level(&i));
}

/// Opening over a small pill and the drop: neither rides the edge that moves away, so nothing ever stands further out
/// than it did at rest, until the open shape itself is there.
#[test]
fn the_pill_opens_over_what_is_beside_it_without_pushing_it_out() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Progress("20 Backup".into()));
    card_frames(&mut i, &f, 2000);
    let (w, slot) = (i.spring.w.pos, i.bubble_slot());
    // from the middle of the surface: the small pill's right end, the drop's left end
    let right = i.bubble_place(0, w).0 + BUBBLE_W - (w + slot) / 2.0;
    let drop = i.fill.as_ref().and_then(|d| d.rect).expect("hanging");
    let left = drop.0 - i.pill_origin.0 - (w + slot) / 2.0;
    let (px, py, _, ph) = i.pill_rect();
    i.pointer_moved(i.now, px + 40.0, py + ph / 2.0);
    let mut grown = false;
    for _ in 0..120 {
        let fr = i.tick(i.now + 8, 8.0);
        i.draw(&mut Scene::new(), &f, fr.surface, 1.0);
        let (w, slot) = (i.spring.w.pos, i.bubble_slot());
        let edge = (w - slot) / 2.0;
        for n in 0..i.bubbles.len() {
            let (x, width) = i.bubble_place(n, w);
            let r = x + width - (w + slot) / 2.0;
            assert!(r <= right.max(edge) + 0.5, "the small pill stands out: {r} past {right} and {edge}");
        }
        if let Some(held) = i.fill.as_ref().and_then(|d| d.held) {
            assert!(held >= left - 0.5, "the drop stays where it hung: {held} against {left}");
        }
        grown |= i.spring.h.pos > 60.0;
    }
    assert!(grown, "it did open");
    assert!(i.bubbles.is_empty() && i.fill.is_none(), "both swallowed");
    // and back: they come out once the pill is at rest again
    i.pointer_left(i.now);
    for _ in 0..300 {
        let fr = i.tick(i.now + 8, 8.0);
        i.draw(&mut Scene::new(), &f, fr.surface, 1.0);
        assert!(i.spring.h.pos < 34.5 || (i.bubbles.is_empty() && i.fill.is_none()), "nothing beside a pill still open");
    }
    assert!(!i.bubbles.is_empty() && i.fill.is_some());
}
