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
type Job = (u8, i64, i64, String);
static QUEUE: OnceLock<Sender<Job>> = OnceLock::new();
const KEEP: Duration = Duration::from_secs(14 * 24 * 3600);

fn cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from)).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("open-island").join("tiles"))
}

/// A cache file name for a tile of a template: different map styles do not mix.
pub fn cache_name(template: &str, z: u8, x: i64, y: i64) -> String {
    let style: u64 = template.bytes().fold(1469598103934665603, |h, b| (h ^ b as u64).wrapping_mul(1099511628211));
    format!("{style:016x}/{z}/{x}/{y}.png")
}

fn decode(bytes: &[u8]) -> Option<Image> {
    let rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
    Some(Image { id: NEXT_ID.fetch_add(1, Ordering::Relaxed), width: rgba.width(), height: rgba.height(), rgba: Arc::new(rgba.into_raw()) })
}

fn fetch(template: &str, z: u8, x: i64, y: i64) -> Option<Vec<u8>> {
    let file = cache_dir().map(|d| d.join(cache_name(template, z, x, y)));
    if let Some(f) = &file {
        if let Ok(meta) = std::fs::metadata(f) {
            if meta.modified().ok().and_then(|m| SystemTime::now().duration_since(m).ok()).is_some_and(|age| age < KEEP) {
                if let Ok(b) = std::fs::read(f) {
                    return Some(b);
                }
            }
        }
    }
    let url = island_core::map::tile_url(template, z, x, y);
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return None;
    }
    let mut buf = Vec::new();
    net::agent().get(&url).call().ok()?.body_mut().as_reader().take(2 * 1024 * 1024).read_to_end(&mut buf).ok()?;
    // only a picture is kept
    decode(&buf)?;
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
        let Some((z, x, y, template)) = job else { return };
        match fetch(&template, z, x, y).and_then(|b| decode(&b)) {
            Some(image) => rep.input(Input::Tile { z, x, y, image }),
            None => log::debug!("tile {z}/{x}/{y} not loaded"),
        }
    }
}

pub fn request(rep: &Reporter, z: u8, x: i64, y: i64, template: String) {
    let tx = QUEUE.get_or_init(|| {
        let (tx, rx) = channel();
        let rx = Arc::new(Mutex::new(rx));
        for _ in 0..3 {
            let (rep, rx) = (rep.clone(), rx.clone());
            std::thread::spawn(move || worker(rep, rx));
        }
        tx
    });
    let _ = tx.send((z, x, y, template));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_have_their_own_cache_and_tiles_their_own_files() {
        let a = cache_name("https://a/{z}/{x}/{y}.png", 14, 8, 9);
        let b = cache_name("https://b/{z}/{x}/{y}.png", 14, 8, 9);
        assert_ne!(a.split('/').next(), b.split('/').next());
        assert!(a.ends_with("/14/8/9.png"));
        assert_eq!(a, cache_name("https://a/{z}/{x}/{y}.png", 14, 8, 9));
    }

    #[test]
    fn a_png_decodes_and_garbage_does_not() {
        let mut png = vec![];
        image::DynamicImage::new_rgba8(4, 4).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let img = decode(&png).unwrap();
        assert_eq!((img.width, img.height, img.rgba.len()), (4, 4, 64));
        assert!(decode(b"<html>nope</html>").is_none());
    }

    #[test]
    #[ignore = "needs the network: cargo test -- --ignored live_tile"]
    fn live_tile() {
        let t = fetch("https://a.basemaps.cartocdn.com/dark_all/{z}/{x}/{y}@2x.png", 14, 8668, 6017).expect("a tile");
        let img = decode(&t).unwrap();
        eprintln!("tile {}x{}", img.width, img.height);
        assert!(img.width == 512 || img.width == 256);
    }
}
