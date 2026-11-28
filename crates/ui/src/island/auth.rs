//! The request for an administrator's password. While one is up it is all the island does: the pill shows a lock, a drop
//! falls from it to the middle of the screen and opens into the request, keys go there alone and a click can press
//! nothing else.
//!
//! Keys reach it through `auth_key` alone, which the platform calls for a real keyboard. Keys and clicks that arrive as
//! inputs (a script on the control socket can send those) are dropped, so that no program can type into the request or
//! press its buttons. The password is held in an `island_core::secret::Secret` and leaves only in `Command::AuthSubmit`.
//! A face check, where the system has one, starts only when asked for (or by itself where a confirmation follows), and a
//! recognised face is confirmed by a key or a click.

use super::*;
use crate::model::{AuthFaceUse, AuthRequest, AuthUpdate, FaceCheck};
use island_core::droplet::{Anchor, Animator, Size};
use island_core::secret::{plain_text, Secret};
use std::sync::Arc;
use zeroize::Zeroize;

/// Keys are not taken for this long after the request appears (or a face is recognised): what was being typed elsewhere,
/// or a second press of the same key, does not land in it.
pub(crate) const AUTH_GRACE_MS: i64 = 700;
/// How long a face is looked for before the request says it was not recognised. The system does not always say: Gaze's
/// simultaneous mode is silent when it gives up. A recognition reported later still counts.
const FACE_PATIENCE_MS: i64 = 6000;
/// A request nobody touches is dismissed, so that the keyboard is not held for ever.
const AUTH_IDLE_MS: i64 = 3 * 60_000;
/// The most requests that wait behind the one shown.
const WAITING: usize = 4;
/// The width of the open request, its height without the message, and the height of a line of it.
pub(crate) const AUTH_W: f64 = 380.0;
const AUTH_BASE_H: f64 = 364.0;
const LINE_H: f64 = 17.0;
/// Characters of the message that surely fit on a line, the most lines it takes, and its longest.
const PER_LINE: usize = 44;
const MOST_LINES: usize = 5;
const MOST_CHARS: usize = 200;
/// Where its top is when the screen's height is not known, and the least it is ever below the pill.
const NEAR_TOP: f64 = 48.0;

pub(crate) struct AuthPrompt {
    pub req: AuthRequest,
    /// Whose password is asked: a position in `req.users`.
    pub user: usize,
    pub typed: Secret,
    /// Sent, and not yet answered.
    pub checking: bool,
    /// What the system asks for ("Password").
    pub asks: String,
    /// A line under the field, and whether it says something went wrong.
    pub notice: Option<(String, bool)>,
    /// The face check under way or over, if one was started.
    pub face: Option<FaceCheck>,
    /// Since when a recognised face waits to be confirmed.
    confirm_since: i64,
    /// An automatic face check has had its turn.
    face_started: bool,
    /// Since when a face is being looked for.
    looking_since: i64,
    /// The system has been asked something (a password or a face check) and has not given its verdict: another face
    /// check cannot be started, and dismissing now counts as a failed attempt.
    pub talking: bool,
    /// The lines the message has room for: counted when the request opens, so that nothing below it moves.
    pub lines: usize,
    opened_at: i64,
    touched_at: i64,
    /// The drop's motion, and when it was born.
    pub born: i64,
    pub anim: Animator,
}

/// The lines a message takes, wrapped by words as the drawing does but counting characters: the fonts are only known
/// there, so the count leaves them room to spare.
fn message_lines(message: &str) -> usize {
    let (mut lines, mut used) = (1, 0);
    for word in message.split_whitespace() {
        let mut left = word.chars().count();
        if used > 0 && used + 1 + left <= PER_LINE {
            used += 1 + left;
            continue;
        }
        if used > 0 {
            lines += 1;
        }
        // a word longer than a line is cut across several
        while left > PER_LINE {
            left -= PER_LINE;
            lines += 1;
        }
        used = left;
    }
    lines
}

/// A message from outside, made to fit: when it is too long its middle goes, so that how it ends is still read.
fn fitting_message(message: &str) -> String {
    let mut most = MOST_CHARS;
    loop {
        let shown = plain_text(message, most);
        if message_lines(&shown) <= MOST_LINES || most <= PER_LINE {
            return shown;
        }
        most -= 8;
    }
}

fn height_for(lines: usize) -> f64 {
    AUTH_BASE_H + lines as f64 * LINE_H
}

impl Island {
    /// A password is being asked: the keyboard is wanted and nothing else is shown.
    pub fn auth_open(&self) -> bool {
        self.auth.is_some()
    }

    /// Whether the keyboard really is the island's (the platform says).
    pub fn keyboard_focus(&mut self, held: bool) {
        self.auth_focus = held;
    }

    /// The height of the screen the island is on, so that a request opens in its middle.
    pub fn set_screen_height(&mut self, height: Option<f64>) {
        self.screen_height = height.filter(|h| h.is_finite() && *h > 0.0);
    }

    pub(super) fn auth_ask(&mut self, mut req: AuthRequest) {
        // everything shown comes from outside: shown plainly
        req.message = fitting_message(&req.message);
        req.action = plain_text(&req.action, 64);
        req.requester = plain_text(&req.requester, 60);
        req.users.iter_mut().for_each(|u| *u = plain_text(u, 32));
        if req.users.is_empty() || self.auth_queue.len() >= WAITING {
            self.pending.push(Command::AuthCancel(req.id));
            return;
        }
        self.auth_queue.push_back(req);
        self.auth_next(None);
    }

    /// Where the top of a request this tall is, below the pill's bottom edge: its middle on the screen's.
    fn auth_top(&self, height: f64) -> f64 {
        let pill = Face::Auth.size().1;
        self.screen_height.map_or(NEAR_TOP, |h| ((h - height) / 2.0 - TOP_MARGIN - pill).max(NEAR_TOP))
    }

    /// A drop from the pill to the middle of the screen.
    fn auth_drop(&self, height: f64) -> Animator {
        Animator::plunge(Size { w: AUTH_W, h: height, corner: 38.0 }, Anchor::pill(Face::Auth.size().1), self.auth_top(height))
    }

    /// Opens the next request waiting. `shown` is the drop of the one just closed: the next takes its place.
    fn auth_next(&mut self, shown: Option<(i64, Animator)>) {
        if self.auth.is_some() {
            return;
        }
        let Some(req) = self.auth_queue.pop_front() else {
            // nothing more: the drop goes back into the pill
            if let Some((born, mut anim)) = shown {
                anim.exit((self.now - born) as f64 / 1000.0, Anchor::pill(Face::Auth.size().1));
                self.auth_leaving = Some((born, anim));
            }
            return;
        };
        self.auth_leaving = None;
        let user = req.chosen.min(req.users.len().saturating_sub(1));
        let lines = message_lines(&req.message).min(MOST_LINES);
        let height = height_for(lines);
        let (born, anim) = match shown {
            // the one open takes the new one's size where it is
            Some((born, mut anim)) => {
                let at = (self.now - born) as f64 / 1000.0;
                anim.resize(Size { w: AUTH_W, h: height, corner: 38.0 }, at);
                anim.move_to(self.auth_top(height), at);
                (born, anim)
            }
            None => (self.now, self.auth_drop(height)),
        };
        self.auth = Some(AuthPrompt { req, user, typed: Secret::new(), checking: false, asks: "Password".into(), notice: None, face: None, confirm_since: 0, face_started: false, looking_since: 0, talking: false, lines, opened_at: self.now, touched_at: self.now, born, anim });
        self.dragging = None;
    }

    pub(super) fn auth_update(&mut self, id: u64, update: AuthUpdate) {
        if matches!(update, AuthUpdate::Gone) {
            self.auth_queue.retain(|r| r.id != id);
        }
        let now = self.now;
        let Some(a) = self.auth.as_mut().filter(|a| a.req.id == id) else { return };
        a.touched_at = now;
        match update {
            AuthUpdate::Prompt(asks) => {
                a.checking = false;
                a.asks = plain_text(asks.trim_end_matches([':', ' ']), 48);
                a.typed.clear();
            }
            AuthUpdate::Ready => a.checking = false,
            AuthUpdate::Face(check) => {
                a.notice = None;
                // a confirmation wants the person; the rest leaves a password already sent being checked
                if check == FaceCheck::Confirm {
                    a.checking = false;
                    if a.face != Some(FaceCheck::Confirm) {
                        a.confirm_since = now;
                    }
                }
                a.face = Some(match check {
                    FaceCheck::Failed(why) => FaceCheck::Failed(plain_text(&why, 50)),
                    other => other,
                });
            }
            AuthUpdate::Info(text) => a.notice = Some((plain_text(&text, 70), false)),
            AuthUpdate::Error(text) => a.notice = Some((plain_text(&text, 70), true)),
            AuthUpdate::Wrong(left) => {
                // that conversation is over: a new one can be started
                a.talking = false;
                a.checking = false;
                a.face = None;
                a.typed.clear();
                a.notice = Some((format!("That was not it · {left} {} left", if left == 1 { "try" } else { "tries" }), true));
            }
            AuthUpdate::Gone => {
                let shown = self.auth.take().map(|a| (a.born, a.anim));
                self.auth_next(shown);
            }
        }
    }

    /// A key of the real keyboard, while a password is asked. What was typed is wiped once it is taken.
    pub fn auth_key(&mut self, now: i64, k: KeyInput) -> Vec<Command> {
        self.now = now;
        let Some(a) = self.auth.as_mut() else { return vec![] };
        if k == KeyInput::Escape {
            self.auth_dismiss();
            return std::mem::take(&mut self.pending);
        }
        let ready = !a.checking && now - a.opened_at >= AUTH_GRACE_MS;
        a.touched_at = now;
        match k {
            KeyInput::Text(mut typed) => {
                if ready && a.typed.push(&typed) {
                    a.notice = None;
                }
                typed.zeroize();
            }
            KeyInput::Backspace if ready => a.typed.pop(),
            KeyInput::NextPage | KeyInput::PrevPage if ready => self.auth_next_user(),
            // Enter does the one thing there is to do: send what was typed, else confirm the face, else look for it
            KeyInput::Enter if ready => {
                if !a.typed.is_empty() {
                    self.auth_submit();
                } else if a.face == Some(FaceCheck::Confirm) {
                    self.auth_confirm();
                } else {
                    self.auth_face();
                }
            }
            _ => {}
        }
        std::mem::take(&mut self.pending)
    }

    pub(super) fn auth_submit(&mut self) {
        let now = self.now;
        let Some(a) = self.auth.as_mut().filter(|a| !a.checking && !a.typed.is_empty() && now - a.opened_at >= AUTH_GRACE_MS) else { return };
        a.checking = true;
        a.talking = true;
        a.notice = None;
        let secret = Arc::new(std::mem::take(&mut a.typed));
        self.pending.push(Command::AuthSubmit { id: a.req.id, user: a.user, secret });
    }

    /// Whether a face check can be started now: only before the system has been asked anything.
    pub(crate) fn auth_can_look(&self) -> bool {
        self.auth.as_ref().is_some_and(|a| a.req.face != AuthFaceUse::No && !a.talking && !a.checking && a.face.is_none())
    }

    pub(super) fn auth_face(&mut self) {
        let ready = self.auth_can_look() && self.auth.as_ref().is_some_and(|a| self.now - a.opened_at >= AUTH_GRACE_MS);
        let Some(a) = self.auth.as_mut().filter(|_| ready) else { return };
        a.face = Some(FaceCheck::Looking);
        a.looking_since = self.now;
        a.face_started = true;
        a.talking = true;
        a.notice = None;
        self.pending.push(Command::AuthFace { id: a.req.id, user: a.user });
    }

    /// The person agrees, after their face was recognised.
    pub(super) fn auth_confirm(&mut self) {
        let now = self.now;
        let Some(a) = self.auth.as_mut().filter(|a| a.face == Some(FaceCheck::Confirm) && now - a.confirm_since >= AUTH_GRACE_MS) else { return };
        a.face = Some(FaceCheck::Recognised);
        a.checking = true;
        self.pending.push(Command::AuthConfirm(a.req.id));
    }

    /// Another of the users that may answer: what was typed, and the face looked for, were for the one before.
    pub(super) fn auth_next_user(&mut self) {
        // a check under way is for the user it started with: it is not switched under it
        if let Some(a) = self.auth.as_mut().filter(|a| !a.checking && !a.talking && a.req.users.len() > 1) {
            a.user = (a.user + 1) % a.req.users.len();
            a.typed.clear();
            a.notice = None;
        }
    }

    /// Whether "another user" is on offer.
    pub(crate) fn auth_can_switch(&self) -> bool {
        self.auth.as_ref().is_some_and(|a| a.req.users.len() > 1 && !a.talking && !a.checking)
    }

    pub(super) fn auth_dismiss(&mut self) {
        let Some(a) = self.auth.take() else { return };
        self.pending.push(Command::AuthCancel(a.req.id));
        self.auth_next(Some((a.born, a.anim)));
    }

    /// Once a frame: a request left alone goes, an automatic face check starts, the drop that left is forgotten.
    pub(super) fn auth_tick(&mut self, now: i64) {
        if self.auth.as_ref().is_some_and(|a| now - a.touched_at > AUTH_IDLE_MS) {
            self.auth_dismiss();
        }
        if self.auth.as_ref().is_some_and(|a| a.req.face == AuthFaceUse::Auto && !a.face_started) {
            self.auth_face();
        }
        if let Some(a) = self.auth.as_mut().filter(|a| a.face == Some(FaceCheck::Looking) && !a.checking && now - a.looking_since >= FACE_PATIENCE_MS) {
            a.face = Some(FaceCheck::Failed("Face not recognised".into()));
        }
        if self.auth_leaving.as_ref().is_some_and(|(born, anim)| (now - born) as f64 / 1000.0 >= anim.end()) {
            self.auth_leaving = None;
        }
    }

    /// The drop is moving, or something in the request is.
    pub(super) fn auth_moving(&self) -> bool {
        let busy = self.auth.as_ref().is_some_and(|a| (self.now - a.born) as f64 / 1000.0 < a.anim.end() || a.face == Some(FaceCheck::Looking) || a.checking);
        busy || self.auth_leaving.is_some()
    }

    /// The drop as it is now: of the request that is up, or of the one on its way back.
    pub(crate) fn auth_motion(&self) -> Option<(f64, &Animator)> {
        let found = self.auth.as_ref().map(|a| (a.born, &a.anim)).or(self.auth_leaving.as_ref().map(|(born, anim)| (*born, anim)));
        found.map(|(born, anim)| (((self.now - born) as f64 / 1000.0).max(0.0), anim))
    }

    /// The room the surface needs for it: its width, and how far below the pill it reaches.
    pub(super) fn auth_reach(&self) -> Option<(f64, f64)> {
        self.auth_motion().map(|(_, a)| (AUTH_W + 40.0, a.top() + a.size().h + 40.0))
    }

    /// Where the open request takes the pointer, in surface coordinates.
    pub(super) fn auth_rect(&self) -> Option<(f64, f64, f64, f64)> {
        let (t, anim) = self.auth_motion().filter(|_| self.auth.is_some())?;
        let s = anim.shape(t);
        let (w, h) = (self.spring.w.pos, self.spring.h.pos);
        (s.opened > 0.5).then(|| (self.pill_origin.0 + w / 2.0 + s.x - s.w / 2.0, self.pill_origin.1 + h + s.top, s.w, s.h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_message_keeps_its_end_and_fits_its_lines() {
        let command = format!("Authentication is needed to run `/home/u/Downloads/{}/installer' as the super user", "a-very-long-folder-name-".repeat(9));
        let shown = fitting_message(&command);
        assert!(shown.starts_with("Authentication is needed") && shown.ends_with("installer' as the super user"), "{shown}");
        assert!(message_lines(&shown) <= MOST_LINES, "{}", message_lines(&shown));
        let words = "word ".repeat(100);
        assert!(message_lines(&fitting_message(&words)) <= MOST_LINES);
        assert_eq!(message_lines("short"), 1);
        assert_eq!(message_lines(&"x".repeat(PER_LINE + 1)), 2);
        assert_eq!(message_lines(&format!("{} {}", "x".repeat(30), "y".repeat(30))), 2, "a word that does not fit goes to the next line");
        assert_eq!(message_lines(""), 1);
    }
}

