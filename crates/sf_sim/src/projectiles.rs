//! Projectiles (WP5.2): the shots that take time to arrive.
//!
//! A weapon whose projectile kind is not [`ProjectileKind::Instant`] does not hurt anyone in the tick
//! it fires. It launches a projectile, a small record in a store of its own, which flies for a number
//! of ticks and then detonates. The rules are in `docs/rules/projectiles.md`; the short version:
//!
//! * **Straight** flies at constant speed toward the aim point (where the target stood when the shot
//!   was fired, no lead) and goes off the moment its path touches its target; a target that has walked
//!   out of the line is missed, and the shell goes off at the aim point.
//! * **Arc** covers the same ground, high above everything on it (its height is a parabola that peaks at
//!   `flight_height`), and so can only go off where it lands. The target is hit if it is still within
//!   reach of the landing point.
//! * **Guided** steers at most `guide_turn` facing steps a tick toward its target's *current* place.
//!   If the target is gone it flies on to the last place it knew.
//! * **Falling** starts `flight_height` above the aim point and drops `projectile_speed` a tick.
//!
//! Every projectile carries a fuse, the number of ticks it may still fly. If it runs out the projectile
//! goes off where it is, hitting nobody. Detonating is a *decision* of the tick like a firing decision:
//! see [`crate::weapons`] for the two passes.
//!
//! # The store
//!
//! [`Projectiles`] is a slot vector with a free list and a generation per slot, iterated in ascending
//! slot order, like the entities but separate from them: a projectile is not an entity (no hit points, no
//! orders, no cell). A [`ProjectileId`] is a slot and a generation. Nothing in a projectile's flight reads
//! another projectile, so the slot a projectile has only decides the order of reports, never an outcome.
//!
//! # What a projectile remembers
//!
//! Its owner (the id of the shooter, which may be stale by now: nothing here asks whether the shooter is
//! alive, so a shell in flight lands when its shooter dies), the owner's player (for the credit of WP5.5
//! and the colour of a tracer), its weapon, its target, where it is, how high, which way it flies, and the
//! fuse. Everything but `prev_pos` and `prev_height` (for render interpolation) is hashed in the
//! projectile section of the checksum.

use sf_math::hash::Fnv1a64;
use sf_math::{Facing, Fx, Vec2};

use crate::entities::Entities;
use crate::ids::{EntityId, PlayerId};
use crate::orders::{live_slot, radius_of};
use crate::units::UnitTable;
use crate::weapons::{ProjectileKind, WeaponStats};

/// How far from the edge of its target a projectile still counts as touching it: a quarter of a cell
/// (a metre). A projectile is not a point.
pub const HIT_SLACK: Fx = Fx::from_ratio(1, 4);

/// A guided projectile may fly this many times its weapon's range before its fuse runs out.
pub const GUIDED_FLIGHT_FACTOR: u32 = 2;

/// A fuse that outlasts the flight of a projectile that cannot miss its aim point by this many ticks.
/// It is a backstop: a straight, arcing or falling projectile arrives before it runs out.
const BALLISTIC_FUSE_SLACK: u32 = 1;

/// Written first into the projectile section of the checksum; bump when its layout changes.
const HASH_TAG: &[u8] = b"sf_sim/projectiles/v1";

/// A handle to a projectile: its slot and the slot's generation when it was launched. A stale id
/// (the projectile has detonated, perhaps replaced by another in the same slot) finds nothing.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ProjectileId {
    index: u32,
    generation: u32,
}

impl ProjectileId {
    /// An id from its two parts.
    #[inline]
    pub const fn new(index: u32, generation: u32) -> ProjectileId {
        ProjectileId { index, generation }
    }

    /// The slot number.
    #[inline]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// The slot's generation when this id was issued.
    #[inline]
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

/// What a projectile is flying at.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Target {
    /// An entity. A guided projectile follows it; the others only check, where they go off, whether it
    /// is there.
    Entity(EntityId),
    /// A point on the ground: the projectile's `aim`.
    Point,
}

/// One projectile in flight.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct Projectile {
    /// The kind number of its weapon.
    pub weapon: u16,
    /// The shooter. May be stale: nothing reads it to decide anything.
    pub owner: EntityId,
    /// The shooter's player, as it was when the shot was fired.
    pub player: PlayerId,
    /// What it flies at.
    pub target: Target,
    /// The point it flies to: fixed for a straight, arcing or falling projectile (where the target stood
    /// when it was fired), and the last known place of the target for a guided one.
    pub aim: Vec2,
    /// Where it is on the ground, in cells.
    pub pos: Vec2,
    /// Where it was one tick earlier. Derived (interpolation); not hashed.
    pub prev_pos: Vec2,
    /// How high it is above the ground, in cells. Presentation only: nothing in the rules reads it
    /// except a falling projectile, which lands when it reaches the ground.
    pub height: Fx,
    /// The height one tick earlier. Derived; not hashed.
    pub prev_height: Fx,
    /// The direction it is flying. Steered by a guided projectile; fixed at launch for the others.
    pub heading: Facing,
    /// The ground distance from the launch to the aim point (an arc's height follows from how much of it
    /// is covered).
    pub span: Fx,
    /// Ticks it may still fly. At zero it goes off where it is.
    pub fuse: u32,
}

/// Why a projectile went off.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ImpactCause {
    /// It touched its target on its way (a straight or guided projectile).
    Proximity,
    /// It reached the point it was flying to: where an arc or a falling shell lands, and where a straight
    /// shell goes off when its target is no longer in its path. The target is hit if it is within reach
    /// of that point.
    Arrived,
    /// Its fuse ran out. A miss.
    Fuse,
}

/// What a projectile decided at the end of its flight: where it went off, why, and the entity (as a
/// slot) it counts as a direct hit on, if any. Nothing has been applied yet.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct Burst {
    pub at: Vec2,
    pub cause: ImpactCause,
    pub victim: Option<usize>,
}

/// The ticks needed to cover `distance` at `speed` per tick, rounded up. `speed` must be positive (the
/// weapon table checks it); a zero or negative one counts as "never".
pub(crate) fn ticks_to_cover(distance: Fx, speed: Fx) -> u32 {
    let (Ok(d), Ok(v)) = (
        u64::try_from(distance.raw().max(0)),
        u64::try_from(speed.raw()),
    ) else {
        return u32::MAX;
    };
    if v == 0 {
        return u32::MAX;
    }
    u32::try_from(d.div_ceil(v)).unwrap_or(u32::MAX)
}

/// The ticks a projectile of `w` is given to fly: the time to cover the ground to the aim point (or the
/// drop) plus a tick for the ballistic kinds, which arrive by themselves; [`GUIDED_FLIGHT_FACTOR`] times
/// the time to fly the weapon's range for a guided one, which may chase.
pub(crate) fn fuse_for(w: &WeaponStats, span: Fx) -> u32 {
    let speed = w.projectile_speed_per_tick;
    match w.projectile {
        ProjectileKind::Instant => 0,
        ProjectileKind::Straight | ProjectileKind::Arc => {
            ticks_to_cover(span, speed).saturating_add(BALLISTIC_FUSE_SLACK)
        }
        ProjectileKind::Falling => {
            ticks_to_cover(w.flight_height, speed).saturating_add(BALLISTIC_FUSE_SLACK)
        }
        ProjectileKind::Guided => ticks_to_cover(w.range, speed)
            .saturating_mul(GUIDED_FLIGHT_FACTOR)
            .max(1),
    }
}

/// A new projectile for a shot of `w` from `origin`, whose weapon points toward `pointing`, at entity
/// `target` which stands at `aim`.
pub(crate) fn launch(
    w: &WeaponStats,
    owner: EntityId,
    player: PlayerId,
    origin: Vec2,
    pointing: Facing,
    target: EntityId,
    aim: Vec2,
) -> Projectile {
    let span = origin.distance(aim);
    let toward = (aim - origin).facing().unwrap_or(pointing);
    let (pos, height, heading) = match w.projectile {
        // A rocket leaves in the direction the launcher points and steers from there.
        ProjectileKind::Guided => (origin, Fx::ZERO, pointing),
        // A shell is aimed at its point.
        ProjectileKind::Straight | ProjectileKind::Arc | ProjectileKind::Instant => {
            (origin, Fx::ZERO, toward)
        }
        // A falling shell starts right above its aim point.
        ProjectileKind::Falling => (aim, w.flight_height, toward),
    };
    Projectile {
        weapon: w.kind,
        owner,
        player,
        target: Target::Entity(target),
        aim,
        pos,
        prev_pos: pos,
        height,
        prev_height: height,
        heading,
        span: if w.projectile == ProjectileKind::Falling {
            Fx::ZERO
        } else {
            span
        },
        fuse: fuse_for(w, span),
    }
}

/// The point on the segment `a` to `b` that is closest to `c`.
fn closest_on_segment(a: Vec2, b: Vec2, c: Vec2) -> Vec2 {
    let d = b - a;
    let len2 = d.length_squared();
    if len2.is_zero() {
        return a;
    }
    let t = ((c - a).dot(d) / len2).clamp(Fx::ZERO, Fx::ONE);
    a + d.scale(t)
}

/// Where the segment `a` to `b` first comes within `radius` of `c`, approximated by the point of the
/// segment closest to `c`; `None` if it never does. The test is on the whole step, so a fast projectile
/// cannot jump over a small target.
fn touches(a: Vec2, b: Vec2, c: Vec2, radius: Fx) -> Option<Vec2> {
    let nearest = closest_on_segment(a, b, c);
    (nearest.distance_squared(c) <= radius.square()).then_some(nearest)
}

/// The height of an arc of `span` that peaks at `peak` after covering `travelled` of it:
/// `4 * peak * t * (span - t) / span^2`, in integers on the raw values with one rounding step. It is zero
/// at both ends and `peak` in the middle.
pub(crate) fn arc_height(peak: Fx, span: Fx, travelled: Fx) -> Fx {
    let s = i128::from(span.raw());
    if s <= 0 {
        return Fx::ZERO;
    }
    let t = i128::from(travelled.raw()).clamp(0, s);
    let top = 4 * i128::from(peak.raw()) * t * (s - t);
    let bottom = s * s;
    let raw = (top + bottom / 2) / bottom;
    Fx::from_raw(i64::try_from(raw).unwrap_or(i64::MAX))
}

/// The entity (as a slot) that a projectile aimed at `target` counts as hitting if it goes off at
/// `at`: the target if it is alive, can be hurt, and its edge is within [`HIT_SLACK`] of `at`.
fn victim_at(e: &Entities, units: &UnitTable, target: Target, at: Vec2) -> Option<usize> {
    let Target::Entity(id) = target else {
        return None;
    };
    let j = hurtable(e, id)?;
    (e.pos[j].distance_squared(at) <= reach_of(e, units, j).square()).then_some(j)
}

/// The slot of `id` if it is alive and has hit points left (zero means it cannot be hurt, or has been
/// shot down: the same reading as for firing, `docs/rules/weapons.md`).
fn hurtable(e: &Entities, id: EntityId) -> Option<usize> {
    live_slot(e, id).filter(|&j| e.hp[j] > 0)
}

/// How near the middle of entity `j` a projectile has to come to hit it: its radius and the slack.
fn reach_of(e: &Entities, units: &UnitTable, j: usize) -> Fx {
    radius_of(e, j, units) + HIT_SLACK
}

/// Moves projectile `p` of weapon `w` one tick and says whether it went off. The decision reads the
/// entities as they are at the start of the tick's weapon stage and changes nothing but `p`.
pub(crate) fn fly(
    p: &mut Projectile,
    w: &WeaponStats,
    e: &Entities,
    units: &UnitTable,
) -> Option<Burst> {
    p.prev_pos = p.pos;
    p.prev_height = p.height;
    let speed = w.projectile_speed_per_tick;
    let burst = match w.projectile {
        ProjectileKind::Instant => None,
        ProjectileKind::Straight => fly_straight(p, speed, e, units),
        ProjectileKind::Arc => fly_arc(p, w, speed, e, units),
        ProjectileKind::Guided => fly_guided(p, w, speed, e, units),
        ProjectileKind::Falling => fly_falling(p, speed, e, units),
    };
    if burst.is_some() {
        return burst;
    }
    p.fuse = p.fuse.saturating_sub(1);
    (p.fuse == 0).then_some(Burst {
        at: p.pos,
        cause: ImpactCause::Fuse,
        victim: None,
    })
}

/// One step toward the aim point: the new place, and whether it is the aim point itself.
fn step_toward_aim(p: &Projectile, speed: Fx) -> (Vec2, bool) {
    let to = p.aim - p.pos;
    if to.length() <= speed {
        (p.aim, true)
    } else {
        (p.pos + to.with_length(speed), false)
    }
}

fn fly_straight(p: &mut Projectile, speed: Fx, e: &Entities, units: &UnitTable) -> Option<Burst> {
    let (next, arrives) = step_toward_aim(p, speed);
    if let Target::Entity(id) = p.target
        && let Some(j) = hurtable(e, id)
        && let Some(at) = touches(p.pos, next, e.pos[j], reach_of(e, units, j))
    {
        return Some(Burst {
            at,
            cause: ImpactCause::Proximity,
            victim: Some(j),
        });
    }
    if arrives {
        // Its path never touched the target: the shell goes off at the aim point, hitting nothing.
        return Some(Burst {
            at: p.aim,
            cause: ImpactCause::Arrived,
            victim: None,
        });
    }
    p.pos = next;
    None
}

fn fly_arc(
    p: &mut Projectile,
    w: &WeaponStats,
    speed: Fx,
    e: &Entities,
    units: &UnitTable,
) -> Option<Burst> {
    let (next, arrives) = step_toward_aim(p, speed);
    if arrives {
        return Some(Burst {
            at: p.aim,
            cause: ImpactCause::Arrived,
            victim: victim_at(e, units, p.target, p.aim),
        });
    }
    p.pos = next;
    let left = p.pos.distance(p.aim);
    p.height = arc_height(w.flight_height, p.span, p.span - left);
    None
}

fn fly_falling(p: &mut Projectile, speed: Fx, e: &Entities, units: &UnitTable) -> Option<Burst> {
    if p.height > speed {
        p.height -= speed;
        return None;
    }
    p.height = Fx::ZERO;
    Some(Burst {
        at: p.aim,
        cause: ImpactCause::Arrived,
        victim: victim_at(e, units, p.target, p.aim),
    })
}

fn fly_guided(
    p: &mut Projectile,
    w: &WeaponStats,
    speed: Fx,
    e: &Entities,
    units: &UnitTable,
) -> Option<Burst> {
    // The target as it is now: followed while it can be hurt, then given up for the last place it
    // was seen.
    let chased = match p.target {
        Target::Entity(id) => hurtable(e, id),
        Target::Point => None,
    };
    match chased {
        Some(j) => p.aim = e.pos[j],
        None => p.target = Target::Point,
    }
    if let Some(bearing) = (p.aim - p.pos).facing() {
        p.heading = p.heading.turn_toward(bearing, w.guide_turn_per_tick);
    }
    let next = p.pos + p.heading.to_vec2().scale(speed);
    let (centre, radius) = match chased {
        Some(j) => (e.pos[j], reach_of(e, units, j)),
        None => (p.aim, HIT_SLACK),
    };
    if let Some(at) = touches(p.pos, next, centre, radius) {
        return Some(Burst {
            at,
            cause: if chased.is_some() {
                ImpactCause::Proximity
            } else {
                ImpactCause::Arrived
            },
            victim: chased,
        });
    }
    p.pos = next;
    None
}

/// Every projectile in flight: a slot vector with a free list and a generation per slot. A slot whose
/// generation is used up is retired, not reused (see [`Projectiles::remove`]).
#[derive(Clone, PartialEq, Eq, Default, Debug)]
pub(crate) struct Projectiles {
    /// How often each slot has been freed.
    generation: Vec<u32>,
    /// The projectile in each slot, or `None` for a free one.
    rows: Vec<Option<Projectile>>,
    /// Free slots, used as a stack: the last freed is reused first.
    free: Vec<u32>,
    /// Live projectiles.
    live: u32,
}

impl Projectiles {
    /// The number of projectiles in flight.
    pub fn len(&self) -> usize {
        self.live as usize
    }

    /// The number of slots, live and free.
    pub fn slot_count(&self) -> usize {
        self.rows.len()
    }

    /// Puts `p` in the most recently freed slot, or a new one, and returns its id.
    pub fn insert(&mut self, p: Projectile) -> ProjectileId {
        self.live += 1;
        if let Some(index) = self.free.pop() {
            self.rows[index as usize] = Some(p);
            return ProjectileId::new(index, self.generation[index as usize]);
        }
        let index = self.rows.len() as u32;
        self.rows.push(Some(p));
        self.generation.push(0);
        ProjectileId::new(index, 0)
    }

    /// Frees slot `index` and bumps its generation. Does nothing for a slot that is already free.
    ///
    /// The generation never wraps and never sticks: a slot whose generation cannot be bumped any further
    /// is **retired** (not put on the free list, so it is never used again) and its last id stays stale for
    /// good, the same rule as for the entities (`World::despawn`). Otherwise a stale id could be mistaken
    /// for a new projectile that reuses the slot.
    pub fn remove(&mut self, index: u32) {
        let i = index as usize;
        if self.rows.get(i).is_some_and(Option::is_some) {
            self.rows[i] = None;
            if let Some(next) = self.generation[i].checked_add(1) {
                self.generation[i] = next;
                self.free.push(index);
            }
            self.live -= 1;
        }
    }

    /// The projectile in slot `index`, if the slot is live.
    pub fn row_mut(&mut self, index: usize) -> Option<&mut Projectile> {
        self.rows.get_mut(index).and_then(Option::as_mut)
    }

    /// The projectile with this id, if it is still in flight.
    pub fn get(&self, id: ProjectileId) -> Option<&Projectile> {
        let i = id.index() as usize;
        (self.generation.get(i) == Some(&id.generation()))
            .then(|| self.rows[i].as_ref())
            .flatten()
    }

    /// Every live projectile with its id, in ascending slot order.
    pub fn iter(&self) -> impl Iterator<Item = (ProjectileId, &Projectile)> {
        self.rows.iter().enumerate().filter_map(|(i, row)| {
            row.as_ref()
                .map(|p| (ProjectileId::new(i as u32, self.generation[i]), p))
        })
    }

    /// Folds the whole store into the checksum: the slots, the free list (it decides which slot a later
    /// launch gets), and every live projectile in slot order.
    pub fn write_hash(&self, h: &mut Fnv1a64) {
        h.write_bytes(HASH_TAG);
        h.write_u64(u64::try_from(self.rows.len()).unwrap_or(u64::MAX));
        h.write_u64(u64::try_from(self.free.len()).unwrap_or(u64::MAX));
        for &i in &self.free {
            h.write_u32(i);
        }
        h.write_u32(self.live);
        for (id, p) in self.iter() {
            h.write_u32(id.index());
            h.write_u32(id.generation());
            h.write_bytes(&p.weapon.to_le_bytes());
            h.write_u32(p.owner.index());
            h.write_u32(p.owner.generation());
            h.write_u8(p.player.0);
            match p.target {
                Target::Entity(t) => {
                    h.write_u8(1);
                    h.write_u32(t.index());
                    h.write_u32(t.generation());
                }
                Target::Point => h.write_u8(0),
            }
            h.write_vec2(p.aim);
            h.write_vec2(p.pos);
            h.write_fx(p.height);
            h.write_facing(p.heading);
            h.write_fx(p.span);
            h.write_u32(p.fuse);
        }
    }
}

#[cfg(test)]
mod tests;
