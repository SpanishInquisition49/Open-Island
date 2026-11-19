//! Is the network working, and if not, where does it break: the link, the router, or beyond it.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Health {
    #[default]
    Online,
    /// No connection at all.
    NoLink,
    /// Connected, but the router does not answer.
    RouterDown,
    /// The router answers, the internet does not.
    NoInternet,
    /// Reachable only through a sign-in page.
    Portal,
}

impl Health {
    pub fn is_ok(self) -> bool {
        self == Health::Online
    }

    /// A title and a line of detail for the card that tells about it.
    pub fn message(self) -> (&'static str, &'static str) {
        match self {
            Health::Online => ("Back online", ""),
            Health::NoLink => ("No network", "Not connected to Wi-Fi or a cable"),
            Health::RouterDown => ("Router not answering", "Connected, but the router is silent"),
            Health::NoInternet => ("No internet", "The router answers: the problem is beyond it"),
            Health::Portal => ("Sign-in needed", "The network wants a sign-in page"),
        }
    }
}

/// Where it breaks, from what was measured: the link, whether the router answered (None: unknown), and NetworkManager's
/// verdict on the internet (`full`, `limited`, `portal`, `none`, `unknown`).
pub fn diagnose(link_up: bool, router_answers: Option<bool>, connectivity: &str) -> Health {
    if !link_up {
        return Health::NoLink;
    }
    if router_answers == Some(false) {
        return Health::RouterDown;
    }
    match connectivity.trim() {
        "portal" => Health::Portal,
        "limited" | "none" => Health::NoInternet,
        _ => Health::Online,
    }
}

/// The interface and the address of the default route, from /proc/net/route (the lowest metric wins).
pub fn default_gateway(route: &str) -> Option<(String, [u8; 4])> {
    route
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let (iface, dest, gw, flags, metric) = (f.first()?, f.get(1)?, f.get(2)?, u32::from_str_radix(f.get(3)?, 16).ok()?, f.get(6)?.parse::<u32>().ok()?);
            (*dest == "00000000" && flags & 2 != 0).then(|| (metric, iface.to_string(), u32::from_str_radix(gw, 16).ok().map(u32::to_le_bytes)))
        })
        .filter_map(|(m, i, gw)| Some((m, i, gw?)))
        .min_by_key(|(m, _, _)| *m)
        .map(|(_, i, gw)| (i, gw))
}

/// A tunnel interface: a VPN is up.
pub fn is_vpn_iface(name: &str) -> bool {
    ["tun", "tap", "wg", "ppp", "tailscale", "proton", "mullvad", "nordlynx", "zt"].iter().any(|p| name.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_break_is_found_from_the_link_outwards() {
        assert_eq!(diagnose(false, None, "none"), Health::NoLink);
        assert_eq!(diagnose(true, Some(false), "none"), Health::RouterDown);
        assert_eq!(diagnose(true, Some(true), "none"), Health::NoInternet);
        assert_eq!(diagnose(true, Some(true), "limited"), Health::NoInternet);
        assert_eq!(diagnose(true, Some(true), "portal"), Health::Portal);
        assert_eq!(diagnose(true, Some(true), "full\n"), Health::Online);
        assert_eq!(diagnose(true, None, "unknown"), Health::Online, "no verdict, no complaint");
        assert!(Health::Online.is_ok() && !Health::NoInternet.is_ok());
        assert_eq!(Health::NoInternet.message().0, "No internet");
    }

    #[test]
    fn the_default_route_is_read() {
        let r = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\n\
                 wlan0\t00000000\t0140A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\n\
                 eth0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\n\
                 docker0\t000011AC\t00000000\t0001\t0\t0\t0\t0000FFFF\t0\t0\t0\n";
        assert_eq!(default_gateway(r), Some(("eth0".into(), [192, 168, 1, 1])), "the lowest metric");
        assert_eq!(default_gateway("Iface\tDestination\n"), None);
        assert_eq!(default_gateway("Iface\n docker0\t000011AC\t00000000\t0001\t0\t0\t0\t0000FFFF\t0\t0\t0\n"), None, "no default route");
    }

    #[test]
    fn tunnels_are_recognised() {
        assert!(is_vpn_iface("wg0") && is_vpn_iface("tun0") && is_vpn_iface("tailscale0"));
        assert!(!is_vpn_iface("wlan0") && !is_vpn_iface("eth0") && !is_vpn_iface("docker0"));
    }
}
