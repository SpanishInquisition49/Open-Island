//! Taildrop: sending files to your own devices over Tailscale (`tailscale file cp <files> <device>:`).
//! Parsers for what the `tailscale` command prints. Receiving is not done here: whatever already empties the Taildrop
//! inbox (`tailscale file get`) keeps doing so, and two readers would take files from each other.

use crate::localsend::Peer;
use std::collections::HashMap;

/// `tailscale file cp --targets`: `100.80.45.77<TAB>homelab-0` for a device that can be sent to now, a third column
/// ("offline; last seen ...") for one that cannot. Returns (address, name) of the first kind.
pub fn parse_targets(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let (ip, name) = (f.next()?.trim(), f.next()?.trim());
            (f.next().is_none() && !ip.is_empty() && !name.is_empty() && ip.chars().all(|c| c.is_ascii_hexdigit() || c == '.' || c == ':')).then(|| (ip.to_string(), name.to_string()))
        })
        .collect()
}

/// `tailscale status --json`: the operating system of each device by its short name ("iphone-12" -> "iOS"). The
/// `HostName` of a phone is just "localhost", so the first label of the DNS name is used.
pub fn parse_os(json: &str) -> HashMap<String, String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return HashMap::new() };
    let mut out = HashMap::new();
    let mut add = |n: &serde_json::Value| {
        let (dns, os) = (n.get("DNSName").and_then(|x| x.as_str()), n.get("OS").and_then(|x| x.as_str()));
        if let (Some(dns), Some(os)) = (dns, os) {
            out.insert(dns.split('.').next().unwrap_or(dns).to_lowercase(), os.to_string());
        }
    };
    if let Some(s) = v.get("Self") {
        add(s);
    }
    if let Some(p) = v.get("Peer").and_then(|p| p.as_object()) {
        p.values().for_each(&mut add);
    }
    out
}

/// The devices as entries of the Send page's list.
pub fn peers(targets: &[(String, String)], os: &HashMap<String, String>) -> Vec<Peer> {
    targets
        .iter()
        .map(|(ip, name)| {
            let kind = match os.get(&name.to_lowercase()).map(|s| s.to_lowercase()).as_deref() {
                Some("ios" | "android") => "mobile",
                Some(_) => "desktop",
                None => "",
            };
            Peer { alias: name.chars().take(40).collect(), ip: ip.clone(), port: 0, model: "Tailscale".into(), https: false, kind: kind.into(), taildrop: true }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_devices_that_can_be_sent_to_now_are_listed() {
        let t = "100.80.45.77\thomelab-0\n100.75.125.95\thomelab-1\toffline; last seen 2429h27m0s ago\n100.116.249.18\tipad-pro-11-gen-2\nnoise\n";
        assert_eq!(parse_targets(t), vec![("100.80.45.77".to_string(), "homelab-0".to_string()), ("100.116.249.18".to_string(), "ipad-pro-11-gen-2".to_string())]);
        assert!(parse_targets("").is_empty());
    }

    #[test]
    fn a_phone_is_known_by_the_first_label_of_its_dns_name() {
        let j = r#"{"Self":{"HostName":"ifrit","DNSName":"ifrit.tail1.ts.net.","OS":"linux"},"Peer":{"a":{"HostName":"localhost","DNSName":"iphone-12.tail1.ts.net.","OS":"iOS"},"b":{"HostName":"homelab-0","DNSName":"homelab-0.tail1.ts.net.","OS":"linux"}}}"#;
        let os = parse_os(j);
        assert_eq!(os.get("iphone-12").map(String::as_str), Some("iOS"));
        let t = vec![("100.1.1.1".to_string(), "iphone-12".to_string()), ("100.1.1.2".to_string(), "homelab-0".to_string()), ("100.1.1.3".to_string(), "unknown".to_string())];
        let p = peers(&t, &os);
        assert_eq!((p[0].kind.as_str(), p[1].kind.as_str(), p[2].kind.as_str()), ("mobile", "desktop", ""));
        assert!(p.iter().all(|x| x.taildrop && x.model == "Tailscale" && x.port == 0));
        assert!(parse_os("not json").is_empty());
    }
}
