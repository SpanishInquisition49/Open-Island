//! Drawing helpers: rounded rectangles, text placed by its vertical centre, icons, bars, buttons.
//! Everything is in logical pixels; the caller sets the scene transform.

use crate::text::{Fonts, Weight};
use crate::theme::{self, with_alpha};
use vello::kurbo::{Affine, Rect, RoundedRect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// A clickable/hoverable region registered while drawing, identified by the caller's id.
#[derive(Clone, Debug)]
pub struct Hit<A> {
    pub rect: Rect,
    pub action: A,
}

pub struct Painter<'a, A> {
    pub scene: &'a mut Scene,
    pub fonts: &'a Fonts,
    pub hits: &'a mut Vec<Hit<A>>,
    /// Multiplied into every colour: the content crossfade.
    pub opacity: f32,
    /// Pointer position in the same space as the hits, for the hover highlight.
    pub pointer: Option<(f64, f64)>,
    /// Applied to everything drawn.
    pub transform: Affine,
    /// The face's own opacity and transform, which `stagger` builds on.
    pub base: (f32, Affine),
    /// Milliseconds since the face began to appear (the stagger clock).
    pub since_enter: i64,
}

impl<A: Clone> Painter<'_, A> {
    fn c(&self, c: Color) -> Color {
        with_alpha(c, self.opacity)
    }

    /// Rows of an open page come in one after another: each fades and rises 28 ms after the previous.
    /// Call before drawing row `i`; everything drawn afterwards follows until the next call.
    pub fn stagger(&mut self, i: usize) {
        let k = ((self.since_enter - i as i64 * 28) as f64 / 170.0).clamp(0.0, 1.0);
        let k = 1.0 - (1.0 - k).powi(3);
        self.opacity = self.base.0 * k as f32;
        self.transform = self.base.1 * Affine::translate((0.0, 8.0 * (1.0 - k)));
    }

    pub fn hovered(&self, r: Rect) -> bool {
        self.pointer.is_some_and(|(x, y)| r.contains(vello::kurbo::Point::new(x, y)))
    }

    pub fn rrect(&mut self, r: Rect, radius: f64, color: Color) {
        let shape = RoundedRect::from_rect(r, radius.min(r.height() / 2.0).min(r.width() / 2.0));
        let color = self.c(color);
        self.scene.fill(Fill::NonZero, self.transform, color, None, &shape);
    }

    pub fn rrect_stroke(&mut self, r: Rect, radius: f64, width: f64, color: Color) {
        let shape = RoundedRect::from_rect(r, radius.min(r.height() / 2.0).min(r.width() / 2.0));
        let color = self.c(color);
        self.scene.stroke(&Stroke::new(width), self.transform, color, None, &shape);
    }

    /// A straight line of a given width and dash pattern (empty: solid).
    pub fn line(&mut self, a: (f64, f64), b: (f64, f64), width: f64, dash: &[f64], color: Color) {
        let color = self.c(color);
        let mut stroke = Stroke::new(width).with_caps(vello::kurbo::Cap::Round);
        if !dash.is_empty() {
            stroke = stroke.with_dashes(0.0, dash.iter().copied());
        }
        self.scene.stroke(&stroke, self.transform, color, None, &vello::kurbo::Line::new(a, b));
    }

    /// A line through several points (a route), round at the joins.
    pub fn polyline(&mut self, pts: &[(f64, f64)], width: f64, dash: &[f64], color: Color) {
        if pts.len() < 2 {
            return;
        }
        let mut path = vello::kurbo::BezPath::new();
        path.move_to(pts[0]);
        for p in &pts[1..] {
            path.line_to(*p);
        }
        let color = self.c(color);
        let mut stroke = Stroke::new(width).with_caps(vello::kurbo::Cap::Round).with_join(vello::kurbo::Join::Round);
        if !dash.is_empty() {
            stroke = stroke.with_dashes(0.0, dash.iter().copied());
        }
        self.scene.stroke(&stroke, self.transform, color, None, &path);
    }

    pub fn circle_stroke(&mut self, cx: f64, cy: f64, r: f64, width: f64, color: Color) {
        let color = self.c(color);
        self.scene.stroke(&Stroke::new(width), self.transform, color, None, &vello::kurbo::Circle::new((cx, cy), r));
    }

    pub fn circle(&mut self, cx: f64, cy: f64, r: f64, color: Color) {
        let shape = vello::kurbo::Circle::new((cx, cy), r);
        let color = self.c(color);
        self.scene.fill(Fill::NonZero, self.transform, color, None, &shape);
    }

    /// Baseline for text whose cap-height centre sits on `cy`.
    fn baseline(cy: f64, size: f32) -> f64 {
        cy + size as f64 * 0.36
    }

    /// Draws one line of text. `x` is the left edge, centre or right edge depending on `align`; returns the
    /// width drawn. Text wider than `max_w` is cut with an ellipsis.
    #[allow(clippy::too_many_arguments)]
    pub fn text(&mut self, s: &str, x: f64, cy: f64, size: f32, w: Weight, color: Color, align: Align, max_w: f32) -> f32 {
        let s = self.fonts.ellipsize(s, size, w, max_w);
        let width = self.fonts.measure(&s, size, w);
        let left = match align {
            Align::Left => x,
            Align::Center => x - width as f64 / 2.0,
            Align::Right => x - width as f64,
        };
        let color = self.c(color);
        let (gx, gy) = (left, Self::baseline(cy, size));
        self.fonts_draw(&s, gx, gy, size, w, color);
        width
    }

    fn fonts_draw(&mut self, s: &str, x: f64, y: f64, size: f32, w: Weight, color: Color) {
        // Fonts::draw takes absolute coordinates; compose the painter transform by drawing into a child scene.
        // vello's `append` of a scene with nothing in it (an empty artist) leaves what is drawn afterwards displaced
        if s.is_empty() {
            return;
        }
        let mut child = Scene::new();
        self.fonts.draw(&mut child, s, x, y, size, w, color);
        self.scene.append(&child, Some(self.transform));
    }

    pub fn icon(&mut self, name: &str, cx: f64, cy: f64, size: f32, color: Color) {
        let mut child = Scene::new();
        self.fonts.draw_icon(&mut child, name, cx - size as f64 / 2.0, cy - size as f64 / 2.0, size, self.c(color));
        self.scene.append(&child, Some(self.transform));
    }

    /// A rounded progress bar (frac 0..=1).
    pub fn bar(&mut self, r: Rect, frac: f64, fill: Color, track: Color) {
        let radius = r.height() / 2.0;
        self.rrect(r, radius, track);
        let f = frac.clamp(0.0, 1.0);
        if f > 0.0 {
            let w = (r.width() * f).max(r.height());
            self.rrect(Rect::new(r.x0, r.y0, r.x0 + w, r.y1), radius, fill);
        }
    }

    /// A pill-shaped text button; registers its hit region.
    pub fn button(&mut self, r: Rect, label: &str, action: A) {
        let hot = self.hovered(r);
        self.rrect(r, r.height() / 2.0, if hot { theme::HOT_CHIP } else { theme::CHIP });
        self.text(label, r.center().x, r.center().y, 12.0, Weight::Semibold, theme::FG, Align::Center, r.width() as f32 - 8.0);
        self.hits.push(Hit { rect: r, action });
    }

    /// Width a `button` with this label needs.
    pub fn button_width(&self, label: &str) -> f64 {
        (self.fonts.measure(label, 12.0, Weight::Semibold) as f64 + 32.0).max(48.0)
    }

    /// A round icon button.
    pub fn icon_button(&mut self, cx: f64, cy: f64, size: f64, icon: &str, icon_size: f32, action: A) {
        let r = Rect::new(cx - size / 2.0, cy - size / 2.0, cx + size / 2.0, cy + size / 2.0);
        if self.hovered(r) {
            self.circle(cx, cy, size / 2.0, theme::HOT_CHIP);
        }
        self.icon(icon, cx, cy, icon_size, theme::FG);
        self.hits.push(Hit { rect: r, action });
    }

    /// An icon button whose icon is drawn `dx` to the side of where its hit region stays (a nudge on a press).
    pub fn icon_button_nudged(&mut self, (cx, cy): (f64, f64), size: f64, icon: &str, icon_size: f32, dx: f64, action: A) {
        let r = Rect::new(cx - size / 2.0, cy - size / 2.0, cx + size / 2.0, cy + size / 2.0);
        if self.hovered(r) {
            self.circle(cx, cy, size / 2.0, theme::HOT_CHIP);
        }
        self.icon(icon, cx + dx, cy, icon_size, theme::FG);
        self.hits.push(Hit { rect: r, action });
    }

    /// A toggle chip: filled white when on, dim when off.
    pub fn chip(&mut self, r: Rect, icon: &str, label: &str, on: bool, action: A) {
        let hot = self.hovered(r);
        let (fill, fg) = if on {
            (if hot { theme::HOT_ACCENT } else { theme::ACCENT }, theme::ON_ACCENT)
        } else {
            (if hot { theme::HOT_CHIP } else { theme::CHIP }, theme::FG)
        };
        self.rrect(r, 12.0, fill);
        self.icon(icon, r.x0 + 18.0, r.center().y, 16.0, fg);
        self.text(label, r.x0 + 32.0, r.center().y, 12.0, Weight::Semibold, fg, Align::Left, (r.width() - 40.0) as f32);
        self.hits.push(Hit { rect: r, action });
    }

    /// A slider: track, fill and a round thumb (value 0..=1).
    pub fn slider(&mut self, r: Rect, value: f64, action: A) {
        self.bar(r, value, theme::FG, theme::TRACK);
        let x = r.x0 + (r.width() - r.height()) * value.clamp(0.0, 1.0) + r.height() / 2.0;
        self.circle(x, r.center().y, r.height() * 0.9, theme::FG);
        self.hits.push(Hit { rect: Rect::new(r.x0 - 6.0, r.y0 - 9.0, r.x1 + 6.0, r.y1 + 9.0), action });
    }

    pub fn hit(&mut self, rect: Rect, action: A) {
        self.hits.push(Hit { rect, action });
    }
}
