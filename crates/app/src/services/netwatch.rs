//! Is the network working (Linux): the link, the router and the internet are checked every few seconds, and a change
//! that lasts is reported with where it breaks. Also: is a VPN up.

use super::Reporter;
use island_core::netstate::{self as ns, Health};
use std::process::{Command, Stdio};
use std::time::Duration;

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(cmd).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn link_up(iface: &str) -> bool {
    std::fs::read_to_string(format!("/sys/class/net/{iface}/operstate")).is_ok_and(|s| matches!(s.trim(), "up" | "unknown"))
}

fn vpn_up() -> bool {
    std::fs::read_dir("/sys/class/net").is_ok_and(|rd| rd.flatten().any(|e| ns::is_vpn_iface(&e.file_name().to_string_lossy()) && link_up(&e.file_name().to_string_lossy())))
}

/// One reading: the verdict, and whether a VPN is up.
fn measure(check_internet: bool) -> (Health, bool) {
    let route = std::fs::read_to_string("/proc/net/route").unwrap_or_default();
    let gateway = ns::default_gateway(&route).filter(|(i, _)| link_up(i));
    let Some((_, gw)) = gateway else { return (ns::diagnose(false, None, ""), vpn_up()) };
    let ip = format!("{}.{}.{}.{}", gw[0], gw[1], gw[2], gw[3]);
    let router = run("ping", &["-c", "1", "-W", "1", "-n", &ip]).is_some();
    // NetworkManager's own probe: asked only now and then, its last answer is enough in between
    let verdict = if check_internet { run("timeout", &["8", "nmcli", "networking", "connectivity", "check"]) } else { run("nmcli", &["-t", "-f", "CONNECTIVITY", "general"]) }.unwrap_or_default();
    (ns::diagnose(true, Some(router), &verdict), vpn_up())
}

/// Switches the connection off and on again: the Wi-Fi radio, or the device of the default route.
pub fn reconnect() {
    let route = std::fs::read_to_string("/proc/net/route").unwrap_or_default();
    let iface = ns::default_gateway(&route).map(|(i, _)| i);
    let wireless = iface.as_ref().is_some_and(|i| std::path::Path::new(&format!("/sys/class/net/{i}/wireless")).exists());
    if wireless || iface.is_none() {
        run("nmcli", &["radio", "wifi", "off"]);
        std::thread::sleep(Duration::from_secs(1));
        run("nmcli", &["radio", "wifi", "on"]);
    } else if let Some(i) = iface {
        run("nmcli", &["device", "disconnect", &i]);
        std::thread::sleep(Duration::from_secs(1));
        run("nmcli", &["device", "connect", &i]);
    }
}

pub fn start(rep: Reporter) {
    if which("nmcli").is_none() {
        return;
    }
    std::thread::spawn(move || {
        let (mut shown, mut pending, mut vpn_shown, mut tick) = (Health::Online, Health::Online, false, 0u32);
        loop {
            // healthy: look every 10 s and ask NetworkManager for a fresh verdict every minute; trouble: every 5 s, fresh
            let healthy = shown.is_ok();
            let (h, vpn) = measure(!healthy || tick % 6 == 0);
            log::debug!("network: {h:?}, vpn {vpn}");
            tick = tick.wrapping_add(1);
            // a change has to hold for two readings: one lost ping is not an outage
            if h == pending && h != shown {
                shown = h;
                rep.patch(move |d| d.net = h);
            }
            pending = h;
            if vpn != vpn_shown {
                vpn_shown = vpn;
                rep.patch(move |d| d.vpn = vpn);
            }
            std::thread::sleep(Duration::from_secs(if shown.is_ok() { 10 } else { 5 }));
        }
    });
}

fn which(cmd: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(cmd)).find(|f| f.is_file()))
}
