//! The European Central Bank's daily euro reference rates, for `20 usd in eur` in the command bar. One small XML file,
//! fetched at most every twelve hours (the ECB publishes once a working day), kept on disk so that the bar works offline
//! with the last rates. Only the machine's address reaches the ECB; the option `currency_rates` switches it off.

use super::Reporter;

const URL: &str = "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml";
const MAX_AGE_S: i64 = 12 * 3600;

fn now_s() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// The kept rates and when they were fetched.
fn load() -> Option<(i64, Vec<(String, f64)>)> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(super::state_file("rates.json")?).ok()?).ok()?;
    let at = v.get("at")?.as_i64()?;
    let rates = v.get("rates")?.as_object()?.iter().filter_map(|(k, v)| Some((k.clone(), v.as_f64()?))).collect();
    Some((at, rates))
}

fn save(at: i64, rates: &[(String, f64)]) {
    let Some(p) = super::state_file("rates.json") else { return };
    let map: serde_json::Map<String, serde_json::Value> = rates.iter().map(|(k, v)| (k.clone(), serde_json::json!(v))).collect();
    let _ = std::fs::write(p, serde_json::json!({"at": at, "rates": map}).to_string());
}

pub fn start(rep: Reporter) {
    std::thread::spawn(move || {
        let mut have_at = 0;
        if let Some((at, rates)) = load() {
            have_at = at;
            rep.patch(move |d| d.rates = rates);
        }
        loop {
            if crate::config::load().ui.currency_rates && now_s() - have_at > MAX_AGE_S {
                match super::net::agent().get(URL).call().map_err(|e| e.to_string()).and_then(|mut r| r.body_mut().read_to_string().map_err(|e| e.to_string())) {
                    Ok(xml) => {
                        let rates = island_core::quick::parse_ecb(&xml);
                        if rates.len() >= 10 {
                            have_at = now_s();
                            save(have_at, &rates);
                            rep.patch(move |d| d.rates = rates);
                        } else {
                            log::warn!("currency rates: the answer held {} rates", rates.len());
                        }
                    }
                    Err(e) => log::warn!("currency rates: {e}"),
                }
            }
            // an hour, or at once when the settings change
            super::wait_for(3600, &[&super::CONFIG_GEN]);
        }
    });
}
