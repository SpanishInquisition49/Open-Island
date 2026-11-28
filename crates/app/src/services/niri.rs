//! Hides the island under fullscreen windows on niri. niri's IPC has no fullscreen flag, so a window counts
//! as fullscreen when it is the focused one and covers its whole output. The island drops from the overlay to
//! the top layer while that is true (niri draws fullscreen windows above the top layer).

use super::Reporter;
use island_core::cover;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

#[derive(Default)]
pub struct State {
    windows: HashMap<u64, Value>,
    focused: Option<u64>,
    /// The screen of each workspace, and the workspaces on show (one a screen).
    workspaces: HashMap<u64, String>,
    shown: Vec<u64>,
    outputs: HashMap<String, (f64, f64)>,
    overview: bool,
    /// The keyboard layouts, and the one in use.
    layouts: Vec<String>,
    layout: Option<usize>,
    /// A layout just switched to, not yet told.
    switched: Option<String>,
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

    /// The focused window: its app id and title.
    pub fn focused_window(&self) -> Option<(String, String)> {
        let w = self.focused.and_then(|id| self.windows.get(&id))?;
        Some((w.get("app_id").and_then(Value::as_str).unwrap_or("").to_string(), w.get("title").and_then(Value::as_str).unwrap_or("").to_string()))
    }

    /// The name of the layout the keyboard has just been switched to, once.
    pub fn take_switch(&mut self) -> Option<String> {
        self.switched.take()
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
            "WorkspaceActivated" => {
                if let Some(id) = body["id"].as_u64() {
                    if let Some(screen) = self.workspaces.get(&id) {
                        self.shown.retain(|other| self.workspaces.get(other) != Some(screen));
                        self.shown.push(id);
                    }
                }
            }
            "WorkspacesChanged" => {
                self.shown = body["workspaces"].as_array().into_iter().flatten().filter(|w| w["is_active"].as_bool() == Some(true)).filter_map(|w| w["id"].as_u64()).collect();
                self.workspaces = body["workspaces"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|w| Some((w["id"].as_u64()?, w["output"].as_str()?.to_string())))
                    .collect();
            }
            // the list of layouts (at the start, and when the configuration changes) is no switch
            "KeyboardLayoutsChanged" => {
                let list = &body["keyboard_layouts"];
                self.layouts = list["names"].as_array().into_iter().flatten().filter_map(|n| n.as_str().map(str::to_string)).collect();
                self.layout = list["current_idx"].as_u64().map(|i| i as usize);
            }
            "KeyboardLayoutSwitched" => {
                let now = body["idx"].as_u64().map(|i| i as usize);
                if now != self.layout {
                    self.layout = now;
                    self.switched = now.and_then(|i| self.layouts.get(i).cloned());
                }
            }
            "OverviewOpenedOrClosed" => self.overview = body["is_open"].as_bool().unwrap_or(false),
            "ConfigLoaded" | "OutputsChanged" => self.refresh_outputs(),
            _ => {}
        }
    }

    /// The screens, by name, on which a window of the workspace on show is over the island's place.
    ///
    /// niri says where a floating window is. For a tiled one it only says how big it is, so two things are worked out:
    /// whether tiled windows reach the top at all, from the room the tallest column of the screen leaves (a bar leaves
    /// more than the island is deep), and whether the columns on show are wide enough, side by side, to reach the middle.
    pub fn covered(&self) -> Vec<String> {
        if self.overview {
            return vec![];
        }
        let mut out: Vec<String> = self.outputs.iter().filter(|(name, size)| self.covers_place(name, **size)).map(|(name, _)| name.clone()).collect();
        out.sort();
        out
    }

    fn covers_place(&self, screen: &str, (width, height): (f64, f64)) -> bool {
        let place = cover::place(cover::Rect { left: 0.0, top: 0.0, right: width, bottom: height }, 1.0);
        let on_screen = |w: &Value| w["workspace_id"].as_u64().filter(|ws| self.workspaces.get(ws).is_some_and(|s| s == screen));
        let pair = |w: &Value, key: &str| {
            let v = w.pointer(&format!("/layout/{key}"))?.as_array()?;
            Some((v.first()?.as_f64()?, v.get(1)?.as_f64()?))
        };
        // each column of each workspace of this screen: how tall its tiles are together, and how wide
        let mut columns: HashMap<(u64, u64), (f64, f64)> = HashMap::new();
        for w in self.windows.values() {
            let Some(ws) = on_screen(w) else { continue };
            if w["is_floating"].as_bool() == Some(true) {
                if !self.shown.contains(&ws) {
                    continue;
                }
                let (Some((x, y)), Some((ww, wh))) = (pair(w, "tile_pos_in_workspace_view"), pair(w, "window_size")) else { continue };
                let (dx, dy) = pair(w, "window_offset_in_tile").unwrap_or((0.0, 0.0));
                let window = cover::Rect { left: x + dx, top: y + dy, right: x + dx + ww, bottom: y + dy + wh };
                if window.overlaps(&place) {
                    return true;
                }
            } else if let (Some((column, _)), Some((tw, th))) = (pair(w, "pos_in_scrolling_layout"), pair(w, "tile_size")) {
                let c = columns.entry((ws, column as u64)).or_default();
                *c = (c.0 + th, c.1.max(tw));
            }
        }
        let tallest = columns.values().map(|c| c.0).fold(0.0, f64::max);
        let reach_the_top = tallest > 0.0 && height - tallest < place.bottom;
        let side_by_side: f64 = columns.iter().filter(|((ws, _), _)| self.shown.contains(ws)).map(|(_, c)| c.1).sum();
        reach_the_top && side_by_side > place.left
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
        let (mut last, mut last_focus, mut last_covered) = (None, None, Vec::new());
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Ok(ev) = serde_json::from_str::<Value>(&line) else { continue };
            state.apply(&ev);
            if let Some(layout) = state.take_switch() {
                rep.input(island_ui::model::Input::KeyboardLayout(layout));
            }
            // where the attention is (for the activity page)
            let focus = state.focused_window();
            if focus != last_focus {
                if let Some((app, title)) = focus.clone() {
                    log::debug!("focus: {app} / {title}");
                    rep.input(island_ui::model::Input::Focused { app, title });
                }
                last_focus = focus;
            }
            let covered = state.covered();
            if covered != last_covered {
                last_covered = covered.clone();
                rep.covered_screens(covered);
            }
            let now = state.fullscreen();
            if last != Some(now) {
                last = Some(now);
                rep.fullscreen(now);
            }
        }
        // niri is gone: nothing is known to be over the island
        rep.covered_screens(Vec::new());
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

    fn tile(id: u64, ws: u64, column: u64, size: (f64, f64)) -> Value {
        json!({"id": id, "is_floating": false, "workspace_id": ws, "layout": {"pos_in_scrolling_layout": [column, 1], "tile_size": [size.0, size.1], "window_size": [size.0, size.1]}})
    }

    fn floating(id: u64, ws: u64, at: (f64, f64), size: (f64, f64)) -> Value {
        json!({"id": id, "is_floating": true, "workspace_id": ws, "layout": {"tile_pos_in_workspace_view": [at.0, at.1], "window_offset_in_tile": [0.0, 0.0], "window_size": [size.0, size.1]}})
    }

    /// Two workspaces on one screen of 1755 × 1097, the first on show.
    fn screen(windows: Vec<Value>) -> State {
        let mut s = State::default();
        s.outputs.insert("eDP-1".into(), (1755.0, 1097.0));
        s.apply(&json!({"WorkspacesChanged": {"workspaces": [{"id": 1, "output": "eDP-1", "is_active": true}, {"id": 2, "output": "eDP-1", "is_active": false}]}}));
        s.apply(&json!({"WindowsChanged": {"windows": windows}}));
        s
    }

    #[test]
    fn a_floating_window_covers_the_place_only_where_it_is_and_on_the_workspace_on_show() {
        let over = |at, size| !screen(vec![floating(1, 1, at, size)]).covered().is_empty();
        assert!(over((700.0, 10.0), (400.0, 300.0)));
        assert!(!over((700.0, 400.0), (900.0, 600.0)), "lower down");
        assert!(!over((100.0, 0.0), (400.0, 300.0)), "to one side");
        let mut s = screen(vec![floating(1, 2, (700.0, 10.0), (400.0, 300.0))]);
        assert!(s.covered().is_empty(), "on a workspace that is not on show");
        s.apply(&json!({"WorkspaceActivated": {"id": 2, "focused": true}}));
        assert_eq!(s.covered(), ["eDP-1"]);
        s.apply(&json!({"OverviewOpenedOrClosed": {"is_open": true}}));
        assert!(s.covered().is_empty(), "the overview shows the island");
        s.apply(&json!({"OverviewOpenedOrClosed": {"is_open": false}}));
        s.apply(&json!({"WindowClosed": {"id": 1}}));
        assert!(s.covered().is_empty());
    }

    #[test]
    fn tiled_windows_cover_the_place_when_no_bar_keeps_them_below_it() {
        // gaps of 4 and nothing above: the windows start under the island
        assert_eq!(screen(vec![tile(1, 1, 1, (1747.0, 1089.0))]).covered(), ["eDP-1"]);
        // with a bar they start below it
        assert!(screen(vec![tile(1, 1, 1, (1747.0, 1055.0))]).covered().is_empty());
        // two in a column count together
        assert_eq!(screen(vec![tile(1, 1, 1, (1747.0, 545.0)), tile(2, 1, 1, (1747.0, 544.0))]).covered(), ["eDP-1"]);
        // a narrow column stays at the left of the middle; two reach it
        assert!(screen(vec![tile(1, 1, 1, (580.0, 1089.0))]).covered().is_empty());
        assert_eq!(screen(vec![tile(1, 1, 1, (580.0, 1089.0)), tile(2, 1, 2, (580.0, 1089.0))]).covered(), ["eDP-1"]);
        // nothing on show covers nothing, whatever the other workspace holds
        assert!(screen(vec![tile(1, 2, 1, (1747.0, 1089.0))]).covered().is_empty());
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
    fn a_switch_of_the_layout_is_told_once() {
        let mut s = State::default();
        s.apply(&json!({"KeyboardLayoutsChanged": {"keyboard_layouts": {"names": ["Italian", "English (US)"], "current_idx": 0}}}));
        assert_eq!(s.take_switch(), None);
        s.apply(&json!({"KeyboardLayoutSwitched": {"idx": 1}}));
        assert_eq!(s.take_switch().as_deref(), Some("English (US)"));
        assert_eq!(s.take_switch(), None);
        s.apply(&json!({"KeyboardLayoutSwitched": {"idx": 1}}));
        s.apply(&json!({"KeyboardLayoutSwitched": {"idx": 7}}));
        assert_eq!(s.take_switch(), None, "the same one, or one that is not there");
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
