//! The small pills beside the pill and the drop that fills at its other side, drawn as liquid (their motion is in
//! `island/liquid.rs`, the melting outline in `island_core::droplet`).

use super::*;
use crate::island::liquid::{DROP_D, DROP_GAP, REST_H, WAVE_MS};
use island_core::droplet::{self, Blob, Shape};
use vello::kurbo::{BezPath, Circle};

/// How strongly a small pill and what it leaves pull on each other.
const BUBBLE_BLEND: f64 = 18.0;
/// The same for the hanging drop, which stays joined by a thin neck.
const DROP_BLEND: f64 = 18.0;

fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn outline(loops: Vec<Vec<(f64, f64)>>) -> BezPath {
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

impl Island {
    /// The black of the small pills and of the necks that join them to what they leave. Drawn before the pill, which
    /// covers where they meet it, and all of those it is opening over.
    pub(super) fn draw_bubble_shapes(&self, scene: &mut Scene, fonts: &Fonts, base: Affine, w: f64, h: f64, radius: f64) {
        // what the next one leaves: the pill, then the small pill before: (left, width)
        let mut before = (0.0, w);
        for (n, b) in self.bubbles.iter().enumerate() {
            let out = b.out(self.now);
            let (left, width) = self.bubble_place(n, w);
            if b.held.is_some() {
                let round = RoundedRect::new(left, 0.0, left + width, REST_H, REST_H / 2.0);
                scene.fill(Fill::NonZero, base, theme::BG, None, &round);
                let at = base * Affine::translate((left, 0.0));
                let mut unused = Vec::new();
                let mut painter: Painter<Action> = Painter { scene, fonts, hits: &mut unused, opacity: 1.0, pointer: None, transform: at, base: (1.0, at), since_enter: 10_000 };
                self.draw_bubble(&mut painter, b.side, BUBBLE_W, REST_H);
                continue;
            }
            if out <= 0.01 {
                continue;
            }
            let r = (h / 2.0).min(width / 2.0).min(30.0);
            let blend = BUBBLE_BLEND * (1.0 - smooth(0.6, 0.95, out));
            let anchor = Blob { cx: before.0 + before.1 / 2.0, cy: 0.0, half_w: before.1 / 2.0, half_h: h / 2.0, radius };
            let drop = Shape { top: left, w: h, h: width, radius: r, opened: out.clamp(0.0, 1.0), blend, content_w: width };
            match droplet::neck_sideways(&anchor, &drop, 0.7) {
                Some(loops) => {
                    // melting swells what overlaps: nothing may stand out of the two shapes taken together
                    let far = (left + width).max(before.0 + before.1);
                    scene.push_clip_layer(Fill::NonZero, base, &RoundedRect::new(before.0 + before.1 / 2.0, 0.0, far, h, r));
                    scene.fill(Fill::NonZero, base * Affine::translate((0.0, h / 2.0)), theme::BG, None, &outline(loops));
                    scene.pop_layer();
                }
                None => scene.fill(Fill::NonZero, base, theme::BG, None, &RoundedRect::new(left, 0.0, left + width, h, r)),
            }
            before = (left, width);
        }
    }

    /// What the small pills say, once they are nearly there.
    pub(super) fn draw_bubble_contents(&self, scene: &mut Scene, fonts: &Fonts, hits: &mut Vec<Hit<Action>>, base: Affine, w: f64, h: f64) {
        for (n, b) in self.bubbles.iter().enumerate() {
            let k = smooth(0.65, 0.97, b.out(self.now)) as f32;
            // (those the pill is opening over were drawn under it)
            if k <= 0.0 || b.held.is_some() {
                continue;
            }
            let (left, width) = self.bubble_place(n, w);
            let at = base * Affine::translate((left + (width - BUBBLE_W) / 2.0, 0.0));
            let mut painter = Painter { scene, fonts, hits: &mut *hits, opacity: k, pointer: None, transform: at, base: (k, at), since_enter: 10_000 };
            self.draw_bubble(&mut painter, b.side, BUBBLE_W, h);
            // the hit region is in the pill's coordinates (the drawing is shifted)
            hits.push(Hit { rect: rect(left, 0.0, width, h), action: Action::Open(side_to_page(b.side)) });
        }
    }

    /// Where the drop beside the pill is: (how far out, its width, its centre along x), when it shows.
    fn fill_place(&self, w: f64) -> Option<(f64, f64, f64)> {
        let f = self.fill.as_ref()?;
        if let Some(held) = f.held {
            // where it was on the screen, sinking into the end of the pill that is coming over it
            let there = held + (w + self.bubble_slot()) / 2.0;
            let inside = DROP_D / 2.0 + 20.0;
            let centre = there + (inside - there).max(0.0) * f.sunk(self.now);
            let gone = centre - DROP_D / 2.0 >= 19.5 || f.out(self.now) < 0.02;
            return (!gone).then_some((0.0, DROP_D, centre));
        }
        let out = f.out(self.now);
        if out <= 0.01 {
            return None;
        }
        // from hidden inside the pill's left end to hanging beside it
        let d = droplet::SEED + (DROP_D - droplet::SEED) * out.min(1.0);
        let hidden = d / 2.0 + 1.0;
        let hanging = -DROP_GAP - d / 2.0;
        Some((out, d, hidden + (hanging - hidden) * out))
    }

    /// The black of the drop and its neck. Drawn before the pill, which covers where they meet it, and all of a drop it
    /// is opening over.
    pub(super) fn draw_fill_shape(&self, scene: &mut Scene, base: Affine, w: f64, h: f64, radius: f64) {
        let Some((out, d, centre)) = self.fill_place(w) else { return };
        let Some(f) = self.fill.as_ref() else { return };
        if f.held.is_some() {
            scene.fill(Fill::NonZero, base, theme::BG, None, &Circle::new((centre, REST_H / 2.0), d / 2.0));
            self.draw_liquid(scene, base, d, centre);
            return;
        }
        // solved leaving the right end, then mirrored
        let shape = Shape { top: w - centre - d / 2.0, w: d, h: d, radius: d / 2.0, opened: out.clamp(0.0, 1.0), blend: DROP_BLEND, content_w: d };
        let anchor = Blob { cx: w / 2.0, cy: 0.0, half_w: w / 2.0, half_h: h / 2.0, radius };
        match droplet::neck_sideways(&anchor, &shape, 0.6) {
            Some(loops) => {
                scene.push_clip_layer(Fill::NonZero, base, &RoundedRect::new(centre - d / 2.0, 0.0, w / 2.0, h, d / 2.0));
                scene.fill(Fill::NonZero, base * Affine::translate((w, h / 2.0)) * Affine::scale_non_uniform(-1.0, 1.0), theme::BG, None, &outline(loops));
                scene.pop_layer();
            }
            None => scene.fill(Fill::NonZero, base, theme::BG, None, &Circle::new((centre, h / 2.0), d / 2.0)),
        }
    }

    /// The liquid in the drop: a level surface that ripples while it changes.
    fn draw_liquid(&self, scene: &mut Scene, base: Affine, d: f64, centre: f64) {
        let Some(f) = self.fill.as_ref() else { return };
        let (now, r) = (self.now, (d / 2.0 - 3.5).max(1.0));
        let under = base * Affine::translate((centre, REST_H / 2.0));
        let level = f.level(now).clamp(0.0, 1.0);
        let surface = r - 2.0 * r * level;
        let since = (now - f.changed) as f64;
        let ripple = 0.5 + 1.6 * (1.0 - since / WAVE_MS as f64).max(0.0);
        let phase = since.min(WAVE_MS as f64) / 110.0;
        let mut wave = BezPath::new();
        wave.move_to((-r, r + 1.0));
        for i in 0..=24 {
            let x = -r + 2.0 * r * i as f64 / 24.0;
            // flat at nothing and at full
            let bend = if level <= 0.001 || level >= 0.999 { 0.0 } else { ripple * (x / r * 3.6 + phase).sin() };
            wave.line_to((x, surface + bend));
        }
        wave.line_to((r, r + 1.0));
        wave.close_path();
        let tint = Color::from_rgba8(f.tint.0, f.tint.1, f.tint.2, 255);
        let inside = Circle::new((0.0, 0.0), r);
        scene.fill(Fill::NonZero, under, with_alpha(tint, 0.22), None, &inside);
        scene.push_clip_layer(Fill::NonZero, under, &inside);
        scene.fill(Fill::NonZero, under, tint, None, &wave);
        scene.pop_layer();
    }

    /// What is in a drop that hangs, and where it takes the pointer.
    pub(super) fn draw_fill(&mut self, scene: &mut Scene, hits: &mut Vec<Hit<Action>>, base: Affine, w: f64) {
        let place = self.fill_place(w).filter(|_| self.fill.as_ref().is_some_and(|f| f.held.is_none()));
        let Some((out, d, centre)) = place else {
            if let Some(f) = self.fill.as_mut() {
                f.rect = None;
            }
            return;
        };
        // the liquid shows once the drop is clear of the pill
        if out > 0.6 {
            self.draw_liquid(scene, base, d, centre);
        }
        let origin = self.pill_origin;
        let Some(f) = self.fill.as_mut() else { return };
        let on_pill = rect(centre - d / 2.0, REST_H / 2.0 - d / 2.0, d, d);
        // it takes the pointer only while it hangs
        f.rect = (out > 0.9).then_some((origin.0 + on_pill.x0, origin.1 + on_pill.y0, d, d));
        if f.rect.is_some() {
            hits.push(Hit { rect: on_pill, action: Action::Open(f.page) });
        }
    }
}
