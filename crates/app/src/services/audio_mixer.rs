//! The audio page's data (Linux, pactl): outputs, inputs, and the volume of each app that is playing. Read when
//! PipeWire/PulseAudio report a change, written by the commands the page sends.

use super::Reporter;
use island_ui::model::{AppStream, Command, Sink};
use serde_json::Value;
use std::process::{Command as Proc, Stdio};
use std::time::Duration;

/// Outputs or inputs from `pactl -f json list sinks|sources`; the monitor of an output is not an input.
pub fn parse_devices(json: &str) -> Vec<Sink> {
    let Ok(Value::Array(a)) = serde_json::from_str::<Value>(json) else { return vec![] };
    a.iter()
        .filter_map(|s| {
            let name = s.get("name")?.as_str()?.to_string();
            if name.ends_with(".monitor") || s.get("monitor_of_sink").is_some_and(|m| !m.is_null()) {
                return None;
            }
            let label = s.get("description").and_then(Value::as_str).unwrap_or(&name).to_string();
            Some(Sink { name, label })
        })
        .collect()
}

/// Apps that are playing, from `pactl -f json list sink-inputs`: the loudest channel is the app's volume.
pub fn parse_streams(json: &str) -> Vec<AppStream> {
    let Ok(Value::Array(a)) = serde_json::from_str::<Value>(json) else { return vec![] };
    a.iter()
        .filter_map(|s| {
            let id = s.get("index")?.as_u64()? as u32;
            let props = s.get("properties");
            let prop = |k: &str| props.and_then(|p| p.get(k)).and_then(Value::as_str);
            let name = prop("application.name").or_else(|| prop("media.name")).unwrap_or("Unknown").to_string();
            let volume = s
                .get("volume")?
                .as_object()?
                .values()
                .filter_map(|c| c.get("value_percent")?.as_str()?.trim_end_matches('%').parse::<f32>().ok())
                .fold(0.0f32, f32::max)
                .round() as u8;
            let muted = s.get("mute").and_then(Value::as_bool).unwrap_or(false);
            Some(AppStream { id, name, volume, muted })
        })
        .collect()
}

fn pactl(args: &[&str]) -> Option<String> {
    let o = Proc::new("pactl").args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn read(rep: &Reporter) {
    let sinks = parse_devices(&pactl(&["-f", "json", "list", "sinks"]).unwrap_or_default());
    let sources = parse_devices(&pactl(&["-f", "json", "list", "sources"]).unwrap_or_default());
    let streams = parse_streams(&pactl(&["-f", "json", "list", "sink-inputs"]).unwrap_or_default());
    let sink = pactl(&["get-default-sink"]).map(|s| s.trim().to_string());
    let source = pactl(&["get-default-source"]).map(|s| s.trim().to_string());
    rep.patch(move |d| {
        d.sinks = sinks;
        d.sink = sink;
        d.sources = sources;
        d.source = source;
        d.streams = streams;
    });
}

/// Runs a mixer command; true when it was one (the lists are read again afterwards).
pub fn command(c: &Command) -> bool {
    match c {
        Command::SetSink(n) => drop(pactl(&["set-default-sink", n])),
        Command::SetSource(n) => drop(pactl(&["set-default-source", n])),
        Command::SetStreamVolume { id, percent } => drop(pactl(&["set-sink-input-volume", &id.to_string(), &format!("{}%", (*percent).min(150))])),
        Command::ToggleStreamMute(id) => drop(pactl(&["set-sink-input-mute", &id.to_string(), "toggle"])),
        _ => return false,
    }
    true
}

pub fn start(rep: Reporter) {
    let Some(events) = super::linux::pactl_subscribe() else { return };
    std::thread::spawn(move || {
        read(&rep);
        while events.recv().is_ok() {
            // a burst of events is one change
            while events.recv_timeout(Duration::from_millis(120)).is_ok() {}
            read(&rep);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_leave_out_the_monitor_of_an_output() {
        let j = r#"[{"name":"alsa_input.mic","description":"Mic","monitor_of_sink":null},{"name":"alsa_output.x.monitor","description":"Monitor of x","monitor_of_sink":"alsa_output.x"}]"#;
        assert_eq!(parse_devices(j), vec![Sink { name: "alsa_input.mic".into(), label: "Mic".into() }]);
        assert!(parse_devices("nope").is_empty());
    }

    #[test]
    fn a_stream_has_the_name_of_its_app_and_the_loudest_channel() {
        let j = r#"[{"index":42,"mute":false,"volume":{"front-left":{"value_percent":"60%"},"front-right":{"value_percent":"75%"}},"properties":{"application.name":"Spotify"}},
                    {"index":43,"mute":true,"volume":{"mono":{"value_percent":"100%"}},"properties":{"media.name":"alert"}}]"#;
        assert_eq!(
            parse_streams(j),
            vec![AppStream { id: 42, name: "Spotify".into(), volume: 75, muted: false }, AppStream { id: 43, name: "alert".into(), volume: 100, muted: true }]
        );
    }
}
