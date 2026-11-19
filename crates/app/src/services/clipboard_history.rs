//! The clipboard history list: newest first, no duplicates, no blank text, capped. Shared by every platform.
//! Pinned entries stay on top and are the only ones written to disk (copied text is otherwise kept in memory only).

use super::Reporter;
use std::sync::Mutex;

pub const MAX: usize = 25;
pub const MAX_CHARS: usize = 4000;
const MAX_PINNED: usize = 12;

/// Adds an entry to the history: newest first, no duplicates, no blank text, capped.
pub fn push(history: &mut Vec<String>, text: &str) -> bool {
    let text: String = text.chars().take(MAX_CHARS).collect();
    if text.trim().is_empty() {
        return false;
    }
    history.retain(|t| *t != text);
    history.insert(0, text);
    history.truncate(MAX);
    true
}

/// The history and its pins; `view` is what the island shows: pinned first, then the rest.
#[derive(Default)]
pub struct History {
    pub recent: Vec<String>,
    pub pinned: Vec<String>,
}

impl History {
    pub fn record(&mut self, text: &str) -> bool {
        if self.pinned.iter().any(|t| t == text) {
            return false;
        }
        push(&mut self.recent, text)
    }

    pub fn toggle_pin(&mut self, text: &str) {
        if let Some(i) = self.pinned.iter().position(|t| t == text) {
            self.pinned.remove(i);
            push(&mut self.recent, text);
        } else {
            self.recent.retain(|t| t != text);
            self.pinned.insert(0, text.to_string());
            self.pinned.truncate(MAX_PINNED);
        }
    }

    pub fn forget(&mut self, text: &str) {
        self.recent.retain(|t| t != text);
        self.pinned.retain(|t| t != text);
    }

    pub fn view(&self) -> Vec<String> {
        self.pinned.iter().chain(self.recent.iter()).cloned().collect()
    }
}

static STATE: Mutex<Option<History>> = Mutex::new(None);
/// Texts that must never enter the history (a one-time code that was just copied).
static IGNORED: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn ignore(text: &str) {
    if let Ok(mut g) = IGNORED.lock() {
        g.push(text.to_string());
        g.truncate(8);
    }
}

fn with(f: impl FnOnce(&mut History)) -> Option<(Vec<String>, Vec<String>)> {
    let mut g = STATE.lock().ok()?;
    let h = g.get_or_insert_with(|| History { recent: vec![], pinned: load_pinned() });
    f(h);
    Some((h.view(), h.pinned.clone()))
}

fn publish(rep: &Reporter, r: Option<(Vec<String>, Vec<String>)>) {
    if let Some((all, pinned)) = r {
        rep.patch(move |d| {
            d.clipboard = all;
            d.clipboard_pinned = pinned;
        });
    }
}

/// Loads the pins from the state directory and shows them (called once when the service starts).
pub fn init(rep: &Reporter) {
    publish(rep, with(|_| {}));
}

/// A new text on the clipboard.
pub fn record(rep: &Reporter, text: &str) {
    if IGNORED.lock().is_ok_and(|g| g.iter().any(|t| t == text)) {
        return;
    }
    let mut added = false;
    let r = with(|h| added = h.record(text));
    if added {
        publish(rep, r);
    }
}

pub fn toggle_pin(rep: &Reporter, text: &str) {
    let r = with(|h| h.toggle_pin(text));
    save_pinned();
    publish(rep, r);
}

pub fn forget(rep: &Reporter, text: &str) {
    let r = with(|h| h.forget(text));
    save_pinned();
    publish(rep, r);
}

fn load_pinned() -> Vec<String> {
    let Some(p) = super::state_file("clipboard-pinned.json") else { return vec![] };
    std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok()).unwrap_or_default()
}

fn save_pinned() {
    let pinned = STATE.lock().ok().and_then(|g| g.as_ref().map(|h| h.pinned.clone())).unwrap_or_default();
    let Some(p) = super::state_file("clipboard-pinned.json") else { return };
    if let Ok(json) = serde_json::to_string(&pinned) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            if let Ok(mut f) = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&p) {
                let _ = std::io::Write::write_all(&mut f, json.as_bytes());
            }
        }
        #[cfg(not(unix))]
        let _ = std::fs::write(&p, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_first_without_duplicates() {
        let mut h = vec![];
        assert!(push(&mut h, "a"));
        assert!(push(&mut h, "b"));
        assert!(push(&mut h, "a"));
        assert_eq!(h, vec!["a", "b"]);
        assert!(!push(&mut h, "   \n"));
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn caps_the_list_and_the_length() {
        let mut h = vec![];
        for i in 0..40 {
            push(&mut h, &i.to_string());
        }
        assert_eq!(h.len(), MAX);
        assert_eq!(h[0], "39");
        push(&mut h, &"x".repeat(10_000));
        assert_eq!(h[0].chars().count(), MAX_CHARS);
    }

    #[test]
    fn pinned_entries_stay_on_top_and_leave_the_recent_list() {
        let mut h = History::default();
        h.record("a");
        h.record("b");
        h.toggle_pin("a");
        h.record("c");
        assert_eq!(h.view(), vec!["a", "c", "b"]);
        assert!(!h.record("a"), "a pinned text is not recorded twice");
        h.toggle_pin("a");
        assert_eq!(h.view(), vec!["a", "c", "b"], "unpinned: back in the recent list, newest");
        assert!(h.pinned.is_empty());
    }

    #[test]
    fn forgetting_removes_an_entry_everywhere() {
        let mut h = History::default();
        h.record("a");
        h.toggle_pin("a");
        h.record("b");
        h.forget("a");
        h.forget("b");
        assert!(h.view().is_empty());
    }
}
