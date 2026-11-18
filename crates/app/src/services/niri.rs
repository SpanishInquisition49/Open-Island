//! Hides the island under fullscreen windows on niri. niri's IPC has no fullscreen flag, so a window counts
//! as fullscreen when it is the focused one and covers its whole output. The island drops from the overlay to
//! the top layer while that is true (niri draws fullscreen windows above the top layer).

use super::Reporter;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

#[derive(Default)]
pub struct State {
    windows: HashMap<u64, Value>,
    focused: Option<u64>,
    workspaces: HashMap<u64, String>,
    outputs: HashMap<String, (f64, f64)>,
    overview: bool,
}

impl State {
    pub fn fullscreen(&self) -> bool {
        let Some(w) = self.focused.and_then(|id| self.windows.get(&id)) else { return false };
        if self.overview || w.get("is_floating").and_then(Value::as_bool).unwrap_or(false) {
            return false;
        }
        let size = w.pointer("/layout/window_size").and_then(Value::as_array);
        let (Some(size), Some(ws)) = (size, w.get("workspace_id").and_then(Value::as_u64)) else { return false };
        let Some(&(ow, oh)) = self.workspaces.get(&ws).and_then(|o| self.outputs.get(o)) else { return false };
        let (sw, sh) = (size.first().and_then(Value::as_f64).unwrap_or(0.0), size.get(1).and_then(Value::as_f64).unwrap_or(0.0));
        sw >= ow - 1.0 && sh >= oh - 1.0
    }

    pub fn apply(&mut self, event: &Value) {
        let Some((kind, body)) = event.as_object().and_then(|o| o.iter().next()) else { return };
        match kind.as_str() {
            "WindowsChanged" => {
                self.windows.clear();
                self.focused = None;
                for w in body["windows"].as_array().into_iter().flatten() {
                    if let Some(id) = w["id"].as_u64() {
                        if w["is_focused"].as_bool() == Some(true) {
                            self.focused = Some(id);
                        }
                        self.windows.insert(id, w.clone());
                    }
                }
            }
            "WindowOpenedOrChanged" => {
                let w = &body["window"];
                if let Some(id) = w["id"].as_u64() {
                    if w["is_focused"].as_bool() == Some(true) {
                        self.focused = Some(id);
                    }
                    self.windows.insert(id, w.clone());
                }
            }
            "WindowClosed" => {
                if let Some(id) = body["id"].as_u64() {
                    self.windows.remove(&id);
                    if self.focused == Some(id) {
                        self.focused = None;
                    }
                }
            }
            "WindowFocusChanged" => self.focused = body["id"].as_u64(),
            "WindowLayoutsChanged" => {
                for ch in body["changes"].as_array().into_iter().flatten() {
                    if let (Some(id), Some(layout)) = (ch[0].as_u64(), ch.get(1)) {
                        if let Some(w) = self.windows.get_mut(&id) {
                            w["layout"] = layout.clone();
                        }
                    }
                }
            }
            "WorkspacesChanged" => {
                self.workspaces = body["workspaces"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|w| Some((w["id"].as_u64()?, w["output"].as_str()?.to_string())))
                    .collect();
            }
            "OverviewOpenedOrClosed" => self.overview = body["is_open"].as_bool().unwrap_or(false),
            "ConfigLoaded" | "OutputsChanged" => self.refresh_outputs(),
            _ => {}
        }
    }

    pub fn refresh_outputs(&mut self) {
        let Ok(out) = Command::new("niri").args(["msg", "--json", "outputs"]).output() else { return };
        let Ok(v) = serde_json::from_slice::<Value>(&out.stdout) else { return };
        self.outputs = v
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(name, o)| Some((name.clone(), (o.pointer("/logical/width")?.as_f64()?, o.pointer("/logical/height")?.as_f64()?))))
            .collect();
    }
}

/// Starts the watcher when niri is the running compositor.
pub fn start(rep: Reporter) {
    if std::env::var_os("NIRI_SOCKET").is_none() {
        return;
    }
    std::thread::spawn(move || loop {
        let Ok(mut child) = Command::new("niri").args(["msg", "--json", "event-stream"]).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else { return };
        let Some(stdout) = child.stdout.take() else { return };
        let mut state = State::default();
        state.refresh_outputs();
        let mut last = None;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Ok(ev) = serde_json::from_str::<Value>(&line) else { continue };
            state.apply(&ev);
            let now = state.fullscreen();
            if last != Some(now) {
                last = Some(now);
                rep.fullscreen(now);
            }
        }
        let _ = child.wait();
        std::thread::sleep(std::time::Duration::from_secs(2)); // niri restarted: reconnect
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn win(id: u64, size: (u32, u32), focused: bool, floating: bool) -> Value {
        json!({"id": id, "is_focused": focused, "is_floating": floating, "workspace_id": 1, "layout": {"window_size": [size.0, size.1]}})
    }

    fn state(windows: Vec<Value>) -> State {
        let mut s = State::default();
        s.outputs.insert("eDP-1".into(), (1755.0, 1097.0));
        s.apply(&json!({"WorkspacesChanged": {"workspaces": [{"id": 1, "output": "eDP-1"}]}}));
        s.apply(&json!({"WindowsChanged": {"windows": windows}}));
        s
    }

    #[test]
    fn tiled_is_not_fullscreen_but_covering_is() {
        assert!(!state(vec![win(1, (1747, 1055), true, false)]).fullscreen());
        assert!(state(vec![win(1, (1755, 1097), true, false)]).fullscreen());
    }

    #[test]
    fn follows_focus_layout_overview_and_close() {
        let mut s = state(vec![win(1, (1755, 1097), false, false), win(2, (800, 600), true, false)]);
        assert!(!s.fullscreen());
        s.apply(&json!({"WindowFocusChanged": {"id": 1}}));
        assert!(s.fullscreen());
        s.apply(&json!({"OverviewOpenedOrClosed": {"is_open": true}}));
        assert!(!s.fullscreen());
        s.apply(&json!({"OverviewOpenedOrClosed": {"is_open": false}}));
        s.apply(&json!({"WindowLayoutsChanged": {"changes": [[1, {"window_size": [1747, 1055]}]]}}));
        assert!(!s.fullscreen());
        s.apply(&json!({"WindowLayoutsChanged": {"changes": [[1, {"window_size": [1755, 1097]}]]}}));
        assert!(s.fullscreen());
        s.apply(&json!({"WindowClosed": {"id": 1}}));
        assert!(!s.fullscreen());
    }

    #[test]
    fn floating_and_unknown_outputs_are_not_fullscreen() {
        assert!(!state(vec![win(1, (1755, 1097), true, true)]).fullscreen());
        let mut s = State::default();
        s.apply(&json!({"WindowsChanged": {"windows": [win(1, (1755, 1097), true, false)]}}));
        assert!(!s.fullscreen());
        s.apply(&json!("garbage"));
    }
}
