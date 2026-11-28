//! The screens of the machine and the three ways of using a second one. Whatever is asked, a screen is switched off only
//! after another is on, and a machine whose screens are all off gets them back (see `rescue`).

use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screen {
    /// The connector (`eDP-1`, `HDMI-A-1`).
    pub name: String,
    /// What it says it is (maker and model); empty when it does not.
    pub label: String,
    /// Part of the machine: a laptop's own panel.
    pub internal: bool,
    pub on: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Extend,
    ExternalOnly,
    InternalOnly,
}

pub fn is_internal(name: &str) -> bool {
    ["eDP", "LVDS", "DSI"].iter().any(|p| name.starts_with(p))
}

/// A connector name as it may be handed to a program: letters, digits and dashes, not starting with a dash.
fn plain_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 32 && !name.starts_with('-') && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The answer of `niri msg -j outputs`: the screens by name, the laptop's own first.
pub fn parse_niri(v: &Value) -> Vec<Screen> {
    let mut out: Vec<Screen> = v
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(name, _)| plain_name(name))
        .map(|(name, o)| {
            let said = |k: &str| o.get(k).and_then(Value::as_str).filter(|s| *s != "Unknown").unwrap_or("");
            let label = [said("make"), said("model")].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" ");
            Screen { name: name.clone(), label, internal: is_internal(name), on: o.get("logical").is_some_and(|l| !l.is_null()) }
        })
        .collect();
    out.sort_by(|a, b| b.internal.cmp(&a.internal).then(a.name.cmp(&b.name)));
    out
}

/// The switches that give `mode`, in order: (screen, on). Those to switch on come first, so there is never a moment with
/// none. None when the mode has no screen to leave on (a desktop asked for its laptop panel).
pub fn plan(mode: Mode, screens: &[Screen]) -> Option<Vec<(String, bool)>> {
    let wanted = |s: &Screen| match mode {
        Mode::Extend => true,
        Mode::ExternalOnly => !s.internal,
        Mode::InternalOnly => s.internal,
    };
    if !screens.iter().any(wanted) {
        return None;
    }
    let on = screens.iter().filter(|s| wanted(s) && !s.on).map(|s| (s.name.clone(), true));
    let off = screens.iter().filter(|s| !wanted(s) && s.on).map(|s| (s.name.clone(), false));
    Some(on.chain(off).collect())
}

/// When screens are connected and every one is off (the external one was pulled while the laptop's was off), the ones to
/// switch on: the laptop's own, else all.
pub fn rescue(screens: &[Screen]) -> Vec<String> {
    if screens.iter().any(|s| s.on) {
        return vec![];
    }
    let own: Vec<String> = screens.iter().filter(|s| s.internal).map(|s| s.name.clone()).collect();
    if own.is_empty() {
        screens.iter().map(|s| s.name.clone()).collect()
    } else {
        own
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn s(name: &str, on: bool) -> Screen {
        Screen { name: name.into(), label: String::new(), internal: is_internal(name), on }
    }

    #[test]
    fn niri_outputs_are_read() {
        let v = json!({"HDMI-A-1": {"make": "Epson", "model": "EB-X", "logical": {"x": 0}}, "eDP-1": {"make": "BOE", "model": "Unknown", "logical": null}, "-evil": {}, "a b;c": {}});
        assert_eq!(parse_niri(&v), vec![Screen { name: "eDP-1".into(), label: "BOE".into(), internal: true, on: false }, Screen { name: "HDMI-A-1".into(), label: "Epson EB-X".into(), internal: false, on: true }]);
        assert!(parse_niri(&json!([1])).is_empty());
    }

    #[test]
    fn a_screen_is_switched_off_only_after_another_is_on() {
        let both = [s("eDP-1", true), s("HDMI-A-1", false)];
        assert_eq!(plan(Mode::ExternalOnly, &both), Some(vec![("HDMI-A-1".into(), true), ("eDP-1".into(), false)]));
        assert_eq!(plan(Mode::Extend, &both), Some(vec![("HDMI-A-1".into(), true)]));
        assert_eq!(plan(Mode::InternalOnly, &both), Some(vec![]), "already so");
        assert_eq!(plan(Mode::ExternalOnly, &[s("eDP-1", true)]), None, "there is no other screen: nothing is switched off");
        assert_eq!(plan(Mode::InternalOnly, &[s("HDMI-A-1", true), s("DP-1", true)]), None);
    }

    #[test]
    fn a_machine_left_with_no_screen_gets_one_back() {
        assert_eq!(rescue(&[s("eDP-1", false)]), ["eDP-1"]);
        assert_eq!(rescue(&[s("eDP-1", false), s("HDMI-A-1", false)]), ["eDP-1"]);
        assert_eq!(rescue(&[s("HDMI-A-1", false), s("DP-1", false)]), ["HDMI-A-1", "DP-1"]);
        assert!(rescue(&[s("eDP-1", false), s("HDMI-A-1", true)]).is_empty());
        assert!(rescue(&[]).is_empty());
    }
}
