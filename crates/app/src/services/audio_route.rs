//! Moves the audio output to headphones when they appear and back when they go.
//!
//! WirePlumber does this itself until something pins a default output (choosing one in a mixer, or the sink
//! button of the quick settings, stores it as `default.configured.audio.sink`): from then on a new Bluetooth or
//! USB headset is not picked up and the pinned output wins whenever it exists. This watches the outputs and sets
//! the default itself, so plugging in or connecting headphones switches to them, and when they go the output
//! falls back to the best one left.

use super::Reporter;
use serde_json::Value;
use std::process::Stdio;
use std::time::Duration;

/// An output as far as the choice goes.
#[derive(Clone, Debug, PartialEq)]
pub struct OutSink {
    pub name: String,
    pub label: String,
    /// A headset or headphones: Bluetooth or USB, or a jack that says so.
    pub headphones: bool,
    /// Its port is not reported as unplugged.
    pub available: bool,
    pub priority: i64,
}

/// `pactl -f json list sinks`, reduced.
pub fn parse_outputs(json: &str) -> Vec<OutSink> {
    let Ok(Value::Array(sinks)) = serde_json::from_str::<Value>(json) else { return vec![] };
    sinks
        .iter()
        .filter_map(|s| {
            let name = s.get("name")?.as_str()?.to_string();
            let props = s.get("properties");
            let prop = |k: &str| props.and_then(|p| p.get(k)).and_then(|v| v.as_str()).unwrap_or("");
            let bus = prop("device.bus");
            let ports = s.get("ports").and_then(|p| p.as_array()).cloned().unwrap_or_default();
            let active = s.get("active_port").and_then(|a| a.as_str()).unwrap_or("");
            let port = ports.iter().find(|p| p.get("name").and_then(|n| n.as_str()) == Some(active)).or(ports.first());
            let port_type = port.and_then(|p| p.get("type")).and_then(|t| t.as_str()).unwrap_or("");
            let available = port.and_then(|p| p.get("availability")).and_then(|a| a.as_str()) != Some("not available");
            let jack = matches!(port_type, "Headphones" | "Headset");
            let headphones = bus == "bluetooth" || bus == "usb" || name.starts_with("bluez_output.") || jack || prop("device.form_factor") == "headphone" || prop("device.form_factor") == "headset";
            Some(OutSink {
                label: s.get("description").and_then(|d| d.as_str()).unwrap_or(&name).to_string(),
                priority: prop("priority.session").parse().unwrap_or(0),
                name,
                headphones,
                available,
            })
        })
        .collect()
}

/// What to make the default, if anything: a headphone output that was not there (or not plugged in) before, else, when
/// the current default has gone, the best output left. Headphones first; among the others the highest priority.
pub fn decide(before: &[OutSink], now: &[OutSink], default: &str) -> Option<String> {
    let was_usable = |name: &str| before.iter().any(|b| b.name == name && b.available);
    if let Some(new) = now.iter().filter(|s| s.headphones && s.available && !was_usable(&s.name)).max_by_key(|s| s.priority) {
        return (new.name != default).then(|| new.name.clone());
    }
    let default_ok = now.iter().any(|s| s.name == default && s.available);
    if default_ok {
        return None;
    }
    // the default is gone (or unplugged): headphones that are still there, else the best of the rest
    now.iter().filter(|s| s.available).max_by_key(|s| (s.headphones, s.priority)).map(|s| s.name.clone())
}

fn pactl(args: &[&str]) -> Option<String> {
    let o = super::tool("pactl").args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn default_sink() -> String {
    pactl(&["get-default-sink"]).map(|s| s.trim().to_string()).unwrap_or_default()
}

/// An output may have come or gone.
fn changes_the_outputs(line: &str) -> bool {
    line.contains(" on sink #") || line.contains(" on card #")
}

pub fn start(rep: Reporter) {
    let Some(events) = super::linux::pactl_events(changes_the_outputs) else { return };
    std::thread::spawn(move || {
        // what is there now is the state to compare with, not a change
        let mut before: Option<Vec<OutSink>> = Some(parse_outputs(&pactl(&["-f", "json", "list", "sinks"]).unwrap_or_default()));
        let mut announce_id = 0u32;
        loop {
            // a connecting headset announces its output a moment before it is usable: wait for the burst to end
            if !super::linux::next_change(&events, Duration::from_millis(500), Duration::from_secs(4)) {
                return;
            }
            let now = parse_outputs(&pactl(&["-f", "json", "list", "sinks"]).unwrap_or_default());
            let Some(prev) = before.replace(now.clone()) else { continue };
            if !crate::config::load().ui.auto_audio_output {
                continue;
            }
            let default = default_sink();
            if let Some(target) = decide(&prev, &now, &default) {
                if pactl(&["set-default-sink", &target]).is_some() {
                    let label = now.iter().find(|s| s.name == target).map_or(target.clone(), |s| s.label.clone());
                    log::info!("audio output: {default} -> {target}");
                    announce_id += 1;
                    rep.input(island_ui::model::Input::Notification(island_ui::model::Incoming { id: 3_500_000 + announce_id, app: "open-island".into(), summary: "Audio output".into(), body: label, expire_ms: Some(2500), ..Default::default() }));
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str, headphones: bool, available: bool, priority: i64) -> OutSink {
        OutSink { name: name.into(), label: name.into(), headphones, available, priority }
    }

    #[test]
    fn a_headset_that_appears_becomes_the_output_even_when_another_is_pinned() {
        let speaker = s("speaker", false, true, 1009);
        let bt = s("bluez_output.x", true, true, 1000);
        assert_eq!(decide(std::slice::from_ref(&speaker), &[speaker.clone(), bt.clone()], "speaker"), Some("bluez_output.x".into()));
        assert_eq!(decide(&[speaker.clone(), bt.clone()], &[speaker.clone(), bt.clone()], "bluez_output.x"), None, "nothing new, and it is already the default");
        assert_eq!(decide(std::slice::from_ref(&speaker), &[speaker.clone(), bt.clone()], "bluez_output.x"), None, "already the output: nothing to do");
        // a plug that was there but not plugged in, now plugged in
        let jack_off = s("jack", true, false, 900);
        let jack_on = s("jack", true, true, 900);
        assert_eq!(decide(&[speaker.clone(), jack_off], &[speaker.clone(), jack_on], "speaker"), Some("jack".into()));
    }

    #[test]
    fn when_the_headset_goes_the_best_output_left_takes_over() {
        let speaker = s("speaker", false, true, 1009);
        let hdmi = s("hdmi", false, true, 600);
        let bt = s("bluez_output.x", true, true, 1000);
        assert_eq!(decide(&[speaker.clone(), hdmi.clone(), bt.clone()], &[speaker.clone(), hdmi.clone()], "bluez_output.x"), Some("speaker".into()));
        // a default that still exists is left alone
        assert_eq!(decide(&[speaker.clone(), bt.clone()], std::slice::from_ref(&speaker), "speaker"), None);
        // nothing usable left: nothing to choose
        assert_eq!(decide(std::slice::from_ref(&bt), &[], "bluez_output.x"), None);
        // an unplugged default counts as gone
        let jack_off = s("jack", true, false, 900);
        assert_eq!(decide(&[speaker.clone(), s("jack", true, true, 900)], &[speaker.clone(), jack_off], "jack"), Some("speaker".into()));
    }

    #[test]
    fn ordinary_outputs_that_come_and_go_do_not_steal_the_output() {
        let speaker = s("speaker", false, true, 1009);
        let hdmi = s("hdmi", false, true, 600);
        assert_eq!(decide(std::slice::from_ref(&speaker), &[speaker.clone(), hdmi], "speaker"), None, "a monitor's speakers do not take over");
    }

    #[test]
    fn reads_pactls_json() {
        let json = r#"[
          {"index":53,"name":"alsa_output.pci.HiFi__Speaker__sink","description":"Family 17h Speaker","state":"SUSPENDED","active_port":"[Out] Speaker","ports":[{"name":"[Out] Speaker","type":"Speaker","availability":"availability unknown"}],"properties":{"device.bus":"pci","priority.session":"1009"}},
          {"index":58,"name":"bluez_output.3C_B0.1","description":"Nothing Ear","state":"RUNNING","active_port":"headset-output","ports":[{"name":"headset-output","type":"Headset","availability":"yes"}],"properties":{"device.bus":"bluetooth","priority.session":"1010"}},
          {"index":60,"name":"alsa_output.usb-X","ports":[{"name":"analog-output","type":"Analog","availability":"not available"}],"properties":{"device.bus":"usb"}}]"#;
        let v = parse_outputs(json);
        assert_eq!(v.len(), 3);
        assert_eq!((v[0].headphones, v[0].available, v[0].priority), (false, true, 1009));
        assert_eq!((v[1].headphones, v[1].label.as_str()), (true, "Nothing Ear"));
        assert!(v[2].headphones && !v[2].available, "a USB device whose port says it is not plugged in");
        assert!(parse_outputs("nope").is_empty() && parse_outputs("[]").is_empty());
    }
}
