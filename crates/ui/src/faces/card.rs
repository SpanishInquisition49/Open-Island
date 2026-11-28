//! The notification card under the pill, laid out as drip's capsule: the icon at the left, the title over the detail,
//! and at the right a ring that drains with the time left, a pinging dot on a card that waits, or the card's buttons.
//! With the pointer on it the icon becomes a close button and the ring becomes snooze (on the island's own cards and a
//! script's, an arrow: a click opens them).

use super::*;
use crate::island::card::{Card, CARD_MAX_W};
use island_core::droplet::{self, Animator, Blob, Shape, Size, GAP, LINE_STAGGER, STACK_GAP};
use island_core::notify::{button_fill, Kind};
use vello::kurbo::{Arc as KArc, BezPath, Circle, Point, Stroke, Vec2};

const PAD: f64 = 14.0;
const ICON: f64 = 24.0;
/// A device's battery: the room its ring takes round the icon, the ring, the room for the number at the right, and how
/// long after the words appear it takes to fill.
const RING_ICON: f64 = 36.0;
const RING_R: f64 = 16.0;
const PERCENT_W: f64 = 40.0;
const FILL_S: f64 = 0.9;
/// The liquid of the machine's battery: how long it takes to pour, and until its surface is flat.
const POUR_S: f64 = 1.3;
const SETTLE_S: f64 = 1.9;
/// A digit of a one-time code rolls in: each starts this long after the one before, takes this long, and passes this
/// many others on its way.
const ROLL_STAGGER: f64 = 0.07;
const ROLL_S: f64 = 0.45;
const ROLL_PASSED: u32 = 3;
const SPACE: f64 = 10.0;
const RING: f64 = 20.0;
const MIN_W: f64 = 200.0;
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
/// The dot of a card that waits: its radius, and its ping (drip's: every 2.4 s it grows to 2.1 times its size as it
/// fades from 0.4, over the first three quarters).
const DOT_R: f64 = 4.0;
const PING_MS: i64 = 2400;
const PING_SCALE: f64 = 2.1;
const PING_OPACITY: f32 = 0.4;
const PING_SPREAD: f64 = 0.75;
const GREY: Color = Color::from_rgba8(0x9e, 0x9e, 0x9e, 255);

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgba8(r, g, b, 255)
}

/// A card's colour: its own, else its kind's, else red when urgent and blue otherwise.
fn accent_of(n: &crate::model::Incoming) -> (u8, u8, u8) {
    n.tint.or(n.kind.map(Kind::color)).unwrap_or(if n.critical { Kind::Fail.color() } else { Kind::Info.color() })
}

/// Snooze is for what other programs send: not for the island's own cards, nor for those a script asks for.
fn snoozes(n: &crate::model::Incoming) -> bool {
    !crate::island::is_own_notification(n) && !crate::island::is_script(n)
}

/// The mark of a kind: a tick, a cross, an exclamation or an i, `size` high.
fn kind_mark(p: P, kind: Kind, x: f64, y: f64, size: f64, color: Color) {
    match kind {
        Kind::Ok => p.icon("check", x, y, size as f32, color),
        Kind::Fail => p.icon("x", x, y, size as f32, color),
        Kind::Warn => {
            p.text("!", x, y, size as f32, SEMI, color, Align::Center, size as f32);
        }
        Kind::Info => {
            p.text("i", x, y, size as f32, SEMI, color, Align::Center, size as f32);
        }
    }
}

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
    /// The room of the icon, and where the words start.
    icon_w: f64,
    text_x: f64,
    size: Size,
}

/// The most lines of text a card shown whole can take: more than any day needs, less than a screen.
const FULL_LINES: usize = 32;

/// `text` broken into at most `max_lines` lines no wider than `max_w`; the last one ends in an ellipsis when it does not
/// all fit.
pub(crate) fn wrap(fonts: &Fonts, text: &str, size: f32, weight: Weight, max_w: f32, max_lines: usize) -> Vec<String> {
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
        // the island's own cards may offer one more (a screenshot's)
        .take(if crate::island::is_own_notification(n) { 4 } else { 3 })
        .map(|a| {
            let label = if a.key == "inline-reply" { "Reply".to_string() } else { a.label.clone() };
            let label = fonts.ellipsize(&label, BUTTON_SIZE, SEMI, 110.0);
            let w = (fonts.measure(&label, BUTTON_SIZE, SEMI) as f64).ceil() + 24.0;
            (label, Action::NotifAction(a.key.clone()), w)
        })
        .collect();
    let snooze = if snoozes(n) { SNOOZE_SLOT } else { 0.0 };
    let icon_w = if n.ring.is_some() { RING_ICON } else { ICON };
    let text_x = PAD + icon_w + SPACE;
    let right = if n.ring.is_some() { PERCENT_W } else if buttons.is_empty() { RING } else { buttons.iter().map(|b| b.2).sum::<f64>() + BUTTON_GAP * (buttons.len() - 1) as f64 + snooze };
    let column = (CARD_MAX_W - 140.0).min(CARD_MAX_W - text_x - SPACE - right - PAD).max(80.0) as f32;

    let own = n.app == "open-island" || crate::island::is_script(n);
    let headline = if n.summary.trim().is_empty() { n.app.as_str() } else { n.summary.as_str() };
    let title = wrap(fonts, headline, TITLE_SIZE, SEMI, column, if n.full { 4 } else { 2 });
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
    let detail = if code.is_some() {
        Vec::new()
    } else if n.full {
        n.body.lines().fold(Vec::new(), |mut all, line| {
            all.extend(wrap(fonts, line, DETAIL_SIZE, REG, column, FULL_LINES.saturating_sub(all.len())));
            all
        })
    } else {
        wrap(fonts, &n.body, DETAIL_SIZE, REG, column, 3)
    };
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
    let w = (text_x + text_w + SPACE + right + PAD).clamp(MIN_W, CARD_MAX_W);
    let h = 2.0 * 12.0 + text_h.max(icon_w);
    // one title line over one detail line stays round at the ends; more lines keep corners
    let tall = title.len() > 1 || detail.len() > 1 || (own_app_line && !detail.is_empty());
    let size = Size { w, h, corner: if tall { 30.0 } else { f64::INFINITY } };
    Layout { title, app, detail, code, buttons, text_w, text_h, icon_w, text_x, size }
}

/// The code at (`x`, the middle of its line), its characters rolling into place one after another, `up` seconds after
/// they started. Returns the width drawn.
fn rolling_code(p: P, text: &str, x: f64, cy: f64, up: f64) -> f32 {
    let mut before = String::new();
    p.scene.push_clip_layer(Fill::NonZero, p.transform, &Rect::new(x - 2.0, cy - CODE_LINE / 2.0, x + 400.0, cy + CODE_LINE / 2.0));
    for (n, ch) in text.chars().enumerate() {
        let left = x + p.fonts.measure(&before, CODE_SIZE, SEMI) as f64;
        before.push(ch);
        let t = ((up - n as f64 * ROLL_STAGGER) / ROLL_S).clamp(0.0, 1.0);
        let to_go = (1.0 - t).powi(3) * f64::from(ROLL_PASSED);
        // the character itself, and those of its sort it passes on the way (a space just waits)
        for k in 0..=ROLL_PASSED {
            let y = cy + (to_go - f64::from(k)) * CODE_LINE;
            if (y - cy).abs() >= CODE_LINE || (ch.is_whitespace() && k > 0) || (t <= 0.0) {
                continue;
            }
            p.text(&passed(ch, k, n).to_string(), left, y, CODE_SIZE, SEMI, theme::FG, Align::Left, 40.0);
        }
    }
    p.scene.pop_layer();
    p.fonts.measure(text, CODE_SIZE, SEMI)
}

/// What shows `k` places before the `n`-th character `ch` as it rolls in: `ch` itself at 0, else another of its sort.
fn passed(ch: char, k: u32, n: usize) -> char {
    let step = (k * 3 + n as u32) % 7 + 1;
    let shift = |first: u8, span: u32| if k == 0 { ch } else { char::from(first + ((u32::from(ch as u8 - first) + step) % span) as u8) };
    match ch {
        '0'..='9' => shift(b'0', 10),
        'A'..='Z' => shift(b'A', 26),
        'a'..='z' => shift(b'a', 26),
        _ => ch,
    }
}

/// An image faded with the content, in a rounded square.
pub(crate) fn faded_image(p: P, img: &Image, x: f64, y: f64, size: f64, radius: f64) {
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
    /// Brings the cards' motion up to date and gives each its place in the stack: the sizes need the fonts, so it
    /// happens as they are drawn.
    pub(super) fn measure_cards(&mut self, fonts: &Fonts) {
        let (now, pill_h) = (self.now, self.spring.h.pos);
        let mut top = GAP;
        for i in 0..self.cards.stack.len() {
            let c = &self.cards.stack[i];
            if now < c.born {
                continue;
            }
            let age = c.age(now);
            if !c.leaving {
                let size = layout(fonts, c).size;
                let (above, from) = self.cards.anchor_above(i, pill_h);
                let c = &mut self.cards.stack[i];
                match c.anim.as_mut() {
                    Some(a) => {
                        let before = (a.top(), a.size().h);
                        a.resize(size, age);
                        a.move_to(top, age);
                        // what is going into this card goes where it goes
                        if before != (a.top(), a.size().h) {
                            let into = droplet::Anchor { bottom: a.top() + a.size().h, height: a.size().h };
                            let key = c.key;
                            self.cards.redirect_into(key, Some(key), into, now);
                        }
                    }
                    None => match c.arrival {
                        // from a drop of its own: it hangs from the pill, if from anything
                        Some(from) => {
                            let mut a = Animator::travel(size, from.from, top);
                            if from.shake {
                                a.shake();
                            }
                            c.anim = Some(a);
                            c.anchor = None;
                        }
                        None => {
                            c.anim = Some(Animator::enter(size, from, top));
                            c.anchor = above;
                        }
                    },
                }
            }
            // a card keeps its place until its drop starts to rise, so none below runs into it
            if let Some(a) = &self.cards.stack[i].anim {
                if a.rise_at().is_none_or(|rise| age < rise) {
                    top += a.size().h + STACK_GAP;
                }
            }
        }
    }

    /// A card's shape at this moment, when it shows.
    fn card_shape(&self, c: &Card) -> Option<Shape> {
        c.anim.as_ref().filter(|_| self.now >= c.born).map(|a| a.shape(c.age(self.now)))
    }

    /// The black of the cards and of the necks that join a drop to the pill or to the card above. Drawn before the pill,
    /// which covers where they meet it.
    pub(super) fn draw_card_shapes(&self, scene: &mut Scene, base: Affine, w: f64, h: f64, radius: f64) {
        let t = base * Affine::translate((w / 2.0, h));
        for c in &self.cards.stack {
            let Some(s) = self.card_shape(c) else { continue };
            // what it hangs from, as it is now
            let anchor = match c.anchor {
                None => Some(Blob { cx: 0.0, cy: -h / 2.0, half_w: w / 2.0, half_h: h / 2.0, radius }),
                Some(key) => self
                    .cards
                    .stack
                    .iter()
                    .find(|o| o.key == key)
                    .and_then(|o| self.card_shape(o))
                    .map(|o| Blob { cx: o.x, cy: o.top + o.h / 2.0, half_w: o.w / 2.0, half_h: o.h / 2.0, radius: o.radius }),
            };
            if let Some(loops) = anchor.and_then(|a| droplet::neck(&a, &s, 0.7)) {
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
            } else {
                scene.fill(Fill::NonZero, t, theme::BG, None, &RoundedRect::new(s.x - s.w / 2.0, s.top, s.x + s.w / 2.0, s.top + s.h, s.radius));
            }
        }
    }

    /// What the cards say; only those that stay take the pointer.
    pub(super) fn draw_card_contents(&mut self, scene: &mut Scene, fonts: &Fonts, hits: &mut Vec<Hit<Action>>, base: Affine, w: f64, h: f64) {
        let pointer = self.pointer_local();
        let mut rects = vec![None; self.cards.stack.len()];
        let mut codes = vec![None; self.cards.stack.len()];
        // the lowest card that stays and shows, for the count under it: (its bottom, how visible)
        let mut lowest = None;
        for (i, c) in self.cards.stack.iter().enumerate() {
            let (Some(s), Some(a)) = (self.card_shape(c), c.anim.as_ref()) else { continue };
            let live = !c.leaving;
            let age = c.age(self.now);
            let l = layout(fonts, c);
            let body = rect(w / 2.0 + s.x - s.w / 2.0, h + s.top, s.w, s.h);
            let mut mine = Vec::new();
            scene.push_clip_layer(Fill::NonZero, base, &RoundedRect::from_rect(body, s.radius));
            {
                let mut p = Painter { scene, fonts, hits: &mut mine, opacity: 0.0, pointer: if live { pointer } else { None }, transform: base, base: (1.0, base), since_enter: 10_000 };
                codes[i] = self.card_content(&mut p, c, a, &s, &l, age, body, live);
            }
            scene.pop_layer();
            // a drop that came with a colour of its own carries it until it opens
            if let Some(tint) = c.arrival.and_then(|from| from.tint).filter(|_| live) {
                let k = (1.0 - s.opened * 3.0).clamp(0.0, 1.0) as f32;
                if k > 0.0 {
                    let r = (s.w.min(s.h) / 2.0 - 4.0).max(1.0);
                    scene.fill(Fill::NonZero, base, with_alpha(rgb(tint), k), None, &vello::kurbo::Circle::new(body.center(), r));
                }
            }
            if live {
                if s.opened > 0.5 {
                    hits.extend(mine.into_iter().map(|hit| Hit { rect: hit.rect, action: Action::OnCard(c.n.id, Box::new(hit.action)) }));
                }
                // where it takes the pointer: the capsule as it is drawn
                rects[i] = Some((self.pill_origin.0 + body.x0, self.pill_origin.1 + body.y0, body.width(), body.height(), s.radius));
                lowest = Some((body.y1, a.content_opacity(age, 0.1, 0.25)));
            }
        }
        for ((c, r), code) in self.cards.stack.iter_mut().zip(rects).zip(codes) {
            (c.rect, c.code_at) = (r, code);
        }
        // a small drop under the stack counts the ones that wait
        let more = self.waiting.len();
        if let Some((bottom, k)) = lowest.filter(|(_, k)| more > 0 && *k > 0.0) {
            let (cx, cy, r) = (w / 2.0, bottom + 8.0 + 11.0, 11.0 * (0.5 + 0.5 * k));
            scene.fill(Fill::NonZero, base, theme::BG, None, &vello::kurbo::Circle::new((cx, cy), r));
            let mut unused = Vec::new();
            let mut p: Painter<Action> = Painter { scene, fonts, hits: &mut unused, opacity: k as f32, pointer: None, transform: base, base: (1.0, base), since_enter: 10_000 };
            p.text(&format!("+{more}"), cx, cy, 10.0, SEMI, theme::FG, Align::Center, 22.0);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn card_content(&self, p: P, c: &Card, a: &Animator, s: &Shape, l: &Layout, age: f64, body: Rect, live: bool) -> Option<(f64, f64)> {
        let n = &c.n;
        // words replaced in place fade in again
        let fresh = ((self.now - c.changed_at) as f64 / 180.0).clamp(0.0, 1.0);
        let at = |delay: f64, over: f64| (a.content_opacity(age, delay, over) * fresh) as f32;
        let cx = body.center().x;
        let (left, right, cy) = (cx - s.content_w / 2.0, cx + s.content_w / 2.0, body.center().y);
        let accent_rgb = accent_of(n);
        let accent = rgb(accent_rgb);
        let controls = live && (p.hovered(body) || self.kb.is_some());
        // a click on the card goes to the app that sent it, or copies the code it carries
        p.hit(body, l.code.clone().map_or(Action::NotifOpen, Action::CopyCode));

        // the icon
        p.opacity = at(0.0, 0.18);
        let icon_cx = left + PAD + l.icon_w / 2.0;
        // how long its words have been up (from the start again when they are replaced)
        let up = (age - a.cue()).min((self.now - c.changed_at) as f64 / 1000.0);
        let filled = n.ring.map(|percent| {
            let t = ((up - 0.05) / FILL_S).clamp(0.0, 1.0);
            f64::from(percent) / 100.0 * (1.0 - (1.0 - t).powi(3))
        });
        if let (Some(flow), Some(percent)) = (n.liquid, n.ring) {
            // the machine's battery: liquid that rises to its level from empty, or falls to it from full
            let t = ((up - 0.05) / POUR_S).clamp(0.0, 1.0);
            let from = if flow == Flow::Rising { 0.0 } else { 1.0 };
            let level = from + (f64::from(percent) / 100.0 - from) * (1.0 - (1.0 - t).powi(3));
            let calm = (1.0 - up / SETTLE_S).clamp(0.0, 1.0);
            liquid_drop(p, Point::new(icon_cx, cy), RING_R, level, 2.2 * calm, up * 9.0, accent);
        } else if let Some(filled) = filled {
            use std::f64::consts::{FRAC_PI_2, TAU};
            let centre = Point::new(icon_cx, cy);
            p.circle_stroke(centre.x, centre.y, RING_R, 3.0, with_alpha(accent, 0.25));
            if filled > 0.001 {
                stroke(p, &KArc::new(centre, Vec2::new(RING_R, RING_R), -FRAC_PI_2, filled * TAU, 0.0), 3.0, accent);
            }
        }
        match (&n.image, &n.app_icon, &n.icon) {
            (Some(img), ..) => faded_image(p, img, icon_cx - ICON / 2.0, cy - ICON / 2.0, ICON, ICON / 2.0),
            (None, Some(img), _) => faded_image(p, img, icon_cx - ICON / 2.0, cy - ICON / 2.0, ICON, 6.0),
            (None, None, Some(name)) if n.liquid.is_some() => p.icon(name, icon_cx, cy, 17.0, theme::FG),
            (None, None, Some(name)) if n.ring.is_some() => p.icon(name, icon_cx, cy, 18.0, theme::FG),
            (None, None, Some(name)) => p.icon(name, icon_cx, cy, ICON as f32, accent),
            (None, None, None) => {
                let kind = n.kind.unwrap_or(if n.critical { Kind::Warn } else { Kind::Info });
                p.circle(icon_cx, cy, ICON / 2.0, accent);
                kind_mark(p, kind, icon_cx, cy, 15.0, theme::FG);
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
        let tx = left + l.text_x;
        let mut code_at = None;
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
            let line = y + CODE_LINE / 2.0;
            code_at = Some((tx, line));
            let text = island_core::codes::display(code);
            // while its digits are on their way into the pill, a faint copy stays behind
            let flying = self.fx.flight.as_ref().is_some_and(|f| f.text == text && f.said(self.now) < 1.0);
            if flying {
                p.opacity *= 0.25;
            }
            let drawn = rolling_code(p, &text, tx, line, up - first);
            p.opacity = at(first, 0.25);
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
        let snoozes = snoozes(n);
        // when the icon says who, the ring says what
        let mark = n.kind.filter(|_| n.image.is_some() || n.app_icon.is_some() || n.icon.is_some());
        let mut end = right - PAD;
        if let Some(filled) = filled {
            p.text(&format!("{}%", (filled * 100.0).round() as i32), end, cy, 15.0, SEMI, theme::FG, Align::Right, PERCENT_W as f32);
        } else if l.buttons.is_empty() {
            let ring = Point::new(end - RING / 2.0, cy);
            if snoozes {
                // snooze takes the ring's place under the pointer
                let r = rect(ring.x - 13.0, cy - 13.0, 26.0, 26.0);
                if controls {
                    p.circle(ring.x, cy, 13.0, if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
                    p.icon("bell-z", ring.x, cy, 14.0, theme::FG);
                } else {
                    self.card_ring(p, ring, accent, c, live);
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
            } else if c.waits() {
                self.card_dot(p, ring, c);
            } else {
                self.card_ring(p, ring, accent, c, live);
                if let Some(kind) = mark {
                    kind_mark(p, kind, ring.x, ring.y, 11.0, theme::FG);
                }
            }
        } else {
            for (i, (label, action, bw)) in l.buttons.iter().enumerate().rev() {
                end -= bw;
                let r = rect(end, cy - BUTTON_H / 2.0, *bw, BUTTON_H);
                let hot = p.hovered(r);
                // the sender's first action is its main one; a script's question ends with "yes", as drip's does
                let main = if crate::island::is_script(n) { l.buttons.len() - 1 } else { 0 };
                let fill = if i == main {
                    let (r, g, b) = button_fill(accent_rgb);
                    let up = |c: u8| if hot { c.saturating_add(24) } else { c };
                    rgb((up(r), up(g), up(b)))
                } else {
                    with_alpha(theme::FG, if hot { 0.25 } else { 0.15 })
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
        code_at
    }

    /// The mark of a card that waits: a white dot that sends out a soft ping, for a while.
    fn card_dot(&self, p: P, centre: Point, c: &Card) {
        let since = self.now - c.born.max(c.changed_at);
        if (0..crate::island::card::PING_FOR_MS).contains(&since) {
            let t = (since % PING_MS) as f64 / (PING_MS as f64 * PING_SPREAD);
            if t < 1.0 {
                let k = 1.0 - (1.0 - t).powi(3);
                p.circle(centre.x, centre.y, DOT_R * (1.0 + (PING_SCALE - 1.0) * k), with_alpha(theme::FG, PING_OPACITY * (1.0 - k as f32)));
            }
        }
        p.circle(centre.x, centre.y, DOT_R, theme::FG);
    }

    /// The countdown: a faint circle and over it the time left, from twelve o'clock, draining clockwise.
    fn card_ring(&self, p: P, centre: Point, accent: Color, c: &Card, live: bool) {
        let r = RING / 2.0 - 1.4;
        p.circle_stroke(centre.x, centre.y, r, 2.4, with_alpha(accent, 0.28));
        let left_over = if live { self.card_time_left(c) } else { 0.0 };
        if left_over > 0.001 {
            use std::f64::consts::{FRAC_PI_2, TAU};
            let arc = KArc::new(centre, Vec2::new(r, r), -FRAC_PI_2 + (1.0 - left_over) * TAU, left_over * TAU, 0.0);
            stroke(p, &arc, 2.4, accent);
        }
    }
}

/// A round vessel with a liquid up to `level` (0 empty, 1 full), its surface a wave of height `ripple`.
fn liquid_drop(p: P, centre: Point, r: f64, level: f64, ripple: f64, phase: f64, colour: Color) {
    let under = p.transform * Affine::translate((centre.x, centre.y));
    let surface = r - 2.0 * r * level.clamp(0.0, 1.0);
    // flat at nothing and at full
    let ripple = if level <= 0.001 || level >= 0.999 { 0.0 } else { ripple };
    let mut wave = BezPath::new();
    wave.move_to((-r, r + 1.0));
    for i in 0..=24 {
        let x = -r + 2.0 * r * f64::from(i) / 24.0;
        wave.line_to((x, surface + ripple * (x / r * 3.6 + phase).sin()));
    }
    wave.line_to((r, r + 1.0));
    wave.close_path();
    let inside = Circle::new((0.0, 0.0), r);
    let alpha = p.opacity;
    p.scene.fill(Fill::NonZero, under, with_alpha(colour, 0.22 * alpha), None, &inside);
    p.scene.push_clip_layer(Fill::NonZero, under, &inside);
    p.scene.fill(Fill::NonZero, under, with_alpha(colour, alpha), None, &wave);
    p.scene.pop_layer();
}
