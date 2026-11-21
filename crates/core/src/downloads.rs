//! Downloads in progress, seen from the folder they go to: a browser writes `name.part` (Firefox), `name.crdownload`
//! (Chrome, Edge, Brave), `name.download` (Safari) or `name.opdownload` (Opera) and renames it when it is done.
//! From two looks at the folder this tells which downloads are running, how fast, and when one ended or was dropped.
//! No browser extension is needed; the size is not known in advance, so there is no percentage, only what has arrived.

use std::collections::HashMap;

const SUFFIXES: [&str; 4] = [".part", ".crdownload", ".download", ".opdownload"];

/// The name the file will have when it is complete, for a name that is a download in progress.
pub fn final_name(file: &str) -> Option<&str> {
    let lower = file.to_lowercase();
    SUFFIXES.iter().find(|s| lower.ends_with(**s)).map(|s| &file[..file.len() - s.len()]).filter(|n| !n.is_empty())
}

/// "12 MB", "1.4 GB", "830 kB"
pub fn fmt_size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e7 {
        format!("{:.0} MB", b / 1e6)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else {
        format!("{:.0} kB", b / 1e3)
    }
}

/// What happened to one download.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Running: what to call it and the status line ("12 MB · 2.1 MB/s").
    Running { id: String, label: String, detail: String },
    /// Done: the file is complete, with its size and how long it took.
    Done { id: String, label: String, detail: String },
    /// The partial file went away and nothing was left in its place (cancelled).
    Dropped { id: String },
}

struct Seen {
    size: u64,
    first_ms: i64,
    /// When the size last changed.
    moved_ms: i64,
    /// Bytes per second, smoothed.
    speed: f64,
    last_ms: i64,
}

#[derive(Default)]
pub struct Tracker {
    seen: HashMap<String, Seen>,
}

impl Tracker {
    /// `listing`: every file of the folder with its size; `now` in ms. Returns what changed worth saying; a download that
    /// is running is reported at every look (the status line changes with the size).
    pub fn update(&mut self, now: i64, listing: &[(String, u64)]) -> Vec<Event> {
        let mut out = vec![];
        let size_of = |name: &str| listing.iter().find(|(n, _)| n == name).map(|(_, s)| *s);
        for (file, size) in listing {
            let Some(done_name) = final_name(file) else { continue };
            let label = shorten(done_name);
            let s = self.seen.entry(file.clone()).or_insert(Seen { size: *size, first_ms: now, moved_ms: now, speed: 0.0, last_ms: now });
            let dt = (now - s.last_ms) as f64 / 1000.0;
            if dt >= 0.9 {
                let inst = size.saturating_sub(s.size) as f64 / dt;
                s.speed = if s.speed == 0.0 { inst } else { 0.6 * s.speed + 0.4 * inst };
                if *size != s.size {
                    s.moved_ms = now;
                }
                s.size = *size;
                s.last_ms = now;
            }
            let stalled = now - s.moved_ms > 20_000;
            let detail = if stalled {
                format!("{} · stalled", fmt_size(s.size))
            } else if s.speed >= 1000.0 {
                format!("{} · {}/s", fmt_size(s.size), fmt_size(s.speed as u64))
            } else {
                fmt_size(s.size)
            };
            out.push(Event::Running { id: format!("dl:{file}"), label, detail });
        }
        // those that were there and are not any more: done when the real file is there, else dropped
        let gone: Vec<String> = self.seen.keys().filter(|k| !listing.iter().any(|(n, _)| n == *k)).cloned().collect();
        for file in gone {
            let s = self.seen.remove(&file).unwrap();
            let id = format!("dl:{file}");
            match final_name(&file).and_then(|n| size_of(n).map(|sz| (n, sz))) {
                Some((name, size)) => out.push(Event::Done { id, label: shorten(name), detail: format!("{} · {}", fmt_size(size), crate::duration::fmt_took(now - s.first_ms)) }),
                None => out.push(Event::Dropped { id }),
            }
        }
        out
    }
}

/// A name that fits a status line: the end of a long one is the part that matters least, the extension the most.
pub fn shorten(name: &str) -> String {
    let n = name.chars().count();
    if n <= 28 {
        return name.to_string();
    }
    let ext_at = name.rfind('.').filter(|i| name.len() - i <= 6).unwrap_or(name.len());
    let (stem, ext) = name.split_at(ext_at);
    let keep = 26usize.saturating_sub(ext.chars().count());
    format!("{}…{ext}", stem.chars().take(keep).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(n: &str, s: u64) -> (String, u64) {
        (n.to_string(), s)
    }

    #[test]
    fn partial_files_of_the_browsers_are_recognised() {
        assert_eq!(final_name("thesis.pdf.part"), Some("thesis.pdf"));
        assert_eq!(final_name("video.mp4.CRDOWNLOAD"), Some("video.mp4"));
        assert_eq!(final_name("a.dmg.download"), Some("a.dmg"));
        assert_eq!(final_name("a.zip.opdownload"), Some("a.zip"));
        assert_eq!(final_name("notes.pdf"), None);
        assert_eq!(final_name(".part"), None);
    }

    #[test]
    fn a_download_is_followed_from_start_to_the_file_appearing() {
        let mut t = Tracker::default();
        let ev = t.update(0, &[f("a.iso.part", 1_000_000), f("other.txt", 5)]);
        assert_eq!(ev, vec![Event::Running { id: "dl:a.iso.part".into(), label: "a.iso".into(), detail: "1.0 MB".into() }]);
        let ev = t.update(2_000, &[f("a.iso.part", 5_000_000)]);
        assert_eq!(ev, vec![Event::Running { id: "dl:a.iso.part".into(), label: "a.iso".into(), detail: "5.0 MB · 2.0 MB/s".into() }]);
        let ev = t.update(4_000, &[f("a.iso", 9_000_000)]);
        assert_eq!(ev, vec![Event::Done { id: "dl:a.iso.part".into(), label: "a.iso".into(), detail: "9.0 MB · 4s".into() }]);
        assert!(t.update(6_000, &[f("a.iso", 9_000_000)]).is_empty(), "nothing more once it is done");
    }

    #[test]
    fn a_partial_that_vanishes_is_dropped_and_a_frozen_one_says_so() {
        let mut t = Tracker::default();
        t.update(0, &[f("x.zip.crdownload", 500_000)]);
        let ev = t.update(30_000, &[f("x.zip.crdownload", 500_000)]);
        assert!(matches!(&ev[0], Event::Running { detail, .. } if detail == "500 kB · stalled"), "{ev:?}");
        assert_eq!(t.update(31_000, &[]), vec![Event::Dropped { id: "dl:x.zip.crdownload".into() }]);
    }

    #[test]
    fn long_names_keep_their_extension() {
        assert_eq!(shorten("short.pdf"), "short.pdf");
        let s = shorten("Lecture-notes-on-distributed-systems-2026-final-v3.pdf");
        assert!(s.chars().count() <= 28 && s.ends_with("….pdf"), "{s}");
    }
}
