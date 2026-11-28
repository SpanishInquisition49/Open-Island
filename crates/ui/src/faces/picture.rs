//! How a picture is handed to the renderer, which knows each one by a number and sends to the graphics card only those
//! whose number is new to it. The same pixels must keep their number from a frame to the next: a new one at every draw
//! had every cover and every map tile sent again sixty times a second.

use crate::model::Image;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

/// Entries kept before those of pictures that are gone are looked for.
const MANY: usize = 128;

/// The number given to each picture, by where its pixels are. Holding them weakly keeps their place from being taken by
/// others while the entry is there, without keeping the pixels.
type Numbers = HashMap<usize, (Weak<Vec<u8>>, u64)>;

thread_local! {
    static NUMBERS: RefCell<Numbers> = RefCell::default();
}

fn blob(pixels: &Arc<Vec<u8>>) -> Blob<u8> {
    let data = pixels.clone() as Arc<dyn AsRef<[u8]> + Send + Sync>;
    NUMBERS.with_borrow_mut(|numbers| {
        let place = Arc::as_ptr(pixels) as usize;
        if let Some((_, number)) = numbers.get(&place) {
            return Blob::from_raw_parts(data, *number);
        }
        if numbers.len() >= MANY {
            numbers.retain(|_, (held, _)| held.strong_count() > 0);
        }
        let new = Blob::new(data);
        numbers.insert(place, (Arc::downgrade(pixels), new.id()));
        new
    })
}

/// The picture as the renderer takes it.
pub(super) fn data(img: &Image) -> ImageData {
    ImageData { data: blob(&img.rgba), format: ImageFormat::Rgba8, alpha_type: ImageAlphaType::Alpha, width: img.width, height: img.height }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(shade: u8) -> Image {
        Image { id: 1, width: 1, height: 1, rgba: Arc::new(vec![shade; 4]) }
    }

    #[test]
    fn the_same_pixels_keep_their_number_and_others_get_their_own() {
        let (a, b) = (picture(1), picture(1));
        let first = data(&a).data.id();
        assert_eq!(data(&a).data.id(), first);
        assert_eq!(data(&a.clone()).data.id(), first, "a copy of the picture shares its pixels");
        assert_ne!(data(&b).data.id(), first, "other pixels, even if alike (and with the same `id` of the island's)");
        assert_eq!(data(&a).data.data(), [1, 1, 1, 1]);
    }

    #[test]
    fn pictures_that_are_gone_are_forgotten_and_their_pixels_not_kept() {
        let kept = picture(2);
        let number = data(&kept).data.id();
        let gone = picture(3);
        let watch = Arc::downgrade(&gone.rgba);
        data(&gone);
        drop(gone);
        assert!(watch.upgrade().is_none(), "the table does not hold the pixels");
        for n in 0..3 * MANY {
            data(&picture(n as u8));
        }
        assert!(NUMBERS.with_borrow(|n| n.len()) <= MANY);
        assert_eq!(data(&kept).data.id(), number, "one still in use stays");
    }
}
