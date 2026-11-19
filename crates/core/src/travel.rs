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

/// A place found by name.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    /// Where it is, in a few words (the street and the town).
    pub detail: String,
    pub lat: f64,
    pub lon: f64,
}

/// The places of a Nominatim answer (`format=jsonv2`).
pub fn parse_places(json: &str) -> Vec<Place> {
    let Ok(Value::Array(a)) = serde_json::from_str::<Value>(json) else { return vec![] };
    a.iter()
        .filter_map(|p| {
            let num = |k: &str| p.get(k).and_then(|x| x.as_str().map(str::to_string).or_else(|| x.as_f64().map(|f| f.to_string()))).and_then(|s| s.parse::<f64>().ok());
            let (lat, lon) = (num("lat")?, num("lon")?);
            let display = p.get("display_name")?.as_str()?;
            let name = p.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| display.split(',').next().unwrap_or(display).trim().to_string());
            // after the name: the next two parts are enough to tell places apart
            let detail: Vec<&str> = display.split(',').map(str::trim).skip(1).filter(|s| *s != name).take(2).collect();
            Some(Place { name: name.chars().take(60).collect(), detail: detail.join(", ").chars().take(70).collect(), lat, lon })
        })
        .collect()
}

/// An encoded polyline (Google's format) at the given precision (5 or 6 digits; 7 for MOTIS): latitude and longitude.
pub fn decode_polyline(s: &str, precision: u32) -> Vec<(f64, f64)> {
    let factor = 10f64.powi(precision as i32);
    let (bytes, mut i, mut lat, mut lon) = (s.as_bytes(), 0usize, 0i64, 0i64);
    let mut out = vec![];
    let next = |i: &mut usize| -> Option<i64> {
        let (mut shift, mut result) = (0u32, 0i64);
        loop {
            let b = (*bytes.get(*i)? as i64) - 63;
            *i += 1;
            result |= (b & 0x1f) << shift;
            shift += 5;
            if b < 0x20 {
                break;
            }
            if shift > 60 {
                return None;
            }
        }
        Some(if result & 1 != 0 { !(result >> 1) } else { result >> 1 })
    };
    while i < bytes.len() {
        let (Some(dlat), Some(dlon)) = (next(&mut i), next(&mut i)) else { break };
        lat += dlat;
        lon += dlon;
        out.push((lat as f64 / factor, lon as f64 / factor));
    }
    out
}

/// A stretch of a route: on foot (drawn dashed) or on a vehicle.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub points: Vec<(f64, f64)>,
    pub walk: bool,
}

/// The line of the first road route (OSRM with `geometries=geojson`).
pub fn parse_osrm_path(json: &str) -> Vec<(f64, f64)> {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return vec![] };
    v.pointer("/routes/0/geometry/coordinates").and_then(Value::as_array).map(|c| c.iter().filter_map(|p| Some((p.get(1)?.as_f64()?, p.get(0)?.as_f64()?))).collect()).unwrap_or_default()
}

fn legs_of(it: &Value) -> Vec<Segment> {
    it.get("legs")
        .and_then(Value::as_array)
        .map(|legs| {
            legs.iter()
                .filter_map(|l| {
                    let g = l.get("legGeometry")?;
                    let points = decode_polyline(g.get("points")?.as_str()?, g.get("precision").and_then(Value::as_u64).unwrap_or(7) as u32);
                    (points.len() >= 2).then(|| Segment { points, walk: l.get("mode").and_then(Value::as_str) == Some("WALK") })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The way by public transport that gets there first, leaving from now on, with its line on the map.
pub fn parse_transit_now(json: &str) -> Option<(Trip, Vec<Segment>)> {
    let v: Value = serde_json::from_str(json).ok()?;
    let best = v
        .get("itineraries")?
        .as_array()?
        .iter()
        .filter_map(|it| {
            let (start, end) = (parse_iso(it.get("startTime")?.as_str()?)?, parse_iso(it.get("endTime")?.as_str()?)?);
            Some((end, start, it))
        })
        .min_by_key(|(end, start, _)| (*end, std::cmp::Reverse(*start)))?;
    let (end, start, it) = best;
    Some((Trip { secs: it.get("duration").and_then(Value::as_i64).unwrap_or((end - start) / 1000), meters: None, leave_ms: Some(start), transfers: it.get("transfers").and_then(Value::as_u64).map(|t| t as u32) }, legs_of(it)))
}

/// The walk the planner offers on its own, with its line.
pub fn parse_walk_path(json: &str) -> Option<(i64, Vec<Segment>)> {
    let v: Value = serde_json::from_str(json).ok()?;
    let d = v.get("direct")?.as_array()?.iter().filter(|d| d.get("legs").and_then(Value::as_array).is_some_and(|l| !l.is_empty() && l.iter().all(|x| x.get("mode").and_then(Value::as_str) == Some("WALK")))).min_by_key(|d| d.get("duration").and_then(Value::as_i64).unwrap_or(i64::MAX))?;
    Some((d.get("duration")?.as_i64()?, legs_of(d).into_iter().map(|s| Segment { walk: true, ..s }).collect()))
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
    fn places_are_read_with_a_name_and_a_few_words_of_where() {
        let j = r#"[{"lat":"43.7078625","lon":"10.3983886","name":"Pisa Centrale","display_name":"Pisa Centrale, 9, Piazza della Stazione, San Giusto, Pisa, Toscana, 56126, Italia"},
                    {"lat":"43.7","lon":"10.4","name":"","display_name":"Via Roma, Pisa, Toscana"},{"lat":"x","lon":"1","display_name":"bad"}]"#;
        let p = parse_places(j);
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].name.as_str(), p[0].detail.as_str()), ("Pisa Centrale", "9, Piazza della Stazione"));
        assert_eq!((p[1].name.as_str(), p[1].detail.as_str()), ("Via Roma", "Pisa, Toscana"), "no name: the first part of the address");
        assert!(parse_places("nope").is_empty());
    }

    #[test]
    fn a_polyline_decodes_and_the_routes_give_their_lines() {
        // the Google documentation's example, at precision 5
        let pts = decode_polyline("_p~iF~ps|U_ulLnnqC_mqNvxq`@", 5);
        assert_eq!(pts, vec![(38.5, -120.2), (40.7, -120.95), (43.252, -126.453)]);
        assert!(decode_polyline("", 5).is_empty() && decode_polyline("~", 5).is_empty(), "garbage does not loop");
        let line = parse_osrm_path(r#"{"routes":[{"geometry":{"coordinates":[[10.4017,43.7167],[10.4,43.72]]}}]}"#);
        assert_eq!(line, vec![(43.7167, 10.4017), (43.72, 10.4)]);
        let j = r#"{"itineraries":[
            {"duration":1200,"startTime":"2026-11-20T07:50:00Z","endTime":"2026-11-20T08:10:00Z","transfers":1,"legs":[{"mode":"WALK","legGeometry":{"points":"_p~iF~ps|U_ulLnnqC","precision":5}},{"mode":"BUS","legGeometry":{"points":"_p~iF~ps|U_ulLnnqC_mqNvxq`@","precision":5}}]},
            {"duration":600,"startTime":"2026-11-20T07:54:00Z","endTime":"2026-11-20T08:04:00Z","transfers":0,"legs":[{"mode":"BUS","legGeometry":{"points":"_p~iF~ps|U_ulLnnqC","precision":5}}]}]}"#;
        let (trip, segs) = parse_transit_now(j).unwrap();
        assert_eq!((trip.secs, trip.transfers), (600, Some(0)), "the one that arrives first");
        assert_eq!(segs.len(), 1);
        let two = parse_transit_now(&j.replace("07:54:00Z", "07:30:00Z").replace("08:04:00Z", "08:20:00Z")).unwrap().1;
        assert!(two[0].walk && !two[1].walk, "walking stretches are marked");
        let (secs, w) = parse_walk_path(r#"{"direct":[{"duration":331,"legs":[{"mode":"WALK","legGeometry":{"points":"_p~iF~ps|U_ulLnnqC","precision":5}}]}]}"#).unwrap();
        assert_eq!((secs, w[0].walk), (331, true));
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
