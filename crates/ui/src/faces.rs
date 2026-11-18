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
            // not part of this build (no clipboard history, focus mode or update check yet)
            Face::Clip | Face::FocusMini | Face::FocusFull | Face::Update => {}
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
        if m.players > 1 {
            p.icon_button(w - 70.0, 30.0, 28.0, "arrows-exchange", 14.0, Action::CyclePlayer);
        }
        // progress + times
        let pos = self.media_position();
        let frac = if m.length_ms > 0.0 { pos / m.length_ms } else { 0.0 };
        let bar = rect(92.0, 62.0, w - 92.0 - 14.0, 5.0);
        p.bar(bar, frac, theme::FG, theme::TRACK);
        if m.can_seek {
            p.hit(rect(bar.x0, bar.y0 - 6.0, bar.width(), 17.0), Action::SeekBar);
        }
        p.text(&duration::fmt_elapsed(pos as i64), bar.x0, 77.0, 10.0, REG, theme::DIM, Align::Left, 60.0);
        let rem = format!("-{}", duration::fmt_elapsed((m.length_ms - pos).max(0.0) as i64));
        p.text(&rem, bar.x1, 77.0, 10.0, REG, theme::DIM, Align::Right, 60.0);
        // controls
        let cy = 112.0;
        let cx = w / 2.0;
        p.icon_button(cx - 56.0, cy, 34.0, "player-skip-back", 20.0, Action::Cmd(Command::MediaPrevious));
        let icon = if m.playing { "player-pause" } else { "player-play" };
        p.icon_button(cx, cy, 40.0, icon, 26.0, Action::Cmd(Command::MediaPlayPause));
        p.icon_button(cx + 56.0, cy, 34.0, "player-skip-forward", 20.0, Action::Cmd(Command::MediaNext));
        if lyric {
            if let Some((prev, cur, next)) = self.lyric_window() {
                let wmax = w as f32 - 40.0;
                p.text(&prev, w / 2.0, 150.0, 11.0, REG, with_alpha(theme::FG, 0.4), Align::Center, wmax);
                let fs = island_core::lyrics::fit_font(&cur, wmax, 15.0, 11.0);
                p.text(&cur, w / 2.0, 174.0, fs, SEMI, theme::FG, Align::Center, wmax);
                p.text(&next, w / 2.0, 198.0, 11.0, REG, with_alpha(theme::FG, 0.4), Align::Center, wmax);
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

    fn page_header(&self, p: P, w: f64, title: &str) {
        p.text(title, 20.0, 28.0 + 12.0, 15.0, SEMI, theme::FG, Align::Left, (w - 120.0) as f32);
    }

    fn face_cal(&self, p: P, w: f64, h: f64) {
        let c = &self.cal;
        let title = format!("{} {}", clock::MONTHS[c.month as usize - 1], c.year);
        self.page_header(p, w, &title);
        p.icon_button(w - 70.0, 40.0, 28.0, "player-skip-back", 14.0, Action::CalPrev);
        p.icon_button(w - 34.0, 40.0, 28.0, "player-skip-forward", 14.0, Action::CalNext);
        let colw = (w - 40.0) / 7.0;
        let top = 28.0 + 34.0 + 6.0;
        for (i, d) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
            p.text(d, 20.0 + colw * (i as f64 + 0.5), top + 8.0, 10.0, SEMI, theme::DIM, Align::Center, 20.0);
        }
        let today = clock::civil(self.now, self.data.utc_offset_secs);
        let grid = calendar::month_grid(c.year, c.month);
        let marks = self.month_marks();
        for (r, row) in grid.iter().enumerate() {
            for (i, d) in row.iter().enumerate() {
                let Some(d) = d else { continue };
                let cx = 20.0 + colw * (i as f64 + 0.5);
                let cy = top + 16.0 + r as f64 * 30.0 + 15.0;
                let is_today = (c.year, c.month, *d) == (today.year, today.month, today.day);
                let selected = *d == c.day;
                if is_today {
                    p.circle(cx, cy, 12.0, theme::FG);
                } else if selected {
                    p.circle(cx, cy, 12.0, theme::HOT_CHIP);
                }
                p.text(&d.to_string(), cx, cy, 12.0, if is_today { SEMI } else { REG }, if is_today { theme::ON_ACCENT } else { theme::FG }, Align::Center, 24.0);
                if marks.get(*d as usize).copied().unwrap_or(false) && !is_today {
                    p.circle(cx, cy + 10.0, 1.8, theme::ORANGE);
                }
                p.hit(rect(cx - colw / 2.0, cy - 15.0, colw, 30.0), Action::CalDay(*d));
            }
        }
        let mut y = top + 16.0 + grid.len() as f64 * 30.0 + 10.0 + 12.0;
        let evs = self.day_events();
        if evs.is_empty() {
            p.text("No events", 20.0, y, 12.0, REG, theme::DIM, Align::Left, 200.0);
        }
        let off = self.data.utc_offset_secs;
        for e in evs.iter().take(4) {
            let when = if e.all_day { "all day".to_string() } else { clock::civil(e.start_ms, off).hhmm() };
            let dot = e.color.map(theme::rgb).unwrap_or(theme::ORANGE);
            p.circle(26.0, y, 3.0, dot);
            p.text(&when, 38.0, y, 11.0, REG, theme::DIM, Align::Left, 60.0);
            p.text(&e.title, 100.0, y, 12.0, SEMI, theme::FG, Align::Left, (w - 120.0) as f32);
            y += 24.0;
        }
        let _ = h;
    }

    /// Days (index = day of month) of the shown month that have an event.
    fn month_marks(&self) -> Vec<bool> {
        let c = &self.cal;
        let off = self.data.utc_offset_secs;
        let days = calendar::days_in_month(c.year, c.month);
        let mut marks = vec![false; days as usize + 2];
        for d in 1..=days {
            let (s, e) = (clock::midnight_ms(c.year, c.month, d, off), clock::midnight_ms(c.year, c.month, d + 1, off));
            marks[d as usize] = self.data.events.iter().any(|ev| ev.start_ms < e && ev.end_ms > s);
        }
        marks
    }

    fn slider_row(&self, p: P, w: f64, cy: f64, icon: &str, value: Option<f32>, action: Action) {
        p.icon(icon, 34.0, cy, 18.0, theme::FG);
        let bar = rect(54.0, cy - 5.0, w - 54.0 - 24.0, 10.0);
        p.bar(bar, value.unwrap_or(0.0) as f64, theme::FG, theme::TRACK);
        p.hit(rect(bar.x0 - 4.0, bar.y0 - 8.0, bar.width() + 8.0, 26.0), action);
    }

    fn face_qs(&self, p: P, w: f64, _h: f64) {
        self.page_header(p, w, "Quick settings");
        let muted_icon = if self.data.muted { "volume-3" } else { "volume" };
        self.slider_row(p, w, 78.0, muted_icon, self.data.volume, Action::VolumeBar);
        self.slider_row(p, w, 112.0, "sun-high", self.data.brightness, Action::BrightnessBar);
        let dnd = if self.data.dnd { "Do Not Disturb: on" } else { "Do Not Disturb" };
        let mute = if self.data.muted { "Unmute" } else { "Mute" };
        self.button_row(p, &[(dnd, Action::ToggleDnd), (mute, Action::Cmd(Command::ToggleMute))], w / 2.0, 156.0);
    }

    fn face_sys(&self, p: P, w: f64, _h: f64) {
        self.page_header(p, w, "Power");
        match self.data.battery {
            Some(b) => {
                p.text(&format!("{}%", b.percent.round() as i32), 24.0, 84.0, 30.0, SEMI, theme::FG, Align::Left, 120.0);
                let state = if b.charging { "Charging" } else if b.on_ac { "Plugged in" } else { "On battery" };
                p.text(state, w - 24.0, 80.0, 13.0, SEMI, theme::FG, Align::Right, 160.0);
                p.bar(rect(24.0, 112.0, w - 48.0, 8.0), b.percent as f64 / 100.0, if b.percent <= self.cfg.low_battery && !b.on_ac { theme::RED } else { theme::FG }, theme::TRACK);
            }
            None => {
                p.text("No battery", 24.0, 84.0, 13.0, REG, theme::DIM, Align::Left, 200.0);
            }
        }
    }

    fn face_notifs(&self, p: P, w: f64, _h: f64) {
        self.page_header(p, w, "Notifications");
        if !self.history.is_empty() {
            let bw = p.button_width("Clear");
            p.button(rect(w - 20.0 - bw, 28.0 + 6.0, bw, 26.0), "Clear", Action::ClearHistory);
        }
        let mut y = 28.0 + 34.0 + 8.0;
        if self.history.is_empty() {
            p.text("Nothing new", 20.0, y + 12.0, 12.0, REG, theme::DIM, Align::Left, 200.0);
        }
        for (n, at) in self.history.iter().take(5) {
            match &n.image {
                Some(img) => draw_image(p, img, 20.0, y + 3.0, 36.0, 8.0),
                None => p.icon("bell", 38.0, y + 23.0, 20.0, theme::DIM),
            }
            p.text(&n.app, 68.0, y + 10.0, 10.0, REG, theme::DIM, Align::Left, (w - 160.0) as f32);
            p.text(&ago(self.now - at), w - 20.0, y + 10.0, 10.0, REG, theme::DIM, Align::Right, 70.0);
            p.text(&n.summary, 68.0, y + 25.0, 12.0, SEMI, theme::FG, Align::Left, (w - 90.0) as f32);
            p.text(&n.body, 68.0, y + 40.0, 11.0, REG, theme::DIM, Align::Left, (w - 90.0) as f32);
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
