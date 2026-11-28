//! Which file a screenshot's notification is about, so that its card can send it, show it or throw it away: the one
//! the notification names, else the newest picture of the screenshot folder.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A picture saved this long before the notification is the one it speaks of.
const FRESH: Duration = Duration::from_secs(15);

fn is_picture(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp"))
}

/// A path or a `file://` address, when it is a picture that is there.
fn named(text: &str) -> Option<PathBuf> {
    let path = PathBuf::from(text.strip_prefix("file://").map_or_else(|| text.to_string(), super::art::percent_decode));
    (path.is_absolute() && is_picture(&path) && path.is_file()).then_some(path)
}

/// The picture of a folder changed last, if that was less than `FRESH` ago.
fn newest(dir: &Path, now: SystemTime) -> Option<(SystemTime, PathBuf)> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| is_picture(&e.path()))
        .filter_map(|e| Some((e.metadata().ok().filter(|m| m.is_file())?.modified().ok()?, e.path())))
        .filter(|(at, _)| now.duration_since(*at).map_or(true, |age| age <= FRESH))
        .max_by_key(|(at, _)| *at)
}

/// The folder of a path as niri's settings write it: `screenshot-path "~/Pictures/Screenshots/Screenshot from %Y.png"`.
fn niri_folder(config: &str, home: &Path) -> Option<PathBuf> {
    let line = config.lines().map(str::trim).find_map(|l| l.strip_prefix("screenshot-path"))?;
    let path = line.split('"').nth(1)?;
    let path = path.strip_prefix("~/").map_or_else(|| PathBuf::from(path), |rest| home.join(rest));
    // a folder named after the date cannot be told: the part before it can
    path.ancestors().skip(1).find(|a| !a.to_string_lossy().contains('%')).map(Path::to_path_buf)
}

fn folders() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from) else { return vec![] };
    let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
    let pictures = home.join("Pictures");
    let mut all: Vec<PathBuf> = std::fs::read_to_string(config.join("niri/config.kdl")).ok().and_then(|t| niri_folder(&t, &home)).into_iter().collect();
    all.extend([pictures.join("Screenshots"), pictures]);
    all
}

/// The file of the notification `summary`, when it is a screenshot's: what it names (its image, its icon), else the
/// picture just saved.
pub fn file_of(summary: &str, image_path: Option<&str>, app_icon: &str) -> Option<String> {
    if !island_core::ocr::is_screenshot(summary) {
        return None;
    }
    let path = image_path
        .and_then(named)
        .or_else(|| named(app_icon))
        .or_else(|| folders().iter().filter_map(|d| newest(d, SystemTime::now())).max_by_key(|(at, _)| *at).map(|(_, p)| p))?;
    Some(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn niris_screenshot_folder_is_read_from_its_settings() {
        let home = Path::new("/home/u");
        assert_eq!(niri_folder("input {}\nscreenshot-path \"~/Pictures/Shots/Screenshot from %Y-%m-%d.png\"\n", home), Some(home.join("Pictures/Shots")));
        assert_eq!(niri_folder("screenshot-path \"/data/%Y/%m-%d.png\"", home), Some(PathBuf::from("/data")));
        assert_eq!(niri_folder("screenshot-path null\n// screenshot-path \"~/x/a.png\"", home), None);
    }

    #[test]
    fn the_file_is_the_one_named_or_the_picture_just_saved() {
        let dir = std::env::temp_dir().join(format!("oi-shots-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (old, new, note) = (dir.join("a.png"), dir.join("b.PNG"), dir.join("c.txt"));
        for f in [&old, &new, &note] {
            std::fs::write(f, "x").unwrap();
        }
        std::fs::File::options().write(true).open(&old).unwrap().set_modified(SystemTime::now() - Duration::from_secs(3600)).unwrap();
        assert_eq!(newest(&dir, SystemTime::now()).map(|(_, p)| p), Some(new.clone()), "not the old one, nor what is no picture");
        assert_eq!(newest(&dir, SystemTime::now() + Duration::from_secs(60)), None, "nothing saved just now");
        assert_eq!(named(&old.to_string_lossy()), Some(old.clone()));
        assert_eq!(named(&format!("file://{}", old.display())), Some(old));
        assert_eq!((named(&note.to_string_lossy()), named("a.png"), named("camera-photo")), (None, None, None));
        assert_eq!(file_of("New message", Some(&new.to_string_lossy()), ""), None, "only for a screenshot");
        assert_eq!(file_of("Screenshot captured", Some(&new.to_string_lossy()), ""), Some(new.to_string_lossy().into_owned()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
