//! Browser downloads as activities: watches the download folder for the files a browser writes while it downloads
//! (`.part`, `.crdownload`, ...) and reports each as an activity with what has arrived and how fast; when the real file
//! shows up the activity turns green with its size and duration. Reads file names and sizes only, nothing leaves the
//! machine. Option `watch_downloads`.

use super::Reporter;
use island_core::downloads::{Event, Tracker};
use island_core::progress::{Activity, State};
use island_ui::model::Input;
use std::path::PathBuf;
use std::time::Duration;

/// The folder downloads go to: the XDG one on Linux (`~/.config/user-dirs.dirs`), else `~/Downloads`.
pub fn folder() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)?;
    let cfg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
    if let Ok(text) = std::fs::read_to_string(cfg.join("user-dirs.dirs")) {
        if let Some(p) = parse_user_dirs(&text, &home) {
            return Some(p);
        }
    }
    Some(home.join("Downloads"))
}

/// `XDG_DOWNLOAD_DIR="$HOME/Scaricati"` -> the path.
pub fn parse_user_dirs(text: &str, home: &std::path::Path) -> Option<PathBuf> {
    let line = text.lines().find_map(|l| l.trim().strip_prefix("XDG_DOWNLOAD_DIR="))?;
    let v = line.trim().trim_matches('"');
    let p = match v.strip_prefix("$HOME") {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None => PathBuf::from(v),
    };
    (p != home).then_some(p)
}

fn listing(dir: &std::path::Path) -> Vec<(String, u64)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    rd.flatten().filter_map(|e| Some((e.file_name().to_string_lossy().into_owned(), e.metadata().ok().filter(|m| m.is_file())?.len()))).collect()
}

pub fn start(rep: Reporter) {
    std::thread::spawn(move || {
        let mut tracker = Tracker::default();
        let mut active;
        loop {
            if !crate::config::load().ui.watch_downloads {
                super::wait_for(3600, &[&super::CONFIG_GEN]);
                continue;
            }
            let Some(dir) = folder() else { return };
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
            let events = tracker.update(now, &listing(&dir));
            active = !events.is_empty();
            for e in events {
                match e {
                    Event::Running { id, label, detail } => rep.input(Input::Activity(State::Run, Activity { id, label, detail, icon: Some("download".into()), ..Default::default() })),
                    Event::Done { id, label, detail } => {
                        // the id names the partial file: the finished one is what its card opens
                        let file = id.strip_prefix("dl:").and_then(island_core::downloads::final_name).map(|n| dir.join(n).to_string_lossy().into_owned());
                        rep.input(Input::Activity(State::Done, Activity { id, label, detail, icon: Some("download".into()), file, ..Default::default() }))
                    }
                    Event::Dropped { id } => rep.input(Input::ActivityClear(id)),
                }
            }
            // looks every two seconds while something downloads, every four otherwise
            std::thread::sleep(Duration::from_secs(if active { 2 } else { 4 }));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_download_folder_is_read_from_the_user_dirs_file() {
        let home = std::path::Path::new("/home/u");
        assert_eq!(parse_user_dirs("XDG_DESKTOP_DIR=\"$HOME/Desktop\"\nXDG_DOWNLOAD_DIR=\"$HOME/Scaricati\"\n", home), Some(home.join("Scaricati")));
        assert_eq!(parse_user_dirs("XDG_DOWNLOAD_DIR=\"/data/dl\"\n", home), Some(PathBuf::from("/data/dl")));
        assert_eq!(parse_user_dirs("XDG_DOWNLOAD_DIR=\"$HOME/\"\n", home), None, "the home itself is not a download folder");
        assert_eq!(parse_user_dirs("nothing", home), None);
    }
}
