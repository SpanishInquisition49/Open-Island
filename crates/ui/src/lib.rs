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
