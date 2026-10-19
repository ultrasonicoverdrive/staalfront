//! The struct-of-arrays entity store: one `Vec` per column, one slot per
//! index.
//!
//! All columns always have the same length (the slot count). A slot is either
//! live or free; free slots keep their place so that indices never move. The
//! store does not decide which slot a new entity gets or when a generation
//! changes; [`World`](crate::World) owns that policy (free list, generation
//! bump) and uses [`Entities::fill`] and [`Entities::clear`] to write rows.

use sf_math::{Facing, Rng, Vec2};

use crate::behavior::{Area, Behavior};
use crate::crowd::CrowdState;
use crate::formation::Member;
use crate::ids::{EntityId, Kind, PlayerId};
use crate::orders::{Order, OrderQueue};
use crate::path::Routes;
use crate::terrain::NO_CELL;
use crate::weapons::{REST, WeaponState};

/// The `def` column value of an entity that has no entry in the unit table
/// (every legacy `Spawn` wanderer and `Place` structure).
pub(crate) const NO_DEF: u32 = u32::MAX;

/// One row of values for a freshly spawned entity.
#[derive(Clone, Debug)]
pub(crate) struct NewEntity {
    pub kind: Kind,
    pub owner: PlayerId,
    pub pos: Vec2,
    pub facing: Facing,
    pub behavior: Behavior,
    pub area: Area,
    pub rng: Rng,
    /// Position in the unit table, or [`NO_DEF`].
    pub def: u32,
    /// Hit points at birth (zero without a table entry).
    pub hp: u32,
}

/// The columns. Index `i` of every vector describes slot `i`.
///
/// Fields of a free slot hold neutral values and are ignored by everything
/// (the checksum, the snapshot, the behaviours) except `generation`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct Entities {
    /// How often the slot has been freed. Part of every [`EntityId`](crate::EntityId).
    pub generation: Vec<u32>,
    /// Whether the slot holds an entity.
    pub alive: Vec<bool>,
    pub kind: Vec<Kind>,
    pub owner: Vec<PlayerId>,
    /// Position in cells at the end of the last executed tick.
    pub pos: Vec<Vec2>,
    /// Position one tick earlier, for render interpolation. Derived data; not
    /// part of the checksum.
    pub prev_pos: Vec<Vec2>,
    pub facing: Vec<Facing>,
    /// Facing one tick earlier, for render interpolation. Derived data; not
    /// part of the checksum.
    pub prev_facing: Vec<Facing>,
    pub behavior: Vec<Behavior>,
    /// Where the entity is walking to, if anywhere.
    pub target: Vec<Option<Vec2>>,
    /// The area wander draws its targets from.
    pub area: Vec<Area>,
    /// The entity's own random stream.
    pub rng: Vec<Rng>,
    /// The row-major index of the cell the entity occupies, or [`NO_CELL`] if
    /// it occupies none (wanderers). Only `Place` sets it; the world's
    /// occupancy grid is the same information indexed by cell, and the
    /// checksum covers it from there.
    pub cell: Vec<u32>,
    /// The position of the entity's kind in the world's unit table, or
    /// [`NO_DEF`] for an entity that was not made from a table entry. Derived
    /// from the kind and the table, so it is not hashed on its own.
    pub def: Vec<u32>,
    /// Current hit points; zero for an entity without a table entry and for
    /// one that cannot be damaged. Hashed (it will change in phase 5).
    pub hp: Vec<u32>,
    /// The current order (WP3.4); [`Order::Idle`] for everything that has none
    /// (every legacy entity, every structure). Hashed in the orders section
    /// while it is not idle.
    pub order: Vec<Order>,
    /// The orders behind the current one: a fixed-size ring per slot.
    pub queue: Vec<OrderQueue>,
    /// Whether a unit that guards an entity is walking toward it (as opposed
    /// to standing near it). The switch of the guard's hysteresis; kept apart
    /// from `target`, which is the route's waypoint. Hashed with the orders.
    pub guard_walking: Vec<bool>,
    /// The route of a unit that walks an order (WP4.2): the how of the order,
    /// reset whenever the order changes. Idle for everything else, and shorter
    /// than the other columns (a slot beyond its end has none). Hashed in the
    /// paths section while the world has pathfinding and some unit has a route.
    pub route: Routes,
    /// What the unit remembers about being in a crowd (WP4.4): how long it has
    /// been unable to get on, its sidesteps, how far it has been pushed. At rest
    /// for every unit that never met one; hashed while it is not.
    pub crowd: Vec<CrowdState>,
    /// The fraction of a facing step a unit that turns in an arc has not yet
    /// turned, in thousandths (WP4.5); zero for everything else. Hashed in the
    /// turning section while it is not zero.
    pub turn_carry: Vec<u16>,
    /// The facing of the turret in the world (not relative to the hull), for a
    /// unit with a turret (WP4.5); the hull's facing for everything else, which
    /// nothing reads. Hashed in the turning section for a unit with a turret.
    pub turret: Vec<Facing>,
    /// The turret's facing one tick earlier, for render interpolation. Derived
    /// data; not part of the checksum.
    pub prev_turret: Vec<Facing>,
    /// What the turret has been told to aim at, besides an attack order
    /// (`Command::AimTurret`; phase 5 target selection will set it too). Hashed
    /// with the turret.
    pub turret_target: Vec<Option<EntityId>>,
    /// The formation the unit marches in, and where it stands in it (WP4.6); none for
    /// everything that is not part of one. Hashed in the formation section.
    pub form: Vec<Member>,
    /// The reload and the running burst of each weapon slot (WP5.1); at rest for
    /// everything that has not fired. Hashed in the weapons section while some slot is not.
    pub weapon: Vec<WeaponState>,
}

impl Entities {
    /// The number of slots (live and free).
    #[inline]
    pub fn len(&self) -> usize {
        self.generation.len()
    }

    /// Appends one free slot with generation zero and returns its index.
    pub fn push_free_slot(&mut self) -> usize {
        let index = self.generation.len();
        self.generation.push(0);
        self.alive.push(false);
        self.kind.push(Kind::default());
        self.owner.push(PlayerId::default());
        self.pos.push(Vec2::ZERO);
        self.prev_pos.push(Vec2::ZERO);
        self.facing.push(Facing::default());
        self.prev_facing.push(Facing::default());
        self.behavior.push(Behavior::Idle);
        self.target.push(None);
        self.area.push(Area::new(Vec2::ZERO, Vec2::ZERO));
        self.rng.push(Rng::new(0));
        self.cell.push(NO_CELL);
        self.def.push(NO_DEF);
        self.hp.push(0);
        self.order.push(Order::Idle);
        self.queue.push(OrderQueue::EMPTY);
        self.guard_walking.push(false);
        self.crowd.push(CrowdState::default());
        self.turn_carry.push(0);
        self.turret.push(Facing::default());
        self.prev_turret.push(Facing::default());
        self.turret_target.push(None);
        self.form.push(Member::NONE);
        self.weapon.push(REST);
        index
    }

    /// Writes a new entity into free slot `i` and marks it live. The previous
    /// position and facing start equal to the current ones, so a new entity
    /// does not streak across the screen on its first frame.
    pub fn fill(&mut self, i: usize, row: NewEntity) {
        self.alive[i] = true;
        self.kind[i] = row.kind;
        self.owner[i] = row.owner;
        self.pos[i] = row.pos;
        self.prev_pos[i] = row.pos;
        self.facing[i] = row.facing;
        self.prev_facing[i] = row.facing;
        self.behavior[i] = row.behavior;
        self.target[i] = None;
        self.area[i] = row.area;
        self.rng[i] = row.rng;
        self.cell[i] = NO_CELL;
        self.def[i] = row.def;
        self.hp[i] = row.hp;
        self.order[i] = Order::Idle;
        self.queue[i] = OrderQueue::EMPTY;
        self.guard_walking[i] = false;
        self.crowd[i] = CrowdState {
            settle: true,
            fresh: true,
            ..CrowdState::default()
        };
        self.turn_carry[i] = 0;
        self.turret[i] = row.facing;
        self.prev_turret[i] = row.facing;
        self.turret_target[i] = None;
        self.form[i] = Member::NONE;
        self.weapon[i] = REST;
        self.route.reset(i);
    }

    /// Frees slot `i`: marks it dead and resets every column to its neutral
    /// value, so nothing of the old entity survives in the slot. The
    /// generation is left to the caller.
    pub fn clear(&mut self, i: usize) {
        self.alive[i] = false;
        self.kind[i] = Kind::default();
        self.owner[i] = PlayerId::default();
        self.pos[i] = Vec2::ZERO;
        self.prev_pos[i] = Vec2::ZERO;
        self.facing[i] = Facing::default();
        self.prev_facing[i] = Facing::default();
        self.behavior[i] = Behavior::Idle;
        self.target[i] = None;
        self.area[i] = Area::new(Vec2::ZERO, Vec2::ZERO);
        self.rng[i] = Rng::new(0);
        self.cell[i] = NO_CELL;
        self.def[i] = NO_DEF;
        self.hp[i] = 0;
        self.order[i] = Order::Idle;
        self.queue[i] = OrderQueue::EMPTY;
        self.guard_walking[i] = false;
        self.crowd[i] = CrowdState::default();
        self.turn_carry[i] = 0;
        self.turret[i] = Facing::default();
        self.prev_turret[i] = Facing::default();
        self.turret_target[i] = None;
        self.form[i] = Member::NONE;
        self.weapon[i] = REST;
        self.route.reset(i);
    }
}
