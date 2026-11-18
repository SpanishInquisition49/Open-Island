//! Colours: contrast helpers for the album-art tint.

pub type Rgb = (u8, u8, u8);

pub fn parse_hex(hex: &str) -> Option<Rgb> {
    let h = hex.strip_prefix('#').unwrap_or(hex);
    if h.len() < 6 || !h.is_char_boundary(6) {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some((p(0)?, p(2)?, p(4)?))
}

pub fn to_hex((r, g, b): Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Relative luminance (0 black .. 1 white), sRGB.
pub fn luminance((r, g, b): Rgb) -> f64 {
    let lin = |c: u8| {
        let c = c as f64 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// Mixes `a` toward `b` by t (0 = a, 1 = b).
pub fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let m = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round().clamp(0.0, 255.0) as u8;
    (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// Album-art tint made readable on the island: on a dark background a dark colour is lightened, on a
/// light one a light colour is darkened, until it clears about 3:1 contrast.
pub fn readable_tint(c: Rgb, dark_background: bool) -> Rgb {
    let target = if dark_background { (255, 255, 255) } else { (0, 0, 0) };
    let ok = |l: f64| if dark_background { l >= 0.1 } else { l <= 0.25 };
    let (mut out, mut t) = (c, 0.0);
    while !ok(luminance(out)) && t < 1.0 {
        t += 0.1;
        out = mix(c, target, t);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex() {
        assert_eq!(parse_hex("#ff8000"), Some((255, 128, 0)));
        assert_eq!(parse_hex("00ff00"), Some((0, 255, 0)));
        assert_eq!(parse_hex("#ff"), None);
        assert_eq!(parse_hex("#gg0000"), None);
        assert_eq!(to_hex((1, 2, 255)), "#0102ff");
    }

    #[test]
    fn tint() {
        let bright = (0xe0, 0x40, 0x40);
        assert_eq!(readable_tint(bright, true), bright);
        assert!(luminance(readable_tint((0x20, 0x20, 0x20), true)) >= 0.1);
        assert!(luminance(readable_tint((0xf0, 0xe8, 0xd0), false)) <= 0.25);
        assert_eq!(readable_tint((0x20, 0x20, 0x20), false), (0x20, 0x20, 0x20));
    }
}
