//! The request for an administrator's password: the lock in the pill, the drop that falls to the middle of the screen,
//! and what it says once open. The password is drawn as dots: only how many were typed ever reaches the drawing.
//!
//! The request is a column down its middle: an emblem that says where things stand (locked, looking for a face,
//! recognised, refused), what is being asked and by whom, whose password, the field and the two answers. Besides the
//! drop, only one thing moves: a thin arc round the emblem while the system is at work.

use super::*;
use crate::island::auth::{AuthPrompt, AUTH_W};
use crate::model::FaceCheck;
use island_core::droplet::{self, Blob};
use std::f64::consts::TAU;
use vello::kurbo::{Arc as KArc, BezPath, Point, RoundedRect, Stroke, Vec2};

const MOST_DOTS: usize = 26;
const SIDE: f64 = 24.0;
/// The radius of the emblem, and where its middle is.
const EMBLEM_R: f64 = 24.0;
const EMBLEM_Y: f64 = 54.0;
/// How wide the user's picture is.
const AVATAR: f64 = 36.0;
/// One turn of the arc round the emblem, in ms.
const TURN_MS: i64 = 1200;

/// What the emblem shows. White at rest: colour is kept for a yes (green) and a no (red).
struct Mood {
    icon: &'static str,
    colour: Color,
}

fn mood(a: &AuthPrompt) -> Mood {
    let refused = a.notice.as_ref().is_some_and(|(_, wrong)| *wrong);
    match &a.face {
        _ if a.checking => Mood { icon: "lock", colour: theme::FG },
        Some(FaceCheck::Looking) => Mood { icon: "eye", colour: theme::FG },
        Some(FaceCheck::Confirm | FaceCheck::Recognised) => Mood { icon: "eye-check", colour: theme::GREEN },
        _ if refused => Mood { icon: "lock", colour: theme::RED },
        _ => Mood { icon: "lock", colour: theme::FG },
    }
}

/// A button as wide as asked: filled with `fill` when it is the answer on offer.
fn answer(p: P, r: Rect, label: &str, fill: Option<Color>, action: Action) {
    let hot = p.hovered(r);
    let (back, text) = match fill {
        Some(c) => (with_alpha(c, if hot { 0.85 } else { 1.0 }), theme::ON_ACCENT),
        None => (if hot { theme::HOT_CHIP } else { theme::CHIP }, theme::FG),
    };
    p.rrect(r, r.height() / 2.0, back);
    p.text(label, r.center().x, r.center().y, 13.0, SEMI, text, Align::Center, r.width() as f32 - 12.0);
    p.hit(r, action);
}

impl Island {
    /// The pill while a request is up: what it waits for.
    pub(super) fn face_auth(&self, p: P, w: f64, h: f64) {
        let Some(a) = &self.auth else { return };
        let m = mood(a);
        let text = match &a.face {
            _ if a.checking => "Checking…",
            Some(FaceCheck::Looking) => "Looking for you",
            Some(FaceCheck::Confirm) => "Confirm to go on",
            _ => "Authentication",
        };
        let tw = p.fonts.measure(text, 12.0, SEMI) as f64;
        let x = (w - tw - 20.0) / 2.0;
        p.icon(m.icon, x + 6.0, h / 2.0, 13.0, m.colour);
        p.text(text, x + 20.0, h / 2.0, 12.0, SEMI, theme::FG, Align::Left, (w - 28.0) as f32);
    }

    /// The black of the drop and of the neck that joins it to the pill. Drawn before the pill, which covers where they
    /// meet it.
    pub(super) fn draw_auth_shape(&self, scene: &mut Scene, base: Affine, w: f64, h: f64, radius: f64) {
        let Some((t, anim)) = self.auth_motion() else { return };
        let s = anim.shape(t);
        let at = base * Affine::translate((w / 2.0, h));
        let pill = Blob { cx: 0.0, cy: -h / 2.0, half_w: w / 2.0, half_h: h / 2.0, radius };
        match droplet::neck_over(&pill, &s, 0.7, &self.floor_lift(w, h, radius)) {
            Some(loops) => {
                let mut path = BezPath::new();
                for l in loops {
                    for (i, (x, y)) in l.into_iter().enumerate() {
                        if i == 0 {
                            path.move_to((x, y));
                        } else {
                            path.line_to((x, y));
                        }
                    }
                    path.close_path();
                }
                scene.fill(Fill::NonZero, at, theme::BG, None, &path);
            }
            None => {
                let body = RoundedRect::new(s.x - s.w / 2.0, s.top, s.x + s.w / 2.0, s.top + s.h, s.radius);
                scene.fill(Fill::NonZero, at, theme::BG, None, &body);
                // a thin edge, so that the black request is seen against a black window
                scene.stroke(&Stroke::new(1.0), at, with_alpha(theme::FG, 0.14 * s.opened.clamp(0.0, 1.0) as f32), None, &body);
            }
        }
    }

    pub(super) fn draw_auth_contents(&mut self, scene: &mut Scene, fonts: &Fonts, hits: &mut Vec<Hit<Action>>, base: Affine, w: f64, h: f64) {
        let Some((t, anim)) = self.auth_motion().filter(|_| self.auth.is_some()) else { return };
        let s = anim.shape(t);
        let size = anim.size();
        let opacity = anim.content_opacity(t, 0.0, 0.2) as f32;
        let body = rect(w / 2.0 + s.x - s.w / 2.0, h + s.top, s.w, s.h);
        // laid out for its full size, centred on the drop as it grows
        let (ox, oy) = (body.center().x - size.w / 2.0, body.center().y - size.h / 2.0);
        let pointer = self.pointer_local().map(|(x, y)| (x - ox, y - oy));
        let at = base * Affine::translate((ox, oy));
        let mut mine = Vec::new();
        scene.push_clip_layer(Fill::NonZero, base, &RoundedRect::from_rect(body, s.radius));
        {
            let mut p = Painter { scene, fonts, hits: &mut mine, opacity, pointer, transform: at, base: (1.0, at), since_enter: 10_000 };
            self.auth_panel(&mut p, size.h);
        }
        scene.pop_layer();
        // its buttons take the pointer only once it is open
        if s.opened > 0.9 {
            hits.extend(mine.into_iter().map(|hit| Hit { rect: hit.rect + Vec2::new(ox, oy), action: hit.action }));
        }
    }

    fn auth_panel(&self, p: P, h: f64) {
        let Some(a) = &self.auth else { return };
        let (mid, room) = (AUTH_W / 2.0, AUTH_W - 2.0 * SIDE);
        self.auth_emblem(p, a, Point::new(mid, EMBLEM_Y));

        p.text("Authentication required", mid, 110.0, 15.0, SEMI, theme::FG, Align::Center, room as f32);
        let mut y = 132.0;
        for line in crate::faces::wrap(p.fonts, &a.req.message, 12.0, REG, room as f32, a.lines) {
            p.text(&line, mid, y, 12.0, REG, with_alpha(theme::FG, 0.78), Align::Center, room as f32 + 1.0);
            y += 17.0;
        }
        // the room the message may take is fixed when the request opens: what follows keeps its place
        let mut y = 132.0 + a.lines as f64 * 17.0;

        // who wants it, and the action as the system names it: the second is the part to trust
        if !a.req.requester.is_empty() {
            let tw = (p.fonts.measure(&a.req.requester, 11.0, SEMI) as f64).min(room - 40.0);
            let chip = rect(mid - (tw + 34.0) / 2.0, y, tw + 34.0, 22.0);
            p.rrect(chip, 11.0, theme::CHIP);
            p.icon("terminal-2", chip.x0 + 15.0, chip.center().y, 12.0, theme::DIM);
            p.text(&a.req.requester, chip.x0 + 26.0, chip.center().y, 11.0, SEMI, theme::FG, Align::Left, tw as f32 + 1.0);
        }
        y += 30.0;
        p.text(&a.req.action, mid, y + 2.0, 9.5, REG, with_alpha(theme::FG, 0.42), Align::Center, room as f32);
        y += 20.0;

        // whose password: their picture (or initial) and name, with the way to another when there is one
        let user = a.req.users.get(a.user).map_or("", String::as_str);
        let initial: String = user.chars().next().map_or_else(String::new, |c| c.to_uppercase().collect());
        let switch = self.auth_can_switch();
        let name_w = (p.fonts.measure(user, 13.0, SEMI) as f64).min(room - 100.0);
        let chip_w = AVATAR + 14.0 + name_w + if switch { 28.0 } else { 14.0 };
        let chip = rect(mid - chip_w / 2.0, y, chip_w, AVATAR + 6.0);
        if switch {
            p.rrect(chip, chip.height() / 2.0, if p.hovered(chip) { theme::HOT_CHIP } else { theme::CHIP });
            p.icon("chevron-right", chip.x1 - 14.0, chip.center().y, 12.0, theme::DIM);
            p.hit(chip, Action::AuthUser);
        }
        let face = Point::new(chip.x0 + 3.0 + AVATAR / 2.0, chip.center().y);
        match a.req.pictures.get(a.user).and_then(Option::as_ref) {
            Some(picture) => crate::faces::faded_image(p, picture, face.x - AVATAR / 2.0, face.y - AVATAR / 2.0, AVATAR, AVATAR / 2.0),
            None => {
                p.circle(face.x, face.y, AVATAR / 2.0, theme::HOT_CHIP);
                p.text(&initial, face.x, face.y, 14.0, SEMI, theme::FG, Align::Center, AVATAR as f32);
            }
        }
        p.circle_stroke(face.x, face.y, AVATAR / 2.0, 1.0, with_alpha(theme::FG, 0.22));
        p.text(user, chip.x0 + AVATAR + 13.0, chip.center().y, 13.0, SEMI, theme::FG, Align::Left, name_w as f32 + 1.0);
        y += AVATAR + 16.0;

        self.auth_field(p, a, rect(SIDE, y, room, 42.0));
        y += 52.0;

        let can_look = self.auth_can_look();
        let (note, colour) = match (&a.notice, &a.face) {
            (Some((text, wrong)), _) => (text.clone(), if *wrong { theme::RED } else { theme::DIM }),
            // a recognised face waits for a yes, whoever holds the keyboard
            (None, Some(FaceCheck::Confirm)) if self.auth_focus => ("Face recognised · press Enter to confirm".into(), theme::GREEN),
            (None, _) if !self.auth_focus => ("Waiting for the keyboard: nothing typed now comes here".into(), theme::ORANGE),
            // the field says it
            (None, _) if a.checking => (String::new(), theme::DIM),
            (None, Some(FaceCheck::Looking)) => ("Looking for you · or type the password".into(), theme::DIM),
            // the system is waiting for the password now, and counts a dismissal
            (None, Some(FaceCheck::Failed(why))) => (if a.talking { format!("{why} · Cancel now counts as a try") } else { why.clone() }, theme::ORANGE),
            (None, _) if can_look && a.typed.is_empty() => ("Enter on the empty field uses your face".into(), theme::DIM),
            (None, _) => ("Enter to authenticate · Esc to cancel".into(), theme::DIM),
        };
        p.text(&note, mid, y + 6.0, 10.5, REG, colour, Align::Center, room as f32);

        // the two answers, side by side at the bottom
        let half = (room - 10.0) / 2.0;
        let y = h - 18.0 - 36.0;
        answer(p, rect(SIDE, y, half, 36.0), "Cancel", None, Action::AuthCancel);
        let right = rect(SIDE + half + 10.0, y, half, 36.0);
        if a.face == Some(FaceCheck::Confirm) && a.typed.is_empty() {
            answer(p, right, "Confirm", Some(theme::GREEN), Action::AuthConfirm);
        } else {
            let ready = !a.typed.is_empty() && !a.checking;
            answer(p, right, "Authenticate", ready.then_some(theme::ACCENT), Action::AuthSubmit);
        }
    }

    /// The disc at the top: its mark and colour say where things stand.
    fn auth_emblem(&self, p: P, a: &AuthPrompt, centre: Point) {
        let m = mood(a);
        p.circle(centre.x, centre.y, EMBLEM_R, with_alpha(m.colour, if m.colour == theme::FG { 0.1 } else { 0.16 }));
        p.icon(m.icon, centre.x, centre.y, 22.0, m.colour);
        if a.checking || a.face == Some(FaceCheck::Looking) {
            let from = self.now.rem_euclid(TURN_MS) as f64 / TURN_MS as f64;
            let shape = KArc::new(centre, Vec2::new(EMBLEM_R + 4.0, EMBLEM_R + 4.0), from * TAU, 0.25 * TAU, 0.0);
            p.scene.stroke(&Stroke::new(1.6).with_caps(vello::kurbo::Cap::Round), p.transform, with_alpha(theme::FG, 0.8 * p.opacity), None, &shape);
        }
    }

    /// The field: dots in its middle, and at its right the way to use a face.
    fn auth_field(&self, p: P, a: &AuthPrompt, field: Rect) {
        let edge = match &a.face {
            _ if a.notice.as_ref().is_some_and(|(_, wrong)| *wrong) => theme::RED,
            Some(FaceCheck::Confirm) => theme::GREEN,
            _ if self.auth_focus => with_alpha(theme::FG, 0.35),
            _ => theme::TRACK,
        };
        p.rrect(field, field.height() / 2.0, theme::CHIP);
        p.rrect_stroke(field, field.height() / 2.0, 1.0, edge);
        let (cx, cy) = (field.center().x, field.center().y);
        let n = a.typed.chars().min(MOST_DOTS);
        if a.checking || n == 0 {
            let (hint, colour) = match a.face {
                _ if a.checking => ("Checking…".to_string(), theme::DIM),
                Some(FaceCheck::Confirm) => ("Face recognised".to_string(), theme::GREEN),
                _ => (a.asks.clone(), theme::DIM),
            };
            p.text(&hint, cx, cy, 12.5, REG, colour, Align::Center, (field.width() - 90.0) as f32);
        } else {
            const STEP: f64 = 11.0;
            let first = cx - (n - 1) as f64 * STEP / 2.0;
            for i in 0..n {
                p.circle(first + i as f64 * STEP, cy, 3.4, theme::FG);
            }
        }
        if self.auth_can_look() {
            let eye = Point::new(field.x1 - 21.0, cy);
            let r = rect(eye.x - 15.0, eye.y - 15.0, 30.0, 30.0);
            if p.hovered(r) {
                p.circle(eye.x, eye.y, 15.0, theme::HOT_CHIP);
            }
            p.icon("eye", eye.x, eye.y, 16.0, if p.hovered(r) { theme::FG } else { theme::DIM });
            p.hit(r, Action::AuthFace);
        }
    }
}
