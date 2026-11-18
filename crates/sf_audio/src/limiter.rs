//! The master limiter (WP11.3c): a kira effect on the main track, so the sum of everything (fight, music,
//! voices, interface sounds) cannot clip. kira and cpal do not limit: a sample above full scale is cut flat by
//! the device. This effect follows the peak of each frame at once (no attack time, so no sample of the output
//! is above the ceiling) and lets go with a release time, so a loud moment turns the whole mix down smoothly
//! and it comes back up. The ceiling and the release come from `audio.ron` (`limiter`,
//! [`LimiterConfig`](crate::config::LimiterConfig)); they can change while the game runs ([`LimiterHandle`]).
//! It is presentation only; nothing here reaches `sf_sim`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use kira::Frame;
use kira::effect::{Effect, EffectBuilder};
use kira::info::Info;

/// The numbers the effect reads, shared with its handle as f32 bits.
struct Shared {
    ceiling: AtomicU32,
    release_ms: AtomicU32,
}

/// Changes the numbers of a running [`MasterLimiter`].
#[derive(Clone)]
pub struct LimiterHandle(Arc<Shared>);

impl LimiterHandle {
    /// The ceiling and the release in ms as they are now.
    pub fn current(&self) -> (f32, f32) {
        (
            f32::from_bits(self.0.ceiling.load(Ordering::Relaxed)),
            f32::from_bits(self.0.release_ms.load(Ordering::Relaxed)),
        )
    }

    /// Peak (of full scale, 0 to 1) the output never exceeds, and the time in ms the turn-down takes to let go.
    pub fn set(&self, ceiling: f32, release_ms: f32) {
        self.0.ceiling.store(ceiling.to_bits(), Ordering::Relaxed);
        self.0
            .release_ms
            .store(release_ms.to_bits(), Ordering::Relaxed);
    }
}

/// Builds the effect; the handle is made with it.
pub struct MasterLimiterBuilder {
    ceiling: f32,
    release_ms: f32,
}

impl MasterLimiterBuilder {
    pub fn new(ceiling: f32, release_ms: f32) -> Self {
        Self {
            ceiling,
            release_ms,
        }
    }
}

impl EffectBuilder for MasterLimiterBuilder {
    type Handle = LimiterHandle;

    fn build(self) -> (Box<dyn Effect>, LimiterHandle) {
        let shared = Arc::new(Shared {
            ceiling: AtomicU32::new(self.ceiling.to_bits()),
            release_ms: AtomicU32::new(self.release_ms.to_bits()),
        });
        (
            Box::new(MasterLimiter {
                shared: shared.clone(),
                follower: 0.0,
            }),
            LimiterHandle(shared),
        )
    }
}

/// The effect: `follower` is the peak envelope (instant rise, exponential fall).
pub struct MasterLimiter {
    shared: Arc<Shared>,
    follower: f32,
}

impl MasterLimiter {
    /// One frame in, one out. `fall` is the per-frame factor the envelope falls by.
    fn step(&mut self, f: Frame, ceiling: f32, fall: f32) -> Frame {
        let peak = f.left.abs().max(f.right.abs());
        self.follower = peak.max(self.follower * fall);
        let gain = if self.follower > ceiling {
            ceiling / self.follower
        } else {
            1.0
        };
        // The gain already brings the peak to the ceiling; the clamp only guards rounding.
        Frame {
            left: (f.left * gain).clamp(-ceiling, ceiling),
            right: (f.right * gain).clamp(-ceiling, ceiling),
        }
    }
}

impl Effect for MasterLimiter {
    fn process(&mut self, input: &mut [Frame], dt: f64, _info: &Info) {
        let ceiling = f32::from_bits(self.shared.ceiling.load(Ordering::Relaxed)).clamp(0.05, 1.0);
        let release_ms = f32::from_bits(self.shared.release_ms.load(Ordering::Relaxed)).max(1.0);
        // The envelope falls to a third in `release_ms`.
        let fall = (-dt as f32 / (release_ms / 1000.0)).exp();
        for f in input.iter_mut() {
            *f = self.step(*f, ceiling, fall);
        }
    }
}

/// The production effect run over a mono signal at `sample_rate` with the numbers of `cfg` (the same code the
/// output runs). For tests that mix real sounds and want to know what the limiter does to the sum.
pub fn limit_samples(x: &mut [f32], sample_rate: u32, cfg: crate::config::LimiterConfig) {
    let (mut fx, _handle) = MasterLimiterBuilder::new(cfg.ceiling, cfg.release_ms).build();
    let mut frames: Vec<Frame> = x.iter().map(|v| Frame::from_mono(*v)).collect();
    let info = kira::info::MockInfoBuilder::new().build();
    fx.process(&mut frames, 1.0 / f64::from(sample_rate), &info);
    for (v, f) in x.iter_mut().zip(frames) {
        *v = f.left;
    }
}

#[cfg(test)]
mod tests {
    use kira::info::MockInfoBuilder;

    use super::*;

    fn run(input: &[f32], ceiling: f32, release_ms: f32) -> Vec<f32> {
        let (mut fx, _h) = MasterLimiterBuilder::new(ceiling, release_ms).build();
        let mut frames: Vec<Frame> = input.iter().map(|v| Frame::from_mono(*v)).collect();
        let info = MockInfoBuilder::new().build();
        fx.process(&mut frames, 1.0 / 22_050.0, &info);
        frames.iter().map(|f| f.left).collect()
    }

    #[test]
    fn nothing_is_above_the_ceiling_and_a_quiet_signal_is_untouched() {
        let loud: Vec<f32> = (0..4000)
            .map(|i| 3.0 * (i as f32 * 0.05).sin() * if i < 100 { 3.0 } else { 1.0 })
            .collect();
        let out = run(&loud, 0.9, 100.0);
        let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak <= 0.9 + 1e-6, "peak {peak}");
        assert!(peak > 0.85, "the limiter must not mute: {peak}");
        let quiet: Vec<f32> = (0..4000).map(|i| 0.5 * (i as f32 * 0.05).sin()).collect();
        assert_eq!(run(&quiet, 0.9, 100.0), quiet);
    }

    #[test]
    fn it_lets_go_after_the_release_time() {
        // A burst of 4.0 then a steady 0.5: right after the burst the 0.5 is turned down, 1 s later it is back.
        let mut x = vec![4.0f32; 50];
        x.extend(vec![0.5f32; 22_050]);
        let out = run(&x, 0.9, 100.0);
        assert!(out[60] < 0.2, "{}", out[60]);
        assert!((out[22_000] - 0.5).abs() < 1e-3, "{}", out[22_000]);
    }

    #[test]
    fn the_handle_changes_the_ceiling_of_a_running_effect() {
        let (mut fx, h) = MasterLimiterBuilder::new(0.9, 100.0).build();
        let info = MockInfoBuilder::new().build();
        let mut a = vec![Frame::from_mono(2.0); 10];
        fx.process(&mut a, 1.0 / 22_050.0, &info);
        assert!((a[9].left - 0.9).abs() < 1e-5);
        h.set(0.5, 100.0);
        let mut b = vec![Frame::from_mono(2.0); 10];
        fx.process(&mut b, 1.0 / 22_050.0, &info);
        assert!((b[9].left - 0.5).abs() < 1e-5);
    }
}
