//! Bluetooth devices that are connected, with their battery (headphones, mice, keyboards), through `bluetoothctl`.
//! A device that connects gets a card; one whose battery runs low gets a warning.

use super::Reporter;
use island_core::bluetooth::{parse_info, parse_list};
use island_ui::model::{Device, Input};
use std::process::{Command, Stdio};
use std::time::Duration;

fn out(args: &[&str]) -> Option<String> {
    let o = Command::new("bluetoothctl").args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn connected() -> Vec<Device> {
    // BlueZ 5.65+ filters on the command line; older ones list every paired device and `info` says which is connected
    let listed = out(&["devices", "Connected"]).map(|t| parse_list(&t)).filter(|l| !l.is_empty()).or_else(|| out(&["devices"]).map(|t| parse_list(&t)));
    listed
        .unwrap_or_default()
        .into_iter()
        .take(12)
        .filter_map(|(mac, name)| {
            let info = parse_info(&out(&["info", &mac])?)?;
            info.connected.then_some(Device { id: mac, name: if info.name.is_empty() { name } else { info.name }, kind: info.kind, battery: info.battery })
        })
        .collect()
}

/// What changed between two polls: devices that appeared, and devices whose battery fell to the warning level.
pub fn changes(before: &[Device], now: &[Device], warn_at: u8) -> (Vec<Device>, Vec<Device>) {
    let appeared = now.iter().filter(|d| !before.iter().any(|b| b.id == d.id)).cloned().collect();
    let low = now.iter().filter(|d| d.battery.is_some_and(|b| b <= warn_at) && before.iter().find(|b| b.id == d.id).is_none_or(|b| b.battery.is_none_or(|x| x > warn_at))).cloned().collect();
    (appeared, low)
}

pub fn start(rep: Reporter) {
    if !crate::doctor::tool_in_path("bluetoothctl", &std::env::var("PATH").unwrap_or_default()) {
        return;
    }
    std::thread::spawn(move || {
        let mut last: Option<Vec<Device>> = None;
        loop {
            let now = connected();
            if last.as_ref() != Some(&now) {
                if let Some(before) = &last {
                    let (appeared, low) = changes(before, &now, 15);
                    for d in appeared {
                        rep.input(Input::DeviceConnected(d));
                    }
                    for d in low {
                        rep.input(Input::DeviceLow(d));
                    }
                }
                let d2 = now.clone();
                rep.patch(move |d| d.devices = d2);
                last = Some(now);
            }
            std::thread::sleep(Duration::from_secs(8));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use island_core::bluetooth::Kind;

    fn d(id: &str, battery: Option<u8>) -> Device {
        Device { id: id.into(), name: id.into(), kind: Kind::Headphones, battery }
    }

    #[test]
    fn a_new_device_is_announced_and_a_dropping_battery_warns_once() {
        let (a, l) = changes(&[], &[d("x", Some(80))], 15);
        assert_eq!((a.len(), l.len()), (1, 0));
        let (a, l) = changes(&[d("x", Some(80))], &[d("x", Some(80)), d("y", Some(10))], 15);
        assert_eq!((a.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), l.iter().map(|d| d.id.as_str()).collect::<Vec<_>>()), (vec!["y"], vec!["y"]), "connecting already low says both");
        let (a, l) = changes(&[d("x", Some(16))], &[d("x", Some(15))], 15);
        assert_eq!((a.len(), l.len()), (0, 1), "crossing the line");
        let (a, l) = changes(&[d("x", Some(15))], &[d("x", Some(14))], 15);
        assert_eq!((a.len(), l.len()), (0, 0), "already low: no second warning");
        let (_, l) = changes(&[d("x", None)], &[d("x", Some(5))], 15);
        assert_eq!(l.len(), 1, "a battery that appears low");
    }
}
