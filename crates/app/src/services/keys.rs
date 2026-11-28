//! Caps Lock, read from the keyboards' lights in sysfs (the compositor lights them), which needs no permission and
//! works under any of them. The layout comes from the compositor: see `niri.rs`. Option `keyboard_alerts`.

use super::Reporter;
use island_ui::model::Input;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const LEDS: &str = "/sys/class/leds";

/// The `brightness` files of the Caps Lock lights (one per keyboard: `input3::capslock`).
fn lights(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    rd.flatten().filter(|e| e.file_name().to_string_lossy().ends_with("::capslock")).map(|e| e.path().join("brightness")).collect()
}

/// Whether any of them is lit; None when none can be read.
fn lit(lights: &[PathBuf]) -> Option<bool> {
    let read: Vec<bool> = lights.iter().filter_map(|p| std::fs::read_to_string(p).ok()).map(|s| s.trim() != "0").collect();
    (!read.is_empty()).then(|| read.contains(&true))
}

/// What to tell after a reading: only a change from one that was seen.
fn change(before: &mut Option<bool>, now: Option<bool>) -> Option<bool> {
    let told = match (*before, now) {
        (Some(a), Some(b)) if a != b => Some(b),
        _ => None,
    };
    *before = now;
    told
}

pub fn start(rep: Reporter) {
    std::thread::spawn(move || {
        let (mut found, mut looked, mut before) = (vec![], None::<Instant>, None);
        loop {
            if !crate::config::load().ui.keyboard_alerts {
                before = None;
                super::wait_for(3600, &[&super::CONFIG_GEN]);
                continue;
            }
            // keyboards come and go; and the setting is read again now and then
            if looked.is_none_or(|at| at.elapsed() > Duration::from_secs(10)) {
                found = lights(Path::new(LEDS));
                looked = Some(Instant::now());
            }
            let config = super::CONFIG_GEN.load(std::sync::atomic::Ordering::Relaxed);
            // a sysfs light tells nobody when it changes: it is looked at
            while looked.is_some_and(|at| at.elapsed() <= Duration::from_secs(10)) && config == super::CONFIG_GEN.load(std::sync::atomic::Ordering::Relaxed) {
                if let Some(on) = change(&mut before, lit(&found)) {
                    rep.input(Input::CapsLock(on));
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            if config != super::CONFIG_GEN.load(std::sync::atomic::Ordering::Relaxed) {
                looked = None;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_lock_is_on_when_any_keyboard_lights_it() {
        let dir = std::env::temp_dir().join(format!("oi-leds-{}", std::process::id()));
        for (name, value) in [("input1::capslock", "0\n"), ("input2::capslock", "0\n"), ("input2::numlock", "1\n")] {
            std::fs::create_dir_all(dir.join(name)).unwrap();
            std::fs::write(dir.join(name).join("brightness"), value).unwrap();
        }
        let found = lights(&dir);
        assert_eq!(found.len(), 2);
        assert_eq!(lit(&found), Some(false));
        std::fs::write(dir.join("input2::capslock/brightness"), "1\n").unwrap();
        assert_eq!(lit(&found), Some(true));
        assert_eq!(lit(&[]), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_change_is_told() {
        let mut before = None;
        assert_eq!(change(&mut before, Some(true)), None, "how it was at the start is no news");
        assert_eq!(change(&mut before, Some(true)), None);
        assert_eq!(change(&mut before, Some(false)), Some(false));
        assert_eq!(change(&mut before, None), None);
        assert_eq!(change(&mut before, Some(true)), None, "a keyboard just plugged in");
    }
}
