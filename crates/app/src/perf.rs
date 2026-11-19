//! `OPEN_ISLAND_PERF=1`: how smooth the animations are. While the island animates every frame's interval and
//! its own cost are recorded, and when the animation ends one line says how it went.

use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Perf {
    on: bool,
    last: Option<Instant>,
    intervals: Vec<f64>,
    work: Vec<f64>,
}

/// (frames, mean interval ms, 95th percentile ms, worst ms, frames over 20 ms, mean work ms, worst work ms)
pub fn summarize(intervals: &[f64], work: &[f64]) -> Option<(usize, f64, f64, f64, usize, f64, f64)> {
    if intervals.is_empty() {
        return None;
    }
    let mut sorted = intervals.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let p95 = sorted[((sorted.len() as f64 * 0.95).ceil() as usize).clamp(1, sorted.len()) - 1];
    let mean = |v: &[f64]| if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 };
    let max = |v: &[f64]| v.iter().cloned().fold(0.0, f64::max);
    Some((intervals.len(), mean(intervals), p95, max(intervals), intervals.iter().filter(|i| **i > 20.0).count(), mean(work), max(work)))
}

impl Perf {
    pub fn from_env() -> Self {
        Self { on: std::env::var_os("OPEN_ISLAND_PERF").is_some(), ..Default::default() }
    }

    /// One frame was drawn; `animating` says the island wants another one at once, `work` is what drawing cost
    /// (without the wait for the display).
    pub fn frame(&mut self, animating: bool, work: Duration) {
        if !self.on {
            return;
        }
        let now = Instant::now();
        if animating {
            if let Some(prev) = self.last {
                self.intervals.push(now.duration_since(prev).as_secs_f64() * 1000.0);
            }
            self.work.push(work.as_secs_f64() * 1000.0);
            self.last = Some(now);
        } else {
            if let Some((n, mean, p95, worst, slow, wmean, wmax)) = summarize(&self.intervals, &self.work) {
                log::info!("animation: {n} frames, interval mean {mean:.1} ms, p95 {p95:.1}, worst {worst:.1}, {slow} over 20 ms; work mean {wmean:.1} ms, worst {wmax:.1}");
            }
            self.intervals.clear();
            self.work.clear();
            self.last = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_a_smooth_and_a_rough_animation() {
        assert!(summarize(&[], &[]).is_none());
        let smooth = vec![16.6; 60];
        let (n, mean, p95, worst, slow, _, _) = summarize(&smooth, &[2.0; 60]).unwrap();
        assert_eq!((n, slow), (60, 0));
        assert!((mean - 16.6).abs() < 1e-9 && (p95 - 16.6).abs() < 1e-9 && (worst - 16.6).abs() < 1e-9);
        let mut rough = vec![16.6; 58];
        rough.extend([45.0, 80.0]);
        let (_, _, p95, worst, slow, wmean, wmax) = summarize(&rough, &[1.0, 3.0]).unwrap();
        assert_eq!((slow, worst), (2, 80.0));
        assert!(p95 < 20.0, "two bad frames in sixty are under the 95th percentile");
        assert_eq!((wmean, wmax), (2.0, 3.0));
    }
}
