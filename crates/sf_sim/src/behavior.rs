//! The placeholder behaviours ("wander" and "move to") and the area type they
//! use.
//!
//! This is engine-level scaffolding so that thousands of entities move without
//! any gameplay. Collision arrives in phase 4 and replaces `advance_all`.
//! Terrain (WP4.1) reaches only entities made from the unit table that have a
//! movement class: see "Terrain" below. The wanderers of the legacy `Spawn`
//! command know no terrain at all.
//!
//! # The motion model
//!
//! Every entity that has a target walks to it the same way each tick:
//!
//! * If the target is within one step (`distance <= step`, compared as squared
//!   lengths so no square root is needed), the entity snaps onto it and has
//!   *arrived*.
//! * Otherwise it turns toward the target: the facing closest to the
//!   direction of the target is found with [`sf_math::Facing::from_vec2`] and
//!   the entity turns toward it by at most `turn_per_tick` steps.
//! * Once its facing is within [`ALIGN_TOLERANCE`] steps of that direction
//!   (a bit over 22 degrees) it moves one step along its facing, which is a
//!   table lookup (`Vec2::from_facing`) and needs no square root or
//!   division. Until then it turns on the spot.
//! * The new position is clamped into the map.
//!
//! Because the facing is re-aimed at the target every tick, the path is a
//! straight line up to the 1/256 turn resolution of the facing. A step along a
//! facing at most 22 degrees off the target, taken from farther away than one
//! step, always shortens the distance by more than half a step, and clamping
//! into the (convex) map can only shorten it further, so arrival is
//! guaranteed. The length of a step is the configured speed per tick times
//! the table vector's length, which is one to within about three parts in a hundred thousand.
//!
//! # Terrain (WP4.1)
//!
//! An entity with a movement class moves at its speed per tick times the
//! multiplier of the cell it stands in, and a step that would end in a cell it
//! may not enter is not taken ([`crate::movement`] has the exact rules): the
//! entity keeps its position and, having turned toward the waypoint, stays
//! there until the order changes. That is [`Walk::Blocked`]; the caller decides
//! what a blocked walk means (a wanderer draws a new target, a move keeps its
//! target, an order keeps the order). An entity without a movement class takes
//! exactly the steps it always took.
//!
//! Wander adds one rule: with no target, draw one inside the entity's own
//! [`Area`] from the entity's own random stream (x first, then y). On arrival
//! the target is cleared and the next tick draws a new one. `MoveTo` has a
//! fixed target and turns the entity [`Behavior::Idle`] on arrival.

use sf_math::{Fx, Rng, Vec2};

use crate::entities::{Entities, NO_DEF};
use crate::ids::Tick;
use crate::movement::{Ground, scale_step};
use crate::orders;
use crate::path::Pathing;
use crate::units::UnitTable;

/// What an entity does each tick.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Behavior {
    /// Stands still.
    Idle,
    /// Walks to random targets inside its [`Area`], forever.
    Wander,
    /// Walks to one target, then becomes [`Behavior::Idle`].
    MoveTo,
    /// Follows its current order and queue (WP3.4, [`crate::orders`]). Idle
    /// exactly when the order is [`Order::Idle`](crate::Order::Idle).
    Ordered,
}

impl Behavior {
    /// The byte the checksum writes for this behaviour. Stable: changing a
    /// value changes every checksum.
    pub(crate) const fn tag(self) -> u8 {
        match self {
            Behavior::Idle => 0,
            Behavior::Wander => 1,
            Behavior::MoveTo => 2,
            Behavior::Ordered => 3,
        }
    }
}

/// An axis-aligned rectangle of the map, in cells: `min` inclusive, `max`
/// exclusive when a point is drawn from it.
///
/// ```
/// use sf_sim::Area;
/// let a = Area::from_cells(10, 20, 30, 40);
/// assert!(!a.is_empty());
/// // Corners given the wrong way round are put in order.
/// assert_eq!(Area::from_cells(30, 40, 10, 20), a);
/// ```
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Area {
    /// The corner with the smallest coordinates.
    pub min: Vec2,
    /// The corner with the largest coordinates.
    pub max: Vec2,
}

impl Area {
    /// An area from two opposite corners, in either order.
    #[inline]
    pub const fn new(a: Vec2, b: Vec2) -> Area {
        Area {
            min: a.min(b),
            max: a.max(b),
        }
    }

    /// An area from whole-cell corners `(x0, y0)` and `(x1, y1)`.
    #[inline]
    pub const fn from_cells(x0: i32, y0: i32, x1: i32, y1: i32) -> Area {
        Area::new(Vec2::from_ints(x0, y0), Vec2::from_ints(x1, y1))
    }

    /// True if no point can be drawn from it (zero width or height).
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.max.x.raw() <= self.min.x.raw() || self.max.y.raw() <= self.min.y.raw()
    }

    /// The part of `self` inside `bounds`, with corners in order. An area
    /// completely outside collapses onto the nearest edge of `bounds` and is
    /// then empty.
    #[inline]
    pub const fn clamped_to(self, bounds: Area) -> Area {
        let a = self.min.clamp(bounds.min, bounds.max);
        let b = self.max.clamp(bounds.min, bounds.max);
        Area::new(a, b)
    }

    /// A uniformly random point of the area (every raw fixed-point value
    /// equally likely), `x` drawn before `y`. On an empty axis the result is
    /// the `min` coordinate, without consuming randomness.
    #[inline]
    pub fn sample(self, rng: &mut Rng) -> Vec2 {
        let x = rng.fx_range(self.min.x, self.max.x);
        let y = rng.fx_range(self.min.y, self.max.y);
        Vec2::new(x, y)
    }
}

/// How far (in facing steps, 256 per turn) the heading may be off the
/// direction of the target while the entity still moves.
pub const ALIGN_TOLERANCE: u8 = 16;

/// Everything [`advance_all`] needs, derived once per tick from the world
/// configuration.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Motion {
    /// Distance covered per tick.
    pub step: Fx,
    /// `step` squared, for the arrival test.
    pub step_sq: Fx,
    /// Facing steps the heading may turn per tick.
    pub turn_per_tick: u8,
    /// Smallest valid position.
    pub pos_min: Vec2,
    /// Largest valid position (one raw unit inside the far edge of the map).
    pub pos_max: Vec2,
    /// The movement class of the entity, if it has one (WP4.1): only an entity
    /// made from the unit table has, and only that one is bound by terrain.
    pub class: Option<u16>,
}

impl Motion {
    /// The motion of one entity: the world's own speed and turn rate for an
    /// entity without a unit table entry, the entry's speed per tick and turn
    /// rate for a unit made from one (WP3.2).
    pub(crate) fn of_entity(&self, def: u32, units: &UnitTable) -> Motion {
        if def == NO_DEF {
            return *self;
        }
        let stats = units.at(def as usize);
        Motion {
            step: stats.speed_per_tick,
            step_sq: stats.speed_per_tick.square(),
            turn_per_tick: stats.turn_per_tick,
            class: stats.movement_class,
            ..*self
        }
    }
}

/// Runs one tick of behaviour for every live entity, in ascending slot
/// order. Also records the previous position and facing for interpolation.
pub(crate) fn advance_all(
    e: &mut Entities,
    world_motion: &Motion,
    units: &UnitTable,
    ground: &Ground<'_>,
    path: &mut Pathing,
    tick: Tick,
) {
    let slots = e.len();
    for i in 0..slots {
        if !e.alive[i] {
            continue;
        }
        e.prev_pos[i] = e.pos[i];
        e.prev_facing[i] = e.facing[i];
        if e.behavior[i] == Behavior::Idle {
            continue;
        }
        let m = &world_motion.of_entity(e.def[i], units);
        match e.behavior[i] {
            Behavior::Idle => {}
            Behavior::Wander => {
                let target = match e.target[i] {
                    Some(t) => t,
                    None => {
                        let t = e.area[i].sample(&mut e.rng[i]).clamp(m.pos_min, m.pos_max);
                        e.target[i] = Some(t);
                        t
                    }
                };
                // Arrived, or blocked by terrain: either way a new target.
                if walk(e, i, target, m, ground) != Walk::Moving {
                    e.target[i] = None;
                }
            }
            Behavior::MoveTo => match e.target[i] {
                Some(target) => {
                    // A blocked move keeps its target and stands at the edge.
                    if walk(e, i, target, m, ground) == Walk::Arrived {
                        e.target[i] = None;
                        e.behavior[i] = Behavior::Idle;
                    }
                }
                None => e.behavior[i] = Behavior::Idle,
            },
            Behavior::Ordered => orders::run(e, i, m, units, ground, path, tick),
        }
    }
}

/// How one tick of walking went.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Walk {
    /// The entity is on the target.
    Arrived,
    /// It is on its way: it moved, or turned toward the target.
    Moving,
    /// The step it wanted would end in a cell its movement class may not
    /// enter. It did not move (it did turn).
    Blocked,
}

/// One tick of walking entity `i` toward `target`.
pub(crate) fn walk(
    e: &mut Entities,
    i: usize,
    target: Vec2,
    m: &Motion,
    ground: &Ground<'_>,
) -> Walk {
    let pos = e.pos[i];
    // An entity with a movement class goes at the speed of the cell it stands
    // in; any other at the speed it always had.
    let (step, step_sq) = match m.class {
        Some(class) => {
            let step = scale_step(m.step, ground.permille_at(class, pos));
            (step, step.square())
        }
        None => (m.step, m.step_sq),
    };
    let delta = target - pos;
    let (next, arrived) = if delta.length_squared() <= step_sq {
        (target, true)
    } else {
        // `delta` is not the zero vector here: its squared length is above
        // `step_sq`, which is not negative.
        match delta.facing() {
            None => (target, true),
            Some(wanted) => {
                let facing = e.facing[i].turn_toward(wanted, m.turn_per_tick);
                e.facing[i] = facing;
                if facing.abs_diff(wanted) > ALIGN_TOLERANCE {
                    return Walk::Moving;
                }
                let next = (pos + Vec2::from_facing(facing, step)).clamp(m.pos_min, m.pos_max);
                (next, false)
            }
        }
    };
    if let Some(class) = m.class
        && !ground.may_step(class, pos, next)
    {
        return Walk::Blocked;
    }
    e.pos[i] = next;
    if arrived { Walk::Arrived } else { Walk::Moving }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn area_new_orders_corners_and_detects_empty() {
        let a = Area::new(Vec2::from_ints(5, 9), Vec2::from_ints(2, 3));
        assert_eq!(a.min, Vec2::from_ints(2, 3));
        assert_eq!(a.max, Vec2::from_ints(5, 9));
        assert!(!a.is_empty());
        assert!(Area::from_cells(4, 4, 4, 9).is_empty());
        assert!(Area::from_cells(4, 4, 9, 4).is_empty());
    }

    #[test]
    fn area_clamping_keeps_inside_and_collapses_outside() {
        let map = Area::from_cells(0, 0, 100, 100);
        let inside = Area::from_cells(10, 10, 20, 20);
        assert_eq!(inside.clamped_to(map), inside);
        let straddling = Area::from_cells(-50, 90, 50, 500);
        assert_eq!(straddling.clamped_to(map), Area::from_cells(0, 90, 50, 100));
        let outside = Area::from_cells(300, 300, 400, 400);
        let c = outside.clamped_to(map);
        assert!(c.is_empty());
        assert_eq!(c.min, Vec2::from_ints(100, 100));
    }

    #[test]
    fn sampling_stays_inside_and_empty_axes_cost_nothing() {
        let area = Area::from_cells(10, 20, 12, 22);
        let mut rng = Rng::new(1);
        for _ in 0..1000 {
            let p = area.sample(&mut rng);
            assert!(p.x >= area.min.x && p.x < area.max.x);
            assert!(p.y >= area.min.y && p.y < area.max.y);
        }
        let empty = Area::from_cells(5, 5, 5, 5);
        let before = rng.clone();
        assert_eq!(empty.sample(&mut rng), Vec2::from_ints(5, 5));
        assert_eq!(rng, before);
    }

    #[test]
    fn behavior_tags_are_distinct_and_stable() {
        assert_eq!(Behavior::Idle.tag(), 0);
        assert_eq!(Behavior::Wander.tag(), 1);
        assert_eq!(Behavior::MoveTo.tag(), 2);
        assert_eq!(Behavior::Ordered.tag(), 3);
    }
}
