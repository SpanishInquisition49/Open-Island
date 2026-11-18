//! The macOS look: a pure black island, white text, the system colours for alerts.

use vello::peniko::Color;

pub const BG: Color = Color::from_rgba8(0, 0, 0, 255);
pub const FG: Color = Color::from_rgba8(255, 255, 255, 255);
pub const DIM: Color = Color::from_rgba8(255, 255, 255, 0x99);
pub const TRACK: Color = Color::from_rgba8(255, 255, 255, 0x33);
pub const CHIP: Color = Color::from_rgba8(255, 255, 255, 0x1f);
pub const HOT_CHIP: Color = Color::from_rgba8(255, 255, 255, 0x3d);
pub const ACCENT: Color = Color::from_rgba8(255, 255, 255, 255);
pub const ON_ACCENT: Color = Color::from_rgba8(0, 0, 0, 255);
/// macOS systemRed / systemOrange / systemGreen (dark appearance).
pub const RED: Color = Color::from_rgba8(0xff, 0x45, 0x3a, 255);
pub const ORANGE: Color = Color::from_rgba8(0xff, 0x9f, 0x0a, 255);
pub const GREEN: Color = Color::from_rgba8(0x30, 0xd1, 0x58, 255);

pub fn with_alpha(c: Color, k: f32) -> Color {
    let [r, g, b, a] = c.components;
    Color::new([r, g, b, (a * k).clamp(0.0, 1.0)])
}

pub fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgba8(r, g, b, 255)
}
