//! The drawing of the short animations whose motion is in `island/effects.rs`. The black of a drop is drawn before the
//! pill (which covers where they meet it), what is on it after.

use super::*;
use crate::island::effects::{Burst, Catch, CONFETTI_DEPTH, REACH, SHOT_MS};
use island_core::droplet::{self, Blob, Shape};
use vello::kurbo::{BezPath, Circle};

/// The drop that reaches for a file: its width, and half the width of the neck where it leaves the pill.
const TONGUE_D: f64 = 34.0;
const TONGUE_MOUTH: f64 = 30.0;
/// The drop that leaves after a send: its width as it sets off and as it vanishes, and how far it goes.
const SHOT_FROM_D: f64 = 22.0;
const SHOT_TO_D: f64 = 8.0;
const SHOT_WAY: f64 = 84.0;
const SHOT_BLEND: f64 = 18.0;
const SHOT_TINT: Color = Color::from_rgba8(0x0a, 0x84, 0xff, 255);
/// Colours for the held notifications whose app has no icon, picked by the name.
const MARKS: [(u8, u8, u8); 6] = [(0x0a, 0x84, 0xff), (0x30, 0xd1, 0x58), (0xff, 0x9f, 0x0a), (0xbf, 0x5a, 0xf2), (0xff, 0x45, 0x3a), (0x64, 0xd2, 0xff)];

fn path(loops: Vec<Vec<(f64, f64)>>) -> BezPath {
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
    path
}

/// A round drop at (`x`, `y`) from the middle of the pill's bottom edge, joined to the pill while they are close.
fn hanging_drop(scene: &mut Scene, base: Affine, (w, h, radius): (f64, f64, f64), (x, y): (f64, f64), d: f64, blend: f64) {
    let at = base * Affine::translate((w / 2.0, h));
    let pill = Blob { cx: 0.0, cy: -h / 2.0, half_w: w / 2.0, half_h: h / 2.0, radius };
    let drop = Shape { x, top: y - d / 2.0, w: d, h: d, radius: d / 2.0, opened: 0.0, blend, content_w: d };
    match droplet::neck(&pill, &drop, 0.7) {
        Some(loops) => scene.fill(Fill::NonZero, at, theme::BG, None, &path(loops)),
        None => scene.fill(Fill::NonZero, at, theme::BG, None, &Circle::new((x, y), d / 2.0)),
    }
}

/// The pill reaching down to (`x`, `y`), in its own coordinates, with a drop `d` wide at the end: unlike a drop that
/// falls, it never lets go, and its neck thins as it stretches.
fn tongue(scene: &mut Scene, base: Affine, h: f64, (x, y): (f64, f64), d: f64) {
    let r = d / 2.0;
    let top = h - 5.0;
    if y > top + 3.0 {
        let stretch = ((y - h) / REACH).clamp(0.0, 1.0);
        let waist = r * (1.0 - 0.6 * stretch);
        let mouth = (TONGUE_MOUTH * d / TONGUE_D).max(waist);
        // the neck meets the drop a little above its widest
        let (side, shoulder) = (r * 0.92, (y - r * 0.4).max(top));
        let mid = (top + shoulder) / 2.0;
        let mut neck = BezPath::new();
        neck.move_to((x - mouth, top));
        neck.curve_to((x - waist * 0.9, top + 4.0), (x - waist * 0.8, mid), (x - side, shoulder));
        neck.line_to((x + side, shoulder));
        neck.curve_to((x + waist * 0.8, mid), (x + waist * 0.9, top + 4.0), (x + mouth, top));
        neck.close_path();
        scene.fill(Fill::NonZero, base, theme::BG, None, &neck);
    }
    scene.fill(Fill::NonZero, base, theme::BG, None, &Circle::new((x, y), r));
}

impl Island {
    /// The confetti of a birthday, bursting from under the pill over what is there.
    pub(super) fn draw_confetti(&self, scene: &mut Scene, base: Affine, w: f64, h: f64) {
        use island_core::birthday::{confetti, COLOURS};
        let Some(born) = self.fx.confetti.as_ref().and_then(|c| c.born) else { return };
        let spread = crate::island::card::CARD_MAX_W / 2.0 - 12.0;
        for bit in confetti(self.now - born, spread, CONFETTI_DEPTH) {
            let (r, g, b) = COLOURS[bit.colour % COLOURS.len()];
            let at = base * Affine::translate((w / 2.0 + bit.x, h - 4.0 + bit.y)) * Affine::rotate(bit.turn);
            let shape = vello::kurbo::Rect::new(-bit.w / 2.0, -bit.h / 2.0, bit.w / 2.0, bit.h / 2.0);
            scene.fill(Fill::NonZero, at, Color::from_rgba8(r, g, b, (bit.alpha * 255.0).round() as u8), None, &shape);
        }
    }

    /// The black of the drops of the effects. Drawn before the pill.
    pub(super) fn draw_effect_shapes(&self, scene: &mut Scene, base: Affine, w: f64, h: f64, radius: f64) {
        let pill = (w, h, radius);
        if let Some(b) = &self.fx.burst {
            for n in 0..b.drops.len() {
                let p = b.place(n, self.now - b.born, h);
                hanging_drop(scene, base, pill, (p.x, p.y), p.d, p.blend);
            }
        }
        if let Some(c) = &self.fx.catch {
            let out = c.out(self.now);
            if out > 0.02 {
                tongue(scene, base, h, c.tongue, 8.0 + (TONGUE_D - 8.0) * out);
            }
        }
        if let Some((x, d, alpha, near)) = self.shot_place(w) {
            let shape = Shape { x: 0.0, top: x - d / 2.0, w: d, h: d, radius: d / 2.0, opened: 0.0, blend: SHOT_BLEND, content_w: d };
            let anchor = Blob { cx: w / 2.0, cy: 0.0, half_w: w / 2.0, half_h: h / 2.0, radius };
            match droplet::neck_sideways(&anchor, &shape, 0.7).filter(|_| near) {
                Some(loops) => scene.fill(Fill::NonZero, base * Affine::translate((0.0, h / 2.0)), theme::BG, None, &path(loops)),
                None => scene.fill(Fill::NonZero, base, with_alpha(theme::BG, alpha), None, &Circle::new((x, h / 2.0), d / 2.0)),
            }
        }
    }

    /// The drop that leaves after a send: (its middle along x in the pill's coordinates, its width, how visible,
    /// whether it is still close enough to the pill to melt with it).
    fn shot_place(&self, w: f64) -> Option<(f64, f64, f32, bool)> {
        let t = (self.now - self.fx.shot?) as f64 / SHOT_MS as f64;
        if !(0.0..1.0).contains(&t) {
            return None;
        }
        let eased = 1.0 - (1.0 - t).powi(2);
        let end = w + self.bubble_slot();
        let x = end - SHOT_FROM_D / 2.0 - 4.0 + (SHOT_WAY + SHOT_FROM_D / 2.0) * eased;
        let d = SHOT_FROM_D + (SHOT_TO_D - SHOT_FROM_D) * eased;
        let alpha = (1.0 - ((t - 0.65) / 0.35).clamp(0.0, 1.0)) as f32;
        Some((x, d, alpha, self.bubbles.is_empty() && t < 0.5))
    }

    /// What is on the drops of the effects, the digits in flight and the pill's word about them. Drawn over everything.
    pub(super) fn draw_effect_contents(&self, scene: &mut Scene, fonts: &Fonts, base: Affine, w: f64, h: f64, radius: f64) {
        let mut unused = Vec::new();
        let mut p: Painter<Action> = Painter { scene, fonts, hits: &mut unused, opacity: 1.0, pointer: None, transform: base, base: (1.0, base), since_enter: 10_000 };
        if let Some(b) = &self.fx.burst {
            self.burst_marks(&mut p, b, w, h);
        }
        if let Some(c) = &self.fx.catch {
            self.catch_mark(&mut p, c, h);
        }
        if let Some((x, d, alpha, _)) = self.shot_place(w).filter(|(x, d, ..)| x - d / 2.0 > w - 6.0) {
            p.opacity = alpha;
            p.circle(x, h / 2.0, (d / 2.0 - 3.0).max(1.5), SHOT_TINT);
        }
        self.draw_flight(&mut p, w, h, radius);
    }

    /// Each held notification shows its app: the icon, else its first letter.
    fn burst_marks(&self, p: P, b: &Burst, w: f64, h: f64) {
        for (n, held) in b.drops.iter().enumerate() {
            let at = b.place(n, self.now - b.born, h);
            if at.content <= 0.01 {
                continue;
            }
            p.opacity = at.content as f32;
            let (cx, cy, size) = (w / 2.0 + at.x, h + at.y, at.d - 9.0);
            match &held.icon {
                Some(img) => {
                    p.scene.push_clip_layer(Fill::NonZero, p.transform, &Circle::new((cx, cy), size / 2.0));
                    draw_image(p, img, cx - size / 2.0, cy - size / 2.0, size, size / 2.0);
                    p.scene.pop_layer();
                }
                None => {
                    let (r, g, b) = MARKS[held.app.bytes().map(usize::from).sum::<usize>() % MARKS.len()];
                    p.circle(cx, cy, size / 2.0, Color::from_rgba8(r, g, b, 255));
                    let first: String = held.app.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into());
                    p.text(&first, cx, cy, 10.0, SEMI, theme::FG, Align::Center, size as f32);
                }
            }
        }
    }

    /// The file on the drop that reaches for it, once that drop is clear of the pill's words.
    fn catch_mark(&self, p: P, c: &Catch, h: f64) {
        let out = c.out(self.now);
        let clear = ((c.tongue.1 - (h - 4.0)) / 14.0).clamp(0.0, 1.0);
        p.opacity = (clear * out) as f32;
        if p.opacity > 0.01 {
            p.icon("file", c.tongue.0, c.tongue.1, (11.0 + 7.0 * out) as f32, theme::FG);
        }
    }

    fn draw_flight(&self, p: P, w: f64, h: f64, radius: f64) {
        let Some(f) = &self.fx.flight else { return };
        let (from_x, from_y) = f.from;
        let mut before = String::new();
        for (n, ch) in f.text.chars().enumerate() {
            let left = p.fonts.measure(&before, 19.0, SEMI) as f64;
            before.push(ch);
            let width = p.fonts.measure(&before, 19.0, SEMI) as f64 - left;
            let t = f.way(n, self.now);
            if ch.is_whitespace() || t >= 1.0 {
                continue;
            }
            // slow to leave, quick to arrive, rising on a slight arc
            let e = t * t * (2.0 - t);
            let start = (from_x + left + width / 2.0, from_y);
            let landing = w / 2.0 + (n as f64 - (f.text.chars().count() as f64 - 1.0) / 2.0) * 8.0;
            let x = start.0 + (landing - start.0) * e;
            let y = start.1 + (h / 2.0 - start.1) * e - (std::f64::consts::PI * t).sin() * 10.0;
            p.opacity = (1.0 - ((t - 0.8) / 0.2).clamp(0.0, 1.0)) as f32;
            p.text(&ch.to_string(), x, y, (19.0 - 10.0 * e) as f32, SEMI, theme::FG, Align::Center, 24.0);
        }
        // the pill says they are in (an open page stays as it is)
        let said = f.said(self.now) as f32;
        if said > 0.01 && self.shown.is_compact() {
            p.opacity = said;
            p.rrect(rect(0.0, 0.0, w, h), radius, theme::BG);
            let label = "Copied";
            let text = p.fonts.measure(label, 13.0, SEMI) as f64;
            let x = (w - text - 20.0) / 2.0;
            p.icon("check", x + 7.0, h / 2.0, 15.0, theme::GREEN);
            p.text(label, x + 20.0, h / 2.0, 13.0, SEMI, theme::FG, Align::Left, 90.0);
        }
    }

    /// The pill while a file is dragged over it.
    pub(super) fn face_catch(&self, p: P, w: f64, h: f64) {
        let dropped = self.fx.catch.as_ref().is_some_and(|c| c.dropped.is_some());
        let (icon, label) = if dropped { ("check", "On the shelf") } else { ("inbox", "Drop on the shelf") };
        let text = p.fonts.measure(label, 13.0, SEMI) as f64;
        let x = (w - text - 22.0) / 2.0;
        p.icon(icon, x + 7.0, h / 2.0, 15.0, if dropped { theme::GREEN } else { theme::FG });
        p.text(label, x + 22.0, h / 2.0, 13.0, SEMI, theme::FG, Align::Left, 160.0);
    }
}
