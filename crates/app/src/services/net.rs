//! Network services: weather (Open-Meteo, no key), calendar feeds (iCal URLs) and lyrics (LRCLIB).
//! The island never guesses a location and never calls out without settings for it.

use super::Reporter;
use crate::config::{CalendarKind, Settings};
use island_core::calendar::Event;
use island_core::lyrics::{self, Version};
use island_core::weather::Slot;
use island_ui::model::Weather;
use std::time::Duration;

pub fn agent() -> ureq::Agent {
    let b = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).user_agent("open-island");
    #[cfg(not(target_os = "linux"))]
    let b = b.tls_config(ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).build());
    b.build().into()
}

pub fn start(rep: Reporter) {
    // the settings are read again on every round, so a location or calendar added from the menu applies at once
    let weather_rep = rep.clone();
    std::thread::spawn(move || loop {
        let s = crate::config::load();
        match (s.latitude, s.longitude) {
            (Some(lat), Some(lon)) => match fetch_weather(lat, lon) {
                Ok(w) => weather_rep.patch(move |d| d.weather = Some(w)),
                Err(e) => log::warn!("weather: {e}"),
            },
            _ => weather_rep.patch(|d| d.weather = None),
        }
        super::wait_or_config_change(600);
    });
    std::thread::spawn(move || loop {
        let s = crate::config::load();
        let (events, problems) = if s.calendars.is_empty() { (vec![], vec![]) } else { fetch_calendars(&s) };
        rep.patch(move |d| d.events = events);
        if let Some(p) = problems.first() {
            rep.input(island_ui::model::Input::SettingsNote(p.clone()));
        }
        super::wait_or_config_change(900);
    });
}

fn fetch_weather(lat: f64, lon: f64) -> Result<Weather, String> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&current=temperature_2m,apparent_temperature,weather_code,is_day&daily=temperature_2m_max,temperature_2m_min&minutely_15=precipitation&forecast_minutely_15=8&timezone=auto&forecast_days=1"
    );
    let v: serde_json::Value = agent().get(&url).call().map_err(|e| e.to_string())?.body_mut().read_json().map_err(|e| e.to_string())?;
    parse_weather(&v).ok_or_else(|| "unexpected response".to_string())
}

pub fn parse_weather(v: &serde_json::Value) -> Option<Weather> {
    let cur = v.get("current")?;
    let f = |x: &serde_json::Value, k: &str| x.get(k).and_then(|n| n.as_f64());
    let mut slots = vec![];
    if let Some(p) = v.get("minutely_15").and_then(|m| m.get("precipitation")).and_then(|p| p.as_array()) {
        for (i, mm) in p.iter().enumerate() {
            slots.push(Slot { offset_min: i as i32 * 15, mm: mm.as_f64().unwrap_or(0.0) });
        }
    }
    let day = v.get("daily");
    Some(Weather {
        temp_c: f(cur, "temperature_2m")? as f32,
        feels_c: f(cur, "apparent_temperature").unwrap_or(0.0) as f32,
        code: cur.get("weather_code")?.as_i64()? as i32,
        day: cur.get("is_day").and_then(|d| d.as_i64()).unwrap_or(1) == 1,
        high_c: day.and_then(|d| d.get("temperature_2m_max")?.get(0)?.as_f64()).unwrap_or(0.0) as f32,
        low_c: day.and_then(|d| d.get("temperature_2m_min")?.get(0)?.as_f64()).unwrap_or(0.0) as f32,
        rain_slots: slots,
    })
}

/// Reads every calendar source. Returns the events (named and coloured per calendar, sorted) and the problems.
fn fetch_calendars(s: &Settings) -> (Vec<Event>, Vec<String>) {
    use chrono::Offset;
    let off = chrono::Local::now().offset().fix().local_minus_utc();
    let now = chrono::Utc::now().timestamp_millis();
    let (from, to) = (now - 86_400_000, now + 62 * 86_400_000);
    let (mut all, mut problems) = (vec![], vec![]);
    for (i, src) in s.calendars.iter().enumerate() {
        let default_color = crate::config::PALETTE[i % crate::config::PALETTE.len()];
        match &src.kind {
            CalendarKind::Ics { url } => {
                // webcal:// is https:// for calendar apps
                let url = url.replacen("webcal://", "https://", 1).replacen("Webcal://", "https://", 1);
                match agent().get(&url).call().and_then(|mut r| r.body_mut().read_to_string()) {
                    Ok(text) => {
                        let name = if src.name.is_empty() { island_core::ics::calendar_name(&text).unwrap_or_else(|| src.detail()) } else { src.name.clone() };
                        let color = src.color.unwrap_or(default_color);
                        all.extend(island_core::ics::parse(&text, off, from, to).into_iter().map(|e| Event { color: Some(color), calendar: name.clone(), ..e }));
                    }
                    Err(e) => {
                        log::warn!("calendar {url}: {e}");
                        problems.push(format!("{}: {}", if src.name.is_empty() { src.detail() } else { src.name.clone() }, short(&e.to_string())));
                    }
                }
            }
            CalendarKind::CalDav { .. } => match fetch_caldav(src, off, from, to, default_color) {
                Ok(evs) => all.extend(evs),
                Err(e) => {
                    log::warn!("calendar {}: {e}", src.detail());
                    problems.push(format!("{}: {e}", src.detail()));
                }
            },
        }
    }
    all.sort_by_key(|e| e.start_ms);
    (all, problems)
}

fn short(s: &str) -> String {
    s.chars().take(80).collect()
}

/// One DAV request; Err carries a sentence a person can act on. (ureq refuses PROPFIND and REPORT.)
fn dav(method: &str, url: &str, auth: &str, depth: &str, body: &str) -> Result<String, String> {
    let method = attohttpc::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?;
    let resp = attohttpc::RequestBuilder::try_new(method, url)
        .map_err(|e| short(&e.to_string()))?
        .header("Authorization", auth)
        .header("Depth", depth)
        .header("Content-Type", "application/xml; charset=utf-8")
        .header("User-Agent", "open-island")
        .timeout(Duration::from_secs(20))
        .bytes(body.as_bytes().to_vec())
        .send()
        .map_err(|e| short(&e.to_string()))?;
    match resp.status().as_u16() {
        200..=299 => resp.text().map_err(|e| short(&e.to_string())),
        401 | 403 => Err("sign-in failed: check the user name and the app-specific password".into()),
        c => Err(format!("the server answered {c}")),
    }
}

fn caldav_password(password: &str, cmd: &str) -> Result<String, String> {
    if cmd.trim().is_empty() {
        return if password.is_empty() { Err("no password set".into()) } else { Ok(password.to_string()) };
    }
    let out = std::process::Command::new("sh").arg("-c").arg(cmd).stderr(std::process::Stdio::null()).output().map_err(|e| e.to_string())?;
    let pw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() && !pw.is_empty() { Ok(pw) } else { Err("password_cmd printed nothing".into()) }
}

fn fetch_caldav(src: &crate::config::CalendarSource, off: i32, from: i64, to: i64, default_color: (u8, u8, u8)) -> Result<Vec<Event>, String> {
    use super::caldav as cd;
    let CalendarKind::CalDav { server, user, password, password_cmd } = &src.kind else { return Ok(vec![]) };
    let auth = cd::basic_auth(user, &caldav_password(password, password_cmd)?);
    let principal = cd::parse_principal(&dav("PROPFIND", server, &auth, "0", cd::PRINCIPAL_BODY)?).ok_or("no account found on the server")?;
    let principal_url = cd::resolve(server, &principal);
    let home = cd::parse_home(&dav("PROPFIND", &principal_url, &auth, "0", cd::HOME_BODY)?).ok_or("no calendars found on the server")?;
    let home_url = cd::resolve(&principal_url, &home);
    let calendars = cd::parse_calendars(&dav("PROPFIND", &home_url, &auth, "1", cd::LIST_BODY)?);
    let fmt = |ms: i64| chrono::DateTime::from_timestamp_millis(ms).unwrap_or_default().format("%Y%m%dT%H%M%SZ").to_string();
    let body = cd::query_body(&fmt(from), &fmt(to));
    let mut out = vec![];
    for cal in &calendars {
        let xml = match dav("REPORT", &cd::resolve(&home_url, &cal.href), &auth, "1", &body) {
            Ok(x) => x,
            Err(e) => {
                log::warn!("calendar {}: {e}", cal.name);
                continue;
            }
        };
        // the account's own name only stands in for a single calendar; otherwise each keeps its own
        let name = if calendars.len() == 1 && !src.name.is_empty() { src.name.clone() } else { cal.name.clone() };
        let color = src.color.or(cal.color).unwrap_or(default_color);
        for ics in cd::parse_report(&xml) {
            out.extend(island_core::ics::parse(&ics, off, from, to).into_iter().map(|e| Event { color: Some(color), calendar: name.clone(), ..e }));
        }
    }
    Ok(out)
}

/// Synced lyrics for a track from LRCLIB, or None.
pub fn fetch_lyrics(title: &str, artist: &str, length_secs: f64) -> Option<String> {
    let clean = lyrics::clean_title(title);
    let url = format!("https://lrclib.net/api/search?track_name={}&artist_name={}", urlenc(&clean), urlenc(artist));
    let v: serde_json::Value = agent().get(&url).call().ok()?.body_mut().read_json().ok()?;
    let versions: Vec<Version> = v
        .as_array()?
        .iter()
        .map(|i| Version {
            id: i.get("id").and_then(|x| x.as_u64()),
            duration: i.get("duration").and_then(|x| x.as_f64()).unwrap_or(0.0),
            synced: i.get("syncedLyrics").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        })
        .collect();
    lyrics::pick(&versions, length_secs, 5.0)
}

fn urlenc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_urls() {
        assert_eq!(urlenc("Touch (feat. X)"), "Touch%20%28feat.%20X%29");
        assert_eq!(urlenc("è"), "%C3%A8");
    }

    #[test]
    fn reads_an_open_meteo_response() {
        let v = serde_json::json!({
            "current": {"temperature_2m": 14.2, "apparent_temperature": 12.0, "weather_code": 2, "is_day": 1},
            "daily": {"temperature_2m_max": [17.0], "temperature_2m_min": [9.0]},
            "minutely_15": {"precipitation": [0.0, 0.0, 0.4]}
        });
        let w = parse_weather(&v).unwrap();
        assert_eq!((w.code, w.day), (2, true));
        assert_eq!(w.high_c, 17.0);
        assert_eq!(w.rain_slots.len(), 3);
        assert_eq!(w.rain_slots[2].offset_min, 30);
        assert!(parse_weather(&serde_json::json!({})).is_none());
    }
}

#[cfg(test)]
mod caldav_tests {
    use super::*;
    use crate::config::{CalendarKind, CalendarSource};
    use std::io::{Read, Write};

    /// A tiny CalDAV server: answers the discovery and the query only for the right credentials.
    fn mock(expected_auth: String) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (mut stream, expected) = (stream, expected_auth.clone());
                std::thread::spawn(move || {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    let head_end = loop {
                        let n = stream.read(&mut chunk).unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break p + 4;
                        }
                    };
                    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                    let len: usize = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
                    while buf.len() < head_end + len {
                        let n = stream.read(&mut chunk).unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    let first = head.lines().next().unwrap_or("").to_string();
                    let authed = head.lines().any(|l| l.to_lowercase().starts_with("authorization:") && l.contains(&expected));
                    let (status, body) = if !authed {
                        ("401 Unauthorized", String::new())
                    } else if first.starts_with("PROPFIND / ") {
                        ("207 Multi-Status", r#"<d:multistatus xmlns:d="DAV:"><d:response><d:propstat><d:prop><d:current-user-principal><d:href>/u/principal/</d:href></d:current-user-principal></d:prop></d:propstat></d:response></d:multistatus>"#.to_string())
                    } else if first.starts_with("PROPFIND /u/principal/ ") {
                        ("207 Multi-Status", r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:propstat><d:prop><c:calendar-home-set><d:href>/u/calendars/</d:href></c:calendar-home-set></d:prop></d:propstat></d:response></d:multistatus>"#.to_string())
                    } else if first.starts_with("PROPFIND /u/calendars/ ") {
                        ("207 Multi-Status", r##"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:a="http://apple.com/ns/ical/"><d:response><d:href>/u/calendars/home/</d:href><d:propstat><d:prop><d:displayname>Personal</d:displayname><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><a:calendar-color>#FF2968FF</a:calendar-color></d:prop></d:propstat></d:response></d:multistatus>"##.to_string())
                    } else if first.starts_with("REPORT /u/calendars/home/ ") {
                        ("207 Multi-Status", "<d:multistatus xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\"><d:response><d:propstat><d:prop><c:calendar-data>BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:e1\nSUMMARY:Dentista\nDTSTART:20261120T090000Z\nDTEND:20261120T100000Z\nEND:VEVENT\nEND:VCALENDAR</c:calendar-data></d:prop></d:propstat></d:response></d:multistatus>".to_string())
                    } else {
                        ("404 Not Found", String::new())
                    };
                    let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                });
            }
        });
        format!("http://127.0.0.1:{port}/")
    }

    fn source(server: &str, password: &str) -> CalendarSource {
        CalendarSource { name: "Account".into(), kind: CalendarKind::CalDav { server: server.into(), user: "me@icloud.com".into(), password: password.into(), password_cmd: String::new() }, color: None }
    }

    #[test]
    fn reads_a_caldav_account_end_to_end() {
        let auth = super::super::caldav::basic_auth("me@icloud.com", "app-pass");
        let server = mock(auth);
        let evs = fetch_caldav(&source(&server, "app-pass"), 0, 0, i64::MAX, (1, 2, 3)).expect("signs in and reads");
        assert_eq!(evs.len(), 1, "{evs:?}");
        assert_eq!(evs[0].title, "Dentista");
        assert_eq!(evs[0].calendar, "Account", "a single calendar takes the account's name");
        assert_eq!(evs[0].color, Some((0xff, 0x29, 0x68)), "the server's colour");
        // a colour chosen for the account wins
        let mut s = source(&server, "app-pass");
        s.color = Some((9, 9, 9));
        assert_eq!(fetch_caldav(&s, 0, 0, i64::MAX, (1, 2, 3)).unwrap()[0].color, Some((9, 9, 9)));
    }

    #[test]
    fn a_wrong_password_says_so() {
        let server = mock(super::super::caldav::basic_auth("me@icloud.com", "right"));
        let err = fetch_caldav(&source(&server, "wrong"), 0, 0, i64::MAX, (1, 2, 3)).unwrap_err();
        assert!(err.contains("sign-in failed"), "{err}");
        assert!(fetch_caldav(&source(&server, ""), 0, 0, i64::MAX, (1, 2, 3)).unwrap_err().contains("no password"));
    }

    #[test]
    fn the_password_can_come_from_a_command() {
        assert_eq!(caldav_password("", "echo secret-from-cmd").unwrap(), "secret-from-cmd");
        assert_eq!(caldav_password("plain", "").unwrap(), "plain");
        assert!(caldav_password("plain", "true").is_err(), "a command that prints nothing is an error");
    }
}
