//! Trains (Trenitalia, through ViaggiaTreno): what the usual train of the evening is doing. Pure parsing and wording; the
//! requests are in the platform layer.
//!
//! The data is Trenitalia's own: ViaggiaTreno is the service behind the railway's departure boards and train status pages.
//! It is not a documented API, so everything here is read defensively and a field that is missing is simply unknown.

use serde_json::{json, Value};

/// One line of a station's departure board.
#[derive(Clone, Debug, PartialEq)]
pub struct Departure {
    pub number: String,
    /// REG, RV, IC, FR...
    pub category: String,
    pub destination: String,
    /// The station the train starts from (`S06500`): with the number and the date it names the run.
    pub origin_id: String,
    /// `17:12`, as the board writes it.
    pub time: String,
    pub depart_ms: i64,
    /// The day of the run, as the status request wants it.
    pub run_date_ms: String,
    pub delay_min: Option<i32>,
    pub cancelled: bool,
    pub platform: Option<String>,
    pub platform_planned: Option<String>,
}

fn text(v: &Value, k: &str) -> Option<String> {
    match v.get(k)? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// `autocompletaStazione`: one `NAME|S06500` per line.
pub fn parse_stations(body: &str) -> Vec<(String, String)> {
    body.lines().filter_map(|l| l.split_once('|')).map(|(n, id)| (n.trim().to_string(), id.trim().to_string())).filter(|(n, id)| !n.is_empty() && id.starts_with('S')).collect()
}

/// The best of the stations a name search found: an exact name first, else the first one.
pub fn best_station(found: &[(String, String)], wanted: &str) -> Option<(String, String)> {
    let norm = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
    let w = norm(wanted);
    found.iter().find(|(n, _)| norm(n) == w).or_else(|| found.iter().find(|(n, _)| norm(n).starts_with(&w))).or_else(|| found.first()).cloned()
}

/// `partenze`: the departure board.
pub fn parse_board(v: &Value) -> Vec<Departure> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    Some(Departure {
                        number: text(t, "numeroTreno")?,
                        category: text(t, "categoria").unwrap_or_default(),
                        destination: text(t, "destinazione").unwrap_or_default(),
                        origin_id: text(t, "codOrigine").unwrap_or_default(),
                        time: text(t, "compOrarioPartenza")?,
                        depart_ms: t.get("orarioPartenza").and_then(Value::as_i64).unwrap_or(0),
                        run_date_ms: text(t, "millisDataPartenza").unwrap_or_default(),
                        delay_min: t.get("ritardo").and_then(Value::as_i64).map(|d| d as i32),
                        // 0 runs, 1 cancelled, 2 cancelled on part of the way
                        cancelled: t.get("provvedimento").and_then(Value::as_i64) == Some(1),
                        platform: text(t, "binarioEffettivoPartenzaDescrizione"),
                        platform_planned: text(t, "binarioProgrammatoPartenzaDescrizione"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A stop of a run.
#[derive(Clone, Debug, PartialEq)]
pub struct Stop {
    pub id: String,
    pub name: String,
    /// The scheduled time at the stop.
    pub sched_ms: i64,
}

/// `andamentoTreno`: the stops of one run, in order.
pub fn parse_run(v: &Value) -> Vec<Stop> {
    v.get("fermate")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|f| Some(Stop { id: text(f, "id")?, name: text(f, "stazione").unwrap_or_default(), sched_ms: f.get("programmata").and_then(Value::as_i64)? })).collect())
        .unwrap_or_default()
}

/// Does the run call at `to` after `from`, and when (scheduled)?
pub fn arrival_at(stops: &[Stop], from: &str, to: &str) -> Option<i64> {
    let at = stops.iter().position(|s| s.id == from)?;
    stops[at + 1..].iter().find(|s| s.id == to).map(|s| s.sched_ms)
}

/// The usual train, pinned: where it is taken, when, and where it goes.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TrainPin {
    pub from_id: String,
    pub from_name: String,
    /// `17:12`
    pub time: String,
    pub number: String,
    pub category: String,
    pub to_name: String,
    pub to_id: String,
    /// When it gets to `to_name`, `17:52`, when that was found.
    pub arrival: String,
    pub origin_id: String,
    /// Where the train is going, as the board writes it (`LUCCA`): other trains to the same place are its alternatives.
    pub headsign: String,
}

impl TrainPin {
    pub fn to_json(&self) -> Value {
        json!({"from_id": self.from_id, "from_name": self.from_name, "time": self.time, "number": self.number, "category": self.category, "to_name": self.to_name, "to_id": self.to_id, "arrival": self.arrival, "origin_id": self.origin_id, "headsign": self.headsign})
    }

    pub fn from_json(v: &Value) -> Option<TrainPin> {
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let pin = TrainPin { from_id: s("from_id"), from_name: s("from_name"), time: s("time"), number: s("number"), category: s("category"), to_name: s("to_name"), to_id: s("to_id"), arrival: s("arrival"), origin_id: s("origin_id"), headsign: s("headsign") };
        (!pin.from_id.is_empty() && !pin.number.is_empty() && !pin.time.is_empty()).then_some(pin)
    }

    /// `REG 18567`
    pub fn label(&self) -> String {
        if self.category.is_empty() { self.number.clone() } else { format!("{} {}", self.category, self.number) }
    }
}

/// What the board says about the pinned train right now.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TrainStatus {
    pub delay_min: Option<i32>,
    pub cancelled: bool,
    pub platform: Option<String>,
    /// When it was read (epoch ms).
    pub at_ms: i64,
    /// The trains after it to the same place, for when it is late or cancelled.
    pub next: Vec<NextTrain>,
}

/// An alternative: when it leaves, what it is, how late.
#[derive(Clone, Debug, PartialEq)]
pub struct NextTrain {
    pub time: String,
    pub label: String,
    pub delay_min: Option<i32>,
}

/// The pinned train among a board's departures.
pub fn status_from_board(board: &[Departure], pin: &TrainPin, at_ms: i64) -> Option<TrainStatus> {
    let d = board.iter().find(|d| d.number == pin.number && d.time == pin.time).or_else(|| board.iter().find(|d| d.number == pin.number))?;
    Some(TrainStatus { delay_min: d.delay_min, cancelled: d.cancelled, platform: d.platform.clone().or_else(|| d.platform_planned.clone()), at_ms, next: next_trains(board, pin, 3) })
}

/// The trains that leave after the pinned one for the same place and are not cancelled, soonest first.
pub fn next_trains(board: &[Departure], pin: &TrainPin, max: usize) -> Vec<NextTrain> {
    if pin.headsign.is_empty() {
        return vec![];
    }
    let mut after: Vec<&Departure> = board.iter().filter(|d| d.number != pin.number && !d.cancelled && d.category != "BUS" && d.destination.eq_ignore_ascii_case(&pin.headsign) && d.time.as_str() > pin.time.as_str()).collect();
    after.sort_by(|a, b| a.time.cmp(&b.time));
    after.into_iter().take(max).map(|d| NextTrain { time: d.time.clone(), label: if d.category.is_empty() { d.number.clone() } else { format!("{} {}", d.category, d.number) }, delay_min: d.delay_min }).collect()
}

/// "17:42 REG 4177 +5 · 18:12 RV 4181": the alternatives on one line.
pub fn next_line(next: &[NextTrain]) -> String {
    next.iter().take(2).map(|n| match n.delay_min {
        Some(d) if d >= 3 => format!("{} {} +{d}", n.time, n.label),
        _ => format!("{} {}", n.time, n.label),
    }).collect::<Vec<_>>().join(" · ")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    /// Nothing known yet (the board only shows it shortly before it leaves)
    Quiet,
    Good,
    Late,
    Bad,
}

/// The words for the right side of the Today row, and how to colour them.
pub fn status_line(status: Option<&TrainStatus>) -> (String, Mood) {
    let Some(s) = status else { return (String::new(), Mood::Quiet) };
    let plat = s.platform.as_deref().map(|p| format!(" · track {p}")).unwrap_or_default();
    if s.cancelled {
        return ("Cancelled".to_string(), Mood::Bad);
    }
    match s.delay_min {
        Some(d) if d >= 10 => (format!("+{d} min{plat}"), Mood::Bad),
        Some(d) if d >= 3 => (format!("+{d} min{plat}"), Mood::Late),
        Some(_) => (format!("On time{plat}"), Mood::Good),
        None if s.platform.is_some() => (format!("Track {}", s.platform.as_deref().unwrap_or("")), Mood::Quiet),
        None => (String::new(), Mood::Quiet),
    }
}

/// Whether the status is worth a card, and a key that changes when the news does (so that it is said once).
pub fn alert(status: Option<&TrainStatus>) -> Option<(String, String)> {
    let s = status?;
    if s.cancelled {
        return Some(("cancelled".into(), "Cancelled".into()));
    }
    let d = s.delay_min.filter(|d| *d >= 10)?;
    // news again each time it is five minutes later
    Some((format!("late-{}", d / 5), format!("{d} min late")))
}

// ---- stations near a place --------------------------------------------------------------------------------

/// Transitous names Trenitalia's stations `it-trenitalia_06500`, ViaggiaTreno `S06500`.
pub fn viaggiatreno_id(transitous_id: &str) -> Option<String> {
    let n = transitous_id.strip_prefix("it-trenitalia_")?.split(':').next()?;
    (!n.is_empty() && n.chars().all(|c| c.is_ascii_digit())).then(|| format!("S{n}"))
}

fn metres(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dlat, dlon) = ((b.0 - a.0).to_radians(), (b.1 - a.1).to_radians());
    let h = (dlat / 2.0).sin().powi(2) + a.0.to_radians().cos() * b.0.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    2.0 * 6_371_000.0 * h.sqrt().asin()
}

/// The railway stations in a Transitous stop list (`/map/stops`), nearest first: (ViaggiaTreno id, name, metres).
pub fn stations_near(stops: &Value, at: (f64, f64)) -> Vec<(String, String, f64)> {
    let mut out: Vec<(String, String, f64)> = stops
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|s| s.get("modes").and_then(Value::as_array).is_some_and(|m| m.iter().filter_map(Value::as_str).any(|m| m.contains("RAIL"))))
                .filter_map(|s| {
                    let id = viaggiatreno_id(s.get("stopId")?.as_str()?)?;
                    let (lat, lon) = (s.get("lat")?.as_f64()?, s.get("lon")?.as_f64()?);
                    Some((id, s.get("name").and_then(Value::as_str).unwrap_or("").to_string(), metres(at, (lat, lon))))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.2.total_cmp(&b.2));
    out
}

/// A place given as `43.7085, 10.4036` (or with a `;`, or only a space): no address, no network.
pub fn parse_coordinates(s: &str) -> Option<(f64, f64)> {
    let parts: Vec<&str> = s.split([',', ';']).map(str::trim).filter(|p| !p.is_empty()).collect();
    let nums: Vec<f64> = match parts.as_slice() {
        [one] => one.split_whitespace().filter_map(|p| p.parse().ok()).collect(),
        two => two.iter().filter_map(|p| p.parse().ok()).collect(),
    };
    match nums.as_slice() {
        [lat, lon] if (-90.0..=90.0).contains(lat) && (-180.0..=180.0).contains(lon) => Some((*lat, *lon)),
        _ => None,
    }
}

/// What a `train` line in the bar asks for: the station words, the time, and a destination if one was named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinQuery {
    pub station: String,
    pub time: String,
    pub to: Option<String>,
}

/// `pisa centrale 17:12`, `da pisa c.le 17.12 verso lucca`, `from pisa centrale at 17:12 to lucca`.
pub fn parse_pin_query(q: &str) -> Option<PinQuery> {
    let words: Vec<&str> = q.split_whitespace().collect();
    let time_at = words.iter().position(|w| clock(w).is_some())?;
    let time = clock(words[time_at])?;
    let is_to = |w: &&str| matches!(w.to_lowercase().as_str(), "to" | "verso" | "per" | "->" | "a");
    let strip = |ws: &[&str]| -> String { ws.iter().filter(|w| !matches!(w.to_lowercase().as_str(), "da" | "from" | "at" | "alle" | "ore")).copied().collect::<Vec<_>>().join(" ") };
    let (before, after) = (&words[..time_at], &words[time_at + 1..]);
    // "pisa centrale 17:12 verso lucca" or "17:12 pisa centrale verso lucca"
    let (station_words, to_words): (Vec<&str>, Vec<&str>) = match (before.iter().position(is_to), after.iter().position(is_to)) {
        (Some(i), _) => (before[..i].to_vec(), [&before[i + 1..], after].concat()),
        (None, Some(j)) => ([before, &after[..j]].concat(), after[j + 1..].to_vec()),
        (None, None) => ([before, after].concat(), vec![]),
    };
    let station = strip(&station_words);
    let to = strip(&to_words);
    (!station.is_empty()).then_some(PinQuery { station, time, to: (!to.is_empty()).then_some(to) })
}

/// `17:12`, `7.05` -> `17:12`, `07:05`.
fn clock(w: &str) -> Option<String> {
    let (h, m) = w.split_once([':', '.'])?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60 && w.len() <= 5).then(|| format!("{h:02}:{m:02}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> Value {
        json!([
            {"numeroTreno": 18286, "categoria": "REG", "destinazione": "FIRENZE SANTA MARIA NOVELLA", "codOrigine": "S06500", "compOrarioPartenza": "10:01", "orarioPartenza": 1000, "ritardo": null, "provvedimento": 0, "binarioEffettivoPartenzaDescrizione": null, "binarioProgrammatoPartenzaDescrizione": null, "millisDataPartenza": "1795215600000"},
            {"numeroTreno": 18567, "categoria": "REG", "destinazione": "LUCCA", "codOrigine": "S06500", "compOrarioPartenza": "10:20", "orarioPartenza": 2000, "ritardo": 7, "provvedimento": 0, "binarioEffettivoPartenzaDescrizione": "2", "binarioProgrammatoPartenzaDescrizione": "3", "millisDataPartenza": "1795215600000"},
            {"numeroTreno": 651, "categoria": "IC", "destinazione": "LIVORNO CENTRALE", "codOrigine": "S01700", "compOrarioPartenza": "10:23", "orarioPartenza": 3000, "ritardo": 0, "provvedimento": 1, "binarioProgrammatoPartenzaDescrizione": "4", "millisDataPartenza": "1795215600000"},
            {"destinazione": "no number or time"}
        ])
    }

    #[test]
    fn the_board_is_read_and_what_is_missing_is_unknown() {
        let b = parse_board(&board());
        assert_eq!(b.len(), 3, "a line with no number and no time is skipped");
        assert_eq!((b[0].delay_min, b[0].platform.clone(), b[0].cancelled), (None, None, false));
        assert_eq!((b[1].delay_min, b[1].platform.as_deref(), b[1].platform_planned.as_deref(), b[1].origin_id.as_str()), (Some(7), Some("2"), Some("3"), "S06500"));
        assert!(b[2].cancelled);
        assert_eq!(b[1].run_date_ms, "1795215600000");
        assert!(parse_board(&json!({"not": "a list"})).is_empty());
    }

    #[test]
    fn stations_are_found_by_name() {
        let found = parse_stations("PISA BINARI AEROPORTO|S06450\nPISA CENTRALE|S06500\nPISA/BINARI PISA S.ROSSORE|S06501\nbroken line\n");
        assert_eq!(found.len(), 3);
        assert_eq!(best_station(&found, "pisa centrale"), Some(("PISA CENTRALE".into(), "S06500".into())));
        assert_eq!(best_station(&found, "Pisa C").map(|s| s.1), Some("S06500".into()), "a name that starts like the word");
        assert_eq!(best_station(&found, "aeroporto").map(|s| s.1), Some("S06450".into()), "else the first found");
        assert_eq!(best_station(&[], "x"), None);
    }

    #[test]
    fn the_pinned_train_is_found_on_the_board_with_its_delay_and_track() {
        let pin = TrainPin { from_id: "S06500".into(), number: "18567".into(), time: "10:20".into(), category: "REG".into(), ..Default::default() };
        let s = status_from_board(&parse_board(&board()), &pin, 99).unwrap();
        assert_eq!((s.delay_min, s.cancelled, s.platform.as_deref(), s.at_ms), (Some(7), false, Some("2"), 99), "the track in force, not the planned one");
        let other = TrainPin { number: "1".into(), ..pin.clone() };
        assert_eq!(status_from_board(&parse_board(&board()), &other, 0), None, "not on the board: not known");
        let canc = TrainPin { number: "651".into(), time: "10:23".into(), ..pin };
        let s = status_from_board(&parse_board(&board()), &canc, 0).unwrap();
        assert!(s.cancelled && s.platform.as_deref() == Some("4"), "the planned track when no other");
    }

    #[test]
    fn when_the_train_is_in_trouble_the_next_ones_to_the_same_place_are_known() {
        let pin = TrainPin { from_id: "S06500".into(), number: "18567".into(), time: "10:20".into(), category: "REG".into(), headsign: "LUCCA".into(), ..Default::default() };
        let board = parse_board(&json!([
            {"numeroTreno": 18567, "categoria": "REG", "destinazione": "LUCCA", "compOrarioPartenza": "10:20", "ritardo": 12, "provvedimento": 0},
            {"numeroTreno": 4019, "categoria": "RV", "destinazione": "LIVORNO CENTRALE", "compOrarioPartenza": "10:32", "provvedimento": 0},
            {"numeroTreno": 4177, "categoria": "REG", "destinazione": "Lucca", "compOrarioPartenza": "10:50", "ritardo": 5, "provvedimento": 0},
            {"numeroTreno": 4178, "categoria": "REG", "destinazione": "LUCCA", "compOrarioPartenza": "10:40", "provvedimento": 1},
            {"numeroTreno": 4181, "categoria": "RV", "destinazione": "LUCCA", "compOrarioPartenza": "11:20", "ritardo": 0, "provvedimento": 0},
            {"numeroTreno": 4155, "categoria": "REG", "destinazione": "LUCCA", "compOrarioPartenza": "10:05", "provvedimento": 0},
            {"numeroTreno": 9, "categoria": "BUS", "destinazione": "LUCCA", "compOrarioPartenza": "10:30", "provvedimento": 0}
        ]));
        let st = status_from_board(&board, &pin, 0).unwrap();
        assert_eq!(st.next.iter().map(|n| n.label.as_str()).collect::<Vec<_>>(), ["REG 4177", "RV 4181"], "later, same destination, running, no bus; the cancelled one and the earlier one are out");
        assert_eq!(next_line(&st.next), "10:50 REG 4177 +5 · 11:20 RV 4181");
        assert!(next_trains(&board, &TrainPin { headsign: String::new(), ..pin }, 3).is_empty(), "no destination known: no alternatives");
    }

    #[test]
    fn the_status_has_words_a_mood_and_an_alert_that_comes_once_per_change() {
        let st = |d: Option<i32>, c: bool, p: Option<&str>| TrainStatus { delay_min: d, cancelled: c, platform: p.map(str::to_string), at_ms: 0, next: vec![] };
        assert_eq!(status_line(None), (String::new(), Mood::Quiet));
        assert_eq!(status_line(Some(&st(Some(0), false, Some("3")))), ("On time · track 3".to_string(), Mood::Good));
        assert_eq!(status_line(Some(&st(Some(4), false, None))), ("+4 min".to_string(), Mood::Late));
        assert_eq!(status_line(Some(&st(Some(12), false, Some("1")))), ("+12 min · track 1".to_string(), Mood::Bad));
        assert_eq!(status_line(Some(&st(None, true, None))), ("Cancelled".to_string(), Mood::Bad));
        assert_eq!(status_line(Some(&st(None, false, Some("7")))), ("Track 7".to_string(), Mood::Quiet));
        assert_eq!(alert(Some(&st(Some(9), false, None))), None);
        assert_eq!(alert(Some(&st(Some(12), false, None))), Some(("late-2".into(), "12 min late".into())));
        assert_eq!(alert(Some(&st(Some(13), false, None))).map(|a| a.0), Some("late-2".into()), "the same news");
        assert_eq!(alert(Some(&st(Some(15), false, None))).map(|a| a.0), Some("late-3".into()), "five minutes more is news");
        assert_eq!(alert(Some(&st(Some(30), true, None))).map(|a| a.0), Some("cancelled".into()));
        assert_eq!(alert(None), None);
    }

    #[test]
    fn a_run_says_when_it_gets_where() {
        let run = json!({"fermate": [{"id": "S06500", "stazione": "PISA CENTRALE", "programmata": 100}, {"id": "S06400", "stazione": "S.GIULIANO TERME", "programmata": 200}, {"id": "S06404", "stazione": "LUCCA", "programmata": 300}]});
        let stops = parse_run(&run);
        assert_eq!(arrival_at(&stops, "S06500", "S06404"), Some(300));
        assert_eq!(arrival_at(&stops, "S06404", "S06500"), None, "backwards is not a trip");
        assert_eq!(arrival_at(&stops, "S06500", "S99999"), None);
        assert!(parse_run(&json!({})).is_empty());
    }

    #[test]
    fn a_pin_survives_the_disk() {
        let pin = TrainPin { from_id: "S06500".into(), from_name: "PISA CENTRALE".into(), time: "17:12".into(), number: "18567".into(), category: "REG".into(), to_name: "LUCCA".into(), to_id: "S06404".into(), arrival: "17:52".into(), origin_id: "S06500".into(), headsign: "LUCCA".into() };
        assert_eq!(TrainPin::from_json(&pin.to_json()), Some(pin.clone()));
        assert_eq!(pin.label(), "REG 18567");
        assert_eq!(TrainPin::from_json(&json!({"number": "1"})), None);
    }

    #[test]
    fn the_stations_near_home_are_the_railway_ones_nearest_first() {
        let stops = json!([
            {"stopId": "it-Toscana-AT_137923", "name": "CORRIDONI 1", "modes": ["BUS"], "lat": 43.7085, "lon": 10.4036},
            {"stopId": "it-trenitalia_06501", "name": "PISA SAN ROSSORE", "modes": ["REGIONAL_RAIL"], "lat": 43.7237, "lon": 10.3874},
            {"stopId": "it-trenitalia_06500", "name": "PISA CENTRALE", "modes": ["HIGHSPEED_RAIL", "REGIONAL_RAIL", "BUS"], "lat": 43.7079, "lon": 10.3982},
            {"stopId": "it-trenitalia_06500:station", "name": "dup", "modes": ["BUS"], "lat": 43.7, "lon": 10.4}
        ]);
        let near = stations_near(&stops, (43.7085, 10.4036));
        assert_eq!(near.iter().map(|n| n.0.as_str()).collect::<Vec<_>>(), ["S06500", "S06501"]);
        assert!(near[0].2 < 600.0 && near[1].2 > 1500.0, "{near:?}");
        assert_eq!(viaggiatreno_id("it-trenitalia_06500:station"), Some("S06500".into()));
        assert_eq!(viaggiatreno_id("it-Toscana-AT_1"), None);
    }

    #[test]
    fn coordinates_are_read_without_a_network() {
        assert_eq!(parse_coordinates("43.7085, 10.4036"), Some((43.7085, 10.4036)));
        assert_eq!(parse_coordinates("43.7085 10.4036"), Some((43.7085, 10.4036)));
        assert_eq!(parse_coordinates("43.7085;10.4036"), Some((43.7085, 10.4036)));
        assert_eq!(parse_coordinates("-33.9, 151.2"), Some((-33.9, 151.2)));
        assert_eq!(parse_coordinates("95, 10"), None);
        assert_eq!(parse_coordinates("Via Roma 1, Pisa"), None, "an address needs a geocoder");
        assert_eq!(parse_coordinates("43.7"), None);
    }

    #[test]
    fn a_train_line_says_where_and_when() {
        let q = |s: &str| parse_pin_query(s);
        assert_eq!(q("pisa centrale 17:12"), Some(PinQuery { station: "pisa centrale".into(), time: "17:12".into(), to: None }));
        assert_eq!(q("da pisa centrale 17.12 verso lucca"), Some(PinQuery { station: "pisa centrale".into(), time: "17:12".into(), to: Some("lucca".into()) }));
        assert_eq!(q("from pisa centrale at 7:05 to lucca"), Some(PinQuery { station: "pisa centrale".into(), time: "07:05".into(), to: Some("lucca".into()) }));
        assert_eq!(q("17:12 pisa centrale"), Some(PinQuery { station: "pisa centrale".into(), time: "17:12".into(), to: None }));
        assert_eq!(q("17:12"), None, "no station");
        assert_eq!(q("pisa centrale"), None, "no time");
        assert_eq!(q("pisa 25:00"), None);
    }
}
