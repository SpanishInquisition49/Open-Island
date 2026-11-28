//! Whether a window is over the place of the island: the middle of the top edge of a screen. Pure: the platforms say
//! where the windows are.

/// The island's place, in logical pixels: half the width of a small pill, and how far down it reaches.
pub const HALF_WIDTH: f64 = 115.0;
pub const DEPTH: f64 = 32.0;

/// A rectangle by its edges. The right and bottom ones are outside it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Rect {
    /// Whether the two have some area in common: touching edges is not covering.
    pub fn overlaps(&self, other: &Rect) -> bool {
        self.left < other.right && other.left < self.right && self.top < other.bottom && other.top < self.bottom
    }
}

/// The island's place on a screen, at `scale` pixels of the screen per logical pixel.
pub fn place(screen: Rect, scale: f64) -> Rect {
    let middle = (screen.left + screen.right) / 2.0;
    Rect { left: middle - HALF_WIDTH * scale, top: screen.top, right: middle + HALF_WIDTH * scale, bottom: screen.top + DEPTH * scale }
}

/// What a window system says of a window, as far as it matters here.
#[derive(Clone, Copy, Debug)]
pub struct Seen {
    pub at: Rect,
    /// On screen: shown, not minimized, not kept out of sight by the system (another virtual desktop, a suspended app).
    pub shown: bool,
    /// The island's own, or the desktop and the taskbar.
    pub furniture: bool,
    /// Lets the pointer through or shows nothing: an overlay, not something in the way.
    pub ghost: bool,
}

impl Seen {
    pub fn is_over(&self, place: &Rect) -> bool {
        self.shown && !self.furniture && !self.ghost && self.at.overlaps(place)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: f64, top: f64, right: f64, bottom: f64) -> Rect {
        Rect { left, top, right, bottom }
    }

    #[test]
    fn a_window_covers_the_place_when_it_takes_some_of_it() {
        let screen = rect(0.0, 0.0, 1920.0, 1080.0);
        let here = place(screen, 1.0);
        assert_eq!(here, rect(845.0, 0.0, 1075.0, 32.0));
        assert!(rect(0.0, 0.0, 1920.0, 1040.0).overlaps(&here), "maximized");
        assert!(rect(1000.0, 20.0, 1400.0, 500.0).overlaps(&here), "a corner of it");
        assert!(!rect(0.0, 0.0, 845.0, 1080.0).overlaps(&here), "beside it, touching");
        assert!(!rect(0.0, 32.0, 1920.0, 1080.0).overlaps(&here), "below it");
        assert!(!rect(900.0, 400.0, 1000.0, 500.0).overlaps(&here), "in the middle of the screen");
    }

    #[test]
    fn only_a_window_that_is_really_there_is_over_it() {
        let place = rect(845.0, 0.0, 1075.0, 32.0);
        let window = Seen { at: rect(0.0, 0.0, 1920.0, 1040.0), shown: true, furniture: false, ghost: false };
        assert!(window.is_over(&place));
        assert!(!Seen { shown: false, ..window }.is_over(&place));
        assert!(!Seen { furniture: true, ..window }.is_over(&place), "a taskbar at the top");
        assert!(!Seen { ghost: true, ..window }.is_over(&place), "an overlay");
        assert!(!Seen { at: rect(0.0, 40.0, 1920.0, 1040.0), ..window }.is_over(&place));
    }

    #[test]
    fn the_place_follows_the_screen_and_its_scale() {
        let second = rect(-2560.0, -200.0, 0.0, 1240.0);
        assert_eq!(place(second, 1.5), rect(-1452.5, -200.0, -1107.5, -152.0));
    }
}
