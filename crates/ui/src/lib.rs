//! open-island UI: the island's state machine and its macOS-style rendering, without windowing.

mod config_check;
mod icon_table;
pub mod clock;
pub mod faces;
pub mod island;
pub mod paint;
pub mod model;
pub mod text;
pub mod theme;
pub use island::{Action, Config, Frame, Island};
pub use config_check::is_calendar_url;

/// True when the bundled icon font has this icon (a script's `--icon` is only used when it does).
pub fn is_known_icon(name: &str) -> bool {
    icon_table::ICONS.iter().any(|(n, _)| *n == name)
}
