//! Public holidays, worked out (no network, no file): a calendar that is always there, so that a day without lectures is
//! known as one. Only Italy so far.

use crate::calendar::{civil_from_days, days_from_civil};

/// Easter Sunday of a year (the Gregorian computus, Meeus/Jones/Butcher): (month, day).
pub fn easter(year: i64) -> (i64, i64) {
    let (a, b, c) = (year % 19, year / 100, year % 100);
    let (d, e) = (b / 4, b % 4);
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let (i, k) = (c / 4, c % 4);
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    (month, day)
}

/// The holidays of a country in a year: (month, day, name). Unknown countries have none.
pub fn holidays(country: &str, year: i64) -> Vec<(i64, i64, String)> {
    match country.to_ascii_uppercase().as_str() {
        "IT" => {
            let (em, ed) = easter(year);
            let easter_day = days_from_civil(year, em, ed);
            let monday = civil_from_days(easter_day + 1);
            let mut v = vec![
                (1, 1, "Capodanno".to_string()),
                (1, 6, "Epifania".into()),
                (em, ed, "Pasqua".into()),
                (monday.1, monday.2, "Lunedì dell'Angelo".into()),
                (4, 25, "Festa della Liberazione".into()),
                (5, 1, "Festa dei Lavoratori".into()),
                (6, 2, "Festa della Repubblica".into()),
                (8, 15, "Assunzione".into()),
                (11, 1, "Ognissanti".into()),
                (12, 8, "Immacolata Concezione".into()),
                (12, 25, "Natale".into()),
                (12, 26, "Santo Stefano".into()),
            ];
            v.sort();
            v
        }
        _ => vec![],
    }
}

/// The countries `holidays` knows.
pub const COUNTRIES: &[&str] = &["IT"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easter_is_right_for_known_years() {
        assert_eq!(easter(2024), (3, 31));
        assert_eq!(easter(2025), (4, 20));
        assert_eq!(easter(2026), (4, 5));
        assert_eq!(easter(2027), (3, 28));
        assert_eq!(easter(2038), (4, 25), "the latest it can be");
        assert_eq!(easter(2285), (3, 22), "and the earliest");
    }

    #[test]
    fn the_italian_year_has_twelve_holidays_with_the_movable_ones_in_place() {
        let h = holidays("it", 2027);
        assert_eq!(h.len(), 12);
        assert!(h.contains(&(3, 28, "Pasqua".to_string())));
        assert!(h.contains(&(3, 29, "Lunedì dell'Angelo".to_string())));
        assert!(h.contains(&(6, 2, "Festa della Repubblica".to_string())));
        assert_eq!(holidays("IT", 2026).iter().find(|x| x.2 == "Lunedì dell'Angelo").map(|x| (x.0, x.1)), Some((4, 6)));
        assert!(h.windows(2).all(|w| (w[0].0, w[0].1) <= (w[1].0, w[1].1)), "in date order");
        assert!(holidays("XX", 2027).is_empty());
    }
}
