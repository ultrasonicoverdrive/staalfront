//! The read-only view of the world that the presentation draws from.
//!
//! The renderer runs at the screen refresh rate and interpolates between the
//! last two ticks (PLAN.md section on simulation and presentation). Every
//! [`EntityView`] therefore carries the position and facing at the end of the
//! previous tick *and* of the current one; with a fraction `alpha` of the way
//! to the next tick the renderer draws `prev + (pos - prev) * alpha`. The
//! sim does the fixed-point part (it stores both states); turning them into
//! floats for the GPU is the presentation's job, using the float conversion
//! helpers of `sf_math` that sit behind its `render` feature (which this crate
//! never enables).
//!
//! A [`Snapshot`] borrows the world immutably, so the world cannot be stepped
//! while one is alive. That is the point: the presentation cannot mutate the
//! simulation through it, and cannot observe a half-finished tick.

use sf_math::{Facing, Fx, Vec2};

use crate::ids::{EntityId, Kind, PlayerId, Tick};
use crate::projectiles::ProjectileId;
use crate::weapons::ProjectileKind;
use crate::world::World;

/// The turret of a unit as the presentation sees it (WP4.5): an absolute facing
/// in the world, not relative to the hull. The renderer subtracts the hull's
/// facing to get the angle of the turret's yaw joint.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct TurretView {
    /// Facing at the end of the previous tick.
    pub prev_facing: Facing,
    /// Facing at the end of the current tick.
    pub facing: Facing,
}

/// One live entity as the presentation sees it.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct EntityView {
    /// The entity's handle; commands name it.
    pub id: EntityId,
    /// What it is (picks a mesh or an icon).
    pub kind: Kind,
    /// Who owns it (picks the colour).
    pub owner: PlayerId,
    /// Position at the end of the previous tick, in cells. Equal to `pos` for
    /// an entity that has not moved, and on the tick it spawned.
    pub prev_pos: Vec2,
    /// Position at the end of the current tick, in cells.
    pub pos: Vec2,
    /// Facing at the end of the previous tick.
    pub prev_facing: Facing,
    /// Facing at the end of the current tick.
    pub facing: Facing,
    /// Whether the entity was made from the world's unit table
    /// ([`Command::SpawnUnit`](crate::Command::SpawnUnit),
    /// [`Command::PlaceStructure`](crate::Command::PlaceStructure)), so that
    /// `kind` is a role and `hp` is real. False for the placeholder wanderers
    /// and structures of the legacy `Spawn` and `Place`, whose kind is an
    /// opaque number.
    pub has_stats: bool,
    /// Current hit points; zero when `has_stats` is false.
    pub hp: u32,
    /// The turret, for a unit that has one (WP4.5); `None` for everything else.
    pub turret: Option<TurretView>,
}

/// One projectile in flight as the presentation sees it (WP5.2).
///
/// Like an [`EntityView`] it carries the state at the end of the previous tick and of the current one
/// for interpolation. `height` is how far above the ground it is, in cells: zero for a straight or guided
/// projectile, the parabola of an arc, the drop of a falling shell. The ground position `pos` is where
/// its shadow would be.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct ProjectileView {
    /// The projectile's handle; stable for as long as it flies.
    pub id: ProjectileId,
    /// The kind number of its weapon (picks a model or a tracer).
    pub weapon: u16,
    /// How it flies.
    pub kind: ProjectileKind,
    /// The shooter; may be gone by now.
    pub owner: EntityId,
    /// The shooter's player (picks the colour).
    pub player: PlayerId,
    /// What it is chasing or aimed at, `None` for a point on the ground (a guided projectile whose
    /// target is gone).
    pub target: Option<EntityId>,
    /// The point it is flying to.
    pub aim: Vec2,
    /// Ground position at the end of the previous tick, in cells. Equal to `pos` on the tick of the
    /// launch.
    pub prev_pos: Vec2,
    /// Ground position at the end of the current tick, in cells.
    pub pos: Vec2,
    /// Height above the ground at the end of the previous tick, in cells.
    pub prev_height: Fx,
    /// Height above the ground at the end of the current tick, in cells.
    pub height: Fx,
    /// The direction it is flying.
    pub heading: Facing,
}

/// A read-only view of every live entity, in ascending slot order.
#[derive(Copy, Clone)]
pub struct Snapshot<'a> {
    world: &'a World,
}

impl<'a> Snapshot<'a> {
    pub(crate) fn new(world: &'a World) -> Snapshot<'a> {
        Snapshot { world }
    }

    /// Ticks executed so far. `pos` in the views is the state after tick
    /// `tick() - 1`, and `prev_pos` the state one tick before that.
    pub fn tick(&self) -> Tick {
        self.world.tick()
    }

    /// The map size in cells, `(width, height)`.
    pub fn map_size(&self) -> (u32, u32) {
        self.world.map_size()
    }

    /// The number of live entities.
    pub fn len(&self) -> usize {
        self.world.len()
    }

    /// True if there are no live entities.
    pub fn is_empty(&self) -> bool {
        self.world.is_empty()
    }

    /// One entity by id; `None` if it is gone or the id is stale.
    pub fn get(&self, id: EntityId) -> Option<EntityView> {
        self.world.get(id)
    }

    /// Every live entity, in ascending slot order. The order is stable from
    /// tick to tick (entities never change slots), so a renderer can keep a
    /// per-entity cache keyed by [`EntityId`] without surprises.
    pub fn iter(&self) -> impl Iterator<Item = EntityView> + 'a {
        let world = self.world;
        (0..world.entities.len())
            .filter(move |&i| world.entities.alive[i])
            .map(move |i| world.view(i))
    }

    /// Replaces the contents of `out` with every live entity (the same order
    /// as [`Snapshot::iter`]), reusing its allocation.
    pub fn collect_into(&self, out: &mut Vec<EntityView>) {
        out.clear();
        out.extend(self.iter());
    }

    /// The number of projectiles in flight (WP5.2); zero in a world without weapons.
    pub fn projectile_count(&self) -> usize {
        self.world.projectile_count()
    }

    /// Every projectile in flight, in ascending slot order (WP5.2). A projectile that goes off leaves
    /// the list in the tick it detonates; its slot may be reused by a later launch with a new
    /// generation, so key a cache by [`ProjectileId`], not by the slot.
    pub fn projectiles(&self) -> impl Iterator<Item = ProjectileView> + 'a {
        let world = self.world;
        world
            .arsenal
            .projectiles
            .iter()
            .map(move |(id, p)| world.projectile_view(id, p))
    }

    /// Replaces the contents of `out` with every projectile in flight (the same order as
    /// [`Snapshot::projectiles`]), reusing its allocation.
    pub fn collect_projectiles_into(&self, out: &mut Vec<ProjectileView>) {
        out.clear();
        out.extend(self.projectiles());
    }
}
