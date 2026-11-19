//! Fonts and text: loading, measuring and drawing. Text is laid out left to right with the font's advance
//! widths (no shaping): enough for the island's UI text and for Japanese, Chinese and Korean titles. A character
//! the UI font lacks is looked for in the other installed fonts (kana and kanji in a Japanese font, Hangul in a
//! Korean one, and so on), loaded the first time one is needed and remembered.

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
#[derive(Clone)]
pub struct Face {
    data: FontData,
    bytes: Arc<Vec<u8>>,
    index: u32,
}

impl Face {
    fn new(bytes: Vec<u8>, index: u32) -> Option<Self> {
        Self::shared(Arc::new(bytes), index)
    }

    /// A face over bytes that other faces share (the faces of one .ttc file).
    fn shared(bytes: Arc<Vec<u8>>, index: u32) -> Option<Self> {
        skrifa::FontRef::from_index(&bytes, index).ok()?;
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

impl Fallback {
    /// Loads the family at the weight (once); the index of its face among `faces`, None when it is not installed.
    fn load_family(&mut self, db: &fontdb::Database, family: &str, weight: Weight) -> Option<usize> {
        let key = (weight, family.to_string());
        if !self.tried.insert(key) {
            return None; // loaded before (its glyphs are searched with the others) or known to be missing
        }
        let q = fontdb::Query { families: &[fontdb::Family::Name(family)], weight: fontdb::Weight(if weight == Weight::Regular { 400 } else { 700 }), ..Default::default() };
        let id = db.query(&q)?;
        self.push_face(db, id, weight)
    }

    fn push_face(&mut self, db: &fontdb::Database, id: fontdb::ID, weight: Weight) -> Option<usize> {
        let info = db.face(id)?;
        let index = info.index;
        // a .ttc holds many faces: read the file once and share the bytes
        let bytes = match &info.source {
            fontdb::Source::File(p) | fontdb::Source::SharedFile(p, _) => match self.files.get(p) {
                Some(b) => b.clone(),
                None => {
                    let b = Arc::new(std::fs::read(p).ok()?);
                    self.files.insert(p.clone(), b.clone());
                    b
                }
            },
            _ => Arc::new(db.with_face_data(id, |d, _| d.to_vec())?),
        };
        let face = Face::shared(bytes, index)?;
        let list = self.faces.entry(weight).or_default();
        list.push(face);
        Some(list.len() - 1)
    }

    /// Last resort: the installed fonts that have not been tried, until one has the character. Each one is read
    /// from disk, so only a few searches are made for the whole run.
    fn scan(&mut self, db: &fontdb::Database, c: char, weight: Weight) -> Option<usize> {
        if self.scans >= 6 {
            return None;
        }
        self.scans += 1;
        let want = if weight == Weight::Regular { 400 } else { 700 };
        let mut ids: Vec<(u16, fontdb::ID, String)> = db
            .faces()
            .filter(|f| f.style == fontdb::Style::Normal && !f.monospaced)
            .filter_map(|f| Some((f.weight.0.abs_diff(want), f.id, f.families.first()?.0.clone())))
            .collect();
        ids.sort_by_key(|(d, ..)| *d);
        let mut tried_files = 0;
        for (_, id, family) in ids {
            if self.tried.contains(&(weight, family.clone())) {
                continue;
            }
            tried_files += 1;
            if tried_files > 80 {
                break;
            }
            self.tried.insert((weight, family));
            if let Some(slot) = self.push_face(db, id, weight) {
                if self.faces[&weight][slot].glyph(c).is_some() {
                    return Some(slot);
                }
                // no use for this character: keep the font out of the way of later lookups
                self.faces.get_mut(&weight).map(Vec::pop);
            }
        }
        None
    }
}

/// Fonts for the fallback come in two weights: regular, and bold for everything heavier (they are large files).
fn fallback_weight(w: Weight) -> Weight {
    if w == Weight::Regular { Weight::Regular } else { Weight::Bold }
}

/// Characters that take no room (zero-width space and joiners, direction marks, variation selectors).
fn invisible(c: char) -> bool {
    matches!(c as u32, 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0xFE00..=0xFE0F | 0xFEFF | 0xE0100..=0xE01EF)
}

/// Which installed families to ask first for a character, by script: kana and kanji in a Japanese font (the
/// shapes of Han characters differ between Japanese and Chinese), Hangul in a Korean one.
fn candidate_families(c: char) -> Vec<&'static str> {
    const JP: &[&str] = &["Noto Sans CJK JP", "Noto Sans JP", "Source Han Sans JP", "Source Han Sans", "IPAexGothic", "IPAGothic", "TakaoPGothic", "Takao Gothic", "VL Gothic", "Hiragino Sans", "Hiragino Kaku Gothic ProN", "Yu Gothic UI", "Yu Gothic", "Meiryo UI", "Meiryo", "MS Gothic"];
    const KR: &[&str] = &["Noto Sans CJK KR", "Noto Sans KR", "Source Han Sans KR", "Malgun Gothic", "Apple SD Gothic Neo", "NanumGothic", "Nanum Gothic"];
    const SC: &[&str] = &["Noto Sans CJK SC", "Noto Sans SC", "Source Han Sans SC", "WenQuanYi Micro Hei", "WenQuanYi Zen Hei", "Microsoft YaHei UI", "Microsoft YaHei", "PingFang SC", "SimHei"];
    const TC: &[&str] = &["Noto Sans CJK TC", "Noto Sans CJK HK", "Noto Sans TC", "Microsoft JhengHei UI", "Microsoft JhengHei", "PingFang TC"];
    const OTHER: &[&str] = &[
        "Noto Sans", "Noto Sans Symbols", "Noto Sans Symbols 2", "Noto Sans Math", "DejaVu Sans", "Segoe UI Symbol", "Apple Symbols", "Arial Unicode MS", "Noto Sans Arabic", "Noto Sans Hebrew", "Noto Sans Thai",
        "Noto Sans Devanagari", "Noto Sans Bengali", "Noto Sans Tamil", "Noto Sans Georgian", "Noto Sans Armenian", "Noto Sans Ethiopic", "Symbola",
    ];
    let u = c as u32;
    let hangul = matches!(u, 0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF);
    let cjk = matches!(u, 0x2E80..=0x2FDF | 0x3000..=0x30FF | 0x31C0..=0x31FF | 0x3200..=0x33FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0x20000..=0x2FA1F);
    let mut v: Vec<&'static str> = vec![];
    if hangul {
        v.extend(KR);
        v.extend(JP);
    } else if cjk {
        v.extend(JP);
        v.extend(SC);
        v.extend(TC);
        v.extend(KR);
    } else {
        v.extend(OTHER);
    }
    v
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
    /// The installed fonts, for the characters the UI font does not have.
    db: fontdb::Database,
    fallback: std::sync::Mutex<Fallback>,
}

/// The fonts found for characters the UI fonts lack, by weight, and what has been tried.
#[derive(Default)]
struct Fallback {
    faces: HashMap<Weight, Vec<Face>>,
    /// (weight, family) already loaded or known to be missing.
    tried: std::collections::HashSet<(Weight, String)>,
    /// (weight, character) -> (index in `faces`, glyph id), or None when no installed font has it.
    found: HashMap<(Weight, char), Option<(usize, u32)>>,
    /// The bytes of font files already read, shared between the faces of one file.
    files: HashMap<std::path::PathBuf, Arc<Vec<u8>>>,
    /// Whole-database searches done (each loads fonts, so there is a limit).
    scans: usize,
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
        Self { text, icons, icon_map, preferred_found, family: resolved, db: db.clone(), fallback: Default::default() }
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

    /// The fallback faces a line of this weight draws with (cheap clones: the bytes are shared).
    fn fallback_faces(&self, w: Weight) -> Vec<Face> {
        self.fallback.lock().map(|f| f.faces.get(&fallback_weight(w)).cloned().unwrap_or_default()).unwrap_or_default()
    }

    /// A glyph for a character the UI fonts lack: (index among the fallback faces of this weight, glyph id,
    /// advance). Looks in the fonts that suit the character's script first, then in any other installed font.
    fn fallback_glyph(&self, c: char, size: f32, w: Weight) -> Option<(usize, u32, f32)> {
        if c.is_control() || c.is_whitespace() {
            return None;
        }
        let mut fb = self.fallback.lock().ok()?;
        let weight = fallback_weight(w);
        let hit = |fb: &Fallback, slot: usize| fb.faces.get(&weight).and_then(|v| v.get(slot)).and_then(|f| f.glyph(c).map(|g| (slot, g, f.advance(g, size))));
        if let Some(known) = fb.found.get(&(weight, c)).copied() {
            return known.and_then(|(slot, _)| hit(&fb, slot));
        }
        // a face loaded earlier may have it
        let loaded = fb.faces.get(&weight).map_or(0, Vec::len);
        let mut found = (0..loaded).find_map(|slot| hit(&fb, slot));
        if found.is_none() {
            for fam in candidate_families(c) {
                if let Some(slot) = fb.load_family(&self.db, fam, weight) {
                    if let Some(h) = hit(&fb, slot) {
                        found = Some(h);
                        break;
                    }
                }
            }
        }
        if found.is_none() {
            found = fb.scan(&self.db, c, weight).and_then(|slot| hit(&fb, slot));
        }
        fb.found.insert((weight, c), found.map(|(slot, g, _)| (slot, g)));
        found
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
                if invisible(c) {
                    continue; // joiners and variation selectors take no room
                }
                match self.fallback_glyph(c, size, w) {
                    Some((slot, g, adv)) => {
                        out.push((faces.len() + slot, Glyph { id: g, x, y: 0.0 }));
                        x += adv;
                    }
                    None => x += size * 0.5, // a gap for a glyph no installed font has
                }
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
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, scene: &mut Scene, text: &str, x: f64, baseline_y: f64, size: f32, w: Weight, color: Color) {
        let (glyphs, _) = self.layout(text, size, w);
        let faces = self.faces(w);
        let fallback = if glyphs.iter().any(|(i, _)| *i >= faces.len()) { self.fallback_faces(w) } else { vec![] };
        for (fi, face) in faces.iter().chain(fallback.iter()).enumerate() {
            let run: Vec<Glyph> = glyphs.iter().filter(|(i, _)| *i == fi).map(|(_, g)| *g).collect();
            if run.is_empty() {
                continue;
            }
            scene
                .draw_glyphs(&face.data)
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

    #[test]
    fn candidates_depend_on_the_script() {
        assert_eq!(candidate_families('あ')[0], "Noto Sans CJK JP", "kana: a Japanese font first");
        assert_eq!(candidate_families('夜')[0], "Noto Sans CJK JP", "and the shared Han characters too");
        assert_eq!(candidate_families('한')[0], "Noto Sans CJK KR", "Hangul: a Korean one");
        assert!(candidate_families('ب').contains(&"Noto Sans Arabic"));
        assert!(!candidate_families('ب').contains(&"Noto Sans CJK JP"), "Arabic does not go looking in the CJK fonts first");
    }

    #[test]
    fn invisible_characters_take_no_room_and_a_missing_one_takes_a_gap() {
        let f = Fonts::from_db(&fontdb::Database::new());
        assert!(invisible('\u{200B}') && invisible('\u{FE0F}') && !invisible('a'));
        // no fonts at all (an empty database): everything is a gap, but the invisible ones are not even that
        assert_eq!(f.measure("a\u{200D}", 10.0, Weight::Regular), 5.0);
        assert_eq!(f.measure("\u{200B}", 10.0, Weight::Regular), 0.0);
    }

    #[test]
    fn japanese_titles_find_a_font_when_one_is_installed() {
        let f = Fonts::load_system(None);
        let have_japanese = f.db.faces().any(|x| x.families.iter().any(|(n, _)| n.contains("CJK JP") || n.contains("Noto Sans JP") || n.contains("Gothic") || n.contains("Yu Gothic") || n.contains("Hiragino")));
        if !have_japanese {
            eprintln!("no Japanese font installed: nothing to check");
            return;
        }
        let faces = f.faces(Weight::Regular).len();
        let (glyphs, width) = f.layout("a夜に駆ける", 20.0, Weight::Semibold);
        assert_eq!(glyphs.len(), 6, "every character is placed");
        assert!(glyphs[1..].iter().all(|(fi, _)| *fi >= faces), "the Japanese ones from a fallback face, not a gap");
        assert!(width > 20.0 * 4.0, "full-width characters take about an em each: {width}");
        assert!(!f.fallback_faces(Weight::Semibold).is_empty() && !f.fallback_faces(Weight::Bold).is_empty(), "the semibold line draws with the bold fallback it was laid out with");
        // a second look finds the same, without loading again
        let (again, w2) = f.layout("夜", 20.0, Weight::Regular);
        assert_eq!(again.len(), 1);
        assert!(w2 > 15.0);
        // Hangul and a bold weight work too
        let (h, _) = f.layout("한글", 20.0, Weight::Bold);
        assert!(h.len() == 2 || h.is_empty(), "either found or a clean gap");
    }
}
