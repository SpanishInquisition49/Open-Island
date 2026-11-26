//! The notification card under the pill, laid out as drip's capsule: the icon at the left, the title over the detail,
//! and at the right a ring that drains with the time left, or the card's buttons. With the pointer on it the icon
//! becomes a close button and the ring becomes snooze (on the island's own cards, an arrow: a click opens them).

use super::*;
use crate::island::card::{Card, CARD_MAX_W};
use island_core::droplet::{self, Animator, Shape, Size, LINE_STAGGER};
use vello::kurbo::{Arc as KArc, BezPath, Point, Stroke, Vec2};

const PAD: f64 = 14.0;
const ICON: f64 = 24.0;
const SPACE: f64 = 10.0;
const RING: f64 = 20.0;
const MIN_W: f64 = 200.0;
const TEXT_X: f64 = PAD + ICON + SPACE;
const TITLE_SIZE: f32 = 14.0;
const TITLE_LINE: f64 = 17.0;
const DETAIL_SIZE: f32 = 12.0;
const DETAIL_LINE: f64 = 15.0;
const CODE_SIZE: f32 = 19.0;
const CODE_LINE: f64 = 23.0;
const APP_SIZE: f32 = 11.0;
const SUB_GAP: f64 = 3.0;
const BUTTON_H: f64 = 26.0;
const BUTTON_GAP: f64 = 6.0;
/// The room snooze takes beside a card's buttons (without buttons it takes the ring's place).
const SNOOZE_SLOT: f64 = 32.0;
const BUTTON_SIZE: f32 = 12.5;
const BLUE: Color = Color::from_rgba8(0x0a, 0x84, 0xff, 255);
const GREY: Color = Color::from_rgba8(0x9e, 0x9e, 0x9e, 255);

/// What a card says and where.
struct Layout {
    title: Vec<String>,
    /// The sender's name (and how many are new), and whether it fits after the last line of the title.
    app: Option<(String, bool)>,
    detail: Vec<String>,
    code: Option<String>,
    buttons: Vec<(String, Action, f64)>,
    text_w: f64,
    text_h: f64,
    size: Size,
}

/// `text` broken into at most `max_lines` lines no wider than `max_w`; the last one ends in an ellipsis when it does not
/// all fit.
fn wrap(fonts: &Fonts, text: &str, size: f32, weight: Weight, max_w: f32, max_lines: usize) -> Vec<String> {
    let text: String = text.chars().take(600).collect();
    let mut words: std::collections::VecDeque<String> = text.split_whitespace().map(str::to_string).collect();
    let mut lines = Vec::new();
    while !words.is_empty() && lines.len() < max_lines {
        if lines.len() + 1 == max_lines {
            let rest = words.drain(..).collect::<Vec<_>>().join(" ");
            lines.push(fonts.ellipsize(&rest, size, weight, max_w));
            break;
        }
        let mut line = String::new();
        while let Some(word) = words.front() {
            let longer = if line.is_empty() { word.clone() } else { format!("{line} {word}") };
            if fonts.measure(&longer, size, weight) <= max_w {
                line = longer;
                words.pop_front();
                continue;
            }
            if line.is_empty() {
                // a word wider than the column is cut where it reaches the edge
                let word = words.pop_front().unwrap_or_default();
                let mut cut = word.chars().count().saturating_sub(1).max(1);
                while cut > 1 && fonts.measure(&word.chars().take(cut).collect::<String>(), size, weight) > max_w {
                    cut -= 1;
                }
                line = word.chars().take(cut).collect();
                let rest: String = word.chars().skip(cut).collect();
                if !rest.is_empty() {
                    words.push_front(rest);
                }
            }
            break;
        }
        lines.push(line);
    }
    lines
}

fn layout(fonts: &Fonts, c: &Card) -> Layout {
    let n = &c.n;
    let buttons: Vec<(String, Action, f64)> = n
        .actions
        .iter()
        .filter(|a| a.key != "default")
        .take(3)
        .map(|a| {
            let label = if a.key == "inline-reply" { "Reply".to_string() } else { a.label.clone() };
            let label = fonts.ellipsize(&label, BUTTON_SIZE, SEMI, 110.0);
            let w = (fonts.measure(&label, BUTTON_SIZE, SEMI) as f64).ceil() + 24.0;
            (label, Action::NotifAction(a.key.clone()), w)
        })
        .collect();
    let snooze = if crate::island::is_own_notification(n) { 0.0 } else { SNOOZE_SLOT };
    let right = if buttons.is_empty() { RING } else { buttons.iter().map(|b| b.2).sum::<f64>() + BUTTON_GAP * (buttons.len() - 1) as f64 + snooze };
    let column = (CARD_MAX_W - 140.0).min(CARD_MAX_W - TEXT_X - SPACE - right - PAD).max(80.0) as f32;

    let own = n.app == "open-island";
    let headline = if n.summary.trim().is_empty() { n.app.as_str() } else { n.summary.as_str() };
    let title = wrap(fonts, headline, TITLE_SIZE, SEMI, column, 2);
    let mut sender = if own || n.summary.trim().is_empty() { String::new() } else { n.app.clone() };
    if c.group > 1 {
        sender = if sender.is_empty() { format!("{} new", c.group) } else { format!("{sender}  ·  {} new", c.group) };
    }
    let measure = |s: &str, size: f32, w: Weight| fonts.measure(s, size, w) as f64;
    let mut text_w = title.iter().map(|l| measure(l, TITLE_SIZE, SEMI)).fold(0.0, f64::max);
    let app = (!sender.is_empty()).then(|| {
        let sender = fonts.ellipsize(&sender, APP_SIZE, REG, column);
        let after = title.last().map_or(0.0, |l| measure(l, TITLE_SIZE, SEMI) + 8.0) + measure(&sender, APP_SIZE, REG);
        let inline = after <= column as f64;
        text_w = text_w.max(if inline { after } else { measure(&sender, APP_SIZE, REG) });
        (sender, inline)
    });

    let code = crate::island::notif_code(n);
    let detail = if code.is_some() { Vec::new() } else { wrap(fonts, &n.body, DETAIL_SIZE, REG, column, 3) };
    text_w = detail.iter().map(|l| measure(l, DETAIL_SIZE, REG)).fold(text_w, f64::max);
    if let Some(code) = &code {
        text_w = text_w.max(measure(&island_core::codes::display(code), CODE_SIZE, SEMI) + 8.0 + measure("tap to copy", 10.0, REG));
    }
    let own_app_line = app.as_ref().is_some_and(|(_, inline)| !inline);
    let mut text_h = title.len() as f64 * TITLE_LINE + if own_app_line { DETAIL_LINE } else { 0.0 };
    if code.is_some() {
        text_h += SUB_GAP + CODE_LINE;
    } else if !detail.is_empty() {
        text_h += SUB_GAP + detail.len() as f64 * DETAIL_LINE;
    }
    if n.progress.is_some() {
        text_w = text_w.max(160.0);
        text_h += 10.0;
    }
    let text_w = text_w.min(column as f64).ceil();
    let w = (TEXT_X + text_w + SPACE + right + PAD).clamp(MIN_W, CARD_MAX_W);
    let h = 2.0 * 12.0 + text_h.max(ICON);
    // one title line over one detail line stays round at the ends; more lines keep corners
    let tall = title.len() > 1 || detail.len() > 1 || (own_app_line && !detail.is_empty());
    let size = Size { w, h, corner: if tall { 30.0 } else { f64::INFINITY } };
    Layout { title, app, detail, code, buttons, text_w, text_h, size }
}

/// An image faded with the content, in a rounded square.
fn faded_image(p: P, img: &Image, x: f64, y: f64, size: f64, radius: f64) {
    p.scene.push_clip_layer(Fill::NonZero, p.transform, &RoundedRect::new(x, y, x + size, y + size, radius));
    if p.opacity >= 0.999 {
        draw_image(p, img, x, y, size, radius);
    } else if p.opacity > 0.0 {
        use vello::peniko::{Blob, ImageAlphaType, ImageBrush, ImageData, ImageFormat};
        let data = ImageData { data: Blob::new(img.rgba.clone() as std::sync::Arc<dyn AsRef<[u8]> + Send + Sync>), format: ImageFormat::Rgba8, alpha_type: ImageAlphaType::Alpha, width: img.width, height: img.height };
        let s = size / img.width.min(img.height).max(1) as f64;
        let (ox, oy) = (x - (img.width as f64 * s - size) / 2.0, y - (img.height as f64 * s - size) / 2.0);
        let brush = ImageBrush::new(data).with_alpha(p.opacity);
        p.scene.draw_image(&brush, p.transform * Affine::translate((ox, oy)) * Affine::scale(s));
    }
    p.scene.pop_layer();
}

fn stroke(p: P, shape: &impl vello::kurbo::Shape, width: f64, color: Color) {
    let color = with_alpha(color, p.opacity);
    p.scene.stroke(&Stroke::new(width), p.transform, color, None, shape);
}

impl Island {
    /// Brings the cards' motion up to date: it needs the fonts, so it happens as they are drawn.
    pub(super) fn measure_cards(&mut self, fonts: &Fonts) {
        let (now, pill_h) = (self.now, self.spring.h.pos);
        let Some(c) = self.cards.live.as_mut() else { return };
        if now < c.born {
            return;
        }
        let size = layout(fonts, c).size;
        let age = c.age(now);
        match c.anim.as_mut() {
            Some(a) => a.resize(size, age),
            None => c.anim = Some(Animator::enter(size, pill_h)),
        }
    }

    /// The black of the cards and of the neck that joins a drop to the pill. Drawn before the pill, which covers its end.
    pub(super) fn draw_card_shapes(&self, scene: &mut Scene, base: Affine, w: f64, h: f64, radius: f64) {
        for c in self.cards.gone.iter().chain(&self.cards.live) {
            let Some(a) = c.anim.as_ref().filter(|_| self.now >= c.born) else { continue };
            let s = a.shape(c.age(self.now));
            let t = base * Affine::translate((w / 2.0, h));
            match droplet::neck(w, h, radius, &s, 0.7) {
                Some(loops) => {
                    let mut path = BezPath::new();
                    for l in loops {
                        for (i, (x, y)) in l.into_iter().enumerate() {
                            if i == 0 {
                                path.move_to((x, y));
                            } else {
                                path.line_to((x, y));
                            }
                        }
                        path.close_path();
                    }
                    scene.fill(Fill::NonZero, t, theme::BG, None, &path);
                }
                None => scene.fill(Fill::NonZero, t, theme::BG, None, &RoundedRect::new(-s.w / 2.0, s.top, s.w / 2.0, s.top + s.h, s.radius)),
            }
        }
    }

    /// What the cards say; only the one on screen takes the pointer.
    pub(super) fn draw_card_contents(&mut self, scene: &mut Scene, fonts: &Fonts, hits: &mut Vec<Hit<Action>>, base: Affine, w: f64, h: f64) {
        let pointer = self.pointer_local();
        self.cards.rect = None;
        for (live, c) in self.cards.gone.iter().map(|c| (false, c)).chain(self.cards.live.iter().map(|c| (true, c))) {
            let Some(a) = c.anim.as_ref().filter(|_| self.now >= c.born) else { continue };
            let age = c.age(self.now);
            let s = a.shape(age);
            let l = layout(fonts, c);
            let body = rect(w / 2.0 - s.w / 2.0, h + s.top, s.w, s.h);
            let mut unused = Vec::new();
            let sink = if live && s.opened > 0.5 { &mut *hits } else { &mut unused };
            scene.push_clip_layer(Fill::NonZero, base, &RoundedRect::from_rect(body, s.radius));
            {
                let mut p = Painter { scene, fonts, hits: sink, opacity: 0.0, pointer: if live { pointer } else { None }, transform: base, base: (1.0, base), since_enter: 10_000 };
                self.card_content(&mut p, c, a, &s, &l, age, body, live);
            }
            scene.pop_layer();
            if live {
                // where it takes the pointer: the capsule as it is drawn
                self.cards.rect = Some((self.pill_origin.0 + body.x0, self.pill_origin.1 + body.y0, body.width(), body.height(), s.radius));
                // a small drop under it counts the ones that wait
                let more = self.queue.len();
                let k = a.content_opacity(age, 0.1, 0.25);
                if more > 0 && k > 0.0 {
                    let (cx, cy, r) = (w / 2.0, body.y1 + 8.0 + 11.0, 11.0 * (0.5 + 0.5 * k));
                    scene.fill(Fill::NonZero, base, theme::BG, None, &vello::kurbo::Circle::new((cx, cy), r));
                    let mut p = Painter { scene, fonts, hits: &mut unused, opacity: k as f32, pointer: None, transform: base, base: (1.0, base), since_enter: 10_000 };
                    p.text(&format!("+{more}"), cx, cy, 10.0, SEMI, theme::FG, Align::Center, 22.0);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn card_content(&self, p: P, c: &Card, a: &Animator, s: &Shape, l: &Layout, age: f64, body: Rect, live: bool) {
        let n = &c.n;
        // words replaced in place fade in again
        let fresh = ((self.now - c.changed_at) as f64 / 180.0).clamp(0.0, 1.0);
        let at = |delay: f64, over: f64| (a.content_opacity(age, delay, over) * fresh) as f32;
        let cx = body.center().x;
        let (left, right, cy) = (cx - s.content_w / 2.0, cx + s.content_w / 2.0, body.center().y);
        let accent = if n.critical { theme::RED } else { BLUE };
        let controls = live && (p.hovered(body) || self.kb.is_some());
        // a click on the card goes to the app that sent it, or copies the code it carries
        p.hit(body, l.code.clone().map_or(Action::NotifOpen, Action::CopyCode));

        // the icon
        p.opacity = at(0.0, 0.18);
        let icon_cx = left + PAD + ICON / 2.0;
        match (&n.image, &n.app_icon) {
            (Some(img), _) => faded_image(p, img, icon_cx - ICON / 2.0, cy - ICON / 2.0, ICON, ICON / 2.0),
            (None, Some(img)) => faded_image(p, img, icon_cx - ICON / 2.0, cy - ICON / 2.0, ICON, 6.0),
            (None, None) => {
                p.circle(icon_cx, cy, ICON / 2.0, accent);
                p.icon(if n.critical { "alert-triangle" } else { "bell" }, icon_cx, cy, 14.0, theme::FG);
            }
        }
        let close = rect(icon_cx - 15.0, cy - 15.0, 30.0, 30.0);
        if controls {
            p.circle(icon_cx, cy, ICON / 2.0 + 1.0, theme::BG);
            let shade = if p.hovered(close) { 0x5c } else { 0x3d };
            p.circle(icon_cx, cy, ICON / 2.0, Color::from_rgba8(shade, shade, shade, 255));
            p.icon("x", icon_cx, cy, 12.0, theme::FG);
        }
        p.hit(close, Action::Dismiss);

        // the words
        let tx = left + TEXT_X;
        let tmax = l.text_w as f32 + 1.0;
        let mut y = cy - l.text_h / 2.0;
        for (i, line) in l.title.iter().enumerate() {
            p.opacity = at(i as f64 * LINE_STAGGER, 0.25);
            let drawn = p.text(line, tx, y + TITLE_LINE / 2.0, TITLE_SIZE, SEMI, theme::FG, Align::Left, tmax);
            if let Some((app, true)) = l.app.as_ref().filter(|_| i + 1 == l.title.len()) {
                p.text(app, tx + drawn as f64 + 8.0, y + TITLE_LINE / 2.0 + 1.0, APP_SIZE, REG, GREY, Align::Left, tmax);
            }
            y += TITLE_LINE;
        }
        let first = 0.04 + l.title.len().saturating_sub(1) as f64 * LINE_STAGGER;
        if let Some((app, false)) = &l.app {
            p.opacity = at(first, 0.25);
            p.text(app, tx, y + DETAIL_LINE / 2.0, APP_SIZE, REG, GREY, Align::Left, tmax);
            y += DETAIL_LINE;
        }
        if let Some(code) = &l.code {
            y += SUB_GAP;
            p.opacity = at(first, 0.25);
            let drawn = p.text(&island_core::codes::display(code), tx, y + CODE_LINE / 2.0, CODE_SIZE, SEMI, theme::FG, Align::Left, tmax);
            let copied = self.code_copied.is_some_and(|(id, until)| id == n.id && self.now < until);
            p.text(if copied { "Copied" } else { "tap to copy" }, tx + drawn as f64 + 8.0, y + CODE_LINE / 2.0 + 2.0, 10.0, REG, if copied { theme::GREEN } else { GREY }, Align::Left, tmax);
            y += CODE_LINE;
        } else if !l.detail.is_empty() {
            y += SUB_GAP;
            for (i, line) in l.detail.iter().enumerate() {
                p.opacity = at(first + i as f64 * LINE_STAGGER, 0.25);
                p.text(line, tx, y + DETAIL_LINE / 2.0, DETAIL_SIZE, REG, GREY, Align::Left, tmax);
                y += DETAIL_LINE;
            }
        }
        if let Some(progress) = n.progress {
            p.opacity = at(first, 0.25);
            p.bar(rect(tx, y + 5.0, l.text_w, 5.0), progress as f64 / 100.0, accent, theme::TRACK);
        }

        // the right end: the buttons, or the time left
        p.opacity = at(0.0, 0.18);
        let snoozes = !crate::island::is_own_notification(n);
        let mut end = right - PAD;
        if l.buttons.is_empty() {
            let ring = Point::new(end - RING / 2.0, cy);
            if snoozes {
                // snooze takes the ring's place under the pointer
                let r = rect(ring.x - 13.0, cy - 13.0, 26.0, 26.0);
                if controls {
                    p.circle(ring.x, cy, 13.0, if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
                    p.icon("bell-z", ring.x, cy, 14.0, theme::FG);
                } else {
                    self.card_ring(p, ring, accent, live);
                }
                p.hit(r, Action::Snooze);
            } else if controls && l.code.is_none() {
                // an arrow up and to the right: a click opens what the card is about
                p.circle(ring.x, ring.y, RING / 2.0 + 1.0, theme::CHIP);
                let mut arrow = BezPath::new();
                arrow.move_to(ring + Vec2::new(-3.5, 3.5));
                arrow.line_to(ring + Vec2::new(3.5, -3.5));
                arrow.move_to(ring + Vec2::new(-1.5, -3.5));
                arrow.line_to(ring + Vec2::new(3.5, -3.5));
                arrow.line_to(ring + Vec2::new(3.5, 1.5));
                stroke(p, &arrow, 1.8, theme::FG);
            } else {
                self.card_ring(p, ring, accent, live);
            }
        } else {
            for (i, (label, action, bw)) in l.buttons.iter().enumerate().rev() {
                end -= bw;
                let r = rect(end, cy - BUTTON_H / 2.0, *bw, BUTTON_H);
                let hot = p.hovered(r);
                let fill = match (i, hot) {
                    (0, false) => BLUE,
                    (0, true) => Color::from_rgba8(0x27, 0x93, 0xff, 255),
                    (_, false) => with_alpha(theme::FG, 0.15),
                    (_, true) => with_alpha(theme::FG, 0.25),
                };
                p.rrect(r, BUTTON_H / 2.0, fill);
                p.text(label, r.center().x, cy, BUTTON_SIZE, SEMI, theme::FG, Align::Center, *bw as f32);
                p.hit(r, action.clone());
                end -= if i > 0 { BUTTON_GAP } else { 0.0 };
            }
            if snoozes {
                let sx = end - SNOOZE_SLOT + 13.0;
                let r = rect(sx - 13.0, cy - 13.0, 26.0, 26.0);
                if controls {
                    p.circle(sx, cy, 13.0, if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
                    p.icon("bell-z", sx, cy, 14.0, theme::FG);
                }
                p.hit(r, Action::Snooze);
            }
        }
    }

    /// The countdown: a faint circle and over it the time left, from twelve o'clock, draining clockwise.
    fn card_ring(&self, p: P, centre: Point, accent: Color, live: bool) {
        let r = RING / 2.0 - 1.4;
        p.circle_stroke(centre.x, centre.y, r, 2.4, with_alpha(accent, 0.28));
        let left_over = if live { self.card_time_left() } else { 0.0 };
        if left_over > 0.001 {
            use std::f64::consts::{FRAC_PI_2, TAU};
            let arc = KArc::new(centre, Vec2::new(r, r), -FRAC_PI_2 + (1.0 - left_over) * TAU, left_over * TAU, 0.0);
            stroke(p, &arc, 2.4, accent);
        }
    }
}
