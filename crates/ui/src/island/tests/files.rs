//! Files and what is copied: sending, receiving, the shelf, the clipboard, screenshots.

use super::*;

#[test]
fn clipboard_rows_copy_back_and_show_a_check() {
    let mut i = Island::new(Config::default());
    i.data.clipboard = vec!["one".into(), "two".into()];
    assert_eq!(i.activate(Action::ClipRestore(1)), vec![Command::RestoreClipboard("two".into())]);
    assert!(i.copied.is_some_and(|(j, until)| j == 1 && until > i.now));
    assert!(i.activate(Action::ClipRestore(9)).is_empty());
}

#[test]
fn the_clipboard_page_pins_and_forgets_an_entry() {
    let mut i = Island::new(Config::default());
    i.data.clipboard = vec!["one".into(), "two".into(), "three".into()];
    i.push(T0, Input::OpenPage(Page::Clip));
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::ClipPin(1)), "a star on each row");
    assert!(i.take_commands().contains(&Command::PinClipboard("two".into())));
    draw_frames(&mut i, 10);
    assert!(press(&mut i, Action::ClipForget(2)), "and an x");
    assert!(i.take_commands().contains(&Command::ForgetClipboard("three".into())));
    i.data.clipboard = (0..12).map(|n| n.to_string()).collect();
    draw_frames(&mut i, 200);
    assert!(i.hits.iter().any(|h| h.action == Action::ClipPin(CLIP_ROWS - 1)));
    assert!(!i.hits.iter().any(|h| h.action == Action::ClipPin(CLIP_ROWS)), "only the first rows are listed");
}

#[test]
fn files_to_send_open_a_page_of_devices_and_a_click_sends_them() {
    let mut i = Island::new(Config::default());
    let peer = |a: &str| island_core::localsend::Peer { alias: a.into(), ip: "10.0.0.2".into(), port: 53317, https: true, model: "Pixel 8".into(), kind: "mobile".into(), taildrop: false };
    i.push(T0, Input::SendFiles(vec![]));
    assert!(i.take_commands().is_empty(), "no files, no page");
    i.push(T0, Input::SendFiles(vec!["/tmp/a.txt".into(), "/tmp/b.txt".into()]));
    assert!(i.take_commands().contains(&Command::ScanPeers), "the network is scanned at once");
    i.data.peers_scanning = true;
    draw_frames(&mut i, 200);
    assert_eq!(i.current_face(), Face::Send);
    i.data.peers = vec![peer("Pixel"), peer("Laptop")];
    i.data.peers_scanning = false;
    draw_frames(&mut i, 200);
    assert!(press(&mut i, Action::SendTo(1)));
    let cmds = i.take_commands();
    assert!(cmds.contains(&Command::SendFiles { peer: peer("Laptop"), paths: vec!["/tmp/a.txt".into(), "/tmp/b.txt".into()] }));
    assert!(i.send_files.is_empty(), "the files went");
    draw_frames(&mut i, 300);
    assert_ne!(i.current_face(), Face::Send, "the page closes once the files are on their way");
}

#[test]
fn a_copied_link_or_mail_has_its_own_button_on_the_clipboard_page() {
    let mut i = Island::new(Config::default());
    i.data.clipboard = vec!["plain words".into(), "https://example.org/x".into(), "me@uni.it".into(), "#ff8000".into()];
    i.push(T0, Input::OpenPage(Page::Clip));
    draw_frames(&mut i, 200);
    assert!(!press(&mut i, Action::ClipAct(0)), "nothing to do with plain words");
    assert!(!press(&mut i, Action::ClipAct(3)), "a colour is only shown");
    assert!(press(&mut i, Action::ClipAct(1)));
    assert_eq!(i.take_commands(), vec![Command::OpenUrl("https://example.org/x".into())]);
    draw_frames(&mut i, 10);
    assert!(press(&mut i, Action::ClipAct(2)));
    assert_eq!(i.take_commands(), vec![Command::OpenUrl("mailto:me@uni.it".into())]);
}

#[test]
fn a_screenshot_card_offers_the_text_in_the_picture() {
    let shot = |id: u32| Incoming { id, app: "niri".into(), summary: "Screenshot captured".into(), body: "You can paste the image from the clipboard.".into(), ..Default::default() };
    let mut i = Island::new(Config::default());
    i.push(T0, Input::Notification(shot(7)));
    let n = i.notif.clone().expect("a card");
    assert!(n.app == "open-island" && n.actions.iter().any(|a| a.key == "open-island:ocr"), "{:?} {:?}", n.app, n.actions.len());
    assert_eq!(n.summary, "Screenshot captured", "it still says what happened");
    assert!(i.history.iter().any(|(h, _)| h.app == "niri"), "the original stays in the history");
    // its button reads the image, and the card goes
    let cmds = i.activate(Action::NotifAction("open-island:ocr".into()));
    assert_eq!(cmds, vec![Command::Ocr]);
    assert!(i.notif.is_none());
    // switched off: the card is the compositor's own
    let mut j = Island::new(Config { ocr_screenshots: false, ..Config::default() });
    j.push(T0, Input::Notification(shot(8)));
    assert_eq!(j.notif.as_ref().map(|n| n.app.as_str()), Some("niri"));
    // other notifications are not touched, and quiet time keeps the card back as it does for any
    let mut k = Island::new(Config::default());
    k.push(T0, Input::Notification(Incoming { id: 9, app: "Telegram".into(), summary: "Enrica".into(), ..Default::default() }));
    assert_eq!(k.notif.as_ref().map(|n| n.app.as_str()), Some("Telegram"));
}

#[test]
fn a_screenshots_card_offers_to_send_show_or_bin_its_file() {
    let mut i = Island::new(Config::default());
    run(&mut i, 2000);
    let shot = |file: Option<&str>| Incoming { id: 7, app: "niri".into(), summary: "Screenshot captured".into(), body: "You can paste it".into(), file: file.map(str::to_string), ..Default::default() };
    let keys = |i: &Island| i.notif.as_ref().map(|n| n.actions.iter().map(|a| (a.label.clone(), a.key.clone())).collect::<Vec<_>>()).unwrap_or_default();
    i.push(i.now, Input::Notification(shot(None)));
    assert_eq!(keys(&i), [("Copy text".to_string(), "open-island:ocr".to_string())], "without the file, as before");
    i.activate(Action::Dismiss);
    run(&mut i, 2000);
    i.push(i.now, Input::Notification(shot(Some("/home/u/Pictures/Screenshots/a b.png"))));
    let k = keys(&i);
    assert_eq!(k.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(), ["Text", "Send", "Folder", "Trash"]);
    assert_eq!(i.notif.as_ref().unwrap().body, "a b.png");
    assert_eq!(k[2].1, "open-island:file:/home/u/Pictures/Screenshots");
    i.take_commands();
    let mut all = i.activate(Action::NotifAction(k[3].1.clone()));
    all.extend(i.take_commands());
    assert!(all.contains(&Command::TrashFile("/home/u/Pictures/Screenshots/a b.png".into())), "{all:?}");
    run(&mut i, 2000);
    i.push(i.now, Input::Notification(shot(Some("/home/u/Pictures/Screenshots/a b.png"))));
    let send = keys(&i)[1].1.clone();
    i.activate(Action::NotifAction(send));
    assert_eq!(i.send_files, ["/home/u/Pictures/Screenshots/a b.png"]);
    // another program cannot use the island's own buttons
    run(&mut i, 2000);
    i.push(i.now, Input::Notification(Incoming { id: 9, app: "x".into(), summary: "hello".into(), actions: vec![NotifAction { key: "open-island:shot-trash:/etc/passwd".into(), label: "Trash".into() }], ..Default::default() }));
    i.take_commands();
    let mut all = i.activate(Action::NotifAction("open-island:shot-trash:/etc/passwd".into()));
    all.extend(i.take_commands());
    assert!(!all.iter().any(|c| matches!(c, Command::TrashFile(_))), "{all:?}");
}

#[test]
fn files_dropped_on_the_island_wait_on_the_shelf_to_be_dragged_away() {
    let f = fonts();
    let mut i = Island::new(Config::default());
    run(&mut i, 2000);
    assert!(!i.pages().contains(&Page::Shelf), "an empty shelf is not a page to go to");
    let files = |n: &[&str]| n.iter().map(|s| format!("/home/u/Documents/{s}")).collect::<Vec<_>>();
    i.push(i.now, Input::FileDrag(Some((300.0, 20.0))));
    i.push(i.now, Input::FileDropped);
    i.push(i.now, Input::Shelve(files(&["a.pdf", "b.png"])));
    i.push(i.now, Input::Shelve(files(&["c.txt", "a.pdf"])));
    assert_eq!(i.shelf(), files(&["c.txt", "a.pdf", "b.png"]), "newest first, each once");
    let draw = |i: &mut Island| {
        for _ in 0..150 {
            let t = i.now + 8;
            i.push(t, Input::OpenPage(Page::Shelf));
            let fr = i.tick(t, 8.0);
            let mut scene = Scene::new();
            i.draw(&mut scene, &f, fr.surface, 1.0);
        }
    };
    draw(&mut i);
    let row = i.hits.iter().find(|h| h.action == Action::ShelfDrag(1)).expect("a row per file").rect;
    let (x, y) = (i.pill_origin.0 + row.x0 + 60.0, i.pill_origin.1 + row.center().y);
    i.take_commands();
    assert!(i.pointer_down(i.now, x, y).is_empty(), "a press alone does nothing");
    assert!(!i.pointer_moved(i.now, x + 3.0, y).iter().any(|c| matches!(c, Command::DragFiles(_))), "a tremble is no drag");
    let cmds = i.pointer_moved(i.now, x + 20.0, y + 4.0);
    assert!(cmds.contains(&Command::DragFiles(files(&["a.pdf"]))), "{cmds:?}");
    assert!(!i.pointer_moved(i.now, x + 60.0, y).iter().any(|c| matches!(c, Command::DragFiles(_))), "once");
    assert!(i.shelf().len() == 3, "it stays on the shelf: what leaves is a copy");
    assert_eq!(i.activate(Action::ShelfOpen(0)), [Command::OpenFile(files(&["c.txt"]).remove(0))]);
    i.activate(Action::ShelfRemove(0));
    i.activate(Action::ShelfSend);
    assert_eq!(i.send_files, files(&["a.pdf", "b.png"]));
    i.activate(Action::ShelfClear);
    assert!(i.shelf().is_empty());
}

#[test]
fn an_offered_file_is_asked_about_and_the_answer_goes_back() {
    let offer = || Input::FileOffer { id: 7, summary: "Pixel wants to send".into(), body: "1 file · 3 kB\nnotes.pdf".into(), message: false };
    for (key, accept) in [("open-island:recv-yes:7", true), ("open-island:recv-no:7", false)] {
        // even while quiet: the person is the one sending
        let mut i = Island::new(Config::default());
        i.push(T0, Input::SetDnd(true));
        i.push(T0, offer());
        run(&mut i, 1500);
        let card = i.notif.clone().expect("a card");
        assert_eq!((card.id, card.actions.iter().map(|a| a.label.as_str()).collect::<Vec<_>>()), (offer_card(7), vec!["Accept", "Decline"]));
        // a stray click on the card is no answer, and neither is time
        i.pending.clear();
        i.activate(Action::NotifOpen);
        run(&mut i, 40_000);
        assert!(i.notif.is_some());
        i.pending.clear();
        i.activate(Action::NotifAction(key.into()));
        assert_eq!(i.pending.first(), Some(&Command::AnswerOffer { id: 7, accept }), "the answer comes before the card closes");
        assert!(i.notif.is_none());
    }
    // taken away by hand: the platform layer hears that the card closed
    let mut i = Island::new(Config::default());
    i.push(T0, offer());
    run(&mut i, 1500);
    i.pending.clear();
    i.push(i.now, Input::DismissCard);
    assert!(i.pending.contains(&Command::NotificationClosed { id: offer_card(7), reason: CloseReason::Dismissed }));
    assert_eq!(card_offer(offer_card(7)), Some(7));
    assert_eq!(card_offer(OWN_ID_BASE + 1), None);
}

#[test]
fn received_files_go_on_the_shelf_without_opening_it() {
    let mut i = Island::new(Config::default());
    i.push(T0, Input::ShelveQuietly(vec!["/tmp/a".into(), "/tmp/b".into()]));
    assert_eq!(i.shelf(), ["/tmp/a", "/tmp/b"]);
    assert!(i.page.is_none() && i.expand_until <= T0);
    let done = |file: &str| Input::Activity(progress::State::Done, progress::Activity { id: "receive".into(), label: "Received".into(), file: Some(file.into()), ..Default::default() });
    let labels = |i: &Island| i.notif.as_ref().map(|n| n.actions.iter().map(|a| a.label.clone()).collect::<Vec<_>>()).unwrap_or_default();
    i.push(T0, done("/tmp/in/a.pdf"));
    assert_eq!(labels(&i), ["Open", "Show in folder"]);
    let mut j = Island::new(Config::default());
    j.push(T0, done("/tmp/in/"));
    assert_eq!(labels(&j), ["Show in folder"]);
}
