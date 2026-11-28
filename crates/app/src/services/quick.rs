//! Quick settings (Linux): Wi-Fi, Bluetooth, keep-awake, power profile, audio outputs. Polled every few
//! seconds while the page is open (see `QUICK_WANTED`) and right after a toggle.

use super::awake::Holder;
use super::Reporter;
use island_ui::model::{Command, Sink};
use std::process::{Child, Command as Proc, Stdio};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

const TLP_HELPER: &str = "/usr/local/bin/tlp-profile-set";
const PROFILES: [&str; 3] = ["performance", "balanced", "powersave"];

fn out(prog: &str, args: &[&str]) -> Option<String> {
    let o = super::tool(prog).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn run(prog: &str, args: &[&str]) {
    let _ = Proc::new(prog).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

/// wlsunset arguments: the sun's real times from the weather location when there is one, else 07:00 to 19:00.
pub fn wlsunset_args(lat: Option<f64>, lon: Option<f64>) -> Vec<String> {
    let mut a: Vec<String> = ["-t", "3800", "-T", "6500"].iter().map(|s| s.to_string()).collect();
    match (lat, lon) {
        (Some(la), Some(lo)) => a.extend(["-l".to_string(), format!("{la:.4}"), "-L".to_string(), format!("{lo:.4}")]),
        _ => a.extend(["-s", "07:00", "-S", "19:00"].iter().map(|s| s.to_string())),
    }
    a
}

fn night_light_installed() -> bool {
    Proc::new("wlsunset").arg("-h").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok()
}

fn night_light_running(ours: &mut Option<Child>) -> bool {
    if let Some(c) = ours.as_mut() {
        if c.try_wait().ok().flatten().is_some() {
            *ours = None;
        }
    }
    ours.is_some() || Proc::new("pgrep").args(["-x", "wlsunset"]).stdout(Stdio::null()).status().is_ok_and(|s| s.success())
}

pub fn parse_wifi(nmcli_radio: &str) -> Option<bool> {
    match nmcli_radio.trim() {
        "enabled" => Some(true),
        "disabled" => Some(false),
        _ => None,
    }
}

pub fn parse_bluetooth(show: &str) -> Option<bool> {
    show.lines().find_map(|l| l.trim().strip_prefix("Powered:").map(|v| v.trim() == "yes"))
}

pub fn parse_sinks(json: &str) -> Vec<Sink> {
    let Ok(serde_json::Value::Array(a)) = serde_json::from_str::<serde_json::Value>(json) else { return vec![] };
    a.iter()
        .filter_map(|s| {
            let name = s.get("name")?.as_str()?.to_string();
            let label = s.get("description").and_then(|d| d.as_str()).unwrap_or(&name).to_string();
            Some(Sink { name, label })
        })
        .collect()
}

pub fn profile_name(sysfs: &str) -> String {
    match sysfs.trim() {
        "low-power" => "powersave".into(),
        p => p.to_string(),
    }
}

fn next<T: PartialEq + Clone>(list: &[T], current: Option<&T>) -> Option<T> {
    if list.is_empty() {
        return None;
    }
    let at = current.and_then(|c| list.iter().position(|x| x == c));
    Some(list[at.map_or(0, |i| (i + 1) % list.len())].clone())
}

pub fn start(rep: Reporter, cmd_rx: Receiver<Command>) {
    std::thread::spawn(move || {
        // held again from the start when it was on at the end of the last run; `cat` lasts as long as the island does
        let mut inhibitor = Holder::restore(super::awake::memory(), "systemd-inhibit", &["--what=idle:sleep", "--who=open-island", "--why=Keep awake", "--mode=block", "cat"]);
        let mut night: Option<Child> = None;
        let has_night_light = night_light_installed();
        let mut sinks: Vec<Sink> = vec![];
        let mut sink: Option<String> = None;
        let mut profile: Option<String> = None;
        let mut wifi: Option<bool> = None;
        let mut sample_now;
        let mut first = true;
        loop {
            let got = if first {
                first = false;
                Ok(Command::RefreshQuick)
            } else {
                cmd_rx.recv_timeout(Duration::from_secs(if super::QUICK_WANTED.load(Ordering::Relaxed) { 3 } else { 30 }))
            };
            match got {
                Ok(Command::ToggleWifi) => {
                    run("nmcli", &["radio", "wifi", if wifi == Some(true) { "off" } else { "on" }]);
                    sample_now = true;
                }
                Ok(Command::ToggleBluetooth) => {
                    let on = parse_bluetooth(&out("bluetoothctl", &["show"]).unwrap_or_default()) == Some(true);
                    run("bluetoothctl", &["power", if on { "off" } else { "on" }]);
                    sample_now = true;
                }
                Ok(Command::ToggleKeepAwake) => {
                    inhibitor.toggle();
                    sample_now = true;
                }
                Ok(Command::ToggleNightLight) => {
                    if night_light_running(&mut night) {
                        match night.take() {
                            Some(mut c) => {
                                let _ = c.kill();
                                let _ = c.wait();
                            }
                            None => run("pkill", &["-x", "wlsunset"]),
                        }
                    } else {
                        let cfg = crate::config::load();
                        night = Proc::new("wlsunset").args(wlsunset_args(cfg.latitude, cfg.longitude)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok();
                    }
                    sample_now = true;
                }
                Ok(Command::NextProfile) => {
                    if let Some(n) = next(&PROFILES.map(String::from), profile.as_ref()) {
                        if std::path::Path::new(TLP_HELPER).exists() {
                            run("sudo", &["-n", TLP_HELPER, &n]);
                        } else {
                            run("powerprofilesctl", &["set", if n == "powersave" { "power-saver" } else { &n }]);
                        }
                    }
                    sample_now = true;
                }
                Ok(Command::NextSink) => {
                    let names: Vec<String> = sinks.iter().map(|s| s.name.clone()).collect();
                    if let Some(n) = next(&names, sink.as_ref()) {
                        run("pactl", &["set-default-sink", &n]);
                    }
                    sample_now = true;
                }
                Ok(Command::RefreshQuick) => sample_now = true,
                Ok(_) => continue,
                Err(RecvTimeoutError::Timeout) => sample_now = super::QUICK_WANTED.load(Ordering::Relaxed),
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if !sample_now {
                continue;
            }
            wifi = parse_wifi(&out("nmcli", &["radio", "wifi"]).unwrap_or_default());
            let bt = parse_bluetooth(&out("bluetoothctl", &["show"]).unwrap_or_default());
            sinks = parse_sinks(&out("pactl", &["-f", "json", "list", "sinks"]).unwrap_or_default());
            sink = out("pactl", &["get-default-sink"]).map(|s| s.trim().to_string());
            profile = std::fs::read_to_string("/sys/firmware/acpi/platform_profile").ok().map(|s| profile_name(&s));
            let night_now = has_night_light.then(|| night_light_running(&mut night));
            let (keep, p, s, k) = (inhibitor.on(), profile.clone(), sinks.clone(), sink.clone());
            rep.patch(move |d| {
                d.wifi = wifi;
                d.bluetooth = bt;
                d.keep_awake = keep;
                d.night_light = night_now;
                d.profile = p;
                d.sinks = s;
                d.sink = k;
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_tools_output() {
        assert_eq!(parse_wifi("enabled\n"), Some(true));
        assert_eq!(parse_wifi("disabled"), Some(false));
        assert_eq!(parse_wifi("garbage"), None);
        assert_eq!(parse_bluetooth("Controller AA\n\tPowered: yes\n\tPairable: yes\n"), Some(true));
        assert_eq!(parse_bluetooth("Controller AA\n\tPowered: no\n"), Some(false));
        assert_eq!(parse_bluetooth("No default controller available"), None);
        let sinks = parse_sinks(r#"[{"name":"a","description":"Speakers"},{"name":"bluez_output.x"}]"#);
        assert_eq!(sinks, vec![Sink { name: "a".into(), label: "Speakers".into() }, Sink { name: "bluez_output.x".into(), label: "bluez_output.x".into() }]);
        assert!(parse_sinks("nope").is_empty());
        assert_eq!(profile_name("low-power\n"), "powersave");
        assert_eq!(profile_name("balanced"), "balanced");
    }

    #[test]
    fn night_light_uses_the_sun_when_it_knows_where_you_are() {
        let with = wlsunset_args(Some(43.7228), Some(10.4017));
        assert_eq!(with, ["-t", "3800", "-T", "6500", "-l", "43.7228", "-L", "10.4017"]);
        let without = wlsunset_args(None, Some(10.0));
        assert_eq!(without, ["-t", "3800", "-T", "6500", "-s", "07:00", "-S", "19:00"], "a half-known location is no location");
    }

    #[test]
    fn cycles_through_a_list() {
        let l = ["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(next(&l, Some(&"a".to_string())).as_deref(), Some("b"));
        assert_eq!(next(&l, Some(&"c".to_string())).as_deref(), Some("a"));
        assert_eq!(next(&l, None).as_deref(), Some("a"), "unknown current starts at the first");
        assert_eq!(next::<String>(&[], None), None);
    }
}
