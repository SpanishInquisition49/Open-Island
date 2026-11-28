//! The machine: quick settings, devices, battery, sound, network, programs and activity.

use super::*;

fn headphones(battery: Option<u8>) -> Device {
    Device { id: "88:C9".into(), name: "Sony XM5".into(), kind: island_core::bluetooth::Kind::Headphones, battery }
}

#[test]
fn low_battery_and_plugging_raise_one_card_each_and_the_newer_takes_the_older_ones_place() {
    let mut i = Island::new(Config::default());
    i.data.battery = Some(Battery { percent: 80.0, charging: false, on_ac: true });
    i.tick(T0, 8.0);
    i.data.battery = Some(Battery { percent: 80.0, charging: false, on_ac: false });
    i.tick(T0 + 8, 8.0);
    assert_eq!(up(&i), ["On battery"]);
    assert_eq!(i.notif.as_ref().unwrap().ring, Some(80));
    i.data.battery = Some(Battery { percent: 10.0, charging: false, on_ac: false });
    i.tick(T0 + 16, 8.0);
    assert_eq!(up(&i), ["Low battery"], "one card for the power");
    run(&mut i, 10_000);
    assert!(up(&i).is_empty(), "low battery is announced once");
}

#[test]
fn a_connecting_device_gets_a_card_and_a_low_battery_warns_once_until_it_recovers() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::DeviceConnected(headphones(Some(80))));
    run(&mut i, 600);
    assert_eq!((i.current_face(), up(&i)), (Face::Idle, vec!["Sony XM5"]));
    let n = i.notif.as_ref().unwrap();
    assert_eq!((n.body.as_str(), n.ring), ("Connected", Some(80)));
    run(&mut i, 4000);
    assert!(up(&i).is_empty(), "it goes away by itself");
    i.data.devices = vec![headphones(Some(12))];
    i.push(i.now, Input::DeviceLow(headphones(Some(12))));
    run(&mut i, 600);
    let n = i.notif.as_ref().unwrap();
    assert_eq!((n.body.as_str(), n.ring), ("Low battery", Some(12)));
    run(&mut i, 9000);
    i.push(i.now, Input::DeviceLow(headphones(Some(11))));
    run(&mut i, 600);
    assert!(up(&i).is_empty(), "told once");
    // charged again, then low again: a new warning
    i.data.devices = vec![headphones(Some(90))];
    run(&mut i, 1000);
    i.push(i.now, Input::DeviceLow(headphones(Some(10))));
    run(&mut i, 600);
    assert_eq!(up(&i), ["Sony XM5"]);
}

#[test]
fn the_system_page_lists_connected_devices_and_grows_for_them() {
    let mut i = Island::new(Config::default());
    i.data.system = Some(Default::default());
    let before = i.sys_rows();
    i.data.devices = vec![headphones(Some(50)), Device { id: "m".into(), name: "MX".into(), kind: island_core::bluetooth::Kind::Mouse, battery: None }];
    assert_eq!(i.sys_rows(), before + 2);
    i.push(T0, Input::OpenPage(Page::Sys));
    draw_frames(&mut i, 300);
}

#[test]
fn the_quick_settings_page_lists_the_devices_and_their_battery() {
    let mut i = Island::new(Config::default());
    let h0 = i.qs_layout().height;
    i.data.devices = vec![headphones(Some(85))];
    assert_eq!(i.qs_layout().height, h0 + 10.0 + 32.0, "one row");
    i.data.devices = vec![headphones(Some(85)), headphones(Some(12)), headphones(None), headphones(Some(5))];
    assert_eq!(i.qs_layout().height, h0 + 10.0 + 3.0 * 32.0 + 2.0 * 6.0, "three at most");
    i.push(T0, Input::OpenPage(Page::Qs));
    draw_frames(&mut i, 300);
    i.data.devices.clear();
    assert_eq!(i.qs_layout().height, h0, "and nothing when none is connected");
}

#[test]
fn the_home_page_is_left_as_it_was() {
    let mut i = Island::new(Config::default());
    let h0 = i.face_size(Face::IdleHover).1;
    i.data.devices = vec![headphones(Some(85))];
    assert_eq!(i.face_size(Face::IdleHover).1, h0);
}

#[test]
fn device_icons_exist_in_the_bundled_font() {
    use island_core::bluetooth::Kind::*;
    for k in [Earbuds, Headphones, Headset, Keyboard, Mouse, Gamepad, Other] {
        assert!(crate::is_known_icon(k.icon()), "{}", k.icon());
    }
}

#[test]
fn quick_settings_toggles_answer_at_once() {
    let mut i = Island::new(Config::default());
    i.data.wifi = Some(true);
    i.data.bluetooth = Some(false);
    assert_eq!(i.activate(Action::Cmd(Command::ToggleWifi)), vec![Command::ToggleWifi]);
    assert_eq!(i.data.wifi, Some(false));
    i.activate(Action::Cmd(Command::ToggleBluetooth));
    assert_eq!(i.data.bluetooth, Some(true));
    i.activate(Action::Cmd(Command::ToggleKeepAwake));
    assert!(i.data.keep_awake);
    i.data.wifi = None;
    i.activate(Action::Cmd(Command::ToggleWifi));
    assert_eq!(i.data.wifi, None, "unknown stays unknown until the next sample");
    i.data.night_light = Some(false);
    i.activate(Action::Cmd(Command::ToggleNightLight));
    assert_eq!(i.data.night_light, Some(true));
    i.data.night_light = None;
    i.activate(Action::Cmd(Command::ToggleNightLight));
    assert_eq!(i.data.night_light, None, "no wlsunset: the chip stays hidden");
}

#[test]
fn quick_settings_hide_what_the_machine_does_not_have() {
    let mut i = Island::new(Config::default());
    // a bare system: DND and keep awake only, no brightness, no outputs
    assert_eq!(i.qs_chips().len(), 2);
    let bare = i.qs_layout();
    assert!(bare.bri_cy.is_none() && bare.out_top.is_none());
    i.data = data();
    i.data.wifi = Some(true);
    i.data.bluetooth = Some(false);
    i.data.profile = Some("balanced".into());
    i.data.night_light = Some(false);
    i.data.brightness = Some(0.5);
    i.data.sinks = vec![Sink { name: "a".into(), label: "A".into() }];
    let full = i.qs_layout();
    assert_eq!(i.qs_chips().len(), 6);
    assert!(full.height > bare.height + 44.0 + 36.0, "two chip rows, a brightness row and the output row");
    assert!(full.bri_cy.is_some() && full.out_top.is_some());
    // the page draws at every size without escaping its own height
    let f = fonts();
    for (page_data, label) in [(Data::default(), "bare"), (i.data.clone(), "full")] {
        let mut j = Island::new(Config::default());
        j.data = page_data;
        j.push(T0, Input::OpenPage(Page::Qs));
        for _ in 0..150 {
            let fr = j.tick(j.now + 8, 8.0);
            let mut scene = Scene::new();
            j.draw(&mut scene, &f, fr.surface, 1.0);
        }
        assert_eq!(j.current_face(), Face::Qs, "{label}");
    }
}

#[test]
fn the_sound_page_picks_devices_and_sets_the_volume_of_an_app() {
    let mut i = Island::new(Config::default());
    let sink = |n: &str| Sink { name: n.into(), label: n.into() };
    i.data.sinks = vec![sink("speaker"), sink("bluez_output.x")];
    i.data.sink = Some("speaker".into());
    i.data.sources = vec![sink("mic")];
    i.data.source = Some("mic".into());
    i.data.streams = vec![AppStream { id: 7, name: "Spotify".into(), volume: 60, muted: false }];
    i.push(T0, Input::OpenPage(Page::Audio));
    draw_frames(&mut i, 200);
    assert_eq!(i.current_face(), Face::Audio);
    assert!(press(&mut i, Action::Cmd(Command::SetSink("bluez_output.x".into()))), "an output row");
    assert!(i.take_commands().contains(&Command::SetSink("bluez_output.x".into())));
    draw_frames(&mut i, 10);
    assert!(press(&mut i, Action::Cmd(Command::SetStreamVolume { id: 7, percent: 70 })), "plus raises by ten");
    draw_frames(&mut i, 10);
    assert!(press(&mut i, Action::Cmd(Command::SetStreamVolume { id: 7, percent: 50 })), "minus lowers by ten");
    draw_frames(&mut i, 10);
    assert!(press(&mut i, Action::Cmd(Command::ToggleStreamMute(7))));
    let (_, h) = draw_frames(&mut i, 200);
    assert!(h >= i.audio_height(), "the page is as tall as its rows");
}

#[test]
fn the_output_row_of_quick_settings_opens_the_sound_page() {
    let mut i = Island::new(Config::default());
    i.data.sinks = vec![Sink { name: "a".into(), label: "A".into() }, Sink { name: "b".into(), label: "B".into() }];
    i.data.sink = Some("a".into());
    i.push(T0, Input::OpenPage(Page::Qs));
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::Open(Page::Audio)));
    draw_frames(&mut i, 200);
    assert_eq!(i.current_face(), Face::Audio);
}

#[test]
fn a_program_that_hogs_the_machine_gets_a_card_once_and_buttons_on_the_system_page() {
    let mut i = Island::new(Config::default());
    let hog = Culprit { name: "ffmpeg".into(), pids: vec![11, 12], cores: 3.4, mem_mb: 900.0, memory: false, protected: false };
    run(&mut i, 1000);
    i.data.culprit = Some(hog.clone());
    run(&mut i, 6000);
    let card = i.notif.clone().expect("a card");
    assert_eq!((card.summary.as_str(), card.body.as_str()), ("ffmpeg is using 3.4 cores", "Tap to pause or quit it"));
    i.notif = None;
    run(&mut i, 6000);
    assert!(i.notif.is_none(), "not again for the same program within ten minutes");
    i.push(i.now, Input::OpenPage(Page::Sys));
    i.data.system = Some(SystemStats::default());
    i.data.paused = vec!["chrome".into()];
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::Cmd(Command::PauseProcs { name: "ffmpeg".into(), pids: vec![11, 12] })));
    assert!(i.take_commands().contains(&Command::PauseProcs { name: "ffmpeg".into(), pids: vec![11, 12] }));
    draw_frames(&mut i, 10);
    assert!(press(&mut i, Action::Cmd(Command::QuitProcs(vec![11, 12]))));
    draw_frames(&mut i, 10);
    assert!(press(&mut i, Action::Cmd(Command::ResumeProcs("chrome".into()))), "what was paused can be resumed");
    i.data.culprit = Some(Culprit { name: "niri".into(), protected: true, ..hog });
    draw_frames(&mut i, 200);
    assert!(!i.hits.iter().any(|h| matches!(&h.action, Action::Cmd(Command::QuitProcs(_)))), "the session cannot be quit from here");
}

#[test]
fn a_broken_network_says_where_and_a_click_reconnects() {
    use island_core::netstate::Health;
    let mut i = Island::new(Config::default());
    run(&mut i, 1000);
    i.data.net = Health::NoInternet;
    run(&mut i, 6000);
    let card = i.notif.clone().expect("a card");
    assert_eq!((card.summary.as_str(), card.body.as_str()), ("No internet", "The router answers: the problem is beyond it · tap to reconnect"));
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::NotifOpen));
    assert!(i.take_commands().contains(&Command::ReconnectNetwork));
    i.notif = None;
    i.data.net = Health::Online;
    run(&mut i, 1000);
    assert_eq!(i.notif.as_ref().map(|n| n.summary.as_str()), Some("Back online"));
    assert_eq!(i.icon_count(false), 0, "no warning icon once it works");
    i.data.net = Health::RouterDown;
    assert_eq!(i.icon_count(false), 1, "a warning icon while it does not");
}

#[test]
fn a_long_stretch_on_one_app_gets_a_break_nudge_once_per_period() {
    let mut i = Island::new(Config { break_minutes: 30, ..Default::default() });
    run(&mut i, 1000);
    i.push(i.now, Input::Focused { app: "zen".into(), title: "x".into() });
    run(&mut i, 29 * 60_000);
    assert!(i.notif.is_none());
    run(&mut i, 63_000);
    let card = i.notif.clone().expect("a nudge");
    assert!(card.summary.starts_with("30 minutes on Zen") || card.summary.starts_with("31 minutes on Zen"), "{}", card.summary);
    i.notif = None;
    run(&mut i, 10 * 60_000);
    assert!(i.notif.is_none(), "not again within the period");
}

#[test]
fn the_activity_page_shows_the_day_and_the_last_half_hour() {
    let mut i = Island::new(Config::default());
    run(&mut i, 1000);
    i.push(i.now, Input::Focused { app: "code".into(), title: "main.rs".into() });
    run(&mut i, 4 * 60_000);
    i.push(i.now, Input::Notification(Incoming { id: 1, app: "Telegram".into(), summary: "Hi".into(), ..Default::default() }));
    i.push(i.now, Input::Focused { app: "zen".into(), title: "Docs".into() });
    run(&mut i, 60_000);
    i.data.clipboard = vec!["first".into()];
    run(&mut i, 3000);
    i.data.clipboard = vec!["second thing".into(), "first".into()];
    run(&mut i, 3000);
    let rows = i.rewind_rows();
    assert_eq!(rows[0].text, "second thing");
    assert_eq!(rows[0].copy.as_deref(), Some("second thing"));
    assert!(rows.iter().any(|r| r.text == "Telegram: Hi") && rows.iter().any(|r| r.text == "Code: main.rs") && rows.iter().any(|r| r.text == "Zen: Docs"));
    i.push(i.now, Input::OpenPage(Page::Activity));
    draw_frames(&mut i, 200);
    assert_eq!(i.current_face(), Face::Activity);
    assert!(press(&mut i, Action::ActivityTab(1)));
    draw_frames(&mut i, 50);
    assert!(press(&mut i, Action::RewindCopy(0)));
    assert!(i.take_commands().contains(&Command::RestoreClipboard("second thing".into())));
    let (day, totals) = i.attention_export();
    assert!(day > 0 && totals.iter().any(|(a, ms)| a == "Code" && *ms >= 4 * 60_000));
    assert!(i.pages().windows(2).any(|w| w == [Page::Sys, Page::Activity]), "the dock shows it after the system page");
}

#[test]
fn the_system_page_lists_who_uses_the_microphone_and_the_camera() {
    let mut i = Island::new(Config::default());
    i.data.system = Some(SystemStats::default());
    i.data.mic_users = vec![MicUser { stream: 7, name: "Zen".into(), pid: Some(100), muted: false }];
    i.data.camera_users = vec![("zoom".into(), vec![200, 201]), ("pipewire".into(), vec![5])];
    i.push(T0, Input::OpenPage(Page::Sys));
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::Cmd(Command::MuteMicStream { stream: 7, mute: true })));
    assert!(i.take_commands().contains(&Command::MuteMicStream { stream: 7, mute: true }));
    draw_frames(&mut i, 10);
    assert!(press(&mut i, Action::Cmd(Command::QuitProcs(vec![200, 201]))), "the camera's program can be quit");
    assert!(!i.hits.iter().any(|h| h.action == Action::Cmd(Command::QuitProcs(vec![5]))), "the session cannot");
    i.data.mic_users[0].muted = true;
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::Cmd(Command::MuteMicStream { stream: 7, mute: false })), "and it can be unmuted");
}

#[test]
fn the_drain_is_learned_while_on_battery_and_forgotten_when_plugged_in() {
    let mut i = Island::new(Config::default());
    on_battery(&mut i, 80.0);
    i.sample_battery(T0);
    assert_eq!(i.drain_rate(), None, "one sample says nothing");
    on_battery(&mut i, 76.0);
    i.sample_battery(T0 + 20 * 60_000);
    assert_eq!(i.drain_rate().map(|r| r.round()), Some(12.0), "4 points in 20 minutes");
    // plugged in: the samples are dropped, what was learned stays
    i.data.battery = Some(Battery { percent: 77.0, charging: true, on_ac: true });
    i.sample_battery(T0 + 25 * 60_000);
    assert!(i.battery_log.is_empty() && i.drain_live.is_none());
    assert_eq!(i.drain_rate().map(|r| r.round()), Some(12.0));
    i.restore_drain(Some(f64::NAN));
    assert_eq!(i.drain_saved(), None, "a bad kept value is ignored");
}

#[test]
fn the_activity_page_shows_the_week() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    let ds = i.day_start_ms(i.today_day());
    i.restore_attention_history(vec![(ds - 2 * 86_400_000, vec![("Code".into(), 3_600_000)])]);
    i.push(T0, Input::Focused { app: "zen".into(), title: String::new() });
    for _ in 0..200 {
        let t = i.now + 8;
        i.push(t, Input::OpenPage(Page::Activity));
        i.show_activity_tab(2);
        let fr = i.tick(t, 8.0);
        let mut scene = Scene::new();
        i.draw(&mut scene, &f, fr.surface, 1.0);
        assert!(fr.surface.1 < 600.0);
    }
    assert_eq!(i.hits.iter().filter(|h| matches!(h.action, Action::ActivityTab(_))).count(), 3);
    assert_eq!(i.attention_history().len(), 1);
}

#[test]
fn the_microphone_is_muted_with_a_click_or_a_word_and_stays_in_sight() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    i.data.mic_muted = Some(false);
    run(&mut i, 3000);
    assert!(i.notif.is_none(), "what it is at the start is no news");
    let hits = |i: &mut Island| {
        // a click leaves the pointer on the island, which would open it
        i.pointer_left(i.now);
        run(i, 1500);
        let t = i.now + 8;
        let fr = i.tick(t, 8.0);
        let mut scene = Scene::new();
        i.draw(&mut scene, &f, fr.surface, 1.0);
        i.hits.iter().filter(|h| h.action == Action::MicToggle).count()
    };
    assert_eq!(hits(&mut i), 0, "nothing records and it is not muted");
    i.data.privacy.mic = true;
    assert_eq!(hits(&mut i), 1);
    assert_eq!(i.activate(Action::MicToggle), [Command::SetMic(Some(true))]);
    i.tick(i.now + 8, 8.0);
    assert_eq!(i.notif.take().map(|n| n.summary), Some("Microphone muted".into()));
    i.data.privacy.mic = false;
    // long enough for the click to have stopped holding the island open
    run(&mut i, 8000);
    i.notif = None;
    assert_eq!(hits(&mut i), 1, "muted, it stays in sight");
    i.push(i.now, Input::Mic(Some(false)));
    i.tick(i.now + 8, 8.0);
    assert!(i.take_commands().contains(&Command::SetMic(Some(false))));
    assert_eq!(i.data.mic_muted, Some(false));
    // not known yet: the platform toggles
    i.data.mic_muted = None;
    assert_eq!(i.activate(Action::MicToggle), [Command::SetMic(None)]);
}

#[test]
fn plugging_in_pours_the_battery_and_then_says_how_long_to_full() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    i.data.battery = Some(Battery { percent: 62.0, charging: false, on_ac: false });
    i.data.battery_eta = Some((200, false));
    i.tick(T0 + 100, 8.0);
    i.data.battery = Some(Battery { percent: 62.0, charging: true, on_ac: true });
    i.tick(T0 + 200, 8.0);
    let n = i.notif.clone().expect("a card for the plug");
    assert_eq!((n.summary.as_str(), n.body.as_str(), n.ring, n.liquid), ("Charging", "Plugged in", Some(62), Some(Flow::Rising)));
    draw_frames(&mut i, 40);
    // the old estimate was for draining: it is not shown
    i.tick(T0 + 3500, 8.0);
    assert_eq!(card(&i).map(|c| c.1), Some("Plugged in".into()));
    i.data.battery_eta = Some((70, true));
    i.tick(T0 + 4000, 8.0);
    assert_eq!(card(&i).map(|c| c.1), Some("Full in 1h10".into()));
    draw_frames(&mut i, 5);
    // off the plug: it falls to its level, and says what is left
    i.data.battery = Some(Battery { percent: 63.0, charging: false, on_ac: false });
    i.data.battery_eta = None;
    i.tick(T0 + 20_000, 8.0);
    let n = i.notif.clone().expect("a card for the unplug");
    assert_eq!((n.summary.as_str(), n.body.as_str(), n.liquid), ("On battery", "Running on battery", Some(Flow::Falling)));
    i.data.battery_eta = Some((200, false));
    i.tick(T0 + 23_000, 8.0);
    assert_eq!(card(&i).map(|c| c.1), Some("About 3h20 left".into()));
}

#[test]
fn a_screen_plugged_in_offers_how_to_use_it_and_a_choice_waits_to_be_kept() {
    use island_core::screens::{Mode, Screen};
    let screen = |name: &str| Screen { name: name.into(), label: "Epson".into(), internal: island_core::screens::is_internal(name), on: true };
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    // only the laptop's own: nothing to say
    i.push(i.now, Input::Screens { all: vec![screen("eDP-1")], asked: false });
    assert!(card(&i).is_none());
    i.push(i.now, Input::Screens { all: vec![screen("eDP-1"), screen("HDMI-A-1")], asked: false });
    let n = i.notif.clone().expect("a card");
    assert_eq!((n.summary.as_str(), n.body.as_str()), ("Screen connected", "Epson · HDMI-A-1"));
    assert_eq!(n.actions.iter().map(|a| a.label.as_str()).collect::<Vec<_>>(), ["Extend", "External", "Laptop"]);
    i.take_commands();
    i.activate(Action::NotifAction("open-island:screens:external".into()));
    assert!(i.take_commands().contains(&Command::SetScreens(Mode::ExternalOnly)));
    i.push(i.now, Input::ScreensChanged);
    assert_eq!(card(&i).map(|c| c.0), Some("Keep the screens like this?".into()));
    i.activate(Action::NotifAction("open-island:screens:keep".into()));
    assert!(i.take_commands().contains(&Command::KeepScreens));
    // still there at the next look: no card again; asked for, it comes back
    i.notif = None;
    i.push(i.now, Input::Screens { all: vec![screen("eDP-1"), screen("HDMI-A-1")], asked: false });
    assert!(card(&i).is_none());
    run_bar(&mut i, "monitor");
    assert!(i.take_commands().contains(&Command::ShowScreens));
    i.push(i.now, Input::Screens { all: vec![screen("eDP-1"), screen("HDMI-A-1")], asked: true });
    assert_eq!(card(&i).map(|c| c.0), Some("Screen connected".into()));
}

#[test]
fn another_program_cannot_press_the_buttons_that_switch_screens() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    let mut n = incoming(7);
    n.actions = vec![NotifAction { key: "open-island:screens:external".into(), label: "OK".into() }];
    i.push(i.now, Input::Notification(n));
    i.take_commands();
    i.activate(Action::NotifAction("open-island:screens:external".into()));
    assert!(!i.take_commands().iter().any(|c| matches!(c, Command::SetScreens(_))));
}
