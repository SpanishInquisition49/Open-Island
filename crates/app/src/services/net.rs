//! Network services: weather (Open-Meteo, no key), calendar feeds (iCal URLs) and lyrics (LRCLIB).
//! The island never guesses a location and never calls out without settings for it.

use super::Reporter;
use crate::config::{CalendarKind, Settings};
use island_core::calendar::Event;
use island_core::lyrics::{self, Version};
use island_core::weather::Slot;
use island_ui::model::Weather;
use std::collections::HashMap;
use std::time::Duration;

/// rustls needs a default crypto provider and two are linked (ring through ureq, aws-lc through attohttpc): ring is
/// installed once, before any TLS connection.
pub fn init_tls() {
    #[cfg(target_os = "linux")]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
        });
    }
}

pub fn agent() -> ureq::Agent {
    init_tls();
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
        // where the machine is, when it follows its position; else the coordinates of the settings
        let at = super::location::point(&s);
        log::debug!("weather for {at:?}");
        match at {
            _ if !s.ui.weather => weather_rep.patch(|d| d.weather = None),
            Some((lat, lon)) => match fetch_weather(lat, lon) {
                Ok(w) => weather_rep.patch(move |d| d.weather = Some(w)),
                Err(e) => log::warn!("weather: {e}"),
            },
            _ => weather_rep.patch(|d| d.weather = None),
        }
        super::wait_for(600, &[&super::CONFIG_GEN, &super::LOCATION_GEN]);
    });
    std::thread::spawn(move || {
        let mut cache: HashMap<String, Vec<Event>> = HashMap::new();
        // the last schedule that was fetched: without a network (a train, a lecture hall) the island still knows the day
        let (saved, saved_at) = load_calendar_cache(&crate::config::load().calendars);
        if !saved.is_empty() {
            let mut all: Vec<Event> = saved.values().flatten().cloned().collect();
            all.sort_by_key(|e| e.start_ms);
            cache = saved;
            rep.patch(move |d| {
                d.events = all;
                d.events_synced_at = saved_at;
            });
        }
        loop {
            let mut s = crate::config::load();
            // events made with `event` and kept here (no account to hold them) are a calendar like any other
            s.calendars.extend(super::organizer::local_calendar());
            let mut retry = false;
            if s.calendars.is_empty() {
                cache.clear();
                rep.patch(|d| {
                    d.events = vec![];
                    d.events_synced_at = 0;
                });
            } else {
                let (events, problems, any_ok) = fetch_calendars(&s, &mut cache);
                let now = chrono::Utc::now().timestamp_millis();
                if any_ok {
                    save_calendar_cache(&cache, now);
                }
                rep.patch(move |d| {
                    d.events = events;
                    if any_ok {
                        d.events_synced_at = now;
                    }
                });
                if let Some(p) = problems.first() {
                    rep.input(island_ui::model::Input::SettingsNote(p.clone()));
                    retry = true;
                }
            }
            // every five minutes, a minute after a failure, at once when asked or when the settings change
            super::wait_for(if retry { 60 } else { 300 }, &[&super::CONFIG_GEN, &super::CALENDAR_REFRESH]);
        }
    });
}

fn fetch_weather(lat: f64, lon: f64) -> Result<Weather, String> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&current=temperature_2m,apparent_temperature,weather_code,is_day&daily=temperature_2m_max,temperature_2m_min&hourly=temperature_2m,precipitation_probability,precipitation,weather_code,is_day&minutely_15=precipitation&forecast_minutely_15=8&timezone=auto&forecast_days=2"
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
        hours: parse_hours(v),
    })
}

/// The hourly arrays of an Open-Meteo response; times are local (`2026-11-18T14:00`).
fn parse_hours(v: &serde_json::Value) -> Vec<island_core::weather::Hour> {
    let Some(h) = v.get("hourly") else { return vec![] };
    let offset = v.get("utc_offset_seconds").and_then(|o| o.as_i64()).unwrap_or(0);
    let arr = |k: &str| h.get(k).and_then(|a| a.as_array());
    let Some(times) = arr("time") else { return vec![] };
    let at = |k: &str, i: usize| arr(k).and_then(|a| a.get(i)).and_then(|x| x.as_f64());
    times
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let t = t.as_str()?;
            let (date, time) = t.split_once('T')?;
            let mut d = date.split('-').filter_map(|x| x.parse::<i64>().ok());
            let (y, mo, da) = (d.next()?, d.next()?, d.next()?);
            let mut hm = time.split(':').filter_map(|x| x.parse::<i64>().ok());
            let (hh, mm) = (hm.next()?, hm.next().unwrap_or(0));
            let secs = island_core::calendar::days_from_civil(y, mo, da) * 86_400 + hh * 3600 + mm * 60 - offset;
            Some(island_core::weather::Hour {
                time_ms: secs * 1000,
                temp_c: at("temperature_2m", i).unwrap_or(0.0) as f32,
                code: at("weather_code", i).unwrap_or(0.0) as i32,
                is_day: at("is_day", i).unwrap_or(1.0) >= 1.0,
                rain_prob: at("precipitation_probability", i).unwrap_or(0.0).clamp(0.0, 100.0) as u8,
                mm: at("precipitation", i).unwrap_or(0.0) as f32,
            })
        })
        .collect()
}

/// Reads every calendar source; a source that fails keeps its last events (a dropped connection must not empty
/// the agenda). Returns the events (sorted), the problems, and whether at least one source was read.
fn fetch_calendars(s: &Settings, cache: &mut HashMap<String, Vec<Event>>) -> (Vec<Event>, Vec<String>, bool) {
    let now = chrono::Utc::now().timestamp_millis();
    let (from, to) = (now - 86_400_000, now + 62 * 86_400_000);
    let results = s.calendars.iter().enumerate().map(|(i, src)| (source_key(src), src.detail(), fetch_source(src, i, from, to))).collect();
    merge_results(results, cache)
}

/// The sources are named on disk by a hash of their settings (which can hold a password), not by the settings.
fn key_hash(key: &str) -> String {
    let h = key.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
    format!("{h:016x}")
}

/// What was fetched last time for the sources that are still configured, and when.
fn load_calendar_cache(sources: &[crate::config::CalendarSource]) -> (HashMap<String, Vec<Event>>, i64) {
    let Some(v) = super::state_file("calendar-cache.json").and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { return (HashMap::new(), 0) };
    let at = v.get("saved_at").and_then(|x| x.as_i64()).unwrap_or(0);
    let mut out = HashMap::new();
    for src in sources {
        let key = source_key(src);
        if let Some(list) = v.get("sources").and_then(|s| s.get(key_hash(&key))).and_then(|l| l.as_array()) {
            out.insert(key, list.iter().filter_map(island_core::calendar::event_from_json).collect());
        }
    }
    (out, at)
}

fn save_calendar_cache(cache: &HashMap<String, Vec<Event>>, at: i64) {
    let Some(p) = super::state_file("calendar-cache.json") else { return };
    let sources: serde_json::Map<String, serde_json::Value> = cache.iter().map(|(k, evs)| (key_hash(k), serde_json::Value::Array(evs.iter().map(island_core::calendar::event_to_json).collect()))).collect();
    let text = serde_json::json!({"saved_at": at, "sources": sources}).to_string();
    // event titles are private: owner-only
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        if let Ok(mut f) = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&p) {
            let _ = f.write_all(text.as_bytes());
        }
    }
    #[cfg(not(unix))]
    let _ = std::fs::write(&p, text);
}

fn source_key(src: &crate::config::CalendarSource) -> String {
    src.to_json().to_string()
}

/// Combines the per-source results with the cache: a success refreshes it, a failure falls back on it, a source
/// that is gone from the settings is dropped.
pub fn merge_results(results: Vec<(String, String, Result<Vec<Event>, String>)>, cache: &mut HashMap<String, Vec<Event>>) -> (Vec<Event>, Vec<String>, bool) {
    let keep: std::collections::HashSet<String> = results.iter().map(|(k, _, _)| k.clone()).collect();
    cache.retain(|k, _| keep.contains(k));
    let (mut all, mut problems, mut any_ok) = (vec![], vec![], false);
    for (key, label, result) in results {
        match result {
            Ok(evs) => {
                any_ok = true;
                cache.insert(key, evs.clone());
                all.extend(evs);
            }
            Err(e) => {
                problems.push(format!("{label}: {e}"));
                all.extend(cache.get(&key).cloned().unwrap_or_default());
            }
        }
    }
    all.sort_by_key(|e| e.start_ms);
    (all, problems, any_ok)
}

/// One source's events, named and coloured.
fn fetch_source(src: &crate::config::CalendarSource, i: usize, from: i64, to: i64) -> Result<Vec<Event>, String> {
    let default_color = crate::config::PALETTE[i % crate::config::PALETTE.len()];
    match &src.kind {
        CalendarKind::Ics { url } => {
            // webcal:// is https:// for calendar apps
            let url = url.replacen("webcal://", "https://", 1).replacen("Webcal://", "https://", 1);
            let text = agent().get(&url).call().and_then(|mut r| r.body_mut().read_to_string()).map_err(|e| short(&e.to_string()))?;
            let name = if src.name.is_empty() { island_core::ics::calendar_name(&text).unwrap_or_else(|| src.detail()) } else { src.name.clone() };
            let color = src.color.unwrap_or(default_color);
            Ok(island_core::ics::parse_in(&text, &super::tz::offset, from, to).into_iter().map(|e| Event { color: Some(color), calendar: name.clone(), ..e }).collect())
        }
        CalendarKind::CalDav { .. } => fetch_caldav(src, from, to, default_color),
        CalendarKind::File { path } => fetch_files(&crate::config::expand_home(path), &src.name, src.color.unwrap_or(default_color), from, to),
        CalendarKind::Holidays { country } => Ok(holiday_events(country, &if src.name.is_empty() { "Holidays".to_string() } else { src.name.clone() }, src.color.unwrap_or(default_color), from, to)),
    }
}

/// An .ics file, or every .ics file of a folder (one level), read now. A file that cannot be read is an error the person can act on.
fn fetch_files(path: &str, name: &str, color: (u8, u8, u8), from: i64, to: i64) -> Result<Vec<Event>, String> {
    let p = std::path::Path::new(path);
    let files: Vec<std::path::PathBuf> = if p.is_dir() {
        let mut v: Vec<_> = std::fs::read_dir(p).map_err(|e| short(&e.to_string()))?.flatten().map(|e| e.path()).filter(|f| f.extension().is_some_and(|x| x.eq_ignore_ascii_case("ics") || x.eq_ignore_ascii_case("ical"))).collect();
        v.sort();
        v
    } else {
        vec![p.to_path_buf()]
    };
    let mut out = vec![];
    for f in &files {
        let text = std::fs::read_to_string(f).map_err(|e| format!("{}: {}", f.display(), short(&e.to_string())))?;
        let label = if !name.is_empty() { name.to_string() } else { island_core::ics::calendar_name(&text).unwrap_or_else(|| f.file_stem().map_or(String::new(), |s| s.to_string_lossy().into_owned())) };
        out.extend(island_core::ics::parse_in(&text, &super::tz::offset, from, to).into_iter().map(|e| Event { color: Some(color), calendar: label.clone(), ..e }));
    }
    Ok(out)
}

/// The holidays that fall in [from, to), as all-day events at local midnight.
fn holiday_events(country: &str, name: &str, color: (u8, u8, u8), from: i64, to: i64) -> Vec<Event> {
    use chrono::TimeZone;
    let year_of = |ms: i64| chrono::DateTime::from_timestamp_millis(ms).unwrap_or_default().format("%Y").to_string().parse::<i64>().unwrap_or(2000);
    let mut out = vec![];
    for year in year_of(from)..=year_of(to) {
        for (m, d, title) in island_core::holidays::holidays(country, year) {
            let Some(start) = chrono::Local.with_ymd_and_hms(year as i32, m as u32, d as u32, 0, 0, 0).single().map(|t| t.timestamp_millis()) else { continue };
            let end = chrono::Local.with_ymd_and_hms(year as i32, m as u32, d as u32, 0, 0, 0).single().and_then(|t| (t + chrono::Duration::days(1)).timestamp_millis().into()).unwrap_or(start + 86_400_000);
            if end > from && start < to {
                out.push(Event { id: format!("holiday-{country}-{year}-{m}-{d}"), title, start_ms: start, end_ms: end, all_day: true, url: None, color: Some(color), calendar: name.to_string(), location: String::new() });
            }
        }
    }
    out
}

fn short(s: &str) -> String {
    s.chars().take(80).collect()
}

/// One DAV request; Err carries a sentence a person can act on. (ureq refuses PROPFIND and REPORT.)
fn dav(method: &str, url: &str, auth: &str, depth: &str, body: &str) -> Result<String, String> {
    init_tls();
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

pub(super) fn caldav_password(password: &str, cmd: &str) -> Result<String, String> {
    if cmd.trim().is_empty() {
        return if password.is_empty() { Err("no password set".into()) } else { Ok(password.to_string()) };
    }
    let out = std::process::Command::new("sh").arg("-c").arg(cmd).stderr(std::process::Stdio::null()).output().map_err(|e| e.to_string())?;
    let pw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() && !pw.is_empty() { Ok(pw) } else { Err("password_cmd printed nothing".into()) }
}

/// A signed-in CalDAV account: what is needed to ask it for more.
pub(super) struct DavAccount {
    pub auth: String,
    pub home_url: String,
}

/// Signs in and finds the calendar home of an account.
pub(super) fn discover(src: &crate::config::CalendarSource) -> Result<DavAccount, String> {
    use super::caldav as cd;
    let CalendarKind::CalDav { server, user, password, password_cmd } = &src.kind else { return Err("not a CalDAV account".into()) };
    let auth = cd::basic_auth(user, &caldav_password(password, password_cmd)?);
    let principal = cd::parse_principal(&dav("PROPFIND", server, &auth, "0", cd::PRINCIPAL_BODY)?).ok_or("no account found on the server")?;
    let principal_url = cd::resolve(server, &principal);
    let home = cd::parse_home(&dav("PROPFIND", &principal_url, &auth, "0", cd::HOME_BODY)?).ok_or("no calendars found on the server")?;
    Ok(DavAccount { auth, home_url: cd::resolve(&principal_url, &home) })
}

/// The calendars of an account that hold `comp` entries (`VEVENT` or `VTODO`).
pub(super) fn dav_calendars(acct: &DavAccount, comp: &str) -> Result<Vec<super::caldav::Found>, String> {
    use super::caldav as cd;
    Ok(cd::parse_calendars_for(&dav("PROPFIND", &acct.home_url, &acct.auth, "1", cd::LIST_BODY)?, comp))
}

/// A REPORT on a calendar (its absolute address).
pub(super) fn dav_report(url: &str, auth: &str, body: &str) -> Result<String, String> {
    dav("REPORT", url, auth, "1", body)
}

/// Writes one iCalendar document at an address (a new entry, or the same one again).
pub(super) fn dav_put(url: &str, auth: &str, ics: &str) -> Result<(), String> {
    init_tls();
    let resp = attohttpc::RequestBuilder::try_new(attohttpc::Method::PUT, url)
        .map_err(|e| short(&e.to_string()))?
        .header("Authorization", auth)
        .header("Content-Type", "text/calendar; charset=utf-8")
        .header("User-Agent", "open-island")
        .timeout(Duration::from_secs(20))
        .bytes(ics.as_bytes().to_vec())
        .send()
        .map_err(|e| short(&e.to_string()))?;
    match resp.status().as_u16() {
        200..=299 => Ok(()),
        401 | 403 => Err("sign-in failed, or the calendar is read-only".into()),
        c => Err(format!("the server answered {c}")),
    }
}

/// Deletes the resource at an address; one that is already gone counts as deleted.
pub(super) fn dav_delete(url: &str, auth: &str) -> Result<(), String> {
    init_tls();
    let resp = attohttpc::RequestBuilder::try_new(attohttpc::Method::DELETE, url)
        .map_err(|e| short(&e.to_string()))?
        .header("Authorization", auth)
        .header("User-Agent", "open-island")
        .timeout(Duration::from_secs(20))
        .send()
        .map_err(|e| short(&e.to_string()))?;
    match resp.status().as_u16() {
        200..=299 | 404 | 410 => Ok(()),
        401 | 403 => Err("sign-in failed, or the calendar is read-only".into()),
        c => Err(format!("the server answered {c}")),
    }
}

fn fetch_caldav(src: &crate::config::CalendarSource, from: i64, to: i64, default_color: (u8, u8, u8)) -> Result<Vec<Event>, String> {
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
            out.extend(island_core::ics::parse_in(&ics, &super::tz::offset, from, to).into_iter().map(|e| Event { color: Some(color), calendar: name.clone(), ..e }));
        }
    }
    Ok(out)
}

/// Synced lyrics for a track from LRCLIB, or None.
/// The synced lyrics of a track: Ok(None) when there are none, Err when the lookup itself failed (no network, the
/// service down), which is worth trying again.
pub fn fetch_lyrics(title: &str, artist: &str, length_secs: f64) -> Result<Option<String>, String> {
    let clean = lyrics::clean_title(title);
    let url = format!("https://lrclib.net/api/search?track_name={}&artist_name={}", urlenc(&clean), urlenc(artist));
    let v: serde_json::Value = agent().get(&url).call().map_err(|e| e.to_string())?.body_mut().read_json().map_err(|e| e.to_string())?;
    let versions: Vec<Version> = v
        .as_array()
        .ok_or("unexpected answer")?
        .iter()
        .map(|i| Version {
            id: i.get("id").and_then(|x| x.as_u64()),
            duration: i.get("duration").and_then(|x| x.as_f64()).unwrap_or(0.0),
            synced: i.get("syncedLyrics").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        })
        .collect();
    Ok(lyrics::pick(&versions, length_secs, 5.0))
}

fn urlenc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_ics_file_or_folder_and_the_holidays_become_events() {
        let dir = std::env::temp_dir().join(format!("oi-ics-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ics = |uid: &str, title: &str| format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:Orario\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nDTSTART:20270120T080000Z\r\nDTEND:20270120T093000Z\r\nSUMMARY:{title}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n");
        std::fs::write(dir.join("a.ics"), ics("a", "Reti")).unwrap();
        std::fs::write(dir.join("b.ICS"), ics("b", "Algoritmi")).unwrap();
        std::fs::write(dir.join("notes.txt"), "not a calendar").unwrap();
        let (from, to) = (1_800_000_000_000, 1_800_000_000_000 + 30 * 86_400_000); // 15 Jan - 14 Feb 2027
        let all = fetch_files(dir.to_str().unwrap(), "", (1, 2, 3), from, to).unwrap();
        assert_eq!(all.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["Reti", "Algoritmi"], "every .ics of the folder, the other files left alone");
        assert!(all.iter().all(|e| e.calendar == "Orario" && e.color == Some((1, 2, 3))), "named by the file's own name");
        let one = fetch_files(dir.join("a.ics").to_str().unwrap(), "Uni", (0, 0, 0), from, to).unwrap();
        assert_eq!((one.len(), one[0].calendar.as_str()), (1, "Uni"), "one file, the name given");
        assert!(fetch_files(dir.join("missing.ics").to_str().unwrap(), "", (0, 0, 0), from, to).is_err(), "a missing file is an error to show");
        let _ = std::fs::remove_dir_all(&dir);
        // holidays: January 2027 has Capodanno (before the window) and Epifania (in it); the window crosses no Easter
        let h = holiday_events("IT", "Holidays", (9, 9, 9), from, to);
        assert!(h.iter().all(|e| e.all_day && e.calendar == "Holidays"));
        assert!(!h.iter().any(|e| e.title == "Epifania"), "6 January is before 15 January");
        let year = holiday_events("IT", "H", (0, 0, 0), 1_798_761_600_000, 1_830_211_200_000); // 1 January to 31 December 2027
        assert_eq!(year.len(), 12, "every holiday of the year, once");
        assert!(year.iter().any(|e| e.title == "Pasqua") && year.iter().all(|e| e.end_ms - e.start_ms >= 23 * 3_600_000 && e.end_ms - e.start_ms <= 25 * 3_600_000));
        assert!(holiday_events("XX", "H", (0, 0, 0), from, to).is_empty());
    }

    #[test]
    fn the_schedule_is_kept_on_disk_without_the_password_and_comes_back_for_the_same_sources() {
        let dir = std::env::temp_dir().join(format!("oi-cal-cache-{}", std::process::id()));
        std::env::set_var("XDG_STATE_HOME", &dir);
        let feed = crate::config::CalendarSource { name: "Uni".into(), kind: crate::config::CalendarKind::Ics { url: "https://x/y.ics".into() }, color: None };
        let dav = crate::config::CalendarSource { name: "Cloud".into(), kind: crate::config::CalendarKind::CalDav { server: "https://s/".into(), user: "me".into(), password: "hunter2-secret".into(), password_cmd: String::new() }, color: None };
        let ev = |id: &str| Event { id: id.into(), title: format!("lesson {id}"), start_ms: 1000, end_ms: 2000, ..Default::default() };
        let mut cache = HashMap::new();
        cache.insert(source_key(&feed), vec![ev("a"), ev("b")]);
        cache.insert(source_key(&dav), vec![ev("c")]);
        save_calendar_cache(&cache, 777);
        let text = std::fs::read_to_string(dir.join("open-island").join("calendar-cache.json")).unwrap();
        assert!(!text.contains("hunter2-secret") && !text.contains("https://x/y.ics"), "the sources are only named by a hash");
        let (back, at) = load_calendar_cache(std::slice::from_ref(&feed));
        assert_eq!(at, 777);
        assert_eq!(back.len(), 1, "a source that is gone from the settings is not brought back");
        assert_eq!(back[&source_key(&feed)].iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert!(load_calendar_cache(&[]).0.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

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
            "minutely_15": {"precipitation": [0.0, 0.0, 0.4]},
            "utc_offset_seconds": 3600,
            "hourly": {"time": ["2026-11-18T00:00", "2026-11-18T01:00"], "temperature_2m": [8.5, 8.0], "weather_code": [3, 61], "is_day": [0, 0], "precipitation_probability": [10, 80], "precipitation": [0.0, 0.6]}
        });
        let w = parse_weather(&v).unwrap();
        assert_eq!((w.code, w.day), (2, true));
        assert_eq!(w.high_c, 17.0);
        assert_eq!(w.rain_slots.len(), 3);
        assert_eq!(w.rain_slots[2].offset_min, 30);
        assert_eq!(w.hours.len(), 2);
        // 2026-11-18T00:00 at UTC+1 is 2026-11-17T23:00Z = 1_794_956_400 s
        assert_eq!(w.hours[0].time_ms, 1_794_956_400_000);
        assert_eq!((w.hours[1].code, w.hours[1].rain_prob, w.hours[1].mm, w.hours[1].is_day), (61, 80, 0.6, false));
        assert_eq!(w.hours[1].time_ms - w.hours[0].time_ms, 3_600_000);
        assert!(parse_weather(&serde_json::json!({})).is_none());
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    fn ev(t: &str, start: i64) -> Event {
        Event { id: t.into(), title: t.into(), start_ms: start, end_ms: start + 1, ..Default::default() }
    }

    #[test]
    fn a_failing_source_keeps_its_last_events_and_a_removed_one_goes() {
        let mut cache = HashMap::new();
        let ok = |k: &str, evs: Vec<Event>| (k.to_string(), k.to_string(), Ok(evs));
        let (all, problems, any) = merge_results(vec![ok("a", vec![ev("one", 20)]), ok("b", vec![ev("two", 10)])], &mut cache);
        assert_eq!(all.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["two", "one"], "sorted by start");
        assert!(problems.is_empty() && any);
        // the network drops: b fails, its events stay
        let (all, problems, any) = merge_results(vec![ok("a", vec![ev("one", 20), ev("three", 30)]), ("b".into(), "Calendar B".into(), Err("timed out".into()))], &mut cache);
        assert_eq!(all.len(), 3, "b's old event is still there");
        assert_eq!(problems, vec!["Calendar B: timed out"]);
        assert!(any);
        // everything fails: the agenda keeps what it had, and nothing counts as synced
        let (all, _, any) = merge_results(vec![("a".into(), "A".into(), Err("x".into())), ("b".into(), "B".into(), Err("y".into()))], &mut cache);
        assert_eq!(all.len(), 3);
        assert!(!any);
        // b is removed from the settings: its cache goes, a failure of a never-read source gives nothing
        let (all, _, _) = merge_results(vec![("a".into(), "A".into(), Err("x".into())), ("c".into(), "C".into(), Err("z".into()))], &mut cache);
        assert_eq!(all.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["one", "three"]);
        assert!(!cache.contains_key("b"));
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
        let evs = fetch_caldav(&source(&server, "app-pass"), 0, i64::MAX, (1, 2, 3)).expect("signs in and reads");
        assert_eq!(evs.len(), 1, "{evs:?}");
        assert_eq!(evs[0].title, "Dentista");
        assert_eq!(evs[0].calendar, "Account", "a single calendar takes the account's name");
        assert_eq!(evs[0].color, Some((0xff, 0x29, 0x68)), "the server's colour");
        // a colour chosen for the account wins
        let mut s = source(&server, "app-pass");
        s.color = Some((9, 9, 9));
        assert_eq!(fetch_caldav(&s, 0, i64::MAX, (1, 2, 3)).unwrap()[0].color, Some((9, 9, 9)));
    }

    #[test]
    fn a_wrong_password_says_so() {
        let server = mock(super::super::caldav::basic_auth("me@icloud.com", "right"));
        let err = fetch_caldav(&source(&server, "wrong"), 0, i64::MAX, (1, 2, 3)).unwrap_err();
        assert!(err.contains("sign-in failed"), "{err}");
        assert!(fetch_caldav(&source(&server, ""), 0, i64::MAX, (1, 2, 3)).unwrap_err().contains("no password"));
    }

    #[test]
    fn the_password_can_come_from_a_command() {
        assert_eq!(caldav_password("", "echo secret-from-cmd").unwrap(), "secret-from-cmd");
        assert_eq!(caldav_password("plain", "").unwrap(), "plain");
        assert!(caldav_password("plain", "true").is_err(), "a command that prints nothing is an error");
    }
}

/// A developer's lens, not a test: `ICS_FILE=feed.ics cargo test -p open-island inspect_ics -- --ignored --nocapture`
/// prints the events of the next days the way the island reads a feed.
#[cfg(test)]
mod inspect {
    #[test]
    #[ignore]
    fn inspect_ics() {
        let Ok(path) = std::env::var("ICS_FILE") else { return };
        let text = std::fs::read_to_string(path).unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let evs = island_core::ics::parse_in(&text, &super::super::tz::offset, now - 86_400_000, now + 4 * 86_400_000);
        for e in evs {
            let t = |ms: i64| chrono::DateTime::from_timestamp_millis(ms).unwrap().with_timezone(&chrono::Local).format("%a %d %H:%M").to_string();
            println!("{} - {}  {}", t(e.start_ms), t(e.end_ms), e.title);
        }
    }
}

/// A developer's lens: reads the CalDAV accounts of the real configuration (read-only) and prints the next days:
/// `cargo test -p open-island inspect_caldav -- --ignored --nocapture`.
#[cfg(test)]
mod inspect_caldav {
    #[test]
    #[ignore]
    fn inspect_caldav() {
        let s = crate::config::load();
        let now = chrono::Utc::now().timestamp_millis();
        for (i, src) in s.calendars.iter().enumerate() {
            if !matches!(src.kind, crate::config::CalendarKind::CalDav { .. }) {
                continue;
            }
            match super::fetch_caldav(src, now - 86_400_000, now + 4 * 86_400_000, crate::config::PALETTE[i % 8]) {
                Err(e) => println!("ERROR {e}"),
                Ok(evs) => {
                    println!("{} events", evs.len());
                    for e in evs {
                        let t = |ms: i64| chrono::DateTime::from_timestamp_millis(ms).unwrap().with_timezone(&chrono::Local).format("%a %d %H:%M").to_string();
                        println!("{} - {}  [{}] {}", t(e.start_ms), t(e.end_ms), e.calendar, e.title);
                    }
                }
            }
        }
    }
}

/// An agent for talking to a device on the local network: LocalSend devices use a self-signed certificate, and a
/// status like 403 ("declined") is an answer to read, not an error.
pub fn lan_agent(timeout: Duration) -> ureq::Agent {
    init_tls();
    let tls = ureq::tls::TlsConfig::builder().disable_verification(true);
    #[cfg(not(target_os = "linux"))]
    let tls = tls.provider(ureq::tls::TlsProvider::NativeTls);
    ureq::Agent::config_builder().tls_config(tls.build()).timeout_global(Some(timeout)).http_status_as_error(false).user_agent("open-island").build().into()
}
