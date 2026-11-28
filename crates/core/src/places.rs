//! Reminders tied to a place ("when I get home buy milk"): when to tell them, and how they are said. Pure: where the
//! machine is comes as a distance and the doubt about it.

/// How close counts as being there, in metres.
const NEAR_M: f64 = 200.0;
/// A position less sure than this says nothing about places a few hundred metres across.
pub const ROUGH_M: f64 = 500.0;

/// A reminder waiting for the machine to reach a place, or to leave it.
#[derive(Clone, Debug, PartialEq)]
pub struct Waiting {
    /// "home", or the name a place was saved under.
    pub place: String,
    pub leaving: bool,
    pub text: String,
    /// The other side of the change has been seen (away before an arrival, there before a departure), so that the next
    /// reading on the right side is a real arrival or departure.
    pub armed: bool,
}

impl Waiting {
    /// Looks at a reading: `distance_m` from the place, sure within `accuracy_m`. True when it is time to tell. A reading
    /// too rough is ignored, and between near and clearly away nothing changes, so that a position that wavers at the
    /// edge tells nothing.
    pub fn sees(&mut self, distance_m: f64, accuracy_m: f64) -> bool {
        if accuracy_m > ROUGH_M {
            return false;
        }
        let near = NEAR_M.max(accuracy_m);
        let (there, away) = (distance_m <= near, distance_m > near * 1.5);
        let (before, now) = if self.leaving { (there, away) } else { (away, there) };
        if before {
            self.armed = true;
        }
        self.armed && now
    }

    /// "when you get home", "when you leave uni"
    pub fn when(&self) -> String {
        match (self.leaving, self.place.as_str()) {
            (false, "home") => "when you get home".into(),
            (false, place) => format!("when you get to {place}"),
            (true, "home") => "when you leave home".into(),
            (true, place) => format!("when you leave {place}"),
        }
    }
}

/// The name a place goes by: one word, small letters, "casa" being home.
pub fn place_name(word: &str) -> String {
    let w = word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    if w == "casa" { "home".into() } else { w }
}

/// Reads what follows "remind me": `when i get home buy milk`, `when i leave uni call sam`, `quando arrivo a casa ...`,
/// `quando esco da lavoro ...`, `at home ...`, `a casa ...`. Returns the place, whether it is about leaving it, and how
/// many words said so (the rest is what to remember).
pub fn parse(words: &[&str]) -> Option<(String, bool, usize)> {
    let low: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
    let at = |i: usize| low.get(i).map(String::as_str);
    if matches!(at(0), Some("at" | "a")) && matches!(at(1), Some("home" | "casa")) {
        return Some(("home".into(), false, 2));
    }
    if !matches!(at(0), Some("when" | "quando")) {
        return None;
    }
    let mut i = 1;
    while matches!(at(i), Some("i" | "i'm" | "im" | "am" | "io")) {
        i += 1;
    }
    let leaving = match at(i) {
        Some("get" | "arrive" | "reach" | "come" | "arrivo" | "torno" | "sono" | "rientro" | "am") => {
            i += 1;
            false
        }
        Some("leave" | "esco" | "lascio" | "parto") => {
            i += 1;
            true
        }
        // "when home", "when i'm home"
        _ => false,
    };
    while matches!(at(i), Some("to" | "at" | "in" | "a" | "da" | "dal" | "dalla" | "di" | "back" | "from" | "al" | "alla")) {
        i += 1;
    }
    let place = place_name(at(i)?);
    (!place.is_empty()).then_some((place, leaving, i + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waiting(leaving: bool) -> Waiting {
        Waiting { place: "home".into(), leaving, text: "x".into(), armed: false }
    }

    #[test]
    fn an_arrival_is_a_reading_there_after_one_away() {
        let mut w = waiting(false);
        assert!(!w.sees(10.0, 30.0), "made while there: not an arrival");
        assert!(!w.sees(250.0, 30.0), "a waver at the edge");
        assert!(!w.sees(10.0, 30.0));
        assert!(!w.sees(900.0, 30.0), "away");
        assert!(!w.sees(10.0, 25_000.0), "too rough to say anything");
        assert!(!w.sees(260.0, 30.0), "still not there");
        assert!(w.sees(150.0, 30.0), "there");
    }

    #[test]
    fn leaving_is_a_reading_away_after_one_there_and_doubt_widens_the_place() {
        let mut w = waiting(true);
        assert!(!w.sees(5000.0, 30.0), "never seen there");
        assert!(!w.sees(350.0, 400.0), "within the doubt of the reading: there");
        assert!(!w.sees(500.0, 400.0), "not clearly away");
        assert!(w.sees(700.0, 400.0));
        assert_eq!(w.when(), "when you leave home");
    }

    #[test]
    fn a_place_is_read_in_english_and_italian() {
        let p = |line: &str| {
            let words: Vec<&str> = line.split(' ').collect();
            parse(&words).map(|(place, leaving, used)| (place, leaving, words[used..].join(" ")))
        };
        let say = |place: &str, leaving: bool, rest: &str| Some((place.to_string(), leaving, rest.to_string()));
        assert_eq!(p("when i get home buy milk"), say("home", false, "buy milk"));
        assert_eq!(p("when I'm home water the plants"), say("home", false, "water the plants"));
        assert_eq!(p("when i get to uni ask Sam"), say("uni", false, "ask Sam"));
        assert_eq!(p("when i leave work call mum"), say("work", true, "call mum"));
        assert_eq!(p("quando arrivo a casa stendere"), say("home", false, "stendere"));
        assert_eq!(p("quando torno a casa stendere"), say("home", false, "stendere"));
        assert_eq!(p("quando esco da palestra bere"), say("palestra", true, "bere"));
        assert_eq!(p("at home feed the cat"), say("home", false, "feed the cat"));
        assert_eq!(p("a casa chiamare Anna"), say("home", false, "chiamare Anna"));
        for no in ["at 9pm call", "a domani", "in 20m x", "when", "tomorrow 8 x"] {
            assert_eq!(p(no), None, "{no}");
        }
    }
}
