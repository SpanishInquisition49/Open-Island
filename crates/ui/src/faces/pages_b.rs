//! Part of the faces of `Island` (see `mod.rs`).

use super::*;

/// Pinned notes shown on Today (the rest stay kept).
const PIN_ROWS: usize = 4;

impl Island {
    /// The map: where the machine is, and where the next event is.
    pub(super) fn face_map(&self, p: P, w: f64, _h: f64) {
        use island_core::map;
        let hy = self.page_header(p, w, "Map");
        let view = self.map_view();
        let area = rect(14.0, 14.0 + 34.0, MAP_W, MAP_H);
        p.stagger(1);
        // the ground
        p.rrect(area, 14.0, theme::CHIP);
        let Some(v) = view else {
            p.text("Looking for where you are…", area.center().x, area.center().y - 8.0, 12.0, REG, theme::DIM, Align::Center, 300.0);
            p.text("It asks GeoClue; the location in the settings is the fallback", area.center().x, area.center().y + 12.0, 10.0, REG, theme::DIM, Align::Center, 360.0);
            let rb = p.button_width("Retry");
            p.button(rect(w - 14.0 - rb, hy - 14.0, rb, 28.0), "Retry", Action::MapRefresh);
            return;
        };
        let t = p.transform;
        p.scene.push_clip_layer(Fill::NonZero, t, &RoundedRect::from_rect(area, 14.0));
        for tile in map::tiles(v.center.0, v.center.1, v.zoom, MAP_W, MAP_H) {
            let (x, y) = (area.x0 + tile.px, area.y0 + tile.py);
            match self.tiles.get(&(tile.z, tile.x, tile.y)) {
                Some(img) => draw_tile(p, img, x, y, map::TILE),
                None => p.rrect(rect(x + 1.0, y + 1.0, map::TILE - 2.0, map::TILE - 2.0), 0.0, with_alpha(theme::FG, 0.04)),
            }
        }
        if map::needs_key(&self.cfg.map_tile_url) && !self.data.config.map_key_set {
            p.text("This map needs an API key", area.center().x, area.y1 - 56.0, 12.0, SEMI, theme::FG, Align::Center, 300.0);
            p.text("Settings > Map > API key", area.center().x, area.y1 - 38.0, 10.5, REG, theme::DIM, Align::Center, 300.0);
        }
        let at = |lat: f64, lon: f64| {
            let (px, py) = map::project(v.center, v.zoom, MAP_W, MAP_H, lat, lon);
            (area.x0 + px, area.y0 + py)
        };
        let me = at(v.me.0, v.me.1);
        let route = self.data.route.as_ref().filter(|r| self.map_dest.as_ref().is_some_and(|d| d.lat == r.dest.lat && d.lon == r.dest.lon));
        if let Some((d, title)) = &v.dest {
            let dp = at(d.0, d.1);
            match route.filter(|r| !r.segments.is_empty()) {
                // the way itself: a line along the roads, dashed where it is on foot
                Some(r) => {
                    for seg in &r.segments {
                        let pts: Vec<(f64, f64)> = seg.points.iter().map(|(la, lo)| at(*la, *lo)).collect();
                        p.polyline(&pts, 6.0, &[], with_alpha(theme::BG, 0.8));
                        p.polyline(&pts, 3.5, if seg.walk { &[5.0, 6.0] } else { &[] }, with_alpha(theme::RAIN, 0.98));
                    }
                }
                None => p.line(me, dp, 2.0, &[6.0, 6.0], with_alpha(theme::RAIN, 0.95)),
            }
            p.circle(dp.0, dp.1, 7.0, theme::ORANGE);
            p.circle(dp.0, dp.1, 2.5, theme::BG);
            // the name next to the pin, on a dark plate so it reads over any map
            let tw = (p.fonts.measure(title, 10.5, SEMI) as f64).min(150.0) + 12.0;
            let label = rect((dp.0 - tw / 2.0).clamp(area.x0 + 4.0, area.x1 - tw - 4.0), (dp.1 - 30.0).clamp(area.y0 + 4.0, area.y1 - 22.0), tw, 18.0);
            p.rrect(label, 9.0, with_alpha(theme::BG, 0.85));
            p.text(title, label.center().x, label.center().y, 10.5, SEMI, theme::FG, Align::Center, 150.0);
        }
        // home, when it is set: a green pin with a house, to click for the way; out of view it sits on the edge, toward home
        if let Some((hla, hlo)) = self.data.config.home_pos {
            let hp = at(hla, hlo);
            let r = 11.0;
            let inside = hp.0 >= area.x0 + 12.0 && hp.0 <= area.x1 - 12.0 && hp.1 >= area.y0 + r * 2.0 + 14.0 && hp.1 <= area.y1 - 8.0;
            let tip = (hp.0.clamp(area.x0 + 14.0, area.x1 - 14.0), hp.1.clamp(area.y0 + r * 2.0 + 14.0, area.y1 - 8.0));
            let a = if inside { 1.0 } else { 0.6 };
            let hot = p.hovered(rect(tip.0 - r - 2.0, tip.1 - r * 2.0 - 6.0, r * 2.0 + 4.0, r * 2.0 + 8.0));
            p.map_pin(tip.0, tip.1, if hot { r + 1.5 } else { r }, with_alpha(theme::GREEN, a), with_alpha(theme::BG, 0.95 * a));
            p.icon("home", tip.0, tip.1 - r * 1.9, 12.0, with_alpha(theme::BG, a));
            p.hit(rect(tip.0 - r - 3.0, tip.1 - r * 2.0 - 8.0, r * 2.0 + 6.0, r * 2.0 + 10.0), Action::MapHome);
            if inside {
                let name = "Home";
                let tw = (p.fonts.measure(name, 10.5, SEMI) as f64) + 12.0;
                let label = rect((tip.0 + r + 6.0).min(area.x1 - tw - 4.0), tip.1 - r * 1.9 - 9.0, tw, 18.0);
                p.rrect(label, 9.0, with_alpha(theme::BG, 0.9));
                p.text(name, label.center().x, label.center().y, 10.5, SEMI, theme::FG, Align::Center, 60.0);
            }
        }
        // me: the doubt around the fix, then the dot
        let r = map::metres_to_px(v.accuracy_m, v.me.0, v.zoom).clamp(0.0, 220.0);
        if r > 8.0 {
            p.circle(me.0, me.1, r, with_alpha(theme::RAIN, 0.16));
            p.circle_stroke(me.0, me.1, r, 1.0, with_alpha(theme::RAIN, 0.6));
        }
        p.circle(me.0, me.1, 7.5, theme::FG);
        p.circle(me.0, me.1, 5.5, theme::RAIN);
        // bottom left: the three ways, and the distance and time of the chosen one
        if let Some((d, _)) = &v.dest {
            let dist = map::fmt_distance(map::distance_m(v.me, *d));
            let (trip, pending, error) = match route {
                Some(r) => (r.trip.as_ref(), false, r.error.clone()),
                None if self.map_dest.is_some() => (None, true, None),
                None => (self.trip_event().and_then(|e| self.trip_info(&e.id, self.cfg.travel_mode)).and_then(|t| t.trip.as_ref()), false, None),
            };
            let text = match (trip, pending, error) {
                (Some(t), _, _) => format!("{dist} · {}", island_core::travel::fmt_secs(t.secs)),
                (None, true, _) => format!("{dist} · …"),
                (None, _, Some(e)) => format!("{dist} · {e}"),
                _ => dist,
            };
            let mut x = area.x0 + 8.0;
            let by = area.y1 - 34.0;
            for mode in [island_core::travel::Mode::Car, island_core::travel::Mode::Walk, island_core::travel::Mode::Transit, island_core::travel::Mode::Train] {
                let b = rect(x, by, 28.0, 28.0);
                let on = mode == self.cfg.travel_mode;
                p.rrect(b, 14.0, if on { theme::RAIN } else { with_alpha(theme::BG, 0.85) });
                p.icon(mode.icon(), b.center().x, b.center().y, 14.0, if on { theme::BG } else if p.hovered(b) { theme::ACCENT } else { theme::FG });
                p.hit(b, Action::TravelMode(mode));
                x += 32.0;
            }
            let cw = (p.fonts.measure(&text, 11.0, SEMI) as f64 + 16.0).min(area.x1 - x - 150.0);
            let chip = rect(x + 2.0, by + 3.0, cw.max(40.0), 22.0);
            p.rrect(chip, 11.0, with_alpha(theme::BG, 0.85));
            p.text(&text, chip.center().x, chip.center().y, 11.0, SEMI, theme::FG, Align::Center, chip.width() as f32 - 10.0);
            p.hit(chip, Action::MapRoute);
        }
        // top left: search a place, or the place picked (with a cross to forget it)
        let bar = rect(area.x0 + 8.0, area.y0 + 8.0, area.width() - 16.0 - 46.0, 30.0);
        match self.edit.as_ref().filter(|e| e.field == Field::MapSearch) {
            Some(e) => self.edit_row(p, bar, e, "Search a place, an address..."),
            None => match &self.map_dest {
                Some(d) => {
                    let tw = (p.fonts.measure(&d.name, 11.5, SEMI) as f64).min(bar.width() - 80.0) + 56.0;
                    let chip = rect(bar.x0, bar.y0, tw, 30.0);
                    p.rrect(chip, 15.0, theme::BG);
                    p.rrect_stroke(chip, 15.0, 1.0, with_alpha(theme::FG, 0.28));
                    p.icon("map-pin", chip.x0 + 16.0, chip.center().y, 13.0, theme::ORANGE);
                    p.text(&d.name, chip.x0 + 30.0, chip.center().y, 11.5, SEMI, theme::FG, Align::Left, (tw - 62.0) as f32);
                    p.hit(chip, Action::EditField(Field::MapSearch));
                    let x = rect(chip.x1 - 28.0, chip.y0, 28.0, 30.0);
                    p.icon("x", x.center().x, x.center().y, 12.0, if p.hovered(x) { theme::FG } else { theme::DIM });
                    p.hit(x, Action::MapClearDest);
                }
                None => {
                    let tw = p.button_width("Search") + 18.0;
                    let chip = rect(bar.x0, bar.y0, tw, 30.0);
                    // an opaque dark plate with a light edge: it reads over any map, light or dark
                    p.rrect(chip, 15.0, if p.hovered(chip) { theme::HOT_CHIP } else { theme::BG });
                    p.rrect_stroke(chip, 15.0, 1.0, with_alpha(theme::FG, 0.28));
                    p.icon("search", chip.x0 + 17.0, chip.center().y, 13.0, theme::ACCENT);
                    p.text("Search", chip.x0 + 31.0, chip.center().y, 11.5, SEMI, theme::ACCENT, Align::Left, tw as f32 - 30.0);
                    p.hit(chip, Action::EditField(Field::MapSearch));
                }
            },
        }
        // what the search found, over the map
        if self.data.places_searching || self.data.places.is_some() {
            let places = self.data.places.clone().unwrap_or_default();
            let rows = if places.is_empty() { 1 } else { places.len().min(5) };
            let plate = rect(bar.x0, bar.y1 + 4.0, bar.width(), rows as f64 * 32.0 + 8.0);
            p.rrect(plate, 14.0, with_alpha(theme::BG, 0.94));
            if places.is_empty() {
                p.text(if self.data.places_searching { "Searching…" } else { "No place found" }, plate.center().x, plate.center().y, 11.5, REG, theme::DIM, Align::Center, 260.0);
            }
            for (n, pl) in places.iter().take(5).enumerate() {
                let row = rect(plate.x0 + 4.0, plate.y0 + 4.0 + n as f64 * 32.0, plate.width() - 8.0, 30.0);
                if p.hovered(row) {
                    p.rrect(row, 10.0, theme::HOT_CHIP);
                }
                let nw = (p.fonts.measure(&pl.name, 11.5, SEMI) as f64).min(row.width() * 0.5);
                p.text(&pl.name, row.x0 + 10.0, row.center().y, 11.5, SEMI, theme::FG, Align::Left, nw as f32);
                p.text(&pl.detail, row.x0 + 16.0 + nw, row.center().y, 10.0, REG, theme::DIM, Align::Left, (row.width() - nw - 26.0) as f32);
                p.hit(row, Action::MapPick(n));
            }
        }
        // the zoom, on the right
        for (n, (icon, d)) in [("plus", 1i8), ("minus", -1)].into_iter().enumerate() {
            let b = rect(area.x1 - 38.0, area.y0 + 8.0 + n as f64 * 34.0, 30.0, 30.0);
            p.rrect(b, 15.0, with_alpha(theme::BG, 0.85));
            p.icon(icon, b.center().x, b.center().y, 14.0, if p.hovered(b) { theme::ACCENT } else { theme::FG });
            p.hit(b, Action::MapZoom(d));
        }
        // back to the position, once the map was dragged away
        if self.map_is_panned() {
            let b = rect(area.x1 - 38.0, area.y0 + 8.0 + 68.0, 30.0, 30.0);
            p.rrect(b, 15.0, with_alpha(theme::BG, 0.85));
            p.icon("map-pin", b.center().x, b.center().y, 14.0, if p.hovered(b) { theme::ACCENT } else { theme::RAIN });
            p.hit(b, Action::MapRecenter);
        }
        // the credit the tile providers ask for
        let credit = map::credit_for(&self.cfg.map_tile_url);
        let cw = p.fonts.measure(credit, 8.5, REG) as f64 + 10.0;
        p.rrect(rect(area.x1 - cw - 4.0, area.y1 - 17.0, cw, 13.0), 6.5, with_alpha(theme::BG, 0.7));
        p.text(credit, area.x1 - cw / 2.0 - 4.0, area.y1 - 10.5, 8.5, REG, theme::DIM, Align::Center, 160.0);
        p.scene.pop_layer();
        // the map is dragged, not clicked: a click on it does nothing (the buttons below open the browser)
        p.hit(area, Action::MapPan);
        // under the map: the numbers, and what can be done with them
        p.stagger(2);
        let y = area.y1 + 8.0 + 15.0;
        let acc = if v.from_settings {
            "the location set in the settings".to_string()
        } else if v.accuracy_m >= 1000.0 {
            format!("± {:.1} km", v.accuracy_m / 1000.0)
        } else {
            format!("± {} m", v.accuracy_m.round() as i64)
        };
        let mut x = w - 14.0;
        for (icon, action) in [("refresh", Action::MapRefresh), ("clipboard-text", Action::MapCopy), ("external-link", Action::MapOpen)] {
            p.icon_button(x - 14.0, y, 30.0, icon, 15.0, action);
            x -= 34.0;
        }
        if v.dest.is_some() {
            let label = if self.map_both { "Me" } else { "Both" };
            let bw = p.button_width(label);
            p.button(rect(x - bw, y - 14.0, bw, 28.0), label, Action::MapView);
            x -= bw + 8.0;
        }
        p.text(&format!("{:.5}, {:.5}  {acc}", v.me.0, v.me.1), 20.0, y, 11.0, REG, theme::DIM, Align::Left, (x - 28.0) as f32);
    }

    /// Where the attention went today, and the last half hour.
    pub(super) fn face_activity(&self, p: P, w: f64, _h: f64) {
        let hy = self.page_header(p, w, "Activity");
        // two tabs on the right of the header
        for (n, name) in ["Today", "Rewind"].into_iter().enumerate() {
            let tw = p.button_width(name);
            let x1 = w - 14.0 - if n == 0 { p.button_width("Rewind") + 6.0 } else { 0.0 };
            let r = rect(x1 - tw, hy - 14.0, tw, 28.0);
            let on = self.activity_tab == n;
            p.rrect(r, 14.0, if on { theme::ACCENT } else if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
            p.text(name, r.center().x, r.center().y, 12.0, SEMI, if on { theme::ON_ACCENT } else { theme::FG }, Align::Center, (tw - 8.0) as f32);
            p.hit(r, Action::ActivityTab(n));
        }
        let top = 14.0 + 34.0 + 8.0;
        if self.activity_tab == 0 {
            self.attention_tab(p, w, top);
        } else {
            self.rewind_tab(p, w, top);
        }
    }

    pub(super) fn attention_tab(&self, p: P, w: f64, top: f64) {
        let ds = {
            let off = self.data.utc_offset_secs;
            let t = clock::civil(self.now, off);
            clock::midnight_ms(t.year, t.month, t.day, off)
        };
        let apps = self.ledger.today(self.now, ds);
        if apps.is_empty() {
            p.stagger(1);
            p.text("Nothing recorded yet", w / 2.0, top + 40.0, 12.0, REG, theme::DIM, Align::Center, 260.0);
            p.text("It follows the focused window (niri)", w / 2.0, top + 60.0, 10.0, REG, theme::DIM, Align::Center, 300.0);
            return;
        }
        let total: i64 = apps.iter().map(|(_, ms)| ms).sum();
        let biggest = apps[0].1.max(1);
        for (n, (app, ms)) in apps.iter().take(5).enumerate() {
            p.stagger(1 + n);
            let y = top + n as f64 * 30.0 + 15.0;
            p.text(app, 20.0, y, 12.0, SEMI, theme::FG, Align::Left, 110.0);
            p.text(&format!("{}%", (*ms as f64 / total as f64 * 100.0).round() as i64), 188.0, y, 10.0, REG, theme::DIM, Align::Right, 34.0);
            p.bar(rect(196.0, y - 3.0, w - 196.0 - 78.0, 6.0), *ms as f64 / biggest as f64, theme::ACCENT, theme::TRACK);
            p.text(&duration::fmt_took(*ms), w - 20.0, y, 11.0, REG, theme::FG, Align::Right, 64.0);
        }
        let y = top + 5.0 * 30.0 + 15.0;
        p.stagger(7);
        let current = self.ledger.current().map(|(a, _, _)| a.to_string());
        let streak = current.as_ref().map_or(0, |c| self.ledger.streak(self.now, 60_000, |a| a == c));
        let line = match (&current, streak >= 5 * 60_000) {
            (Some(c), true) => format!("Active today {} · {} min on {c} now", duration::fmt_took(total), streak / 60_000),
            _ => format!("Active today {}", duration::fmt_took(total)),
        };
        p.text(&line, 20.0, y, 11.0, REG, theme::DIM, Align::Left, (w - 40.0) as f32);
    }

    pub(super) fn rewind_tab(&self, p: P, w: f64, top: f64) {
        let rows = self.rewind_rows();
        if rows.is_empty() {
            p.stagger(1);
            p.text("Nothing in the last half hour", w / 2.0, top + 40.0, 12.0, REG, theme::DIM, Align::Center, 260.0);
            return;
        }
        let off = self.data.utc_offset_secs;
        for (n, r) in rows.iter().enumerate() {
            p.stagger(1 + n);
            let row = rect(8.0, top + n as f64 * 30.0, w - 16.0, 28.0);
            let clickable = r.copy.is_some();
            if clickable && p.hovered(row) {
                p.rrect(row, 8.0, theme::CHIP);
            }
            let cy = row.center().y;
            p.text(&clock::civil(r.at_ms, off).hhmm(), 20.0, cy, 11.0, REG, theme::DIM, Align::Left, 44.0);
            p.icon(r.icon, 82.0, cy, 14.0, theme::DIM);
            let text: String = r.text.split_whitespace().collect::<Vec<_>>().join(" ");
            p.text(&text, 100.0, cy, 12.0, REG, theme::FG, Align::Left, (w - 100.0 - 24.0) as f32);
            if clickable {
                p.hit(row, Action::RewindCopy(n));
            }
        }
    }

    const SEND_ROWS: usize = 5;

    pub(crate) fn send_height(&self) -> f64 {
        let rows = self.data.peers.len().clamp(1, Self::SEND_ROWS) as f64;
        14.0 + 34.0 + 8.0 + rows * 34.0 + if self.data.peers_scanning && !self.data.peers.is_empty() { 14.0 } else { 0.0 } + 10.0
    }

    /// "report.pdf" or "3 files · 4.2 MB"
    pub(crate) fn send_summary(&self) -> String {
        let sizes: u64 = self.send_files.iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
        match self.send_files.as_slice() {
            [one] => format!("{} · {}", one.rsplit(['/', '\\']).next().unwrap_or(one), island_core::localsend::size_text(sizes)),
            many => format!("{} files · {}", many.len(), island_core::localsend::size_text(sizes)),
        }
    }

    pub(super) fn face_send(&self, p: P, w: f64, _h: f64) {
        let hy = self.page_header(p, w, "Send to…");
        let summary = self.send_summary();
        p.text(&summary, w - 14.0, hy, 11.0, REG, theme::DIM, Align::Right, (w - 130.0) as f32);
        let mut y = 14.0 + 34.0 + 8.0;
        if self.data.peers.is_empty() {
            p.stagger(1);
            let t = if self.data.peers_scanning { "Looking for devices with LocalSend…" } else { "No device found. Open LocalSend on the other one." };
            p.text(t, w / 2.0, y + 16.0, 12.0, REG, theme::DIM, Align::Center, (w - 40.0) as f32);
            return;
        }
        for (n, peer) in self.data.peers.iter().take(Self::SEND_ROWS).enumerate() {
            p.stagger(1 + n);
            let r = rect(14.0, y, w - 28.0, 30.0);
            p.rrect(r, 11.0, if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
            let icon = match peer.kind.as_str() {
                "mobile" => "device-mobile",
                "desktop" => "device-laptop",
                _ => "devices",
            };
            p.icon(icon, r.x0 + 18.0, r.center().y, 15.0, theme::FG);
            p.text(&peer.alias, r.x0 + 34.0, r.center().y, 12.0, SEMI, theme::FG, Align::Left, (r.width() - 150.0) as f32);
            if !peer.model.is_empty() {
                p.text(&peer.model, r.x1 - 12.0, r.center().y, 10.0, REG, theme::DIM, Align::Right, 110.0);
            }
            p.hit(r, Action::SendTo(n));
            y += 34.0;
        }
        if self.data.peers_scanning {
            p.text("still looking…", w / 2.0, y + 4.0, 10.0, REG, theme::DIM, Align::Center, 200.0);
        }
    }

    /// How to get to the next event with a place: the three ways, and the time for the chosen one.
    pub(super) fn trip_row(&self, p: P, w: f64, y: f64) {
        use island_core::travel::{self, Mode};
        let Some(e) = self.trip_event() else { return };
        p.stagger(9);
        let off = self.data.utc_offset_secs;
        for (n, mode) in [Mode::Car, Mode::Walk, Mode::Transit, Mode::Train].into_iter().enumerate() {
            let r = rect(14.0 + n as f64 * 32.0, y, 28.0, 28.0);
            let on = mode == self.cfg.travel_mode;
            p.rrect(r, 14.0, if on { theme::ACCENT } else if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
            p.icon(mode.icon(), r.center().x, r.center().y, 15.0, if on { theme::ON_ACCENT } else { theme::FG });
            p.hit(r, Action::TravelMode(mode));
        }
        let x = 14.0 + 4.0 * 32.0 + 8.0;
        let text = match self.trip_info(&e.id, self.cfg.travel_mode) {
            None => "Looking up the way…".to_string(),
            Some(t) => match (&t.trip, &t.error) {
                (Some(trip), _) => {
                    let leave = trip.leave_ms.map(|l| format!(" · leave {}", crate::clock::civil(l, off).hhmm())).unwrap_or_default();
                    format!("{}{leave} · {}", travel::fmt_secs(trip.secs), e.title)
                }
                (None, Some(err)) => err.clone(),
                _ => String::new(),
            },
        };
        p.text(&text, x, y + 14.0, 12.0, REG, theme::FG, Align::Left, (w - x - 14.0) as f32);
    }

    /// Rows the today page lists at most (the now line is one more).
    const TODAY_ROWS: usize = 7;

    /// Today's items that fit the page, the place of the now line among them, and how many finished ones were left out.
    pub(crate) fn today_view(&self) -> (Vec<timeline::Item>, usize, usize) {
        let off = self.data.utc_offset_secs;
        let t = clock::civil(self.now, off);
        let (start, end) = (clock::midnight_ms(t.year, t.month, t.day, off), clock::midnight_ms(t.year, t.month, t.day + 1, off));
        let reminders: Vec<(i64, String)> = self.reminders.iter().map(|r| (r.due_ms, r.text.clone())).collect();
        let items = timeline::build(&self.data.events, &reminders, start, end);
        let at = timeline::now_index(&items, self.now);
        // what is over goes first when there is no room, but the last finished one stays as context
        let skip = items.len().saturating_sub(Self::TODAY_ROWS).min(at.saturating_sub(1));
        let skip = if items.iter().take(skip).any(|i| i.all_day) { 0 } else { skip };
        let shown: Vec<_> = items.into_iter().skip(skip).take(Self::TODAY_ROWS).collect();
        (shown, at - skip, skip)
    }

    /// What is running right now, for the top of the Today page: the timer and the long command or activity.
    pub(crate) fn running_now(&self) -> Vec<RunningRow> {
        let mut rows = Vec::new();
        if let Some(t) = &self.timer {
            let (label, right) = match t.mode {
                TimerMode::Stopwatch => ("Stopwatch".to_string(), self.timer_text()),
                TimerMode::Countdown => (if t.label.is_empty() { "Timer".to_string() } else { t.label.clone() }, self.timer_text()),
                TimerMode::Pomodoro => (format!("{} · round {}", if t.phase == island_core::pomodoro::Phase::Break { "Break" } else { "Focus" }, t.round), self.timer_text()),
            };
            let frac = (t.mode != TimerMode::Stopwatch && t.total_ms > 0).then(|| 1.0 - self.remaining_ms() as f64 / t.total_ms as f64);
            rows.push(RunningRow { icon: self.timer_icon().to_string(), label, right, frac, page: Page::Timer });
        }
        for e in &self.extra_timers {
            let left = e.end_ms.map_or(0, |x| (x - self.now).max(0));
            let frac = (e.total_ms > 0).then(|| 1.0 - left as f64 / e.total_ms as f64);
            rows.push(RunningRow { icon: "hourglass".into(), label: if e.label.is_empty() { "Timer".into() } else { e.label.clone() }, right: duration::fmt_remaining(left), frac, page: Page::Timer });
        }
        for o in self.ops.running() {
            let label = if o.detail.is_empty() { o.label.clone() } else { format!("{} · {}", o.label, o.detail) };
            rows.push(RunningRow { icon: o.icon.clone().unwrap_or_else(|| "terminal-2".into()), label, right: o.frac.map_or("…".to_string(), |f| format!("{:.0}%", f * 100.0)), frac: o.frac, page: Page::Home });
        }
        rows
    }

    /// Tomorrow's first items, shown when nothing is left of today (what is behind the "now" line only).
    pub(crate) fn tomorrow_view(&self) -> Vec<timeline::Item> {
        let (items, now_at, _) = self.today_view();
        if now_at < items.len() {
            return vec![];
        }
        self.tomorrow_items().into_iter().take(3).collect()
    }

    /// Everything planned for tomorrow, in time order.
    pub(crate) fn tomorrow_items(&self) -> Vec<timeline::Item> {
        let off = self.data.utc_offset_secs;
        let t = clock::civil(self.now, off);
        let (start, end) = (clock::midnight_ms(t.year, t.month, t.day + 1, off), clock::midnight_ms(t.year, t.month, t.day + 2, off));
        let reminders: Vec<(i64, String)> = self.reminders.iter().map(|r| (r.due_ms, r.text.clone())).collect();
        timeline::build(&self.data.events, &reminders, start, end)
    }

    /// How many pauses of at least 45 minutes sit between the shown events.
    pub(crate) fn today_gaps(&self, items: &[timeline::Item]) -> usize {
        items.windows(2).filter(|w| timeline::gap_between(&w[0], &w[1], GAP_MIN_MS).is_some()).count()
    }

    /// Deadlines worth a row on the page: the next three within two weeks.
    pub(crate) fn deadline_rows(&self) -> Vec<&Deadline> {
        self.deadlines_soon(14).into_iter().take(3).collect()
    }

    /// The trains after the pinned one, when it is late (3 minutes or more) or cancelled; None otherwise.
    pub(crate) fn train_next_line(&self) -> Option<String> {
        use island_core::train::{next_line, status_line, Mood};
        let st = self.data.train_status.as_ref()?;
        let (_, mood) = status_line(Some(st));
        let line = next_line(&st.next);
        (matches!(mood, Mood::Late | Mood::Bad) && !line.is_empty()).then_some(line)
    }

    /// The to-dos that fit on the page.
    pub(crate) fn todo_rows(&self) -> usize {
        self.open_todos().len().min(4)
    }

    pub(crate) fn today_height(&self) -> f64 {
        let (items, _, _) = self.today_view();
        let tomorrow = self.tomorrow_view().len();
        self.running_now().len() as f64 * 30.0
            + if self.data.train.is_some() { 30.0 + if self.train_next_line().is_some() { 16.0 } else { 0.0 } } else { 0.0 }
            + 14.0 + 34.0 + 6.0
            + if items.is_empty() { 40.0 } else { items.len() as f64 * 28.0 + 18.0 + self.today_gaps(&items) as f64 * 18.0 }
            + if tomorrow > 0 { 22.0 + tomorrow as f64 * 28.0 } else { 0.0 }
            + if self.pins.is_empty() { 0.0 } else { 22.0 + self.pins.len().min(PIN_ROWS) as f64 * 28.0 }
            + if self.deadline_rows().is_empty() { 0.0 } else { 22.0 + self.deadline_rows().len() as f64 * 28.0 }
            + if self.todo_rows() > 0 { 22.0 + self.todo_rows() as f64 * 28.0 } else { 0.0 }
            + if self.trip_event().is_some() { 38.0 } else { 0.0 }
            + 10.0
    }

    pub(super) fn face_today(&self, p: P, w: f64, _h: f64) {
        let hy = self.page_header(p, w, "Today");
        let (items, now_at, skipped) = self.today_view();
        let off = self.data.utc_offset_secs;
        let all = {
            let t = clock::civil(self.now, off);
            let reminders: Vec<(i64, String)> = self.reminders.iter().map(|r| (r.due_ms, r.text.clone())).collect();
            timeline::build(&self.data.events, &reminders, clock::midnight_ms(t.year, t.month, t.day, off), clock::midnight_ms(t.year, t.month, t.day + 1, off))
        };
        let leave = self.leave_plan(&all).filter(|(l, _)| *l > self.now);
        let summary = match leave {
            Some((l, ev)) => format!("Leave {} · {} {}", clock::civil(l, off).hhmm(), clock::civil(ev.at_ms, off).hhmm(), ev.title),
            None => match timeline::next_up(&all, self.now) {
                Some((i, m)) => format!("{} · {}", i.title, if m < 60 { format!("in {m} min") } else { format!("in {} h {} min", m / 60, m % 60) }),
                None if all.is_empty() => String::new(),
                None => "Nothing more today".to_string(),
            },
        };
        p.text(&summary, w - 14.0, hy, 11.0, REG, theme::DIM, Align::Right, (w - 150.0) as f32);
        let mut y = 14.0 + 34.0 + 6.0;
        for (n, r) in self.running_now().iter().enumerate() {
            p.stagger(n);
            let row = rect(8.0, y, w - 16.0, 26.0);
            if r.page != Page::Home && p.hovered(row) {
                p.rrect(row, 8.0, theme::CHIP);
            }
            let cy = y + 13.0;
            p.icon(&r.icon, 22.0, cy, 14.0, theme::ACCENT);
            p.text(&r.label, 40.0, cy, 12.0, SEMI, theme::FG, Align::Left, (w - 40.0 - 80.0) as f32);
            p.text(&r.right, w - 16.0, cy, 11.0, REG, theme::DIM, Align::Right, 64.0);
            if let Some(f) = r.frac {
                p.bar(rect(40.0, y + 23.0, w - 40.0 - 16.0, 2.0), f.clamp(0.0, 1.0), theme::ACCENT, with_alpha(theme::DIM, 0.3));
            }
            if r.page != Page::Home {
                p.hit(row, Action::Goto(r.page));
            }
            y += 30.0;
        }
        // the usual train: when, where, and what the board says about it
        if let Some(t) = &self.data.train {
            use island_core::train::{status_line, Mood};
            let cy = y + 13.0;
            p.icon("train", 22.0, cy, 14.0, theme::ACCENT);
            let (words, mood) = status_line(self.data.train_status.as_ref());
            let color = match mood {
                Mood::Good => theme::GREEN,
                Mood::Late => theme::ORANGE,
                Mood::Bad => theme::RED,
                Mood::Quiet => theme::DIM,
            };
            let to = if t.arrival.is_empty() { t.to_name.clone() } else { format!("{} {}", t.to_name, t.arrival) };
            p.text(&format!("{} {} → {}", t.time, t.from_name, to), 40.0, cy, 12.0, SEMI, theme::FG, Align::Left, (w - 40.0 - 130.0) as f32);
            p.text(if words.is_empty() { &t.number } else { &words }, w - 16.0, cy, 11.0, if words.is_empty() { REG } else { SEMI }, if words.is_empty() { theme::DIM } else { color }, Align::Right, 128.0);
            y += 30.0;
            // when it is late or cancelled: what else goes there
            if let Some(line) = self.train_next_line() {
                p.text(&format!("Next: {line}"), 40.0, y - 3.0, 10.5, REG, theme::DIM, Align::Left, (w - 56.0) as f32);
                y += 16.0;
            }
        }
        if items.is_empty() {
            p.text("Nothing planned today", w / 2.0, y + 20.0, 12.0, REG, theme::DIM, Align::Center, 260.0);
            y += 40.0;
        }
        let line = |p: P, y: f64| {
            p.rrect(rect(14.0, y + 8.0, w - 28.0 - 34.0, 1.5), 0.75, with_alpha(theme::ACCENT, 0.55));
            p.circle(18.0, y + 8.75, 3.5, theme::ACCENT);
            p.text("now", w - 14.0, y + 8.75, 9.0, SEMI, theme::ACCENT, Align::Right, 40.0);
        };
        for (n, it) in items.iter().enumerate() {
            if n == now_at {
                line(p, y);
                y += 18.0;
            }
            // a real pause between two lectures is time to study in
            if let Some(gap) = n.checked_sub(1).and_then(|k| timeline::gap_between(&items[k], it, GAP_MIN_MS)) {
                if it.at_ms > self.now {
                    p.text(&format!("Free · {}", island_core::duration::fmt_took(gap)), 86.0, y + 4.0, 10.0, REG, with_alpha(theme::DIM, 0.8), Align::Left, 200.0);
                }
                y += 18.0;
            }
            p.stagger(1 + n);
            let past = !it.all_day && it.end_ms.max(it.at_ms) <= self.now;
            let (fg, dim) = if past { (theme::DIM, with_alpha(theme::DIM, 0.7)) } else { (theme::FG, theme::DIM) };
            let row = rect(8.0, y, w - 16.0, 26.0);
            if it.url.is_some() && p.hovered(row) {
                p.rrect(row, 8.0, theme::CHIP);
            }
            let cy = y + 13.0;
            let when = if it.all_day { "All day".to_string() } else { clock::civil(it.at_ms, off).hhmm() };
            p.text(&when, 20.0, cy, 11.0, REG, dim, Align::Left, 48.0);
            match it.kind {
                timeline::Kind::Event => p.rrect(rect(72.0, cy - 8.0, 3.0, 16.0), 1.5, if past { dim } else { it.color.map(|c| theme::rgb(color::readable_tint(c, true))).unwrap_or(theme::ACCENT) }),
                timeline::Kind::Reminder => p.icon("bell", 74.0, cy, 12.0, dim),
            }
            let ongoing = !it.all_day && it.at_ms <= self.now && it.end_ms > self.now;
            // the room, dim, at the right (before the link's icon): where to go
            let right = if it.url.is_some() { w - 36.0 } else { w - 16.0 };
            let place_w = if it.place.is_empty() { 0.0 } else { (p.fonts.measure(&it.place, 10.5, REG) as f64).min(120.0) };
            if place_w > 0.0 {
                p.text(&it.place, right, cy, 10.5, REG, dim, Align::Right, place_w as f32 + 2.0);
            }
            p.text(&it.title, 86.0, cy, 12.0, if ongoing { SEMI } else { REG }, fg, Align::Left, (right - 86.0 - place_w - if place_w > 0.0 { 10.0 } else { 4.0 }).max(60.0) as f32);
            if let Some(u) = &it.url {
                p.icon("external-link", w - 26.0, cy, 13.0, dim);
                p.hit(row, Action::Join(u.clone()));
            }
            y += 28.0;
        }
        if !items.is_empty() && now_at >= items.len() {
            line(p, y);
            y += 18.0;
        }
        let _ = skipped;
        // what is left of today is done: the start of tomorrow
        let tomorrow = self.tomorrow_view();
        if !tomorrow.is_empty() {
            p.text("Tomorrow", 20.0, y + 8.0, 10.0, SEMI, theme::DIM, Align::Left, 120.0);
            y += 22.0;
            for it in &tomorrow {
                let cy = y + 13.0;
                let when = if it.all_day { "All day".to_string() } else { clock::civil(it.at_ms, off).hhmm() };
                p.text(&when, 20.0, cy, 11.0, REG, theme::DIM, Align::Left, 48.0);
                match it.kind {
                    timeline::Kind::Event => p.rrect(rect(72.0, cy - 8.0, 3.0, 16.0), 1.5, it.color.map(|c| theme::rgb(color::readable_tint(c, true))).unwrap_or(theme::ACCENT)),
                    timeline::Kind::Reminder => p.icon("bell", 74.0, cy, 12.0, theme::DIM),
                }
                let place_w = if it.place.is_empty() { 0.0 } else { (p.fonts.measure(&it.place, 10.5, REG) as f64).min(120.0) };
                if place_w > 0.0 {
                    p.text(&it.place, w - 16.0, cy, 10.5, REG, theme::DIM, Align::Right, place_w as f32 + 2.0);
                }
                p.text(&it.title, 86.0, cy, 12.0, REG, theme::FG, Align::Left, (w - 16.0 - 86.0 - place_w - if place_w > 0.0 { 10.0 } else { 4.0 }).max(60.0) as f32);
                y += 28.0;
            }
        }
        // notes kept for later: a click copies, the cross takes it off
        if !self.pins.is_empty() {
            p.text("Pinned", 20.0, y + 8.0, 10.0, SEMI, theme::DIM, Align::Left, 120.0);
            y += 22.0;
            for (n, t) in self.pins.iter().take(PIN_ROWS).enumerate() {
                let row = rect(8.0, y, w - 16.0, 26.0);
                let hot = p.hovered(row);
                if hot {
                    p.rrect(row, 8.0, theme::CHIP);
                }
                let cy = y + 13.0;
                p.icon("pin", 26.0, cy, 13.0, theme::ACCENT);
                p.text(t, 44.0, cy, 12.0, REG, theme::FG, Align::Left, (w - 44.0 - 34.0) as f32);
                p.hit(row, Action::PinCopy(n));
                p.icon_button(row.x1 - 14.0, cy, 22.0, "x", 11.0, Action::PinRemove(n));
                y += 28.0;
            }
        }
        // exams and hand-ins coming up
        let due = self.deadline_rows();
        if !due.is_empty() {
            p.text("Deadlines", 20.0, y + 8.0, 10.0, SEMI, theme::DIM, Align::Left, 120.0);
            y += 22.0;
            for (n, d) in due.iter().enumerate() {
                let left = d.day - self.day_number(self.now);
                let row = rect(8.0, y, w - 16.0, 26.0);
                let hot = p.hovered(row);
                if hot {
                    p.rrect(row, 8.0, theme::CHIP);
                }
                let cy = y + 13.0;
                let urgent = left <= 3;
                p.icon("flame", 26.0, cy, 13.0, if urgent { theme::ORANGE } else { theme::DIM });
                p.text(&d.text, 44.0, cy, 12.0, REG, theme::FG, Align::Left, (w - 44.0 - 90.0) as f32);
                let when = match left { i64::MIN..=-1 => "yesterday".to_string(), 0 => "today".to_string(), 1 => "tomorrow".to_string(), n => format!("in {n} d") };
                p.text(&when, w - 18.0, cy, 11.0, if urgent { SEMI } else { REG }, if urgent { theme::ORANGE } else { theme::DIM }, Align::Right, 70.0);
                p.hit(row, Action::DeadlineDone(n));
                y += 28.0;
            }
        }
        // the list: a circle to tick, a click removes it
        if self.todo_rows() > 0 {
            let open = self.open_todos();
            let more = open.len().saturating_sub(self.todo_rows());
            p.text(&if more > 0 { format!("To do · +{more} more") } else { "To do".to_string() }, 20.0, y + 8.0, 10.0, SEMI, theme::DIM, Align::Left, 160.0);
            y += 22.0;
            for (n, t) in open.iter().take(self.todo_rows()).copied() {
                let row = rect(8.0, y, w - 16.0, 26.0);
                let hot = p.hovered(row);
                if hot {
                    p.rrect(row, 8.0, theme::CHIP);
                }
                let cy = y + 13.0;
                p.circle_stroke(26.0, cy, 7.0, 1.5, if hot { theme::ACCENT } else { theme::DIM });
                if hot {
                    p.icon("check", 26.0, cy, 10.0, theme::ACCENT);
                }
                p.text(&t.text, 44.0, cy, 12.0, REG, theme::FG, Align::Left, (w - 44.0 - 24.0) as f32);
                p.hit(row, Action::TodoDone(n));
                y += 28.0;
            }
        }
        self.trip_row(p, w, y + 6.0);
    }

    /// Rows of each section of the audio page.
    pub(super) fn audio_sections(&self) -> (usize, usize, usize) {
        (self.data.sinks.len().min(4), self.data.sources.len().min(3), self.data.streams.len().min(4))
    }

    pub(crate) fn audio_height(&self) -> f64 {
        let (o, i, a) = self.audio_sections();
        let sections = [o, i, a].iter().filter(|n| **n > 0).count().max(1) as f64;
        14.0 + 34.0 + sections * 22.0 + (o + i + a).max(1) as f64 * 34.0 + 10.0
    }

    pub(super) fn face_audio(&self, p: P, w: f64, _h: f64) {
        self.page_header(p, w, "Sound");
        let d = &self.data;
        let mut y = 14.0 + 34.0;
        let mut n = 1;
        if d.sinks.is_empty() && d.sources.is_empty() && d.streams.is_empty() {
            p.text("No audio devices", w / 2.0, y + 24.0, 12.0, REG, theme::DIM, Align::Center, 200.0);
        }
        let mut section = |p: P, y: &mut f64, title: &str| {
            p.stagger(n);
            n += 1;
            p.text(title, 20.0, *y + 12.0, 10.0, SEMI, theme::DIM, Align::Left, 200.0);
            *y += 22.0;
        };
        let device = |p: P, y: f64, dev: &Sink, current: bool, icon: &str, action: Action| {
            let r = rect(14.0, y, w - 28.0, 30.0);
            p.rrect(r, 11.0, if current { with_alpha(theme::ACCENT, 0.16) } else if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
            p.icon(icon, r.x0 + 18.0, r.center().y, 15.0, if current { theme::ACCENT } else { theme::FG });
            p.text(&dev.label, r.x0 + 34.0, r.center().y, 12.0, if current { SEMI } else { REG }, theme::FG, Align::Left, (r.width() - 70.0) as f32);
            if current {
                p.icon("check", r.x1 - 18.0, r.center().y, 14.0, theme::ACCENT);
            }
            p.hit(r, action);
        };
        if !d.sinks.is_empty() {
            section(p, &mut y, "OUTPUT");
            for s in d.sinks.iter().take(4) {
                let bt = s.name.contains("bluez") || s.name.contains("usb");
                device(p, y, s, d.sink.as_ref() == Some(&s.name), if bt { "headphones" } else { "device-speaker" }, Action::Cmd(Command::SetSink(s.name.clone())));
                y += 34.0;
            }
        }
        if !d.sources.is_empty() {
            section(p, &mut y, "INPUT");
            for s in d.sources.iter().take(3) {
                device(p, y, s, d.source.as_ref() == Some(&s.name), "microphone", Action::Cmd(Command::SetSource(s.name.clone())));
                y += 34.0;
            }
        }
        if !d.streams.is_empty() {
            section(p, &mut y, "APPS");
            for a in d.streams.iter().take(4) {
                let r = rect(14.0, y, w - 28.0, 30.0);
                p.rrect(r, 11.0, theme::CHIP);
                let mute = rect(r.x0, y, 34.0, 30.0);
                p.icon(if a.muted { "volume-3" } else { "volume-2" }, r.x0 + 18.0, r.center().y, 15.0, if a.muted { theme::DIM } else { theme::FG });
                p.hit(mute, Action::Cmd(Command::ToggleStreamMute(a.id)));
                p.text(&a.name, r.x0 + 34.0, r.center().y, 12.0, REG, if a.muted { theme::DIM } else { theme::FG }, Align::Left, (r.width() - 34.0 - 112.0) as f32);
                let (minus, plus) = (rect(r.x1 - 104.0, y, 30.0, 30.0), rect(r.x1 - 32.0, y, 30.0, 30.0));
                p.icon("minus", minus.center().x, r.center().y, 13.0, if p.hovered(minus) { theme::FG } else { theme::DIM });
                p.hit(minus, Action::Cmd(Command::SetStreamVolume { id: a.id, percent: (a.volume as u32).saturating_sub(10) }));
                p.text(&format!("{}%", a.volume), r.x1 - 53.0, r.center().y, 11.0, REG, theme::FG, Align::Center, 40.0);
                p.icon("plus", plus.center().x, r.center().y, 13.0, if p.hovered(plus) { theme::FG } else { theme::DIM });
                p.hit(plus, Action::Cmd(Command::SetStreamVolume { id: a.id, percent: a.volume as u32 + 10 }));
                y += 34.0;
            }
        }
    }

    pub(super) fn face_notifs(&self, p: P, w: f64, _h: f64) {
        let hy = self.page_header(p, w, "Notifications");
        let clear = p.button_width("Clear");
        if !self.history.is_empty() {
            p.button(rect(w - 14.0 - clear, hy - 15.0, clear, 30.0), "Clear", Action::ClearHistory);
        }
        let dnd = rect(w - 14.0 - clear - 8.0 - 76.0, hy - 15.0, 76.0, 30.0);
        let on = self.quiet();
        p.rrect(dnd, 15.0, if on { theme::ACCENT } else if p.hovered(dnd) { theme::HOT_CHIP } else { theme::CHIP });
        p.icon("bell-off", dnd.x0 + 16.0, hy, 14.0, if on { theme::ON_ACCENT } else { theme::FG });
        p.text("DND", dnd.x0 + 28.0, hy, 12.0, SEMI, if on { theme::ON_ACCENT } else { theme::FG }, Align::Left, 40.0);
        p.hit(dnd, Action::ToggleDnd);
        let mut y = 14.0 + 34.0 + 6.0;
        if self.history.is_empty() {
            p.text("No notifications", w / 2.0, y + 20.0, 12.0, REG, theme::DIM, Align::Center, 200.0);
        }
        for (row_n, (i, count)) in self.history_groups().into_iter().take(5).enumerate() {
            let (n, at) = &self.history[i];
            p.stagger(1 + row_n);
            let row = rect(14.0, y, w - 28.0, 40.0);
            p.rrect(row, 12.0, if p.hovered(row) { theme::HOT_CHIP } else { theme::CHIP });
            match n.image.as_ref().or(n.app_icon.as_ref()) {
                Some(img) => draw_image(p, img, row.x0 + 10.0, y + 6.0, 28.0, if n.image.is_some() { 14.0 } else { 7.0 }),
                None => p.icon("bell", row.x0 + 24.0, y + 20.0, 18.0, theme::DIM),
            }
            let tx = row.x0 + 48.0;
            let tmax = (row.width() - 48.0 - 66.0) as f32;
            let many = if count > 1 { format!("  ·  {count} messages") } else { String::new() };
            p.text(&format!("{}  ·  {}{many}", n.app, ago(self.now - at)), tx, y + 12.0, 10.0, REG, theme::DIM, Align::Left, tmax);
            // a conversation of several shows its latest words after the name
            let title = if n.summary.is_empty() { n.body.clone() } else { n.summary.clone() };
            let tw = (p.fonts.measure(&title, 12.0, SEMI) as f64).min(tmax as f64 * 0.6);
            p.text(&title, tx, y + 27.0, 12.0, SEMI, theme::FG, Align::Left, tmax);
            if count > 1 && !n.summary.is_empty() && !n.body.is_empty() {
                p.text(&n.body, tx + tw + 8.0, y + 27.0, 11.0, REG, theme::DIM, Align::Left, (tmax as f64 - tw - 8.0).max(20.0) as f32);
            }
            p.icon_button(row.x1 - 18.0, y + 20.0, 26.0, "x", 13.0, Action::HistoryRemove(i));
            if !crate::island::is_own_notification(n) {
                p.icon_button(row.x1 - 44.0, y + 20.0, 26.0, "bell-off", 12.0, Action::MuteApp(i));
            }
            p.hit(row, Action::HistoryOpen(i));
            y += 46.0;
        }
    }

    pub(super) fn draw_bubble(&self, p: P, side: Side, w: f64, h: f64) {
        // the activity a pill stands for: the main one, or the n-th of the others
        let op = match side {
            Side::OpAt(k) => self.ops.also.get(k as usize),
            _ => self.ops.op.as_ref(),
        };
        let (icon, text) = match side {
            Side::Timer => (self.timer_icon(), match &self.timer {
                Some(t) if t.mode == TimerMode::Stopwatch => duration::fmt_elapsed(self.now - t.start_ms),
                _ => duration::fmt_bubble(self.remaining_ms()),
            }),
            Side::Meeting => ("calendar-event", self.next_event().map(|e| duration::fmt_bubble((e.start_ms - self.now).max(0))).unwrap_or_default()),
            Side::Focus => ("target", self.focus_minutes_left().map_or("on".to_string(), |m| format!("{m}m"))),
            Side::Media => ("music", String::new()),
            Side::Op | Side::OpAt(_) => match &op {
                Some(o) => (
                    match (o.state, o.icon.as_deref()) {
                        (OpState::Fail, _) => "alert-triangle",
                        (OpState::Done, _) => "circle-check",
                        (OpState::Run, Some(i)) => i,
                        (OpState::Run, None) => "download",
                    },
                    match o.state {
                        OpState::Done => "done".to_string(),
                        OpState::Fail => "failed".to_string(),
                        OpState::Run => match (o.frac, o.detail.is_empty()) {
                            (Some(f), _) => format!("{}%", (f * 100.0).round() as i32),
                            (None, false) => o.detail.clone(),
                            (None, true) => "…".to_string(),
                        },
                    },
                ),
                None => ("download", String::new()),
            },
        };
        let tint = match (&side, op.map(|o| (o.state, o.tint))) {
            (Side::Op | Side::OpAt(_), Some((OpState::Done, _))) => theme::GREEN,
            (Side::Op | Side::OpAt(_), Some((OpState::Fail, _))) => theme::RED,
            (Side::Op | Side::OpAt(_), Some((OpState::Run, Some(t)))) => theme::rgb(t),
            _ => theme::ACCENT,
        };
        let tw = (p.fonts.measure(&text, 12.0, SEMI) as f64).min(50.0);
        let group = 13.0 + 4.0 + tw;
        let x0 = (w - group) / 2.0;
        p.icon(icon, x0 + 6.5, h / 2.0, 13.0, tint);
        p.text(&text, x0 + 17.0, h / 2.0, 12.0, SEMI, if tint == theme::ACCENT { theme::FG } else { tint }, Align::Left, 50.0);
    }
}

/// A pause between two events this long is shown as free time.
pub(crate) const GAP_MIN_MS: i64 = 45 * 60_000;

/// One running thing on the Today page.
pub(crate) struct RunningRow {
    pub icon: String,
    pub label: String,
    pub right: String,
    pub frac: Option<f64>,
    pub page: Page,
}
