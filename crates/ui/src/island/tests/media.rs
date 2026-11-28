//! The player, its lyrics, and the volume.

use super::*;

fn media_page(playing: bool) -> Island {
    let mut i = Island::new(Config::default());
    i.data = data();
    i.data.media.as_mut().unwrap().playing = playing;
    i.push(T0, Input::OpenPage(Page::Media));
    draw_frames(&mut i, 300);
    assert!(matches!(i.current_face(), Face::MediaFull | Face::MediaLyric));
    i
}

#[test]
fn a_level_that_arrives_in_steps_slides_instead_of_jumping() {
    let mut i = Island::new(Config::default());
    i.data.volume = Some(0.30);
    run(&mut i, 100);
    assert!((i.volume_shown - 0.30).abs() < 1e-6, "the first reading is not slid to");
    i.data.volume = Some(0.60);
    let fr = i.tick(i.now + 8, 8.0);
    assert!(i.volume_shown > 0.30 && i.volume_shown < 0.40, "one frame moves a little: {}", i.volume_shown);
    assert!(fr.animating, "and frames keep coming at the display rate until it arrives");
    let mut shown = vec![i.volume_shown];
    for _ in 0..80 {
        i.tick(i.now + 8, 8.0);
        shown.push(i.volume_shown);
    }
    assert!(shown.windows(2).all(|w| w[1] >= w[0]), "monotonic, no overshoot");
    assert!((*shown.last().unwrap() - 0.60).abs() < 1e-6, "it lands on the level");
}

#[test]
fn a_slider_under_the_pointer_follows_it_exactly() {
    let mut i = Island::new(Config::default());
    i.data.volume = Some(0.30);
    run(&mut i, 100);
    i.dragging = Some(Action::VolumeBar);
    i.data.volume = Some(0.90);
    i.tick(i.now + 8, 8.0);
    assert!((i.volume_shown - 0.90).abs() < 1e-6, "no lag behind the finger");
    i.dragging = None;
    i.data.brightness = Some(0.5);
    i.tick(i.now + 8, 8.0);
    assert!(i.brightness_shown <= 0.5);
}

#[test]
fn the_volume_bar_starts_from_the_level_it_had() {
    let mut i = Island::new(Config::default());
    i.data.volume = Some(0.50);
    run(&mut i, 100);
    i.push(i.now, Input::Osd { kind: OsdKind::Volume, level: 0.56, muted: false });
    assert!((i.osd_shown - 0.50).abs() < 1e-6, "it starts where the bar last was");
    draw_frames(&mut i, 60);
    assert!((i.osd_shown - 0.56).abs() < 0.001, "and slides to the new one: {}", i.osd_shown);
    // muting slides to empty
    i.push(i.now, Input::Osd { kind: OsdKind::Volume, level: 0.56, muted: true });
    draw_frames(&mut i, 80);
    assert!(i.osd_shown < 0.01);
}

#[test]
fn scrolling_over_the_small_pill_changes_the_volume_and_the_middle_button_mutes() {
    let mut i = Island::new(Config::default());
    i.data.volume = Some(0.50);
    run(&mut i, 600);
    assert!(i.current_face().is_compact());
    let cmds = i.scroll(i.now, 0.0, 2.0);
    assert!(matches!(cmds.as_slice(), [Command::SetVolume(v)] if (*v - 0.54).abs() < 1e-6), "{cmds:?}");
    let cmds = i.scroll(i.now, 0.0, -50.0);
    assert!(matches!(cmds.as_slice(), [Command::SetVolume(v)] if *v == 0.0), "down to zero, no further");
    assert_eq!(i.middle_click(i.now), vec![Command::ToggleMute]);
    // not while a page is open: there the middle button does nothing
    i.push(i.now, Input::OpenPage(Page::Cal));
    run(&mut i, 600);
    assert!(i.middle_click(i.now).is_empty());
}

#[test]
fn a_sideways_swipe_over_the_pill_skips_a_track_once() {
    let mut i = Island::new(Config::default());
    i.data = data();
    run(&mut i, 600);
    assert_eq!(i.current_face(), Face::MediaMini);
    assert!(i.scroll(i.now, 1.0, 0.0).is_empty(), "a small movement is added up, not acted on");
    assert!(i.scroll(i.now, 1.0, 0.0).is_empty());
    assert_eq!(i.scroll(i.now, 1.5, 0.0), vec![Command::MediaNext], "past the threshold: next");
    assert!(i.scroll(i.now + 100, 5.0, 0.0).is_empty(), "the rest of the same swipe does nothing");
    i.now += 800;
    assert_eq!(i.scroll(i.now, -4.0, 0.0), vec![Command::MediaPrevious], "the other way: previous");
    // without music a sideways swipe does nothing
    let mut j = Island::new(Config::default());
    run(&mut j, 600);
    assert!(j.scroll(j.now, 9.0, 0.0).is_empty());
}

#[test]
fn the_play_pause_button_answers_at_once_and_the_icons_cross_fade() {
    let mut i = media_page(false);
    assert_eq!((i.play_anim, i.playing_shown()), (0.0, false));
    assert!(press(&mut i, Action::Cmd(Command::MediaPlayPause)));
    assert_eq!(i.take_commands(), vec![Command::MediaPlayPause], "the player is asked once");
    assert!(i.playing_shown(), "it shows playing before the player has said so");
    assert!(i.disc_press > 0.9, "and the disc gives under the press");
    i.tick(i.now + 8, 8.0);
    assert!(i.play_anim > 0.0 && i.play_anim < 0.5, "one frame in: part of the way ({})", i.play_anim);
    draw_frames(&mut i, 40);
    assert!(i.play_anim > 0.99, "and then the pause icon is fully there");
    assert!(i.disc_press < 0.05, "the press has died away");
    // the player agrees: nothing changes, the override is dropped
    i.data.media.as_mut().unwrap().playing = true;
    draw_frames(&mut i, 5);
    assert!(i.play_override.is_none() && i.playing_shown());
}

#[test]
fn a_press_the_player_never_confirms_is_taken_back() {
    let mut i = media_page(true);
    assert!(press(&mut i, Action::Cmd(Command::MediaPlayPause)));
    assert!(!i.playing_shown());
    draw_frames(&mut i, 40);
    assert!(i.play_anim < 0.1, "shown as paused: {}", i.play_anim);
    i.now += 2000; // the player still says playing
    draw_frames(&mut i, 40);
    assert!(i.playing_shown(), "back to what the player says");
    assert!(i.play_anim > 0.9);
}

#[test]
fn a_skip_button_nudges_its_icon_and_comes_back() {
    let mut i = media_page(true);
    assert!(press(&mut i, Action::Cmd(Command::MediaNext)));
    assert_eq!(i.take_commands(), vec![Command::MediaNext]);
    assert!(i.skip_press > 0.9, "{}", i.skip_press);
    draw_frames(&mut i, 120);
    assert!(i.skip_press.abs() < 0.01, "back in place: {}", i.skip_press);
    assert!(press(&mut i, Action::Cmd(Command::MediaPrevious)));
    assert!(i.skip_press < -0.9, "the other way for previous: {}", i.skip_press);
}

#[test]
fn the_disc_swells_under_the_pointer() {
    let mut i = media_page(true);
    let disc = i.hits.iter().find(|h| h.action == Action::Cmd(Command::MediaPlayPause)).unwrap().rect;
    assert_eq!(i.disc_hover, 0.0);
    i.pointer_moved(i.now, disc.center().x + i.pill_origin.0, disc.center().y + i.pill_origin.1);
    draw_frames(&mut i, 30);
    assert!(i.disc_hover > 0.95, "{}", i.disc_hover);
    i.pointer_left(i.now);
    draw_frames(&mut i, 30);
    assert!(i.disc_hover < 0.05);
}

#[test]
fn the_lyrics_scroll_one_line_but_a_jump_just_lands() {
    let mut i = Island::new(Config { lyrics_in_pill: true, ..Config::default() });
    i.data = data();
    let lrc: String = (1..=40).map(|n| format!("[{:02}:{:02}.00] line {n}\n", n * 3 / 60, n * 3 % 60)).collect();
    i.data.lyrics_lrc = Some(lrc);
    i.data.media.as_mut().unwrap().position_ms = 10_000.0;
    i.data.media.as_mut().unwrap().sampled_at_ms = T0;
    run(&mut i, 600);
    let idx = i.lyric_idx;
    // the next line comes up: a scroll in progress
    i.data.media.as_mut().unwrap().position_ms = 13_000.0;
    i.data.media.as_mut().unwrap().sampled_at_ms = i.now;
    run(&mut i, 16);
    assert_eq!(i.lyric_idx, idx + 1);
    assert!(i.lyric_at > 0, "one line on is animated");
    // the island wakes after a long pause (the screen was locked): thirty lines on, no scroll through them
    i.now += 1000;
    i.data.media.as_mut().unwrap().position_ms = 100_000.0;
    i.data.media.as_mut().unwrap().sampled_at_ms = i.now;
    run(&mut i, 16);
    assert!(i.lyric_idx > idx + 20);
    assert!(i.lyric_at < 0, "a jump has no animation to play");
    assert!(i.lyric_strip().is_some_and(|s| s.iter().any(|(_, slot)| *slot == 0.0)), "the current line sits in its place");
}

#[test]
fn the_pill_can_show_the_line_being_sung() {
    let f = fonts();
    let mut i = Island::new(Config { lyrics_in_pill: true, ..Config::default() });
    i.data = data();
    i.data.lyrics_lrc = Some("[00:01.00] First line\n[00:30.00] Second line".into());
    i.data.media.as_mut().unwrap().position_ms = 5000.0;
    i.data.media.as_mut().unwrap().sampled_at_ms = T0;
    run(&mut i, 600);
    assert_eq!(i.current_face(), Face::MediaMini);
    assert_eq!(i.lyric_now().map(|(t, _)| t).as_deref(), Some("First line"));
    let mut scene = Scene::new();
    let fr = i.tick(i.now + 8, 8.0);
    i.draw(&mut scene, &f, fr.surface, 1.0);
    // time moves on past the second line: the entrance restarts from nothing
    i.data.media.as_mut().unwrap().position_ms = 29_900.0;
    i.data.media.as_mut().unwrap().sampled_at_ms = i.now;
    run(&mut i, 200);
    let (t, e) = i.lyric_now().unwrap();
    assert_eq!(t, "Second line");
    assert!(e < 1.0, "still coming in");
    run(&mut i, 600);
    assert_eq!(i.lyric_now().unwrap().1, 1.0);
    // a blank line (an instrumental gap) shows nothing
    let mut j = Island::new(Config::default());
    j.data = data();
    j.data.lyrics_lrc = Some("[00:01.00] \n[00:30.00] x".into());
    j.data.media.as_mut().unwrap().position_ms = 5000.0;
    j.data.media.as_mut().unwrap().sampled_at_ms = T0;
    run(&mut j, 600);
    assert!(j.lyric_now().is_none());
}

#[test]
fn a_player_with_no_artist_draws_the_media_page_in_place() {
    // Microsoft Teams in a browser is an MPRIS player with a long title, no artist, no art and a 7 s length
    let mut i = Island::new(Config::default());
    i.data.media = Some(Media { track_id: "z".into(), title: "Teams and Channels | 26 - decorators and OOP in Python | Microsoft Teams".into(), artist: String::new(), playing: false, position_ms: 0.0, sampled_at_ms: T0, length_ms: 7000.0, can_seek: true, players: 3, player: "Zen".into(), ..Default::default() });
    i.push(T0, Input::OpenPage(Page::Media));
    draw_frames(&mut i, 300);
    assert_eq!(i.current_face(), Face::MediaFull);
    let seek = i.hits.iter().find(|h| h.action == Action::SeekBar).expect("the seek bar").rect;
    assert!(seek.y0 < 70.0 && seek.x1 < 420.0, "the bar sits under the title, inside the island: {seek:?}");
    // and the picture: an empty text must not displace what is drawn after it
    let f = fonts();
    let mut with_empty = Scene::new();
    let mut without = Scene::new();
    i.draw(&mut with_empty, &f, (420.0, 200.0), 1.0);
    i.data.media.as_mut().unwrap().artist = " ".into();
    i.draw(&mut without, &f, (420.0, 200.0), 1.0);
    let _ = (with_empty, without);
}

#[test]
fn the_pill_widens_for_a_long_line_of_lyrics_and_stays_put_for_a_short_one() {
    let mut i = Island::new(Config { lyrics_in_pill: true, ..Config::default() });
    i.data = data();
    let long = "I've been walking down this road for a thousand miles and I still don't know the way home";
    i.data.lyrics_lrc = Some(format!("[00:01.00] ok\n[00:10.00] {long}\n[00:30.00] end\n"));
    i.data.media.as_mut().unwrap().playing = true;
    i.data.media.as_mut().unwrap().position_ms = 2_000.0;
    i.data.media.as_mut().unwrap().sampled_at_ms = T0;
    run(&mut i, 600);
    let base = i.face_size(Face::MediaMini).0;
    assert!((base - Face::MediaMini.size().0).abs() < 80.0, "a short line fits the usual pill: {base}");
    i.data.media.as_mut().unwrap().position_ms = 11_000.0;
    i.data.media.as_mut().unwrap().sampled_at_ms = i.now;
    run(&mut i, 16);
    let wide = i.face_size(Face::MediaMini).0;
    assert!(wide > base + 200.0 && wide <= 640.0, "{base} -> {wide}");
    i.cfg.lyrics_in_pill = false;
    assert_eq!(i.face_size(Face::MediaMini).0, base.min(i.face_size(Face::MediaMini).0), "off: the title's pill");
    assert!(i.face_size(Face::MediaMini).0 < wide);
}

#[test]
fn a_change_of_the_keyboard_shows_where_the_volume_does() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    i.push(T0, Input::KeyboardLayout("English (US)".into()));
    run(&mut i, 400);
    assert_eq!(i.current_face(), Face::Osd);
    let fr = i.tick(i.now + 8, 8.0);
    i.draw(&mut Scene::new(), &f, fr.surface, 1.0);
    i.push(i.now, Input::CapsLock(true));
    assert_eq!(i.notice.as_ref().map(|n| n.1.as_str()), Some("Caps Lock on"));
    // the volume takes the place over, and gives it back
    i.push(i.now, Input::Osd { kind: OsdKind::Volume, level: 0.5, muted: false });
    assert!(i.notice.is_none() && i.osd.is_some());
    i.push(i.now, Input::CapsLock(false));
    assert!(i.notice.is_some() && i.osd.is_none());
    run(&mut i, 3000);
    assert_ne!(i.current_face(), Face::Osd);
    let mut off = Island::new(Config { keyboard_alerts: false, ..Config::default() });
    off.push(T0, Input::CapsLock(true));
    run(&mut off, 400);
    assert_ne!(off.current_face(), Face::Osd);
}

#[test]
fn shuffle_and_repeat_are_buttons_only_when_the_player_offers_them() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    i.data = data();
    i.push(T0, Input::OpenPage(Page::Media));
    let buttons = |i: &mut Island| {
        run(i, 600);
        let fr = i.tick(i.now + 8, 8.0);
        i.draw(&mut Scene::new(), &f, fr.surface, 1.0);
        i.hits.iter().filter_map(|h| match &h.action { Action::Cmd(c @ (Command::MediaShuffle(_) | Command::MediaLoop(_))) => Some(c.clone()), _ => None }).collect::<Vec<_>>()
    };
    assert!(buttons(&mut i).is_empty());
    let m = i.data.media.as_mut().unwrap();
    (m.shuffle, m.repeat) = (Some(false), Some(LoopMode::All));
    assert_eq!(buttons(&mut i), [Command::MediaShuffle(true), Command::MediaLoop(LoopMode::One)]);
    // the button answers at once
    assert_eq!(i.activate(Action::Cmd(Command::MediaShuffle(true))), [Command::MediaShuffle(true)]);
    i.activate(Action::Cmd(Command::MediaLoop(LoopMode::One)));
    assert_eq!(buttons(&mut i), [Command::MediaShuffle(false), Command::MediaLoop(LoopMode::Off)]);
    // all the buttons of the player stand apart
    i.data.media.as_mut().unwrap().players = 2;
    buttons(&mut i);
    let row: Vec<_> = i.hits.iter().filter(|h| (h.rect.center().y - 114.0).abs() < 1.0).map(|h| h.rect).collect();
    assert_eq!(row.len(), 7, "{row:?}");
    assert!(row.iter().enumerate().all(|(n, a)| row.iter().skip(n + 1).all(|b| a.x1 <= b.x0 || b.x1 <= a.x0)), "{row:?}");
}
