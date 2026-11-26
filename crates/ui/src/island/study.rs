//! The files for the next lecture: found by the title of the event (see `island_core::materials`), one click to open, on the
//! Today page and in the morning and evening cards.

use super::*;
use island_core::materials::Material;
use island_core::timeline::{Item, Kind};

/// How often the same title is looked up again.
const ASK_AGAIN_MS: i64 = 10 * 60_000;

impl Island {
    /// The first timed event that has not ended.
    pub(crate) fn next_lecture(&self) -> Option<Item> {
        self.today_left().into_iter().find(|i| i.kind == Kind::Event && !i.all_day)
    }

    /// The files found for this title, best first.
    pub(crate) fn materials_for(&self, title: &str) -> Vec<Material> {
        self.data.materials.get(&island_core::materials::fold(title)).map(|(_, v)| v.clone()).unwrap_or_default()
    }

    /// The next lecture's title with its files, when it starts within eight hours (or is on) and there are files.
    pub(crate) fn lecture_files(&self) -> Option<(String, Vec<Material>)> {
        let ev = self.next_lecture().filter(|e| e.at_ms - self.now <= 8 * 3_600_000)?;
        let files = self.materials_for(&ev.title);
        (!files.is_empty()).then_some((ev.title, files))
    }

    /// Asks the platform layer to look for the files of the next lecture and of tomorrow's first one, every ten minutes.
    pub(super) fn ask_materials(&mut self, now: i64) {
        if now - self.materials_checked < 30_000 {
            return;
        }
        self.materials_checked = now;
        let mut titles: Vec<String> = vec![];
        titles.extend(self.next_lecture().map(|e| e.title));
        titles.extend(self.tomorrow_items().into_iter().find(|i| i.kind == Kind::Event && !i.all_day).map(|e| e.title));
        for t in titles {
            if island_core::materials::keywords(&t).is_empty() {
                continue;
            }
            let key = island_core::materials::fold(&t);
            if self.materials_asked.get(&key).is_none_or(|at| now - at >= ASK_AGAIN_MS) {
                self.materials_asked.insert(key, now);
                self.pending.push(Command::FindMaterials(t));
            }
        }
    }

    /// "slides-03.pdf" for a card line: the best file of the lecture.
    pub(crate) fn files_line(&self, items: &[Item]) -> Option<String> {
        let ev = items.iter().find(|i| i.kind == Kind::Event && !i.all_day)?;
        let f = self.materials_for(&ev.title);
        let best = f.first()?;
        Some(format!("Files for {}: {}{}", ev.title.split_whitespace().next().unwrap_or(&ev.title), best.name, if f.len() > 1 { format!(" +{}", f.len() - 1) } else { String::new() }))
    }
}
