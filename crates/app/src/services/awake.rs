//! Keeping the machine awake. The choice is written down the moment it is made, so it is still there after a restart or
//! a crash. What holds the machine awake never outlives the island: on Linux and macOS it is a program that ends with
//! it (and so none is left behind, holding the machine awake while the button says off), on Windows a thread's state.

use std::path::Path;

/// Whether the machine was being kept awake, as written in `memory`.
pub fn remembered(memory: Option<&Path>) -> bool {
    memory.and_then(|p| std::fs::read_to_string(p).ok()).is_some_and(|s| s.trim() == "on")
}

pub fn remember(memory: Option<&Path>, on: bool) {
    if let Some(p) = memory {
        if let Err(e) = std::fs::write(p, if on { "on" } else { "off" }) {
            log::warn!("keep awake will not be remembered: {e}");
        }
    }
}

/// The file of the state folder it is written in.
pub fn memory() -> Option<std::path::PathBuf> {
    super::state_file("keep-awake")
}

#[cfg(unix)]
pub use holder::Holder;

#[cfg(unix)]
mod holder {
    use super::{remember, remembered};
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};

    /// A program that keeps the machine awake for as long as it runs. Its input is a pipe from the island, which the
    /// system closes when the island is gone, however it went: a program that ends with its input ends with the island.
    pub struct Holder {
        program: String,
        args: Vec<String>,
        memory: Option<PathBuf>,
        child: Option<Child>,
    }

    impl Holder {
        /// Holds again at once when it was held the last time.
        pub fn restore(memory: Option<PathBuf>, program: &str, args: &[&str]) -> Self {
            let mut h = Self { program: program.into(), args: args.iter().map(|a| a.to_string()).collect(), memory, child: None };
            if remembered(h.memory.as_deref()) {
                h.hold();
            }
            h
        }

        fn hold(&mut self) {
            match Command::new(&self.program).args(&self.args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
                Ok(c) => self.child = Some(c),
                Err(e) => log::warn!("keep awake: {}: {e}", self.program),
            }
        }

        /// Whether the machine is held awake now. A program that ended by itself holds nothing any more (the choice
        /// stays written: only the person changes it).
        pub fn on(&mut self) -> bool {
            if self.child.as_mut().is_some_and(|c| c.try_wait().ok().flatten().is_some()) {
                self.child = None;
            }
            self.child.is_some()
        }

        /// The button: holds or lets go, and writes down which.
        pub fn toggle(&mut self) {
            if self.on() {
                if let Some(mut c) = self.child.take() {
                    let _ = c.kill();
                    // (this closes its input too, which ends what it had started)
                    let _ = c.wait();
                }
            } else {
                self.hold();
            }
            remember(self.memory.as_deref(), self.child.is_some());
        }

        #[cfg(test)]
        pub(super) fn child(&mut self) -> Option<&mut Child> {
            self.child.as_mut()
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    fn memory_of(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("open-island-awake-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    /// `cat` stands for the inhibitor: it runs until its input ends.
    fn holder(memory: &Path) -> Holder {
        Holder::restore(Some(memory.to_path_buf()), "cat", &[])
    }

    fn ends_soon(c: &mut std::process::Child) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            if c.try_wait().unwrap().is_some() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn the_choice_is_still_there_after_a_restart() {
        let memory = memory_of("restart");
        let mut first = holder(&memory);
        assert!(!first.on(), "off until asked for");
        first.toggle();
        assert!(first.on());
        assert!(remembered(Some(&memory)), "written at once, not on the way out: a crash loses nothing");
        drop(first);
        let mut second = holder(&memory);
        assert!(second.on(), "held again at the next start");
        second.toggle();
        assert!(!second.on() && !remembered(Some(&memory)));
        assert!(!holder(&memory).on(), "and off stays off");
        let _ = std::fs::remove_file(&memory);
    }

    #[test]
    fn what_holds_the_machine_awake_ends_with_the_island() {
        let memory = memory_of("ends");
        let mut h = holder(&memory);
        h.toggle();
        let child = h.child().expect("held");
        assert!(child.try_wait().unwrap().is_none(), "it runs while the island does");
        // what the system does to the pipe when the island is gone, even killed
        drop(child.stdin.take());
        assert!(ends_soon(child), "left behind");
        let _ = std::fs::remove_file(&memory);
    }

    #[test]
    fn one_that_ended_by_itself_shows_as_off_and_is_tried_again_at_the_next_start() {
        let memory = memory_of("died");
        remember(Some(&memory), true);
        let mut h = Holder::restore(Some(memory.clone()), "true", &[]);
        assert!(h.child().is_some_and(ends_soon));
        assert!(!h.on());
        assert!(remembered(Some(&memory)));
        assert!(holder(&memory).on());
        let _ = std::fs::remove_file(&memory);
    }

    #[test]
    fn a_program_that_is_not_there_leaves_it_off() {
        let memory = memory_of("missing");
        let mut h = Holder::restore(Some(memory.clone()), "open-island-no-such-program", &[]);
        h.toggle();
        assert!(!h.on() && !remembered(Some(&memory)));
        let _ = std::fs::remove_file(&memory);
    }
}
