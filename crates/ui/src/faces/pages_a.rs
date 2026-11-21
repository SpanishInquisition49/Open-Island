//! Part of the faces of `Island` (see `mod.rs`).

use super::*;

impl Island {
    pub(super) fn face_cal(&self, p: P, w: f64, h: f64) {
        let c = &self.cal;
        let hy = self.page_header(p, w, &format!("{} {}", clock::MONTHS[c.month as usize - 1], c.year));
        // how fresh the events are
        if !self.data.config.calendars.is_empty() {
            let t = if self.data.events_synced_at == 0 { "Not synced yet".to_string() } else { sync_text(self.now - self.data.events_synced_at) };
            p.text(&t, w - 14.0, h - 8.0, 9.0, REG, theme::DIM, Align::Right, 160.0);
        }
        p.icon_button(w - 14.0 - 14.0 - 90.0, hy, 28.0, "refresh", 15.0, Action::Cmd(Command::RefreshCalendars));
        p.icon_button(w - 14.0 - 14.0 - 60.0, hy, 28.0, "chevron-left", 16.0, Action::CalPrev);
        p.icon_button(w - 14.0 - 14.0 - 30.0, hy, 28.0, "calendar-event", 15.0, Action::CalToday);
        p.icon_button(w - 14.0 - 14.0, hy, 28.0, "chevron-right", 16.0, Action::CalNext);
        let colw = (w - 28.0) / 7.0;
        let top = 14.0 + 34.0 + 4.0;
        p.stagger(1);
        for (i, d) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
            p.text(d, 14.0 + colw * (i as f64 + 0.5), top + 8.0, 10.0, REG, theme::DIM, Align::Center, 20.0);
        }
        let today = clock::civil(self.now, self.data.utc_offset_secs);
        let grid = calendar::month_grid(c.year, c.month);
        let marks = self.month_marks();
        for (r, row) in grid.iter().enumerate() {
            for (i, d) in row.iter().enumerate() {
                let Some(d) = d else { continue };
                p.stagger(2 + r);
                let cx = 14.0 + colw * (i as f64 + 0.5);
                let cy = top + 16.0 + r as f64 * 30.0 + 14.0;
                let cell = rect(cx - colw / 2.0 + 1.0, cy - 14.0, colw - 2.0, 28.0);
                let is_today = (c.year, c.month, *d) == (today.year, today.month, today.day);
                let selected = *d == c.day;
                if selected {
                    p.rrect(cell, 10.0, theme::ACCENT);
                } else if p.hovered(cell) {
                    p.rrect(cell, 10.0, theme::HOT_CHIP);
                }
                let fg = if selected { theme::ON_ACCENT } else { theme::FG };
                let dy = if marks.get(*d as usize).copied().flatten().is_some() { -3.0 } else { 0.0 };
                p.text(&d.to_string(), cx, cy + dy, 12.0, if is_today || selected { SEMI } else { REG }, fg, Align::Center, 24.0);
                if let Some(Some(col)) = marks.get(*d as usize) {
                    p.circle(cx, cy + 9.0, 2.0, if selected { theme::ON_ACCENT } else { *col });
                }
                p.hit(cell, Action::CalDay(*d));
            }
        }
        let mut y = top + 16.0 + grid.len() as f64 * 30.0 + 10.0 + 11.0;
        p.stagger(2 + grid.len());
        let evs = self.day_events();
        if evs.is_empty() {
            p.text(if self.data.events.is_empty() { "Calendar not synced yet" } else { "No events" }, 14.0, y, 12.0, REG, theme::DIM, Align::Left, 300.0);
        }
        let off = self.data.utc_offset_secs;
        for e in evs.iter().take(4) {
            let when = if e.all_day { "All day".to_string() } else { clock::civil(e.start_ms, off).hhmm() };
            let row = rect(8.0, y - 11.0, w - 16.0, 22.0);
            let has_url = e.url.as_deref().is_some_and(|u| !u.is_empty());
            if has_url && p.hovered(row) {
                p.rrect(row, 8.0, theme::CHIP);
            }
            p.rrect(rect(16.0, y - 8.0, 3.0, 16.0), 1.5, e.color.map(|c| theme::rgb(color::readable_tint(c, true))).unwrap_or(theme::ACCENT));
            p.text(&when, 28.0, y, 11.0, REG, theme::DIM, Align::Left, 44.0);
            let cal_w = if e.calendar.is_empty() { 0.0 } else { p.fonts.measure(&e.calendar, 10.0, REG).min(110.0) as f64 + 10.0 };
            p.text(&e.title, 76.0, y, 12.0, SEMI, theme::FG, Align::Left, (w - 76.0 - 40.0 - cal_w) as f32);
            if cal_w > 0.0 {
                p.text(&e.calendar, w - 40.0, y, 10.0, REG, theme::DIM, Align::Right, 110.0);
            }
            if has_url {
                p.icon("external-link", w - 26.0, y, 13.0, theme::DIM);
                p.hit(row, Action::Join(e.url.clone().unwrap_or_default()));
            }
            y += 24.0;
        }
    }

    /// Days (index = day of month) of the shown month that have an event, with the first event's colour.
    pub(super) fn month_marks(&self) -> Vec<Option<Color>> {
        let c = &self.cal;
        let off = self.data.utc_offset_secs;
        let days = calendar::days_in_month(c.year, c.month);
        let mut marks = vec![None; days as usize + 2];
        for d in 1..=days {
            let (s, e) = (clock::midnight_ms(c.year, c.month, d, off), clock::midnight_ms(c.year, c.month, d + 1, off));
            if let Some(ev) = self.data.events.iter().find(|ev| ev.start_ms < e && ev.end_ms > s) {
                marks[d as usize] = Some(ev.color.map(|c| theme::rgb(color::readable_tint(c, true))).unwrap_or(theme::ORANGE));
            }
        }
        marks
    }

    pub(super) fn slider_row(&self, p: P, w: f64, cy: f64, icon: &str, level: Option<(f32, f64)>, action: Action) {
        p.icon(icon, 14.0 + 8.0, cy, 16.0, theme::FG);
        let bar = rect(14.0 + 26.0, cy - 3.0, w - 14.0 - 26.0 - 14.0 - 44.0, 6.0);
        p.slider(bar, level.map_or(0.0, |(_, shown)| shown), action);
        let t = level.map_or("–".to_string(), |(v, _)| format!("{}%", (v * 100.0).round() as i32));
        p.text(&t, w - 14.0, cy, 11.0, REG, theme::DIM, Align::Right, 40.0);
    }

    /// The toggle chips that exist on this machine, in order (a missing tool or an unsupported system hides its chip).
    pub(crate) fn qs_chips(&self) -> Vec<(&'static str, String, bool, Action)> {
        let d = &self.data;
        let mut v: Vec<(&'static str, String, bool, Action)> = vec![];
        if let Some(on) = d.wifi {
            v.push((if on { "wifi" } else { "wifi-off" }, "Wi-Fi".into(), on, Action::Cmd(Command::ToggleWifi)));
        }
        if let Some(on) = d.bluetooth {
            v.push((if on { "bluetooth" } else { "bluetooth-off" }, "Bluetooth".into(), on, Action::Cmd(Command::ToggleBluetooth)));
        }
        v.push(("bell-off", "DND".into(), d.dnd || self.auto_quiet.is_some(), Action::ToggleDnd));
        v.push(("coffee", "Keep awake".into(), d.keep_awake, Action::Cmd(Command::ToggleKeepAwake)));
        if let Some(profile) = d.profile.as_deref().filter(|p| !p.is_empty()) {
            let label = profile[..1].to_uppercase() + &profile[1..];
            let icon = match profile {
                "performance" => "bolt",
                "powersave" => "leaf",
                _ => "gauge",
            };
            v.push((icon, label, profile == "performance", Action::Cmd(Command::NextProfile)));
        }
        if let Some(on) = d.night_light {
            v.push(("moon-stars", "Night light".into(), on, Action::Cmd(Command::ToggleNightLight)));
        }
        v
    }

    /// Where the parts of the quick settings page sit, and how tall it is (without the dock).
    pub(crate) fn qs_layout(&self) -> QsLayout {
        let d = &self.data;
        let rows = self.qs_chips().len().div_ceil(3).max(1) as f64;
        let chips_top = 14.0 + 34.0 + 8.0;
        let mut y = chips_top + rows * 36.0 + (rows - 1.0) * 8.0;
        let focus_y = d.focus.map(|_| {
            let fy = y + 8.0;
            y = fy + 36.0;
            fy
        });
        let vol_cy = y + 14.0 + 14.0;
        let mut bottom = vol_cy + 14.0;
        let bri_cy = d.brightness.map(|_| {
            let c = vol_cy + 36.0;
            bottom = c + 14.0;
            c
        });
        let out_top = (!d.sinks.is_empty()).then(|| {
            let t = bottom + 10.0;
            bottom = t + 32.0;
            t
        });
        let dev_top = (!d.devices.is_empty()).then(|| {
            let t = bottom + 10.0;
            let n = d.devices.len().min(3) as f64;
            bottom = t + n * 32.0 + (n - 1.0) * 6.0;
            t
        });
        QsLayout { chips_top, focus_y, vol_cy, bri_cy, out_top, dev_top, height: bottom + 14.0 }
    }

    pub(super) fn face_qs(&self, p: P, w: f64, _h: f64) {
        self.page_header(p, w, "Quick settings");
        let d = &self.data;
        let l = self.qs_layout();
        let cw = (w - 28.0 - 16.0) / 3.0;
        for (i, (icon, label, on, action)) in self.qs_chips().into_iter().enumerate() {
            let (row, col) = (i / 3, i % 3);
            p.stagger(1 + row);
            p.chip(rect(14.0 + col as f64 * (cw + 8.0), l.chips_top + row as f64 * 44.0, cw, 36.0), icon, &label, on, action);
        }
        if let (Some(f), Some(fy)) = (d.focus, l.focus_y) {
            p.stagger(3);
            let label = if f.active { self.focus_minutes_left().map_or("Focus · on".to_string(), |m| format!("Focus · {m} min left")) } else { "Focus 50 min".to_string() };
            p.chip(rect(14.0, fy, w - 28.0, 36.0), "target", &label, f.active, Action::Cmd(if f.active { Command::StopFocus } else { Command::StartFocus(50) }));
        }
        p.stagger(4);
        self.slider_row(p, w, l.vol_cy, if d.muted { "volume-3" } else { "volume" }, d.volume.map(|v| (v, self.volume_shown)), Action::VolumeBar);
        if let Some(cy) = l.bri_cy {
            p.stagger(5);
            self.slider_row(p, w, cy, "brightness-half", d.brightness.map(|v| (v, self.brightness_shown)), Action::BrightnessBar);
        }
        if let Some(top) = l.out_top {
            p.stagger(6);
            let out = d.sinks.iter().find(|s| Some(&s.name) == d.sink.as_ref());
            let headphones = out.is_some_and(|s| s.name.contains("bluez"));
            let r = rect(14.0, top, w - 28.0, 32.0);
            p.rrect(r, 12.0, if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
            p.icon(if headphones { "headphones" } else { "device-speaker" }, r.x0 + 18.0, r.center().y, 16.0, theme::FG);
            p.text(out.map_or("Output", |s| s.label.as_str()), r.x0 + 34.0, r.center().y, 12.0, SEMI, theme::FG, Align::Left, (r.width() - 120.0) as f32);
            if d.sinks.len() > 1 || !d.streams.is_empty() {
                p.text("mixer", r.x1 - 12.0, r.center().y, 11.0, REG, theme::DIM, Align::Right, 60.0);
            }
            p.hit(r, Action::Open(Page::Audio));
        }
        // connected Bluetooth devices with their battery, in the same quiet rows
        if let Some(top) = l.dev_top {
            for (i, dev) in d.devices.iter().take(3).enumerate() {
                p.stagger(7 + i);
                let r = rect(14.0, top + i as f64 * 38.0, w - 28.0, 32.0);
                let low = dev.battery.is_some_and(|b| b <= 20);
                p.rrect(r, 12.0, theme::CHIP);
                p.icon(dev.kind.icon(), r.x0 + 18.0, r.center().y, 16.0, if low { theme::RED } else { theme::FG });
                let right = match dev.battery {
                    Some(b) => {
                        let t = format!("{b}%");
                        let tw = 34.0; // a fixed column, so the bars line up
                        p.text(&t, r.x1 - 14.0, r.center().y, 12.0, SEMI, if low { theme::RED } else { theme::FG }, Align::Right, 44.0);
                        p.bar(rect(r.x1 - 14.0 - tw - 10.0 - 46.0, r.center().y - 2.0, 46.0, 4.0), b as f64 / 100.0, if low { theme::RED } else { theme::GREEN }, theme::TRACK);
                        tw + 10.0 + 46.0 + 14.0
                    }
                    None => {
                        p.text("connected", r.x1 - 14.0, r.center().y, 11.0, REG, theme::DIM, Align::Right, 70.0);
                        70.0 + 14.0
                    }
                };
                p.text(&dev.name, r.x0 + 34.0, r.center().y, 12.0, SEMI, theme::FG, Align::Left, (r.width() - 34.0 - right - 8.0) as f32);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn metric_row(&self, p: P, w: f64, y: f64, label: &str, frac: Option<f64>, text: &str, color: Color) {
        p.stagger(1 + ((y - 62.0) / 28.0).round().max(0.0) as usize);
        p.text(label, 14.0, y, 11.0, SEMI, theme::DIM, Align::Left, 60.0);
        let tw = p.fonts.measure(text, 11.0, REG) as f64;
        let right = w - 14.0;
        if let Some(f) = frac {
            let bar_x = 14.0 + 62.0;
            let bar_end = (right - tw - 10.0).max(bar_x + 20.0);
            p.bar(rect(bar_x, y - 3.0, bar_end - bar_x, 6.0), f, color, theme::TRACK);
        }
        p.text(text, right, y, 11.0, REG, theme::FG, Align::Right, (w * 0.62) as f32);
    }

    pub(super) fn face_sys(&self, p: P, w: f64, _h: f64) {
        let hy = self.page_header(p, w, "System");
        let Some(st) = &self.data.system else {
            p.text("Collecting…", 14.0, 14.0 + 34.0 + 14.0, 12.0, REG, theme::DIM, Align::Left, 200.0);
            return;
        };
        if let Some(l) = st.load {
            p.text(&format!("load {:.2}  {:.2}  {:.2}", l[0], l[1], l[2]), w - 14.0, hy, 11.0, REG, theme::DIM, Align::Right, 200.0);
        }
        let load_color = |f: f64| if f >= 0.9 { theme::RED } else if f >= 0.7 { theme::ORANGE } else { theme::ACCENT };
        let mut y = 14.0 + 34.0 + 14.0;
        let mut parts = vec![format!("{}%", st.cpu_percent.round() as i32)];
        if let Some(t) = st.cpu_temp_c {
            parts.push(format!("{}°C", t.round() as i32));
        }
        if let Some(g) = st.cpu_ghz {
            parts.push(format!("{g:.1} GHz"));
        }
        let f = st.cpu_percent as f64 / 100.0;
        self.metric_row(p, w, y, "CPU", Some(f), &parts.join(" · "), load_color(f));
        y += 28.0;
        // per-core load as small bars
        p.stagger(2);
        p.text("Cores", 14.0, y, 10.0, REG, theme::DIM, Align::Left, 60.0);
        let n = st.cores.len().max(1) as f64;
        let (x0, x1) = (14.0 + 62.0, w - 14.0);
        let bw = ((x1 - x0) - 2.0 * (n - 1.0)) / n;
        for (i, u) in st.cores.iter().enumerate() {
            let f = (*u as f64 / 100.0).clamp(0.0, 1.0);
            let bh = (18.0 * f).max(3.0);
            let col = if f < 0.15 { theme::TRACK } else { load_color(f) };
            p.rrect(rect(x0 + i as f64 * (bw + 2.0), y + 9.0 - bh, bw, bh), 1.0, col);
        }
        y += 28.0;
        if let Some(m) = &st.memory {
            let mut t = format!("{} / {}", island_core::system::fmt_mb(m.used_mb), island_core::system::fmt_mb(m.total_mb));
            if m.swap_used_mb > 0.5 {
                t += &format!(" · swap {}", island_core::system::fmt_mb(m.swap_used_mb));
            }
            let f = if m.total_mb > 0.0 { m.used_mb / m.total_mb } else { 0.0 };
            self.metric_row(p, w, y, "Memory", Some(f), &t, load_color(f));
            y += 28.0;
        }
        if let Some(g) = &st.gpu {
            let t = format!("{} · {:.1} W · {}°C · {}%", g.pstate, g.watts, g.temp_c.round() as i32, g.util.round() as i32);
            self.metric_row(p, w, y, "GPU", Some(g.util as f64 / 100.0), &t, load_color(g.util as f64 / 100.0));
            y += 28.0;
        }
        if let Some(b) = self.data.battery {
            let mut t = format!("{}%", b.percent.round() as i32);
            if let Some(wt) = st.battery_watts.filter(|w| *w > 0.1) {
                t += &format!(" · {wt:.1} W");
            }
            match st.battery_eta_min {
                Some(m) => t += &format!(" · {} {}", duration::fmt_minutes(Some(m)), if b.charging { "to full" } else { "left" }),
                None if !b.on_ac => {}
                None => t += if b.charging { " · charging" } else { " · plugged in" },
            }
            let col = if b.percent < 15.0 { theme::RED } else if b.percent < 30.0 { theme::ORANGE } else { theme::FG };
            self.metric_row(p, w, y, "Battery", Some(b.percent as f64 / 100.0), &t, col);
            y += 28.0;
        }
        for d in self.data.devices.iter().take(4) {
            let (frac, t) = match d.battery {
                Some(b) => (Some(b as f64 / 100.0), format!("{b}%")),
                None => (None, "connected".to_string()),
            };
            let col = if d.battery.is_some_and(|b| b <= 20) { theme::RED } else { theme::FG };
            self.metric_row(p, w, y, &d.name, frac, &t, col);
            y += 28.0;
        }
        let net = format!("↓ {}   ↑ {}{}", island_core::system::fmt_rate(st.net_rx), island_core::system::fmt_rate(st.net_tx), if self.data.vpn { "   ·  VPN" } else { "" });
        self.metric_row(p, w, y, "Network", None, &net, theme::FG);
        y += 28.0;
        if let Some((used, total)) = st.disk {
            let f = if total > 0.0 { used / total } else { 0.0 };
            self.metric_row(p, w, y, "Disk /", Some(f), &format!("{} / {}", island_core::system::fmt_mb(used), island_core::system::fmt_mb(total)), load_color(f));
            y += 28.0;
        }
        // the program that hogs the machine, with what can be done about it
        if let Some(c) = &self.data.culprit {
            p.stagger(9);
            let what = if c.memory { island_core::system::fmt_mb(c.mem_mb as f64) } else { format!("{:.1} cores", c.cores) };
            p.icon("flame", 22.0, y, 14.0, theme::ORANGE);
            let mut right = w - 14.0;
            if !c.protected {
                let (qw, pw) = (p.button_width("Quit"), p.button_width("Pause"));
                p.button(rect(right - qw, y - 13.0, qw, 26.0), "Quit", Action::Cmd(Command::QuitProcs(c.pids.clone())));
                right -= qw + 6.0;
                p.button(rect(right - pw, y - 13.0, pw, 26.0), "Pause", Action::Cmd(Command::PauseProcs { name: c.name.clone(), pids: c.pids.clone() }));
                right -= pw + 10.0;
            }
            p.text(&format!("{} · {what}", c.name), 38.0, y, 12.0, SEMI, theme::FG, Align::Left, (right - 38.0) as f32);
            y += 28.0;
        }
        for name in self.data.paused.iter().take(3) {
            p.stagger(10);
            p.icon("player-pause", 22.0, y, 14.0, theme::DIM);
            let rw = p.button_width("Resume");
            p.button(rect(w - 14.0 - rw, y - 13.0, rw, 26.0), "Resume", Action::Cmd(Command::ResumeProcs(name.clone())));
            p.text(&format!("{name} · paused"), 38.0, y, 12.0, REG, theme::DIM, Align::Left, (w - 38.0 - rw - 28.0) as f32);
            y += 28.0;
        }
        // who is using the microphone and the camera
        for u in self.data.mic_users.iter().take(2) {
            p.stagger(11);
            p.icon("microphone", 22.0, y, 14.0, theme::ORANGE);
            // a stream can be muted (PipeWire); without one (Windows) the program can only be quit
            let quit = u.stream == 0;
            let label = if quit { "Quit" } else if u.muted { "Unmute" } else { "Mute" };
            let can = !quit || (u.pid.is_some() && !island_core::culprit::is_protected(&u.name));
            let bw = if can { p.button_width(label) } else { 0.0 };
            if can {
                let cmd = if quit { Command::QuitProcs(u.pid.into_iter().collect()) } else { Command::MuteMicStream { stream: u.stream, mute: !u.muted } };
                p.button(rect(w - 14.0 - bw, y - 13.0, bw, 26.0), label, Action::Cmd(cmd));
            }
            p.text(&format!("{} · {}", u.name, if u.muted { "microphone muted" } else { "uses the microphone" }), 38.0, y, 12.0, REG, theme::FG, Align::Left, (w - 38.0 - bw - 28.0) as f32);
            y += 28.0;
        }
        for (name, pids) in self.data.camera_users.iter().take(2) {
            p.stagger(12);
            p.icon("video", 22.0, y, 14.0, theme::GREEN);
            let protected = island_core::culprit::is_protected(name);
            let bw = if protected { 0.0 } else { p.button_width("Quit") };
            if !protected {
                p.button(rect(w - 14.0 - bw, y - 13.0, bw, 26.0), "Quit", Action::Cmd(Command::QuitProcs(pids.clone())));
            }
            p.text(&format!("{name} · uses the camera"), 38.0, y, 12.0, REG, theme::FG, Align::Left, (w - 38.0 - bw - 28.0) as f32);
            y += 28.0;
        }
        let _ = y;
    }

    pub(super) fn face_focus_mini(&self, p: P, w: f64, h: f64) {
        let cy = h / 2.0;
        p.icon("target", 24.0, cy, 15.0, theme::ACCENT);
        p.text("Focus", 40.0, cy, 12.0, SEMI, theme::FG, Align::Left, 60.0);
        let mut right = self.pill_clock(p, w - 14.0, cy);
        right = self.status_icons(p, right, cy, 13.0);
        let left = self.focus_minutes_left().map_or("on".to_string(), |m| format!("{m} min"));
        p.text(&left, right, cy, 12.0, REG, theme::DIM, Align::Right, 70.0);
        p.hit(rect(0.0, 0.0, w, h), Action::Open(Page::Focus));
    }

    pub(super) fn face_focus_full(&self, p: P, w: f64, _h: f64) {
        let label = self.focus_minutes_left().map_or("Focus on".to_string(), |m| format!("{m} min left"));
        let tw = p.fonts.measure(&label, 22.0, SEMI) as f64;
        let x = (w - (22.0 + 10.0 + tw)) / 2.0;
        p.icon("target", x + 11.0, 30.0, 22.0, theme::ACCENT);
        p.text(&label, x + 32.0, 30.0, 22.0, SEMI, theme::FG, Align::Left, (w - 60.0) as f32);
        p.text("Sites blocked · notifications silenced", w / 2.0, 56.0, 11.0, REG, theme::DIM, Align::Center, (w - 28.0) as f32);
        self.button_row(p, &[("End focus", Action::Cmd(Command::StopFocus))], w / 2.0, 88.0);
    }

    /// A text field in place of a row while a setting is edited.
    pub(super) fn edit_row(&self, p: P, r: Rect, e: &Edit, placeholder: &str) {
        let masked;
        let e = if e.secret() {
            masked = Edit { text: "•".repeat(e.text.chars().count()), ..e.clone() };
            &masked
        } else {
            e
        };
        p.rrect(r, 10.0, theme::CHIP);
        p.rrect_stroke(r, 10.0, 1.5, theme::FG);
        let inner = (r.width() - 24.0) as f32;
        if e.text.is_empty() {
            p.text(placeholder, r.x0 + 12.0, r.center().y, 12.0, REG, theme::DIM, Align::Left, inner);
        } else {
            // keep the caret in view: show the tail of a long text
            let before: String = e.text.chars().take(e.cursor).collect();
            let skip = (0..e.text.chars().count()).find(|n| p.fonts.measure(&before.chars().skip(*n).collect::<String>(), 12.0, REG) <= inner - 6.0).unwrap_or(0);
            let shown: String = e.text.chars().skip(skip).collect();
            p.text(&shown, r.x0 + 12.0, r.center().y, 12.0, REG, theme::FG, Align::Left, inner);
            if (self.now / 530) % 2 == 0 {
                let cx = r.x0 + 12.0 + p.fonts.measure(&before.chars().skip(skip).collect::<String>(), 12.0, REG).min(inner) as f64;
                p.rrect(rect(cx, r.center().y - 7.0, 1.5, 14.0), 0.75, theme::FG);
            }
            return;
        }
        if (self.now / 530) % 2 == 0 {
            p.rrect(rect(r.x0 + 11.0, r.center().y - 7.0, 1.5, 14.0), 0.75, theme::FG);
        }
    }

    pub(super) fn face_settings(&self, p: P, w: f64, _h: f64) {
        self.page_header(p, w, "Settings");
        let c = &self.data.config;
        let editing = |f: Field| self.edit.as_ref().filter(|e| e.field == f);
        let mut y = 14.0 + 34.0;
        let mut n = 1;
        let section = |p: P, y: f64, title: &str, n: usize| {
            p.stagger(n);
            p.text(&title.to_uppercase(), 18.0, y + 11.0, 10.0, SEMI, theme::DIM, Align::Left, 200.0);
        };
        // calendars
        section(p, y, "Calendars", n);
        y += 22.0;
        for (i, cal) in c.calendars.iter().take(4).enumerate() {
            n += 1;
            p.stagger(n);
            let r = rect(14.0, y, w - 28.0, 30.0);
            if self.picking == Some(i) {
                // the colour choice: the eight system colours, the current one ringed
                p.rrect(r, 10.0, theme::CHIP);
                let step = (r.width() - 24.0) / 8.0;
                for (k, col) in PALETTE.iter().enumerate() {
                    let cx = r.x0 + 12.0 + step * (k as f64 + 0.5);
                    let swatch = rect(cx - 12.0, y + 3.0, 24.0, 24.0);
                    if *col == cal.color {
                        p.circle(cx, y + 15.0, 12.0, with_alpha(theme::FG, 0.9));
                        p.circle(cx, y + 15.0, 10.0, theme::BG);
                    }
                    p.circle(cx, y + 15.0, if p.hovered(swatch) { 9.0 } else { 8.0 }, theme::rgb(*col));
                    p.hit(swatch, Action::SetColor(i, *col));
                }
                y += 34.0;
                continue;
            }
            if let Some(e) = editing(Field::CalendarName(i)) {
                self.edit_row(p, r, e, &cal.detail);
                y += 34.0;
                continue;
            }
            p.rrect(r, 10.0, if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
            let dot = rect(r.x0 + 6.0, y + 3.0, 24.0, 24.0);
            p.circle(r.x0 + 18.0, y + 15.0, if p.hovered(dot) { 7.5 } else { 6.5 }, theme::rgb(cal.color));
            let name = if cal.name.is_empty() { cal.detail.as_str() } else { cal.name.as_str() };
            let dw = p.fonts.measure(&cal.detail, 10.0, REG) as f64;
            let name_max = (r.width() - 34.0 - 40.0 - if cal.name.is_empty() { 0.0 } else { dw.min(150.0) + 12.0 }) as f32;
            p.text(name, r.x0 + 34.0, y + 15.0, 12.0, if cal.name.is_empty() { REG } else { SEMI }, theme::FG, Align::Left, name_max);
            if !cal.name.is_empty() {
                p.text(&cal.detail, r.x1 - 38.0, y + 15.0, 10.0, REG, theme::DIM, Align::Right, 150.0);
            }
            p.icon_button(r.x1 - 18.0, y + 15.0, 26.0, "x", 13.0, Action::RemoveCalendar(i));
            p.hit(rect(r.x0 + 30.0, y, r.width() - 30.0 - 38.0, 30.0), Action::EditField(Field::CalendarName(i)));
            p.hit(dot, Action::PickColor(i));
            y += 34.0;
        }
        p.stagger(n + 1);
        let r = rect(14.0, y, w - 28.0, 30.0);
        match editing(Field::Calendar) {
            Some(e) => self.edit_row(p, r, e, "Paste an .ics or webcal:// address"),
            None => {
                p.rrect(r, 10.0, if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
                p.icon("plus", r.x0 + 18.0, y + 15.0, 14.0, theme::DIM);
                p.text("Add a calendar (feed, .ics file, or holidays)", r.x0 + 34.0, y + 15.0, 12.0, REG, theme::DIM, Align::Left, 300.0);
                p.hit(r, Action::EditField(Field::Calendar));
            }
        }
        y += 34.0;
        let r = rect(14.0, y, w - 28.0, 30.0);
        match editing(Field::ICloudUser).or_else(|| editing(Field::ICloudPassword)) {
            Some(e) => self.edit_row(p, r, e, if e.field == Field::ICloudUser { "Apple ID (email)" } else { "App-specific password" }),
            None => {
                p.rrect(r, 10.0, if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
                p.icon("brand-apple", r.x0 + 18.0, y + 15.0, 14.0, theme::DIM);
                p.text("Add an iCloud account", r.x0 + 34.0, y + 15.0, 12.0, REG, theme::DIM, Align::Left, 300.0);
                p.hit(r, Action::EditField(Field::ICloudUser));
            }
        }
        y += 34.0;
        let r = rect(14.0, y, w - 28.0, 30.0);
        match editing(Field::CalDavServer).or_else(|| editing(Field::CalDavUser)).or_else(|| editing(Field::CalDavPassword)) {
            Some(e) => self.edit_row(p, r, e, match e.field { Field::CalDavServer => "Server address (https://...)", Field::CalDavUser => "User name", _ => "Password" }),
            None => {
                p.rrect(r, 10.0, if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
                p.icon("calendar-event", r.x0 + 18.0, y + 15.0, 14.0, theme::DIM);
                p.text("Add a CalDAV account (Nextcloud, Fastmail...)", r.x0 + 34.0, y + 15.0, 12.0, REG, theme::DIM, Align::Left, 360.0);
                p.hit(r, Action::EditField(Field::CalDavServer));
            }
        }
        y += 34.0;
        // a labelled value row
        let kv = |p: P, y: f64, label: &str, value: &str, f: Field, placeholder: &str, n: usize| {
            p.stagger(n);
            let r = rect(14.0, y, w - 28.0, 30.0);
            match editing(f) {
                Some(e) => self.edit_row(p, r, e, placeholder),
                None => {
                    p.rrect(r, 10.0, if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
                    p.text(label, r.x0 + 12.0, y + 15.0, 12.0, SEMI, theme::FG, Align::Left, 120.0);
                    let lw = p.fonts.measure(label, 12.0, SEMI) as f64;
                    p.text(value, r.x1 - 12.0, y + 15.0, 12.0, REG, theme::DIM, Align::Right, (r.width() - lw as f32 as f64 - 40.0).max(60.0) as f32);
                    p.hit(r, Action::EditField(f));
                }
            }
        };
        section(p, y, "Weather location", n + 2);
        y += 22.0;
        let num = |v: Option<f64>| v.map_or("not set".to_string(), |v| format!("{v:.4}"));
        kv(p, y, "Latitude", &num(c.latitude), Field::Latitude, "e.g. 43.7228", n + 3);
        y += 34.0;
        kv(p, y, "Longitude", &num(c.longitude), Field::Longitude, "e.g. 10.4017", n + 4);
        y += 34.0;
        section(p, y, "Home and train", n + 11);
        y += 22.0;
        kv(p, y, "Home", if c.home.is_empty() { "not set" } else { &c.home }, Field::Home, "43.7085, 10.4036 or an address; empty = remove", n + 12);
        y += 34.0;
        let train = self.data.train.as_ref().map_or("not pinned".to_string(), |t| format!("{} · {} {} → {}", t.label(), t.time, t.from_name, t.to_name));
        kv(p, y, "Train", &train, Field::Train, "pisa centrale 17:12 verso lucca; empty = unpin", n + 13);
        y += 34.0;
        section(p, y, "Appearance", n + 5);
        y += 22.0;
        let font = if c.font.is_empty() { "Default".to_string() } else if c.font_missing { format!("{} (not found)", c.font) } else { c.font.clone() };
        kv(p, y, "Font", &font, Field::Font, "A family name or a font file path; empty = default", n + 6);
        y += 34.0;
        section(p, y, "Notifications", n + 7);
        y += 22.0;
        let sound = if c.notification_sound.is_empty() { "None".to_string() } else { c.notification_sound.rsplit('/').next().unwrap_or("").to_string() };
        kv(p, y, "Sound", &sound, Field::Sound, "Path of a sound file; empty = silent", n + 8);
        y += 34.0;
        section(p, y, "Map", n + 9);
        y += 22.0;
        kv(p, y, "API key", if c.map_key_set { "set" } else { "not set" }, Field::MapKey, "The key of the tile provider, for {key} in the tile URL; empty = remove it", n + 10);
        y += 34.0;
        if let Some((t, _)) = &self.note {
            p.text(t, w / 2.0, y + 8.0, 11.0, SEMI, theme::ORANGE, Align::Center, (w - 28.0) as f32);
        }
    }

    pub(super) fn face_options(&self, p: P, w: f64, _h: f64) {
        use crate::options::{self, TABS};
        self.page_header(p, w, "Options");
        let tab = self.options_tab.min(TABS.len() - 1);
        // the tabs
        p.stagger(1);
        let tab_y = 14.0 + 34.0 + 4.0;
        let tw = (w - 28.0 - 8.0 * (TABS.len() as f64 - 1.0)) / TABS.len() as f64;
        for (t, name) in TABS.iter().enumerate() {
            let r = rect(14.0 + t as f64 * (tw + 8.0), tab_y, tw, 28.0);
            let on = t == tab;
            p.rrect(r, 14.0, if on { theme::ACCENT } else if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
            p.text(name, r.center().x, r.center().y, 12.0, SEMI, if on { theme::ON_ACCENT } else { theme::FG }, Align::Center, (tw - 12.0) as f32);
            p.hit(r, Action::OptionsTab(t));
        }
        let col_w = (w - 28.0 - 8.0) / 2.0;
        let mut y = tab_y + 28.0 + 12.0;
        let cell = |i: usize, y: f64| rect(14.0 + (i % 2) as f64 * (col_w + 8.0), y + (i / 2) as f64 * 34.0, col_w, 30.0);
        let mut n = 2;
        // what the option under the pointer controls, shown under the options
        let mut help: Option<&str> = None;
        // switches
        if tab == options::RULES_TAB {
            self.options_rules(p, w, y, n);
            return;
        }
        let toggles = options::toggles(tab);
        for (i, (key, label)) in toggles.iter().enumerate() {
            p.stagger(n + i / 2);
            let r = cell(i, y);
            let on = options::get_bool(&self.cfg, key);
            p.rrect(r, 10.0, if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
            if p.hovered(r) {
                help = Some(options::describe(key));
            }
            p.text(label, r.x0 + 12.0, r.center().y, 12.0, SEMI, theme::FG, Align::Left, (r.width() - 12.0 - 50.0) as f32);
            let track = rect(r.x1 - 12.0 - 32.0, r.center().y - 9.0, 32.0, 18.0);
            p.rrect(track, 9.0, if on { theme::GREEN } else { theme::TRACK });
            p.circle(if on { track.x1 - 9.0 } else { track.x0 + 9.0 }, track.center().y, 7.0, theme::FG);
            p.hit(r, Action::Toggle(key));
        }
        y += toggles.len().div_ceil(2) as f64 * 34.0;
        n += toggles.len().div_ceil(2);
        // numbers
        let steppers = options::steppers(tab);
        for (i, (key, label, unit, ..)) in steppers.iter().enumerate() {
            p.stagger(n + i / 2);
            let r = cell(i, y);
            p.rrect(r, 10.0, theme::CHIP);
            if p.hovered(r) {
                help = Some(options::describe(key));
            }
            // from the right: plus, the value in a fixed column, minus
            p.text(label, r.x0 + 12.0, r.center().y, 12.0, SEMI, theme::FG, Align::Left, (r.width() - 12.0 - 118.0) as f32);
            let value = options::fmt_num(options::get_num(&self.cfg, key), unit);
            p.text(&value, r.x1 - 37.0, r.center().y, 11.0, REG, theme::DIM, Align::Right, 52.0);
            p.icon_button(r.x1 - 104.0, r.center().y, 22.0, "minus", 12.0, Action::Step(key, -1));
            p.icon_button(r.x1 - 22.0, r.center().y, 22.0, "plus", 12.0, Action::Step(key, 1));
        }
        y += steppers.len().div_ceil(2) as f64 * 34.0;
        n += steppers.len().div_ceil(2);
        // text fields
        if options::text_rows(tab) > 0 {
            let c = &self.data.config;
            let editing = |f: Field| self.edit.as_ref().filter(|e| e.field == f);
            let screen = if c.screen == "focused" { "Focused".to_string() } else { c.screen.clone() };
            for (i, key) in options::text_keys(tab).iter().enumerate() {
                let (label, value, f, hint) = match *key {
                    "timer_presets" => ("Timer presets", c.timer_presets.clone(), Field::TimerPresets, "5m, 10m, 25m, 1h30m"),
                    "screen" => ("Screen", screen.clone(), Field::Screen, "A screen name such as eDP-1; empty = the focused one"),
                    _ => ("Map tiles", if c.map_tile_url.is_empty() { "OpenStreetMap".to_string() } else { c.map_tile_url.clone() }, Field::MapTiles, "A preset (esri-dark, osm, carto-dark...) or a URL; empty = OpenStreetMap"),
                };
                p.stagger(n + i);
                let r = rect(14.0, y + i as f64 * 34.0, w - 28.0, 30.0);
                if p.hovered(r) {
                    help = Some(options::describe(key));
                }
                match editing(f) {
                    Some(e) => self.edit_row(p, r, e, hint),
                    None => {
                        p.rrect(r, 10.0, if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
                        p.text(label, r.x0 + 12.0, r.center().y, 12.0, SEMI, theme::FG, Align::Left, 120.0);
                        let lw = p.fonts.measure(label, 12.0, SEMI) as f64;
                        p.text(&value, r.x1 - 12.0, r.center().y, 12.0, REG, theme::DIM, Align::Right, (r.width() - lw - 40.0).max(60.0) as f32);
                        p.hit(r, Action::EditField(f));
                    }
                }
            }
        }
        // under the last row: the line that explains the option the pointer is on
        let rows = options::toggles(tab).len().div_ceil(2) + options::steppers(tab).len().div_ceil(2) + options::text_rows(tab);
        let hy = tab_y + 28.0 + 12.0 + rows as f64 * 34.0 + options::HELP_H / 2.0 - 2.0;
        match help {
            Some(t) => {
                p.text(t, w / 2.0, hy, 11.0, REG, theme::FG, Align::Center, (w - 28.0) as f32);
            }
            None => {
                // what the tab is for, until the pointer rests on an option
                let blurb = options::tab_blurb(tab);
                p.text(if blurb.is_empty() { "Rest on an option to see what it does" } else { blurb }, w / 2.0, hy, 11.0, REG, theme::DIM, Align::Center, (w - 28.0) as f32);
            }
        }
    }

    /// The notification rules: one row each, and two rows to add one.
    pub(super) fn options_rules(&self, p: P, w: f64, mut y: f64, n: usize) {
        let rules = &self.data.config.filters;
        let editing = |f: Field| self.edit.as_ref().filter(|e| e.field == f);
        if rules.is_empty() {
            p.stagger(n);
            p.text("No rules: every notification shows", 14.0 + 4.0, y + 15.0, 12.0, REG, theme::DIM, Align::Left, (w - 40.0) as f32);
            y += 34.0;
        }
        for (i, r) in rules.iter().take(4).enumerate() {
            p.stagger(n + i);
            let row = rect(14.0, y, w - 28.0, 30.0);
            p.rrect(row, 10.0, theme::CHIP);
            p.icon(if r.show { "volume-3" } else { "bell-off" }, row.x0 + 18.0, y + 15.0, 14.0, theme::DIM);
            let what = match (r.app.is_empty(), r.content.is_empty()) {
                (false, false) => format!("{} · {}", r.app, r.content),
                (false, true) => r.app.clone(),
                (true, false) => format!("\"{}\"", r.content),
                (true, true) => "everything".to_string(),
            };
            let state = if r.show { "silent" } else { "hidden" };
            let sw = p.fonts.measure(state, 11.0, REG) as f64;
            p.text(&what, row.x0 + 34.0, y + 15.0, 12.0, SEMI, theme::FG, Align::Left, (row.width() - 34.0 - sw as f32 as f64 - 52.0).max(40.0) as f32);
            p.text(state, row.x1 - 38.0, y + 15.0, 11.0, REG, theme::DIM, Align::Right, 60.0);
            p.icon_button(row.x1 - 18.0, y + 15.0, 26.0, "x", 13.0, Action::RemoveRule(i));
            y += 34.0;
        }
        // (field, icon, label, hint) of the two rows that add a rule
        type AddRow<'a> = (Field, &'a str, &'a str, &'a str);
        n_rule_row(self, p, w, (y, n + 5), (Field::RuleHide, "bell-off", "Hide an app's notifications", "App name, e.g. Zen Browser"));
        n_rule_row(self, p, w, (y + 34.0, n + 6), (Field::RuleSilence, "volume-3", "Silence an app's sound", "App name, e.g. Telegram"));
        fn n_rule_row(s: &Island, p: P, w: f64, (y, n): (f64, usize), (f, icon, label, hint): AddRow) {
            p.stagger(n);
            let r = rect(14.0, y, w - 28.0, 30.0);
            match s.edit.as_ref().filter(|e| e.field == f) {
                Some(e) => s.edit_row(p, r, e, hint),
                None => {
                    p.rrect(r, 10.0, if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
                    p.icon(icon, r.x0 + 18.0, y + 15.0, 14.0, theme::DIM);
                    p.text(label, r.x0 + 34.0, y + 15.0, 12.0, REG, theme::DIM, Align::Left, 300.0);
                    p.hit(r, Action::EditField(f));
                }
            }
        }
        let _ = editing;
    }

    pub(super) fn face_clip(&self, p: P, w: f64, _h: f64) {
        let hy = self.page_header(p, w, "Clipboard");
        p.text("click to copy", w - 14.0, hy, 11.0, REG, theme::DIM, Align::Right, 120.0);
        let mut y = 14.0 + 34.0 + 4.0;
        if self.data.clipboard.is_empty() {
            p.text("Clipboard history is empty", 14.0, y + 14.0, 12.0, REG, theme::DIM, Align::Left, 300.0);
        }
        for (i, t) in self.data.clipboard.iter().take(crate::island::CLIP_ROWS).enumerate() {
            p.stagger(1 + i);
            let row = rect(14.0, y, w - 28.0, 28.0);
            p.rrect(row, 10.0, if p.hovered(row) { theme::HOT_CHIP } else { theme::CHIP });
            let copied = self.copied.is_some_and(|(j, until)| j == i && self.now < until);
            let pinned = self.data.clipboard_pinned.contains(t);
            let act = island_core::clipact::classify(t);
            match island_core::clipact::color(t) {
                Some(c) if !copied => p.circle(row.x0 + 18.0, y + 14.0, 7.0, theme::rgb(c)),
                _ => p.icon(if copied { "circle-check" } else { "clipboard-text" }, row.x0 + 18.0, y + 14.0, 14.0, if copied { theme::ACCENT } else { theme::DIM }),
            }
            let one_line: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
            p.text(&one_line, row.x0 + 34.0, y + 14.0, 12.0, REG, theme::FG, Align::Left, (row.width() - 34.0 - if act.is_some() { 96.0 } else { 66.0 }) as f32);
            p.hit(row, Action::ClipRestore(i));
            if let Some(a) = &act {
                let b = rect(row.x1 - 92.0, y, 30.0, 28.0);
                let icon = match a.kind {
                    island_core::clipact::Kind::Link => "external-link",
                    island_core::clipact::Kind::Email => "mail",
                    island_core::clipact::Kind::Place => "map-pin",
                };
                p.icon(icon, b.x0 + 15.0, y + 14.0, 13.0, if p.hovered(b) { theme::ACCENT } else { theme::FG });
                p.hit(b, Action::ClipAct(i));
            }
            // pin and forget sit on top of the row, so they are registered after it
            let (pin, del) = (rect(row.x1 - 62.0, y, 30.0, 28.0), rect(row.x1 - 32.0, y, 30.0, 28.0));
            p.icon(if pinned { "star-filled" } else { "star" }, pin.x0 + 15.0, y + 14.0, 13.0, if pinned { theme::ACCENT } else if p.hovered(pin) { theme::FG } else { theme::DIM });
            p.hit(pin, Action::ClipPin(i));
            p.icon("x", del.x0 + 15.0, y + 14.0, 12.0, if p.hovered(del) { theme::FG } else { theme::DIM });
            p.hit(del, Action::ClipForget(i));
            y += 32.0;
        }
    }

}
