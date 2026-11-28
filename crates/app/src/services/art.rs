//! Cover art: loads an image from a file path or an http(s) URL and decodes it to RGBA8 (at most 160 px).

use island_ui::model::Image;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn load(url: &str) -> Option<Image> {
    let bytes = if let Some(path) = url.strip_prefix("file://") {
        std::fs::read(percent_decode(path)).ok()?
    } else if url.starts_with("http://") || url.starts_with("https://") {
        let mut buf = Vec::new();
        use std::io::Read;
        super::net::agent().get(url).call().ok()?.body_mut().as_reader().take(8 * 1024 * 1024).read_to_end(&mut buf).ok()?;
        buf
    } else {
        std::fs::read(url).ok()?
    };
    decode(&bytes)
}

/// The same pixels under a fresh id (the renderer caches by id).
pub fn reid(img: Image) -> Image {
    Image { id: NEXT_ID.fetch_add(1, Ordering::Relaxed), ..img }
}

pub fn decode(bytes: &[u8]) -> Option<Image> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width().max(img.height()) > 160 { img.resize(160, 160, image::imageops::FilterType::Triangle) } else { img };
    let rgba = img.to_rgba8();
    Some(Image { id: NEXT_ID.fetch_add(1, Ordering::Relaxed), width: rgba.width(), height: rgba.height(), rgba: Arc::new(rgba.into_raw()) })
}

pub(crate) fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_png_and_rejects_garbage() {
        let mut png = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png, 2, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 9, 9, 9, 255]).unwrap();
        }
        let img = decode(&png).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(&img.rgba[0..4], &[255, 0, 0, 255]);
        assert!(decode(b"not an image").is_none());
    }

    #[test]
    fn decodes_file_urls() {
        assert_eq!(percent_decode("/home/a%20b/c.png"), "/home/a b/c.png");
        assert_eq!(percent_decode("100%"), "100%");
    }
}
