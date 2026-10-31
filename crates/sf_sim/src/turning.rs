//! Turning the hull (WP4.5): how a unit gets its heading right before, and while,
//! it drives. The rules and the numbers are in `docs/rules/turning.md`; this is
//! the code.
//!
//! Every movement class has a [`Turning`] profile (`turning` in
//! `unit_classes.ron`), and the unit's own `turn_rate` (facing steps per tick,
//! 256 to the turn) says how fast it can turn at best. Three modes:
//!
//! | [`TurnMode`] | Rule | Shipped for |
//! |---|---|---|
//! | `Instant` | the heading snaps onto the wanted one; it never waits to turn | foot |
//! | `Pivot` | turns in place at the turn rate; drives only when the heading is within the `drive_tolerance` of the wanted one, and then steers on at the turn rate | tracked |
//! | `Arc` | **has to move to turn**: it turns by at most what a circle of `min_radius` allows for the distance it covers; a target inside its turning circles is reached by a stand-in three-point turn, a turn on the spot at `pivot_permille` of the turn rate | wheeled, hover, water |
//!
//! Everything is integer. A heading is a [`Facing`] (one of 256); the shortest way
//! round is [`Facing::turn_toward`], which turns the decreasing way when the
//! target is exactly opposite (the fixed tie-break at 128). The turn of an arc is
//! a whole number of steps per tick, so the fraction is carried from tick to tick
//! in the unit's `turn_carry` (in thousandths of a step; it is part of the
//! checksum, in the turning section).

use sf_math::{Facing, Fx, Vec2};

/// The drive tolerance of a class that does not say (and of every entity without
/// a movement class): 16 steps, a bit over 22 degrees. The value the walk had
/// before WP4.5.
pub const DEFAULT_DRIVE_TOLERANCE: u8 = 16;

/// The largest drive tolerance a class may have: a quarter turn.
pub const MAX_DRIVE_TOLERANCE: u8 = 64;

/// How far short of a waypoint (not the last of a route) a unit that has to
/// move to turn counts as having reached it: a quarter of a cell, the margin the
/// routes are smoothed with. A unit cannot land within one step of a point that
/// lies off its line without circling it.
pub const ARC_CAPTURE: Fx = Fx::from_ratio(1, 4);

/// The smallest turning radius an arc class may have, in cells.
pub const MIN_ARC_RADIUS: Fx = Fx::from_ratio(1, 4);

/// Thousandths of a facing step in one cell of path on a circle of radius one
/// cell: 256 steps make a turn and a turn is 2 pi radians, so a radian is
/// 256 / (2 pi) steps, which is about forty and three quarters; rounded to
/// thousandths of a step.
const MILLI_STEPS_PER_RADIAN: i64 = 40_744;

/// How a class turns its hull.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum TurnMode {
    /// Turns in place at the turn rate, drives within the tolerance.
    Pivot,
    /// Must move to turn; radius-limited.
    Arc,
    /// The heading snaps onto the wanted one.
    Instant,
}

impl TurnMode {
    /// The byte the digest writes. Stable.
    pub(crate) const fn tag(self) -> u8 {
        match self {
            TurnMode::Pivot => 0,
            TurnMode::Arc => 1,
            TurnMode::Instant => 2,
        }
    }
}

/// How the units of one movement class turn.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Turning {
    /// The mode.
    pub mode: TurnMode,
    /// The heading error, in facing steps, up to which the unit drives. Larger,
    /// it turns first (`Pivot`) or arcs (`Arc`). Not used by `Instant`.
    pub drive_tolerance: u8,
    /// `Arc` only: the smallest radius of its turns, in cells (at least
    /// [`MIN_ARC_RADIUS`]); zero for the other modes.
    pub min_radius: Fx,
    /// `Arc` only: the share of the unit's turn rate, in thousandths, that it
    /// has when it turns on the spot (1 to 1000); 1000 for the other modes.
    pub pivot_permille: u16,
}

impl Turning {
    /// What the walk did before WP4.5: pivot, drive within
    /// [`DEFAULT_DRIVE_TOLERANCE`]. Every class that says nothing, and every entity
    /// without a movement class (the legacy wanderers), turns like this.
    pub const LEGACY: Turning = Turning {
        mode: TurnMode::Pivot,
        drive_tolerance: DEFAULT_DRIVE_TOLERANCE,
        min_radius: Fx::ZERO,
        pivot_permille: 1000,
    };

    /// A pivoting profile.
    pub const fn pivot(drive_tolerance: u8) -> Turning {
        Turning {
            mode: TurnMode::Pivot,
            drive_tolerance,
            min_radius: Fx::ZERO,
            pivot_permille: 1000,
        }
    }

    /// An instantly turning profile.
    pub const fn instant() -> Turning {
        Turning {
            mode: TurnMode::Instant,
            drive_tolerance: DEFAULT_DRIVE_TOLERANCE,
            min_radius: Fx::ZERO,
            pivot_permille: 1000,
        }
    }

    /// An arc profile.
    pub const fn arc(min_radius: Fx, drive_tolerance: u8, pivot_permille: u16) -> Turning {
        Turning {
            mode: TurnMode::Arc,
            drive_tolerance,
            min_radius,
            pivot_permille,
        }
    }

    /// Whether the numbers hang together; the reason when they do not.
    pub fn check(&self) -> Result<(), TurningError> {
        if self.drive_tolerance > MAX_DRIVE_TOLERANCE {
            return Err(TurningError::Tolerance);
        }
        if self.mode == TurnMode::Arc {
            if self.min_radius < MIN_ARC_RADIUS {
                return Err(TurningError::Radius);
            }
            if !(1..=1000).contains(&self.pivot_permille) {
                return Err(TurningError::Pivot);
            }
        }
        Ok(())
    }

    pub(crate) fn write_hash(&self, h: &mut sf_math::hash::Fnv1a64) {
        h.write_u8(self.mode.tag());
        h.write_u8(self.drive_tolerance);
        h.write_fx(self.min_radius);
        h.write_bytes(&self.pivot_permille.to_le_bytes());
    }
}

/// Why a turning profile was refused.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum TurningError {
    /// The drive tolerance is above [`MAX_DRIVE_TOLERANCE`].
    Tolerance,
    /// An arc with a radius below [`MIN_ARC_RADIUS`].
    Radius,
    /// An arc whose pivot share is not in 1 to 1000: it could never turn on the
    /// spot, and a target inside its turning circles would be out of reach.
    Pivot,
}

impl core::fmt::Display for TurningError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TurningError::Tolerance => {
                write!(f, "drive tolerance is above {MAX_DRIVE_TOLERANCE} steps")
            }
            TurningError::Radius => write!(f, "an arc needs a radius of at least a quarter cell"),
            TurningError::Pivot => write!(f, "an arc needs a pivot share of 1 to 1000"),
        }
    }
}

impl std::error::Error for TurningError {}

/// What one tick of steering decided.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct Heading {
    /// The heading after the tick's turn.
    pub facing: Facing,
    /// The fraction of a step carried to the next tick, in thousandths.
    pub carry: u16,
    /// Whether the unit drives this tick.
    pub drive: bool,
}

/// Adds `milli` thousandths of a step to the `carry`, turns `facing` toward
/// `wanted` by the whole steps that make, and keeps the rest. Reaching `wanted`
/// clears the carry: a unit that is on its heading has nothing stored.
fn turn_carrying(facing: Facing, carry: u16, wanted: Facing, milli: i64) -> (Facing, u16) {
    let total = i64::from(carry) + milli;
    let steps = (total / 1000).min(i64::from(u8::MAX)) as u8;
    let turned = facing.turn_toward(wanted, steps);
    if turned == wanted {
        (turned, 0)
    } else {
        (turned, (total % 1000) as u16)
    }
}

/// A turn on the spot at the unit's `turn_per_tick`, for the unit that is
/// standing and wants to face `wanted`: a pivot at the full rate, an instant turn
/// at once, an arc at its pivot share.
pub(crate) fn turn_in_place(
    turning: &Turning,
    facing: Facing,
    carry: u16,
    wanted: Facing,
    turn_per_tick: u8,
) -> (Facing, u16) {
    match turning.mode {
        TurnMode::Instant => (wanted, 0),
        TurnMode::Pivot => (facing.turn_toward(wanted, turn_per_tick), 0),
        TurnMode::Arc => turn_carrying(
            facing,
            carry,
            wanted,
            i64::from(turn_per_tick) * i64::from(turning.pivot_permille),
        ),
    }
}

/// Thousandths of a step an arc may turn while the unit covers `distance`.
fn arc_milli(distance: Fx, min_radius: Fx) -> i64 {
    distance.raw() * MILLI_STEPS_PER_RADIAN / min_radius.raw().max(1)
}

/// Whether `target` lies inside one of the two circles of the smallest turn,
/// which touch the unit's position on either side of its heading: a unit that
/// can only turn in an arc goes round such a point and never reaches it. The
/// test is `a^2 + (|b| - r)^2 < r^2`, that is `a^2 + b^2 < 2 r |b|`, with `a`
/// along the heading and `b` across it. `r` is the radius the unit really
/// turns at: its minimum, or larger when its turn rate cannot follow it.
fn inside_turning_circles(
    turning: &Turning,
    facing: Facing,
    pos: Vec2,
    target: Vec2,
    step: Fx,
    turn_per_tick: u8,
) -> bool {
    let rate_radius = Fx::from_raw(
        step.raw() * MILLI_STEPS_PER_RADIAN / (i64::from(turn_per_tick.max(1)) * 1000),
    );
    let r = turning.min_radius.max(rate_radius);
    let delta = target - pos;
    let across = facing.to_vec2().cross(delta).abs();
    delta.length_squared() < r.mul_int(2).saturating_mul(across)
}

/// A unit as the steering sees it for one tick.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Mover {
    /// Where it stands.
    pub pos: Vec2,
    /// Its heading.
    pub facing: Facing,
    /// The fraction of a step carried from the tick before, in thousandths.
    pub carry: u16,
    /// How far it drives this tick, in cells (terrain included).
    pub step: Fx,
    /// Its turn rate, in facing steps per tick.
    pub turn_per_tick: u8,
}

/// One tick of steering for `unit` walking toward `target`, which lies in the
/// direction `wanted`.
///
/// `#[inline]`: without it the compiler stopped inlining this into
/// `behavior::walk` once `harvest.rs` grew (WP7.5), +3.1 % instructions on
/// `wander_5000`.
#[inline]
pub(crate) fn steer(turning: &Turning, unit: &Mover, wanted: Facing, target: Vec2) -> Heading {
    let Mover {
        pos,
        facing,
        carry,
        step,
        turn_per_tick,
    } = *unit;
    let error = facing.abs_diff(wanted);
    match turning.mode {
        TurnMode::Instant => Heading {
            facing: wanted,
            carry: 0,
            drive: true,
        },
        TurnMode::Pivot => {
            let turned = facing.turn_toward(wanted, turn_per_tick);
            Heading {
                facing: turned,
                carry: 0,
                drive: turned.abs_diff(wanted) <= turning.drive_tolerance,
            }
        }
        TurnMode::Arc => {
            let off = error > turning.drive_tolerance;
            if off && inside_turning_circles(turning, facing, pos, target, step, turn_per_tick) {
                let (facing, carry) = turn_in_place(turning, facing, carry, wanted, turn_per_tick);
                return Heading {
                    facing,
                    carry,
                    drive: false,
                };
            }
            let milli = arc_milli(step, turning.min_radius).min(i64::from(turn_per_tick) * 1000);
            let (facing, carry) = turn_carrying(facing, carry, wanted, milli);
            Heading {
                facing,
                carry,
                drive: true,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHEELED: Turning = Turning::arc(Fx::from_int(2), 16, 250);

    fn mover(facing: Facing, carry: u16, step: Fx, turn_per_tick: u8) -> Mover {
        Mover {
            pos: Vec2::ZERO,
            facing,
            carry,
            step,
            turn_per_tick,
        }
    }

    #[test]
    fn the_legacy_profile_is_the_old_walk() {
        assert_eq!(Turning::LEGACY.mode, TurnMode::Pivot);
        assert_eq!(Turning::LEGACY.drive_tolerance, crate::ALIGN_TOLERANCE);
        assert_eq!(Turning::LEGACY.check(), Ok(()));
    }

    #[test]
    fn bad_profiles_are_refused() {
        assert_eq!(
            Turning::pivot(MAX_DRIVE_TOLERANCE + 1).check(),
            Err(TurningError::Tolerance)
        );
        assert_eq!(
            Turning::arc(Fx::ZERO, 8, 500).check(),
            Err(TurningError::Radius)
        );
        assert_eq!(
            Turning::arc(Fx::from_int(2), 8, 0).check(),
            Err(TurningError::Pivot)
        );
        assert_eq!(
            Turning::arc(Fx::from_int(2), 8, 1001).check(),
            Err(TurningError::Pivot)
        );
        assert_eq!(WHEELED.check(), Ok(()));
        assert!(!TurningError::Pivot.to_string().is_empty());
    }

    #[test]
    fn instant_snaps_and_pivot_turns_at_the_rate() {
        let target = Vec2::from_ints(0, 5);
        let wanted = Facing::PLUS_Y;
        let step = Fx::from_ratio(1, 10);
        let h = steer(
            &Turning::instant(),
            &mover(Facing::PLUS_X, 0, step, 3),
            wanted,
            target,
        );
        assert_eq!((h.facing, h.drive), (wanted, true));
        let p = Turning::pivot(8);
        // 64 steps off, 10 per tick: turned to 10, still 54 off: no driving.
        let h = steer(&p, &mover(Facing::PLUS_X, 0, step, 10), wanted, target);
        assert_eq!((h.facing, h.drive), (Facing::new(10), false));
        // 14 off after turning 10 is still above 8, 4 off is not.
        let h = steer(&p, &mover(Facing::new(40), 0, step, 10), wanted, target);
        assert_eq!((h.facing, h.drive), (Facing::new(50), false));
        let h = steer(&p, &mover(Facing::new(54), 0, step, 10), wanted, target);
        assert_eq!((h.facing, h.drive), (Facing::new(64), true));
    }

    #[test]
    fn an_arc_turns_by_the_circle_and_carries_the_fraction() {
        // Radius two cells, a step of 6553 raw units (a tenth of a cell): the
        // heading may change by 6553 * 40744 / 131072 thousandths of a step.
        let step = Fx::from_raw(6553);
        let per_tick = 6553 * 40_744 / 131_072;
        assert_eq!(per_tick, 2037);
        // The target far off to the left of a unit heading east: no circle in the
        // way, it arcs.
        let target = Vec2::from_ints(0, 40);
        let (mut facing, mut carry) = (Facing::PLUS_X, 0);
        let mut turned = Vec::new();
        for _ in 0..4 {
            let h = steer(
                &WHEELED,
                &mover(facing, carry, step, 8),
                Facing::PLUS_Y,
                target,
            );
            assert!(h.drive);
            turned.push(i32::from(h.facing.raw()) - i32::from(facing.raw()));
            (facing, carry) = (h.facing, h.carry);
        }
        assert_eq!(
            turned.iter().sum::<i32>(),
            4 * per_tick / 1000,
            "{turned:?}"
        );
        assert!(turned.iter().all(|&t| t == 2 || t == 3), "{turned:?}");
        // The turn rate caps it: a slow turner follows the circle only so far.
        let h = steer(
            &WHEELED,
            &mover(Facing::PLUS_X, 0, Fx::HALF, 3),
            Facing::PLUS_Y,
            target,
        );
        assert_eq!(h.facing, Facing::new(3));
    }

    #[test]
    fn a_target_inside_the_turning_circles_is_reached_by_a_turn_on_the_spot() {
        let step = Fx::from_raw(6553);
        // A cell to the left of a unit that heads east, radius two cells: inside.
        let near = Vec2::from_ints(0, 1);
        let h = steer(
            &WHEELED,
            &mover(Facing::PLUS_X, 0, step, 8),
            Facing::PLUS_Y,
            near,
        );
        assert!(!h.drive);
        // The pivot share is a quarter of 8 steps: two steps a tick.
        assert_eq!(h.facing, Facing::new(2));
        // Straight ahead and near is not inside a circle: it drives.
        let ahead = Vec2::from_ints(1, 0);
        let h = steer(
            &WHEELED,
            &mover(Facing::PLUS_X, 0, step, 8),
            Facing::PLUS_X,
            ahead,
        );
        assert!(h.drive);
    }

    #[test]
    fn halfway_round_the_decreasing_way() {
        let (f, _) = turn_in_place(&Turning::pivot(8), Facing::new(0), 0, Facing::new(128), 5);
        assert_eq!(f, Facing::new(251));
        let (f, _) = turn_in_place(&Turning::instant(), Facing::new(9), 0, Facing::new(137), 5);
        assert_eq!(f, Facing::new(137));
    }

    #[test]
    fn the_carry_is_dropped_on_the_heading() {
        let (f, c) = turn_carrying(Facing::new(10), 900, Facing::new(11), 500);
        assert_eq!((f, c), (Facing::new(11), 0));
        let (f, c) = turn_carrying(Facing::new(10), 900, Facing::new(40), 500);
        assert_eq!((f, c), (Facing::new(11), 400));
    }
}
