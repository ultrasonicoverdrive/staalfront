//! Choosing which in-game sounds play (WP11.3): the part between "things
//! happened at these places" and `AudioSystem::play_at`. Pure bookkeeping
//! with no device and no game types, so a fight can be run through it in a
//! test.
//!
//! The volume and pan of a sound are not worked out here a second time: the
//! [`Planner`] asks [`spatial::mix`] (the one model, tuned by
//! `assets/config/audio.ron`) and uses the result to decide.
//!
//! One call of [`Planner::plan`] is one frame. For each request, in order:
//!
//! 1. no sound of that name (nothing in the bank, no Red Alert mapping):
//!    skipped, counted in `silent`, never an error;
//! 2. gain below `audible_gain` at the listener: dropped (`inaudible`);
//! 3. the survivors are sorted by higher `priority`, then louder, then
//!    earlier tick, then the order they came in. The sort is total, so the
//!    same requests always give the same cues;
//! 4. in that order a request is **merged** into a cue already chosen when
//!    it has the same sound and tick and lies within `merge_radius_m` of it
//!    (`merged`; the cue counts them in [`Cue::merged`]); else it is dropped
//!    when its event already has `max_per_batch` cues in the frame, or
//!    played less than the spacing ago, or the frame is full
//!    (`limited`, `spaced`); else it becomes a [`Cue`].
//!
//! The spacing between two playbacks of an event grows from
//! `min_spacing_ticks` at full volume to `far_spacing_ticks` as the gain
//! falls to the audible limit. The gain includes the zoom, so zooming out
//! also makes a sound less frequent, not only quieter.

use std::collections::BTreeMap;

use crate::config::PositionalConfig;
use crate::spatial::{self, Listener, SpatialParams};

/// Something that happened at a place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Request<'a> {
    /// The sound event name.
    pub sound: &'a str,
    /// World position (`+Y` up; the height does not count).
    pub pos: [f32; 3],
    /// The simulation tick it happened in.
    pub tick: u64,
}

/// A playback to start: what to hand to `AudioSystem::play_at`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cue<'a> {
    pub sound: &'a str,
    pub pos: [f32; 3],
    pub tick: u64,
    /// Gain and pan the model gives it at the listener (before the bank's
    /// base volume).
    pub gain: f32,
    pub pan: f32,
    /// Requests this cue stands for, itself included (at least 1).
    pub merged: u32,
}

/// What one frame's planning did with its requests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlanStats {
    pub requested: u32,
    /// No sound of that name.
    pub silent: u32,
    /// Too quiet at the listener.
    pub inaudible: u32,
    /// Folded into another cue.
    pub merged: u32,
    /// Over an event's `max_per_batch` or the frame's budget.
    pub limited: u32,
    /// Too soon after the event last played.
    pub spaced: u32,
    /// Cues made.
    pub planned: u32,
}

#[derive(Clone, Copy, Debug)]
struct Candidate {
    index: usize,
    gain: f32,
    pan: f32,
    priority: u8,
}

/// The state between frames: when each event last played.
#[derive(Debug)]
pub struct Planner {
    config: PositionalConfig,
    last_played: BTreeMap<String, u64>,
    scratch: Vec<Candidate>,
}

impl Planner {
    pub fn new(config: PositionalConfig) -> Self {
        Self {
            config,
            last_played: BTreeMap::new(),
            scratch: Vec::new(),
        }
    }

    /// Forget when things last played (a new game).
    pub fn reset(&mut self) {
        self.last_played.clear();
    }

    /// Choose the cues of one frame; they are appended to `out` (cleared
    /// first) in the order they were chosen. `has_sound` says whether a
    /// name has a sound to play. `params` are the spatial numbers the output
    /// uses (`AudioSystem::spatial_params`), passed in every frame so the
    /// planner never keeps a copy that could disagree with the output.
    pub fn plan<'a>(
        &mut self,
        listener: &Listener,
        params: &SpatialParams,
        requests: &[Request<'a>],
        has_sound: impl Fn(&str) -> bool,
        out: &mut Vec<Cue<'a>>,
    ) -> PlanStats {
        out.clear();
        let mut stats = PlanStats {
            requested: requests.len() as u32,
            ..PlanStats::default()
        };
        let mut candidates = std::mem::take(&mut self.scratch);
        candidates.clear();
        for (index, r) in requests.iter().enumerate() {
            if !has_sound(r.sound) {
                stats.silent += 1;
                continue;
            }
            let m = spatial::mix(listener, r.pos, params);
            if !m.is_audible(params) {
                stats.inaudible += 1;
                continue;
            }
            candidates.push(Candidate {
                index,
                gain: m.gain,
                pan: m.pan,
                priority: self.config.rule(r.sound).priority,
            });
        }
        candidates.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then(b.gain.total_cmp(&a.gain))
                .then(requests[a.index].tick.cmp(&requests[b.index].tick))
                .then(a.index.cmp(&b.index))
        });

        let mut played: Vec<(&str, u64)> = Vec::new();
        for c in &candidates {
            let r = &requests[c.index];
            let rule = *self.config.rule(r.sound);
            if let Some(cue) = out.iter_mut().find(|cue| {
                cue.sound == r.sound
                    && cue.tick == r.tick
                    && horizontal(cue.pos, r.pos) <= rule.merge_radius_m
            }) {
                cue.merged += 1;
                stats.merged += 1;
                continue;
            }
            let of_event = out.iter().filter(|cue| cue.sound == r.sound).count();
            if of_event >= rule.max_per_batch as usize
                || out.len() >= self.config.max_cues_per_frame as usize
            {
                stats.limited += 1;
                continue;
            }
            // Spacing counts from the earlier frames: cues of one event in this frame are
            // `max_per_batch`'s business, not the spacing's.
            let spacing = spacing_ticks(rule.min_spacing_ticks, rule.far_spacing_ticks, c.gain);
            if let Some(&last) = self.last_played.get(r.sound)
                && last.abs_diff(r.tick) < u64::from(spacing)
            {
                stats.spaced += 1;
                continue;
            }
            played.push((r.sound, r.tick));
            out.push(Cue {
                sound: r.sound,
                pos: r.pos,
                tick: r.tick,
                gain: c.gain,
                pan: c.pan,
                merged: 1,
            });
            stats.planned += 1;
        }
        // Remember the latest tick each event played at in this frame (replacing what an
        // earlier frame left, so a tick counter that starts over does not hold an event back).
        let mut latest: Vec<(&str, u64)> = Vec::new();
        for (sound, tick) in played {
            match latest.iter_mut().find(|(s, _)| *s == sound) {
                Some((_, t)) => *t = (*t).max(tick),
                None => latest.push((sound, tick)),
            }
        }
        for (sound, tick) in latest {
            match self.last_played.get_mut(sound) {
                Some(t) => *t = tick,
                None => {
                    self.last_played.insert(sound.to_string(), tick);
                }
            }
        }
        self.scratch = candidates;
        stats
    }
}

/// Ticks between two playbacks at `gain` (`0..=1`): `min` at full volume,
/// `far` at none, linear in between, rounded to the nearest tick.
pub fn spacing_ticks(min: u32, far: u32, gain: f32) -> u32 {
    let g = if gain.is_finite() {
        gain.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let extra = (far.saturating_sub(min)) as f32 * (1.0 - g);
    min + (extra + 0.5) as u32
}

fn horizontal(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (dx, dz) = (a[0] - b[0], a[2] - b[2]);
    (dx * dx + dz * dz).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AudioConfig, EventRule};

    fn planner() -> Planner {
        let c = AudioConfig::embedded();
        Planner::new(c.positional)
    }

    fn rule(priority: u8, max: u32, min_sp: u32, far_sp: u32, merge: f32) -> EventRule {
        EventRule {
            priority,
            max_per_batch: max,
            min_spacing_ticks: min_sp,
            far_spacing_ticks: far_sp,
            merge_radius_m: merge,
        }
    }

    /// A planner with its own small rules, the shipped spatial numbers.
    fn with(rules: &[(&str, EventRule)], cap: u32) -> Planner {
        let mut c = AudioConfig::embedded().positional;
        c.max_cues_per_frame = cap;
        c.events = rules.iter().map(|(n, r)| (n.to_string(), *r)).collect();
        Planner::new(c)
    }

    fn listener() -> Listener {
        // Looking at the origin from 30 m: no zoom attenuation.
        Listener::new([0.0; 3], [1.0, 0.0, 0.0], 30.0)
    }

    fn req(sound: &str, x: f32, tick: u64) -> Request<'_> {
        Request {
            sound,
            pos: [x, 0.0, 0.0],
            tick,
        }
    }

    fn all(_: &str) -> bool {
        true
    }

    fn run<'a>(p: &mut Planner, l: &Listener, reqs: &[Request<'a>]) -> (Vec<Cue<'a>>, PlanStats) {
        let mut out = Vec::new();
        let s = p.plan(
            l,
            &AudioConfig::embedded().spatial.params(),
            reqs,
            all,
            &mut out,
        );
        (out, s)
    }

    #[test]
    fn spacing_runs_from_min_to_far() {
        assert_eq!(spacing_ticks(1, 9, 1.0), 1);
        assert_eq!(spacing_ticks(1, 9, 0.0), 9);
        assert_eq!(spacing_ticks(1, 9, 0.5), 5);
        assert_eq!(spacing_ticks(3, 3, 0.2), 3);
        assert_eq!(spacing_ticks(1, 9, f32::NAN), 9);
    }

    #[test]
    fn a_name_without_a_sound_is_skipped_quietly() {
        let mut p = planner();
        let mut out = Vec::new();
        let reqs = [req("weapon.fire.bullet", 1.0, 5), req("ui.click", 1.0, 5)];
        let s = p.plan(
            &listener(),
            &AudioConfig::embedded().spatial.params(),
            &reqs,
            |n| n == "ui.click",
            &mut out,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].sound, "ui.click");
        assert_eq!((s.silent, s.planned), (1, 1));
    }

    #[test]
    fn the_gain_and_pan_are_the_model_s() {
        let mut p = planner();
        let l = Listener::new([10.0, 0.0, 10.0], [1.0, 0.0, 0.0], 120.0);
        let pos = [60.0, 0.0, 20.0];
        let (out, _) = run(
            &mut p,
            &l,
            &[Request {
                sound: "explosion.large",
                pos,
                tick: 3,
            }],
        );
        let m = spatial::mix(&l, pos, &AudioConfig::embedded().spatial.params());
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].gain, out[0].pan), (m.gain, m.pan));
        assert!(m.gain > 0.0 && m.pan > 0.0);
    }

    #[test]
    fn too_far_is_dropped_not_played() {
        let mut p = planner();
        let (out, s) = run(&mut p, &listener(), &[req("explosion.large", 500.0, 1)]);
        assert!(out.is_empty());
        assert_eq!(s.inaudible, 1);
    }

    #[test]
    fn shots_of_one_tick_near_each_other_merge_into_the_nearest() {
        let mut p = with(&[("a", rule(1, 4, 0, 0, 5.0))], 16);
        let reqs: Vec<_> = [30.0, 28.0, 33.0, 31.0, 29.0]
            .iter()
            .map(|&x| req("a", x, 7))
            .collect();
        let (out, s) = run(&mut p, &listener(), &reqs);
        assert_eq!(out.len(), 1);
        // 28 m is the nearest to the listener at the origin, so the loudest: it is the anchor.
        assert_eq!(out[0].pos[0], 28.0);
        assert_eq!(out[0].merged, 5);
        assert_eq!((s.merged, s.planned), (4, 1));
    }

    #[test]
    fn a_different_tick_a_different_sound_or_a_gap_does_not_merge() {
        let mut p = with(
            &[("a", rule(1, 9, 0, 0, 5.0)), ("b", rule(1, 9, 0, 0, 5.0))],
            16,
        );
        let reqs = [
            req("a", 10.0, 1),
            req("a", 11.0, 2), // other tick
            req("b", 10.0, 1), // other sound
            req("a", 20.0, 1), // 10 m apart: beyond the radius
        ];
        let (out, s) = run(&mut p, &listener(), &reqs);
        assert_eq!(out.len(), 4);
        assert_eq!(s.merged, 0);
    }

    #[test]
    fn the_result_does_not_depend_on_the_order_of_equal_requests() {
        let reqs_a = [req("a", 10.0, 1), req("a", 40.0, 1), req("a", 70.0, 1)];
        let mut reqs_b = reqs_a;
        reqs_b.reverse();
        let rules = [("a", rule(1, 2, 0, 0, 5.0))];
        let (a, _) = run(&mut with(&rules, 16), &listener(), &reqs_a);
        let (b, _) = run(&mut with(&rules, 16), &listener(), &reqs_b);
        let key = |v: &[Cue]| v.iter().map(|c| c.pos[0].to_bits()).collect::<Vec<_>>();
        assert_eq!(key(&a), key(&b));
        assert_eq!(a.len(), 2);
        // the nearest two stay
        assert_eq!(a[0].pos[0], 10.0);
        assert_eq!(a[1].pos[0], 40.0);
    }

    #[test]
    fn the_voice_limit_of_an_event_keeps_the_nearest() {
        let mut p = with(&[("a", rule(1, 2, 0, 0, 0.0))], 16);
        let reqs: Vec<_> = (0..6).map(|i| req("a", 5.0 + 20.0 * i as f32, 1)).collect();
        let (out, s) = run(&mut p, &listener(), &reqs);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].pos[0], 5.0);
        assert_eq!(out[1].pos[0], 25.0);
        assert_eq!(s.limited, 4);
    }

    #[test]
    fn priority_beats_loudness_when_the_frame_is_full() {
        let rules = [
            ("quiet.big", rule(5, 9, 0, 0, 0.0)),
            ("loud.small", rule(1, 9, 0, 0, 0.0)),
        ];
        let mut p = with(&rules, 2);
        let reqs = [
            req("loud.small", 1.0, 1),
            req("loud.small", 2.0, 1),
            req("quiet.big", 90.0, 1),
        ];
        let (out, s) = run(&mut p, &listener(), &reqs);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].sound, "quiet.big");
        assert_eq!(out[1].sound, "loud.small");
        assert_eq!(out[1].pos[0], 1.0);
        assert_eq!(s.limited, 1);
    }

    #[test]
    fn an_event_is_held_back_by_its_spacing_and_comes_back_after_it() {
        let mut p = with(&[("a", rule(1, 9, 3, 3, 0.0))], 16);
        let l = listener();
        // Same planner over several frames: ticks 10 (plays), 11, 12 (held), 13 (plays).
        let mut played = Vec::new();
        for t in 10..16 {
            played.push(run(&mut p, &l, &[req("a", 1.0, t)]).0.len());
        }
        assert_eq!(played, [1, 0, 0, 1, 0, 0]);
    }

    #[test]
    fn a_far_sound_waits_longer_than_a_near_one() {
        let rules = [("a", rule(1, 9, 1, 9, 0.0))];
        let l = listener();
        let plays_in = |x: f32| {
            let mut p = with(&rules, 16);
            (0..40)
                .filter(|&t| !run(&mut p, &l, &[req("a", x, t)]).0.is_empty())
                .count()
        };
        let (near, mid, far) = (plays_in(10.0), plays_in(80.0), plays_in(120.0));
        assert!(near > mid && mid > far, "{near} {mid} {far}");
        assert_eq!(near, 40);
    }

    #[test]
    fn zooming_out_makes_the_same_event_less_frequent() {
        let rules = [("a", rule(1, 9, 1, 9, 0.0))];
        let plays_at = |cam: f32| {
            let mut p = with(&rules, 16);
            let l = Listener::new([0.0; 3], [1.0, 0.0, 0.0], cam);
            (0..60)
                .filter(|&t| !run(&mut p, &l, &[req("a", 10.0, t)]).0.is_empty())
                .count()
        };
        assert!(plays_at(30.0) > plays_at(100.0));
        assert!(plays_at(100.0) > plays_at(400.0));
    }

    #[test]
    fn restarting_the_tick_counter_does_not_silence_the_event() {
        let mut p = with(&[("a", rule(1, 9, 5, 5, 0.0))], 16);
        let l = listener();
        assert_eq!(run(&mut p, &l, &[req("a", 1.0, 5000)]).0.len(), 1);
        assert_eq!(run(&mut p, &l, &[req("a", 1.0, 3)]).0.len(), 1);
    }

    #[test]
    fn a_hundred_shots_in_one_tick_make_a_few_cues() {
        let mut p = planner();
        let reqs: Vec<_> = (0..100)
            .map(|i| {
                let mut r = req("weapon.fire.bullet", (i % 10) as f32 * 0.5, 9);
                r.pos[2] = (i / 10) as f32 * 0.5;
                r
            })
            .collect();
        let (out, s) = run(&mut p, &listener(), &reqs);
        assert_eq!(out.len(), 1, "all within 8 m of each other merge");
        assert_eq!(out[0].merged, 100);
        assert_eq!(s.merged, 99);
    }
}
