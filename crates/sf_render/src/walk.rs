//! The procedural walk cycle of a figure (WP11.6i): rigid node animation, render only.
//!
//! A figure model whose eight figure nodes have their origins at the joints (the glb loader checks
//! it, [`crate::glb::figure_joints`]) gets a [`Joint::Figure`] on each of them. Per unit the
//! instancer keeps a [`WalkState`]: the **phase** of the cycle advances with the distance the unit
//! travelled between the interpolated positions of two frames, one cycle per
//! [`WalkConfig::stride_m`] (two steps), so a slower unit steps slower. Wall time only drives the
//! **weight**: a moving unit blends into the cycle and a still one back to its stand pose in
//! [`WalkConfig::blend_s`].
//!
//! **Feet** (round 2): each foot follows a path relative to its hip ([`foot`]). Half the cycle it
//! is the stance foot: it slides back under the hip at exactly the speed the body moves forward,
//! so in the world it stays planted. The other half it swings forward on a smooth curve, lifted by
//! [`WalkConfig::lift_m`]. The hip sits as high as the stance leg reaches at
//! [`WalkConfig::knee_reach`] of its length (the bob), and a two-bone solve ([`solve`]) turns the
//! thigh at the hip and the shin at the knee so the ankle lands on its path. The torso leans
//! forward about the waist and carries the head and the shoulders with it; each arm swings
//! against the leg on its side, the rifle arm less. Nothing here reaches the simulation.

use std::f32::consts::{PI, TAU};

use glam::{Mat4, Vec3};
use serde::Deserialize;

use crate::model::{Joint, Limb, RigPart};

/// The shipped walk settings, compiled in like the other render settings.
pub const EMBEDDED_FIGURE_MOTION: &str = include_str!("../../../assets/config/figure_motion.ron");

/// `assets/config/figure_motion.ron`.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WalkConfig {
    /// Swing of the free arm about the shoulder, either way, degrees.
    pub arm_swing_deg: f32,
    /// Swing of the arm that carries the rifle (`arm_r`), degrees.
    pub rifle_arm_swing_deg: f32,
    /// Forward lean of the torso, head and shoulders at full walk, degrees.
    pub lean_deg: f32,
    /// Ground covered by one full cycle (two steps) at true model size, metres.
    pub stride_m: f32,
    /// How high the swing foot lifts at mid swing, metres.
    pub lift_m: f32,
    /// The stance leg reaches this share of its full length (a soft knee); sets the bob.
    pub knee_reach: f32,
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

    /// Parse and check: every angle in `0..=60` degrees, the lengths and the blend time positive,
    /// `knee_reach` in `0.8..=1`.
    pub fn from_ron(text: &str) -> Result<WalkConfig, String> {
        let c: WalkConfig = ron::from_str(text).map_err(|e| format!("figure_motion.ron: {e}"))?;
        for (name, deg) in [
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
            ("lift_m", c.lift_m),
            ("blend_s", c.blend_s),
            ("max_step_m", c.max_step_m),
        ] {
            if !(v.is_finite() && v > 0.0) {
                return Err(format!("figure_motion.ron: {name} must be positive"));
            }
        }
        if !(0.8..=1.0).contains(&c.knee_reach) {
            return Err("figure_motion.ron: knee_reach must be 0.8..=1".into());
        }
        Ok(c)
    }
}

/// What the walk asks of a figure in one frame: where in the cycle, how much of the walk, and the
/// settings that shape it (lengths in model metres). [`WalkPose::STAND`] is the rest pose.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WalkPose {
    /// `0..1`; at 0 the left foot is fully forward and lands.
    pub phase: f32,
    /// 0 = standing, 1 = full walk.
    pub weight: f32,
    pub stride: f32,
    pub lift: f32,
    pub knee_reach: f32,
    /// Arm angles about the shoulders, radians (positive swings the hand forward).
    pub arm_l: f32,
    pub arm_r: f32,
    /// Forward lean of the torso, radians.
    pub lean: f32,
}

impl WalkPose {
    /// The rest pose: every joint as in the file.
    pub const STAND: WalkPose = WalkPose {
        phase: 0.0,
        weight: 0.0,
        stride: 0.0,
        lift: 0.0,
        knee_reach: 0.0,
        arm_l: 0.0,
        arm_r: 0.0,
        lean: 0.0,
    };
}

/// The pose at `phase` (cycles) with `weight` (0 = stand, 1 = full walk). Each arm swings against
/// the foot on its side: the left foot is forward at phase 0, so the left arm is back.
pub fn walk_pose(phase: f32, weight: f32, cfg: &WalkConfig) -> WalkPose {
    let w = weight.clamp(0.0, 1.0);
    if w == 0.0 {
        return WalkPose::STAND;
    }
    let c = (phase * TAU).cos() * w;
    WalkPose {
        phase: phase.rem_euclid(1.0),
        weight: w,
        stride: cfg.stride_m,
        lift: cfg.lift_m,
        knee_reach: cfg.knee_reach,
        arm_l: -cfg.arm_swing_deg.to_radians() * c,
        arm_r: cfg.rifle_arm_swing_deg.to_radians() * c,
        lean: cfg.lean_deg.to_radians() * w,
    }
}

/// The ankle of foot `side` (0 left, 1 right) relative to its hip along the walk (`+X`, metres)
/// and its lift above the ground: the stance half moves it back linearly at the body's speed (a
/// planted foot), the swing half brings it forward on a smooth curve, lifted at mid swing.
pub fn foot(pose: &WalkPose, side: usize) -> (f32, f32) {
    let phi = (pose.phase + 0.5 * side as f32).rem_euclid(1.0);
    let s = 0.5 * pose.stride;
    let (x, lift) = if phi < 0.5 {
        (s * (0.5 - 2.0 * phi), 0.0)
    } else {
        let u = (phi - 0.5) * 2.0;
        (
            -0.5 * s + s * u * u * (3.0 - 2.0 * u),
            pose.lift * (PI * u).sin(),
        )
    };
    (x * pose.weight, lift * pose.weight)
}

/// The joints of a figure in its rest pose (model metres): the hips and knees (left, right) and
/// the waist.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FigureRig {
    pub hip: [Vec3; 2],
    pub knee: [Vec3; 2],
    pub waist: Vec3,
}

impl FigureRig {
    /// From a level's parts: `None` unless it has both thighs, both shins and the torso as
    /// figure parts.
    pub fn of(parts: &[RigPart]) -> Option<FigureRig> {
        let at = |limb: Limb| {
            parts
                .iter()
                .find(|p| p.joint == Joint::Figure(limb))
                .map(|p| p.rest.w_axis.truncate())
        };
        Some(FigureRig {
            hip: [at(Limb::LegL)?, at(Limb::LegR)?],
            knee: [at(Limb::ShinL)?, at(Limb::ShinR)?],
            waist: at(Limb::Torso)?,
        })
    }
}

/// A solved walk pose for one rig: how far the figure drops and the thigh and shin angles per
/// side (radians from hanging straight down, positive forward), plus the pose's arms and lean.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Solved {
    pub drop: f32,
    pub thigh: [f32; 2],
    pub shin: [f32; 2],
    pub pose: WalkPose,
}

/// Solve the legs of `rig` for `pose`: the hip at the height the stance leg reaches, then for
/// each leg the two-bone angles that put its ankle (the shin's end at the ground) on [`foot`].
pub fn solve(pose: &WalkPose, rig: &FigureRig) -> Solved {
    let l1 = (rig.hip[0].y - rig.knee[0].y).max(1e-3);
    let l2 = rig.knee[0].y.max(1e-3);
    let len = l1 + l2;
    let feet = [foot(pose, 0), foot(pose, 1)];
    let stance = if pose.phase < 0.5 { 0 } else { 1 };
    let reach = len * (1.0 - pose.weight * (1.0 - pose.knee_reach));
    let xs = feet[stance].0.min(reach * 0.999);
    let hip = (reach * reach - xs * xs).sqrt();
    let mut thigh = [0.0; 2];
    let mut shin = [0.0; 2];
    for side in 0..2 {
        let (dx, lift) = feet[side];
        let dy = lift - hip;
        let d = dx.hypot(dy).clamp((l1 - l2).abs() + 1e-4, len * 0.9999);
        let alpha = dx.atan2(-dy);
        let b = ((l1 * l1 + d * d - l2 * l2) / (2.0 * l1 * d))
            .clamp(-1.0, 1.0)
            .acos();
        let k = ((l1 * l1 + l2 * l2 - d * d) / (2.0 * l1 * l2))
            .clamp(-1.0, 1.0)
            .acos();
        thigh[side] = alpha + b;
        shin[side] = thigh[side] - (PI - k);
    }
    Solved {
        drop: len - hip,
        thigh,
        shin,
        pose: *pose,
    }
}

/// The model matrix of a figure part (its rest matrix `rest`) under the solved walk.
pub fn limb_matrix(limb: Limb, rest: Mat4, rig: &FigureRig, s: &Solved) -> Mat4 {
    let drop = Mat4::from_translation(Vec3::new(0.0, -s.drop, 0.0));
    let lean = Mat4::from_translation(rig.waist)
        * Mat4::from_rotation_z(-s.pose.lean)
        * Mat4::from_translation(-rig.waist);
    let rz = Mat4::from_rotation_z;
    match limb {
        Limb::LegL | Limb::LegR => {
            let side = usize::from(limb == Limb::LegR);
            drop * rest * rz(s.thigh[side])
        }
        Limb::ShinL | Limb::ShinR => {
            let side = usize::from(limb == Limb::ShinR);
            let hip = rig.hip[side];
            drop * Mat4::from_translation(hip)
                * rz(s.thigh[side])
                * Mat4::from_translation(-hip)
                * rest
                * rz(s.shin[side] - s.thigh[side])
        }
        Limb::Torso | Limb::Head => drop * lean * rest,
        Limb::ArmL => drop * lean * rest * rz(s.pose.arm_l),
        Limb::ArmR => drop * lean * rest * rz(s.pose.arm_r),
    }
}

/// The walk state of one unit, kept by the instancer between frames.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WalkState {
    /// Where in the cycle, `0..1`.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> WalkConfig {
        WalkConfig::embedded()
    }

    /// The trial figure's joints (`units_infantry::JOINTS`, metres).
    fn rig() -> FigureRig {
        FigureRig {
            hip: [Vec3::new(0.0, 0.92, -0.1), Vec3::new(0.0, 0.92, 0.1)],
            knee: [Vec3::new(0.0, 0.495, -0.1), Vec3::new(0.0, 0.495, 0.1)],
            waist: Vec3::new(0.0, 0.98, 0.0),
        }
    }

    /// The world position of the left (0) or right (1) ankle of a body at `x` in pose `p`.
    fn ankle(x: f32, p: &WalkPose, side: usize) -> Vec3 {
        let r = rig();
        let s = solve(p, &r);
        let limb = [Limb::ShinL, Limb::ShinR][side];
        let m = Mat4::from_translation(Vec3::new(x, 0.0, 0.0))
            * limb_matrix(limb, Mat4::from_translation(r.knee[side]), &r, &s);
        m.transform_point3(Vec3::new(0.0, -r.knee[side].y, 0.0))
    }

    #[test]
    fn the_shipped_settings_are_pinned() {
        let c = cfg();
        assert_eq!(c.arm_swing_deg, 18.0);
        assert_eq!(c.rifle_arm_swing_deg, 4.0);
        assert_eq!(c.lean_deg, 5.0);
        assert_eq!(c.stride_m, 1.6);
        assert_eq!(c.lift_m, 0.14);
        assert_eq!(c.knee_reach, 0.97);
        assert_eq!(c.blend_s, 0.25);
        assert_eq!(c.max_step_m, 2.0);
    }

    #[test]
    fn a_bad_setting_is_refused() {
        for (from, to) in [
            ("stride_m: 1.60", "stride_m: 0.0"),
            ("lean_deg: 5.0", "lean_deg: 90.0"),
            ("knee_reach: 0.97", "knee_reach: 1.2"),
        ] {
            assert!(EMBEDDED_FIGURE_MOTION.contains(from), "{from}");
            let text = EMBEDDED_FIGURE_MOTION.replace(from, to);
            assert!(WalkConfig::from_ron(&text).is_err(), "{to}");
        }
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
        let before = slow.phase;
        for _ in 0..100 {
            slow.advance([c.stride_m * 0.0125 * 40.0, 0.0], 1.0, 0.1, &c);
        }
        assert_eq!(slow.phase, before);
        let mut big = WalkState::default();
        big.advance([0.0, 0.0], 2.0, 0.016, &c);
        big.advance([c.stride_m * 0.5, 0.0], 2.0, 0.016, &c);
        assert!((big.phase - 0.25).abs() < 1e-4);
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
        let mut last = f32::MAX;
        for _ in 0..16 {
            s.advance([0.87, 0.0], 1.0, 1.0 / 60.0, &c);
            let now = solve(&s.pose(&c), &rig()).drop;
            assert!(now <= last + 1e-6, "{now} > {last}");
            last = now;
        }
        assert_eq!(s.weight, 0.0);
        assert_eq!(s.pose(&c), WalkPose::STAND);
    }

    /// The two feet run the same path half a cycle apart, and each arm swings against the foot on
    /// its side.
    #[test]
    fn the_feet_are_opposite_and_each_arm_counters_its_foot() {
        let c = cfg();
        for k in 0..16 {
            let a = walk_pose(k as f32 / 16.0, 1.0, &c);
            let b = walk_pose(k as f32 / 16.0 + 0.5, 1.0, &c);
            let (l, r) = (foot(&a, 0), foot(&b, 1));
            assert!((l.0 - r.0).abs() < 1e-5 && (l.1 - r.1).abs() < 1e-5);
            assert!(a.arm_l * foot(&a, 0).0 <= 1e-6, "{a:?}");
            assert!(a.arm_r * foot(&a, 1).0 <= 1e-6, "{a:?}");
        }
        assert!(foot(&walk_pose(0.0, 1.0, &c), 0).0 > 0.0);
    }

    /// The stance foot stays planted: the body moves on with the phase and the ankle of the
    /// stance leg stays within 3 mm of where it landed, on the ground; the swing foot lifts.
    #[test]
    fn the_stance_foot_stays_planted_and_the_swing_foot_lifts() {
        let c = cfg();
        let at = |k: u32| -> (f32, WalkPose) {
            let phase = k as f32 / 100.0;
            (phase * c.stride_m, walk_pose(phase, 1.0, &c))
        };
        let (x0, p0) = at(0);
        let land = ankle(x0, &p0, 0);
        for k in 0..50 {
            let (x, p) = at(k);
            let a = ankle(x, &p, 0);
            assert!((a - land).length() < 0.003, "phase {k}: {a} from {land}");
            assert!(a.y.abs() < 0.003, "{a}");
        }
        let (x, p) = at(75);
        assert!(ankle(x, &p, 0).y > 0.1, "the swing foot lifts");
    }

    /// A positive lean takes the head forward (`+X`) and down, about the waist.
    #[test]
    fn the_lean_takes_the_head_and_shoulders_forward() {
        let c = cfg();
        let r = rig();
        let p = walk_pose(0.25, 1.0, &c);
        assert!(p.lean > 0.0);
        let s = solve(&p, &r);
        let neck = Mat4::from_translation(Vec3::new(0.0, 1.49, 0.0));
        let head = limb_matrix(Limb::Head, neck, &r, &s).transform_point3(Vec3::ZERO);
        assert!(head.x > 0.03, "{head}");
        let shoulder = Mat4::from_translation(Vec3::new(0.0, 1.42, 0.215));
        let arm = limb_matrix(Limb::ArmR, shoulder, &r, &s).transform_point3(Vec3::ZERO);
        // The shoulder moves with the torso: the same lean, no seam.
        let torso = limb_matrix(Limb::Torso, Mat4::from_translation(r.waist), &r, &s);
        let on_torso = torso.transform_point3(Vec3::new(0.0, 0.44, 0.215));
        assert!((arm - on_torso).length() < 1e-5, "{arm} {on_torso}");
    }

    /// The stride is reachable: at full walk the stance leg never needs more than its length.
    #[test]
    fn the_stride_is_reachable_for_the_trial_figure() {
        let c = cfg();
        let len = 0.92;
        let s = 0.5 * c.stride_m;
        assert!(0.5 * s < c.knee_reach * len * 0.7, "{s}");
    }
}
