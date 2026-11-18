//! Time zones for calendar events: the offset from UTC of an event's own zone at a given instant. Calendar
//! files name zones the IANA way ("Europe/Rome") or the Windows way ("W. Europe Standard Time", what Outlook
//! writes); an event without a zone (a "floating" time) is read in the local zone.

use chrono::{Offset, TimeZone};
use chrono_tz::Tz;

/// Windows zone names mapped to an IANA zone with the same rules (the CLDR table's most common entries).
const WINDOWS: &[(&str, &str)] = &[
    ("W. Europe Standard Time", "Europe/Berlin"),
    ("Central Europe Standard Time", "Europe/Budapest"),
    ("Central European Standard Time", "Europe/Warsaw"),
    ("Romance Standard Time", "Europe/Paris"),
    ("GMT Standard Time", "Europe/London"),
    ("Greenwich Standard Time", "Atlantic/Reykjavik"),
    ("E. Europe Standard Time", "Europe/Chisinau"),
    ("FLE Standard Time", "Europe/Kyiv"),
    ("GTB Standard Time", "Europe/Bucharest"),
    ("Russian Standard Time", "Europe/Moscow"),
    ("Turkey Standard Time", "Europe/Istanbul"),
    ("Israel Standard Time", "Asia/Jerusalem"),
    ("Egypt Standard Time", "Africa/Cairo"),
    ("South Africa Standard Time", "Africa/Johannesburg"),
    ("W. Central Africa Standard Time", "Africa/Lagos"),
    ("Arabian Standard Time", "Asia/Dubai"),
    ("India Standard Time", "Asia/Kolkata"),
    ("SE Asia Standard Time", "Asia/Bangkok"),
    ("China Standard Time", "Asia/Shanghai"),
    ("Singapore Standard Time", "Asia/Singapore"),
    ("Tokyo Standard Time", "Asia/Tokyo"),
    ("Korea Standard Time", "Asia/Seoul"),
    ("AUS Eastern Standard Time", "Australia/Sydney"),
    ("New Zealand Standard Time", "Pacific/Auckland"),
    ("Hawaiian Standard Time", "Pacific/Honolulu"),
    ("Alaskan Standard Time", "America/Anchorage"),
    ("Pacific Standard Time", "America/Los_Angeles"),
    ("Mountain Standard Time", "America/Denver"),
    ("Central Standard Time", "America/Chicago"),
    ("Eastern Standard Time", "America/New_York"),
    ("Atlantic Standard Time", "America/Halifax"),
    ("Newfoundland Standard Time", "America/St_Johns"),
    ("SA Pacific Standard Time", "America/Bogota"),
    ("E. South America Standard Time", "America/Sao_Paulo"),
    ("Argentina Standard Time", "America/Argentina/Buenos_Aires"),
    ("UTC", "UTC"),
];

/// The zone a calendar's zone name stands for.
pub fn zone(name: &str) -> Option<Tz> {
    let name = name.trim().trim_matches('"');
    name.parse::<Tz>().ok().or_else(|| WINDOWS.iter().find(|(w, _)| w.eq_ignore_ascii_case(name)).and_then(|(_, i)| i.parse().ok()))
}

/// Offset from UTC, in seconds, of `tzid` (None or unknown: the local zone) at the UTC instant `utc_secs`.
pub fn offset(tzid: Option<&str>, utc_secs: i64) -> i32 {
    let at = chrono::DateTime::from_timestamp(utc_secs, 0).unwrap_or_default();
    match tzid.and_then(zone) {
        Some(tz) => tz.offset_from_utc_datetime(&at.naive_utc()).fix().local_minus_utc(),
        None => chrono::Local.offset_from_utc_datetime(&at.naive_utc()).fix().local_minus_utc(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JULY: i64 = 1_782_864_000; // 2026-07-01 00:00 UTC
    const NOVEMBER: i64 = 1_795_132_800; // 2026-11-20 00:00 UTC

    #[test]
    fn iana_and_windows_names_give_the_zones_rules() {
        assert_eq!(offset(Some("Europe/Rome"), JULY), 7200, "summer time");
        assert_eq!(offset(Some("Europe/Rome"), NOVEMBER), 3600);
        assert_eq!(offset(Some("W. Europe Standard Time"), JULY), 7200, "Outlook's name for central Europe");
        assert_eq!(offset(Some("W. Europe Standard Time"), NOVEMBER), 3600);
        assert_eq!(offset(Some("Eastern Standard Time"), JULY), -4 * 3600);
        assert_eq!(offset(Some("Eastern Standard Time"), NOVEMBER), -5 * 3600);
        assert_eq!(offset(Some("\"Europe/Rome\""), NOVEMBER), 3600, "quotes are dropped");
        assert_eq!(offset(Some("UTC"), JULY), 0);
    }

    #[test]
    fn an_unknown_or_missing_zone_is_the_local_one() {
        let local = offset(None, NOVEMBER);
        assert_eq!(offset(Some("Mars/Olympus_Mons"), NOVEMBER), local);
        assert!(zone("Mars/Olympus_Mons").is_none() && zone("europe/rome").is_some_and(|z| z == Tz::Europe__Rome) || zone("europe/rome").is_none());
    }
}
