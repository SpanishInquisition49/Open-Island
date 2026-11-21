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
    /// Who reports it: several activities run side by side, each under its own id ("" is the shared slot of `progress`).
    pub id: String,
    pub label: String,
    pub frac: Option<f64>,
    pub state: State,
    pub updated_ms: i64,
    /// What a script said about its own activity (`open-island activity`): a status line, an icon, a colour.
    pub detail: String,
    pub icon: Option<String>,
    pub tint: Option<(u8, u8, u8)>,
}

/// An activity reported by a script.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Activity {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub icon: Option<String>,
    pub tint: Option<(u8, u8, u8)>,
    /// 0..=1
    pub frac: Option<f64>,
}

/// A colour by name (the system palette) or `#rrggbb`.
pub fn parse_color(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim().to_ascii_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            let v = u32::from_str_radix(hex, 16).ok()?;
            return Some(((v >> 16) as u8, (v >> 8) as u8, v as u8));
        }
        return None;
    }
    Some(match s.as_str() {
        "red" => (0xff, 0x45, 0x3a),
        "orange" => (0xff, 0x9f, 0x0a),
        "yellow" => (0xff, 0xd6, 0x0a),
        "green" => (0x30, 0xd1, 0x58),
        "teal" | "cyan" => (0x64, 0xd2, 0xff),
        "blue" => (0x0a, 0x84, 0xff),
        "purple" => (0xbf, 0x5a, 0xf2),
        "pink" => (0xff, 0x37, 0x5f),
        "white" => (255, 255, 255),
        "gray" | "grey" => (0x8e, 0x8e, 0x93),
        _ => return None,
    })
}

/// A run that stops reporting for this long is dropped.
pub const STALE_MS: i64 = 120_000;

#[derive(Default)]
pub struct Tracker {
    /// The one the pill and the card show: a running one, or one that has just ended (shown until `flash_until`).
    pub op: Option<Op>,
    /// Others running beside it, oldest first. One that ends takes the main place for a moment, so its result is seen.
    pub also: Vec<Op>,
    /// The card (rather than the side bubble) is shown until this instant.
    pub flash_until: i64,
}

fn blank(id: &str, now: i64) -> Op {
    Op { id: id.to_string(), label: String::new(), frac: None, state: State::Run, updated_ms: now, detail: String::new(), icon: None, tint: None }
}

impl Tracker {
    /// The running one with this id, wherever it is.
    fn running_mut(&mut self, id: &str) -> Option<&mut Op> {
        if let Some(o) = self.op.as_mut().filter(|o| o.state == State::Run && o.id == id) {
            return Some(o);
        }
        self.also.iter_mut().find(|o| o.id == id)
    }

    /// Every activity that is running, the main one first.
    pub fn running(&self) -> Vec<&Op> {
        self.op.iter().filter(|o| o.state == State::Run).chain(self.also.iter()).collect()
    }

    /// Puts a running op where it goes: the main place when that is free (the card announces it once), else beside it.
    fn place(&mut self, now: i64, op: Op) {
        if self.op.as_ref().is_some_and(|o| o.state == State::Run) {
            self.also.push(op);
        } else {
            self.op = Some(op);
            self.flash_until = now + 3000; // announce the start once; updates stay quiet
        }
    }

    pub fn report(&mut self, now: i64, payload: &str) {
        let (frac, label) = parse(payload);
        self.upsert(now, "", &label, frac);
    }

    fn upsert(&mut self, now: i64, id: &str, label: &str, frac: Option<f64>) {
        if let Some(o) = self.running_mut(id) {
            if !label.is_empty() {
                o.label = label.to_string();
            }
            o.frac = frac;
            o.updated_ms = now;
            return;
        }
        let mut op = blank(id, now);
        op.label = label.to_string();
        op.frac = frac;
        self.place(now, op);
    }

    pub fn finish(&mut self, now: i64, state: State, label: &str) {
        self.finish_id(now, state, "", label);
    }

    /// An op ends: it takes the main place for a moment so that the result is seen, and what was running there waits beside it.
    fn finish_id(&mut self, now: i64, state: State, id: &str, label: &str) {
        let mut op = if self.op.as_ref().is_some_and(|o| o.id == id) {
            self.op.take().unwrap_or_else(|| blank(id, now))
        } else if let Some(at) = self.also.iter().position(|o| o.id == id) {
            self.also.remove(at)
        } else {
            blank(id, now)
        };
        if let Some(main) = self.op.take().filter(|o| o.state == State::Run) {
            self.also.insert(0, main);
        }
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

    /// A script's activity starts or goes on (the card announces it once, updates stay quiet).
    pub fn run_activity(&mut self, now: i64, a: &Activity) {
        self.upsert(now, &a.id, &a.label, a.frac.map(|f| f.clamp(0.0, 1.0)));
        if let Some(o) = self.running_mut(&a.id) {
            o.detail = a.detail.clone();
            o.icon = a.icon.clone();
            o.tint = a.tint;
        }
    }

    /// A script's activity ends well or badly; what it says now replaces the status line.
    pub fn finish_activity(&mut self, now: i64, state: State, a: &Activity) {
        let known_icon = self.running_mut(&a.id).and_then(|o| o.icon.clone());
        self.finish_id(now, state, &a.id, &a.label);
        if let Some(o) = self.op.as_mut() {
            o.detail = a.detail.clone();
            o.icon = a.icon.clone().or(known_icon).or(o.icon.take());
            o.tint = a.tint;
        }
    }

    pub fn clear(&mut self) {
        self.op = None;
        self.also.clear();
        self.flash_until = 0;
    }

    /// Forgets one activity (a session that ended); an empty id forgets everything.
    pub fn clear_id(&mut self, id: &str) {
        if id.is_empty() {
            return self.clear();
        }
        if self.op.as_ref().is_some_and(|o| o.id == id) {
            self.op = None;
            self.flash_until = 0;
        }
        self.also.retain(|o| o.id != id);
        self.promote();
    }

    fn promote(&mut self) {
        if self.op.is_none() && !self.also.is_empty() {
            self.op = Some(self.also.remove(0));
        }
    }

    pub fn expire(&mut self, now: i64) {
        self.also.retain(|o| now - o.updated_ms <= STALE_MS);
        let drop = match &self.op {
            Some(o) if o.state == State::Run => now - o.updated_ms > STALE_MS,
            Some(_) => now > self.flash_until,
            None => false,
        };
        if drop {
            self.op = None;
        }
        self.promote();
    }

    pub fn flashing(&self, now: i64) -> bool {
        self.op.is_some() && now < self.flash_until
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_by_name_or_hex() {
        assert_eq!(parse_color("Orange"), Some((0xff, 0x9f, 0x0a)));
        assert_eq!(parse_color("#0a84ff"), Some((0x0a, 0x84, 0xff)));
        assert_eq!(parse_color("#fff"), None);
        assert_eq!(parse_color("mauve"), None);
    }

    #[test]
    fn an_activity_carries_its_look_and_ends_with_a_word() {
        let mut t = Tracker::default();
        let a = Activity { id: String::new(), label: "CI main".into(), detail: "building".into(), icon: Some("git-branch".into()), tint: Some((1, 2, 3)), frac: Some(0.4) };
        t.run_activity(0, &a);
        let o = t.op.clone().unwrap();
        assert_eq!((o.label.as_str(), o.detail.as_str(), o.icon.as_deref(), o.tint, o.frac, o.state), ("CI main", "building", Some("git-branch"), Some((1, 2, 3)), Some(0.4), State::Run));
        assert!(t.flashing(1000));
        t.run_activity(5000, &Activity { detail: "testing".into(), ..a.clone() });
        assert!(!t.flashing(5001), "an update does not flash again");
        assert_eq!(t.op.as_ref().unwrap().detail, "testing");
        t.finish_activity(9000, State::Fail, &Activity { label: "CI main".into(), detail: "2 tests failed".into(), ..Default::default() });
        let o = t.op.clone().unwrap();
        assert_eq!((o.state, o.detail.as_str()), (State::Fail, "2 tests failed"));
        assert_eq!(o.icon.as_deref(), Some("git-branch"), "the icon stays");
    }

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

    fn act(id: &str, label: &str) -> Activity {
        Activity { id: id.into(), label: label.into(), ..Default::default() }
    }

    #[test]
    fn activities_with_their_own_ids_run_side_by_side() {
        let mut t = Tracker::default();
        t.run_activity(0, &act("a", "Claude · site"));
        t.run_activity(10, &act("b", "Codex · api"));
        assert_eq!(t.running().iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(t.op.as_ref().unwrap().id, "a", "the first one keeps the main place");
        t.run_activity(20, &Activity { detail: "editing".into(), ..act("b", "Codex · api") });
        assert_eq!(t.also[0].detail, "editing", "an update goes to its own activity");
        assert_eq!(t.running().len(), 2);
        // the second one fails: it is seen for a moment, the first goes on beside it
        t.finish_activity(30, State::Fail, &Activity { detail: "tests failed".into(), ..act("b", "") });
        assert_eq!((t.op.as_ref().unwrap().id.as_str(), t.op.as_ref().unwrap().state), ("b", State::Fail));
        assert_eq!(t.also.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["a"]);
        assert_eq!(t.running().len(), 1);
        t.expire(30 + 7000);
        assert_eq!(t.op.as_ref().unwrap().id, "a", "when the result has been seen the running one is main again");
        assert!(t.also.is_empty());
    }

    #[test]
    fn the_shared_slot_does_not_overwrite_an_agent() {
        let mut t = Tracker::default();
        t.run_activity(0, &act("claude", "Claude"));
        t.report(5, "40 backup");
        assert_eq!(t.running().len(), 2, "a shell command and an agent are two things");
        t.finish(10, State::Done, "backup");
        assert_eq!(t.op.as_ref().unwrap().label, "backup");
        assert_eq!(t.also[0].id, "claude");
    }

    #[test]
    fn one_activity_can_be_cleared_and_a_silent_one_goes_stale() {
        let mut t = Tracker::default();
        t.run_activity(0, &act("a", "A"));
        t.run_activity(0, &act("b", "B"));
        t.clear_id("a");
        assert_eq!(t.op.as_ref().unwrap().id, "b", "the next one takes the place");
        t.run_activity(1000, &act("c", "C"));
        t.expire(STALE_MS + 500);
        assert_eq!(t.running().iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["c"], "b stopped reporting, c did not");
        t.clear_id("");
        assert!(t.running().is_empty());
    }
}
