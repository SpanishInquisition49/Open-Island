//! Platform-independent logic of open-island. Nothing here touches the operating system, a window
//! or the clock: every function is a function of its arguments, so all of it is unit-tested.

pub mod bar;
pub mod calendar;
pub mod color;
pub mod duration;
pub mod face;
pub mod ics;
pub mod lyrics;
pub mod notify;
pub mod pomodoro;
pub mod progress;
pub mod spectrum;
pub mod spring;
pub mod system;
pub mod sysmon;
pub mod weather;
