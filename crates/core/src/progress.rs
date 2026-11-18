//! Long operations reported by scripts (`island-progress`): the payload "<percent|?> [label...]".

/// Fraction in 0..=1 (None when unknown or not finite) and the label.
pub fn parse(payload: &str) -> (Option<f64>, String) {
    let t = payload.trim();
    let (first, rest) = match t.split_once(char::is_whitespace) {
        Some((a, b)) => (a, b.trim()),
        None => (t, ""),
    };
    if first.is_empty() {
        return (None, String::new());
    }
    let n = first.trim_end_matches('%').parse::<f64>().ok().filter(|n| n.is_finite());
    (n.map(|n| n.clamp(0.0, 100.0) / 100.0), rest.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Run,
    Done,
    Fail,
}

/// One operation at a time: the latest.
#[derive(Clone, Debug)]
pub struct Op {
    pub label: String,
    pub frac: Option<f64>,
    pub state: State,
    pub updated_ms: i64,
}

/// A run that stops reporting for this long is dropped.
pub const STALE_MS: i64 = 120_000;

#[derive(Default)]
pub struct Tracker {
    pub op: Option<Op>,
    /// The card (rather than the side bubble) is shown until this instant.
    pub flash_until: i64,
}

impl Tracker {
    pub fn report(&mut self, now: i64, payload: &str) {
        let (frac, label) = parse(payload);
        let first = !matches!(&self.op, Some(o) if o.state == State::Run);
        let mut op = match self.op.take() {
            Some(o) if o.state == State::Run => o,
            _ => Op { label: String::new(), frac: None, state: State::Run, updated_ms: now },
        };
        if !label.is_empty() {
            op.label = label;
        }
        op.frac = frac;
        op.updated_ms = now;
        self.op = Some(op);
        if first {
            self.flash_until = now + 3000; // announce the start once; updates stay quiet
        }
    }

    pub fn finish(&mut self, now: i64, state: State, label: &str) {
        let mut op = self.op.take().unwrap_or(Op { label: String::new(), frac: None, state, updated_ms: now });
        if !label.is_empty() {
            op.label = label.to_string();
        }
        op.state = state;
        if state == State::Done {
            op.frac = Some(1.0);
        }
        op.updated_ms = now;
        self.op = Some(op);
        self.flash_until = now + if state == State::Fail { 6000 } else { 3500 };
    }

    pub fn clear(&mut self) {
        self.op = None;
        self.flash_until = 0;
    }

    pub fn expire(&mut self, now: i64) {
        let drop = match &self.op {
            Some(o) if o.state == State::Run => now - o.updated_ms > STALE_MS,
            Some(_) => now > self.flash_until,
            None => false,
        };
        if drop {
            self.op = None;
        }
    }

    pub fn flashing(&self, now: i64) -> bool {
        self.op.is_some() && now < self.flash_until
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        assert_eq!(parse("42 Copying photos"), (Some(0.42), "Copying photos".into()));
        assert_eq!(parse("12.5% rsync"), (Some(0.125), "rsync".into()));
        assert_eq!(parse("? Installing"), (None, "Installing".into()));
        assert_eq!(parse("250 x").0, Some(1.0));
        assert_eq!(parse("-5 x").0, Some(0.0));
        assert_eq!(parse("nan x").0, None);
        assert_eq!(parse("inf x").0, None);
        assert_eq!(parse(""), (None, String::new()));
    }

    #[test]
    fn lifecycle() {
        let mut t = Tracker::default();
        t.report(0, "10 Backup");
        assert!(t.flashing(1000));
        assert!(!t.flashing(3000));
        t.report(5000, "20");
        assert!(!t.flashing(5001), "updates do not flash again");
        assert_eq!(t.op.as_ref().unwrap().label, "Backup");
        t.finish(6000, State::Done, "");
        assert!(t.flashing(6001));
        assert_eq!(t.op.as_ref().unwrap().frac, Some(1.0));
        t.expire(6001);
        assert!(t.op.is_some());
        t.expire(10_000);
        assert!(t.op.is_none());
    }

    #[test]
    fn stale_run_is_dropped_and_restarts() {
        let mut t = Tracker::default();
        t.report(0, "5 A");
        t.expire(STALE_MS + 1);
        assert!(t.op.is_none());
        t.report(STALE_MS + 2, "7 B");
        assert!(t.flashing(STALE_MS + 3));
    }
}
