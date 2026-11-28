//! Whose birthday an entry of a calendar is, from its title; and the confetti that says so.

use crate::sky::chance;

/// The name in the title of a birthday ("Compleanno di Ada", "Ada's birthday", "🎂 Ada"): empty when it names nobody,
/// None when the title is not a birthday's. A title that only speaks of one ("gift for the birthday") is not.
pub fn who(title: &str) -> Option<String> {
    let title = title.trim();
    let tidy = |name: &str| name.trim_matches(|c: char| c.is_whitespace() || "-–—:,!".contains(c)).to_string();
    if title.contains('🎂') {
        let rest = tidy(&title.replace('🎂', " "));
        return Some(who(&rest).unwrap_or(rest));
    }
    // the title in lower case, character by character, so that places found in it are places in the title too
    let low: String = title.chars().map(|c| c.to_lowercase().next().filter(|l| l.len_utf8() == c.len_utf8()).unwrap_or(c)).collect();
    for start in ["buon compleanno", "compleanno", "happy birthday", "birthday", "cumpleaños", "anniversaire"] {
        if let Some(rest) = low.strip_prefix(start) {
            if rest.chars().next().is_some_and(char::is_alphanumeric) {
                continue;
            }
            let mut rest = title[start.len()..].trim_start_matches([' ', ':', '-', '–', '—']);
            for of in ["di ", "del ", "della ", "of ", "to ", "de "] {
                if rest.get(..of.len()).is_some_and(|s| s.eq_ignore_ascii_case(of)) {
                    rest = &rest[of.len()..];
                    break;
                }
            }
            return Some(tidy(rest));
        }
    }
    // after the name: "Ada's birthday", "Ada - compleanno"
    for end in ["'s birthday", "’s birthday", "' birthday", "’ birthday"] {
        if low.ends_with(end) {
            return Some(tidy(&title[..title.len() - end.len()]));
        }
    }
    for word in ["compleanno", "birthday"] {
        if let Some(rest) = low.strip_suffix(word) {
            let rest = rest.trim_end();
            if rest.ends_with(['-', '–', '—', ':', ',', '(']) {
                return Some(tidy(&title[..rest.len() - rest.chars().last().map_or(0, char::len_utf8)]));
            }
        }
    }
    None
}

/// How long the confetti is in the air.
pub const CONFETTI_MS: i64 = 2_600;
/// How many pieces.
const PIECES: u64 = 70;
/// The colours they come in.
pub const COLOURS: [(u8, u8, u8); 6] = [(255, 92, 92), (255, 190, 60), (255, 232, 92), (80, 214, 130), (92, 170, 255), (196, 122, 255)];

/// A piece of confetti: where it is from where they all set off, how it is turned, its size and colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bit {
    pub x: f64,
    pub y: f64,
    pub turn: f64,
    pub w: f64,
    pub h: f64,
    pub colour: usize,
    pub alpha: f64,
}

/// The confetti `age_ms` after it burst: thrown out sideways over `spread` either way, then fluttering down to about
/// `depth` below, and fading.
pub fn confetti(age_ms: i64, spread: f64, depth: f64) -> Vec<Bit> {
    if !(0..CONFETTI_MS).contains(&age_ms) {
        return vec![];
    }
    let t = age_ms as f64 / 1000.0;
    let last = CONFETTI_MS as f64 / 1000.0;
    // the air stops the throw quickly; the fall is slow and steady after it
    let thrown = 1.0 - (-t * 4.0).exp();
    (0..PIECES)
        .map(|i| {
            let side = chance(i, 61) * 2.0 - 1.0;
            let speed = 0.45 + 0.55 * chance(i, 62);
            let flutter = (t * (5.0 + 5.0 * chance(i, 63)) + chance(i, 64) * 6.0).sin();
            let x = side * spread * thrown + flutter * 4.0;
            let y = depth * speed * (t / last + 0.25 * thrown * chance(i, 65));
            let turn = t * (chance(i, 66) * 12.0 - 6.0) + chance(i, 67) * 6.0;
            // seen edge on and flat in turn
            let w = 2.0 + 3.5 * flutter.abs();
            let fade = ((last - t) / 0.7).clamp(0.0, 1.0);
            Bit { x, y, turn, w, h: 2.5 + chance(i, 68) * 1.5, colour: (i % COLOURS.len() as u64) as usize, alpha: fade }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn birthdays_are_told_by_their_titles() {
        for (title, name) in [
            ("Compleanno di Ada", "Ada"),
            ("compleanno Ada Lovelace", "Ada Lovelace"),
            ("Compleanno: nonna", "nonna"),
            ("Buon compleanno Ada!", "Ada"),
            ("Ada's birthday", "Ada"),
            ("Ada’s Birthday", "Ada"),
            ("James' birthday", "James"),
            ("Birthday of Ada", "Ada"),
            ("Birthday: Ada", "Ada"),
            ("Ada - compleanno", "Ada"),
            ("Ada – Birthday", "Ada"),
            ("Ada (compleanno", "Ada"),
            ("🎂 Ada", "Ada"),
            ("Ada 🎂", "Ada"),
            ("🎂 Compleanno di Ada", "Ada"),
            ("COMPLEANNO DI ADA", "ADA"),
            ("Compleanno di Nicolò", "Nicolò"),
            ("Compleanno", ""),
        ] {
            assert_eq!(who(title).as_deref(), Some(name), "{title}");
        }
        for title in ["Regalo di compleanno per Ada", "Cena compleanno", "Algorithms", "Birthdays committee", "compleannone", "İstanbul trip", ""] {
            assert_eq!(who(title), None, "{title}");
        }
    }

    #[test]
    fn confetti_bursts_falls_and_is_gone() {
        let start = confetti(0, 150.0, 160.0);
        assert_eq!(start.len(), PIECES as usize);
        assert!(start.iter().all(|b| b.x.abs() <= 4.0 && b.y.abs() < 1e-9));
        let later = confetti(1_500, 150.0, 160.0);
        assert!(later.iter().any(|b| b.x < -60.0) && later.iter().any(|b| b.x > 60.0));
        assert!(later.iter().all(|b| b.y > 0.0 && b.y <= 200.0 && b.x.abs() <= 154.0 && (0.0..=1.0).contains(&b.alpha)), "{later:?}");
        assert!(confetti(CONFETTI_MS - 20, 150.0, 160.0).iter().all(|b| b.alpha < 0.05));
        assert!(confetti(CONFETTI_MS, 150.0, 160.0).is_empty() && confetti(-1, 150.0, 160.0).is_empty());
    }
}
