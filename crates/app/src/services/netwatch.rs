//! Is the network working (Linux): the link, the router and the internet are checked every few seconds, and a change
//! that lasts is reported with where it breaks. Also: is a VPN up.

use super::Reporter;
use island_core::netstate::{self as ns, Health};
use std::time::Duration;

#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};

#[cfg(target_os = "linux")]
fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(cmd).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

#[cfg(target_os = "linux")]
fn link_up(iface: &str) -> bool {
    std::fs::read_to_string(format!("/sys/class/net/{iface}/operstate")).is_ok_and(|s| matches!(s.trim(), "up" | "unknown"))
}

#[cfg(target_os = "linux")]
fn vpn_up() -> bool {
    std::fs::read_dir("/sys/class/net").is_ok_and(|rd| rd.flatten().any(|e| ns::is_vpn_iface(&e.file_name().to_string_lossy()) && link_up(&e.file_name().to_string_lossy())))
}

/// One reading: the verdict, and whether a VPN is up.
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
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
    #[cfg(target_os = "linux")]
    if which("nmcli").is_none() {
        return;
    }
    std::thread::spawn(move || {
        let (mut shown, mut pending, mut vpn_shown, mut tick) = (Health::Online, Health::Online, false, 0u32);
        loop {
            // healthy: look every 10 s and ask NetworkManager for a fresh verdict every minute; trouble: every 5 s, fresh
            let healthy = shown.is_ok();
            #[cfg(target_os = "linux")]
            let (h, vpn) = measure(!healthy || tick % 6 == 0);
            #[cfg(windows)]
            let (h, vpn) = measure(!healthy || tick % 6 == 0, tick % 3 == 0, vpn_shown);
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

#[cfg(target_os = "linux")]
fn which(cmd: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(cmd)).find(|f| f.is_file()))
}

// ---- Windows --------------------------------------------------------------------------------------------

#[cfg(windows)]
fn command_output(cmd: &str, args: &[&str]) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let o = std::process::Command::new(cmd).args(args).creation_flags(0x0800_0000).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// The router answers a ping (a "Destination host unreachable" reply still exits 0 on Windows, so the TTL is looked for).
#[cfg(windows)]
fn router_answers(ip: &str) -> bool {
    command_output("ping", &["-n", "1", "-w", "1000", ip]).is_some_and(|t| t.contains("TTL=") || t.contains("ttl="))
}

/// Windows' own internet probe, the one its network icon uses: a fixed page with a fixed answer. Another answer
/// means a sign-in page sits in between; none at all means no internet.
#[cfg(windows)]
fn connectivity() -> &'static str {
    let agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(6))).http_status_as_error(false).build().new_agent();
    match agent.get("http://www.msftconnecttest.com/connecttest.txt").call() {
        Ok(mut r) => {
            let body = r.body_mut().read_to_string().unwrap_or_default();
            if body.trim() == "Microsoft Connect Test" {
                "full"
            } else {
                "portal"
            }
        }
        Err(_) => "limited",
    }
}

#[cfg(windows)]
fn vpn_up() -> bool {
    adapters_up().is_some_and(|t| ns::any_vpn_adapter(&t))
}

/// The descriptions of the network adapters that are up, one to a line, as the system lists them (asking a PowerShell
/// for them costs seconds of processor time).
#[cfg(windows)]
fn adapters_up() -> Option<String> {
    use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
    use windows::Win32::NetworkManagement::IpHelper::{GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, GAA_FLAG_SKIP_UNICAST, IP_ADAPTER_ADDRESSES_LH};
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows::Win32::Networking::WinSock::AF_UNSPEC;
    let flags = GAA_FLAG_SKIP_UNICAST | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut bytes = 16 * 1024u32;
    // the list can grow between two calls: a few tries with the size it asks for
    for _ in 0..3 {
        // u64s, so that the records are aligned as they must be
        let mut buffer = vec![0u64; (bytes as usize).div_ceil(8)];
        let first = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        // SAFETY: `first` points to at least `bytes` bytes, which is what the call is told
        let result = unsafe { GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(first), &mut bytes) };
        if result == ERROR_BUFFER_OVERFLOW.0 {
            continue;
        }
        if result != NO_ERROR.0 {
            return None;
        }
        let (mut at, mut up) = (first, String::new());
        while !at.is_null() {
            // SAFETY: the records and their strings live in `buffer`, linked and terminated by the call above
            let adapter = unsafe { &*at };
            if adapter.OperStatus == IfOperStatusUp {
                if let Ok(description) = unsafe { adapter.Description.to_string() } {
                    up.push_str(&description);
                    up.push('\n');
                }
            }
            at = adapter.Next;
        }
        return Some(up);
    }
    None
}

#[cfg(windows)]
fn measure(check_internet: bool, check_vpn: bool, last_vpn: bool) -> (Health, bool) {
    let vpn = if check_vpn { vpn_up() } else { last_vpn };
    let Some(gw) = command_output("route", &["print", "-4", "0.0.0.0"]).and_then(|t| ns::windows_gateway(&t)) else { return (ns::diagnose(false, None, ""), vpn) };
    let ip = format!("{}.{}.{}.{}", gw[0], gw[1], gw[2], gw[3]);
    let router = router_answers(&ip);
    // the HTTP probe costs a request: only now and then while all is well, as the Linux side asks NetworkManager
    let verdict = if check_internet || !router { connectivity() } else { "full" };
    (ns::diagnose(true, Some(router), verdict), vpn)
}

/// Windows: the Wi-Fi radio is switched off and on again through the radio switch the quick settings use.
#[cfg(windows)]
pub fn reconnect() {
    super::win_more::command(island_ui::model::Command::ToggleWifi);
    std::thread::sleep(Duration::from_secs(2));
    super::win_more::command(island_ui::model::Command::ToggleWifi);
}
