//! Before you go: the things worth knowing before an event that the calendar alone does not say — rain on the way,
//! a battery that will not last.

use crate::weather::Outlook;

/// Rain that is falling or will fall before the person has arrived: `leave_in_min` is how long until they leave,
/// `trip_min` how long they are on the way (an hour of margin after that is enough to matter).
pub fn rain_note(outlook: Outlook, leave_in_min: i32, trip_min: i32) -> Option<String> {
    match outlook {
        Outlook::Raining { .. } => Some("it is raining: take an umbrella".into()),
        Outlook::Rain { in_min } if in_min <= leave_in_min.max(0) + trip_min.max(0) + 30 => Some(format!("rain in {in_min} min: take an umbrella")),
        _ => None,
    }
}

/// A battery that is low and not charging, with an event coming.
pub fn battery_note(percent: f32, on_ac: bool) -> Option<String> {
    (!on_ac && percent < 30.0).then(|| format!("battery {}%: bring the charger", percent.round() as i32))
}

/// The notes as one line.
pub fn line(notes: impl IntoIterator<Item = Option<String>>) -> String {
    notes.into_iter().flatten().collect::<Vec<_>>().join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rain_matters_when_it_falls_before_arriving() {
        assert!(rain_note(Outlook::Raining { clears_in_min: None }, 20, 10).unwrap().contains("raining"));
        assert_eq!(rain_note(Outlook::Rain { in_min: 25 }, 20, 10).unwrap(), "rain in 25 min: take an umbrella");
        assert!(rain_note(Outlook::Rain { in_min: 200 }, 20, 10).is_none(), "far off");
        assert!(rain_note(Outlook::Dry, 20, 10).is_none());
        assert!(rain_note(Outlook::Rain { in_min: 50 }, -5, 10).is_none(), "an overdue leave time does not stretch the window");
    }

    #[test]
    fn the_battery_matters_only_when_low_and_unplugged() {
        assert_eq!(battery_note(22.4, false).unwrap(), "battery 22%: bring the charger");
        assert!(battery_note(22.0, true).is_none() && battery_note(80.0, false).is_none());
        assert_eq!(line([Some("a".into()), None, Some("b".into())]), "a · b");
        assert_eq!(line([None, None]), "");
    }
}
