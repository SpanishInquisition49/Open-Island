//! The picture of a user, for the password request: the one the login screens show (AccountsService), and for the person
//! running the island also `~/.face`.

use island_ui::model::Image;
use std::io::Read;
use std::path::{Path, PathBuf};

const ACCOUNTS: &str = "/var/lib/AccountsService/icons";
/// Larger files are not read, nor larger pictures decoded.
const MOST_BYTES: u64 = 8 * 1024 * 1024;
const MOST_SIDE: u32 = 4096;

/// Where the picture of `user` may be, the first that counts first. `home` is given for the person's own user only.
fn places(accounts: &Path, user: &str, home: Option<&Path>) -> Vec<PathBuf> {
    // a user name is one part of a path, never more
    if user.is_empty() || user.contains('/') || user.starts_with('.') {
        return vec![];
    }
    let mut out = vec![accounts.join(user)];
    out.extend(home.into_iter().flat_map(|h| [h.join(".face"), h.join(".face.icon")]));
    out
}

/// A PNG or JPEG file of a reasonable size, decoded small. Anything else (a pipe, a huge picture, another format) is
/// no picture.
fn read(path: &Path) -> Option<Image> {
    // looked at before it is opened: opening a pipe would wait for a writer
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MOST_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path).ok()?.take(MOST_BYTES).read_to_end(&mut bytes).ok()?;
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().ok()?.into_dimensions().ok()?;
    if w.max(h) > MOST_SIDE {
        return None;
    }
    super::art::decode(&bytes)
}

fn first(places: &[PathBuf]) -> Option<Image> {
    places.iter().find_map(|p| read(p))
}

/// The picture of a user, if there is one.
pub fn of(user: &str, own: bool) -> Option<Image> {
    let home = own.then(|| std::env::var_os("HOME").map(PathBuf::from)).flatten();
    first(&places(Path::new(ACCOUNTS), user, home.as_deref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(path: &Path, side: u32) {
        let img = image::RgbaImage::from_pixel(side, side, image::Rgba([200, 40, 40, 255]));
        img.save_with_format(path, image::ImageFormat::Png).unwrap();
    }

    #[test]
    fn the_login_screens_picture_comes_first_then_the_ones_in_the_home() {
        let dir = std::env::temp_dir().join(format!("oi-avatar-{}", std::process::id()));
        let (accounts, home) = (dir.join("accounts"), dir.join("home"));
        std::fs::create_dir_all(&accounts).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let at = |home: Option<&Path>| first(&places(&accounts, "me", home));
        assert!(at(Some(&home)).is_none());
        png(&home.join(".face"), 40);
        assert_eq!(at(Some(&home)).map(|i| i.width), Some(40));
        assert!(at(None).is_none(), "another user's home is not looked into");
        png(&accounts.join("me"), 400);
        assert_eq!(at(Some(&home)).map(|i| (i.width, i.height)), Some((160, 160)), "made small");
        // what is not a picture is passed over, as is one too large to decode
        std::fs::write(accounts.join("me"), b"not a picture").unwrap();
        assert_eq!(at(Some(&home)).map(|i| i.width), Some(40));
        png(&accounts.join("me"), MOST_SIDE + 1);
        assert_eq!(at(Some(&home)).map(|i| i.width), Some(40));
        // a folder in its place is no picture either
        std::fs::remove_file(accounts.join("me")).unwrap();
        std::fs::create_dir(accounts.join("me")).unwrap();
        assert_eq!(at(Some(&home)).map(|i| i.width), Some(40));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_name_cannot_lead_elsewhere() {
        for name in ["", "../etc/passwd", "a/b", ".hidden"] {
            assert!(places(Path::new("/x"), name, None).is_empty(), "{name}");
        }
    }
}
