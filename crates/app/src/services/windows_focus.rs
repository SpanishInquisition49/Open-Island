//! Brings the window of the app behind a notification to the front (niri, Hyprland, sway), and starts the app
//! when it has no window. The matching is a plain function so it is tested without a compositor.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::process::{Command, Stdio};

/// A window as the compositors report it: an id for the focus call, the application id and the title.
#[derive(Clone, Debug, PartialEq)]
pub struct Win {
    pub id: String,
    pub app_id: String,
    pub title: String,
}

fn norm(s: &str) -> String {
    let s = s.trim().to_lowercase();
    s.strip_suffix(".desktop").unwrap_or(&s).to_string()
}

/// How well a window's app id matches a name: 3 equal, 2 equal after the reverse-DNS prefix, 1 one contains the
/// other (at least four letters), 0 no.
fn score(app_id: &str, name: &str) -> u8 {
    let (a, n) = (norm(app_id), norm(name));
    if a.is_empty() || n.is_empty() {
        return 0;
    }
    if a == n {
        return 3;
    }
    let tail = |s: &str| s.rsplit('.').next().unwrap_or(s).to_string();
    if a.contains('.') != n.contains('.') && (tail(&a) == n || a == tail(&n)) || (a.contains('.') && n.contains('.') && tail(&a) == tail(&n) && tail(&a).len() > 3) {
        return 2;
    }
    let word_prefix = |long: &str, short: &str| short.len() >= 3 && long.strip_prefix(short).is_some_and(|r| r.starts_with(['-', '_', '.', ' ']));
    if word_prefix(&a, &n) || word_prefix(&n, &a) {
        return 2;
    }
    let squash = |s: &str| s.replace(['.', '-', '_', ' '], "");
    let (a, n) = (squash(&a), squash(&n));
    if a.len() >= 4 && n.len() >= 4 && (a.contains(&n) || n.contains(&a)) {
        1
    } else {
        0
    }
}

/// The window that belongs to a notification: the best match for its desktop entry, else for its app name.
pub fn pick(windows: &[Win], desktop_entry: &str, app: &str) -> Option<Win> {
    let best = |name: &str| windows.iter().map(|w| (score(&w.app_id, name), w)).filter(|(s, _)| *s > 0).max_by_key(|(s, _)| *s).map(|(_, w)| w.clone());
    if !desktop_entry.is_empty() {
        if let Some(w) = best(desktop_entry) {
            return Some(w);
        }
    }
    best(app)
}

fn out(prog: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(prog).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

pub fn parse_niri(json: &str) -> Vec<Win> {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    v.as_array().into_iter().flatten().filter_map(|w| Some(Win { id: w["id"].as_u64()?.to_string(), app_id: w["app_id"].as_str().unwrap_or("").into(), title: w["title"].as_str().unwrap_or("").into() })).collect()
}

pub fn parse_hypr(json: &str) -> Vec<Win> {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    v.as_array().into_iter().flatten().filter_map(|w| Some(Win { id: w["address"].as_str()?.into(), app_id: w["class"].as_str().unwrap_or("").into(), title: w["title"].as_str().unwrap_or("").into() })).collect()
}

/// Focuses the app's window; starts the app (by its desktop entry) when none is open. Blocking: call it off the
/// render thread.
pub fn focus_app(desktop_entry: &str, app: &str) {
    let found = if std::env::var_os("NIRI_SOCKET").is_some() {
        out("niri", &["msg", "--json", "windows"]).and_then(|j| pick(&parse_niri(&j), desktop_entry, app)).map(|w| out("niri", &["msg", "action", "focus-window", "--id", &w.id]).is_some())
    } else if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        out("hyprctl", &["clients", "-j"]).and_then(|j| pick(&parse_hypr(&j), desktop_entry, app)).map(|w| out("hyprctl", &["dispatch", "focuswindow", &format!("address:{}", w.id)]).is_some())
    } else if std::env::var_os("SWAYSOCK").is_some() {
        let id = if desktop_entry.is_empty() { app } else { desktop_entry };
        Some(out("swaymsg", &[&format!("[app_id=\"(?i){}\"] focus", id.trim_end_matches(".desktop"))]).is_some())
    } else {
        None
    };
    if found != Some(true) && !desktop_entry.is_empty() {
        // not open: start it (gtk-launch finds the .desktop file)
        let _ = Command::new("gtk-launch").arg(desktop_entry.trim_end_matches(".desktop")).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(id: &str, app_id: &str) -> Win {
        Win { id: id.into(), app_id: app_id.into(), title: String::new() }
    }

    #[test]
    fn finds_the_window_of_the_app_that_sent_the_notification() {
        let all = [w("1", "kitty"), w("2", "org.telegram.desktop"), w("3", "zen"), w("4", "org.gnome.Nautilus")];
        assert_eq!(pick(&all, "org.telegram.desktop", "Telegram Desktop").unwrap().id, "2", "the exact desktop entry");
        assert_eq!(pick(&all, "org.telegram.desktop.desktop", "").unwrap().id, "2", "a .desktop suffix");
        assert_eq!(pick(&all, "zen.desktop", "Zen").unwrap().id, "3");
        assert_eq!(pick(&all, "", "Zen").unwrap().id, "3", "the app name alone");
        assert_eq!(pick(&all, "", "kitty").unwrap().id, "1");
        assert_eq!(pick(&all, "nautilus", "Files").unwrap().id, "4", "the last part of a reverse-DNS id");
    }

    #[test]
    fn a_browser_with_a_longer_id_still_matches() {
        let all = [w("1", "zen-browser"), w("2", "kitty")];
        assert_eq!(pick(&all, "zen", "").unwrap().id, "1");
        assert_eq!(pick(&all, "", "Zen Browser").unwrap().id, "1");
    }

    #[test]
    fn no_window_and_no_false_match() {
        let all = [w("1", "kitty"), w("2", "foot")];
        assert!(pick(&all, "org.telegram.desktop", "Telegram").is_none());
        assert!(pick(&all, "", "").is_none());
        assert!(pick(&[w("1", "ab")], "", "abcdef").is_none(), "a short id is not a substring match");
    }

    #[test]
    fn reads_the_window_lists_of_niri_and_hyprland() {
        let n = parse_niri(r#"[{"id":12,"title":"x","app_id":"zen"},{"id":13,"title":null,"app_id":null}]"#);
        assert_eq!(n[0], Win { id: "12".into(), app_id: "zen".into(), title: "x".into() });
        assert_eq!(n[1].app_id, "");
        let h = parse_hypr(r#"[{"address":"0xabc","class":"kitty","title":"t"}]"#);
        assert_eq!(h, vec![Win { id: "0xabc".into(), app_id: "kitty".into(), title: "t".into() }]);
        assert!(parse_niri("garbage").is_empty());
    }
}
