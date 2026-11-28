//! The shelf page: the files set down on the island, each to be dragged away, opened or taken off.

use super::*;

const ROW: f64 = 34.0;

impl Island {
    pub(crate) fn shelf_height(&self) -> f64 {
        14.0 + 34.0 + 6.0 + self.shelf.len().max(1) as f64 * ROW + 22.0 + 10.0
    }

    pub(super) fn face_shelf(&self, p: P, w: f64, _h: f64) {
        let head = self.page_header(p, w, "Shelf");
        if !self.shelf.is_empty() {
            let (send, clear) = (p.button_width("Send"), p.button_width("Clear"));
            p.button(rect(w - 16.0 - clear, head - 12.0, clear, 24.0), "Clear", Action::ShelfClear);
            p.button(rect(w - 24.0 - clear - send, head - 12.0, send, 24.0), "Send", Action::ShelfSend);
        }
        let mut y = 14.0 + 34.0 + 6.0;
        if self.shelf.is_empty() {
            p.stagger(1);
            p.text("Drop files on the island to keep them here", w / 2.0, y + ROW / 2.0, 12.0, REG, theme::DIM, Align::Center, (w - 40.0) as f32);
            y += ROW;
        }
        for (n, path) in self.shelf.iter().enumerate() {
            p.stagger(1 + n);
            let row = rect(8.0, y, w - 16.0, ROW - 4.0);
            p.rrect(row, 10.0, if p.hovered(row) { theme::HOT_CHIP } else { theme::CHIP });
            let cy = row.center().y;
            let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
            let folder = path[..path.len() - file.len()].trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or("");
            p.icon("file", 26.0, cy, 14.0, theme::ACCENT);
            p.text(file, 44.0, cy, 12.0, REG, theme::FG, Align::Left, (w - 44.0 - 160.0) as f32);
            p.text(folder, row.x1 - 62.0, cy, 10.0, REG, theme::DIM, Align::Right, 90.0);
            p.hit(row, Action::ShelfDrag(n));
            p.icon_button(row.x1 - 40.0, cy, 22.0, "external-link", 11.0, Action::ShelfOpen(n));
            p.icon_button(row.x1 - 15.0, cy, 22.0, "x", 11.0, Action::ShelfRemove(n));
            y += ROW;
        }
        let hint = if self.data.can_drag_files { "Drag a file away to drop it somewhere else" } else { "Open, send, or take files off" };
        p.text(hint, w / 2.0, y + 11.0, 10.0, REG, theme::DIM, Align::Center, (w - 32.0) as f32);
    }
}
