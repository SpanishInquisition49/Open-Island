//! Drawing of the faces (the macOS look). Everything is laid out by hand in the pill's own coordinates,
//! origin top-left, logical pixels.

use crate::clock;
use crate::island::*;
use crate::model::*;
use crate::paint::{Align, Hit, Painter};
use crate::text::{Fonts, Weight};
use crate::theme::{self, with_alpha};
use island_core::calendar;
use island_core::color;
use island_core::duration;
use island_core::face::{Face, Page, Side};
use island_core::progress::State as OpState;
use island_core::weather;
use vello::kurbo::{Affine, Rect, RoundedRect};
use vello::peniko::{Color, Fill};
use vello::Scene;

type P<'a, 'b> = &'a mut Painter<'b, Action>;
const SEMI: Weight = Weight::Semibold;
const REG: Weight = Weight::Regular;

fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect::new(x, y, x + w, y + h)
}

impl Island {
    /// Draws the island into `scene`, in surface coordinates scaled by `scale`.
    pub fn draw(&mut self, scene: &mut Scene, fonts: &Fonts, surface: (f64, f64), scale: f64) {
        let (w, h) = (self.spring.w.pos.max(1.0), self.spring.h.pos.max(1.0));
        let slot = if self.shown_side.is_some() { (BUBBLE_W + BUBBLE_GAP) * self.bubble_k } else { 0.0 };
        let origin = (((surface.0 - (w + slot)) / 2.0).round(), TOP_MARGIN);
        self.pill_origin = origin;
        let base = Affine::scale(scale) * Affine::translate(origin);
        let radius = (h / 2.0).min(30.0);
        let mut hits: Vec<Hit<Action>> = std::mem::take(&mut self.hits);
        hits.clear();
        let pointer = self.pointer_local();

        // the pill
        scene.fill(Fill::NonZero, base, theme::BG, None, &RoundedRect::new(0.0, 0.0, w, h, radius));

        // its content, clipped to the pill and crossfaded: the old face leaves, the new one scales in
        let op = self.content_opacity();
        let k = 0.94 + 0.06 * op as f64;
        let content_t = base
            * Affine::translate((w / 2.0, h / 2.0 - 3.0 * (1.0 - op as f64)))
            * Affine::scale(k)
            * Affine::translate((-w / 2.0, -h / 2.0));
        scene.push_clip_layer(Fill::NonZero, base, &RoundedRect::new(0.0, 0.0, w, h, radius));
        {
            let mut p = Painter { scene, fonts, hits: &mut hits, opacity: op, pointer, transform: content_t };
            self.draw_face(&mut p, self.shown, w, h);
        }
        scene.pop_layer();

        // the side bubble
        if let (Some(side), true) = (self.shown_side, self.bubble_k > 0.01) {
            let kk = self.bubble_k;
            let bx = w + BUBBLE_GAP;
            let bt = base
                * Affine::translate((bx + BUBBLE_W / 2.0, h / 2.0))
                * Affine::scale(0.6 + 0.4 * kk)
                * Affine::translate((-BUBBLE_W / 2.0, -h / 2.0));
            scene.fill(Fill::NonZero, bt, with_alpha(theme::BG, kk as f32), None, &RoundedRect::new(0.0, 0.0, BUBBLE_W, h, (h / 2.0).min(30.0)));
            let mut p = Painter { scene, fonts, hits: &mut hits, opacity: kk as f32, pointer: None, transform: bt };
            self.draw_bubble(&mut p, side, BUBBLE_W, h);
        }
        self.hits = hits;
    }

    fn pointer_local(&self) -> Option<(f64, f64)> {
        self.pointer_pos().map(|(x, y)| (x - self.pill_origin.0, y - self.pill_origin.1))
    }

    fn draw_face(&self, p: P, face: Face, w: f64, h: f64) {
        match face {
            Face::Idle => self.face_idle(p, w, h),
            Face::IdleHover => self.face_home(p, w, h),
            Face::MediaMini => self.face_media_mini(p, w, h),
            Face::MediaFull | Face::MediaLyric => self.face_media_full(p, w, h, face == Face::MediaLyric),
            Face::TimerMini => self.face_timer_mini(p, w, h),
            Face::TimerFull => self.face_timer_full(p, w, h),
            Face::TimerDone => self.face_simple(p, w, h, "bell-ringing", theme::ACCENT, &self.timer_done_text.0, &self.timer_done_text.1),
            Face::Notif | Face::NotifAct => self.face_notif(p, w, h, face == Face::NotifAct),
            Face::Reply => self.face_reply(p, w, h),
            Face::Osd => self.face_osd(p, w, h),
            Face::Power => self.face_power(p, w, h),
            Face::Progress => self.face_progress(p, w, h),
            Face::Rain => {
                let t = format!("Rain in about {} min", self.rain_in.unwrap_or(0));
                self.face_simple(p, w, h, "umbrella", theme::ACCENT, &t, "Click to dismiss")
            }
            Face::MeetMini => self.face_meet_mini(p, w, h),
            Face::Meeting => self.face_meeting(p, w, h),
            Face::Cal => self.face_cal(p, w, h),
            Face::Qs => self.face_qs(p, w, h),
            Face::Sys => self.face_sys(p, w, h),
            Face::Notifs => self.face_notifs(p, w, h),
            Face::Clip => self.face_clip(p, w, h),
            // not part of this build (no clipboard history, focus mode or update check yet)
            Face::FocusMini | Face::FocusFull | Face::Update => {}
        }
    }

    fn clock_text(&self) -> String {
        clock::civil(self.now, self.data.utc_offset_secs).hhmm()
    }

    /// Privacy dots (microphone, camera, screen capture) drawn right to left from `x_right`.
    fn status_icons(&self, p: P, x_right: f64, cy: f64, size: f32) -> f64 {
        let pr = self.data.privacy;
        if !self.cfg.status_icons {
            return x_right;
        }
        let mut x = x_right;
        for (on, icon, color) in [(pr.screen, "screen-share", theme::ORANGE), (pr.camera, "video", theme::GREEN), (pr.mic, "microphone", theme::ORANGE)] {
            if on {
                p.icon(icon, x - size as f64 / 2.0, cy, size, color);
                x -= ICON_W;
            }
        }
        x
    }

    fn face_idle(&self, p: P, w: f64, h: f64) {
        let n = self.icon_count(false) as f64;
        let t = self.clock_text();
        let tw = p.fonts.measure(&t, 14.0, SEMI) as f64;
        let group = tw + n * ICON_W;
        let x0 = (w - group) / 2.0;
        p.text(&t, x0, h / 2.0, 14.0, SEMI, theme::FG, Align::Left, 100.0);
        self.status_icons(p, x0 + group, h / 2.0, 13.0);
        p.hit(rect(0.0, 0.0, w, h), Action::Open(Page::Home));
    }

    fn equalizer(&self, p: P, x: f64, cy: f64, n: usize, max_h: f64, bar_w: f64, playing: bool, tint: Color) {
        let t = self.now as f64 / 1000.0;
        for i in 0..n {
            let hh = if playing {
                let ph = i as f64 * 1.7;
                3.0 + (max_h - 3.0) * (0.5 + 0.5 * (t * (5.0 + i as f64 * 1.3) + ph).sin()) * (0.55 + 0.45 * (t * 2.1 + ph).cos().abs())
            } else {
                3.0
            };
            let bx = x + i as f64 * (bar_w + 2.0);
            p.rrect(rect(bx, cy - hh / 2.0, bar_w, hh), bar_w / 2.0, tint);
        }
    }

    fn art_tint(&self) -> Color {
        self.data
            .media
            .as_ref()
            .and_then(|m| m.art.as_ref())
            .and_then(|a| a.average())
            .map(|c| theme::rgb(color::readable_tint(c, true)))
            .unwrap_or(theme::FG)
    }

    fn draw_art(&self, p: P, art: Option<&Image>, x: f64, y: f64, size: f64, radius: f64) {
        match art {
            Some(img) => draw_image(p, img, x, y, size, radius),
            None => {
                p.rrect(rect(x, y, size, size), radius, theme::CHIP);
                p.icon("music", x + size / 2.0, y + size / 2.0, (size * 0.5) as f32, theme::DIM);
            }
        }
    }

    fn pill_clock(&self, p: P, x_right: f64, cy: f64) -> f64 {
        if !self.cfg.clock_in_pill {
            return x_right;
        }
        let t = self.clock_text();
        let w = p.text(&t, x_right, cy, 12.0, SEMI, theme::FG, Align::Right, 80.0) as f64;
        x_right - w - 8.0
    }

    fn face_media_mini(&self, p: P, w: f64, h: f64) {
        let Some(m) = self.data.media.as_ref() else { return };
        let cy = h / 2.0;
        self.draw_art(p, m.art.as_ref(), 8.0, cy - 10.0, 20.0, 6.0);
        let mut right = self.pill_clock(p, w - 14.0, cy);
        right = self.status_icons(p, right, cy, 13.0);
        self.equalizer(p, right - 22.0, cy, 4, 12.0, 2.5, m.playing, self.art_tint());
        let max = (right - 26.0 - 38.0).max(20.0) as f32;
        p.text(&m.title, 38.0, cy, 12.0, SEMI, theme::FG, Align::Left, max);
        p.hit(rect(0.0, 0.0, w, h), Action::Open(Page::Media));
    }

    fn face_media_full(&self, p: P, w: f64, _h: f64, lyric: bool) {
        let Some(m) = self.data.media.as_ref() else { return };
        self.draw_art(p, m.art.as_ref(), 14.0, 14.0, 64.0, 12.0);
        p.text(&m.title, 92.0, 30.0, 14.0, SEMI, theme::FG, Align::Left, (w - 92.0 - 56.0) as f32);
        p.text(&m.artist, 92.0, 50.0, 12.0, REG, theme::DIM, Align::Left, (w - 92.0 - 56.0) as f32);
        self.equalizer(p, w - 40.0, 30.0, 4, 14.0, 3.0, m.playing, self.art_tint());
        // progress (a slider you can drag) + times
        let pos = self.media_position();
        let frac = if m.length_ms > 0.0 { pos / m.length_ms } else { 0.0 };
        let bar = rect(92.0, 62.0, w - 92.0 - 18.0, 5.0);
        if m.can_seek {
            p.slider(bar, frac, Action::SeekBar);
        } else {
            p.bar(bar, frac, theme::FG, theme::TRACK);
        }
        p.text(&duration::fmt_elapsed(pos as i64), bar.x0, 80.0, 10.0, REG, theme::DIM, Align::Left, 60.0);
        let rem = format!("-{}", duration::fmt_elapsed((m.length_ms - pos).max(0.0) as i64));
        p.text(&rem, bar.x1, 80.0, 10.0, REG, theme::DIM, Align::Right, 60.0);
        // controls: previous, a white play/pause disc, next
        let cy = 114.0;
        let cx = w / 2.0;
        p.icon_button(cx - 64.0, cy, 36.0, "player-skip-back", 20.0, Action::Cmd(Command::MediaPrevious));
        let disc = rect(cx - 22.0, cy - 22.0, 44.0, 44.0);
        p.circle(cx, cy, if p.hovered(disc) { 23.0 } else { 22.0 }, theme::FG);
        p.icon(if m.playing { "player-pause" } else { "player-play" }, cx, cy, 22.0, theme::ON_ACCENT);
        p.hit(disc, Action::Cmd(Command::MediaPlayPause));
        p.icon_button(cx + 64.0, cy, 36.0, "player-skip-forward", 20.0, Action::Cmd(Command::MediaNext));
        if m.players > 1 {
            p.icon_button(cx - 128.0, cy, 34.0, "arrows-exchange", 17.0, Action::CyclePlayer);
        }
        if self.cfg.clipboard_history {
            p.icon_button(cx + 128.0, cy, 34.0, "clipboard-text", 17.0, Action::Open(Page::Clip));
        }
        if lyric {
            if let Some((prev, cur, next)) = self.lyric_window() {
                let wmax = w as f32 - 40.0;
                p.text(&prev, w / 2.0, 158.0, 11.0, REG, with_alpha(theme::FG, 0.4), Align::Center, wmax);
                let fs = island_core::lyrics::fit_font(&cur, wmax, 15.0, 11.0);
                p.text(&cur, w / 2.0, 182.0, fs, SEMI, theme::FG, Align::Center, wmax);
                p.text(&next, w / 2.0, 206.0, 11.0, REG, with_alpha(theme::FG, 0.4), Align::Center, wmax);
            }
        }
    }

    fn timer_text(&self) -> String {
        match &self.timer {
            Some(t) if t.mode == TimerMode::Stopwatch => duration::fmt_elapsed(self.now - t.start_ms),
            Some(_) => duration::fmt_remaining(self.remaining_ms()),
            None => String::new(),
        }
    }

    fn timer_icon(&self) -> &'static str {
        match self.timer.as_ref().map(|t| (t.mode, t.phase)) {
            Some((TimerMode::Pomodoro, island_core::pomodoro::Phase::Break)) => "coffee",
            Some((TimerMode::Pomodoro, _)) => "target",
            _ => "hourglass",
        }
    }

    fn face_timer_mini(&self, p: P, w: f64, h: f64) {
        let cy = h / 2.0;
        p.icon(self.timer_icon(), 24.0, cy, 16.0, theme::ACCENT);
        p.text(&self.timer_text(), 40.0, cy, 14.0, SEMI, theme::FG, Align::Left, 90.0);
        let right = self.pill_clock(p, w - 14.0, cy);
        self.status_icons(p, right, cy, 13.0);
        p.hit(rect(0.0, 0.0, w, h), Action::Open(Page::Timer));
    }

    fn face_timer_full(&self, p: P, w: f64, h: f64) {
        let Some(t) = &self.timer else { return };
        let cx = w / 2.0;
        let label = self.timer_text();
        let tw = p.fonts.measure(&label, 26.0, SEMI) as f64;
        p.icon(self.timer_icon(), cx - tw / 2.0 - 20.0, 28.0, 22.0, theme::ACCENT);
        p.text(&label, cx + 10.0, 28.0, 26.0, SEMI, theme::FG, Align::Center, 200.0);
        let mut y = 52.0;
        if t.mode == TimerMode::Stopwatch {
            p.text("Stopwatch", cx, y + 4.0, 11.0, REG, theme::DIM, Align::Center, 200.0);
        } else {
            let frac = if t.total_ms > 0 { 1.0 - self.remaining_ms() as f64 / t.total_ms as f64 } else { 0.0 };
            p.bar(rect(14.0, y, w - 28.0, 4.0), frac, theme::ACCENT, theme::TRACK);
        }
        y += 24.0;
        let buttons: Vec<(&str, Action)> = match t.mode {
            TimerMode::Stopwatch => vec![("Stop", Action::TimerCancel)],
            TimerMode::Pomodoro => vec![("Skip", Action::PomodoroSkip), ("Stop", Action::TimerCancel)],
            TimerMode::Countdown => vec![("+1 min", Action::TimerAddMinute), ("Cancel", Action::TimerCancel)],
        };
        self.button_row(p, &buttons, cx, y + 14.0);
        let _ = h;
    }

    /// Buttons centred on `cx` at vertical centre `cy`.
    fn button_row(&self, p: P, buttons: &[(&str, Action)], cx: f64, cy: f64) {
        let widths: Vec<f64> = buttons.iter().map(|(l, _)| p.button_width(l)).collect();
        let total: f64 = widths.iter().sum::<f64>() + 8.0 * (buttons.len().saturating_sub(1)) as f64;
        let mut x = cx - total / 2.0;
        for ((label, action), bw) in buttons.iter().zip(widths) {
            p.button(rect(x, cy - 15.0, bw, 30.0), label, action.clone());
            x += bw + 8.0;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn face_simple(&self, p: P, w: f64, h: f64, icon: &str, tint: Color, title: &str, sub: &str) {
        p.icon(icon, 36.0, h / 2.0, 24.0, tint);
        p.text(title, 60.0, h / 2.0 - 9.0, 14.0, SEMI, theme::FG, Align::Left, (w - 78.0) as f32);
        p.text(sub, 60.0, h / 2.0 + 10.0, 11.0, REG, theme::DIM, Align::Left, (w - 78.0) as f32);
    }

    fn face_notif(&self, p: P, w: f64, h: f64, actions: bool) {
        let Some(n) = &self.notif else { return };
        let top = if actions { 14.0 + 30.0 } else { h / 2.0 };
        let lead_cy = if actions { 14.0 + 30.0 } else { h / 2.0 };
        match n.image.as_ref().or(n.app_icon.as_ref()) {
            Some(img) => draw_image(p, img, 14.0, lead_cy - 22.0, 44.0, if n.image.is_some() { 10.0 } else { 9.0 }),
            None => p.icon(if n.critical { "alert-triangle" } else { "bell" }, 36.0, lead_cy, 26.0, if n.critical { theme::RED } else { theme::FG }),
        }
        let more = self.queue.len();
        let tx = 72.0;
        let tmax = (w - tx - if more > 0 { 56.0 } else { 18.0 }) as f32;
        let mut app = n.app.clone();
        let group = self.queue.iter().find(|q| q.app == n.app).map(|q| q.group_count).unwrap_or(1);
        if group > 1 {
            app = format!("{app}  ·  {group} new");
        }
        p.text(&app, tx, top - 19.0, 11.0, REG, theme::DIM, Align::Left, tmax);
        p.text(&n.summary, tx, top - 2.0, 14.0, SEMI, theme::FG, Align::Left, tmax);
        p.text(&n.body, tx, top + 16.0, 12.0, REG, theme::DIM, Align::Left, tmax);
        if let (Some(pr), false) = (n.progress, actions) {
            p.bar(rect(tx, h - 18.0, tmax as f64, 5.0), pr as f64 / 100.0, theme::FG, theme::TRACK);
        }
        if more > 0 {
            p.icon_button(w - 28.0, lead_cy - 6.0, 28.0, "x", 14.0, Action::NextNotif);
            p.text(&format!("+{more}"), w - 28.0, lead_cy + 14.0, 10.0, SEMI, theme::DIM, Align::Center, 30.0);
        }
        if actions {
            let btns: Vec<(&str, Action)> = n
                .actions
                .iter()
                .filter(|a| a.key != "default")
                .take(3)
                .map(|a| (if a.key == "inline-reply" { "Reply" } else { a.label.as_str() }, Action::NotifAction(a.key.clone())))
                .collect();
            let widths: Vec<f64> = btns.iter().map(|(l, _)| p.button_width(l)).collect();
            let mut x = w - 14.0;
            for ((label, action), bw) in btns.iter().zip(widths).rev() {
                x -= bw;
                p.button(rect(x, h - 14.0 - 30.0, bw, 30.0), label, action.clone());
                x -= 8.0;
            }
        }
        // a click on the card body opens the app's default action
        if n.actions.iter().any(|a| a.key == "default") {
            p.hit(rect(0.0, 0.0, w, h - if actions { 50.0 } else { 0.0 }), Action::NotifAction("default".into()));
        } else {
            p.hit(rect(0.0, 0.0, w, h - if actions { 50.0 } else { 0.0 }), Action::Dismiss);
        }
    }

    fn face_reply(&self, p: P, w: f64, h: f64) {
        let Some(r) = &self.reply else { return };
        p.icon("message-reply", 30.0, 26.0, 18.0, theme::ACCENT);
        let title = if r.summary.is_empty() { &r.app } else { &r.summary };
        p.text(title, 50.0, 26.0, 13.0, SEMI, theme::FG, Align::Left, (w - 70.0) as f32);
        p.text(&r.body, 20.0, 50.0, 12.0, REG, theme::DIM, Align::Left, (w - 40.0) as f32);
        let field = rect(20.0, 64.0, w - 40.0, 36.0);
        p.rrect(field, 10.0, theme::CHIP);
        p.rrect_stroke(field, 10.0, 1.0, theme::TRACK);
        let inner = (field.width() - 24.0) as f32;
        if r.text.is_empty() {
            p.text(&r.placeholder, field.x0 + 12.0, field.center().y, 13.0, REG, theme::DIM, Align::Left, inner);
        } else {
            p.text(&r.text, field.x0 + 12.0, field.center().y, 13.0, REG, theme::FG, Align::Left, inner);
        }
        // the caret blinks while typing
        if (self.now / 530) % 2 == 0 {
            let before: String = r.text.chars().take(r.cursor).collect();
            let cx = field.x0 + 12.0 + p.fonts.measure(&before, 13.0, REG).min(inner) as f64;
            p.rrect(rect(cx, field.center().y - 8.0, 1.5, 16.0), 0.75, theme::FG);
        }
        let (cw, sw) = (p.button_width("Cancel"), p.button_width("Send"));
        p.button(rect(w - 20.0 - sw, h - 14.0 - 30.0, sw, 30.0), "Send", Action::ReplySend);
        p.button(rect(w - 20.0 - sw - 8.0 - cw, h - 14.0 - 30.0, cw, 30.0), "Cancel", Action::ReplyCancel);
    }

    fn face_osd(&self, p: P, w: f64, h: f64) {
        let Some(o) = self.osd else { return };
        let icon = match (o.kind, o.muted || o.level <= 0.001) {
            (OsdKind::Volume, true) => "volume-3",
            (OsdKind::Volume, false) => "volume",
            (OsdKind::Brightness, _) => "sun-high",
        };
        p.icon(icon, 30.0, h / 2.0, 20.0, theme::FG);
        let bar = rect(50.0, h / 2.0 - 3.0, w - 50.0 - 56.0, 6.0);
        p.bar(bar, if o.muted { 0.0 } else { o.level as f64 }, theme::FG, theme::TRACK);
        let pct = format!("{}%", (o.level * 100.0).round() as i32);
        p.text(&pct, w - 16.0, h / 2.0, 13.0, SEMI, theme::FG, Align::Right, 44.0);
    }

    fn face_power(&self, p: P, w: f64, h: f64) {
        let Some(ev) = self.power else { return };
        let cap = self.data.battery.map_or(0.0, |b| b.percent as f64);
        let (label, icon, tint) = match ev {
            PowerEvent::Plugged => ("Charging", "battery-charging", theme::GREEN),
            PowerEvent::Unplugged => ("On battery", "battery", theme::FG),
            PowerEvent::Low => ("Low battery", "battery-1", theme::RED),
        };
        p.icon(icon, 30.0, h / 2.0, 20.0, tint);
        p.text(label, 50.0, h / 2.0, 13.0, SEMI, if ev == PowerEvent::Low { theme::RED } else { theme::FG }, Align::Left, 110.0);
        p.bar(rect(170.0, h / 2.0 - 3.0, w - 170.0 - 56.0, 6.0), cap / 100.0, tint, theme::TRACK);
        p.text(&format!("{}%", cap.round() as i32), w - 16.0, h / 2.0, 13.0, SEMI, theme::FG, Align::Right, 44.0);
    }

    fn face_progress(&self, p: P, w: f64, h: f64) {
        let Some(op) = &self.ops.op else { return };
        let failed = op.state == OpState::Fail;
        let done = op.state == OpState::Done;
        let tint = if failed { theme::RED } else { theme::ACCENT };
        let title = if !op.label.is_empty() { op.label.as_str() } else if failed { "Failed" } else if done { "Done" } else { "Working" };
        p.icon(if failed { "alert-triangle" } else if done { "circle-check" } else { "download" }, 36.0, h / 2.0, 22.0, tint);
        p.text(title, 60.0, h / 2.0 - 9.0, 13.0, SEMI, theme::FG, Align::Left, (w - 60.0 - 70.0) as f32);
        p.bar(rect(60.0, h / 2.0 + 6.0, w - 60.0 - 70.0, 6.0), op.frac.unwrap_or(0.0), tint, theme::TRACK);
        let pct = match op.frac {
            _ if failed => "failed".to_string(),
            Some(f) => format!("{}%", (f * 100.0).round() as i32),
            None => "…".into(),
        };
        p.text(&pct, w - 18.0, h / 2.0, 13.0, SEMI, if failed { theme::RED } else { theme::FG }, Align::Right, 60.0);
    }

    fn until_text(&self, e: &calendar::Event) -> String {
        if let Some(c) = duration::countdown(e.start_ms - self.now) {
            return c;
        }
        let off = self.data.utc_offset_secs;
        let c = clock::civil(e.start_ms, off);
        let today = clock::civil(self.now, off);
        if (c.year, c.month, c.day) == (today.year, today.month, today.day) {
            format!("at {}", c.hhmm())
        } else {
            format!("{} {}", &clock::WEEKDAYS[c.weekday as usize][..3], c.hhmm())
        }
    }

    fn face_meet_mini(&self, p: P, w: f64, h: f64) {
        let Some(e) = self.next_event() else { return };
        let cy = h / 2.0;
        p.icon("calendar-event", 24.0, cy, 15.0, theme::ACCENT);
        let mut right = self.pill_clock(p, w - 14.0, cy);
        right = self.status_icons(p, right, cy, 13.0);
        let until = self.until_text(e);
        let uw = p.text(&until, right, cy, 12.0, REG, theme::DIM, Align::Right, 90.0) as f64;
        p.text(&e.title, 40.0, cy, 12.0, SEMI, theme::FG, Align::Left, (right - uw - 48.0).max(30.0) as f32);
        p.hit(rect(0.0, 0.0, w, h), Action::Open(Page::Meeting));
    }

    fn face_meeting(&self, p: P, w: f64, h: f64) {
        let e = self.meeting.as_ref().or_else(|| self.next_event());
        let Some(e) = e else { return };
        let join = e.url.as_deref().filter(|u| !u.is_empty());
        let right = if join.is_some() { 78.0 } else { 18.0 };
        p.icon("calendar-event", 36.0, h / 2.0, 24.0, theme::ACCENT);
        p.text(&e.title, 60.0, h / 2.0 - 9.0, 14.0, SEMI, theme::FG, Align::Left, (w - 60.0 - right) as f32);
        p.text(&self.until_text(e), 60.0, h / 2.0 + 10.0, 11.0, REG, theme::DIM, Align::Left, (w - 60.0 - right) as f32);
        if let Some(url) = join {
            let bw = p.button_width("Join");
            p.button(rect(w - 18.0 - bw, h / 2.0 - 15.0, bw, 30.0), "Join", Action::Join(url.to_string()));
        }
    }

    fn face_home(&self, p: P, w: f64, h: f64) {
        let off = self.data.utc_offset_secs;
        let c = clock::civil(self.now, off);
        p.text(&c.hhmm(), 22.0, 28.0 + 14.0, 30.0, SEMI, theme::FG, Align::Left, 140.0);
        p.text(&c.long_date(), 22.0, 28.0 + 38.0, 12.0, REG, theme::DIM, Align::Left, 200.0);
        if let Some(wx) = &self.data.weather {
            let x = w - 22.0;
            p.icon(weather::glyph(wx.code, wx.day), x - 72.0, 40.0, 28.0, theme::FG);
            p.text(&format!("{}°", wx.temp_c.round() as i32), x, 38.0, 22.0, SEMI, theme::FG, Align::Right, 80.0);
            p.text(&format!("{}° / {}°", wx.high_c.round() as i32, wx.low_c.round() as i32), x, 62.0, 11.0, REG, theme::DIM, Align::Right, 100.0);
        }
        let mut y = 28.0 + 44.0 + 10.0 + 17.0;
        // buttons row
        let mut buttons: Vec<(String, Action)> = vec![];
        for s in self.cfg.timer_presets.iter().take(3) {
            buttons.push((duration::preset_label(*s), Action::TimerStart(*s)));
        }
        buttons.push(("Pomodoro".into(), Action::PomodoroStart));
        buttons.push(("Calendar".into(), Action::Open(Page::Cal)));
        let widths: Vec<f64> = buttons.iter().map(|(l, _)| p.button_width(l)).collect();
        let total: f64 = widths.iter().sum::<f64>() + 8.0 * (buttons.len() - 1) as f64;
        let mut x = (w - total) / 2.0;
        for ((label, action), bw) in buttons.iter().zip(widths) {
            p.button(rect(x, y - 17.0, bw, 34.0), label, action.clone());
            x += bw + 8.0;
        }
        y += 17.0 + 10.0 + 9.0;
        if let Some(e) = self.next_event() {
            if y < h {
                p.icon("calendar-event", 30.0, y, 14.0, theme::ACCENT);
                let when = self.until_text(e);
                let ww = p.text(&when, w - 22.0, y, 11.0, REG, theme::DIM, Align::Right, 100.0) as f64;
                p.text(&e.title, 46.0, y, 12.0, SEMI, theme::FG, Align::Left, (w - 46.0 - ww - 36.0) as f32);
            }
        }
    }

    /// The header of a page: a back button and the title; returns the vertical centre of the header.
    fn page_header(&self, p: P, w: f64, title: &str) -> f64 {
        let cy = 14.0 + 17.0;
        p.icon_button(14.0 + 14.0, cy, 28.0, "arrow-left", 15.0, Action::Back);
        p.text(title, 14.0 + 28.0 + 8.0, cy, 14.0, SEMI, theme::FG, Align::Left, (w - 160.0) as f32);
        cy
    }

    fn face_cal(&self, p: P, w: f64, h: f64) {
        let c = &self.cal;
        let hy = self.page_header(p, w, &format!("{} {}", clock::MONTHS[c.month as usize - 1], c.year));
        p.icon_button(w - 14.0 - 14.0 - 60.0, hy, 28.0, "chevron-left", 16.0, Action::CalPrev);
        p.icon_button(w - 14.0 - 14.0 - 30.0, hy, 28.0, "calendar-event", 15.0, Action::CalToday);
        p.icon_button(w - 14.0 - 14.0, hy, 28.0, "chevron-right", 16.0, Action::CalNext);
        let colw = (w - 28.0) / 7.0;
        let top = 14.0 + 34.0 + 4.0;
        for (i, d) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
            p.text(d, 14.0 + colw * (i as f64 + 0.5), top + 8.0, 10.0, REG, theme::DIM, Align::Center, 20.0);
        }
        let today = clock::civil(self.now, self.data.utc_offset_secs);
        let grid = calendar::month_grid(c.year, c.month);
        let marks = self.month_marks();
        for (r, row) in grid.iter().enumerate() {
            for (i, d) in row.iter().enumerate() {
                let Some(d) = d else { continue };
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
            p.text(&e.title, 76.0, y, 12.0, SEMI, theme::FG, Align::Left, (w - 76.0 - 40.0) as f32);
            if has_url {
                p.icon("external-link", w - 26.0, y, 13.0, theme::DIM);
                p.hit(row, Action::Join(e.url.clone().unwrap_or_default()));
            }
            y += 24.0;
        }
        let _ = h;
    }

    /// Days (index = day of month) of the shown month that have an event, with the first event's colour.
    fn month_marks(&self) -> Vec<Option<Color>> {
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

    fn slider_row(&self, p: P, w: f64, cy: f64, icon: &str, value: Option<f32>, action: Action) {
        p.icon(icon, 14.0 + 8.0, cy, 16.0, theme::FG);
        let bar = rect(14.0 + 26.0, cy - 3.0, w - 14.0 - 26.0 - 14.0 - 44.0, 6.0);
        p.slider(bar, value.unwrap_or(0.0) as f64, action);
        let t = value.map_or("–".to_string(), |v| format!("{}%", (v * 100.0).round() as i32));
        p.text(&t, w - 14.0, cy, 11.0, REG, theme::DIM, Align::Right, 40.0);
    }

    fn face_qs(&self, p: P, w: f64, _h: f64) {
        self.page_header(p, w, "Quick settings");
        let d = &self.data;
        let cw = (w - 28.0 - 16.0) / 3.0;
        let chip = |p: P, row: usize, col: usize, icon: &str, label: &str, on: bool, a: Action| {
            let y = 14.0 + 34.0 + 8.0 + row as f64 * 44.0;
            p.chip(rect(14.0 + col as f64 * (cw + 8.0), y, cw, 36.0), icon, label, on, a);
        };
        chip(p, 0, 0, if d.wifi == Some(false) { "wifi-off" } else { "wifi" }, "Wi-Fi", d.wifi == Some(true), Action::Cmd(Command::ToggleWifi));
        chip(p, 0, 1, if d.bluetooth == Some(false) { "bluetooth-off" } else { "bluetooth" }, "Bluetooth", d.bluetooth == Some(true), Action::Cmd(Command::ToggleBluetooth));
        chip(p, 0, 2, "bell-off", "DND", d.dnd, Action::ToggleDnd);
        let profile = d.profile.as_deref().unwrap_or("");
        let plabel = if profile.is_empty() { "Profile".to_string() } else { profile[..1].to_uppercase() + &profile[1..] };
        let picon = match profile {
            "performance" => "bolt",
            "powersave" => "leaf",
            _ => "gauge",
        };
        chip(p, 1, 0, "coffee", "Keep awake", d.keep_awake, Action::Cmd(Command::ToggleKeepAwake));
        chip(p, 1, 1, picon, &plabel, profile == "performance", Action::Cmd(Command::NextProfile));
        let sy = 14.0 + 34.0 + 8.0 + 2.0 * 44.0 + 14.0;
        self.slider_row(p, w, sy, if d.muted { "volume-3" } else { "volume" }, d.volume, Action::VolumeBar);
        self.slider_row(p, w, sy + 36.0, "brightness-half", d.brightness, Action::BrightnessBar);
        let out = d.sinks.iter().find(|s| Some(&s.name) == d.sink.as_ref());
        let headphones = out.is_some_and(|s| s.name.contains("bluez"));
        let r = rect(14.0, sy + 36.0 + 24.0, w - 28.0, 32.0);
        p.rrect(r, 12.0, if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
        p.icon(if headphones { "headphones" } else { "device-speaker" }, r.x0 + 18.0, r.center().y, 16.0, theme::FG);
        p.text(out.map_or("Output", |s| s.label.as_str()), r.x0 + 34.0, r.center().y, 12.0, SEMI, theme::FG, Align::Left, (r.width() - 120.0) as f32);
        if d.sinks.len() > 1 {
            p.text("switch", r.x1 - 12.0, r.center().y, 11.0, REG, theme::DIM, Align::Right, 60.0);
        }
        p.hit(r, Action::Cmd(Command::NextSink));
    }

    fn metric_row(&self, p: P, w: f64, y: f64, label: &str, frac: Option<f64>, text: &str, color: Color) {
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

    fn face_sys(&self, p: P, w: f64, _h: f64) {
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
        let net = format!("↓ {}   ↑ {}", island_core::system::fmt_rate(st.net_rx), island_core::system::fmt_rate(st.net_tx));
        self.metric_row(p, w, y, "Network", None, &net, theme::FG);
        y += 28.0;
        if let Some((used, total)) = st.disk {
            let f = if total > 0.0 { used / total } else { 0.0 };
            self.metric_row(p, w, y, "Disk /", Some(f), &format!("{} / {}", island_core::system::fmt_mb(used), island_core::system::fmt_mb(total)), load_color(f));
        }
    }

    fn face_clip(&self, p: P, w: f64, _h: f64) {
        let hy = self.page_header(p, w, "Clipboard");
        p.text("click to copy", w - 14.0, hy, 11.0, REG, theme::DIM, Align::Right, 120.0);
        let mut y = 14.0 + 34.0 + 4.0;
        if self.data.clipboard.is_empty() {
            p.text("Clipboard history is empty", 14.0, y + 14.0, 12.0, REG, theme::DIM, Align::Left, 300.0);
        }
        for (i, t) in self.data.clipboard.iter().take(5).enumerate() {
            let row = rect(14.0, y, w - 28.0, 28.0);
            p.rrect(row, 10.0, if p.hovered(row) { theme::HOT_CHIP } else { theme::CHIP });
            let copied = self.copied.is_some_and(|(j, until)| j == i && self.now < until);
            p.icon(if copied { "circle-check" } else { "clipboard-text" }, row.x0 + 18.0, y + 14.0, 14.0, if copied { theme::ACCENT } else { theme::DIM });
            let one_line: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
            p.text(&one_line, row.x0 + 34.0, y + 14.0, 12.0, REG, theme::FG, Align::Left, (row.width() - 46.0) as f32);
            p.hit(row, Action::ClipRestore(i));
            y += 32.0;
        }
    }

    fn face_notifs(&self, p: P, w: f64, _h: f64) {
        let hy = self.page_header(p, w, "Notifications");
        let clear = p.button_width("Clear");
        if !self.history.is_empty() {
            p.button(rect(w - 14.0 - clear, hy - 15.0, clear, 30.0), "Clear", Action::ClearHistory);
        }
        let dnd = rect(w - 14.0 - clear - 8.0 - 76.0, hy - 15.0, 76.0, 30.0);
        let on = self.data.dnd;
        p.rrect(dnd, 15.0, if on { theme::ACCENT } else if p.hovered(dnd) { theme::HOT_CHIP } else { theme::CHIP });
        p.icon("bell-off", dnd.x0 + 16.0, hy, 14.0, if on { theme::ON_ACCENT } else { theme::FG });
        p.text("DND", dnd.x0 + 28.0, hy, 12.0, SEMI, if on { theme::ON_ACCENT } else { theme::FG }, Align::Left, 40.0);
        p.hit(dnd, Action::ToggleDnd);
        let mut y = 14.0 + 34.0 + 6.0;
        if self.history.is_empty() {
            p.text("No notifications", w / 2.0, y + 20.0, 12.0, REG, theme::DIM, Align::Center, 200.0);
        }
        for (i, (n, at)) in self.history.iter().take(5).enumerate() {
            let row = rect(14.0, y, w - 28.0, 40.0);
            p.rrect(row, 12.0, if p.hovered(row) { theme::HOT_CHIP } else { theme::CHIP });
            match n.image.as_ref().or(n.app_icon.as_ref()) {
                Some(img) => draw_image(p, img, row.x0 + 10.0, y + 6.0, 28.0, if n.image.is_some() { 14.0 } else { 7.0 }),
                None => p.icon("bell", row.x0 + 24.0, y + 20.0, 18.0, theme::DIM),
            }
            let tx = row.x0 + 48.0;
            let tmax = (row.width() - 48.0 - 40.0) as f32;
            p.text(&format!("{}  ·  {}", n.app, ago(self.now - at)), tx, y + 12.0, 10.0, REG, theme::DIM, Align::Left, tmax);
            p.text(if n.summary.is_empty() { &n.body } else { &n.summary }, tx, y + 27.0, 12.0, SEMI, theme::FG, Align::Left, tmax);
            p.icon_button(row.x1 - 18.0, y + 20.0, 26.0, "x", 13.0, Action::HistoryRemove(i));
            p.hit(row, Action::HistoryOpen(i));
            y += 46.0;
        }
    }

    fn draw_bubble(&self, p: P, side: Side, w: f64, h: f64) {
        let (icon, text) = match side {
            Side::Timer => (self.timer_icon(), match &self.timer {
                Some(t) if t.mode == TimerMode::Stopwatch => duration::fmt_elapsed(self.now - t.start_ms),
                _ => duration::fmt_bubble(self.remaining_ms()),
            }),
            Side::Meeting => ("calendar-event", self.next_event().map(|e| duration::fmt_bubble((e.start_ms - self.now).max(0))).unwrap_or_default()),
            Side::Focus => ("target", "on".into()),
            Side::Media => ("music", String::new()),
            Side::Op => match &self.ops.op {
                Some(o) => (
                    match o.state {
                        OpState::Fail => "alert-triangle",
                        OpState::Done => "circle-check",
                        OpState::Run => "download",
                    },
                    o.frac.map_or("…".to_string(), |f| format!("{}%", (f * 100.0).round() as i32)),
                ),
                None => ("download", String::new()),
            },
        };
        let tw = p.fonts.measure(&text, 12.0, SEMI) as f64;
        let group = 13.0 + 4.0 + tw;
        let x0 = (w - group) / 2.0;
        p.icon(icon, x0 + 6.5, h / 2.0, 13.0, theme::ACCENT);
        p.text(&text, x0 + 17.0, h / 2.0, 12.0, SEMI, theme::FG, Align::Left, 50.0);
        p.hit(rect(0.0, 0.0, w, h), Action::Open(side_to_page(side)));
    }
}

fn side_to_page(s: Side) -> Page {
    match s {
        Side::Timer => Page::Timer,
        Side::Meeting => Page::Meeting,
        Side::Focus => Page::Focus,
        Side::Media => Page::Media,
        Side::Op => Page::Op,
    }
}

fn ago(delta_ms: i64) -> String {
    let m = delta_ms.max(0) / 60_000;
    if m < 1 {
        "now".into()
    } else if m < 60 {
        format!("{m} min")
    } else if m < 24 * 60 {
        format!("{} h", m / 60)
    } else {
        format!("{} d", m / (24 * 60))
    }
}

/// An image drawn into a rounded square (`cover` fit).
fn draw_image(p: P, img: &Image, x: f64, y: f64, size: f64, radius: f64) {
    use vello::peniko::{Blob, ImageAlphaType, ImageBrush, ImageData, ImageFormat};
    let data = ImageData {
        data: Blob::new(img.rgba.clone() as std::sync::Arc<dyn AsRef<[u8]> + Send + Sync>),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width: img.width,
        height: img.height,
    };
    let brush = ImageBrush::new(data);
    let s = size / img.width.min(img.height).max(1) as f64;
    let ox = x - (img.width as f64 * s - size) / 2.0;
    let oy = y - (img.height as f64 * s - size) / 2.0;
    let clip = RoundedRect::new(x, y, x + size, y + size, radius);
    let t = p.transform;
    p.scene.push_clip_layer(Fill::NonZero, t, &clip);
    p.scene.draw_image(&brush, t * Affine::translate((ox, oy)) * Affine::scale(s));
    p.scene.pop_layer();
}
