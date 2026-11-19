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
use island_core::timeline;
use island_core::progress::State as OpState;
use island_core::weather;

const PALETTE: [(u8, u8, u8); 8] = [(0xff, 0x45, 0x3a), (0xff, 0x9f, 0x0a), (0xff, 0xd6, 0x0a), (0x30, 0xd1, 0x58), (0x0a, 0x84, 0xff), (0xbf, 0x5a, 0xf2), (0xff, 0x37, 0x5f), (0x98, 0x98, 0x9d)];
use vello::kurbo::{Affine, Rect, RoundedRect};
use vello::peniko::{Color, Fill};
use vello::Scene;

type P<'a, 'b> = &'a mut Painter<'b, Action>;

/// Positions on the quick settings page.
pub(crate) struct QsLayout {
    pub chips_top: f64,
    pub focus_y: Option<f64>,
    pub vol_cy: f64,
    pub bri_cy: Option<f64>,
    pub out_top: Option<f64>,
    /// The first of the connected Bluetooth devices (rows of 32 with 6 between).
    pub dev_top: Option<f64>,
    pub height: f64,
}
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
        // a page change slides sideways (the way the pages are ordered); other changes rise a little
        let (dx, dy) = if self.slide != 0.0 && has_dock(self.shown) { (self.slide * 22.0 * (1.0 - op as f64), 0.0) } else { (0.0, -3.0 * (1.0 - op as f64)) };
        let content_t = base
            * Affine::translate((w / 2.0 + dx, h / 2.0 + dy))
            * Affine::scale(k)
            * Affine::translate((-w / 2.0, -h / 2.0));
        scene.push_clip_layer(Fill::NonZero, base, &RoundedRect::new(0.0, 0.0, w, h, radius));
        {
            let mut p = Painter { scene, fonts, hits: &mut hits, opacity: op, pointer, transform: content_t, base: (op, content_t), since_enter: self.now - self.enter_at };
            let dock = if has_dock(self.shown) { DOCK_H } else { 0.0 };
            self.draw_face(&mut p, self.shown, w, h - dock);
            if dock > 0.0 {
                self.draw_dock(&mut p, w, h);
            }
        }
        // the keyboard focus ring
        if let Some(kb) = self.kb {
            let nav: Vec<usize> = hits.iter().enumerate().filter(|(_, hit)| !(hit.rect.width() > w * 0.9 && hit.rect.height() > h * 0.5)).map(|(i, _)| i).collect();
            if let Some(hit) = nav.get(kb.index % nav.len().max(1)).and_then(|i| hits.get(*i)) {
                let r = hit.rect.inflate(2.0, 2.0);
                scene.stroke(&vello::kurbo::Stroke::new(2.0), content_t, theme::FG, None, &RoundedRect::from_rect(r, (r.height() / 2.0).min(14.0)));
            }
        }
        // the cover art is one image that travels between the small pill's spot and the open player's,
        // following the shape's height, so it never cross-fades with the rest of the content
        if matches!(self.shown, Face::MediaMini | Face::MediaFull | Face::MediaLyric) {
            let t = ((h - 28.0) / (160.0 - 28.0)).clamp(0.0, 1.0);
            let t = t * t * (3.0 - 2.0 * t);
            let lerp = |a: f64, b: f64| a + (b - a) * t;
            let size = lerp(20.0, 64.0);
            let (x, y, r) = (lerp(8.0, 14.0), lerp((28.0 - 20.0) / 2.0, 14.0), lerp(6.0, 12.0));
            let mut p = Painter { scene, fonts, hits: &mut hits, opacity: 1.0, pointer: None, transform: base, base: (1.0, base), since_enter: 10_000 };
            let art = self.data.media.as_ref().and_then(|m| m.art.as_ref());
            self.draw_art(&mut p, art, x, y, size, r);
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
            let mut p = Painter { scene, fonts, hits: &mut hits, opacity: kk as f32, pointer: None, transform: bt, base: (kk as f32, bt), since_enter: 10_000 };
            self.draw_bubble(&mut p, side, BUBBLE_W, h);
            // the bubble's hit region is in the pill's coordinates (its own drawing is shifted and scaled)
            hits.push(Hit { rect: rect(bx, 0.0, BUBBLE_W, h), action: Action::Open(side_to_page(side)) });
        }
        self.hits = hits;
    }

    /// The row of page icons at the bottom of a page: a click goes straight to that page.
    fn draw_dock(&self, p: P, w: f64, h: f64) {
        p.stagger(9);
        let pages = self.pages();
        let current = self.page.or_else(|| page_of(self.shown));
        let n = pages.len().max(1) as f64;
        let step = ((w - 28.0) / n).min(38.0);
        let x0 = (w - step * n) / 2.0;
        let cy = h - DOCK_H / 2.0 - 1.0;
        p.rrect(rect(14.0, h - DOCK_H, w - 28.0, 1.0), 0.5, with_alpha(theme::FG, 0.1));
        for (i, pg) in pages.iter().enumerate() {
            let cx = x0 + step * (i as f64 + 0.5);
            let cell = rect(cx - step / 2.0 + 1.0, cy - 12.0, step - 2.0, 24.0);
            let here = Some(*pg) == current;
            if here {
                p.rrect(cell, 12.0, theme::CHIP);
            } else if p.hovered(cell) {
                p.rrect(cell, 12.0, with_alpha(theme::FG, 0.08));
            }
            p.icon(page_icon(*pg), cx, cy, 14.0, if here { theme::FG } else if p.hovered(cell) { with_alpha(theme::FG, 0.8) } else { with_alpha(theme::FG, 0.45) });
            p.hit(cell, Action::Goto(*pg));
        }
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
            Face::Device => self.face_device(p, w, h),
            Face::Weather => self.face_weather(p, w, h),
            Face::Options => self.face_options(p, w, h),
            Face::Audio => self.face_audio(p, w, h),
            Face::Today => self.face_today(p, w, h),
            Face::Send => self.face_send(p, w, h),
            Face::Activity => self.face_activity(p, w, h),
            Face::Bar => self.face_bar(p, w, h),
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
            Face::Settings => self.face_settings(p, w, h),
            Face::FocusMini => self.face_focus_mini(p, w, h),
            Face::FocusFull => self.face_focus_full(p, w, h),
            // not part of this build (no clipboard history, focus mode or update check yet)
            Face::Update => {}
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
        for (on, icon, color) in [(self.quiet(), "bell-off", theme::DIM), (pr.screen, "screen-share", theme::ORANGE), (pr.camera, "video", theme::GREEN), (pr.mic, "microphone", theme::ORANGE), (!self.data.net.is_ok(), "wifi-off", theme::RED)] {
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

    #[allow(clippy::too_many_arguments)]
    fn equalizer(&self, p: P, x: f64, cy: f64, n: usize, max_h: f64, bar_w: f64, playing: bool, tint: Color) {
        let t = self.now as f64 / 1000.0;
        // real audio levels when fresh (lows on the left, a little gain), else a cheap animation
        let live = playing && !self.spectrum.is_empty() && self.now - self.spectrum_at < 300;
        for i in 0..n {
            let hh = if live {
                let band = self.spectrum[(i * self.spectrum.len() / n).min(self.spectrum.len() - 1)] as f64;
                3.0 + (max_h - 3.0) * (band * 1.2).min(1.0)
            } else if playing {
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
        if !self.cfg.art_tint {
            return theme::FG;
        }
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
        let mut right = self.pill_clock(p, w - 14.0, cy);
        right = self.status_icons(p, right, cy, 13.0);
        self.equalizer(p, right - 22.0, cy, 4, 12.0, 2.5, self.playing_shown(), self.art_tint());
        let max = (right - 26.0 - 38.0).max(20.0) as f32;
        // with the option on, the line being sung replaces the title and rises into place on every change
        match self.cfg.lyrics_in_pill.then(|| self.lyric_now()).flatten().filter(|_| m.playing) {
            Some((line, e)) => {
                let base = p.opacity;
                p.opacity = base * e as f32;
                p.text(&line, 38.0, cy + 5.0 * (1.0 - e), 12.0, SEMI, theme::FG, Align::Left, max);
                p.opacity = base;
            }
            None => {
                p.text(&m.title, 38.0, cy, 12.0, SEMI, theme::FG, Align::Left, max);
            }
        }
        p.hit(rect(0.0, 0.0, w, h), Action::Open(Page::Media));
    }

    fn face_media_full(&self, p: P, w: f64, _h: f64, lyric: bool) {
        let Some(m) = self.data.media.as_ref() else { return };
        p.text(&m.title, 92.0, 30.0, 14.0, SEMI, theme::FG, Align::Left, (w - 92.0 - 56.0) as f32);
        p.text(&m.artist, 92.0, 50.0, 12.0, REG, theme::DIM, Align::Left, (w - 92.0 - 56.0) as f32);
        self.equalizer(p, w - 40.0, 30.0, 4, 14.0, 3.0, self.playing_shown(), self.art_tint());
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
        p.icon_button_nudged((cx - 64.0, cy), 36.0, "player-skip-back", 20.0, 5.0 * self.skip_press.min(0.0), Action::Cmd(Command::MediaPrevious));
        let disc = rect(cx - 24.0, cy - 24.0, 48.0, 48.0);
        // the disc swells under the pointer and gives when pressed; the two icons cross-fade, each growing in
        let (hover, press, a) = (self.disc_hover, self.disc_press, self.play_anim.clamp(0.0, 1.0));
        p.circle(cx, cy, 22.0 + 1.5 * hover - 3.5 * press, theme::FG);
        let base = p.opacity;
        for (icon, shown) in [("player-play", 1.0 - a), ("player-pause", a)] {
            if shown > 0.01 {
                p.opacity = base * shown as f32;
                p.icon(icon, cx, cy, (17.0 + 5.0 * shown - 2.0 * press) as f32, theme::ON_ACCENT);
            }
        }
        p.opacity = base;
        p.hit(disc, Action::Cmd(Command::MediaPlayPause));
        p.icon_button_nudged((cx + 64.0, cy), 36.0, "player-skip-forward", 20.0, 5.0 * self.skip_press.max(0.0), Action::Cmd(Command::MediaNext));
        if m.players > 1 {
            p.icon_button(cx - 128.0, cy, 34.0, "arrows-exchange", 17.0, Action::CyclePlayer);
        }
        if self.cfg.clipboard_history {
            p.icon_button(cx + 128.0, cy, 34.0, "clipboard-text", 17.0, Action::Open(Page::Clip));
        }
        if lyric {
            if let Some(strip) = self.lyric_strip() {
                self.draw_lyrics(p, w, &strip);
            }
        }
    }

    /// The lyrics strip: lines scroll up together as the song moves on; the old current line dims into the
    /// previous slot while the incoming one grows and brightens in the middle.
    fn draw_lyrics(&self, p: P, w: f64, strip: &[(String, f32)]) {
        let (centre, row_h) = (182.0, 24.0);
        let wmax = w as f32 - 40.0;
        let base = p.opacity;
        // the strip scrolls under the controls: keep it inside its own area
        let t = p.transform;
        p.scene.push_clip_layer(Fill::NonZero, t, &Rect::new(0.0, 146.0, w, 400.0));
        for (text, slot) in strip {
            let (emphasis, opacity) = island_core::lyrics::line_look(*slot);
            if text.is_empty() || opacity <= 0.01 {
                continue;
            }
            let size = 11.0 + 4.0 * emphasis;
            let size = island_core::lyrics::fit_font(text, wmax, size, 10.0);
            p.opacity = base * opacity;
            let weight = if emphasis > 0.5 { SEMI } else { REG };
            p.text(text, w / 2.0, centre + *slot as f64 * row_h, size, weight, theme::FG, Align::Center, wmax);
        }
        p.scene.pop_layer();
        p.opacity = base;
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
        let tmax = (w - tx - 84.0) as f32;
        let mut app = n.app.clone();
        let group = self.queue.iter().find(|q| q.app == n.app).map(|q| q.group_count).unwrap_or(1);
        if group > 1 {
            app = format!("{app}  ·  {group} new");
        }
        p.text(&app, tx, top - 19.0, 11.0, REG, theme::DIM, Align::Left, tmax);
        p.text(&n.summary, tx, top - 2.0, 14.0, SEMI, theme::FG, Align::Left, tmax);
        let code = super::island::notif_code(n);
        match &code {
            Some(c) => {
                p.text(&island_core::codes::display(c), tx, top + 18.0, 19.0, SEMI, theme::ACCENT, Align::Left, tmax);
                let copied = self.code_copied.is_some_and(|(id, until)| id == n.id && self.now < until);
                p.text(if copied { "Copied" } else { "tap to copy" }, w - 14.0, top + 18.0, 10.0, REG, if copied { theme::ACCENT } else { theme::DIM }, Align::Right, 80.0);
            }
            None => {
                p.text(&n.body, tx, top + 16.0, 12.0, REG, theme::DIM, Align::Left, tmax);
            }
        }
        if let (Some(pr), false) = (n.progress, actions) {
            p.bar(rect(tx, h - 18.0, tmax as f64, 5.0), pr as f64 / 100.0, theme::FG, theme::TRACK);
        }
        // every card has an x that dismisses it (the next waiting one comes up); how many wait shows under it
        let xy = lead_cy - if more > 0 { 6.0 } else { 0.0 };
        p.icon_button(w - 28.0, xy, 28.0, "x", 14.0, Action::Dismiss);
        if !super::island::is_own_notification(n) {
            p.icon_button(w - 58.0, xy, 28.0, "bell-z", 14.0, Action::Snooze);
        }
        if more > 0 {
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
        // a click on the card body goes to the app that sent it
        p.hit(rect(0.0, 0.0, w, h - if actions { 50.0 } else { 0.0 }), code.map_or(Action::NotifOpen, Action::CopyCode));
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

    fn face_bar(&self, p: P, w: f64, h: f64) {
        let Some(b) = &self.bar else { return };
        let field = rect(16.0, 14.0, w - 32.0, 38.0);
        p.rrect(field, 12.0, theme::CHIP);
        p.rrect_stroke(field, 12.0, 1.0, theme::TRACK);
        p.icon("terminal-2", field.x0 + 20.0, field.center().y, 16.0, theme::DIM);
        let tx = field.x0 + 38.0;
        let inner = (field.width() - 50.0) as f32;
        if b.text.is_empty() {
            p.text("10m tea · remind 20m … · 12*7 · 5 km in mi", tx, field.center().y, 13.0, REG, theme::DIM, Align::Left, inner);
        } else {
            p.text(&b.text, tx, field.center().y, 14.0, REG, theme::FG, Align::Left, inner);
        }
        if (self.now / 530) % 2 == 0 {
            let before: String = b.text.chars().take(b.cursor).collect();
            let cx = tx + p.fonts.measure(&before, 14.0, REG).min(inner) as f64;
            p.rrect(rect(cx, field.center().y - 8.0, 1.5, 16.0), 0.75, theme::FG);
        }
        let (line, color) = match (self.bar_cmd(), b.text.trim().is_empty()) {
            (Some(c), _) => (island_core::bar::preview(&c), theme::FG),
            (None, true) => ("Enter runs it · Esc closes".to_string(), theme::DIM),
            (None, false) => ("Not understood".to_string(), theme::DIM),
        };
        p.text(&line, 24.0, h - 20.0, 12.0, SEMI, color, Align::Left, (w - 48.0) as f32);
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
        p.bar(bar, self.osd_shown, theme::FG, theme::TRACK);
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

    /// "25 min", "3 h", "3 h 20 min".
    pub(crate) fn span_text(min: i32) -> String {
        if min < 90 {
            format!("{} min", min.max(1))
        } else {
            let (h, m) = ((min + 5) / 60, (min + 5) % 60 / 10 * 10);
            if m == 0 || h >= 6 { format!("{h} h") } else { format!("{h} h {m} min") }
        }
    }

    /// The rain line, short for the home page, with the time of day for the weather page. None when dry.
    pub(crate) fn rain_text(&self, o: weather::Outlook, long: bool) -> Option<String> {
        use weather::Outlook::*;
        Some(match o {
            Raining { clears_in_min: Some(m) } => format!("Raining · clears in {}", Self::span_text(m)),
            Raining { clears_in_min: None } => "Raining now".to_string(),
            Rain { in_min } if long && in_min >= 90 => {
                let c = clock::civil(self.now + in_min as i64 * 60_000, self.data.utc_offset_secs);
                format!("Rain around {} · in {}", c.hhmm(), Self::span_text(in_min))
            }
            Rain { in_min } => format!("Rain in {}", Self::span_text(in_min)),
            Dry => return None,
        })
    }

    fn face_weather(&self, p: P, w: f64, _h: f64) {
        let hy = self.page_header(p, w, "Weather");
        let Some(wx) = &self.data.weather else {
            p.text("No forecast yet", 14.0, hy + 34.0, 12.0, REG, theme::DIM, Align::Left, 200.0);
            return;
        };
        let top = 14.0 + 34.0 + 6.0;
        p.stagger(1);
        p.icon(weather::glyph(wx.code, wx.day), 14.0 + 20.0, top + 26.0, 36.0, theme::FG);
        p.text(&format!("{}°", wx.temp_c.round() as i32), 14.0 + 50.0, top + 20.0, 34.0, SEMI, theme::FG, Align::Left, 110.0);
        p.text(&format!("{} · feels {}°", weather::describe(wx.code), wx.feels_c.round() as i32), 14.0 + 50.0, top + 50.0, 12.0, REG, theme::DIM, Align::Left, 220.0);
        p.text(&format!("H {}°", wx.high_c.round() as i32), w - 14.0, top + 14.0, 13.0, SEMI, theme::FG, Align::Right, 80.0);
        p.text(&format!("L {}°", wx.low_c.round() as i32), w - 14.0, top + 34.0, 13.0, REG, theme::DIM, Align::Right, 80.0);
        // when it rains next
        p.stagger(2);
        let row = rect(14.0, top + 66.0, w - 28.0, 32.0);
        p.rrect(row, 12.0, theme::CHIP);
        let o = self.rain_outlook().unwrap_or(weather::Outlook::Dry);
        let (icon, text, col) = match self.rain_text(o, true) {
            Some(t) => ("cloud-rain", t, theme::RAIN),
            None => ("sun", "No rain expected in the next 24 hours".to_string(), theme::DIM),
        };
        p.icon(icon, row.x0 + 18.0, row.center().y, 16.0, col);
        p.text(&text, row.x0 + 36.0, row.center().y, 13.0, SEMI, if col == theme::DIM { theme::DIM } else { theme::FG }, Align::Left, (row.width() - 48.0) as f32);
        // today in steps of three hours
        let off = self.data.utc_offset_secs;
        let today = clock::civil(self.now, off);
        let mid = clock::midnight_ms(today.year, today.month, today.day, off);
        let hour_at = |i: i64| wx.hours.iter().find(|h| h.time_ms == mid + i * 3_600_000);
        let now_h = ((self.now - mid) / 3_600_000).clamp(0, 23);
        p.stagger(3);
        let cols_top = row.y1 + 12.0;
        let cw = (w - 28.0) / 8.0;
        for c in 0..8i64 {
            let hr = c * 3;
            let x = 14.0 + c as f64 * cw;
            let current = now_h >= hr && now_h < hr + 3;
            if current {
                p.rrect(rect(x + 2.0, cols_top, cw - 4.0, 66.0), 12.0, theme::CHIP);
            }
            let past = hr + 3 <= now_h;
            let dim = |col: Color| if past { theme::with_alpha(col, 0.45) } else { col };
            let cx = x + cw / 2.0;
            p.text(&format!("{hr:02}"), cx, cols_top + 11.0, 10.0, REG, dim(theme::DIM), Align::Center, 30.0);
            match hour_at(hr + 1).or_else(|| hour_at(hr)) {
                Some(h) => {
                    p.icon(weather::glyph(h.code, h.is_day), cx, cols_top + 33.0, 17.0, dim(theme::FG));
                    p.text(&format!("{}°", h.temp_c.round() as i32), cx, cols_top + 55.0, 12.0, SEMI, dim(theme::FG), Align::Center, 36.0);
                }
                None => {
                    p.text("–", cx, cols_top + 40.0, 12.0, REG, theme::DIM, Align::Center, 20.0);
                }
            }
        }
        // the chance of rain hour by hour
        p.stagger(4);
        let chart_top = cols_top + 66.0 + 14.0;
        p.text("Chance of rain", 14.0 + 4.0, chart_top + 4.0, 10.0, REG, theme::DIM, Align::Left, 120.0);
        let base = chart_top + 14.0 + 34.0;
        let bw = (w - 28.0) / 24.0;
        for i in 0..24i64 {
            let x = 14.0 + i as f64 * bw;
            let frac = hour_at(i).map_or(0.0, |h| (h.rain_prob as f64 / 100.0).max((h.mm as f64 / 2.0).min(1.0)));
            p.rrect(rect(x + 1.5, base - 3.0, bw - 3.0, 3.0), 1.5, theme::TRACK);
            if frac > 0.02 {
                let bh = (frac * 34.0).max(3.0);
                let col = if i < now_h { theme::with_alpha(theme::RAIN, 0.4) } else { theme::RAIN };
                p.rrect(rect(x + 1.5, base - bh, bw - 3.0, bh), 1.5, col);
            }
            if i == now_h {
                p.rrect(rect(x + 1.5, base + 3.0, bw - 3.0, 2.0), 1.0, theme::FG);
            }
        }
        for hr in [0i64, 6, 12, 18] {
            p.text(&format!("{hr:02}"), 14.0 + hr as f64 * bw + bw / 2.0, base + 14.0, 9.0, REG, theme::DIM, Align::Center, 24.0);
        }
    }

    fn face_device(&self, p: P, w: f64, h: f64) {
        let Some((d, low)) = &self.device_ev else { return };
        let tint = if *low { theme::RED } else { theme::FG };
        p.icon(d.kind.icon(), 30.0, h / 2.0, 20.0, tint);
        let label = d.name.clone();
        match d.battery {
            Some(b) => {
                p.text(&label, 50.0, h / 2.0, 13.0, SEMI, if *low { theme::RED } else { theme::FG }, Align::Left, (w - 50.0 - 160.0) as f32);
                p.bar(rect(w - 150.0, h / 2.0 - 3.0, 90.0, 6.0), b as f64 / 100.0, if b <= 20 { theme::RED } else { theme::GREEN }, theme::TRACK);
                p.text(&format!("{b}%"), w - 16.0, h / 2.0, 13.0, SEMI, theme::FG, Align::Right, 44.0);
            }
            None => {
                p.text(&label, 50.0, h / 2.0, 13.0, SEMI, theme::FG, Align::Left, (w - 130.0) as f32);
                p.text("Connected", w - 16.0, h / 2.0, 12.0, REG, theme::DIM, Align::Right, 80.0);
            }
        }
    }

    fn face_progress(&self, p: P, w: f64, h: f64) {
        let Some(op) = &self.ops.op else { return };
        let failed = op.state == OpState::Fail;
        let done = op.state == OpState::Done;
        let tint = if failed { theme::RED } else if done { theme::GREEN } else { op.tint.map_or(theme::ACCENT, theme::rgb) };
        let label = if !op.label.is_empty() { op.label.as_str() } else if failed { "Failed" } else if done { "Done" } else { "Working" };
        // a script's own status line goes beside the title while there is a bar, in the bar's place while there is none
        let bar = op.frac.is_some() || done || op.detail.is_empty();
        let title = if bar && !op.detail.is_empty() { format!("{label} · {}", op.detail) } else { label.to_string() };
        // One 26 px margin on both sides; the icon, the title+bar block and the status share the vertical centre.
        let (pad, col) = (26.0, 52.0);
        let (tx, right) = (pad + 22.0 + 14.0, w - pad - col - 12.0);
        let cy = h / 2.0;
        let icon = op.icon.as_deref().unwrap_or(if failed { "alert-triangle" } else if done { "circle-check" } else { "download" });
        let icon = if failed && op.icon.is_none() { "alert-triangle" } else if done && op.icon.is_none() { "circle-check" } else { icon };
        p.icon(icon, pad + 11.0, cy, 22.0, tint);
        p.text(&title, tx, cy - 10.0, 13.0, SEMI, theme::FG, Align::Left, (w - pad - tx - if bar { col + 12.0 } else { 0.0 }) as f32);
        if bar {
            p.bar(rect(tx, cy + 5.0, right - tx, 6.0), if done { 1.0 } else { op.frac.unwrap_or(0.0) }, tint, theme::TRACK);
        } else {
            p.text(&op.detail, tx, cy + 8.0, 12.0, REG, theme::DIM, Align::Left, (w - pad - tx) as f32);
        }
        let pct = match op.frac {
            _ if failed => "failed".to_string(),
            _ if done => "done".to_string(),
            Some(f) => format!("{}%", (f * 100.0).round() as i32),
            None if bar => "…".into(),
            None => String::new(),
        };
        p.text(&pct, w - pad, cy, 13.0, SEMI, if failed { theme::RED } else if done { theme::GREEN } else { theme::FG }, Align::Right, col as f32);
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
            let rain = self.rain_outlook().filter(|o| weather::worth_showing_at_home(*o)).and_then(|o| self.rain_text(o, false));
            match rain {
                Some(t) => {
                    p.text(&t, x, 62.0, 11.0, SEMI, theme::RAIN, Align::Right, 170.0);
                }
                None => {
                    p.text(&format!("{}° / {}°", wx.high_c.round() as i32, wx.low_c.round() as i32), x, 62.0, 11.0, REG, theme::DIM, Align::Right, 100.0);
                }
            }
            p.hit(rect(x - 170.0, 22.0, 170.0, 50.0), Action::Open(Page::Weather));
        }
        p.icon_button(w - 22.0, 18.0, 24.0, "settings", 14.0, Action::Open(Page::Settings));
        let mut y = 28.0 + 44.0 + 10.0 + 17.0;
        // buttons row
        let mut buttons: Vec<(String, Action)> = vec![];
        for s in self.cfg.timer_presets.iter().take(3) {
            buttons.push((duration::preset_label(*s), Action::TimerStart(*s)));
        }
        buttons.push(("Pomodoro".into(), Action::PomodoroStart));
        buttons.push(("Calendar".into(), Action::Open(Page::Cal)));
        let mut widths: Vec<f64> = buttons.iter().map(|(l, _)| p.button_width(l)).collect();
        let avail = w - 28.0;
        let mut total: f64 = widths.iter().sum::<f64>() + 8.0 * (buttons.len() - 1) as f64;
        if total > avail {
            // a wide font: trim the padding of every chip so the row still fits
            let cut = ((total - avail) / buttons.len() as f64).min(16.0);
            widths.iter_mut().for_each(|x| *x -= cut);
            total = widths.iter().sum::<f64>() + 8.0 * (buttons.len() - 1) as f64;
        }
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
        let _ = y;
    }

    /// The header of a page: a back button and the title; returns the vertical centre of the header.
    fn page_header(&self, p: P, w: f64, title: &str) -> f64 {
        p.stagger(0);
        let cy = 14.0 + 17.0;
        p.icon_button(14.0 + 14.0, cy, 28.0, "arrow-left", 15.0, Action::Back);
        p.text(title, 14.0 + 28.0 + 8.0, cy, 14.0, SEMI, theme::FG, Align::Left, (w - 160.0) as f32);
        cy
    }

    fn face_cal(&self, p: P, w: f64, h: f64) {
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

    fn slider_row(&self, p: P, w: f64, cy: f64, icon: &str, level: Option<(f32, f64)>, action: Action) {
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

    fn face_qs(&self, p: P, w: f64, _h: f64) {
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
    fn metric_row(&self, p: P, w: f64, y: f64, label: &str, frac: Option<f64>, text: &str, color: Color) {
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
            let label = if u.muted { "Unmute" } else { "Mute" };
            let bw = p.button_width(label);
            p.button(rect(w - 14.0 - bw, y - 13.0, bw, 26.0), label, Action::Cmd(Command::MuteMicStream { stream: u.stream, mute: !u.muted }));
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

    fn face_focus_mini(&self, p: P, w: f64, h: f64) {
        let cy = h / 2.0;
        p.icon("target", 24.0, cy, 15.0, theme::ACCENT);
        p.text("Focus", 40.0, cy, 12.0, SEMI, theme::FG, Align::Left, 60.0);
        let mut right = self.pill_clock(p, w - 14.0, cy);
        right = self.status_icons(p, right, cy, 13.0);
        let left = self.focus_minutes_left().map_or("on".to_string(), |m| format!("{m} min"));
        p.text(&left, right, cy, 12.0, REG, theme::DIM, Align::Right, 70.0);
        p.hit(rect(0.0, 0.0, w, h), Action::Open(Page::Focus));
    }

    fn face_focus_full(&self, p: P, w: f64, _h: f64) {
        let label = self.focus_minutes_left().map_or("Focus on".to_string(), |m| format!("{m} min left"));
        let tw = p.fonts.measure(&label, 22.0, SEMI) as f64;
        let x = (w - (22.0 + 10.0 + tw)) / 2.0;
        p.icon("target", x + 11.0, 30.0, 22.0, theme::ACCENT);
        p.text(&label, x + 32.0, 30.0, 22.0, SEMI, theme::FG, Align::Left, (w - 60.0) as f32);
        p.text("Sites blocked · notifications silenced", w / 2.0, 56.0, 11.0, REG, theme::DIM, Align::Center, (w - 28.0) as f32);
        self.button_row(p, &[("End focus", Action::Cmd(Command::StopFocus))], w / 2.0, 88.0);
    }

    /// A text field in place of a row while a setting is edited.
    fn edit_row(&self, p: P, r: Rect, e: &Edit, placeholder: &str) {
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

    fn face_settings(&self, p: P, w: f64, _h: f64) {
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
                p.text("Add a calendar (feed address)", r.x0 + 34.0, y + 15.0, 12.0, REG, theme::DIM, Align::Left, 300.0);
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
        if let Some((t, _)) = &self.note {
            p.text(t, w / 2.0, y + 8.0, 11.0, SEMI, theme::ORANGE, Align::Center, (w - 28.0) as f32);
        }
    }

    fn face_options(&self, p: P, w: f64, _h: f64) {
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
            for (i, (label, value, f, hint)) in [("Timer presets", c.timer_presets.clone(), Field::TimerPresets, "5m, 10m, 25m, 1h30m"), ("Screen", screen, Field::Screen, "A screen name such as eDP-1; empty = the focused one")].into_iter().enumerate() {
                p.stagger(n + i);
                let r = rect(14.0, y + i as f64 * 34.0, w - 28.0, 30.0);
                if p.hovered(r) {
                    help = Some(options::describe(if f == Field::TimerPresets { "timer_presets" } else { "screen" }));
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
                p.text("Rest on an option to see what it does", w / 2.0, hy, 11.0, REG, theme::DIM, Align::Center, (w - 28.0) as f32);
            }
        }
    }

    /// The notification rules: one row each, and two rows to add one.
    fn options_rules(&self, p: P, w: f64, mut y: f64, n: usize) {
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

    fn face_clip(&self, p: P, w: f64, _h: f64) {
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

    /// Where the attention went today, and the last half hour.
    fn face_activity(&self, p: P, w: f64, _h: f64) {
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

    fn attention_tab(&self, p: P, w: f64, top: f64) {
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

    fn rewind_tab(&self, p: P, w: f64, top: f64) {
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

    fn face_send(&self, p: P, w: f64, _h: f64) {
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
    fn trip_row(&self, p: P, w: f64, y: f64) {
        use island_core::travel::{self, Mode};
        let Some(e) = self.trip_event() else { return };
        p.stagger(9);
        let off = self.data.utc_offset_secs;
        for (n, mode) in [Mode::Car, Mode::Walk, Mode::Transit].into_iter().enumerate() {
            let r = rect(14.0 + n as f64 * 32.0, y, 28.0, 28.0);
            let on = mode == self.cfg.travel_mode;
            p.rrect(r, 14.0, if on { theme::ACCENT } else if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
            p.icon(mode.icon(), r.center().x, r.center().y, 15.0, if on { theme::ON_ACCENT } else { theme::FG });
            p.hit(r, Action::TravelMode(mode));
        }
        let x = 14.0 + 3.0 * 32.0 + 8.0;
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

    pub(crate) fn today_height(&self) -> f64 {
        let (items, _, _) = self.today_view();
        14.0 + 34.0 + 6.0 + if items.is_empty() { 40.0 } else { items.len() as f64 * 28.0 + 18.0 } + if self.trip_event().is_some() { 38.0 } else { 0.0 } + 10.0
    }

    fn face_today(&self, p: P, w: f64, _h: f64) {
        let hy = self.page_header(p, w, "Today");
        let (items, now_at, skipped) = self.today_view();
        let off = self.data.utc_offset_secs;
        let all = {
            let t = clock::civil(self.now, off);
            let reminders: Vec<(i64, String)> = self.reminders.iter().map(|r| (r.due_ms, r.text.clone())).collect();
            timeline::build(&self.data.events, &reminders, clock::midnight_ms(t.year, t.month, t.day, off), clock::midnight_ms(t.year, t.month, t.day + 1, off))
        };
        let summary = match timeline::next_up(&all, self.now) {
            Some((i, m)) => format!("{} · {}", i.title, if m < 60 { format!("in {m} min") } else { format!("in {} h {} min", m / 60, m % 60) }),
            None if all.is_empty() => String::new(),
            None => "Nothing more today".to_string(),
        };
        p.text(&summary, w - 14.0, hy, 11.0, REG, theme::DIM, Align::Right, (w - 150.0) as f32);
        let mut y = 14.0 + 34.0 + 6.0;
        if items.is_empty() {
            p.text("Nothing planned today", w / 2.0, y + 20.0, 12.0, REG, theme::DIM, Align::Center, 260.0);
            return;
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
            p.text(&it.title, 86.0, cy, 12.0, if ongoing { SEMI } else { REG }, fg, Align::Left, (w - 86.0 - 40.0) as f32);
            if let Some(u) = &it.url {
                p.icon("external-link", w - 26.0, cy, 13.0, dim);
                p.hit(row, Action::Join(u.clone()));
            }
            y += 28.0;
        }
        if now_at >= items.len() {
            line(p, y);
            y += 18.0;
        }
        let _ = skipped;
        self.trip_row(p, w, y + 6.0);
    }

    /// Rows of each section of the audio page.
    fn audio_sections(&self) -> (usize, usize, usize) {
        (self.data.sinks.len().min(4), self.data.sources.len().min(3), self.data.streams.len().min(4))
    }

    pub(crate) fn audio_height(&self) -> f64 {
        let (o, i, a) = self.audio_sections();
        let sections = [o, i, a].iter().filter(|n| **n > 0).count().max(1) as f64;
        14.0 + 34.0 + sections * 22.0 + (o + i + a).max(1) as f64 * 34.0 + 10.0
    }

    fn face_audio(&self, p: P, w: f64, _h: f64) {
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

    fn face_notifs(&self, p: P, w: f64, _h: f64) {
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
        for (i, (n, at)) in self.history.iter().take(5).enumerate() {
            p.stagger(1 + i);
            let row = rect(14.0, y, w - 28.0, 40.0);
            p.rrect(row, 12.0, if p.hovered(row) { theme::HOT_CHIP } else { theme::CHIP });
            match n.image.as_ref().or(n.app_icon.as_ref()) {
                Some(img) => draw_image(p, img, row.x0 + 10.0, y + 6.0, 28.0, if n.image.is_some() { 14.0 } else { 7.0 }),
                None => p.icon("bell", row.x0 + 24.0, y + 20.0, 18.0, theme::DIM),
            }
            let tx = row.x0 + 48.0;
            let tmax = (row.width() - 48.0 - 66.0) as f32;
            p.text(&format!("{}  ·  {}", n.app, ago(self.now - at)), tx, y + 12.0, 10.0, REG, theme::DIM, Align::Left, tmax);
            p.text(if n.summary.is_empty() { &n.body } else { &n.summary }, tx, y + 27.0, 12.0, SEMI, theme::FG, Align::Left, tmax);
            p.icon_button(row.x1 - 18.0, y + 20.0, 26.0, "x", 13.0, Action::HistoryRemove(i));
            if !super::island::is_own_notification(n) {
                p.icon_button(row.x1 - 44.0, y + 20.0, 26.0, "bell-off", 12.0, Action::MuteApp(i));
            }
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
            Side::Focus => ("target", self.focus_minutes_left().map_or("on".to_string(), |m| format!("{m}m"))),
            Side::Media => ("music", String::new()),
            Side::Op => match &self.ops.op {
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
        let tint = match (&side, self.ops.op.as_ref().map(|o| (o.state, o.tint))) {
            (Side::Op, Some((OpState::Done, _))) => theme::GREEN,
            (Side::Op, Some((OpState::Fail, _))) => theme::RED,
            (Side::Op, Some((OpState::Run, Some(t)))) => theme::rgb(t),
            _ => theme::ACCENT,
        };
        let tw = (p.fonts.measure(&text, 12.0, SEMI) as f64).min(50.0);
        let group = 13.0 + 4.0 + tw;
        let x0 = (w - group) / 2.0;
        p.icon(icon, x0 + 6.5, h / 2.0, 13.0, tint);
        p.text(&text, x0 + 17.0, h / 2.0, 12.0, SEMI, if tint == theme::ACCENT { theme::FG } else { tint }, Align::Left, 50.0);
    }
}

fn page_icon(p: Page) -> &'static str {
    match p {
        Page::Home => "home",
        Page::Media => "music",
        Page::Timer => "hourglass",
        Page::Focus => "target",
        Page::Meeting => "calendar-event",
        Page::Op => "download",
        Page::Cal => "calendar",
        Page::Qs => "adjustments-horizontal",
        Page::Sys => "cpu",
        Page::Notifs => "bell",
        Page::Clip => "clipboard-text",
        Page::Settings => "settings",
        Page::Weather => "cloud-sun",
        Page::Options => "toggle-right",
        Page::Audio => "volume",
        Page::Today => "clock",
        Page::Send => "send",
        Page::Activity => "chart-donut",
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

/// "Synced just now", "Synced 3 min ago", "Synced 2 h ago".
fn sync_text(delta_ms: i64) -> String {
    match ago(delta_ms).as_str() {
        "now" => "Synced just now".to_string(),
        a => format!("Synced {a} ago"),
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
