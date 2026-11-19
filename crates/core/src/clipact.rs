//! What can be done with a text that was just copied: open a link, write to an address, show a place on the map, see a colour.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Link,
    Email,
    Place,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Act {
    pub kind: Kind,
    /// What to open: an https link or a mailto link.
    pub target: String,
}

fn encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn is_email(t: &str) -> bool {
    let Some((user, host)) = t.split_once('@') else { return false };
    !user.is_empty()
        && user.chars().all(|c| c.is_ascii_alphanumeric() || "._%+-".contains(c))
        && host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && host.chars().all(|c| c.is_ascii_alphanumeric() || ".-".contains(c))
}

/// `43.7,10.4` or `43.7 10.4`: a pair of coordinates.
fn coordinates(t: &str) -> Option<(f64, f64)> {
    let (a, b) = t.split_once(',').or_else(|| t.split_once(' '))?;
    let (lat, lon) = (a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?);
    (a.contains('.') && b.contains('.') && lat.abs() <= 90.0 && lon.abs() <= 180.0).then_some((lat, lon))
}

/// A street address: a street word and a number, on one short line.
fn is_address(t: &str) -> bool {
    const STREETS: &[&str] = &["via", "viale", "piazza", "corso", "largo", "vicolo", "lungarno", "street", "st.", "road", "rd.", "avenue", "ave", "strasse", "straße", "rue", "calle"];
    if t.contains('\n') || t.len() > 90 || t.len() < 6 {
        return false;
    }
    let lower = t.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).collect();
    words.len() >= 2 && words.iter().any(|w| STREETS.contains(w)) && words.iter().any(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))
}

/// The action for a copied text, when it is one of the things worth one.
pub fn classify(text: &str) -> Option<Act> {
    let t = text.trim();
    if t.is_empty() || t.len() > 300 {
        return None;
    }
    if (t.starts_with("https://") || t.starts_with("http://")) && !t.contains(char::is_whitespace) && t.len() > 10 {
        return Some(Act { kind: Kind::Link, target: t.into() });
    }
    if is_email(t) {
        return Some(Act { kind: Kind::Email, target: format!("mailto:{t}") });
    }
    if let Some((lat, lon)) = coordinates(t) {
        return Some(Act { kind: Kind::Place, target: format!("https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map=17/{lat}/{lon}") });
    }
    if is_address(t) {
        return Some(Act { kind: Kind::Place, target: format!("https://www.openstreetmap.org/search?query={}", encode(t)) });
    }
    None
}

/// A colour written as `#rgb` or `#rrggbb`.
pub fn color(text: &str) -> Option<(u8, u8, u8)> {
    let h = text.trim().strip_prefix('#')?;
    if !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = |s: &str| u8::from_str_radix(s, 16).ok();
    match h.len() {
        3 => Some((v(&h[0..1])? * 17, v(&h[1..2])? * 17, v(&h[2..3])? * 17)),
        6 => Some((v(&h[0..2])?, v(&h[2..4])?, v(&h[4..6])?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_and_emails_open() {
        assert_eq!(classify("https://example.org/a?b=1").map(|a| a.kind), Some(Kind::Link));
        assert_eq!(classify(" me@uni.it\n"), Some(Act { kind: Kind::Email, target: "mailto:me@uni.it".into() }));
        assert!(classify("see https://example.org for more").is_none(), "a sentence is not a link");
        assert!(classify("not@anemail").is_none() && classify("@x.it").is_none());
        assert!(classify("ftp://x.org/f").is_none());
    }

    #[test]
    fn places_open_on_the_map() {
        let a = classify("Via Santa Maria 36, Pisa").unwrap();
        assert_eq!(a.kind, Kind::Place);
        assert_eq!(a.target, "https://www.openstreetmap.org/search?query=Via%20Santa%20Maria%2036%2C%20Pisa");
        assert!(classify("43.7167, 10.4").unwrap().target.contains("mlat=43.7167&mlon=10.4"));
        assert!(classify("Via della libertà").is_none(), "no number, no address");
        assert!(classify("3.14, 2.71").is_some() && classify("1, 2").is_none(), "coordinates have decimals");
        assert!(classify("95.0, 10.0").is_none());
    }

    #[test]
    fn colours_are_read() {
        assert_eq!(color("#ff8000"), Some((255, 128, 0)));
        assert_eq!(color("#f80"), Some((255, 136, 0)));
        assert_eq!(color("ff8000"), None);
        assert_eq!(color("#ggg"), None);
        assert_eq!(color("#ff80"), None);
    }
}
