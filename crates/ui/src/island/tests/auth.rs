//! The password request: who can type into it and press it, and what becomes of what is typed.

use super::*;
use crate::model::{AuthFaceUse, AuthRequest, AuthUpdate, FaceCheck};

fn request(id: u64) -> AuthRequest {
    AuthRequest {
        id,
        action: "org.freedesktop.policykit.exec".into(),
        message: "Authentication is needed to run `/usr/bin/true' as the super user".into(),
        requester: "zsh (pid 42)".into(),
        users: vec!["me".into(), "root".into()],
        chosen: 0,
        pictures: vec![],
        face: AuthFaceUse::No,
    }
}

/// An island with a request up, past the moment in which keys are not yet taken.
fn asked() -> Island {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    i.push(T0, Input::AuthAsk(request(1)));
    i.keyboard_focus(true);
    i.tick(T0 + 1000, 8.0);
    i
}

fn type_password(i: &mut Island, text: &str) {
    for c in text.chars() {
        let sent = i.auth_key(i.now, KeyInput::Text(c.to_string()));
        assert!(sent.is_empty());
    }
}

fn submitted(cmds: &[Command]) -> Option<(u64, usize, Vec<u8>)> {
    cmds.iter().find_map(|c| match c {
        Command::AuthSubmit { id, user, secret } => Some((*id, *user, secret.expose().to_vec())),
        _ => None,
    })
}

#[test]
fn the_password_typed_on_the_keyboard_leaves_once_and_is_not_kept() {
    let mut i = asked();
    assert!(i.auth_open() && i.wants_keyboard());
    assert!(i.snapshot().0.authing);
    type_password(&mut i, "hunterx");
    i.auth_key(i.now, KeyInput::Backspace);
    type_password(&mut i, "2");
    let cmds = i.auth_key(i.now, KeyInput::Enter);
    assert_eq!(submitted(&cmds), Some((1, 0, b"hunter2".to_vec())));
    let a = i.auth.as_ref().expect("still up while it is checked");
    assert!(a.checking && a.typed.is_empty());
    // nothing more is taken or sent while it is checked
    type_password(&mut i, "more");
    assert!(i.auth_key(i.now, KeyInput::Enter).is_empty());
    assert!(!format!("{cmds:?}").contains("hunter"), "a command that is logged does not show it");
    i.push(i.now, Input::AuthState { id: 1, update: AuthUpdate::Gone });
    assert!(!i.auth_open() && !i.wants_keyboard());
}

#[test]
fn keys_and_clicks_sent_by_programs_do_not_reach_the_request() {
    let mut i = asked();
    draw_frames(&mut i, 200);
    i.take_commands();
    i.push(i.now, Input::Key(KeyInput::Text("injected".into())));
    assert!(i.auth.as_ref().unwrap().typed.is_empty());
    type_password(&mut i, "abc");
    i.push(i.now, Input::Key(KeyInput::Enter));
    i.push(i.now, Input::Key(KeyInput::Escape));
    let (ox, oy) = i.pill_origin;
    for wanted in [Action::AuthSubmit, Action::AuthCancel] {
        let r = i.hits.iter().find(|h| h.action == wanted).expect("drawn").rect;
        i.push(i.now, Input::Click(r.center().x + ox, r.center().y + oy));
    }
    // the command bar opened over it takes nothing either
    i.push(i.now, Input::OpenBar);
    i.push(i.now, Input::Key(KeyInput::Text("x".into())));
    assert!(i.take_commands().is_empty());
    assert!(i.auth.as_ref().is_some_and(|a| !a.checking && a.typed.chars() == 3));
    assert!(i.snapshot().0.authing);
}

#[test]
fn only_its_own_buttons_can_be_pressed_while_it_is_up() {
    let mut i = asked();
    assert!(i.activate(Action::Open(Page::Plan)).is_empty());
    assert!(i.activate(Action::ToggleDnd).is_empty());
    assert!(i.activate(Action::NotifAction("open-island:screens:external".into())).is_empty());
    i.right_click(i.now);
    assert!(i.middle_click(i.now).is_empty() && i.scroll(i.now, 0.0, 5.0).is_empty());
    assert!(i.auth_open());
    draw_frames(&mut i, 200);
    assert!(i.hits.iter().all(|h| matches!(h.action, Action::AuthSubmit | Action::AuthCancel | Action::AuthUser)), "{:?}", i.hits.iter().map(|h| &h.action).collect::<Vec<_>>());
    type_password(&mut i, "pw");
    assert!(press(&mut i, Action::AuthSubmit));
    assert_eq!(submitted(&i.take_commands()), Some((1, 0, b"pw".to_vec())));
}

#[test]
fn what_was_being_typed_elsewhere_does_not_land_in_it_and_an_empty_password_is_not_sent() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    i.push(T0, Input::AuthAsk(request(1)));
    assert!(i.auth_key(T0 + 10, KeyInput::Text("a".into())).is_empty());
    assert!(i.auth_key(T0 + 20, KeyInput::Enter).is_empty());
    assert!(i.auth.as_ref().unwrap().typed.is_empty());
    assert!(i.auth_key(T0 + 600, KeyInput::Enter).is_empty(), "nothing typed");
    // Escape works at once
    i.push(T0 + 700, Input::AuthAsk(request(2)));
    assert_eq!(i.auth_key(T0 + 701, KeyInput::Escape), vec![Command::AuthCancel(1)]);
    assert_eq!(i.auth.as_ref().map(|a| a.req.id), Some(2), "the next one waited");
}

#[test]
fn a_wrong_password_another_user_and_another_question_start_from_nothing() {
    let mut i = asked();
    type_password(&mut i, "bad");
    i.auth_key(i.now, KeyInput::Enter);
    i.push(i.now, Input::AuthState { id: 1, update: AuthUpdate::Wrong(2) });
    let a = i.auth.as_ref().unwrap();
    assert!(!a.checking && a.typed.is_empty());
    assert_eq!(a.notice, Some(("That was not it · 2 tries left".into(), true)));
    type_password(&mut i, "half");
    i.auth_key(i.now, KeyInput::NextPage);
    assert!(i.auth.as_ref().is_some_and(|a| a.user == 1 && a.typed.is_empty()), "typed for the other one");
    type_password(&mut i, "again");
    i.push(i.now, Input::AuthState { id: 1, update: AuthUpdate::Prompt("PIN: ".into()) });
    assert!(i.auth.as_ref().is_some_and(|a| a.asks == "PIN" && a.typed.is_empty()));
    // word of another request changes nothing
    i.push(i.now, Input::AuthState { id: 9, update: AuthUpdate::Gone });
    assert!(i.auth_open());
}

#[test]
fn it_goes_when_withdrawn_or_left_alone_and_only_so_many_wait() {
    let mut i = asked();
    for id in 2..=8 {
        i.push(i.now, Input::AuthAsk(request(id)));
    }
    let refused: Vec<u64> = i.take_commands().iter().filter_map(|c| if let Command::AuthCancel(id) = c { Some(*id) } else { None }).collect();
    assert_eq!(refused, [6, 7, 8]);
    i.push(i.now, Input::AuthState { id: 3, update: AuthUpdate::Gone });
    i.push(i.now, Input::AuthState { id: 1, update: AuthUpdate::Gone });
    assert_eq!(i.auth.as_ref().map(|a| a.req.id), Some(2));
    i.push(i.now, Input::AuthState { id: 2, update: AuthUpdate::Gone });
    assert_eq!(i.auth.as_ref().map(|a| a.req.id), Some(4), "the withdrawn one is skipped");
    i.take_commands();
    i.tick(i.now + 3 * 60_000 + 1000, 8.0);
    assert!(i.take_commands().contains(&Command::AuthCancel(4)));
    // a request that names nobody is refused
    let mut j = Island::new(Config::default());
    j.push(T0, Input::AuthAsk(AuthRequest { users: vec![], ..request(1) }));
    assert!(!j.auth_open() && j.take_commands() == [Command::AuthCancel(1)]);
}

#[test]
fn no_card_shows_beside_the_request_and_text_from_outside_is_shown_plainly() {
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    let mut r = request(1);
    r.message = "run `/bin/\u{202e}evil`\nPassword accepted".into();
    i.push(T0, Input::AuthAsk(r));
    assert_eq!(i.auth.as_ref().unwrap().req.message, "run `/bin/evil` Password accepted");
    i.push(T0 + 10, Input::Notification(incoming(7)));
    draw_frames(&mut i, 200);
    assert!(!i.has_card() && i.input_rects().len() == 2, "the pill and the request: {:?}", i.input_rects());
    i.auth_key(i.now, KeyInput::Escape);
    draw_frames(&mut i, 5);
    assert!(i.has_card(), "it comes once the request is gone");
}

const MID: f64 = 1000.0;

/// An island on a screen 1000 tall with a request that a face check can answer.
fn asked_with_face(face: AuthFaceUse) -> Island {
    let mut i = Island::new(Config::default());
    i.set_screen_height(Some(MID));
    i.tick(T0, 8.0);
    i.push(T0, Input::AuthAsk(AuthRequest { face, ..request(1) }));
    i.keyboard_focus(true);
    i
}

fn face_commands(cmds: &[Command]) -> Vec<&'static str> {
    cmds.iter()
        .filter_map(|c| match c {
            Command::AuthFace { .. } => Some("look"),
            Command::AuthConfirm(_) => Some("confirm"),
            Command::AuthSubmit { .. } => Some("password"),
            _ => None,
        })
        .collect()
}

#[test]
fn a_drop_falls_from_the_pill_to_the_middle_of_the_screen_and_goes_back() {
    let mut i = asked_with_face(AuthFaceUse::No);
    let (surface_w, surface_h) = draw_frames(&mut i, 300);
    let (pw, ph) = i.face_size(Face::Auth);
    assert!(ph < 40.0 && pw < 260.0, "the pill stays small");
    let (x, y, w, h) = i.auth_rect().expect("open");
    assert!((w - 380.0).abs() < 2.0 && (380.0..470.0).contains(&h), "{w} x {h}");
    assert!((y + h / 2.0 - MID / 2.0).abs() < 4.0, "its middle is the screen's: {y} + {h} / 2");
    assert!(surface_h >= y + h && surface_w >= w && x >= 0.0, "the surface holds it: {surface_w} x {surface_h}");
    draw_frames(&mut i, 5);
    assert!(press(&mut i, Action::AuthCancel), "its buttons are where it is");
    assert!(i.auth_rect().is_none() && i.auth_leaving.is_some(), "going: it takes no more clicks");
    let (_, tall) = draw_frames(&mut i, 5);
    assert!(tall > 200.0, "still room for it on the way up");
    let (_, after) = draw_frames(&mut i, 400);
    assert!(i.auth_leaving.is_none() && after < 200.0, "{after}");
}

#[test]
fn without_the_screens_height_it_opens_just_under_the_pill_and_one_request_takes_the_others_place() {
    let mut i = asked();
    draw_frames(&mut i, 300);
    let y = i.auth_rect().expect("open").1;
    assert!(y < 150.0, "{y}");
    i.push(i.now, Input::AuthAsk(request(2)));
    i.push(i.now, Input::AuthState { id: 1, update: AuthUpdate::Gone });
    draw_frames(&mut i, 5);
    assert!(i.auth_rect().is_some_and(|r| (r.1 - y).abs() < 2.0), "already open: no second fall");
}

#[test]
fn a_face_is_looked_for_only_when_asked_and_a_recognised_one_waits_for_a_yes() {
    let mut i = asked_with_face(AuthFaceUse::Ask);
    i.tick(T0 + 2000, 8.0);
    assert!(face_commands(&i.take_commands()).is_empty(), "nothing starts by itself");
    assert_eq!(face_commands(&i.auth_key(i.now, KeyInput::Enter)), ["look"]);
    assert!(face_commands(&i.auth_key(i.now, KeyInput::Enter)).is_empty(), "once");
    i.push(i.now, Input::AuthState { id: 1, update: AuthUpdate::Face(FaceCheck::Looking) });
    // recognised: the second press of the same key does not confirm it
    i.push(i.now + 100, Input::AuthState { id: 1, update: AuthUpdate::Face(FaceCheck::Confirm) });
    assert!(face_commands(&i.auth_key(i.now + 50, KeyInput::Enter)).is_empty(), "too soon to be meant");
    // nor does a program
    i.push(i.now + 2000, Input::Key(KeyInput::Enter));
    assert!(face_commands(&i.take_commands()).is_empty());
    draw_frames(&mut i, 300);
    assert!(i.hits.iter().any(|h| h.action == Action::AuthConfirm));
    assert_eq!(face_commands(&i.auth_key(i.now, KeyInput::Enter)), ["confirm"]);
    assert!(i.auth.as_ref().is_some_and(|a| a.checking));
}

#[test]
fn the_password_can_be_typed_while_the_face_is_looked_for_and_after_it_fails() {
    let mut i = asked_with_face(AuthFaceUse::Ask);
    draw_frames(&mut i, 300);
    assert!(press(&mut i, Action::AuthFace), "its button");
    assert_eq!(face_commands(&i.take_commands()), ["look"]);
    type_password(&mut i, "pw");
    assert_eq!(face_commands(&i.auth_key(i.now, KeyInput::Enter)), ["password"]);
    // the face failing leaves the password sent being checked; when the system asks again the field is free
    i.push(i.now, Input::AuthState { id: 1, update: AuthUpdate::Face(FaceCheck::Failed("No face seen".into())) });
    assert!(i.auth.as_ref().is_some_and(|a| a.checking));
    i.push(i.now, Input::AuthState { id: 1, update: AuthUpdate::Ready });
    // the conversation goes on: another look cannot be had, so none is offered
    assert!(!i.auth_can_look());
    draw_frames(&mut i, 5);
    assert!(!i.hits.iter().any(|h| h.action == Action::AuthFace));
    // what is typed stays when the system comes to ask for it
    type_password(&mut i, "half");
    i.push(i.now, Input::AuthState { id: 1, update: AuthUpdate::Ready });
    assert_eq!(i.auth.as_ref().map(|a| a.typed.chars()), Some(4));
    // something typed wins over a confirmation
    i.push(i.now, Input::AuthState { id: 1, update: AuthUpdate::Face(FaceCheck::Confirm) });
    assert_eq!(face_commands(&i.auth_key(i.now + 1000, KeyInput::Enter)), ["password"]);
}

#[test]
fn an_automatic_check_starts_once_the_request_is_up_and_not_again() {
    let mut i = asked_with_face(AuthFaceUse::Auto);
    i.tick(T0 + 100, 8.0);
    assert!(face_commands(&i.take_commands()).is_empty(), "not before it can be read");
    i.tick(T0 + 900, 8.0);
    assert_eq!(face_commands(&i.take_commands()), ["look"]);
    i.push(T0 + 2000, Input::AuthState { id: 1, update: AuthUpdate::Face(FaceCheck::Failed("Too dark".into())) });
    i.tick(T0 + 3000, 8.0);
    assert!(face_commands(&i.take_commands()).is_empty(), "it does not go round");
    // where there is no face check Enter on an empty field does nothing
    let mut j = asked();
    assert!(j.auth_key(j.now, KeyInput::Enter).is_empty());
}

#[test]
fn the_user_is_not_switched_under_a_face_check() {
    let mut i = asked_with_face(AuthFaceUse::Ask);
    i.tick(T0 + 1000, 8.0);
    i.auth_key(i.now, KeyInput::NextPage);
    assert_eq!(i.auth.as_ref().map(|a| a.user), Some(1), "before it, yes");
    i.auth_key(i.now, KeyInput::Enter);
    i.auth_key(i.now, KeyInput::NextPage);
    assert_eq!(i.auth.as_ref().map(|a| a.user), Some(1));
    assert!(!i.auth_can_switch());
}

#[test]
fn the_request_is_as_tall_as_its_message_and_stays_in_the_middle() {
    let mut i = asked_with_face(AuthFaceUse::No);
    draw_frames(&mut i, 300);
    let short = i.auth_rect().expect("open");
    // the next one, with a long message, takes its place and its own height
    let mut long = request(2);
    long.message = "Authentication is needed to run `/usr/bin/some-program --with a good many arguments --and more of them` as the super user".into();
    i.push(i.now, Input::AuthAsk(long));
    i.push(i.now, Input::AuthState { id: 1, update: AuthUpdate::Gone });
    draw_frames(&mut i, 300);
    let tall = i.auth_rect().expect("open");
    assert!(tall.3 > short.3 + 15.0, "{} then {}", short.3, tall.3);
    assert!((tall.1 + tall.3 / 2.0 - MID / 2.0).abs() < 4.0, "{tall:?}");
    // everything it can be pressed on is inside it
    let oy = i.pill_origin.1;
    assert!(i.hits.iter().all(|h| h.rect.y0 + oy >= tall.1 && h.rect.y1 + oy <= tall.1 + tall.3), "{tall:?}");
}

#[test]
fn the_open_request_moves_only_while_the_system_is_at_work() {
    let mut i = asked();
    draw_frames(&mut i, 400);
    assert!(!i.auth_moving(), "at rest it asks for no frames");
    type_password(&mut i, "bad");
    assert!(!i.auth_moving(), "typing moves nothing");
    i.auth_key(i.now, KeyInput::Enter);
    i.tick(i.now + 2000, 8.0);
    assert!(i.auth_moving(), "checking");
    i.push(i.now, Input::AuthState { id: 1, update: AuthUpdate::Wrong(2) });
    assert!(!i.auth_moving(), "refused: said in red, standing still");
}

#[test]
fn the_picture_of_the_user_is_drawn_and_one_without_it_has_an_initial() {
    use crate::model::Image;
    let picture = Image { id: 77, width: 4, height: 4, rgba: std::sync::Arc::new(vec![200; 64]) };
    let mut i = Island::new(Config::default());
    i.tick(T0, 8.0);
    // the second user has none: both are drawn
    i.push(T0, Input::AuthAsk(AuthRequest { pictures: vec![Some(picture), None], ..request(1) }));
    i.keyboard_focus(true);
    draw_frames(&mut i, 300);
    assert_eq!(i.auth.as_ref().map(|a| a.req.pictures.len()), Some(2));
    i.auth_key(i.now, KeyInput::NextPage);
    draw_frames(&mut i, 5);
    assert_eq!(i.auth.as_ref().map(|a| a.user), Some(1));
    // what is printed of a request does not carry the pixels
    let text = format!("{:?}", i.auth.as_ref().map(|a| &a.req));
    assert!(text.contains("Image 4x4 #77") && !text.contains("200, 200"), "{text}");
}

#[test]
fn a_face_check_that_goes_silent_is_given_up_and_a_late_recognition_still_counts() {
    let mut i = asked_with_face(AuthFaceUse::Auto);
    i.tick(T0 + 900, 8.0);
    i.push(T0 + 950, Input::AuthState { id: 1, update: AuthUpdate::Face(FaceCheck::Looking) });
    i.take_commands();
    i.tick(T0 + 6000, 8.0);
    assert_eq!(i.auth.as_ref().and_then(|a| a.face.clone()), Some(FaceCheck::Looking), "still looking");
    i.tick(T0 + 7000, 8.0);
    assert_eq!(i.auth.as_ref().and_then(|a| a.face.clone()), Some(FaceCheck::Failed("Face not recognised".into())));
    assert!(!i.auth_moving() && !i.auth_can_look() && i.auth.as_ref().is_some_and(|a| a.talking));
    assert!(i.auth_key(i.now, KeyInput::Enter).is_empty(), "nothing to start again");
    // the system recognised it after all
    i.push(T0 + 8000, Input::AuthState { id: 1, update: AuthUpdate::Face(FaceCheck::Confirm) });
    assert_eq!(face_commands(&i.auth_key(T0 + 9000, KeyInput::Enter)), ["confirm"]);
}

#[test]
fn after_a_refusal_a_face_can_be_asked_for_again_but_does_not_start_by_itself() {
    let mut i = asked_with_face(AuthFaceUse::Auto);
    i.tick(T0 + 900, 8.0);
    i.take_commands();
    type_password(&mut i, "bad");
    i.auth_key(i.now, KeyInput::Enter);
    i.push(i.now, Input::AuthState { id: 1, update: AuthUpdate::Wrong(2) });
    i.take_commands();
    i.tick(i.now + 3000, 8.0);
    assert!(face_commands(&i.take_commands()).is_empty(), "not by itself");
    assert!(i.auth_can_look());
    assert_eq!(face_commands(&i.auth_key(i.now, KeyInput::Enter)), ["look"], "asked for: yes");
}

#[test]
fn once_a_password_was_asked_of_the_system_no_face_check_is_offered() {
    let mut i = asked_with_face(AuthFaceUse::Ask);
    i.tick(T0 + 1000, 8.0);
    type_password(&mut i, "pw");
    i.auth_key(i.now, KeyInput::Enter);
    i.push(i.now, Input::AuthState { id: 1, update: AuthUpdate::Ready });
    assert!(!i.auth_can_look() && i.auth_key(i.now, KeyInput::Enter).is_empty());
}

