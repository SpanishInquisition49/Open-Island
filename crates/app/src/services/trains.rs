//! The usual train: pin it once (`train pisa centrale 17:12 verso lucca`), and the island follows it every day through
//! Trenitalia's ViaggiaTreno service: its delay, its track, whether it runs. Home (coordinates by hand, or an address looked up
//! once) is where the destination station comes from when none is named.
//!
//! ViaggiaTreno is the service behind the railway's own departure boards. It is not a documented API and it turns away
//! clients that do not say who they are, so every request carries the island's name; it is asked once a minute, and only
//! in the hour and a half before the pinned train leaves.

use super::Reporter;
use chrono::{TimeZone, Timelike};
use island_core::train::{self, TrainPin};
use island_ui::model::{Command, Incoming, Input};
use std::sync::Mutex;
use std::time::Duration;

const VT: &str = "http://www.viaggiatreno.it/infomobilita/resteasy/viaggiatreno";
const TRANSITOUS: &str = "https://api.transitous.org/api/v1";
/// The board is read from this long before the train leaves until a few minutes after.
const WINDOW_BEFORE_MS: i64 = 90 * 60_000;
const WINDOW_AFTER_MS: i64 = 10 * 60_000;

static PIN: Mutex<Option<TrainPin>> = Mutex::new(None);

fn pin_file() -> Option<std::path::PathBuf> {
    super::state_file("pinned-train.json")
}

fn load_pin() -> Option<TrainPin> {
    TrainPin::from_json(&serde_json::from_str(&std::fs::read_to_string(pin_file()?).ok()?).ok()?)
}

fn save_pin(pin: Option<&TrainPin>) {
    let Some(p) = pin_file() else { return };
    match pin {
        Some(t) => drop(std::fs::write(p, t.to_json().to_string())),
        None => drop(std::fs::remove_file(p)),
    }
}

fn urlenc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// A moment as ViaggiaTreno writes it in its URLs: `Sat Nov 21 2026 10:11:43 GMT+0100` (Italian time).
pub fn vt_date(ms: i64) -> String {
    let t = chrono_tz::Europe::Rome.timestamp_millis_opt(ms).single().unwrap_or_else(|| chrono_tz::Europe::Rome.timestamp_opt(0, 0).unwrap());
    t.format("%a %b %d %Y %H:%M:%S GMT%z").to_string()
}

fn get_text(url: &str) -> Result<String, String> {
    super::net::agent().get(url).call().map_err(|e| e.to_string()).and_then(|mut r| r.body_mut().read_to_string().map_err(|e| e.to_string()))
}

fn get_json(url: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(&get_text(url)?).map_err(|e| format!("unexpected answer: {e}"))
}

fn stations_named(text: &str) -> Result<Vec<(String, String)>, String> {
    Ok(train::parse_stations(&get_text(&format!("{VT}/autocompletaStazione/{}", urlenc(text.trim())))?))
}

fn board(station_id: &str, at_ms: i64) -> Result<Vec<train::Departure>, String> {
    Ok(train::parse_board(&get_json(&format!("{VT}/partenze/{station_id}/{}", urlenc(&vt_date(at_ms))))?))
}

/// The railway station nearest to a place (a few kilometres around it are looked at): (name, id), like a name search gives.
fn station_near(lat: f64, lon: f64) -> Option<(String, String)> {
    let d = 0.06;
    let url = format!("{TRANSITOUS}/map/stops?min={},{}&max={},{}", lat - d, lon - d, lat + d, lon + d);
    let stops = get_json(&url).ok()?;
    train::stations_near(&stops, (lat, lon)).into_iter().next().map(|(id, name, _)| (name, id))
}

/// Local midnight-based moment of `HH:MM` on the day of `now_ms` (Italian time).
fn at_clock(now_ms: i64, hhmm: &str) -> Option<i64> {
    let (h, m) = hhmm.split_once(':')?;
    let local = chrono_tz::Europe::Rome.timestamp_millis_opt(now_ms).single()?;
    let t = local.with_hour(h.parse().ok()?)?.with_minute(m.parse().ok()?)?.with_second(0)?.with_nanosecond(0)?;
    Some(t.timestamp_millis())
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn clock_of(ms: i64) -> String {
    chrono_tz::Europe::Rome.timestamp_millis_opt(ms).single().map(|t| t.format("%H:%M").to_string()).unwrap_or_default()
}

/// Finds the train a `train ...` line means, and what it needs to be followed.
pub fn resolve(q: &train::PinQuery, home: Option<(f64, f64, String)>) -> Result<TrainPin, String> {
    let from = train::best_station(&stations_named(&q.station)?, &q.station).ok_or_else(|| format!("No station called {}", q.station))?;
    // where it goes: the one named, else the station near home
    let to = match &q.to {
        Some(name) => Some(train::best_station(&stations_named(name)?, name).ok_or_else(|| format!("No station called {name}"))?),
        None => home.and_then(|(la, lo, _)| station_near(la, lo)),
    };
    let at = at_clock(now_ms(), &q.time).ok_or("That time is not valid")?;
    // asked from a couple of minutes before, the train is the first lines of the board
    let candidates: Vec<train::Departure> = board(&from.1, at - 2 * 60_000)?.into_iter().filter(|d| d.time == q.time && d.category != "BUS").collect();
    if candidates.is_empty() {
        return Err(format!("No train at {} from {}", q.time, from.0));
    }
    let mut pick = None;
    for d in candidates.iter().take(4) {
        let run = get_json(&format!("{VT}/andamentoTreno/{}/{}/{}", d.origin_id, d.number, d.run_date_ms)).map(|v| train::parse_run(&v)).unwrap_or_default();
        match &to {
            Some((_, to_id)) => {
                if let Some(arr) = train::arrival_at(&run, &from.1, to_id) {
                    pick = Some((d.clone(), Some(clock_of(arr))));
                    break;
                }
            }
            None => {
                pick = Some((d.clone(), run.last().map(|s| clock_of(s.sched_ms))));
                break;
            }
        }
    }
    let Some((d, arrival)) = pick else { return Err(format!("No train at {} from {} that stops at {}", q.time, from.0, to.map(|t| t.0).unwrap_or_default())) };
    Ok(TrainPin {
        from_id: from.1,
        from_name: title_case(&from.0),
        time: d.time,
        number: d.number,
        category: d.category,
        to_name: title_case(&to.as_ref().map_or(d.destination.clone(), |t| t.0.clone())),
        to_id: to.map(|t| t.1).unwrap_or_default(),
        arrival: arrival.unwrap_or_default(),
        origin_id: d.origin_id,
        headsign: d.destination,
    })
}

/// `PISA CENTRALE` -> `Pisa Centrale`.
pub fn title_case(s: &str) -> String {
    s.split(' ').map(|w| w.split('/').map(cap).collect::<Vec<_>>().join("/")).collect::<Vec<_>>().join(" ")
}

fn cap(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) if w.len() > 2 || !w.contains('.') => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn say(rep: &Reporter, summary: &str, body: &str) {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(3_000_000);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    rep.input(Input::Notification(Incoming { id, app: "open-island".into(), summary: summary.into(), body: body.into(), expire_ms: Some(6000), ..Default::default() }));
}

/// An address or coordinates for home; nothing clears it. Coordinates need no network.
fn set_home(rep: &Reporter, text: &str) {
    if text.trim().is_empty() {
        let _ = crate::config::update(|o| {
            for k in ["home_latitude", "home_longitude", "home_name"] {
                o.remove(k);
            }
        });
        super::signal::bump(&super::CONFIG_GEN);
        rep.patch(|d| {
            d.config.home = String::new();
            d.config.home_pos = None;
        });
        return say(rep, "Home removed", "");
    }
    let found = match train::parse_coordinates(text) {
        Some((la, lo)) => Ok((la, lo, String::new())),
        None => geocode(text),
    };
    match found {
        Ok((la, lo, name)) => {
            let label = crate::config::home_label(&Some((la, lo, name.clone())));
            match crate::config::update(|o| {
                o.insert("home_latitude".into(), serde_json::json!(la));
                o.insert("home_longitude".into(), serde_json::json!(lo));
                o.insert("home_name".into(), serde_json::json!(name));
            }) {
                Ok(()) => {
                    super::signal::bump(&super::CONFIG_GEN);
                    let shown = label.clone();
                    rep.patch(move |d| {
                        d.config.home = shown;
                        d.config.home_pos = Some((la, lo));
                    });
                    say(rep, "Home set", &label);
                }
                Err(e) => say(rep, "Could not save home", &e),
            }
        }
        Err(e) => say(rep, "Home not set", &e),
    }
}

/// An address to coordinates, once (Transitous' geocoder, which is OpenStreetMap's data).
fn geocode(address: &str) -> Result<(f64, f64, String), String> {
    let v = get_json(&format!("{TRANSITOUS}/geocode?text={}&language=it", urlenc(address)))?;
    let first = v.as_array().and_then(|a| a.iter().find(|p| p.get("type").and_then(|t| t.as_str()) != Some("STOP")).or_else(|| a.first())).ok_or("No such place")?;
    let (lat, lon) = (first.get("lat").and_then(|x| x.as_f64()).ok_or("No such place")?, first.get("lon").and_then(|x| x.as_f64()).ok_or("No such place")?);
    Ok((lat, lon, first.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string()))
}

pub fn command(rep: &Reporter, c: &Command) {
    match c {
        Command::PinTrain(q) => {
            let (rep, q) = (rep.clone(), q.clone());
            std::thread::spawn(move || {
                let Some(query) = train::parse_pin_query(&q) else { return say(&rep, "Train not pinned", "Say the station and the time: train pisa centrale 17:12") };
                match resolve(&query, crate::config::load().home) {
                    Ok(pin) => {
                        save_pin(Some(&pin));
                        *PIN.lock().unwrap() = Some(pin.clone());
                        let line = format!("{} · {} {} → {}{}", pin.label(), pin.from_name, pin.time, pin.to_name, if pin.arrival.is_empty() { String::new() } else { format!(" {}", pin.arrival) });
                        let patched = pin.clone();
                        rep.patch(move |d| {
                            d.train = Some(patched);
                            d.train_status = None;
                        });
                        say(&rep, "Train pinned", &line);
                        super::signal::bump(&super::CONFIG_GEN);
                    }
                    Err(e) => say(&rep, "Train not pinned", &e),
                }
            });
        }
        Command::UnpinTrain => {
            save_pin(None);
            *PIN.lock().unwrap() = None;
            rep.patch(|d| {
                d.train = None;
                d.train_status = None;
            });
        }
        Command::SetHome(text) => {
            let (rep, text) = (rep.clone(), text.clone());
            std::thread::spawn(move || set_home(&rep, &text));
        }
        _ => {}
    }
}

pub fn start(rep: Reporter) {
    if let Some(pin) = load_pin() {
        *PIN.lock().unwrap() = Some(pin.clone());
        rep.patch(move |d| d.train = Some(pin));
    }
    std::thread::spawn(move || {
        let mut last: Option<Option<train::TrainStatus>> = None;
        loop {
            let pin = PIN.lock().unwrap().clone();
            let now = now_ms();
            let status = pin.as_ref().and_then(|p| {
                let leaves = at_clock(now, &p.time)?;
                if now < leaves - WINDOW_BEFORE_MS || now > leaves + WINDOW_AFTER_MS {
                    return None;
                }
                match board(&p.from_id, leaves - 2 * 60_000) {
                    Ok(b) => {
                        // a train pinned before the board's destination was kept learns it now (the alternatives need it)
                        let mut pin = p.clone();
                        if pin.headsign.is_empty() {
                            if let Some(d) = b.iter().find(|d| d.number == pin.number) {
                                pin.headsign = d.destination.clone();
                                save_pin(Some(&pin));
                                *PIN.lock().unwrap() = Some(pin.clone());
                            }
                        }
                        train::status_from_board(&b, &pin, now)
                    }
                    Err(e) => {
                        log::debug!("train: {e}");
                        None
                    }
                }
            });
            // what is already on screen stays when the board could not be read (a tunnel is no news)
            let keep = pin.is_some() && status.is_none() && last.as_ref().is_some_and(|l| l.is_some()) && {
                let leaves = pin.as_ref().and_then(|p| at_clock(now, &p.time)).unwrap_or(0);
                now <= leaves + WINDOW_AFTER_MS
            };
            if !keep && last.as_ref() != Some(&status) {
                last = Some(status.clone());
                rep.patch(move |d| d.train_status = status);
            }
            std::thread::sleep(Duration::from_secs(60));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_date_is_written_as_viaggiatreno_wants_it() {
        // 2026-11-21 09:11:43 UTC is 10:11:43 in Italy (CET)
        assert_eq!(vt_date(1_795_252_303_000), "Sat Nov 21 2026 10:11:43 GMT+0100");
        // in summer CEST is +0200
        assert_eq!(vt_date(1_782_043_200_000), "Sun Jun 21 2026 14:00:00 GMT+0200");
    }

    #[test]
    fn a_clock_time_is_on_the_day_in_italy() {
        let noon = 1_795_252_303_000; // Saturday 21 November 2026, 10:11 in Italy
        assert_eq!(at_clock(noon, "17:12").map(clock_of).as_deref(), Some("17:12"));
        assert_eq!(at_clock(noon, "17:12").map(|t| t - noon), Some((17 * 3600 + 12 * 60 - (10 * 3600 + 11 * 60 + 43)) * 1000));
        assert_eq!(at_clock(noon, "25:00"), None);
        assert_eq!(at_clock(noon, "banana"), None);
    }

    #[test]
    fn names_of_stations_lose_their_shouting() {
        assert_eq!(title_case("PISA CENTRALE"), "Pisa Centrale");
        assert_eq!(title_case("PISA/BINARI PISA S.ROSSORE"), "Pisa/Binari Pisa S.rossore");
        assert_eq!(title_case("FIRENZE SANTA MARIA NOVELLA"), "Firenze Santa Maria Novella");
    }

    /// Asks the real service: `cargo test -p open-island live_train -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_train() {
        let found = stations_named("pisa centrale").unwrap();
        println!("{found:?}");
        assert!(found.iter().any(|s| s.1 == "S06500"));
        let b = board("S06500", now_ms()).unwrap();
        println!("{} departures; first: {:?}", b.len(), b.first());
        assert!(!b.is_empty());
        println!("near home: {:?}", station_near(43.7085, 10.4036));
        // the first train that is not cancelled, pinned with a named destination and with the one near home
        let d = b.iter().find(|d| !d.cancelled && d.category != "BUS" && d.destination.contains("LUCCA")).or_else(|| b.iter().find(|d| !d.cancelled)).unwrap();
        let q = train::PinQuery { station: "pisa centrale".into(), time: d.time.clone(), to: None };
        println!("pin with home: {:?}", resolve(&q, Some((43.8372, 10.5061, "Lucca".into()))));
        println!("pin without: {:?}", resolve(&q, None));
    }
}
