//! The procedural walk cycle of a figure (WP11.6i): rigid node animation, render only.
//!
//! A figure model whose six figure nodes have their origins at the joints (the glb loader checks
//! it, [`crate::glb`]) gets a [`Joint::Figure`](crate::model::Joint::Figure) on each of them. Per
//! unit the instancer keeps a [`WalkState`]: the **phase** of the cycle advances with the distance
//! the unit travelled between the interpolated positions of two frames, one cycle per
//! [`WalkConfig::stride_m`] (two steps), so the feet do not slide and a slower unit steps slower.
//! Wall time only drives the **weight**: a moving unit blends into the cycle and a still one back
//! to its stand pose in [`WalkConfig::blend_s`].
//!
//! The pose ([`WalkPose`]): the legs swing opposite each other about the hip, each arm opposite to
//! the leg on its side about the shoulder (the rifle arm less), the torso leans a little forward,
//! and the whole figure drops by `hip * (1 - cos(leg angle))` so the forward foot stays on the
//! ground (the bob, twice per cycle). Nothing here reaches the simulation.

use std::f32::consts::TAU;

use serde::Deserialize;

/// The shipped walk settings, compiled in like the other render settings.
pub const EMBEDDED_FIGURE_MOTION: &str = include_str!("../../../assets/config/figure_motion.ron");

/// `assets/config/figure_motion.ron`.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WalkConfig {
    /// Swing of each leg about the hip, either way of upright, degrees.
    pub leg_swing_deg: f32,
    /// Swing of the free arm about the shoulder, degrees.
    pub arm_swing_deg: f32,
    /// Swing of the arm that carries the rifle (`arm_r`), degrees.
    pub rifle_arm_swing_deg: f32,
    /// Forward lean of the torso and head at full walk, degrees.
    pub lean_deg: f32,
    /// Ground covered by one full cycle (two steps) at true model size, metres.
    pub stride_m: f32,
    /// Time to blend from standing to walking and back, seconds.
    pub blend_s: f32,
    /// A jump longer than this per frame (metres at true size) is a teleport or a unit back on
    /// screen after a while: it does not advance the phase.
    pub max_step_m: f32,
}

impl WalkConfig {
    /// The shipped settings.
    pub fn embedded() -> WalkConfig {
        Self::from_ron(EMBEDDED_FIGURE_MOTION).expect("assets/config/figure_motion.ron is valid")
    }

    /// Parse and check: every angle in `0..=60` degrees, the lengths and the blend time positive.
    pub fn from_ron(text: &str) -> Result<WalkConfig, String> {
        let c: WalkConfig = ron::from_str(text).map_err(|e| format!("figure_motion.ron: {e}"))?;
        for (name, deg) in [
            ("leg_swing_deg", c.leg_swing_deg),
            ("arm_swing_deg", c.arm_swing_deg),
            ("rifle_arm_swing_deg", c.rifle_arm_swing_deg),
            ("lean_deg", c.lean_deg),
        ] {
            if !(0.0..=60.0).contains(&deg) {
                return Err(format!("figure_motion.ron: {name} must be 0..=60 degrees"));
            }
        }
        for (name, v) in [
            ("stride_m", c.stride_m),
            ("blend_s", c.blend_s),
            ("max_step_m", c.max_step_m),
        ] {
            if !(v.is_finite() && v > 0.0) {
                return Err(format!("figure_motion.ron: {name} must be positive"));
            }
        }
        Ok(c)
    }
}

/// The joint angles of a figure in one frame, radians (positive turns a limb's lower end
/// forward, toward `+X`). [`WalkPose::STAND`] is the rest pose of the file.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WalkPose {
    pub leg_l: f32,
    pub leg_r: f32,
    pub arm_l: f32,
    pub arm_r: f32,
    /// Forward lean of the torso and the head.
    pub lean: f32,
}

impl WalkPose {
    /// The rest pose: every joint as in the file.
    pub const STAND: WalkPose = WalkPose {
        leg_l: 0.0,
        leg_r: 0.0,
        arm_l: 0.0,
        arm_r: 0.0,
        lean: 0.0,
    };

    /// How far the figure drops for a hip at `hip` metres: the forward leg's foot stays on the
    /// ground.
    pub fn drop(&self, hip: f32) -> f32 {
        let a = self.leg_l.abs().max(self.leg_r.abs());
        hip * (1.0 - a.cos())
    }
}

/// The walk state of one unit, kept by the instancer between frames.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WalkState {
    /// Where in the cycle, `0..1` (0 and 0.5 are the passing poses, legs upright).
    pub phase: f32,
    /// Blend between standing (0) and walking (1).
    pub weight: f32,
    /// The ground position of the last frame, metres; `None` before the first.
    last: Option<[f32; 2]>,
}

impl WalkState {
    /// Advance to the unit's position `pos` (metres) in a frame `dt` seconds after the last one.
    /// `scale` is the factor the model is drawn at (the stride grows with it, so the drawn feet
    /// keep pace with the ground).
    pub fn advance(&mut self, pos: [f32; 2], scale: f32, dt: f32, cfg: &WalkConfig) {
        let scale = scale.max(1e-3);
        let moved = match self.last {
            Some(p) => {
                let d = (pos[0] - p[0]).hypot(pos[1] - p[1]);
                if d > cfg.max_step_m * scale { 0.0 } else { d }
            }
            None => 0.0,
        };
        self.last = Some(pos);
        if moved > 0.0 {
            self.phase = (self.phase + moved / (cfg.stride_m * scale)).fract();
        }
        let target = if moved > 0.0 { 1.0 } else { 0.0 };
        let step = (dt.max(0.0) / cfg.blend_s).min(1.0);
        self.weight = if self.weight < target {
            (self.weight + step).min(target)
        } else {
            (self.weight - step).max(target)
        };
    }

    /// The pose for this state.
    pub fn pose(&self, cfg: &WalkConfig) -> WalkPose {
        walk_pose(self.phase, self.weight, cfg)
    }
}

/// The pose at `phase` (cycles) with `weight` (0 = stand, 1 = full walk): the left leg is
/// `swing * sin(2π phase)`, the right leg its opposite, each arm opposite to its own leg.
pub fn walk_pose(phase: f32, weight: f32, cfg: &WalkConfig) -> WalkPose {
    let w = weight.clamp(0.0, 1.0);
    let s = (phase * TAU).sin() * w;
    let leg = cfg.leg_swing_deg.to_radians() * s;
    WalkPose {
        leg_l: leg,
        leg_r: -leg,
        arm_l: -cfg.arm_swing_deg.to_radians() * s,
        arm_r: cfg.rifle_arm_swing_deg.to_radians() * s,
        lean: cfg.lean_deg.to_radians() * w,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> WalkConfig {
        WalkConfig::embedded()
    }

    #[test]
    fn the_shipped_settings_are_pinned() {
        let c = cfg();
        assert_eq!(c.leg_swing_deg, 30.0);
        assert_eq!(c.arm_swing_deg, 24.0);
        assert_eq!(c.rifle_arm_swing_deg, 5.0);
        assert_eq!(c.lean_deg, 4.0);
        assert_eq!(c.stride_m, 1.8);
        assert_eq!(c.blend_s, 0.25);
        assert_eq!(c.max_step_m, 2.0);
    }

    #[test]
    fn a_bad_setting_is_refused() {
        let text = EMBEDDED_FIGURE_MOTION.replace("stride_m: 1.80", "stride_m: 0.0");
        assert!(WalkConfig::from_ron(&text).is_err());
        let text = EMBEDDED_FIGURE_MOTION.replace("leg_swing_deg: 30.0", "leg_swing_deg: 90.0");
        assert!(WalkConfig::from_ron(&text).is_err());
    }

    /// The phase follows the distance, not the time: half a stride is half a cycle however many
    /// frames or seconds it takes, and time alone moves nothing.
    #[test]
    fn the_phase_advances_with_distance_and_not_with_time() {
        let c = cfg();
        let mut fast = WalkState::default();
        let mut slow = WalkState::default();
        fast.advance([0.0, 0.0], 1.0, 0.016, &c);
        slow.advance([0.0, 0.0], 1.0, 0.016, &c);
        for i in 1..=10 {
            fast.advance([c.stride_m * 0.05 * i as f32, 0.0], 1.0, 0.016, &c);
        }
        for i in 1..=40 {
            slow.advance([c.stride_m * 0.0125 * i as f32, 0.0], 1.0, 0.016, &c);
        }
        assert!((fast.phase - 0.5).abs() < 1e-4, "{}", fast.phase);
        assert!((slow.phase - 0.5).abs() < 1e-4, "{}", slow.phase);
        // Standing still for a long time does not move the phase.
        let before = slow.phase;
        for _ in 0..100 {
            slow.advance([c.stride_m * 0.0125 * 40.0, 0.0], 1.0, 0.1, &c);
        }
        assert_eq!(slow.phase, before);
        // A model drawn twice as large takes twice the ground per cycle.
        let mut big = WalkState::default();
        big.advance([0.0, 0.0], 2.0, 0.016, &c);
        big.advance([c.stride_m * 0.5, 0.0], 2.0, 0.016, &c);
        assert!((big.phase - 0.25).abs() < 1e-4);
        // A jump (a unit back on screen) does not.
        let mut jump = WalkState::default();
        jump.advance([0.0, 0.0], 1.0, 0.016, &c);
        jump.advance([50.0, 0.0], 1.0, 0.016, &c);
        assert_eq!(jump.phase, 0.0);
    }

    #[test]
    fn a_still_unit_reaches_the_stand_pose() {
        let c = cfg();
        let mut s = WalkState::default();
        for i in 0..30 {
            s.advance([0.03 * i as f32, 0.0], 1.0, 1.0 / 60.0, &c);
        }
        assert_eq!(s.weight, 1.0);
        assert_ne!(s.pose(&c), WalkPose::STAND);
        // Stopped: back to standing within the blend time (plus a frame), smoothly.
        let mut last = s.pose(&c).leg_l.abs();
        for _ in 0..16 {
            s.advance([0.87, 0.0], 1.0, 1.0 / 60.0, &c);
            let now = s.pose(&c).leg_l.abs();
            assert!(now <= last + 1e-6, "{now} > {last}");
            last = now;
        }
        assert_eq!(s.weight, 0.0);
        assert_eq!(s.pose(&c), WalkPose::STAND);
    }

    #[test]
    fn the_legs_are_opposite_and_each_arm_counters_its_leg() {
        let c = cfg();
        for k in 0..16 {
            let p = walk_pose(k as f32 / 16.0, 1.0, &c);
            assert_eq!(p.leg_l, -p.leg_r);
            assert!(
                p.arm_l * p.leg_l <= 0.0 && p.arm_r * p.leg_r <= 0.0,
                "{p:?}"
            );
        }
        let quarter = walk_pose(0.25, 1.0, &c);
        assert!((quarter.leg_l - 30_f32.to_radians()).abs() < 1e-5);
        assert!((quarter.arm_l + 24_f32.to_radians()).abs() < 1e-5);
        // Feet on the ground: a straight leg at angle a reaches down hip * cos(a).
        let hip = 0.9;
        assert!((quarter.drop(hip) - hip * (1.0 - 30_f32.to_radians().cos())).abs() < 1e-6);
        assert_eq!(WalkPose::STAND.drop(hip), 0.0);
    }

    /// The stride matches the swing: one cycle is two steps of `2 * hip * sin(swing)` each for the
    /// trial figure's hip (0.9 m), so the planted foot does not slide (within 3 %).
    #[test]
    fn the_stride_matches_the_leg_swing() {
        let c = cfg();
        let hip = 0.9_f32;
        let ideal = 4.0 * hip * c.leg_swing_deg.to_radians().sin();
        assert!((c.stride_m - ideal).abs() < 0.03 * ideal, "{ideal}");
    }
}
