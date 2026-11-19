//! Where this machine is: GeoClue (through its `where-am-i` demo client, the one program GeoClue lets ask without a
//! configuration change), else the location set for the weather. The fix comes from GeoClue's own sources (Wi-Fi,
//! GeoIP): the island does not call any location service itself.

use super::Reporter;
use island_ui::model::{PosSource, Position};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Asks GeoClue (waits `secs`: the first answer is often a coarse GeoIP one and a better one follows).
pub fn fix(secs: u32) -> Option<(f64, f64, f64)> {
    if !std::path::Path::new(WHERE_AM_I).exists() {
        return None;
    }
    let out = Command::new(WHERE_AM_I).args(["-t", &secs.to_string()]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    let best = parse_where_am_i(&String::from_utf8_lossy(&out.stdout)).into_iter().last()?;
    if let Ok(mut l) = LAST.lock() {
        *l = Some((best.0, best.1, best.2, now_ms()));
    }
    Some(best)
}

/// The position the trips start from: a GeoClue fix of the last quarter hour, else None (the caller uses the settings).
pub fn recent() -> Option<(f64, f64)> {
    let l = LAST.lock().ok()?;
    l.filter(|(_, _, _, at)| now_ms() - at < 15 * 60_000).map(|(la, lo, _, _)| (la, lo))
}

/// Looks for a fix and reports it (or the location of the settings when GeoClue gives none).
pub fn refresh(rep: &Reporter) {
    let rep = rep.clone();
    std::thread::spawn(move || {
        let fixed = fix(14);
        log::debug!("position: {fixed:?}");
        let found = fixed.map(|(lat, lon, accuracy_m)| Position { lat, lon, accuracy_m, at_ms: now_ms(), source: PosSource::Geoclue });
        let pos = found.or_else(settings_position);
        if let Some(p) = pos {
            rep.patch(move |d| d.position = Some(p));
        }
    });
}

fn settings_position() -> Option<Position> {
    let s = crate::config::load();
    Some(Position { lat: s.latitude?, lon: s.longitude?, accuracy_m: 0.0, at_ms: now_ms(), source: PosSource::Settings })
}

pub fn start(rep: Reporter) {
    if let Some(p) = settings_position() {
        rep.patch(move |d| d.position = Some(p));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fixes_of_where_am_i_are_read() {
        let t = "Client object: /org/freedesktop/GeoClue2/Client/2\n\nNew location:\nLatitude:    43.708500°\nLongitude:   10.403600°\nAccuracy:    25000 meters\nDescription: GeoIP (ichnaea)\n\nNew location:\nLatitude:    43.725502°\nLongitude:   10.390814°\nAccuracy:    1120.5 meters\n";
        assert_eq!(parse_where_am_i(t), vec![(43.7085, 10.4036, 25000.0), (43.725502, 10.390814, 1120.5)]);
        assert!(parse_where_am_i("Failed to connect").is_empty());
        assert!(parse_where_am_i("Latitude: 1°\nAccuracy: 5 meters\n").is_empty(), "a fix needs both coordinates");
    }
}
