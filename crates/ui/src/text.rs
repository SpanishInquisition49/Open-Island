//! Fonts and text: loading, measuring and drawing. Text is laid out left to right with the font's advance
//! widths (no shaping): enough for the island's Latin UI text; complex scripts fall back to the first
//! font that has the glyph.

use skrifa::{instance::Size, MetadataProvider};
use std::collections::HashMap;
use std::sync::Arc;
use vello::kurbo::Affine;
use vello::peniko::{Blob, Color, Fill, FontData};
use vello::{Glyph, Scene};

const TABLER: &[u8] = include_bytes!("../assets/tabler.ttf");

/// Lower case without spaces, dashes and underscores: "JetBrains Mono" and "jetbrainsmono" compare equal.
fn normalise(name: &str) -> String {
    name.chars().filter(|c| !matches!(c, ' ' | '-' | '_' | '.')).flat_map(|c| c.to_lowercase()).collect()
}

/// The installed family closest to what the user typed: the same name ignoring case and spacing, else a
/// family that starts with it, else one that contains it ("jetbrains" finds "JetBrainsMono Nerd Font").
/// Shorter names win among equals, so "Inter" gives "Inter", not "Inter Tight".
pub fn resolve_family(db: &fontdb::Database, wanted: &str) -> Option<String> {
    let w = normalise(wanted);
    if w.is_empty() {
        return None;
    }
    let mut best: Option<(u8, usize, String)> = None;
    for face in db.faces() {
        for (name, _) in &face.families {
            let n = normalise(name);
            let rank = if n == w {
                0
            } else if n.starts_with(&w) {
                1
            } else if n.contains(&w) {
                2
            } else {
                continue;
            };
            if best.as_ref().is_none_or(|(r, l, _)| (rank, n.len()) < (*r, *l)) {
                best = Some((rank, n.len(), name.clone()));
            }
        }
    }
    best.map(|(_, _, n)| n)
}

fn shellexpand_home(p: &str) -> std::path::PathBuf {
    match (p.strip_prefix("~/"), std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))) {
        (Some(rest), Some(h)) => std::path::Path::new(&h).join(rest),
        _ => p.into(),
    }
}

/// One loaded face.
pub struct Face {
    data: FontData,
    bytes: Arc<Vec<u8>>,
    index: u32,
}

impl Face {
    fn new(bytes: Vec<u8>, index: u32) -> Option<Self> {
        skrifa::FontRef::from_index(&bytes, index).ok()?;
        let bytes = Arc::new(bytes);
        let blob = Blob::new(bytes.clone() as Arc<dyn AsRef<[u8]> + Send + Sync>);
        Some(Self { data: FontData::new(blob, index), bytes, index })
    }

    fn font(&self) -> skrifa::FontRef<'_> {
        skrifa::FontRef::from_index(&self.bytes, self.index).expect("validated in new")
    }

    fn glyph(&self, c: char) -> Option<u32> {
        self.font().charmap().map(c).map(|g| g.to_u32()).filter(|g| *g != 0)
    }

    fn advance(&self, gid: u32, size: f32) -> f32 {
        let font = self.font();
        font.glyph_metrics(Size::new(size), skrifa::instance::LocationRef::default())
            .advance_width(skrifa::GlyphId::new(gid))
            .unwrap_or(size * 0.5)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Weight {
    Regular,
    Semibold,
    Bold,
}

/// The fonts the island draws with.
pub struct Fonts {
    text: HashMap<Weight, Vec<Face>>, // each weight: preferred face first, then fallbacks
    icons: Face,
    icon_map: HashMap<&'static str, u32>,
    preferred_found: bool,
    family: Option<String>,
}

/// Preferred UI families, first match wins: the macOS look needs SF Pro (not redistributable, so it is
/// picked up when installed), then the closest common sans-serifs per platform.
const FAMILIES: &[&str] = &[
    "SF Pro Text",
    "SF Pro Display",
    "SF Pro",
    ".AppleSystemUIFont",
    "Helvetica Neue",
    "Inter",
    "Segoe UI Variable",
    "Segoe UI",
    "Roboto",
    "Noto Sans",
    "DejaVu Sans",
    "Arial",
];

impl Fonts {
    /// Loads the system fonts. `preferred` is a family name ("Inter", "SF Pro Text") or the path of a font
    /// file (.ttf, .otf, .ttc); it is tried before the built-in list.
    pub fn load_system(preferred: Option<&str>) -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let mut family = preferred.map(str::trim).filter(|p| !p.is_empty()).map(str::to_string);
        if let Some(p) = family.clone().filter(|p| p.contains('/') || p.contains('\\') || [".ttf", ".otf", ".ttc"].iter().any(|e| p.to_lowercase().ends_with(e))) {
            let before = db.len();
            family = match db.load_font_file(shellexpand_home(&p)) {
                Ok(()) => db.faces().nth(before).and_then(|f| f.families.first().map(|(n, _)| n.clone())),
                Err(e) => {
                    log::warn!("font file {p}: {e}");
                    None
                }
            };
        }
        Self::from_db_with(&db, family.as_deref())
    }

    pub fn from_db(db: &fontdb::Database) -> Self {
        Self::from_db_with(db, None)
    }

    /// The installed family the requested name resolved to.
    pub fn family(&self) -> Option<&str> {
        self.family.as_deref()
    }

    /// True when the requested family was found (always true when none was requested).
    pub fn preferred_found(&self) -> bool {
        self.preferred_found
    }

    pub fn from_db_with(db: &fontdb::Database, preferred: Option<&str>) -> Self {
        let mut text = HashMap::new();
        let resolved = preferred.and_then(|p| resolve_family(db, p));
        let preferred_found = preferred.is_none() || resolved.is_some();
        let preferred = resolved.as_deref();
        let families: Vec<&str> = preferred.into_iter().chain(FAMILIES.iter().copied()).collect();
        for (weight, w) in [(Weight::Regular, 400), (Weight::Semibold, 600), (Weight::Bold, 700)] {
            let mut faces = Vec::new();
            for fam in families.iter() {
                let q = fontdb::Query {
                    families: &[fontdb::Family::Name(fam)],
                    weight: fontdb::Weight(w),
                    stretch: fontdb::Stretch::Normal,
                    style: fontdb::Style::Normal,
                };
                if let Some(id) = db.query(&q) {
                    if let Some(face) = Self::face_of(db, id) {
                        faces.push(face);
                    }
                }
                if faces.len() >= 3 {
                    break; // preferred face plus two fallbacks is plenty
                }
            }
            if faces.is_empty() {
                let q = fontdb::Query {
                    families: &[fontdb::Family::SansSerif],
                    weight: fontdb::Weight(w),
                    ..Default::default()
                };
                if let Some(face) = db.query(&q).and_then(|id| Self::face_of(db, id)) {
                    faces.push(face);
                }
            }
            text.insert(weight, faces);
        }
        let icons = Face::new(TABLER.to_vec(), 0).expect("bundled icon font is valid");
        let icon_map = crate::icon_table::ICONS.iter().copied().collect();
        Self { text, icons, icon_map, preferred_found, family: resolved }
    }

    fn face_of(db: &fontdb::Database, id: fontdb::ID) -> Option<Face> {
        db.with_face_data(id, |data, index| Face::new(data.to_vec(), index))?
    }

    /// True when at least one text font was found.
    pub fn has_text_font(&self) -> bool {
        self.text.values().any(|f| !f.is_empty())
    }

    fn faces(&self, w: Weight) -> &[Face] {
        match self.text.get(&w) {
            Some(f) if !f.is_empty() => f,
            _ => self.text.get(&Weight::Regular).map(|v| v.as_slice()).unwrap_or(&[]),
        }
    }

    /// Lays `text` out on one line: (face index, glyph id, x) per glyph and the total width.
    fn layout(&self, text: &str, size: f32, w: Weight) -> (Vec<(usize, Glyph)>, f32) {
        let faces = self.faces(w);
        let mut out = Vec::with_capacity(text.len());
        let mut x = 0.0f32;
        for c in text.chars() {
            let mut placed = false;
            for (fi, f) in faces.iter().enumerate() {
                if let Some(g) = f.glyph(c) {
                    out.push((fi, Glyph { id: g, x, y: 0.0 }));
                    x += f.advance(g, size);
                    placed = true;
                    break;
                }
            }
            if !placed {
                x += size * 0.5; // a gap for a glyph no font has
            }
        }
        (out, x)
    }

    pub fn measure(&self, text: &str, size: f32, w: Weight) -> f32 {
        self.layout(text, size, w).1
    }

    /// `text` cut to `max_w` with an ellipsis when it does not fit.
    pub fn ellipsize(&self, text: &str, size: f32, w: Weight, max_w: f32) -> String {
        if self.measure(text, size, w) <= max_w {
            return text.to_string();
        }
        let dots = self.measure("…", size, w);
        let mut out = String::new();
        let mut width = 0.0;
        for c in text.chars() {
            let cw = self.measure(c.encode_utf8(&mut [0; 4]), size, w);
            if width + cw + dots > max_w {
                break;
            }
            out.push(c);
            width += cw;
        }
        out.push('…');
        out
    }

    /// Draws one line with its baseline at `baseline_y`, left edge at `x`.
    pub fn draw(&self, scene: &mut Scene, text: &str, x: f64, baseline_y: f64, size: f32, w: Weight, color: Color) {
        let (glyphs, _) = self.layout(text, size, w);
        let faces = self.faces(w);
        for fi in 0..faces.len() {
            let run: Vec<Glyph> = glyphs.iter().filter(|(i, _)| *i == fi).map(|(_, g)| *g).collect();
            if run.is_empty() {
                continue;
            }
            scene
                .draw_glyphs(&faces[fi].data)
                .font_size(size)
                .transform(Affine::translate((x, baseline_y)))
                .brush(color)
                .draw(Fill::NonZero, run.into_iter());
        }
    }

    /// Draws a Tabler icon centred in a `size` square at (x, y) (top-left). Unknown names draw nothing.
    pub fn draw_icon(&self, scene: &mut Scene, name: &str, x: f64, y: f64, size: f32, color: Color) {
        let Some(&cp) = self.icon_map.get(name) else { return };
        let Some(c) = char::from_u32(cp) else { return };
        let Some(g) = self.icons.glyph(c) else { return };
        let adv = self.icons.advance(g, size);
        // Tabler glyphs sit on a 1 em box with the baseline at 0.8 em from the top (ascent ~ 0.88)
        let gx = x + (size - adv) as f64 / 2.0;
        let gy = y + size as f64 * 0.86;
        scene
            .draw_glyphs(&self.icons.data)
            .font_size(size)
            .transform(Affine::translate((gx, gy)))
            .brush(color)
            .draw(Fill::NonZero, std::iter::once(Glyph { id: g, x: 0.0, y: 0.0 }));
    }

    pub fn has_icon(&self, name: &str) -> bool {
        self.icon_map.contains_key(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preferred_family_comes_first_and_an_unknown_one_is_reported() {
        let mut db = fontdb::Database::new();
        db.load_font_data(TABLER.to_vec());
        let family = db.faces().next().unwrap().families[0].0.clone();
        let f = Fonts::from_db_with(&db, Some(&family));
        assert!(f.has_text_font() && f.preferred_found(), "the font's own family is found");
        let f = Fonts::from_db_with(&db, Some("No Such Family"));
        assert!(!f.preferred_found());
        assert!(Fonts::from_db_with(&db, None).preferred_found(), "nothing requested counts as found");
    }

    #[test]
    fn family_names_are_matched_loosely() {
        let mut db = fontdb::Database::new();
        db.load_font_data(TABLER.to_vec());
        let real = db.faces().next().unwrap().families[0].0.clone();
        let squashed = real.to_uppercase().replace(['-', ' '], "");
        assert_eq!(resolve_family(&db, &squashed).as_deref(), Some(real.as_str()), "case and spacing do not matter");
        assert_eq!(resolve_family(&db, &real[..4]).as_deref(), Some(real.as_str()), "a prefix finds it");
        assert_eq!(resolve_family(&db, &real[1..real.len() - 1]).as_deref(), Some(real.as_str()), "so does a part of the name");
        assert_eq!(resolve_family(&db, "No Such Family"), None);
        assert_eq!(resolve_family(&db, "  "), None);
        let f = Fonts::from_db_with(&db, Some(&real[..4]));
        assert!(f.preferred_found());
        assert_eq!(f.family(), Some(real.as_str()));
    }

    #[test]
    fn icon_table_resolves_in_the_bundled_font() {
        let f = Fonts::from_db(&fontdb::Database::new());
        for (name, cp) in crate::icon_table::ICONS {
            let c = char::from_u32(*cp).unwrap();
            assert!(f.icons.glyph(c).is_some(), "icon {name} (U+{cp:X}) is missing from the subset font");
        }
        assert!(f.has_icon("player-play") && !f.has_icon("nope"));
    }
}
