//! Finding an application's icon from its name (freedesktop icon themes, PNG only).

use island_ui::model::Image;

const SIZES: [&str; 7] = ["64x64", "48x48", "128x128", "256x256", "96x96", "32x32", "512x512"];
const THEMES: [&str; 3] = ["hicolor", "Adwaita", "breeze"];

fn dirs() -> Vec<std::path::PathBuf> {
    let mut v = vec![];
    if let Some(h) = std::env::var_os("XDG_DATA_HOME") {
        v.push(std::path::PathBuf::from(h));
    } else if let Some(h) = std::env::var_os("HOME") {
        v.push(std::path::PathBuf::from(h).join(".local/share"));
    }
    let data = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    v.extend(data.split(':').filter(|d| !d.is_empty()).map(std::path::PathBuf::from));
    v
}

/// The path of the best PNG for `name`, or None. A path is returned as it is.
pub fn find(name: &str) -> Option<std::path::PathBuf> {
    if name.is_empty() {
        return None;
    }
    if name.starts_with('/') {
        return std::path::Path::new(name).is_file().then(|| name.into());
    }
    for base in dirs() {
        for theme in THEMES {
            for size in SIZES {
                let p = base.join("icons").join(theme).join(size).join("apps").join(format!("{name}.png"));
                if p.is_file() {
                    return Some(p);
                }
            }
        }
        let p = base.join("pixmaps").join(format!("{name}.png"));
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

pub fn load(name: &str) -> Option<Image> {
    let name = name.strip_prefix("file://").unwrap_or(name);
    super::art::load(find(name)?.to_str()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_empty_names() {
        assert!(find("").is_none());
        assert!(find("/nonexistent/icon.png").is_none());
        assert!(find("definitely-not-an-icon-name-xyz").is_none());
        let exe = std::env::current_exe().unwrap();
        assert_eq!(find(exe.to_str().unwrap()).as_deref(), Some(exe.as_path()));
    }
}
