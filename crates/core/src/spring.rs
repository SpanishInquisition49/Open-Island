//! The island's shape animation: two critically-tuned springs (width, height).
//!
//! Semi-implicit Euler with implicit damping. Time steps are clamped to 0..=50 ms and every value is checked to be finite:
//! the Luau original once fed a non-finite size to the compositor after a lock/resume and the shell
//! disabled the layer, leaving a blank rectangle on screen. Here a diverged spring resets to its
//! target instead.

pub const K_GROW: f64 = 520.0;
pub const C_GROW: f64 = 37.0;
pub const K_SHRINK: f64 = 620.0;
pub const C_SHRINK: f64 = 50.0;

#[derive(Clone, Copy, Debug)]
pub struct Axis {
    pub pos: f64,
    pub vel: f64,
}

impl Axis {
    fn step(&mut self, goal: f64, dt: f64) {
        // growing is quick with a small overshoot, shrinking is softer and does not bounce
        let (k, c) = if goal > self.pos { (K_GROW, C_GROW) } else { (K_SHRINK, C_SHRINK) };
        // The spring force is explicit, the damping implicit. Fully explicit damping is unstable as soon as
        // c·dt > 2 (c = 50 and a 50 ms step: every step multiplies the velocity by -1.5), which is what a
        // 1 Hz frame tick after a screen lock did to the Luau original.
        self.vel = (self.vel + k * (goal - self.pos) * dt) / (1.0 + c * dt);
        self.pos += self.vel * dt;
        if !self.pos.is_finite() || !self.vel.is_finite() {
            *self = Axis { pos: goal, vel: 0.0 };
        }
    }

    fn settled(&self, goal: f64) -> bool {
        (goal - self.pos).abs() < 0.4 && self.vel.abs() < 2.0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub w: Axis,
    pub h: Axis,
}

impl Spring {
    pub fn new(w: f64, h: f64) -> Self {
        Self { w: Axis { pos: w, vel: 0.0 }, h: Axis { pos: h, vel: 0.0 } }
    }

    /// Advances towards (tw, th). `dt_ms` outside 0..=50 is clamped; a non-finite target is ignored.
    /// Returns true when both axes are at rest on the target (and snaps them there).
    pub fn step(&mut self, tw: f64, th: f64, dt_ms: f64) -> bool {
        if !(tw.is_finite() && th.is_finite()) {
            return self.settled(self.w.pos, self.h.pos);
        }
        let dt = if dt_ms.is_finite() { dt_ms.clamp(0.0, 50.0) / 1000.0 } else { 0.0 };
        self.w.step(tw, dt);
        self.h.step(th, dt);
        self.settled(tw, th)
    }

    fn settled(&mut self, tw: f64, th: f64) -> bool {
        if self.w.settled(tw) && self.h.settled(th) {
            self.w = Axis { pos: tw, vel: 0.0 };
            self.h = Axis { pos: th, vel: 0.0 };
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(s: &mut Spring, tw: f64, th: f64, frames: usize) -> usize {
        for i in 0..frames {
            if s.step(tw, th, 1000.0 / 120.0) {
                return i;
            }
        }
        frames
    }

    #[test]
    fn grows_and_settles() {
        let mut s = Spring::new(150.0, 28.0);
        let n = run(&mut s, 420.0, 160.0, 600);
        assert!(n < 120, "settles within a second, took {n} frames");
        assert_eq!((s.w.pos, s.h.pos), (420.0, 160.0));
    }

    #[test]
    fn growing_overshoots_a_little_shrinking_does_not() {
        let mut s = Spring::new(150.0, 28.0);
        let mut peak: f64 = 0.0;
        for _ in 0..200 {
            s.step(420.0, 28.0, 8.0);
            peak = peak.max(s.w.pos);
        }
        assert!(peak > 420.0 && peak < 460.0, "peak {peak}");
        let mut s = Spring::new(420.0, 160.0);
        let mut low: f64 = 1e9;
        for _ in 0..200 {
            s.step(150.0, 28.0, 8.0);
            low = low.min(s.w.pos);
        }
        assert!(low >= 149.0, "shrinking must not bounce, went down to {low}");
    }

    #[test]
    fn bad_time_steps_cannot_blow_it_up() {
        for dt in [-5000.0, f64::NAN, f64::INFINITY, -f64::INFINITY, 1e12] {
            let mut s = Spring::new(150.0, 28.0);
            for _ in 0..100 {
                s.step(420.0, 160.0, dt);
                assert!(s.w.pos.is_finite() && s.h.pos.is_finite(), "dt {dt}");
                assert!(s.w.pos.abs() < 1e4 && s.h.pos.abs() < 1e4, "dt {dt}: {:?}", s);
            }
        }
    }

    #[test]
    fn slow_frames_converge() {
        // a 1 Hz frame tick (screen locked) clamps to 50 ms steps: shrinking and growing must still settle
        for (from, to) in [((420.0, 160.0), (150.0, 28.0)), ((150.0, 28.0), (420.0, 160.0))] {
            let mut s = Spring::new(from.0, from.1);
            let mut done = false;
            for _ in 0..200 {
                if s.step(to.0, to.1, 1000.0) {
                    done = true;
                    break;
                }
                assert!(s.w.pos.abs() < 1e3 && s.h.pos.abs() < 1e3, "diverging: {s:?}");
            }
            assert!(done, "never settled: {s:?}");
        }
    }

    #[test]
    fn non_finite_target_is_ignored() {
        let mut s = Spring::new(150.0, 28.0);
        s.step(f64::NAN, f64::INFINITY, 8.0);
        assert_eq!((s.w.pos, s.h.pos), (150.0, 28.0));
    }

    #[test]
    fn diverged_state_recovers() {
        let mut s = Spring::new(150.0, 28.0);
        s.w.vel = f64::INFINITY;
        s.step(300.0, 40.0, 8.0);
        assert_eq!((s.w.pos, s.w.vel), (300.0, 0.0));
    }
}
