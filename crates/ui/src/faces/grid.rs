//! The page of all pages: every page the island has, in groups, one click to open. The dock only holds Home, what is live
//! and the pages opened most; this is where the others are.

use super::*;
use island_core::face::GROUPS;

const COLS: usize = 4;
const CELL_H: f64 = 50.0;

impl Island {
    /// The pages the grid offers: the ones that exist now, and Sound (reached from Quick settings otherwise).
    fn grid_pages(&self) -> Vec<Page> {
        let mut v = self.pages();
        if !v.contains(&Page::Audio) {
            v.push(Page::Audio);
        }
        v.retain(|p| !matches!(p, Page::Send | Page::Grid));
        v
    }

    /// Non-empty groups with their pages, in the grid's order.
    pub(crate) fn grid_groups(&self) -> Vec<(&'static str, Vec<Page>)> {
        let all = self.grid_pages();
        GROUPS.iter().map(|g| (*g, island_core::face::ALL_PAGES.iter().copied().filter(|p| p.group() == *g && all.contains(p)).collect::<Vec<_>>())).filter(|(_, v)| !v.is_empty()).collect()
    }

    pub(crate) fn grid_height(&self) -> f64 {
        let groups = self.grid_groups();
        let rows: usize = groups.iter().map(|(_, v)| v.len().div_ceil(COLS)).sum();
        14.0 + 34.0 + groups.len() as f64 * 22.0 + rows as f64 * (CELL_H + 6.0) + 10.0
    }

    pub(super) fn face_grid(&self, p: P, w: f64, _h: f64) {
        self.page_header(p, w, "All pages");
        let gap = 6.0;
        let cw = (w - 28.0 - gap * (COLS as f64 - 1.0)) / COLS as f64;
        let mut y = 14.0 + 34.0;
        for (n, (title, pages)) in self.grid_groups().into_iter().enumerate() {
            p.stagger(n + 1);
            p.text(title, 20.0, y + 10.0, 10.0, SEMI, theme::DIM, Align::Left, 160.0);
            y += 22.0;
            for (i, page) in pages.iter().enumerate() {
                let (col, row) = (i % COLS, i / COLS);
                let r = rect(14.0 + col as f64 * (cw + gap), y + row as f64 * (CELL_H + 6.0), cw, CELL_H);
                p.rrect(r, 12.0, if p.hovered(r) { theme::HOT_CHIP } else { theme::CHIP });
                p.icon(page_icon(*page), r.center().x, r.y0 + 18.0, 17.0, theme::FG);
                p.text(page.label(), r.center().x, r.y0 + 38.0, 10.5, REG, theme::DIM, Align::Center, cw as f32 - 6.0);
                p.hit(r, Action::Open(*page));
            }
            y += pages.len().div_ceil(COLS) as f64 * (CELL_H + 6.0);
        }
    }
}
