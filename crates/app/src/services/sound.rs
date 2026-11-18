//! Playing sound files (notification sounds, the timer), and choosing which one a notification gets.

use crate::config::{Filter, Settings};
use std::process::{Command, Stdio};

/// The sound for a notification, or None: a sender's `sound-file` hint, then the app's own sound, then the
/// default one; nothing when the sender asked for silence, do-not-disturb is on, or a filter silences it.
#[allow(clippy::too_many_arguments)]
pub fn choose(s: &Settings, app: &str, desktop_entry: &str, summary: &str, body: &str, hint_file: Option<&str>, suppress: bool, dnd: bool, critical: bool) -> Option<String> {
    if suppress || (dnd && !critical) {
        return None;
    }
    if s.filters.iter().any(|f: &Filter| f.matches(app, summary, body) && !f.sound) {
        return None;
    }
    if let Some(f) = hint_file.filter(|f| !f.is_empty()) {
        return Some(f.strip_prefix("file://").unwrap_or(f).to_string());
    }
    let hay = format!("{} {}", app.to_lowercase(), desktop_entry.to_lowercase());
    if let Some((_, file)) = s.app_sounds.iter().find(|(k, _)| hay.contains(&k.to_lowercase())) {
        return Some(file.clone());
    }
    s.notification_sound.clone()
}

/// Plays a file without waiting for it, with the first player found.
pub fn play(path: &str) {
    if !std::path::Path::new(path).is_file() {
        log::debug!("sound file not found: {path}");
        return;
    }
    for (prog, args) in [("pw-play", vec![]), ("paplay", vec![]), ("ffplay", vec!["-nodisp", "-autoexit", "-loglevel", "quiet"])] {
        let spawned = Command::new(prog).args(&args).arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
        if let Ok(mut child) = spawned {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            return;
        }
    }
    log::warn!("no sound player (pw-play, paplay, ffplay) found");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        Settings {
            notification_sound: Some("/s/default.ogg".into()),
            app_sounds: vec![("telegram".into(), "/s/tg.ogg".into())],
            filters: vec![Filter { app: "zen".into(), content: "unseen".into(), show: true, sound: false }],
            ..Settings::default()
        }
    }

    fn pick(app: &str, summary: &str, hint: Option<&str>, suppress: bool, dnd: bool, critical: bool) -> Option<String> {
        choose(&settings(), app, "", summary, "", hint, suppress, dnd, critical)
    }

    #[test]
    fn precedence_hint_then_app_then_default() {
        assert_eq!(pick("Mail", "x", None, false, false, false).as_deref(), Some("/s/default.ogg"));
        assert_eq!(pick("Telegram Desktop", "x", None, false, false, false).as_deref(), Some("/s/tg.ogg"));
        assert_eq!(pick("Telegram", "x", Some("file:///s/own.ogg"), false, false, false).as_deref(), Some("/s/own.ogg"));
    }

    #[test]
    fn silence_wins() {
        assert_eq!(pick("Mail", "x", None, true, false, false), None, "suppress-sound");
        assert_eq!(pick("Mail", "x", None, false, true, false), None, "do not disturb");
        assert!(pick("Mail", "x", None, false, true, true).is_some(), "but not for a critical one");
        assert_eq!(pick("Zen Browser", "You have unseen notifications.", None, false, false, false), None, "a filter");
        assert!(pick("Zen Browser", "A message", None, false, false, false).is_some());
    }

    #[test]
    fn a_missing_file_is_not_an_error() {
        play("/nonexistent/sound.ogg");
    }
}
