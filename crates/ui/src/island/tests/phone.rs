//! The iPhone page.

use super::*;

#[test]
fn the_phone_page_lists_conversations_opens_one_and_sends_a_reply() {
    use island_core::phone::{Link, Msg, Thread};
    let mut i = Island::new(Config::default());
    assert!(!i.pages().contains(&Page::Phone), "no phone, no page");
    let th = |name: &str, key: &str| Thread { name: name.into(), key: key.into(), when: "2026-10-26 09:23".into(), preview: "Ciao".into() };
    i.data.phone = Some(Phone { link: Link { present: true, messages: true, battery: Some(80), ..Default::default() }, threads: vec![th("Enrica", "tel:+39347"), th("iliad", "sender:iliad")], messages: None, at_ms: T0, ..Default::default() });
    assert!(i.pages().contains(&Page::Phone));
    i.push(T0, Input::OpenPage(Page::Phone));
    draw_frames(&mut i, 200);
    assert_eq!(i.current_face(), Face::Phone);
    let cmds = i.activate(Action::PhoneThread(1));
    assert_eq!(cmds, vec![Command::PhoneOpen("sender:iliad".into())]);
    assert_eq!(i.phone_open.as_deref(), Some("sender:iliad"));
    // the messages arrive; the page draws them and a reply is typed and sent
    i.data.phone.as_mut().unwrap().messages = Some(("sender:iliad".into(), vec![Msg { when: "2026-10-26 09:23".into(), mine: false, text: "Ciao".into() }]));
    draw_frames(&mut i, 50);
    // through the Reply button, as a click does, and the field must survive the frames that follow
    i.activate(Action::EditField(Field::PhoneReply));
    draw_frames(&mut i, 50);
    assert!(i.edit.as_ref().is_some_and(|e| e.field == Field::PhoneReply), "the reply field stays open on the Phone page");
    i.push(i.now, Input::Key(KeyInput::Text("va bene".into())));
    i.push(i.now, Input::Key(KeyInput::Enter));
    let sent: Vec<Command> = i.take_commands();
    assert!(sent.contains(&Command::PhoneSend { key: "sender:iliad".into(), text: "va bene".into() }), "{sent:?}");
    i.activate(Action::PhoneList);
    assert!(i.phone_open.is_none());
    // the phone goes away: the page is not offered any more
    i.data.phone = None;
    assert!(!i.pages().contains(&Page::Phone));
}

#[test]
fn the_phone_page_answers_calls_searches_contacts_and_starts_a_new_conversation() {
    use island_core::phone::{Call, Contact, Link};
    let mut i = Island::new(Config::default());
    i.data.phone = Some(Phone { link: Link { present: true, messages: true, calls: false, ..Default::default() }, ..Default::default() });
    i.push(T0, Input::OpenPage(Page::Phone));
    draw_frames(&mut i, 200);
    let off = i.phone_height();
    // calls are off: the page offers to turn them on
    assert!(i.hits.iter().any(|h| h.action == Action::PhoneCallDo(PhoneCall::Enable(true))));
    assert_eq!(i.activate(Action::PhoneCallDo(PhoneCall::Enable(true))), vec![Command::PhoneCall(PhoneCall::Enable(true))]);
    // a ringing call shows with the two buttons
    i.data.phone = Some(Phone { link: Link { present: true, messages: true, calls: true, ..Default::default() }, calls: vec![Call { state: "ringing".into(), number: "+39347".into(), name: "Enrica".into() }], ..Default::default() });
    draw_frames(&mut i, 200);
    assert!(i.hits.iter().any(|h| h.action == Action::PhoneCallDo(PhoneCall::Answer)) && i.hits.iter().any(|h| h.action == Action::PhoneCallDo(PhoneCall::Hangup)));
    assert!(i.phone_height() > off - 30.0 + 40.0, "room for the call row");
    // a search for a contact: typed, sent as a command, the result opens a conversation with the number
    i.activate(Action::EditField(Field::PhoneSearch));
    draw_frames(&mut i, 20);
    i.push(i.now, Input::Key(KeyInput::Text("enri".into())));
    i.push(i.now, Input::Key(KeyInput::Enter));
    assert!(i.take_commands().contains(&Command::PhoneSearch("enri".into())));
    assert!(i.phone_search);
    i.data.phone.as_mut().unwrap().contacts = Some(vec![Contact { name: "Enrica Rossi".into(), numbers: vec!["+39 347 123 4567".into()] }]);
    draw_frames(&mut i, 100);
    assert!(i.hits.iter().any(|h| h.action == Action::PhoneContact(0)));
    let cmds = i.activate(Action::PhoneContact(0));
    assert_eq!(cmds, vec![Command::PhoneOpen("tel:+393471234567".into())]);
    assert!(!i.phone_search && i.phone_open.as_deref() == Some("tel:+393471234567"));
    // in the conversation, a call button when calls are on
    draw_frames(&mut i, 100);
    assert!(i.hits.iter().any(|h| h.action == Action::PhoneCallDo(PhoneCall::Dial("+393471234567".into()))));
}

#[test]
fn the_phone_page_chooses_where_the_phones_audio_plays() {
    use island_core::phone::Link;
    let mut i = Island::new(Config::default());
    i.data.phone = Some(Phone { link: Link { present: true, messages: true, calls: true, ..Default::default() }, audio_here: Some(false), ..Default::default() });
    let without = { let mut j = Island::new(Config::default()); j.data.phone = Some(Phone { link: Link { present: true, messages: true, calls: true, ..Default::default() }, ..Default::default() }); j.phone_height() };
    assert_eq!(i.phone_height(), without + 30.0, "a row for the choice");
    i.push(T0, Input::OpenPage(Page::Phone));
    draw_frames(&mut i, 200);
    assert!(i.hits.iter().any(|h| h.action == Action::PhoneAudioDo(true)), "it offers to play it here");
    assert_eq!(i.activate(Action::PhoneAudioDo(true)), vec![Command::PhoneAudio(true)]);
}
