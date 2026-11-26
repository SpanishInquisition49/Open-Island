//! The files for a lecture: given the title of an event, looks in the study folders for documents whose path mentions the
//! same words (see `island_core::materials`) and reports the best few. Only file names and dates are read, never contents.

use super::Reporter;
use island_core::materials::{self as m, Material};
use island_ui::model::Command;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_ENTRIES: usize = 60_000;
const MAX_DEPTH: usize = 6;
const MAX_TIME: Duration = Duration::from_millis(2500);

/// The folders to look in: the configured ones (`~` expanded) or Documents and Downloads.
pub(super) fn folders() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let Some(home) = home else { return vec![] };
    let set = crate::config::load().study_folders;
    if set.is_empty() {
        // Documents, Downloads, and the folders students call their courses' (only those that exist)
        let mut v = vec![home.join("Documents")];
        v.extend(super::downloads::folder());
        v.extend(["Uni", "University", "Universita", "Università", "Studio", "Study", "Corsi", "Courses"].iter().map(|n| home.join(n)));
        v.retain(|d| d.is_dir());
        return v;
    }
    set.iter().map(|f| match f.strip_prefix("~/") { Some(rest) => home.join(rest), None => if f == "~" { home.clone() } else { PathBuf::from(f) } }).collect()
}

/// Walks the folders (never into hidden or build folders, never through links) for the study files that mention the words.
pub fn scan(roots: &[PathBuf], keywords: &[String]) -> Vec<(Material, usize)> {
    let began = Instant::now();
    let mut found = vec![];
    let mut seen = 0usize;
    let mut stack: Vec<(PathBuf, usize)> = roots.iter().filter(|r| r.is_dir()).map(|r| (r.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            seen += 1;
            if seen > MAX_ENTRIES || began.elapsed() > MAX_TIME {
                return found;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if depth < MAX_DEPTH && !m::skip_dir(&name) {
                    stack.push((e.path(), depth + 1));
                }
            } else if ft.is_file() && !name.starts_with('.') && m::is_study_file(&name) {
                let path = e.path().to_string_lossy().into_owned();
                let words = m::score(&path, keywords);
                if words > 0 {
                    // most words first, then slides and books before notes and sources
                    let score = words * 10 + m::kind_weight(&name);
                    let modified_ms = e.metadata().ok().and_then(|md| md.modified().ok()).and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64);
                    found.push((Material { path, name, modified_ms }, score));
                }
            }
        }
    }
    found
}

/// Opens a file with the program the system has set for it.
fn open_path(path: &str) {
    #[cfg(target_os = "windows")]
    // not `cmd /C start`: a file name with an `&` in it would be read by the shell
    let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
}

pub fn command(rep: &Reporter, c: &Command) {
    match c.clone() {
        Command::FindMaterials(title) => {
            let rep = rep.clone();
            std::thread::spawn(move || {
                let kws = m::keywords(&title);
                let best = if kws.is_empty() { vec![] } else { m::rank(scan(&folders(), &kws), 4) };
                let at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
                let key = m::fold(&title);
                rep.patch(move |d| {
                    d.materials.insert(key, (at, best));
                });
            });
        }
        Command::OpenFile(path) => {
            // only a real file or folder the island itself offered (an absolute path), opened by the system's own handler
            let p = Path::new(&path);
            if p.is_absolute() && (p.is_file() || p.is_dir()) {
                open_path(&path);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cargo test -p open-island live_materials -- --ignored --nocapture`: the real folders, for a title in TITLE.
    #[test]
    #[ignore]
    fn live_materials() {
        let title = std::env::var("TITLE").unwrap_or_else(|_| "Reti di calcolatori".into());
        let kws = m::keywords(&title);
        let t = Instant::now();
        let found = scan(&folders(), &kws);
        let best = m::rank(found.clone(), 4);
        println!("{} words {:?}: {} candidates in {:?}", title, kws, found.len(), t.elapsed());
        for b in best {
            println!("  {} ({})", b.name, b.path);
        }
    }

    #[test]
    fn the_scan_finds_study_files_by_the_words_of_their_path_and_skips_the_rest() {
        let root = std::env::temp_dir().join(format!("open-island-materials-{}", std::process::id()));
        let _ = std::fs::create_dir_all(root.join("Reti/slides"));
        let _ = std::fs::create_dir_all(root.join("node_modules/reti"));
        let _ = std::fs::create_dir_all(root.join("Cucina"));
        for f in ["Reti/slides/lezione-03.pdf", "Reti/appunti.md", "Reti/foto.jpg", "node_modules/reti/x.pdf", "Cucina/ricette.pdf", "reti-esame.docx"] {
            std::fs::write(root.join(f), b"x").unwrap();
        }
        let kws = m::keywords("Reti di calcolatori");
        let mut names: Vec<String> = scan(std::slice::from_ref(&root), &kws).into_iter().map(|(x, _)| x.name).collect();
        names.sort();
        assert_eq!(names, ["appunti.md", "lezione-03.pdf", "reti-esame.docx"], "no photo, nothing under node_modules, nothing about cooking");
        let _ = std::fs::remove_dir_all(&root);
    }
}
