//! Getting to an event: the place of the event is looked up (Nominatim, OpenStreetMap), then the trip is timed by road
//! (OSRM), on foot or by public transport (Transitous). The start is the location set for the weather.
//! What leaves the machine: the event's place, and the start and arrival coordinates. Switch it off with `travel_times`.

use super::{net, Reporter};
use island_core::travel::{self as tr, Mode, Trip};
use island_ui::model::{Command, TripInfo};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const NOMINATIM: &str = "https://nominatim.openstreetmap.org/search";
const OSRM: &str = "https://router.project-osrm.org/route/v1/driving";
const TRANSITOUS: &str = "https://api.transitous.org/api/v1/plan";
const STOPS: &str = "https://api.transitous.org/api/v1/map/stops";

/// Places already looked up (None: nothing found), kept for the run.
type Coords = Option<(f64, f64)>;
static PLACES: Mutex<Option<HashMap<String, Coords>>> = Mutex::new(None);
/// The geocoder allows one request a second.
static LAST_GEOCODE: Mutex<Option<Instant>> = Mutex::new(None);

fn get(url: &str, query: &[(&str, &str)]) -> Result<String, String> {
    let mut req = net::agent().get(url);
    for (k, v) in query {
        req = req.query(k, v);
    }
    let mut r = req.call().map_err(|e| e.to_string())?;
    r.body_mut().read_to_string().map_err(|e| e.to_string())
}

fn geocode(place: &str) -> Option<(f64, f64)> {
    if let Some(hit) = PLACES.lock().ok()?.as_ref().and_then(|m| m.get(place).copied()) {
        return hit;
    }
    // look up from the whole address down to the street: the first that the map knows
    let mut found = None;
    for q in tr::queries(place) {
        if let Ok(mut last) = LAST_GEOCODE.lock() {
            if let Some(t) = *last {
                std::thread::sleep(Duration::from_millis(1100).saturating_sub(t.elapsed()));
            }
            *last = Some(Instant::now());
        }
        if let Some(c) = get(NOMINATIM, &[("q", &q), ("format", "json"), ("limit", "1")]).ok().and_then(|j| tr::parse_geocode(&j)) {
            found = Some(c);
            break;
        }
    }
    if let Ok(mut g) = PLACES.lock() {
        g.get_or_insert_with(HashMap::new).insert(place.to_string(), found);
    }
    found
}

/// The trip from `from` to `to`, arriving by `start_ms`.
pub fn plan(mode: Mode, from: (f64, f64), to: (f64, f64), start_ms: i64, margin_secs: i64) -> Result<Trip, String> {
    match mode {
        Mode::Car => {
            let url = format!("{OSRM}/{},{};{},{}", from.1, from.0, to.1, to.0);
            let (secs, meters) = tr::parse_osrm(&get(&url, &[("overview", "false")])?).ok_or("no road route")?;
            Ok(Trip { secs, meters: Some(meters), leave_ms: Some(tr::leave_for(start_ms, secs, margin_secs)), transfers: None })
        }
        Mode::Walk => {
            let secs = transit_plan(from, to, start_ms, false, false).ok().and_then(|j| tr::parse_walk(&j)).unwrap_or_else(|| tr::walk_estimate(from, to));
            Ok(Trip { secs, meters: None, leave_ms: Some(tr::leave_for(start_ms, secs, margin_secs)), transfers: None })
        }
        Mode::Transit | Mode::Train => {
            let rail = mode == Mode::Train;
            if rail {
                if let Some(t) = rail_trip(from, to, start_ms, margin_secs) {
                    return Ok(t);
                }
            }
            let j = transit_plan(from, to, start_ms, true, rail)?;
            // a short trip has no bus worth taking: the planner then offers the walk
            let Some(mut t) = tr::parse_transit(&j, start_ms) else {
                let secs = tr::parse_walk(&j).ok_or(if rail { "no train in time" } else { "no public transport in time" })?;
                return Ok(Trip { secs, meters: None, leave_ms: Some(tr::leave_for(start_ms, secs, margin_secs)), transfers: None });
            };
            // the margin: leave earlier by what was asked for
            t.leave_ms = t.leave_ms.map(|l| l - margin_secs * 1000);
            Ok(t)
        }
    }
}

/// The railway stop nearest to a place: a few kilometres around it are looked at, then a wider square.
fn rail_stop_near(at: (f64, f64)) -> Option<tr::RailStop> {
    for d in [0.06, 0.25] {
        let (min, max) = (format!("{},{}", at.0 - d, at.1 - d), format!("{},{}", at.0 + d, at.1 + d));
        if let Some(stop) = get(STOPS, &[("min", &min), ("max", &max)]).ok().and_then(|j| tr::rail_stops_near(&j, at).into_iter().next()) {
            return Some(stop);
        }
    }
    None
}

/// The ride between two stops, rail only.
fn rail_ride(from_stop: &str, to_stop: &str, start_ms: i64, arrive_by: bool) -> Result<String, String> {
    let iso = tr::iso_utc(start_ms);
    get(TRANSITOUS, &[("fromPlace", from_stop), ("toPlace", to_stop), ("time", &iso), ("arriveBy", if arrive_by { "true" } else { "false" }), ("numItineraries", "5"), ("transitModes", "RAIL")])
}

/// A walk between two points: the planner's own, else a straight line and an estimate.
fn walk_leg(a: (f64, f64), b: (f64, f64), at_ms: i64) -> (i64, Vec<tr::Segment>) {
    transit_plan(a, b, at_ms, false, false).ok().as_deref().and_then(tr::parse_walk_path).unwrap_or_else(|| (tr::walk_estimate(a, b), vec![tr::Segment { points: vec![a, b], walk: true }]))
}

/// By train as a person thinks of it: on foot to the station nearest to where you are, the train, off at the station nearest to where you
/// are going, on foot from there. None when there is no such pair of stations or no train between them (the caller then lets the planner choose).
fn rail_route(from: (f64, f64), to: (f64, f64), now_ms: i64) -> Option<(Trip, Vec<tr::Segment>)> {
    let (s1, s2) = (rail_stop_near(from)?, rail_stop_near(to)?);
    if s1.id == s2.id {
        return None;
    }
    let walk_in = walk_leg(from, (s1.lat, s1.lon), now_ms);
    let walk_out = walk_leg((s2.lat, s2.lon), to, now_ms + walk_in.0 * 1000);
    let ride = rail_ride(&s1.id, &s2.id, now_ms + walk_in.0 * 1000, false).ok().as_deref().and_then(tr::parse_transit_now)?;
    Some(tr::compose_rail(now_ms, walk_in, ride, walk_out))
}

/// The same for arriving by `arrive_ms`: when to leave, door to door.
fn rail_trip(from: (f64, f64), to: (f64, f64), arrive_ms: i64, margin_secs: i64) -> Option<Trip> {
    let (s1, s2) = (rail_stop_near(from)?, rail_stop_near(to)?);
    if s1.id == s2.id {
        return None;
    }
    let walk_in = tr::walk_estimate(from, (s1.lat, s1.lon));
    let walk_out = tr::walk_estimate((s2.lat, s2.lon), to);
    let at_stop2 = arrive_ms - walk_out * 1000;
    let ride = rail_ride(&s1.id, &s2.id, at_stop2, true).ok().as_deref().and_then(|j| tr::parse_transit(j, at_stop2))?;
    Some(Trip { secs: walk_in + ride.secs + walk_out, meters: None, leave_ms: ride.leave_ms.map(|l| l - (walk_in + margin_secs) * 1000), transfers: ride.transfers })
}

/// A plan from the transit planner; `rail` keeps only the railways (Trenitalia and the like), with the walks to the stations.
fn transit_plan(from: (f64, f64), to: (f64, f64), start_ms: i64, arrive_by: bool, rail: bool) -> Result<String, String> {
    let (f, t) = (format!("{},{}", from.0, from.1), format!("{},{}", to.0, to.1));
    let iso = tr::iso_utc(start_ms);
    let mut q: Vec<(&str, &str)> = vec![("fromPlace", &f), ("toPlace", &t), ("time", &iso), ("arriveBy", if arrive_by { "true" } else { "false" }), ("numItineraries", "5"), ("maxDirectTime", "7200")];
    if rail {
        q.push(("transitModes", "RAIL"));
        // the stations may be an hour's walk away: the planner gives up after fifteen minutes unless it is told otherwise
        q.push(("maxPreTransitTime", "3600"));
        q.push(("maxPostTransitTime", "3600"));
    }
    get(TRANSITOUS, &q)
}

/// The places for a name, near a point when there is one (the answers nearest to it come first).
fn search(query: &str, near: Option<(f64, f64)>) -> Result<Vec<island_core::travel::Place>, String> {
    if let Ok(mut last) = LAST_GEOCODE.lock() {
        if let Some(t) = *last {
            std::thread::sleep(Duration::from_millis(1100).saturating_sub(t.elapsed()));
        }
        *last = Some(Instant::now());
    }
    let view = near.map(|(la, lo)| format!("{},{},{},{}", lo - 0.3, la + 0.2, lo + 0.3, la - 0.2));
    let mut q: Vec<(&str, &str)> = vec![("q", query), ("format", "jsonv2"), ("limit", "6")];
    if let Some(v) = &view {
        q.push(("viewbox", v));
    }
    get(NOMINATIM, &q).map(|j| tr::parse_places(&j))
}

/// The way from `from` to `to`, leaving now, with the line to draw.
pub fn route_now(mode: Mode, from: (f64, f64), to: (f64, f64), now_ms: i64) -> Result<(Trip, Vec<tr::Segment>), String> {
    let straight = |walk: bool| vec![tr::Segment { points: vec![from, to], walk }];
    match mode {
        Mode::Car => {
            let url = format!("{OSRM}/{},{};{},{}", from.1, from.0, to.1, to.0);
            let j = get(&url, &[("overview", "full"), ("geometries", "geojson")])?;
            let (secs, meters) = tr::parse_osrm(&j).ok_or("no road route")?;
            let path = tr::parse_osrm_path(&j);
            Ok((Trip { secs, meters: Some(meters), leave_ms: Some(now_ms), transfers: None }, if path.len() >= 2 { vec![tr::Segment { points: path, walk: false }] } else { straight(false) }))
        }
        Mode::Walk | Mode::Transit | Mode::Train => {
            if mode == Mode::Train {
                if let Some(found) = rail_route(from, to, now_ms) {
                    return Ok(found);
                }
            }
            let j = transit_plan(from, to, now_ms, false, mode == Mode::Train).ok();
            if mode != Mode::Walk {
                if let Some((trip, segs)) = j.as_deref().and_then(tr::parse_transit_now) {
                    return Ok((trip, segs));
                }
            }
            // a walk: the planner's own, else a straight line and an estimate (a short trip has no bus worth taking)
            let (secs, segs) = j.as_deref().and_then(tr::parse_walk_path).unwrap_or_else(|| (tr::walk_estimate(from, to), straight(true)));
            Ok((Trip { secs, meters: None, leave_ms: Some(now_ms), transfers: None }, segs))
        }
    }
}

pub fn command(rep: &Reporter, c: Command) {
    match c {
        Command::SearchPlaces { query, near } => {
            let rep = rep.clone();
            std::thread::spawn(move || {
                let found = search(&query, near).unwrap_or_default();
                rep.patch(move |d| {
                    d.places = Some(found);
                    d.places_searching = false;
                });
            });
            return;
        }
        Command::PlanRoute { dest, mode, from } => {
            let rep = rep.clone();
            std::thread::spawn(move || {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
                let outcome = route_now(mode, from, (dest.lat, dest.lon), now);
                let info = match outcome {
                    Ok((trip, segments)) => island_ui::model::RouteInfo { dest, mode, trip: Some(trip), error: None, segments },
                    Err(e) => island_ui::model::RouteInfo { dest, mode, trip: None, error: Some(e), segments: vec![] },
                };
                rep.patch(move |d| {
                    // an answer for a place that is no longer the one picked is dropped
                    d.route = Some(info);
                    d.route_pending = false;
                });
            });
            return;
        }
        _ => {}
    }
    let Command::PlanTrip { event_id, location, start_ms, mode, margin_secs } = c else { return };
    let rep = rep.clone();
    std::thread::spawn(move || {
        let settings = crate::config::load();
        // from where the machine is (a GeoClue fix of the last quarter hour, asked for when there is none), else the settings
        let here = if settings.ui.follow_position { super::location::recent().or_else(|| super::location::fix(6).map(|(la, lo, _)| (la, lo))) } else { None };
        let from = here.or(settings.latitude.zip(settings.longitude));
        let mut dest_found = None;
        let outcome = match from {
            Some(from) => match geocode(&location) {
                Some(to) => {
                    dest_found = Some(to);
                    plan(mode, from, to, start_ms, margin_secs)
                }
                None => Err("place not found on the map".to_string()),
            },
            None => Err("set your location in the settings".to_string()),
        };
        let at_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
        let info = TripInfo { event_id, mode, trip: outcome.as_ref().ok().cloned(), error: outcome.err(), at_ms, dest: dest_found };
        rep.patch(move |d| {
            d.trips.retain(|t| !(t.event_id == info.event_id && t.mode == info.mode));
            d.trips.push(info);
            d.trips.truncate(12);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs the network: cargo test -- --ignored live_search_and_route"]
    fn live_search_and_route() {
        let here = (43.7255, 10.3908);
        let found = search("Pisa Centrale", Some(here)).expect("a search");
        eprintln!("found {:?}", found.iter().map(|p| (p.name.clone(), p.detail.clone())).collect::<Vec<_>>());
        assert!(!found.is_empty());
        let to = (found[0].lat, found[0].lon);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
        for mode in [Mode::Car, Mode::Walk, Mode::Transit, Mode::Train] {
            let (trip, segs) = route_now(mode, here, to, now).unwrap_or_else(|e| panic!("{mode:?}: {e}"));
            eprintln!("{mode:?}: {} {} segments, {} points", tr::fmt_secs(trip.secs), segs.len(), segs.iter().map(|s| s.points.len()).sum::<usize>());
            assert!(segs.iter().all(|s| s.points.len() >= 2) && trip.secs > 0);
        }
    }

    #[test]
    #[ignore = "needs the network: cargo test -- --ignored live_trip"]
    fn live_trip() {
        let from = (43.7167, 10.4017);
        let to = geocode("Piazza dei Cavalieri, Pisa").expect("geocoded");
        let far = geocode("Piazza della Signoria, Firenze").expect("geocoded");
        let t = plan(Mode::Transit, (43.7083, 10.3986), far, tr::parse_iso("2026-11-23T09:30:00Z").unwrap(), 300).expect("transit to Florence");
        eprintln!("Firenze by transit: {} leave {:?} transfers {:?}", tr::fmt_secs(t.secs), t.leave_ms.map(tr::iso_utc), t.transfers);
        assert!(t.secs > 1800, "a train ride");
        let start = tr::parse_iso("2026-11-23T08:30:00Z").unwrap();
        for mode in [Mode::Car, Mode::Walk, Mode::Transit, Mode::Train] {
            let t = plan(mode, from, to, start, 300).unwrap_or_else(|e| panic!("{mode:?}: {e}"));
            eprintln!("{mode:?}: {} leave {:?}", tr::fmt_secs(t.secs), t.leave_ms);
            assert!(t.secs > 0);
        }
    }

    /// The way by train from the university in Pisa to a home near Pietrasanta: `cargo test -p open-island live_by_train -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs the network"]
    fn live_by_train() {
        let (polo, home) = ((43.7208, 10.4072), (43.964268, 10.221497));
        let (s1, s2) = (rail_stop_near(polo).unwrap(), rail_stop_near(home).unwrap());
        eprintln!("nearest to the polo: {} ({:.0} m); nearest to home: {} ({:.0} m)", s1.name, s1.metres, s2.name, s2.metres);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
        let (trip, segs) = route_now(Mode::Train, polo, home, now).expect("a way by train");
        eprintln!("by train: {} in {} segments ({:?}), transfers {:?}", tr::fmt_secs(trip.secs), segs.len(), segs.iter().map(|s| if s.walk { "walk" } else { "train" }).collect::<Vec<_>>(), trip.transfers);
        assert_eq!(segs.iter().map(|s| s.walk).collect::<Vec<_>>(), [true, false, true], "on foot, by train, on foot");
        assert!(trip.secs > 40 * 60, "{}", trip.secs);
        let t = plan(Mode::Train, polo, home, now + 4 * 3_600_000, 300).expect("a trip by train");
        eprintln!("to arrive in 4 h: leave {:?}, {}", t.leave_ms.map(tr::iso_utc), tr::fmt_secs(t.secs));
        assert!(t.leave_ms.unwrap() < now + 4 * 3_600_000);
    }
}

