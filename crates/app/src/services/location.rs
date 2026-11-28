//! Where this machine is: GeoClue (through its `where-am-i` demo client, the one program GeoClue lets ask without a
//! configuration change), else the location set for the weather. The fix comes from GeoClue's own sources (Wi-Fi,
//! GeoIP): the island does not call any location service itself.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use super::Reporter;
use island_ui::model::{PosSource, Position};
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_os = "linux")]
const WHERE_AM_I: &str = "/usr/lib/geoclue-2.0/demos/where-am-i";

/// The newest fix: latitude, longitude, accuracy in metres, when (epoch ms).
static LAST: Mutex<Option<(f64, f64, f64, i64)>> = Mutex::new(None);

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// The fixes in the output of `where-am-i`, oldest first: (latitude, longitude, accuracy).
pub fn parse_where_am_i(text: &str) -> Vec<(f64, f64, f64)> {
    let num = |line: &str, key: &str| line.strip_prefix(key).and_then(|v| v.trim().trim_end_matches("°").trim_end_matches("meters").trim().parse::<f64>().ok());
    let (mut out, mut lat, mut lon) = (vec![], None, None);
    for l in text.lines() {
        let l = l.trim();
        if let Some(v) = num(l, "Latitude:") {
            lat = Some(v);
        } else if let Some(v) = num(l, "Longitude:") {
            lon = Some(v);
        } else if let Some(acc) = num(l, "Accuracy:") {
            if let (Some(la), Some(lo)) = (lat.take(), lon.take()) {
                out.push((la, lo, acc));
            }
        }
    }
    out
}

/// Asks the system where it is: GeoClue on Linux (waits `secs`: the first answer is often a coarse GeoIP one and a
/// better one follows), the location service on Windows; nothing on macOS yet.
pub fn fix(secs: u32) -> Option<(f64, f64, f64)> {
    fix_from(secs).map(|(f, _)| f)
}

/// The same, and where the fix came from.
fn fix_from(secs: u32) -> Option<((f64, f64, f64), PosSource)> {
    // the Wi-Fi lookup is opt-in (it sends the networks around to beacondb.net); GeoClue is the fallback
    #[cfg(target_os = "linux")]
    if crate::config::load().ui.wifi_position {
        if let Some(w) = wifi_fix() {
            remember(w);
            return Some((w, PosSource::Wifi));
        }
    }
    let best = ask(secs)?;
    remember(best);
    Some((best, PosSource::Geoclue))
}

fn remember(best: (f64, f64, f64)) {
    if let Ok(mut l) = LAST.lock() {
        *l = Some((best.0, best.1, best.2, now_ms()));
    }
}

/// The networks `nmcli -t -f BSSID,SSID,SIGNAL dev wifi list` prints, as (hardware address, dBm), strongest first, at
/// most `max`. Networks whose name ends in `_nomap` ask not to be used for positioning and are left out.
pub fn parse_nmcli_aps(text: &str, max: usize) -> Vec<(String, i32)> {
    let mut v: Vec<(String, i32)> = text
        .lines()
        .filter_map(|l| {
            // colons inside the address are escaped (A4\:C7\:...) in the terse format
            let l = l.replace("\\:", "\u{1}");
            let mut f = l.split(':');
            let mac = f.next()?.replace('\u{1}', ":");
            let ssid = f.next()?;
            let signal: i32 = f.next()?.trim().parse().ok()?;
            (mac.len() == 17 && !ssid.to_lowercase().ends_with("_nomap") && !locally_administered(&mac)).then(|| (mac.to_lowercase(), signal / 2 - 100))
        })
        .collect();
    v.sort_by_key(|(_, dbm)| -dbm);
    v.dedup_by(|a, b| a.0 == b.0);
    v.truncate(max);
    v
}

/// Phones' hotspots and virtual networks use made-up addresses (the "locally administered" bit): they move with their owner and say nothing about the place.
fn locally_administered(mac: &str) -> bool {
    mac.get(1..2).and_then(|c| u8::from_str_radix(c, 16).ok()).is_some_and(|d| d & 2 != 0)
}

/// The request body of the Geolocation API (the Ichnaea / Google format that beacondb.net speaks): Wi-Fi networks only,
/// the caller's IP address is not to be used.
pub fn geolocate_body(aps: &[(String, i32)]) -> serde_json::Value {
    serde_json::json!({
        "considerIp": false,
        "wifiAccessPoints": aps.iter().map(|(mac, dbm)| serde_json::json!({"macAddress": mac, "signalStrength": dbm})).collect::<Vec<_>>(),
    })
}

/// (latitude, longitude, accuracy in metres) from an answer of the API.
pub fn parse_geolocate(v: &serde_json::Value) -> Option<(f64, f64, f64)> {
    let loc = v.get("location")?;
    Some((loc.get("lat")?.as_f64()?, loc.get("lng")?.as_f64()?, v.get("accuracy")?.as_f64()?))
}

/// Looks the position up from the Wi-Fi networks in range (at most 50, the strongest: the few nearest are often one building's and the service may not know them). At least two are needed.
/// Sends their hardware addresses and signal strengths to beacondb.net: only called when the user turned
/// `wifi_position` on. None when fewer than two are seen or the service does not know them.
#[cfg(target_os = "linux")]
fn wifi_fix() -> Option<(f64, f64, f64)> {
    let out = super::tool("nmcli").args(["-t", "-f", "BSSID,SSID,SIGNAL", "dev", "wifi", "list"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    let aps = parse_nmcli_aps(&String::from_utf8_lossy(&out.stdout), 50);
    if aps.len() < 2 {
        return None;
    }
    let v: serde_json::Value = super::net::agent().post("https://api.beacondb.net/v1/geolocate").send_json(geolocate_body(&aps)).ok()?.body_mut().read_json().ok()?;
    let got = parse_geolocate(&v);
    log::debug!("wifi position from {} networks: {got:?}", aps.len());
    got
}

#[cfg(target_os = "linux")]
fn ask(secs: u32) -> Option<(f64, f64, f64)> {
    if !std::path::Path::new(WHERE_AM_I).exists() {
        return None;
    }
    let out = Command::new(WHERE_AM_I).args(["-t", &secs.to_string()]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    parse_where_am_i(&String::from_utf8_lossy(&out.stdout)).into_iter().last()
}

#[cfg(target_os = "windows")]
fn ask(_secs: u32) -> Option<(f64, f64, f64)> {
    super::win_more::position()
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn ask(_secs: u32) -> Option<(f64, f64, f64)> {
    None
}

/// The position the trips start from: a GeoClue fix of the last quarter hour, else None.
pub fn recent() -> Option<(f64, f64)> {
    let l = LAST.lock().ok()?;
    l.filter(|(_, _, _, at)| now_ms() - at < 15 * 60_000).map(|(la, lo, _, _)| (la, lo))
}

/// The point the weather is for: where the machine is (a fix of the last twelve hours, not a guess wider than 50 km) when it
/// follows the position, else the coordinates set in the settings.
pub fn point(s: &crate::config::Settings) -> Option<(f64, f64)> {
    if s.ui.follow_position {
        if let Some((la, lo, acc, at)) = LAST.lock().ok().and_then(|l| *l) {
            if now_ms() - at < 12 * 3_600_000 && acc <= 50_000.0 {
                return Some((la, lo));
            }
        }
    }
    s.latitude.zip(s.longitude)
}

/// Looks for a fix now (the loop in `start` does it, and wakes at once).
pub fn refresh() {
    super::signal::bump(&super::LOCATION_ASK);
}

fn settings_position() -> Option<Position> {
    let s = crate::config::load();
    Some(Position { lat: s.latitude?, lon: s.longitude?, accuracy_m: 0.0, at_ms: now_ms(), source: PosSource::Settings })
}

/// Where the machine is, kept up to date: a fix at the start and every ten minutes, at once when asked, and the settings'
/// location when the position is not to be followed (or GeoClue gives none).
pub fn start(rep: Reporter) {
    if let Some(p) = settings_position() {
        rep.patch(move |d| d.position = Some(p));
    }
    std::thread::spawn(move || {
        let mut published: Option<(f64, f64)> = None;
        loop {
            let follow = crate::config::load().ui.follow_position;
            let found = if follow { fix_from(14).map(|((lat, lon, accuracy_m), source)| Position { lat, lon, accuracy_m, at_ms: now_ms(), source }) } else { None };
            log::debug!("position: follow {follow}, {:?}", found.as_ref().map(|p| (p.lat, p.lon, p.accuracy_m)));
            let pos = found.or_else(settings_position);
            if let Some(p) = pos {
                // the weather is for a place: ask it again only when the machine has moved a few kilometres
                let moved = published.is_none_or(|(la, lo)| island_core::map::distance_m((la, lo), (p.lat, p.lon)) > 3_000.0);
                if moved {
                    published = Some((p.lat, p.lon));
                    super::signal::bump(&super::LOCATION_GEN);
                }
                rep.patch(move |d| d.position = Some(p));
            }
            super::wait_for(600, &[&super::CONFIG_GEN, &super::LOCATION_ASK]);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_point_is_the_fix_when_following_and_the_settings_otherwise() {
        let mut s = crate::config::Settings { latitude: Some(43.0), longitude: Some(10.0), ..Default::default() };
        let set = |v: Option<(f64, f64, f64, i64)>| *LAST.lock().unwrap() = v;
        set(None);
        assert_eq!(point(&s), Some((43.0, 10.0)), "no fix: the settings");
        set(Some((44.5, 11.5, 300.0, now_ms())));
        assert_eq!(point(&s), Some((44.5, 11.5)), "a fix: here");
        s.ui.follow_position = false;
        assert_eq!(point(&s), Some((43.0, 10.0)), "fixed coordinates when not following");
        s.ui.follow_position = true;
        set(Some((44.5, 11.5, 80_000.0, now_ms())));
        assert_eq!(point(&s), Some((43.0, 10.0)), "a guess wider than 50 km is no position");
        set(Some((44.5, 11.5, 300.0, now_ms() - 13 * 3_600_000)));
        assert_eq!(point(&s), Some((43.0, 10.0)), "an old fix is not where you are");
        s.latitude = None;
        assert_eq!(point(&s), None, "nothing at all");
        set(None);
    }

    #[test]
    fn the_wifi_networks_are_listed_strongest_first_without_the_ones_that_opt_out() {
        let t = "A4\\:C7\\:F6\\:21\\:31\\:14:UniPisa:49\nA4\\:C7\\:F6\\:21\\:31\\:16:Teaching:80\nB8\\:00\\:00\\:00\\:00\\:01:Home_nomap:90\nA4\\:C7\\:F6\\:21\\:31\\:18::47\nbroken line\n";
        let aps = parse_nmcli_aps(t, 15);
        assert_eq!(aps, vec![("a4:c7:f6:21:31:16".to_string(), -60), ("a4:c7:f6:21:31:14".to_string(), -76), ("a4:c7:f6:21:31:18".to_string(), -77)]);
        assert_eq!(parse_nmcli_aps(t, 1).len(), 1, "capped");
        assert!(parse_nmcli_aps("B2\\:CA\\:4B\\:35\\:6F\\:22:moto g:87\n", 9).is_empty(), "a phone hotspot (made-up address) says nothing about the place");
        let body = geolocate_body(&aps);
        assert_eq!(body["considerIp"], false, "the IP address is not used");
        assert_eq!(body["wifiAccessPoints"][0]["macAddress"], "a4:c7:f6:21:31:16");
        assert_eq!(body["wifiAccessPoints"][0]["signalStrength"], -60);
    }

    #[test]
    fn the_answer_of_the_geolocation_api_is_read() {
        let v = serde_json::json!({"location": {"lat": 43.7, "lng": 10.4}, "accuracy": 35.0});
        assert_eq!(parse_geolocate(&v), Some((43.7, 10.4, 35.0)));
        assert_eq!(parse_geolocate(&serde_json::json!({"error": {"code": 404}})), None);
    }

    #[test]
    fn the_fixes_of_where_am_i_are_read() {
        let t = "Client object: /org/freedesktop/GeoClue2/Client/2\n\nNew location:\nLatitude:    43.708500°\nLongitude:   10.403600°\nAccuracy:    25000 meters\nDescription: GeoIP (ichnaea)\n\nNew location:\nLatitude:    43.725502°\nLongitude:   10.390814°\nAccuracy:    1120.5 meters\n";
        assert_eq!(parse_where_am_i(t), vec![(43.7085, 10.4036, 25000.0), (43.725502, 10.390814, 1120.5)]);
        assert!(parse_where_am_i("Failed to connect").is_empty());
        assert!(parse_where_am_i("Latitude: 1°\nAccuracy: 5 meters\n").is_empty(), "a fix needs both coordinates");
    }
}
