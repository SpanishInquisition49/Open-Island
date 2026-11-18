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
}

impl<A: Clone> Painter<'_, A> {
    fn c(&self, c: Color) -> Color {
        with_alpha(c, self.opacity)
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

    pub fn hit(&mut self, rect: Rect, action: A) {
        self.hits.push(Hit { rect, action });
    }
}
