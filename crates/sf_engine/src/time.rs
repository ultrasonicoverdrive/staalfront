//! Fixed-step accumulator and rolling frame-time statistics.
//!
//! Both are pure: time is injected as `Duration`s, so they are unit-tested
//! without a window or a real clock.

use std::collections::VecDeque;
use std::time::Duration;

/// Simulation rate (PLAN.md 1.2).
pub const SIM_HZ: u32 = 20;
/// Length of one simulation tick (50 ms).
pub const SIM_STEP: Duration = Duration::from_millis(1000 / SIM_HZ as u64);
/// Default cap on ticks run in a single frame (spiral-of-death protection).
pub const MAX_TICKS_PER_FRAME: u32 = 5;

/// Result of feeding one frame's elapsed time to [`FixedStep::advance`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Advance {
    /// Number of the first tick to run now.
    pub first_tick: u64,
    /// How many ticks to run now: `first_tick .. first_tick + ticks`.
    pub ticks: u32,
    /// Whole ticks of backlog discarded because the cap was hit.
    pub dropped_ticks: u64,
}

impl Advance {
    /// Tick numbers to run, in order.
    pub fn tick_range(&self) -> std::ops::Range<u64> {
        self.first_tick..self.first_tick + u64::from(self.ticks)
    }
}

/// Accumulator-based fixed timestep.
///
/// Feed it the real elapsed time each frame; it says how many fixed ticks to
/// run and exposes the interpolation factor for rendering.
#[derive(Clone, Debug)]
pub struct FixedStep {
    step: Duration,
    max_ticks: u32,
    accumulator: Duration,
    next_tick: u64,
}

impl FixedStep {
    /// `step` must be non-zero and `max_ticks_per_frame` at least 1.
    pub fn new(step: Duration, max_ticks_per_frame: u32) -> Self {
        assert!(!step.is_zero(), "fixed step must be non-zero");
        assert!(max_ticks_per_frame >= 1, "need at least one tick per frame");
        Self {
            step,
            max_ticks: max_ticks_per_frame,
            accumulator: Duration::ZERO,
            next_tick: 0,
        }
    }

    /// The simulation default: 20 Hz, at most [`MAX_TICKS_PER_FRAME`] per frame.
    pub fn sim_default() -> Self {
        Self::new(SIM_STEP, MAX_TICKS_PER_FRAME)
    }

    pub fn step(&self) -> Duration {
        self.step
    }

    /// Number of the next tick that will run.
    pub fn next_tick(&self) -> u64 {
        self.next_tick
    }

    /// Add `dt` of real time and decide which ticks to run.
    ///
    /// If more than `max_ticks` are due, only `max_ticks` run and the rest of
    /// the backlog (whole steps) is dropped: the game slows down instead of
    /// freezing in a catch-up loop. The sub-step remainder is kept.
    pub fn advance(&mut self, dt: Duration) -> Advance {
        self.accumulator = self.accumulator.saturating_add(dt);
        let due = self.accumulator.as_nanos() / self.step.as_nanos();
        let ticks = due.min(u128::from(self.max_ticks)) as u32;
        self.accumulator -= self.step * ticks;

        let mut dropped = 0u64;
        if due > u128::from(self.max_ticks) {
            // Keep only the remainder below one step.
            let whole = self.accumulator.as_nanos() / self.step.as_nanos();
            dropped = whole as u64;
            self.accumulator -= self.step * whole as u32;
        }

        let first_tick = self.next_tick;
        self.next_tick += u64::from(ticks);
        Advance {
            first_tick,
            ticks,
            dropped_ticks: dropped,
        }
    }

    /// Interpolation factor in `[0, 1)`: how far we are between the last run
    /// tick and the next one.
    pub fn alpha(&self) -> f32 {
        let a = self.accumulator.as_secs_f64() / self.step.as_secs_f64();
        let a = a as f32;
        // Rounding to f32 can reach exactly 1.0; keep the half-open range.
        if a >= 1.0 { ONE_MINUS_EPS } else { a }
    }
}

/// Largest `f32` below 1.0.
const ONE_MINUS_EPS: f32 = 1.0 - f32::EPSILON / 2.0;

/// Summary of the frames currently in the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameSummary {
    pub count: usize,
    pub avg: Duration,
    pub p50: Duration,
    pub p95: Duration,
    pub p99: Duration,
    pub max: Duration,
}

impl FrameSummary {
    /// Frames per second implied by the average frame time.
    pub fn fps(&self) -> f64 {
        let s = self.avg.as_secs_f64();
        if s > 0.0 { 1.0 / s } else { 0.0 }
    }
}

/// Rolling window over the last N frame times.
#[derive(Clone, Debug)]
pub struct FrameStats {
    capacity: usize,
    frames: VecDeque<Duration>,
}

impl FrameStats {
    /// Default window: about four seconds at 60 fps.
    pub const DEFAULT_CAPACITY: usize = 240;

    pub fn new(capacity: usize) -> Self {
        assert!(capacity >= 1, "window must hold at least one frame");
        Self {
            capacity,
            frames: VecDeque::with_capacity(capacity),
        }
    }

    pub fn record(&mut self, frame_time: Duration) {
        if self.frames.len() == self.capacity {
            self.frames.pop_front();
        }
        self.frames.push_back(frame_time);
    }

    pub fn last(&self) -> Option<Duration> {
        self.frames.back().copied()
    }

    /// Frame times oldest to newest (for a frame-time graph).
    pub fn frames(&self) -> impl Iterator<Item = Duration> + '_ {
        self.frames.iter().copied()
    }

    /// Statistics over the window; `None` while it is empty.
    /// Percentiles use the nearest-rank method.
    pub fn summary(&self) -> Option<FrameSummary> {
        if self.frames.is_empty() {
            return None;
        }
        let mut sorted: Vec<Duration> = self.frames.iter().copied().collect();
        sorted.sort_unstable();
        let n = sorted.len();
        let total: Duration = sorted.iter().sum();
        let rank = |p: usize| sorted[(n * p).div_ceil(100).max(1) - 1];
        Some(FrameSummary {
            count: n,
            avg: total / n as u32,
            p50: rank(50),
            p95: rank(95),
            p99: rank(99),
            max: sorted[n - 1],
        })
    }
}

impl Default for FrameStats {
    fn default() -> Self {
        Self::new(Self::DEFAULT_CAPACITY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn sim_step_is_20hz() {
        assert_eq!(SIM_STEP, ms(50));
    }

    #[test]
    fn exactly_twenty_ticks_per_second_at_60fps() {
        let mut fs = FixedStep::sim_default();
        let frame = Duration::from_nanos(1_000_000_000 / 60);
        let mut ticks = 0u64;
        for _ in 0..60 {
            ticks += u64::from(fs.advance(frame).ticks);
        }
        // 60 frames of 1/60 s (minus <60 ns rounding): 20 ticks, or 19 if the
        // rounding left the accumulator a hair short of the 20th.
        assert!((19..=20).contains(&ticks), "{ticks}");
        // Whole seconds of exact multiples are exact.
        let mut fs = FixedStep::sim_default();
        let total: u64 = (0..100).map(|_| u64::from(fs.advance(ms(10)).ticks)).sum();
        assert_eq!(total, 20);
    }

    #[test]
    fn tick_numbers_are_contiguous() {
        let mut fs = FixedStep::sim_default();
        let a = fs.advance(ms(120));
        assert_eq!(a.tick_range(), 0..2);
        let b = fs.advance(ms(30)); // 20 ms left + 30 ms = 50 ms
        assert_eq!(b.tick_range(), 2..3);
        assert_eq!(fs.next_tick(), 3);
        let c = fs.advance(ms(10));
        assert_eq!(c.ticks, 0);
        assert_eq!(c.first_tick, 3);
    }

    #[test]
    fn remainder_carries_over_and_alpha_tracks_it() {
        let mut fs = FixedStep::sim_default();
        fs.advance(ms(25));
        assert!((fs.alpha() - 0.5).abs() < 1e-6);
        let a = fs.advance(ms(30));
        assert_eq!(a.ticks, 1);
        assert!((fs.alpha() - 0.1).abs() < 1e-6);
    }

    #[test]
    fn catch_up_is_capped_and_backlog_dropped() {
        let mut fs = FixedStep::new(ms(50), 5);
        // A 10 s stall = 200 ticks due; only 5 run.
        let a = fs.advance(Duration::from_secs(10));
        assert_eq!(a.ticks, 5);
        assert_eq!(a.dropped_ticks, 195);
        assert!(fs.alpha() < 1e-6);
        // Next normal frame is back to normal: no lingering backlog.
        let b = fs.advance(ms(50));
        assert_eq!(b.ticks, 1);
        assert_eq!(b.dropped_ticks, 0);
        assert_eq!(b.first_tick, 5);
    }

    #[test]
    fn exactly_at_cap_drops_nothing() {
        let mut fs = FixedStep::new(ms(50), 5);
        let a = fs.advance(ms(250));
        assert_eq!((a.ticks, a.dropped_ticks), (5, 0));
    }

    #[test]
    fn cap_keeps_subtick_remainder() {
        let mut fs = FixedStep::new(ms(50), 2);
        let a = fs.advance(ms(425)); // 8 ticks + 25 ms
        assert_eq!((a.ticks, a.dropped_ticks), (2, 6));
        assert!((fs.alpha() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn alpha_is_always_below_one() {
        let mut fs = FixedStep::new(ms(50), 5);
        fs.advance(ms(50) - Duration::from_nanos(1));
        let a = fs.alpha();
        assert!((0.0..1.0).contains(&a), "{a}");
        // Sweep many odd frame lengths.
        let mut fs = FixedStep::sim_default();
        for i in 1..5000u64 {
            fs.advance(Duration::from_nanos(i * 7_919));
            let a = fs.alpha();
            assert!((0.0..1.0).contains(&a), "{a}");
        }
    }

    #[test]
    fn zero_dt_does_nothing() {
        let mut fs = FixedStep::sim_default();
        let a = fs.advance(Duration::ZERO);
        assert_eq!((a.ticks, a.dropped_ticks, a.first_tick), (0, 0, 0));
        assert_eq!(fs.alpha(), 0.0);
    }

    #[test]
    fn stats_empty_has_no_summary() {
        assert!(FrameStats::default().summary().is_none());
        assert!(FrameStats::default().last().is_none());
    }

    #[test]
    fn stats_percentiles_nearest_rank() {
        let mut s = FrameStats::new(100);
        for i in 1..=100 {
            s.record(ms(i));
        }
        let r = s.summary().unwrap();
        assert_eq!(r.count, 100);
        assert_eq!(r.p50, ms(50));
        assert_eq!(r.p95, ms(95));
        assert_eq!(r.p99, ms(99));
        assert_eq!(r.max, ms(100));
        assert_eq!(r.avg, Duration::from_micros(50_500));
        assert_eq!(s.last(), Some(ms(100)));
    }

    #[test]
    fn stats_window_rolls() {
        let mut s = FrameStats::new(3);
        for i in [100, 1, 2, 3] {
            s.record(ms(i));
        }
        let r = s.summary().unwrap();
        assert_eq!(r.count, 3);
        assert_eq!(r.max, ms(3)); // the 100 ms spike rolled out
        assert_eq!(s.frames().collect::<Vec<_>>(), [ms(1), ms(2), ms(3)]);
    }

    #[test]
    fn stats_single_frame_and_fps() {
        let mut s = FrameStats::new(10);
        s.record(ms(20));
        let r = s.summary().unwrap();
        assert_eq!(
            (r.p50, r.p95, r.p99, r.max),
            (ms(20), ms(20), ms(20), ms(20))
        );
        assert!((r.fps() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn stats_spike_shows_in_p99_not_p50() {
        let mut s = FrameStats::new(200);
        for _ in 0..197 {
            s.record(ms(16));
        }
        s.record(ms(100));
        s.record(ms(100));
        s.record(ms(200));
        let r = s.summary().unwrap();
        assert_eq!(r.p50, ms(16));
        assert_eq!(r.p99, ms(100));
        assert_eq!(r.max, ms(200));
    }
}
