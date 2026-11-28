//! Map tiles: fetched once, kept on disk for two weeks, decoded to RGBA and handed to the island. A few workers share
//! the queue so a view of nine tiles arrives together without opening nine connections at once.

use super::{net, Reporter};
use island_ui::model::{Image, Input};
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

static NEXT_ID: AtomicU64 = AtomicU64::new(1 << 40);
/// A tile to load: zoom, x, y and the URL template.
type Job = (u8, i64, i64, String, bool);
static QUEUE: OnceLock<Sender<Job>> = OnceLock::new();
const KEEP: Duration = Duration::from_secs(14 * 24 * 3600);

fn cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from)).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("open-island").join("tiles"))
}

/// A cache file name for a tile of a template: different map styles do not mix.
pub fn cache_name(template: &str, key: &str, z: u8, x: i64, y: i64) -> String {
    // the key is part of the style: a tile fetched without the key (a watermark) is not the one fetched with it
    let style: u64 = template.bytes().chain(std::iter::once(0)).chain(key.bytes()).fold(1469598103934665603, |h, b| (h ^ b as u64).wrapping_mul(1099511628211));
    format!("{style:016x}/{z}/{x}/{y}.png")
}

fn decode(bytes: &[u8], dark: bool) -> Option<Image> {
    let mut rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
    if dark {
        for px in rgba.pixels_mut() {
            let (r, g, b) = island_core::map::darken(px[0], px[1], px[2]);
            px.0 = [r, g, b, px[3]];
        }
    }
    Some(Image { id: NEXT_ID.fetch_add(1, Ordering::Relaxed), width: rgba.width(), height: rgba.height(), rgba: Arc::new(rgba.into_raw()) })
}

fn fetch(template: &str, key: &str, z: u8, x: i64, y: i64) -> Option<Vec<u8>> {
    if island_core::map::needs_key(template) && key.is_empty() {
        return None;
    }
    let file = cache_dir().map(|d| d.join(cache_name(template, key, z, x, y)));
    if let Some(f) = &file {
        if let Ok(meta) = std::fs::metadata(f) {
            if meta.modified().ok().and_then(|m| SystemTime::now().duration_since(m).ok()).is_some_and(|age| age < KEEP) {
                if let Ok(b) = std::fs::read(f) {
                    return Some(b);
                }
            }
        }
    }
    let url = island_core::map::tile_url(template, z, x, y, key);
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return None;
    }
    let mut buf = Vec::new();
    net::agent().get(&url).call().ok()?.body_mut().as_reader().take(2 * 1024 * 1024).read_to_end(&mut buf).ok()?;
    // only a picture is kept
    decode(&buf, false)?;
    if let Some(f) = file {
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(f, &buf);
    }
    Some(buf)
}

fn worker(rep: Reporter, rx: Arc<Mutex<Receiver<Job>>>) {
    loop {
        let job = rx.lock().ok().and_then(|r| r.recv().ok());
        let Some((z, x, y, template, dark)) = job else { return };
        let s = crate::config::load();
        let key = if s.map_api_key.is_empty() && s.map_key_in_keyring && island_core::map::needs_key(&template) {
            crate::services::secrets::cached(crate::services::secrets::MAP_KEY).unwrap_or_default()
        } else {
            s.map_api_key
        };
        match fetch(&template, &key, z, x, y).and_then(|b| decode(&b, dark)) {
            Some(image) => rep.input(Input::Tile { z, x, y, image }),
            None => log::debug!("tile {z}/{x}/{y} not loaded"),
        }
    }
}

pub fn request(rep: &Reporter, z: u8, x: i64, y: i64, template: String, dark: bool) {
    let tx = QUEUE.get_or_init(|| {
        let (tx, rx) = channel();
        let rx = Arc::new(Mutex::new(rx));
        for _ in 0..3 {
            let (rep, rx) = (rep.clone(), rx.clone());
            std::thread::spawn(move || worker(rep, rx));
        }
        tx
    });
    let _ = tx.send((z, x, y, template, dark));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_have_their_own_cache_and_tiles_their_own_files() {
        let a = cache_name("https://a/{z}/{x}/{y}.png", "", 14, 8, 9);
        let b = cache_name("https://b/{z}/{x}/{y}.png", "", 14, 8, 9);
        assert_ne!(a.split('/').next(), b.split('/').next());
        assert!(a.ends_with("/14/8/9.png"));
        assert_eq!(a, cache_name("https://a/{z}/{x}/{y}.png", "", 14, 8, 9));
        assert_ne!(a.split('/').next(), cache_name("https://a/{z}/{x}/{y}.png", "k1", 14, 8, 9).split('/').next(), "another key, another cache");
    }

    #[test]
    fn a_png_decodes_and_garbage_does_not() {
        let mut png = vec![];
        image::DynamicImage::new_rgba8(4, 4).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let img = decode(&png, false).unwrap();
        assert_eq!((img.width, img.height, img.rgba.len()), (4, 4, 64));
        assert!(decode(b"<html>nope</html>", false).is_none());
        let dark = decode(&png, true).unwrap();
        assert_ne!(dark.rgba[..4], img.rgba[..4], "a dark tile is not the light one");
    }

    #[test]
    #[ignore = "needs the network: cargo test -- --ignored live_presets"]
    fn live_presets() {
        // every key-less preset answers with a picture for a tile of Pisa
        for (name, template) in island_core::map::PRESETS.iter().filter(|(_, t)| !island_core::map::needs_key(t)) {
            let bytes = fetch(template, "", 15, 17330, 11950).unwrap_or_else(|| panic!("{name}: no tile"));
            let img = decode(&bytes, false).unwrap_or_else(|| panic!("{name}: not a picture"));
            eprintln!("{name}: {}x{}", img.width, img.height);
        }
    }

    #[test]
    #[ignore = "needs the network: cargo test -- --ignored live_tile"]
    fn live_tile() {
        let t = fetch(island_core::map::DEFAULT_TILES, "", 14, 8668, 6017).expect("a tile");
        let img = decode(&t, true).unwrap();
        eprintln!("tile {}x{}", img.width, img.height);
        assert!(img.width == 512 || img.width == 256);
        // Pisa, as the island would draw it: a 2 x 2 patch written out to look at
        let (cx, cy) = island_core::map::world_px(43.7228, 10.4017, 15);
        let (tx, ty) = ((cx / 256.0) as i64, (cy / 256.0) as i64);
        let mut out = image::RgbaImage::new(512, 512);
        for (i, (dx, dy)) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
            let t = decode(&fetch(island_core::map::DEFAULT_TILES, "", 15, tx + dx, ty + dy).expect("tile"), true).unwrap();
            let t = image::RgbaImage::from_raw(t.width, t.height, t.rgba.to_vec()).unwrap();
            let t = image::imageops::resize(&t, 256, 256, image::imageops::FilterType::Triangle);
            image::imageops::overlay(&mut out, &t, (i as i64 % 2) * 256, (i as i64 / 2) * 256);
        }
        let _ = out.save(std::env::var("OI_TILE_PNG").unwrap_or_else(|_| "/tmp/claude-1000/pisa-dark.png".into()));
    }
}
