//! Slippy-map arithmetic (Web Mercator, 256-pixel tiles) and distances. No I/O: the app fetches the tiles.

/// OpenStreetMap's own tiles: no key, light style (the island darkens them itself). Other providers want a key.
pub const DEFAULT_TILES: &str = "https://tile.openstreetmap.org/{z}/{x}/{y}.png";
/// The credit the tile provider asks for.
pub const CREDIT: &str = "© OpenStreetMap contributors";
pub const TILE: f64 = 256.0;
pub const MIN_ZOOM: u8 = 3;
pub const MAX_ZOOM: u8 = 18;

/// A point on the world at a zoom, in pixels from the top left of the whole map.
pub fn world_px(lat: f64, lon: f64, zoom: u8) -> (f64, f64) {
    let n = TILE * (1u64 << zoom) as f64;
    let lat = lat.clamp(-85.0511, 85.0511).to_radians();
    let x = (lon + 180.0) / 360.0 * n;
    let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0 * n;
    (x, y)
}

/// The tile that holds a world pixel, and where in the view it lands: the view is `w` x `h` pixels centred on `center`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub z: u8,
    pub x: i64,
    pub y: i64,
    /// Top left of the tile in the view, in pixels.
    pub px: f64,
    pub py: f64,
}

/// The tiles that cover a view (the ones off the world's top and bottom are left out; x wraps around).
pub fn tiles(center_lat: f64, center_lon: f64, zoom: u8, w: f64, h: f64) -> Vec<Placed> {
    let (cx, cy) = world_px(center_lat, center_lon, zoom);
    let (left, top) = (cx - w / 2.0, cy - h / 2.0);
    let n = 1i64 << zoom;
    let (x0, x1) = ((left / TILE).floor() as i64, ((left + w) / TILE).floor() as i64);
    let (y0, y1) = ((top / TILE).floor() as i64, ((top + h) / TILE).floor() as i64);
    let mut out = vec![];
    for ty in y0..=y1 {
        if ty < 0 || ty >= n {
            continue;
        }
        for tx in x0..=x1 {
            out.push(Placed { z: zoom, x: tx.rem_euclid(n), y: ty, px: tx as f64 * TILE - left, py: ty as f64 * TILE - top });
        }
    }
    out
}

/// Where a point lands in a view centred on `center`.
pub fn project(center: (f64, f64), zoom: u8, w: f64, h: f64, lat: f64, lon: f64) -> (f64, f64) {
    let (cx, cy) = world_px(center.0, center.1, zoom);
    let (px, py) = world_px(lat, lon, zoom);
    (px - cx + w / 2.0, py - cy + h / 2.0)
}

/// The highest zoom at which both points fit in the view with `margin` pixels around.
pub fn zoom_to_fit(a: (f64, f64), b: (f64, f64), w: f64, h: f64, margin: f64) -> u8 {
    for z in (MIN_ZOOM..=16).rev() {
        let (ax, ay) = world_px(a.0, a.1, z);
        let (bx, by) = world_px(b.0, b.1, z);
        if (ax - bx).abs() <= w - 2.0 * margin && (ay - by).abs() <= h - 2.0 * margin {
            return z;
        }
    }
    MIN_ZOOM
}

/// The middle of two points (good enough at city scale).
pub fn midpoint(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
}

/// Metres between two points.
pub fn distance_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dlat, dlon) = ((b.0 - a.0).to_radians(), (b.1 - a.1).to_radians());
    let h = (dlat / 2.0).sin().powi(2) + a.0.to_radians().cos() * b.0.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    2.0 * 6_371_000.0 * h.sqrt().asin()
}

/// `450 m`, `1.2 km`, `38 km`.
pub fn fmt_distance(m: f64) -> String {
    if m < 1000.0 {
        format!("{} m", (m / 10.0).round() as i64 * 10)
    } else if m < 10_000.0 {
        format!("{:.1} km", m / 1000.0)
    } else {
        format!("{} km", (m / 1000.0).round() as i64)
    }
}

/// Pixels that `metres` take at a latitude and zoom (for the accuracy circle).
pub fn metres_to_px(metres: f64, lat: f64, zoom: u8) -> f64 {
    let m_per_px = 156_543.033_92 * lat.to_radians().cos() / (1u64 << zoom) as f64;
    metres / m_per_px
}

/// A light map pixel as a dark one: invert, then turn the hue back half way round so water stays blue and parks stay
/// green (the CSS `invert(1) hue-rotate(180deg)` trick), a little dimmer so it sits well on the black island.
pub fn darken(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let (r, g, b) = (255.0 - r as f32, 255.0 - g as f32, 255.0 - b as f32);
    let m = [[-0.574, 1.430, 0.144], [0.426, 0.430, 0.144], [0.426, 1.430, -0.856]];
    let out = |row: [f32; 3]| ((row[0] * r + row[1] * g + row[2] * b) * 0.92).clamp(0.0, 255.0).round() as u8;
    (out(m[0]), out(m[1]), out(m[2]))
}

/// A tile address filled into a URL template with `{z}`, `{x}`, `{y}`.
pub fn tile_url(template: &str, z: u8, x: i64, y: i64, key: &str) -> String {
    template.replace("{z}", &z.to_string()).replace("{x}", &x.to_string()).replace("{y}", &y.to_string()).replace("{key}", key)
}

/// Whether a tile URL template wants the API key.
pub fn needs_key(template: &str) -> bool {
    template.contains("{key}")
}

/// A template the island accepts: https, with the three tile fields.
pub fn valid_template(t: &str) -> bool {
    t.starts_with("https://") && t.contains("{z}") && t.contains("{x}") && t.contains("{y}")
}

/// OpenStreetMap's directions page between two points; `engine` is `car`, `foot` or `bike`.
pub fn directions_url(engine: &str, from: (f64, f64), to: (f64, f64)) -> String {
    format!("https://www.openstreetmap.org/directions?engine=fossgis_osrm_{engine}&route={:.6}%2C{:.6}%3B{:.6}%2C{:.6}", from.0, from.1, to.0, to.1)
}

/// The zoom to start from for a fix with this much doubt: close for a precise one, wide for a GeoIP guess.
pub fn zoom_for_accuracy(accuracy_m: f64) -> u8 {
    match accuracy_m {
        a if a > 20_000.0 => 10,
        a if a > 5_000.0 => 12,
        a if a > 1_500.0 => 13,
        a if a > 500.0 => 14,
        _ => 16,
    }
}

/// The OpenStreetMap page for a point (for "open in the browser").
pub fn osm_url(lat: f64, lon: f64, zoom: u8) -> String {
    format!("https://www.openstreetmap.org/?mlat={lat:.6}&mlon={lon:.6}#map={zoom}/{lat:.6}/{lon:.6}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_equator_and_the_meridian_are_the_middle_of_the_world() {
        let (x, y) = world_px(0.0, 0.0, 1);
        assert!((x - 256.0).abs() < 1e-6 && (y - 256.0).abs() < 1e-6, "{x} {y}");
        let (x, _) = world_px(0.0, -180.0, 3);
        assert!(x.abs() < 1e-9);
    }

    #[test]
    fn pisa_falls_in_the_known_tile() {
        // Pisa at zoom 10 is tile 540/374 (checked against the OSM tile grid)
        let p = world_px(43.7228, 10.4017, 10);
        assert_eq!(((p.0 / TILE) as i64, (p.1 / TILE) as i64), (541, 373));
    }

    #[test]
    fn a_view_is_covered_by_whole_tiles_and_the_centre_is_in_the_middle() {
        let ts = tiles(43.72, 10.40, 14, 400.0, 220.0);
        assert!(ts.len() >= 4 && ts.len() <= 9);
        let (cx, cy) = project((43.72, 10.40), 14, 400.0, 220.0, 43.72, 10.40);
        assert_eq!((cx, cy), (200.0, 110.0));
        // the tile that holds the centre has the centre inside it
        assert!(ts.iter().any(|t| t.px <= 200.0 && 200.0 < t.px + TILE && t.py <= 110.0 && 110.0 < t.py + TILE));
        assert!(ts.iter().all(|t| t.x >= 0 && t.x < (1 << 14) && t.y >= 0 && t.y < (1 << 14)));
    }

    #[test]
    fn tiles_wrap_around_the_date_line_and_stop_at_the_poles() {
        let ts = tiles(0.0, 179.9, 2, 600.0, 200.0);
        assert!(ts.iter().any(|t| t.x == 0) && ts.iter().any(|t| t.x == 3), "x wraps");
        let polar = tiles(85.0, 0.0, 2, 300.0, 900.0);
        assert!(polar.iter().all(|t| t.y >= 0 && t.y < 4));
    }

    #[test]
    fn the_zoom_fits_both_points() {
        let a = (43.7228, 10.4017); // Pisa
        let b = (43.7764, 11.2486); // Firenze
        let z = zoom_to_fit(a, b, 392.0, 200.0, 24.0);
        let (ax, _) = world_px(a.0, a.1, z);
        let (bx, _) = world_px(b.0, b.1, z);
        assert!((ax - bx).abs() <= 392.0 - 48.0 && z >= 8, "zoom {z}");
        assert_eq!(zoom_to_fit(a, a, 392.0, 200.0, 24.0), 16, "the same place: as close as it goes");
        assert_eq!(zoom_to_fit((0.0, -170.0), (0.0, 170.0), 392.0, 200.0, 24.0), MIN_ZOOM);
    }

    #[test]
    fn distances_read_well() {
        let d = distance_m((43.7228, 10.4017), (43.7764, 11.2486));
        assert!((d - 69_000.0).abs() < 2_000.0, "{d}");
        assert_eq!(distance_m((1.0, 1.0), (1.0, 1.0)), 0.0);
        assert_eq!(fmt_distance(437.0), "440 m");
        assert_eq!(fmt_distance(1234.0), "1.2 km");
        assert_eq!(fmt_distance(38_400.0), "38 km");
        let px = metres_to_px(100.0, 43.7, 16);
        assert!((px - 100.0 / (156_543.033_92 * 43.7f64.to_radians().cos() / 65_536.0)).abs() < 1e-9);
    }

    #[test]
    fn directions_and_the_starting_zoom() {
        assert_eq!(directions_url("foot", (43.7, 10.4), (43.72, 10.4)), "https://www.openstreetmap.org/directions?engine=fossgis_osrm_foot&route=43.700000%2C10.400000%3B43.720000%2C10.400000");
        assert_eq!((zoom_for_accuracy(25_000.0), zoom_for_accuracy(1_120.0), zoom_for_accuracy(30.0)), (10, 14, 16));
    }

    #[test]
    fn a_light_map_becomes_a_dark_one_that_keeps_its_colours() {
        let lum = |(r, g, b): (u8, u8, u8)| 0.3 * r as f32 + 0.59 * g as f32 + 0.11 * b as f32;
        let land = darken(242, 239, 233); // OSM land
        let water = darken(170, 211, 223); // OSM water
        let park = darken(200, 250, 204); // OSM park
        let road = darken(255, 255, 255);
        assert!(lum(land) < 40.0, "land is dark: {land:?}");
        assert!(lum(road) < 12.0, "white becomes black: {road:?}");
        assert!(water.2 > water.0, "water stays blue: {water:?}");
        assert!(park.1 > park.0 && park.1 > park.2, "parks stay green: {park:?}");
        assert!(lum(darken(0, 0, 0)) > 200.0, "black map text becomes light, so it reads on the dark ground");
    }

    #[test]
    fn urls_are_filled_in() {
        assert_eq!(tile_url("https://t/{z}/{x}/{y}.png", 3, 4, 5, ""), "https://t/3/4/5.png");
        assert_eq!(tile_url("https://t/{z}/{x}/{y}.png?key={key}", 3, 4, 5, "abc"), "https://t/3/4/5.png?key=abc");
        assert!(needs_key("https://t/{z}/{x}/{y}.png?key={key}") && !needs_key(DEFAULT_TILES));
        assert!(valid_template(DEFAULT_TILES) && !valid_template("http://t/{z}/{x}/{y}") && !valid_template("https://t/{z}/{x}"));
        assert!(osm_url(43.7, 10.4, 15).ends_with("#map=15/43.700000/10.400000"));
    }
}
