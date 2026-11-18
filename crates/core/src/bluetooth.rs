//! What `bluetoothctl` prints about connected devices: the list and one device's details.

/// What a device is, from BlueZ's `Icon` and its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Earbuds,
    Headphones,
    Headset,
    Keyboard,
    Mouse,
    Gamepad,
    Other,
}

impl Kind {
    pub fn icon(self) -> &'static str {
        match self {
            Kind::Earbuds => "device-airpods",
            Kind::Headphones => "headphones",
            Kind::Headset => "headset",
            Kind::Keyboard => "keyboard",
            Kind::Mouse => "mouse",
            Kind::Gamepad => "device-gamepad-2",
            Kind::Other => "bluetooth",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub name: String,
    pub kind: Kind,
    pub connected: bool,
    /// 0..=100 when the device reports it.
    pub battery: Option<u8>,
}

/// `Device AA:BB:CC:DD:EE:FF Name` lines -> (address, name).
pub fn parse_list(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("Device ")?;
            let (mac, name) = rest.split_once(' ').unwrap_or((rest, ""));
            (mac.matches(':').count() == 5).then(|| (mac.to_string(), name.trim().to_string()))
        })
        .collect()
}

/// The output of `bluetoothctl info <address>`.
pub fn parse_info(text: &str) -> Option<Info> {
    let field = |key: &str| text.lines().find_map(|l| l.trim().strip_prefix(key)).map(str::trim);
    let name = field("Alias:").or_else(|| field("Name:"))?.to_string();
    let icon = field("Icon:").unwrap_or("");
    let lower = name.to_lowercase();
    let kind = if lower.contains("pods") || lower.contains("buds") || lower.contains("earbud") || lower.contains("earphone") || lower.split_whitespace().any(|w| w == "ear" || w.starts_with("ear(")) {
        Kind::Earbuds
    } else {
        match icon {
            "audio-headphones" => Kind::Headphones,
            "audio-headset" => Kind::Headset,
            "audio-card" if lower.contains("head") => Kind::Headphones,
            "input-keyboard" => Kind::Keyboard,
            "input-mouse" | "input-tablet" => Kind::Mouse,
            "input-gaming" => Kind::Gamepad,
            _ => Kind::Other,
        }
    };
    // "Battery Percentage: 0x55 (85)"
    let battery = field("Battery Percentage:").and_then(|v| v.split(['(', ')']).nth(1).and_then(|n| n.trim().parse::<u8>().ok()).or_else(|| v.trim_start_matches("0x").parse::<u8>().ok())).filter(|b| *b <= 100);
    Some(Info { name, kind, connected: field("Connected:") == Some("yes"), battery })
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: &str = "Device 88:C9:E8:12:34:56 (public)\n\tName: WH-1000XM5\n\tAlias: Sony XM5\n\tClass: 0x00240418\n\tIcon: audio-headphones\n\tPaired: yes\n\tConnected: yes\n\tUUID: Audio Sink\n\tBattery Percentage: 0x55 (85)\n";

    #[test]
    fn lists_connected_devices() {
        let l = parse_list("Device 88:C9:E8:12:34:56 Sony XM5\nDevice AA:BB:CC:DD:EE:FF MX Master 3\nnot a device\nDevice nope x\n");
        assert_eq!(l, vec![("88:C9:E8:12:34:56".into(), "Sony XM5".into()), ("AA:BB:CC:DD:EE:FF".into(), "MX Master 3".into())]);
        assert!(parse_list("").is_empty());
    }

    #[test]
    fn reads_a_device_with_its_battery() {
        let i = parse_info(INFO).unwrap();
        assert_eq!(i, Info { name: "Sony XM5".into(), kind: Kind::Headphones, connected: true, battery: Some(85) });
    }

    #[test]
    fn works_without_a_battery_and_knows_kinds() {
        let i = parse_info("Device X\n\tAlias: Keychron K2\n\tIcon: input-keyboard\n\tConnected: no\n").unwrap();
        assert_eq!((i.kind, i.connected, i.battery), (Kind::Keyboard, false, None));
        assert_eq!(parse_info("\tAlias: AirPods Pro\n\tIcon: audio-card\n\tConnected: yes\n").unwrap().kind, Kind::Earbuds, "by name");
        assert_eq!(parse_info("\tAlias: Nothing Ear Scanna\n\tIcon: audio-headset\n\tConnected: yes\n").unwrap().kind, Kind::Earbuds, "Ear by name");
        assert_eq!(parse_info("\tAlias: Cuffie Scanna\n\tIcon: audio-headset\n\tConnected: yes\n").unwrap().kind, Kind::Headset);
        assert_eq!(parse_info("\tAlias: Galaxy Buds2\n\tConnected: yes\n").unwrap().kind, Kind::Earbuds);
        assert_eq!(parse_info("\tAlias: Mouse\n\tIcon: input-mouse\n\tConnected: yes\n").unwrap().kind, Kind::Mouse);
        assert_eq!(parse_info("\tAlias: X\n\tIcon: phone\n\tConnected: yes\n").unwrap().kind, Kind::Other);
        assert!(parse_info("Device not available").is_none());
    }

    #[test]
    fn a_nonsense_battery_is_ignored() {
        assert_eq!(parse_info("\tAlias: A\n\tConnected: yes\n\tBattery Percentage: 0xff (255)\n").unwrap().battery, None);
        assert_eq!(parse_info("\tAlias: A\n\tConnected: yes\n\tBattery Percentage: 0x0 (0)\n").unwrap().battery, Some(0));
    }

    #[test]
    fn every_kind_has_a_bundled_icon_name() {
        for k in [Kind::Earbuds, Kind::Headphones, Kind::Headset, Kind::Keyboard, Kind::Mouse, Kind::Gamepad, Kind::Other] {
            assert!(!k.icon().is_empty());
        }
    }
}
