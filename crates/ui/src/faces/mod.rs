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
        let slot = self.side_slot(self.shown_side.is_some(), self.shown_more.len()) * self.bubble_k;
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
        for (n, side) in self.sides_shown().into_iter().enumerate().filter(|_| self.bubble_k > 0.01) {
            let kk = self.bubble_k;
            let bx = w + BUBBLE_GAP + n as f64 * (BUBBLE_W + BUBBLE_GAP);
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
        let mut pages = self.dock_pages();
        pages.push(Page::Grid);
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
            Face::Phone => self.face_phone(p, w, h),
            Face::Grid => self.face_grid(p, w, h),
            Face::Options => self.face_options(p, w, h),
            Face::Audio => self.face_audio(p, w, h),
            Face::Today => self.face_today(p, w, h),
            Face::Send => self.face_send(p, w, h),
            Face::Activity => self.face_activity(p, w, h),
            Face::Map => self.face_map(p, w, h),
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
        // the countdowns running beside this one
        y += 34.0;
        for (n, e) in self.extra_timers.iter().enumerate().take(3) {
            let cy = y + 14.0;
            p.icon("hourglass", 24.0, cy, 12.0, theme::DIM);
            p.text(if e.label.is_empty() { "Timer" } else { &e.label }, 42.0, cy, 12.0, REG, theme::FG, Align::Left, (w - 42.0 - 110.0) as f32);
            p.text(&duration::fmt_remaining(e.end_ms.map_or(0, |x| (x - self.now).max(0))), w - 48.0, cy, 12.0, REG, theme::DIM, Align::Right, 70.0);
            let x = rect(w - 36.0, cy - 12.0, 24.0, 24.0);
            p.icon("x", w - 24.0, cy, 12.0, theme::DIM);
            p.hit(x, Action::TimerCancelExtra(n));
            y += 28.0;
        }
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
        // new messages of this conversation that joined the card, or others of the same app waiting
        let group = self.notif_group.max(self.queue.iter().find(|q| q.app == n.app).map(|q| q.group_count).unwrap_or(1));
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
            // the examples change every few seconds: what the bar can do, a set at a time
            let hint = island_core::bar::HINTS[(self.now / 4000) as usize % island_core::bar::HINTS.len()];
            p.text(hint, tx, field.center().y, 13.0, REG, theme::DIM, Align::Left, inner);
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
            (None, true) => ("Enter runs it · Up recalls · Esc closes".to_string(), theme::DIM),
            (None, false) => (island_core::bar::usage(&b.text).map_or("Not understood".to_string(), str::to_string), theme::DIM),
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
}

mod pages_a;
mod pages_b;
mod grid;
mod phone;


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
        Page::Map => "map-pin",
        Page::Phone => "device-mobile",
        Page::Grid => "layout-grid",
    }
}

fn side_to_page(s: Side) -> Page {
    match s {
        Side::Timer => Page::Timer,
        Side::Meeting => Page::Meeting,
        Side::Focus => Page::Focus,
        Side::Media => Page::Media,
        Side::Op | Side::OpAt(_) => Page::Op,
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
/// A map tile (256 logical pixels square) at a place in the page; the caller clips.
fn draw_tile(p: P, img: &Image, x: f64, y: f64, size: f64) {
    use vello::peniko::{Blob, ImageAlphaType, ImageBrush, ImageData, ImageFormat};
    let data = ImageData {
        data: Blob::new(img.rgba.clone() as std::sync::Arc<dyn AsRef<[u8]> + Send + Sync>),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width: img.width,
        height: img.height,
    };
    let s = size / img.width.max(1) as f64;
    let t = p.transform;
    p.scene.draw_image(&ImageBrush::new(data), t * Affine::translate((x, y)) * Affine::scale(s));
}

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
