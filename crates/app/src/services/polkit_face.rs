//! Whether a face check is part of polkit's authentication on this system, and how the island may use it.

use island_ui::model::{AuthFaceUse, FaceChoice};
use std::path::{Path, PathBuf};

/// The PAM modules of Gaze.
const MODULES: [&str; 2] = ["pam_gaze.so", "pam_gaze_grosshack.so"];
/// Where Gaze's settings are.
const GAZE_CONFIG: &str = "/etc/gaze/config.toml";

/// The file PAM reads for a service: the system's own wins over the package's.
fn service_file(roots: &[PathBuf], service: &str) -> Option<String> {
    roots.iter().find_map(|r| std::fs::read_to_string(r.join(service)).ok())
}

/// Whether the authentication of `service` runs a face module, following what it includes.
fn stack_has_face(roots: &[PathBuf], service: &str, depth: u8) -> bool {
    let Some(text) = service_file(roots, service) else { return false };
    text.lines().map(str::trim).filter(|l| !l.starts_with('#')).any(|line| {
        let words: Vec<&str> = line.split_whitespace().collect();
        let included = match words.as_slice() {
            ["@include", other] => Some(*other),
            [kind, "include" | "substack", other, ..] if kind.trim_start_matches('-') == "auth" => Some(*other),
            _ => None,
        };
        match included {
            // a name only: nothing outside the PAM folders is followed
            Some(other) => depth > 0 && !other.contains('/') && stack_has_face(roots, other, depth - 1),
            None => words.first().is_some_and(|k| k.trim_start_matches('-') == "auth") && words.iter().any(|w| MODULES.iter().any(|m| w.ends_with(m))),
        }
    })
}

/// Whether Gaze is set to ask for a confirmation after recognising a face for polkit. Only a plain `true` counts.
fn asks_confirmation(config: &str) -> bool {
    config.lines().map(str::trim).filter(|l| !l.starts_with('#')).any(|l| l.split_once('=').is_some_and(|(k, v)| k.trim() == "require_confirmation_elevation" && v.split('#').next().is_some_and(|v| v.trim() == "true")))
}

/// What the island offers, from the setting (`polkit_face`: "ask", "auto" or "off") and what the system has. "auto" is
/// kept only where a confirmation follows the recognition: without one, a program could be authorised just by someone
/// looking at the screen.
pub fn choose(setting: &str, in_stack: bool, confirms: bool) -> AuthFaceUse {
    match setting {
        _ if !in_stack => AuthFaceUse::No,
        "off" => AuthFaceUse::No,
        "auto" if confirms => AuthFaceUse::Auto,
        _ => AuthFaceUse::Ask,
    }
}

/// The choice as the settings page shows it: what was chosen, and what that means here now.
pub fn describe(setting: &str, in_stack: bool, confirms: bool) -> FaceChoice {
    let chosen = match setting {
        "off" => AuthFaceUse::No,
        "auto" => AuthFaceUse::Auto,
        _ => AuthFaceUse::Ask,
    };
    let note = match chosen {
        _ if !in_stack => "Gaze is not in polkit's PAM stack: see the guide",
        // the island offers nothing, yet the PAM stack is not the island's to change
        AuthFaceUse::No => "Not offered. The camera may still start when you send a password (PAM file)",
        AuthFaceUse::Auto if confirms => "The camera starts with the request; a recognised face still waits for Enter",
        AuthFaceUse::Auto => "Gaze asks no confirmation here, so it still waits to be asked",
        AuthFaceUse::Ask if confirms => "Enter on the empty field, or Use face",
        AuthFaceUse::Ask => "Gaze asks no confirmation here: a face you ask for authorises at once",
    };
    FaceChoice { chosen, note: note.into() }
}

/// Where the stack stands on this system.
pub fn in_stack() -> bool {
    let roots = [Path::new("/etc/pam.d").to_path_buf(), Path::new("/usr/lib/pam.d").to_path_buf()];
    stack_has_face(&roots, "polkit-1", 3)
}

pub fn confirms() -> bool {
    std::fs::read_to_string(GAZE_CONFIG).is_ok_and(|c| asks_confirmation(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stack_of_polkit_is_read_with_what_it_includes() {
        let dir = std::env::temp_dir().join(format!("oi-pam-{}", std::process::id()));
        let (etc, lib) = (dir.join("etc"), dir.join("lib"));
        std::fs::create_dir_all(&etc).unwrap();
        std::fs::create_dir_all(&lib).unwrap();
        let roots = [etc.clone(), lib.clone()];
        std::fs::write(lib.join("polkit-1"), "#%PAM-1.0\nauth include system-auth\naccount include system-auth\n").unwrap();
        std::fs::write(etc.join("system-auth"), "# auth sufficient pam_gaze.so\nauth required pam_unix.so\naccount sufficient pam_gaze.so\n").unwrap();
        assert!(!stack_has_face(&roots, "polkit-1", 3), "a comment, or another kind of line, is not authentication");
        std::fs::write(etc.join("system-auth"), "-auth sufficient /usr/lib/security/pam_gaze.so\n").unwrap();
        assert!(stack_has_face(&roots, "polkit-1", 3), "through an include");
        std::fs::write(etc.join("system-auth"), "auth required pam_unix.so\n").unwrap();
        // the system's own file takes the package's place
        std::fs::write(etc.join("polkit-1"), "auth requisite pam_faillock.so preauth\nauth sufficient pam_gaze.so simultaneous\nauth include system-auth\n").unwrap();
        assert!(stack_has_face(&roots, "polkit-1", 3));
        // a loop of includes ends
        std::fs::write(etc.join("polkit-1"), "auth include polkit-1\n").unwrap();
        assert!(!stack_has_face(&roots, "polkit-1", 3));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_automatic_check_needs_a_confirmation_after_it() {
        assert!(asks_confirmation("[auth]\nrequire_confirmation_elevation = true\n"));
        assert!(asks_confirmation("require_confirmation_elevation=true # yes"));
        for no in ["", "# require_confirmation_elevation = true", "require_confirmation_elevation = false", "require_confirmation_lock_screen = true"] {
            assert!(!asks_confirmation(no), "{no}");
        }
        assert_eq!(choose("auto", true, true), AuthFaceUse::Auto);
        assert_eq!(choose("auto", true, false), AuthFaceUse::Ask);
        assert_eq!(choose("ask", true, true), AuthFaceUse::Ask);
        assert_eq!(choose("anything", true, true), AuthFaceUse::Ask);
        assert_eq!(choose("off", true, true), AuthFaceUse::No);
        assert_eq!(choose("auto", false, true), AuthFaceUse::No);
        // the page shows what was chosen even where it cannot be had as chosen, and says so
        let d = describe("auto", true, false);
        assert!(d.chosen == AuthFaceUse::Auto && d.note.contains("waits to be asked"), "{d:?}");
        assert!(describe("auto", false, true).note.contains("not in polkit's PAM stack"));
        assert!(describe("off", true, true).note.contains("may still start"));
    }
}
