//! Pomodoro cycle: 25 min focus, 5 min break, a 15 min break after every fourth focus.

pub const FOCUS: u64 = 25 * 60;
pub const SHORT: u64 = 5 * 60;
pub const LONG: u64 = 15 * 60;
pub const ROUNDS: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Focus,
    Break,
}

/// After `phase` of round `round` (1-based): the next phase, its round and its length in seconds.
pub fn next(phase: Phase, round: u32) -> (Phase, u32, u64) {
    match phase {
        Phase::Focus => (Phase::Break, round, if round % ROUNDS == 0 { LONG } else { SHORT }),
        Phase::Break => (Phase::Focus, round + 1, FOCUS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle() {
        assert_eq!(next(Phase::Focus, 1), (Phase::Break, 1, SHORT));
        assert_eq!(next(Phase::Break, 1), (Phase::Focus, 2, FOCUS));
        assert_eq!(next(Phase::Focus, 4), (Phase::Break, 4, LONG));
        assert_eq!(next(Phase::Focus, 8), (Phase::Break, 8, LONG));
    }
}
