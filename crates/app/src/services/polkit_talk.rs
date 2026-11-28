//! The conversation with polkit's helper (see `polkit`), as a state machine: what it says comes in, what to answer and
//! what to tell the person come out. It touches no socket, so every path can be tried without one.
//!
//! The helper runs PAM. With a face recognition module in the stack (Gaze's `pam_gaze`) the helper may first look for a
//! face and say so in info lines; if the module asks for a confirmation, it asks through a password prompt: the answer
//! that confirms is a fixed word, anything else is taken as the password.
//!
//! The rules that matter:
//! * the password goes only to a hidden prompt that asks for a password, and once;
//! * the confirmation goes only when the module asked for it, a prompt is waiting and the person said yes;
//! * nothing is sent that the person did not send or say yes to.

use island_core::secret::Secret;
use island_ui::model::FaceCheck;
use std::sync::Arc;

/// What the helper says, besides lines for the person.
#[derive(Debug, PartialEq)]
pub enum Tell {
    /// It asks for something else than a password: to be typed.
    Asks(String),
    /// It waits for the password.
    Ready,
    Face(FaceCheck),
    Info(String),
    Error(String),
    Accepted,
    Refused,
    /// Refused because the account is locked after too many failures: trying again now is no use. What the system said.
    Locked(String),
    /// The conversation could not be held.
    Broken(String),
}

pub enum Act {
    Tell(Tell),
    /// Write the password as a line.
    Password(Arc<Secret>),
    /// Write the line that confirms a recognised face.
    Confirm(&'static str),
}

/// A prompt that asks for the password itself (the helper runs in the C locale; the others are for stacks that do not).
fn wants_password(prompt: &str) -> bool {
    let p = prompt.to_lowercase();
    ["password", "passwort", "mot de passe", "contraseña", "senha"].iter().any(|w| p.contains(w))
}

/// Whether the system says the account is locked ("The account is locked due to 3 failed logins.").
fn says_locked(line: &str) -> bool {
    line.to_lowercase().contains("account is locked")
}

/// What a line of Gaze means, and for a confirmation the answer that gives it. Gaze speaks in fixed words to an agent
/// that says it knows them (see `polkit::tell_gaze`), in English to the others: both are read.
fn face_line(line: &str) -> Option<(Option<FaceCheck>, Option<&'static str>)> {
    let failed = |why: &str| Some((Some(FaceCheck::Failed(why.to_string())), None));
    match line {
        "GAZE_MSG_LOOK_CAMERA" | "GAZE_MSG_LOOK_OR_PASSWORD" => Some((Some(FaceCheck::Looking), None)),
        "GAZE_REQUIRE_CONFIRMATION" => Some((Some(FaceCheck::Confirm), Some("GAZE_CONFIRMED"))),
        "Face Verified. Press Enter to confirm." | "Face Verified. Press Enter to confirm, Esc to cancel." => Some((Some(FaceCheck::Confirm), Some(""))),
        "GAZE_MSG_FACE_VERIFIED" | "Face Verified." => Some((Some(FaceCheck::Recognised), None)),
        "GAZE_MSG_FACE_NOT_RECOGNIZED" => failed("Face not recognised"),
        "GAZE_MSG_FACE_NOT_DETECTED" => failed("No face seen"),
        "GAZE_MSG_FACE_TOO_DARK" => failed("Too dark for a face check"),
        "GAZE_MSG_FACE_TIMED_OUT" => failed("The face check took too long"),
        "GAZE_MSG_FACE_UNAVAILABLE" => failed("Face check unavailable"),
        "GAZE_MSG_PASSWORD_REQUIRED" => failed("This needs the password"),
        l if l.starts_with("Please look at the camera") || l.starts_with("Password incorrect. Please look") => Some((Some(FaceCheck::Looking), None)),
        l if l.starts_with("Face not ") || l.starts_with("Too dark for face") || l.starts_with("Face authentication ") => failed(l.split('.').next().unwrap_or(l)),
        // a word of Gaze's that means nothing here is not shown as it is
        l if l.starts_with("GAZE_") => Some((None, None)),
        _ => None,
    }
}

/// One attempt at answering a request.
#[derive(Default)]
pub struct Talk {
    /// Confirmed by the person and not yet asked for.
    password: Option<Arc<Secret>>,
    /// A prompt waits for an answer.
    waiting: bool,
    /// The waiting prompt is for a password, shown as dots.
    for_password: bool,
    /// The answer that confirms a recognised face, once the module has asked for it.
    confirm: Option<&'static str>,
    /// Started to look for a face, and nothing has been said about one yet.
    expects_face: bool,
    /// The system's words about a locked account (`pam_faillock`, which refuses before anything else runs).
    locked: Option<String>,
}

impl Talk {
    /// An attempt that starts with the password typed, or (None) one that lets the face check run first.
    pub fn new(password: Option<Arc<Secret>>) -> Self {
        Self { expects_face: password.is_none(), password, ..Default::default() }
    }

    /// A line from the helper.
    pub fn heard(&mut self, line: &str) -> Vec<Act> {
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        match word {
            "PAM_PROMPT_ECHO_OFF" | "PAM_PROMPT_ECHO_ON" => {
                self.waiting = true;
                self.for_password = word == "PAM_PROMPT_ECHO_OFF" && wants_password(rest);
                if !self.for_password {
                    // never the password in its place
                    self.password = None;
                    return vec![Act::Tell(Tell::Asks(rest.to_string()))];
                }
                match self.password.take() {
                    Some(p) => {
                        self.waiting = false;
                        vec![Act::Password(p)]
                    }
                    None => {
                        let mut told = vec![];
                        // the password is asked and no face was ever mentioned: the module did not run (nobody enrolled)
                        if std::mem::take(&mut self.expects_face) {
                            told.push(Act::Tell(Tell::Face(FaceCheck::Failed("The face check did not start".into()))));
                        }
                        told.push(Act::Tell(Tell::Ready));
                        if self.confirm.is_some() {
                            told.push(Act::Tell(Tell::Face(FaceCheck::Confirm)));
                        }
                        told
                    }
                }
            }
            "PAM_ERROR_MSG" | "PAM_TEXT_INFO" if says_locked(rest) => {
                self.locked = Some(rest.trim().to_string());
                self.expects_face = false;
                vec![Act::Tell(Tell::Error(rest.to_string()))]
            }
            "PAM_ERROR_MSG" | "PAM_TEXT_INFO" => match face_line(rest.trim()) {
                Some((check, answer)) => {
                    self.expects_face = false;
                    let offered = match &check {
                        // offered to the person only while a prompt waits for the answer
                        Some(FaceCheck::Confirm) => {
                            self.confirm = answer;
                            self.waiting && self.for_password
                        }
                        Some(_) => {
                            self.confirm = None;
                            true
                        }
                        None => false,
                    };
                    check.filter(|_| offered).map(|c| Act::Tell(Tell::Face(c))).into_iter().collect()
                }
                None if word == "PAM_ERROR_MSG" => vec![Act::Tell(Tell::Error(rest.to_string()))],
                None => vec![Act::Tell(Tell::Info(rest.to_string()))],
            },
            "SUCCESS" if rest.is_empty() => vec![Act::Tell(Tell::Accepted)],
            "FAILURE" if rest.is_empty() => vec![Act::Tell(self.locked.take().map_or(Tell::Refused, Tell::Locked))],
            _ => vec![Act::Tell(Tell::Broken("polkit's helper said something unknown".into()))],
        }
    }

    /// The person sent what they typed.
    pub fn typed(&mut self, secret: Arc<Secret>) -> Vec<Act> {
        if self.waiting {
            self.waiting = false;
            self.confirm = None;
            return vec![Act::Password(secret)];
        }
        // the face check is still at work: kept for when the password is asked
        self.password = Some(secret);
        vec![]
    }

    /// The person said yes to a recognised face.
    pub fn confirmed(&mut self) -> Vec<Act> {
        match self.confirm {
            Some(answer) if self.waiting && self.for_password => {
                self.waiting = false;
                self.confirm = None;
                vec![Act::Confirm(answer)]
            }
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(s: &str) -> Arc<Secret> {
        let mut x = Secret::new();
        assert!(x.push(s));
        Arc::new(x)
    }

    /// What came out, in short: "password:x", "confirm:x" or what is told.
    fn said(acts: Vec<Act>) -> Vec<String> {
        acts.into_iter()
            .map(|a| match a {
                Act::Password(p) => format!("password:{}", String::from_utf8_lossy(p.expose())),
                Act::Confirm(c) => format!("confirm:{c}"),
                Act::Tell(t) => format!("{t:?}"),
            })
            .collect()
    }

    #[test]
    fn a_password_confirmed_goes_to_the_prompt_for_it_and_only_once() {
        let mut t = Talk::new(Some(secret("hunter2")));
        assert_eq!(said(t.heard("PAM_PROMPT_ECHO_OFF Password: ")), ["password:hunter2"]);
        assert_eq!(said(t.heard("PAM_PROMPT_ECHO_OFF Password: ")), ["Ready"], "asked again, the person is asked again");
        assert_eq!(said(t.typed(secret("two"))), ["password:two"]);
        assert_eq!(said(t.heard("SUCCESS")), ["Accepted"]);
    }

    #[test]
    fn the_password_never_answers_another_question() {
        let mut t = Talk::new(Some(secret("hunter2")));
        assert_eq!(said(t.heard("PAM_PROMPT_ECHO_OFF Enter PIN: ")), ["Asks(\"Enter PIN: \")"]);
        assert_eq!(said(t.heard("PAM_PROMPT_ECHO_OFF Password: ")), ["Ready"], "and it is gone");
        let mut t = Talk::new(Some(secret("hunter2")));
        assert_eq!(said(t.heard("PAM_PROMPT_ECHO_ON Password: ")), ["Asks(\"Password: \")"], "one that would show it");
    }

    #[test]
    fn a_face_recognised_waits_for_a_yes_and_gets_the_word_that_was_asked() {
        // Gaze in sequence: it looks, then asks to confirm through a password prompt
        let mut t = Talk::new(None);
        assert_eq!(said(t.heard("PAM_TEXT_INFO GAZE_MSG_LOOK_OR_PASSWORD")), ["Face(Looking)"]);
        assert!(said(t.confirmed()).is_empty(), "nothing was asked yet");
        assert_eq!(said(t.heard("PAM_PROMPT_ECHO_OFF Password: ")), ["Ready"]);
        assert!(said(t.confirmed()).is_empty(), "a password prompt alone is not a request to confirm");
        assert_eq!(said(t.heard("PAM_TEXT_INFO GAZE_REQUIRE_CONFIRMATION")), ["Face(Confirm)"]);
        assert_eq!(said(t.confirmed()), ["confirm:GAZE_CONFIRMED"]);
        assert!(said(t.confirmed()).is_empty(), "once");
        assert_eq!(said(t.heard("SUCCESS")), ["Accepted"]);
        // the same in English: an empty line confirms
        let mut t = Talk::new(None);
        t.heard("PAM_PROMPT_ECHO_OFF Password: ");
        t.heard("PAM_TEXT_INFO Face Verified. Press Enter to confirm.");
        assert_eq!(said(t.confirmed()), ["confirm:"]);
    }

    #[test]
    fn a_confirmation_before_the_prompt_or_after_a_failure_sends_nothing() {
        let mut t = Talk::new(None);
        t.heard("PAM_TEXT_INFO GAZE_REQUIRE_CONFIRMATION");
        assert!(said(t.confirmed()).is_empty(), "no prompt waits");
        t.heard("PAM_PROMPT_ECHO_OFF Password: ");
        t.heard("PAM_TEXT_INFO GAZE_MSG_FACE_NOT_RECOGNIZED");
        assert!(said(t.confirmed()).is_empty(), "the face was not recognised after all");
        // a prompt that is not for a password is never answered with a confirmation
        let mut t = Talk::new(None);
        t.heard("PAM_TEXT_INFO GAZE_REQUIRE_CONFIRMATION");
        t.heard("PAM_PROMPT_ECHO_ON Name: ");
        assert!(said(t.confirmed()).is_empty());
    }

    #[test]
    fn a_failed_face_leaves_the_password_and_a_password_typed_early_waits_for_its_prompt() {
        let mut t = Talk::new(None);
        t.heard("PAM_TEXT_INFO Please look at the camera");
        assert!(said(t.typed(secret("early"))).is_empty(), "no prompt yet");
        assert_eq!(said(t.heard("PAM_ERROR_MSG Face not recognized. Enter your password.")), ["Face(Failed(\"Face not recognized\"))"]);
        assert_eq!(said(t.heard("PAM_PROMPT_ECHO_OFF Password: ")), ["password:early"]);
        assert_eq!(said(t.heard("FAILURE")), ["Refused"]);
    }

    #[test]
    fn a_face_check_that_never_starts_is_said_and_a_confirmation_is_offered_only_with_its_prompt() {
        // nobody enrolled: the module says nothing and the password is asked
        let mut t = Talk::new(None);
        assert_eq!(said(t.heard("PAM_PROMPT_ECHO_OFF Password: ")), ["Face(Failed(\"The face check did not start\"))", "Ready"]);
        // the password already answered the prompt: a confirmation that follows is not put to the person
        let mut t = Talk::new(Some(secret("pw")));
        t.heard("PAM_TEXT_INFO GAZE_MSG_LOOK_OR_PASSWORD");
        t.heard("PAM_PROMPT_ECHO_OFF Password: ");
        assert!(said(t.heard("PAM_TEXT_INFO GAZE_REQUIRE_CONFIRMATION")).is_empty());
        assert!(said(t.confirmed()).is_empty());
        // asked before its prompt, it is offered when the prompt comes
        let mut t = Talk::new(None);
        assert!(said(t.heard("PAM_TEXT_INFO GAZE_REQUIRE_CONFIRMATION")).is_empty());
        assert_eq!(said(t.heard("PAM_PROMPT_ECHO_OFF Password: ")), ["Ready", "Face(Confirm)"]);
        assert_eq!(said(t.confirmed()), ["confirm:GAZE_CONFIRMED"]);
    }

    #[test]
    fn a_locked_account_is_told_as_such_and_not_as_a_wrong_password() {
        let mut t = Talk::new(None);
        assert_eq!(said(t.heard("PAM_TEXT_INFO The account is locked due to 3 failed logins.")), ["Error(\"The account is locked due to 3 failed logins.\")"]);
        assert_eq!(said(t.heard("PAM_TEXT_INFO (10 minutes left to unlock)")), ["Info(\"(10 minutes left to unlock)\")"]);
        assert_eq!(said(t.heard("FAILURE")), ["Locked(\"The account is locked due to 3 failed logins.\")"]);
        assert!(!says_locked("Screen locked") && !says_locked("Account expired") && !says_locked("The key is locked"));
    }

    #[test]
    fn simultaneous_mode_takes_the_password_at_once() {
        let mut t = Talk::new(Some(secret("now")));
        t.heard("PAM_TEXT_INFO GAZE_MSG_LOOK_OR_PASSWORD");
        assert_eq!(said(t.heard("PAM_PROMPT_ECHO_OFF Password: ")), ["password:now"]);
    }

    #[test]
    fn other_lines_are_told_as_they_are_and_nonsense_ends_the_attempt() {
        let mut t = Talk::new(None);
        assert_eq!(said(t.heard("PAM_TEXT_INFO Swipe your finger")), ["Info(\"Swipe your finger\")"]);
        assert_eq!(said(t.heard("PAM_ERROR_MSG Account locked")), ["Error(\"Account locked\")"]);
        assert!(said(t.heard("PAM_TEXT_INFO GAZE_MSG_SOMETHING_NEW")).is_empty());
        for line in ["HELLO there", "SUCCESSFUL", "SUCCESS yes", ""] {
            assert!(said(t.heard(line))[0].starts_with("Broken"), "{line}");
        }
    }
}
