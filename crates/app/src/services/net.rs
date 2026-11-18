//! Network services: weather (Open-Meteo, no key), calendar feeds (iCal URLs) and lyrics (LRCLIB).
//! The island never guesses a location and never calls out without settings for it.

use super::Reporter;
use crate::config::Settings;
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
    let settings = crate::config::load();
    if let (Some(lat), Some(lon)) = (settings.latitude, settings.longitude) {
        let rep = rep.clone();
        std::thread::spawn(move || loop {
            match fetch_weather(lat, lon) {
                Ok(w) => rep.patch(move |d| d.weather = Some(w)),
                Err(e) => log::warn!("weather: {e}"),
            }
            std::thread::sleep(Duration::from_secs(600));
        });
    }
    if !settings.calendar_urls.is_empty() {
        std::thread::spawn(move || loop {
            let events = fetch_calendars(&settings);
            rep.patch(move |d| d.events = events);
            std::thread::sleep(Duration::from_secs(900));
        });
    }
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

fn fetch_calendars(s: &Settings) -> Vec<Event> {
    use chrono::Offset;
    let off = chrono::Local::now().offset().fix().local_minus_utc();
    let now = chrono::Utc::now().timestamp_millis();
    let (from, to) = (now - 86_400_000, now + 62 * 86_400_000);
    let mut all = vec![];
    for url in &s.calendar_urls {
        // webcal:// is https:// for calendar apps
        let url = url.replacen("webcal://", "https://", 1);
        match agent().get(&url).call().and_then(|mut r| r.body_mut().read_to_string()) {
            Ok(text) => all.extend(island_core::ics::parse(&text, off, from, to)),
            Err(e) => log::warn!("calendar {url}: {e}"),
        }
    }
    all.sort_by_key(|e| e.start_ms);
    all
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
