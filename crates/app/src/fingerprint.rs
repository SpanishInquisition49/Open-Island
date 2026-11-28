//! A number that says what a scene draws, so that a frame equal to the one on screen need not be rendered again.
//!
//! Everything the renderer reads goes into it. The structures are taken apart field by field, by name: a field that a
//! new version of vello adds does not compile until it is looked at here, instead of being missed and leaving an old
//! picture on the screen.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::mem::discriminant;
use vello::low_level::vello_encoding::{Encoding, GlyphRun, Patch, Resources, StreamOffsets, Transform};
use vello::peniko::color::cache_key::BitHash;
use vello::kurbo::Stroke;
use vello::peniko::{FontData, ImageData, Style};
use vello::Scene;

pub fn of(scene: &Scene) -> u64 {
    let mut h = DefaultHasher::new();
    encoding(scene.encoding(), &mut h);
    h.finish()
}

/// A list, with its length: where one ends and the next begins is part of what they say.
fn list<T: bytemuck::NoUninit>(items: &[T], h: &mut DefaultHasher) {
    items.len().hash(h);
    h.write(bytemuck::cast_slice(items));
}

fn encoding(e: &Encoding, h: &mut DefaultHasher) {
    let Encoding { path_tags, path_data, draw_tags, draw_data, transforms, styles, resources, n_paths, n_path_segments, n_clips, n_open_clips, flags } = e;
    list(path_tags, h);
    list(path_data, h);
    list(draw_tags, h);
    list(draw_data, h);
    list(transforms, h);
    list(styles, h);
    (n_paths, n_path_segments, n_clips, n_open_clips, flags).hash(h);
    let Resources { patches, color_stops, glyphs, glyph_runs, normalized_coords } = resources;
    patches.len().hash(h);
    patches.iter().for_each(|p| patch(p, h));
    color_stops.len().hash(h);
    color_stops.iter().for_each(|s| s.bit_hash(h));
    glyphs.len().hash(h);
    glyphs.iter().for_each(|g| (g.id, g.x.to_bits(), g.y.to_bits()).hash(h));
    glyph_runs.len().hash(h);
    glyph_runs.iter().for_each(|r| glyph_run(r, h));
    list(normalized_coords, h);
}

fn patch(p: &Patch, h: &mut DefaultHasher) {
    discriminant(p).hash(h);
    match p {
        Patch::Ramp { draw_data_offset, stops, extend, interpolation_alpha_space } => {
            (draw_data_offset, stops).hash(h);
            (discriminant(extend), discriminant(interpolation_alpha_space)).hash(h);
        }
        Patch::GlyphRun { index } => index.hash(h),
        Patch::Image { draw_data_offset, image } => {
            draw_data_offset.hash(h);
            // the pixels are known by their number (see `island_ui`'s pictures)
            let ImageData { data, format, alpha_type, width, height } = image;
            (data.id(), discriminant(format), discriminant(alpha_type), width, height).hash(h);
        }
    }
}

fn transform(t: Option<&Transform>, h: &mut DefaultHasher) {
    match t {
        Some(t) => list(std::slice::from_ref(t), h),
        None => u8::MAX.hash(h),
    }
}

fn glyph_run(run: &GlyphRun, h: &mut DefaultHasher) {
    let GlyphRun { font, transform: global, glyph_transform, brush_transform, font_size, font_embolden, hint, normalized_coords, style, glyphs, stream_offsets } = run;
    let FontData { data, index } = font;
    (data.id(), index).hash(h);
    transform(Some(global), h);
    transform(glyph_transform.as_ref(), h);
    transform(brush_transform.as_ref(), h);
    (font_size.to_bits(), hint, normalized_coords, glyphs).hash(h);
    let e = font_embolden;
    (e.amount.xx.to_bits(), e.amount.yy.to_bits(), discriminant(&e.join), e.miter_limit.to_bits(), e.tolerance.to_bits()).hash(h);
    paint(style, h);
    let StreamOffsets { path_tags, path_data, draw_tags, draw_data, transforms, styles } = stream_offsets;
    (path_tags, path_data, draw_tags, draw_data, transforms, styles).hash(h);
}

/// Whether glyphs are filled or stroked, and how.
fn paint(style: &Style, h: &mut DefaultHasher) {
    discriminant(style).hash(h);
    match style {
        Style::Fill(rule) => discriminant(rule).hash(h),
        Style::Stroke(Stroke { width, join, miter_limit, start_cap, end_cap, dash_pattern, dash_offset }) => {
            (width.to_bits(), discriminant(join), miter_limit.to_bits(), discriminant(start_cap), discriminant(end_cap), dash_offset.to_bits()).hash(h);
            dash_pattern.len().hash(h);
            dash_pattern.iter().for_each(|d| d.to_bits().hash(h));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use island_ui::text::Fonts;
    use island_ui::{Config, Island};
    use vello::kurbo::{Affine, Rect};
    use vello::peniko::{Blob, Color, Fill, Gradient, ImageAlphaType, ImageBrush, ImageFormat};

    fn scene(draw: impl FnOnce(&mut Scene)) -> u64 {
        let mut s = Scene::new();
        draw(&mut s);
        of(&s)
    }

    fn fill(s: &mut Scene, x: f64, colour: Color) {
        s.fill(Fill::NonZero, Affine::IDENTITY, colour, None, &Rect::new(x, 0.0, x + 10.0, 10.0));
    }

    #[test]
    fn what_looks_different_has_another_number() {
        let (red, blue) = (Color::from_rgb8(255, 0, 0), Color::from_rgb8(0, 0, 255));
        let picture = |data: &Blob<u8>| ImageBrush::new(ImageData { data: data.clone(), format: ImageFormat::Rgba8, alpha_type: ImageAlphaType::Alpha, width: 1, height: 1 });
        let (one, other): (Blob<u8>, Blob<u8>) = (vec![1u8; 4].into(), vec![1u8; 4].into());
        let gradient = |end: Color| Gradient::new_linear((0.0, 0.0), (10.0, 0.0)).with_stops([red, end]);
        let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
        let all = [
            scene(|_| {}),
            scene(|s| fill(s, 0.0, red)),
            scene(|s| fill(s, 0.5, red)),
            scene(|s| fill(s, 0.0, blue)),
            scene(|s| {
                fill(s, 0.0, red);
                fill(s, 0.0, red);
            }),
            scene(|s| s.fill(Fill::NonZero, Affine::translate((1.0, 0.0)), red, None, &rect)),
            scene(|s| s.stroke(&Stroke::new(1.0), Affine::IDENTITY, red, None, &rect)),
            scene(|s| s.stroke(&Stroke::new(2.0), Affine::IDENTITY, red, None, &rect)),
            scene(|s| s.fill(Fill::NonZero, Affine::IDENTITY, &gradient(blue), None, &rect)),
            scene(|s| s.fill(Fill::NonZero, Affine::IDENTITY, &gradient(Color::WHITE), None, &rect)),
            scene(|s| s.draw_image(&picture(&one), Affine::IDENTITY)),
            scene(|s| s.draw_image(&picture(&other), Affine::IDENTITY)),
            scene(|s| s.draw_image(&picture(&one).with_alpha(0.5), Affine::IDENTITY)),
            scene(|s| {
                s.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &rect);
                fill(s, 0.0, red);
                s.pop_layer();
            }),
        ];
        for (i, a) in all.iter().enumerate() {
            assert!(!all[i + 1..].contains(a), "scene {i} is taken for a later one");
        }
        assert_eq!(scene(|s| s.draw_image(&picture(&one), Affine::IDENTITY)), all[10], "the same again");
    }

    /// The island itself: two frames of a clock within the same minute are one picture, and what changes it (the
    /// minute, a word on a card, a glyph) is seen.
    #[test]
    fn a_resting_island_draws_the_same_picture_until_something_changes() {
        let fonts = Fonts::load_system(None);
        if !fonts.has_text_font() {
            return;
        }
        let start = 1_800_000_000_000; // a whole minute
        let mut island = Island::new(Config::default());
        let mut t = start;
        let mut frame = |island: &mut Island, ms: i64| {
            let mut f = island.tick(t, 16.0);
            for _ in 0..ms / 16 {
                t += 16;
                f = island.tick(t, 16.0);
            }
            let mut s = Scene::new();
            island.draw(&mut s, &fonts, f.surface, 1.0);
            of(&s)
        };
        let resting = frame(&mut island, 5_000);
        assert_eq!(frame(&mut island, 1_000), resting);
        assert_eq!(frame(&mut island, 20_000), resting);
        let later = frame(&mut island, 40_000);
        assert_ne!(later, resting, "another minute");
        island.push(start + 70_000, island_ui::model::Input::Toast { summary: "Hello".into(), body: String::new() });
        let a = frame(&mut island, 1_000);
        assert_ne!(a, later, "a card");
    }
}
