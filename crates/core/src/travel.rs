//! Getting to an event: which locations are places, how to look them up, and how to read the answers of the geocoder
//! (Nominatim), the road router (OSRM) and the public transport planner (Transitous, MOTIS). No I/O here.

use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mode {
    #[default]
    Car,
    Walk,
    Transit,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Car => "car",
            Mode::Walk => "walk",
            Mode::Transit => "transit",
        }
    }

    pub fn parse(s: &str) -> Option<Mode> {
        match s.trim().to_lowercase().as_str() {
            "car" | "drive" | "driving" | "auto" => Some(Mode::Car),
            "walk" | "foot" | "walking" | "piedi" => Some(Mode::Walk),
            "transit" | "bus" | "train" | "public" | "mezzi" => Some(Mode::Transit),
            _ => None,
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Mode::Car => "car",
            Mode::Walk => "walk",
            Mode::Transit => "bus",
        }
    }
}

/// How long it takes and when to leave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trip {
    pub secs: i64,
    pub meters: Option<i64>,
    /// When to leave to arrive on time (None: unknown).
    pub leave_ms: Option<i64>,
    pub transfers: Option<u32>,
}

/// A place worth a route: not a video call, not a link, not a room number on its own.
pub fn is_physical(location: &str) -> bool {
    let l = location.trim().to_lowercase();
    if l.len() < 4 || l.contains("http://") || l.contains("https://") || l.contains("www.") {
        return false;
    }
    const ONLINE: &[&str] = &["teams", "zoom", "meet.google", "google meet", "webex", "skype", "online", "remote", "virtual", "telefono", "phone call", "microsoft"];
    !ONLINE.iter().any(|w| l.contains(w))
}

/// What to ask the geocoder, most specific first: a room and a building are not on the map, the street and the town are.
pub fn queries(location: &str) -> Vec<String> {
    let parts: Vec<&str> = location.split([',', '\n', ';']).map(str::trim).filter(|p| !p.is_empty()).collect();
    let mut out = vec![];
    if !parts.is_empty() {
        out.push(parts.join(", "));
    }
    for skip in 1..parts.len().saturating_sub(1) {
        out.push(parts[skip..].join(", "));
    }
    out.dedup();
    out
}

pub fn parse_geocode(json: &str) -> Option<(f64, f64)> {
    let v: Value = serde_json::from_str(json).ok()?;
    let first = v.as_array()?.first()?;
    let num = |k: &str| first.get(k).and_then(|x| x.as_str().map(str::to_string).or_else(|| x.as_f64().map(|f| f.to_string()))).and_then(|s| s.parse::<f64>().ok());
    let (lat, lon) = (num("lat")?, num("lon")?);
    (lat.abs() <= 90.0 && lon.abs() <= 180.0).then_some((lat, lon))
}

/// Seconds and meters of the first route.
pub fn parse_osrm(json: &str) -> Option<(i64, i64)> {
    let v: Value = serde_json::from_str(json).ok()?;
    if v.get("code")?.as_str()? != "Ok" {
        return None;
    }
    let r = v.get("routes")?.as_array()?.first()?;
    Some((r.get("duration")?.as_f64()?.round() as i64, r.get("distance")?.as_f64()?.round() as i64))
}

/// `2026-11-20T07:34:00Z` (or with a +hh:mm offset) in milliseconds since the epoch.
pub fn parse_iso(s: &str) -> Option<i64> {
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let (clock, off) = match time.find(['Z', '+']).or_else(|| time.rfind('-')) {
        Some(i) => (&time[..i], &time[i..]),
        None => (time, "Z"),
    };
    let mut t = clock.split(':').map(|x| x.split('.').next().and_then(|v| v.parse::<i64>().ok()));
    let (h, mi, se) = (t.next()??, t.next()??, t.next().flatten().unwrap_or(0));
    let off_secs = match off {
        "Z" => 0,
        o => {
            let sign = if o.starts_with('-') { -1 } else { 1 };
            let mut p = o[1..].split(':').map(|x| x.parse::<i64>().ok());
            sign * (p.next()?? * 3600 + p.next().flatten().unwrap_or(0) * 60)
        }
    };
    // days from civil (Howard Hinnant)
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 86_400) + h * 3600 + mi * 60 + se - off_secs) * 1000)
}

/// Milliseconds since the epoch as `2026-11-20T07:34:09Z`.
pub fn iso_utc(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// The best way by public transport that arrives by `arrive_by_ms`: the one that leaves latest. None when there is none.
pub fn parse_transit(json: &str, arrive_by_ms: i64) -> Option<Trip> {
    let v: Value = serde_json::from_str(json).ok()?;
    v.get("itineraries")?
        .as_array()?
        .iter()
        .filter_map(|it| {
            let (start, end) = (parse_iso(it.get("startTime")?.as_str()?)?, parse_iso(it.get("endTime")?.as_str()?)?);
            (end <= arrive_by_ms).then(|| Trip { secs: it.get("duration").and_then(Value::as_i64).unwrap_or((end - start) / 1000), meters: None, leave_ms: Some(start), transfers: it.get("transfers").and_then(Value::as_u64).map(|t| t as u32) })
        })
        .max_by_key(|t| t.leave_ms)
}

/// The walk the planner offers on its own (`direct`), in seconds.
pub fn parse_walk(json: &str) -> Option<i64> {
    let v: Value = serde_json::from_str(json).ok()?;
    v.get("direct")?.as_array()?.iter().filter(|d| d.get("legs").and_then(Value::as_array).is_some_and(|l| !l.is_empty() && l.iter().all(|x| x.get("mode").and_then(Value::as_str) == Some("WALK")))).filter_map(|d| d.get("duration")?.as_i64()).min()
}

/// A straight-line walk (with the detour streets make) at 1.25 m/s, for when no planner answers.
pub fn walk_estimate(from: (f64, f64), to: (f64, f64)) -> i64 {
    let (r, to_rad) = (6_371_000.0f64, std::f64::consts::PI / 180.0);
    let (dlat, dlon) = ((to.0 - from.0) * to_rad, (to.1 - from.1) * to_rad);
    let a = (dlat / 2.0).sin().powi(2) + (from.0 * to_rad).cos() * (to.0 * to_rad).cos() * (dlon / 2.0).sin().powi(2);
    let meters = 2.0 * r * a.sqrt().asin() * 1.3;
    (meters / 1.25).round() as i64
}

/// When to leave for a trip by road or on foot: with a few minutes of margin.
pub fn leave_for(start_ms: i64, secs: i64, margin_secs: i64) -> i64 {
    start_ms - (secs + margin_secs) * 1000
}

/// `12 min`, `1 h 05 min`.
pub fn fmt_secs(secs: i64) -> String {
    let m = (secs + 30) / 60;
    if m < 60 { format!("{} min", m.max(1)) } else { format!("{} h {:02} min", m / 60, m % 60) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_calls_and_links_are_not_places() {
        assert!(is_physical("Aula 12, Polo Fibonacci, Largo Bruno Pontecorvo 3, Pisa"));
        assert!(is_physical("Piazza dei Cavalieri, Pisa"));
        assert!(!is_physical("https://teams.microsoft.com/l/meetup-join/abc"));
        assert!(!is_physical("Microsoft Teams Meeting"));
        assert!(!is_physical("Zoom"));
        assert!(!is_physical("Online") && !is_physical("") && !is_physical("  "));
    }

    #[test]
    fn the_geocoder_is_asked_from_the_full_address_down_to_the_street() {
        assert_eq!(
            queries("Aula 12, Polo Fibonacci, Largo Pontecorvo 3, Pisa"),
            ["Aula 12, Polo Fibonacci, Largo Pontecorvo 3, Pisa", "Polo Fibonacci, Largo Pontecorvo 3, Pisa", "Largo Pontecorvo 3, Pisa"]
        );
        assert_eq!(queries("Piazza dei Cavalieri, Pisa"), ["Piazza dei Cavalieri, Pisa"]);
        assert_eq!(queries("Pisa"), ["Pisa"]);
        assert!(queries("").is_empty());
    }

    #[test]
    fn the_answers_are_read() {
        assert_eq!(parse_geocode(r#"[{"lat":"43.7194618","lon":"10.4000909","name":"x"}]"#), Some((43.7194618, 10.4000909)));
        assert_eq!(parse_geocode("[]"), None);
        assert_eq!(parse_geocode(r#"[{"lat":"95","lon":"1"}]"#), None);
        assert_eq!(parse_osrm(r#"{"code":"Ok","routes":[{"duration":196.7,"distance":1577.1}]}"#), Some((197, 1577)));
        assert_eq!(parse_osrm(r#"{"code":"NoRoute","routes":[]}"#), None);
    }

    #[test]
    fn times_are_read_with_and_without_offsets() {
        assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso("2026-11-20T07:34:00Z"), Some(1_795_160_040_000));
        assert_eq!(parse_iso("2026-11-20T08:34:00+01:00"), parse_iso("2026-11-20T07:34:00Z"));
        assert_eq!(parse_iso("2026-11-20T02:34:00-05:00"), parse_iso("2026-11-20T07:34:00Z"));
        assert_eq!(parse_iso("nonsense"), None);
    }

    #[test]
    fn a_time_goes_out_and_comes_back() {
        for ms in [0, 1_795_160_049_000, 951_782_400_000, 4_102_444_799_000] {
            assert_eq!(parse_iso(&iso_utc(ms)), Some(ms), "{}", iso_utc(ms));
        }
        assert_eq!(iso_utc(1_795_160_049_000), "2026-11-20T07:34:09Z");
        assert_eq!(iso_utc(951_782_400_000), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn transit_picks_the_latest_departure_that_arrives_in_time() {
        let j = r#"{"itineraries":[
            {"duration":600,"startTime":"2026-11-20T07:34:00Z","endTime":"2026-11-20T07:44:00Z","transfers":0},
            {"duration":600,"startTime":"2026-11-20T07:54:00Z","endTime":"2026-11-20T08:04:00Z","transfers":1},
            {"duration":900,"startTime":"2026-11-20T07:44:00Z","endTime":"2026-11-20T07:59:00Z","transfers":2}]}"#;
        let by = parse_iso("2026-11-20T08:00:00Z").unwrap();
        let t = parse_transit(j, by).unwrap();
        assert_eq!((t.secs, t.leave_ms, t.transfers), (900, parse_iso("2026-11-20T07:44:00Z"), Some(2)));
        assert!(parse_transit(j, parse_iso("2026-11-20T07:00:00Z").unwrap()).is_none(), "nothing arrives by seven");
        assert!(parse_transit(r#"{"itineraries":[]}"#, by).is_none());
    }

    #[test]
    fn the_walk_is_the_planners_own_or_an_estimate() {
        assert_eq!(parse_walk(r#"{"direct":[{"duration":331,"legs":[{"mode":"WALK"}]},{"duration":200,"legs":[{"mode":"BIKE"}]}]}"#), Some(331));
        assert_eq!(parse_walk(r#"{"direct":[]}"#), None);
        // 1 km north is about 1.3 km of streets: 17 minutes
        let s = walk_estimate((43.0, 10.0), (43.0 + 1000.0 / 111_195.0, 10.0));
        assert!((s - 1040).abs() < 15, "{s}");
        assert_eq!(walk_estimate((43.0, 10.0), (43.0, 10.0)), 0);
    }

    #[test]
    fn the_leave_time_has_a_margin_and_durations_read_well() {
        assert_eq!(leave_for(10_000_000, 600, 300), 10_000_000 - 900_000);
        assert_eq!(fmt_secs(197), "3 min");
        assert_eq!(fmt_secs(3960), "1 h 06 min");
        assert_eq!(fmt_secs(5), "1 min");
        assert_eq!(Mode::parse("Transit"), Some(Mode::Transit));
        assert_eq!(Mode::parse("piedi"), Some(Mode::Walk));
        assert_eq!(Mode::parse("rocket"), None);
        assert_eq!((Mode::Car.icon(), Mode::Walk.name()), ("car", "walk"));
    }
}
