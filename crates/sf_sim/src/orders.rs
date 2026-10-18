//! Orders as state (WP3.4).
//!
//! A command is something a player *asks*; an order is what a unit *has*. A
//! unit stores its current [`Order`] and a short queue of the orders that come
//! after it, and carries them out on its own, tick after tick, until it is
//! done, loses its target or is given something else (receiving an order does
//! not mean the action is completed). Nothing here depends on the click that
//! created the order.
//!
//! # The orders
//!
//! | Order | The unit... | Ends when |
//! |---|---|---|
//! | [`Order::Idle`] | has no order (the command that puts a unit there is `Stop`) | never; any other order replaces it |
//! | [`Order::HoldPosition`] | stands still on purpose and never moves | never by itself; it blocks the queue behind it |
//! | [`Order::Move`] | goes to a point | it is within [`ARRIVE_TOLERANCE`] of the point |
//! | [`Order::Attack`] | goes toward an entity until it is within its first weapon's reach, then faces it | the target is gone (dead, despawned, stale id) |
//! | [`Order::AttackMove`] | goes to a point and would engage what it meets on the way | it is within [`ARRIVE_TOLERANCE`] of the point |
//! | [`Order::Guard`] | goes to a point and stays there, or follows an entity and stays near it | a point guard never ends by itself; an entity guard ends when the entity is gone |
//! | [`Order::Scatter`] | takes a short step in a deterministic direction | it is within [`ARRIVE_TOLERANCE`] of the spot |
//!
//! `Scatter` is a *command-time* order: when it becomes current it is replaced
//! by the [`Order::Move`] to the spot it picked (a pure function of the unit's
//! id and the tick, see [`scatter_offset`]), so a unit never has `Scatter` as
//! its current order, only in its queue.
//!
//! # The queue
//!
//! Each unit has a ring of [`ORDER_QUEUE_CAP`] orders in its own storage: no
//! allocation, no growth. A command with `queued: false` replaces the current
//! order and empties the queue; with `queued: true` it is appended (it becomes
//! the current order at once when the unit is idle). An append to a full queue
//! is refused for that unit, like any other order the unit cannot take. When
//! the current order ends, the next one in the queue starts on the following
//! tick; with an empty queue the unit goes idle.
//!
//! # Destination and route
//!
//! The order holds the *destination*: where the unit wants to be. The *route*
//! is how it gets there, and it is not part of the order. The unit keeps the
//! point it is steering at (the next waypoint) in its `target` column, and
//! [`next_waypoint`] is the only place that decides it. Today the route is the
//! straight line, so the waypoint is the destination itself; pathfinding
//! (WP4.2) replaces `next_waypoint` and nothing else: the order, the arrival
//! test and the queue stay as they are. Arrival is always judged against the
//! destination, never against the waypoint.
//!
//! # What phase 5 fills in
//!
//! There is no combat here. [`would_acquire_targets_here`] is called every
//! tick from the places where a unit would look for a target ([`Order::AttackMove`]
//! and [`Order::Guard`]); it does nothing. Phase 5 puts target selection behind
//! it: scan for an enemy in sight, and for an attack-move or a guard turn the
//! unit toward it (keeping the interrupted order, for instance by putting it at
//! the front of the queue). Idle units are skipped by the behaviour pass, so
//! the return-fire of an idle unit needs a pass of its own. The reach used for
//! [`Order::Attack`] is `UnitStats::weapon_range`, the first weapon's range
//! (WP5.1 adds the others and the minimum range).

use sf_math::{Facing, Fx, Vec2};

use crate::behavior::{Motion, walk};
use crate::entities::{Entities, NO_DEF};
use crate::ids::{EntityId, Tick};
use crate::units::{UnitClass, UnitTable};
use crate::world::mix64;

/// How many orders wait behind the current one. A queue append beyond it is
/// refused. A constant, not data: it sizes every unit's storage.
pub const ORDER_QUEUE_CAP: usize = 8;

/// How near to its destination a unit counts as arrived, in cells: an eighth
/// of a cell is half a metre. A unit that is within one step of the
/// destination snaps onto it, as the legacy `MoveTo` does.
pub const ARRIVE_TOLERANCE: Fx = Fx::from_ratio(1, 8);

/// A guard that follows an entity stops when it is this near the entity's edge,
/// in cells.
pub const GUARD_CLOSE: Fx = Fx::from_ratio(3, 2);

/// A guard that stands near an entity starts following again when it is this
/// far from the entity's edge, in cells. Bigger than [`GUARD_CLOSE`], so a
/// guard does not shuffle back and forth at the boundary.
pub const GUARD_LEASH: Fx = Fx::from_ratio(5, 2);

/// The shortest step of a scatter, in cells.
pub const SCATTER_MIN: Fx = Fx::ONE;

/// The longest step of a scatter, in cells.
pub const SCATTER_MAX: Fx = Fx::from_int(3);

/// What a guard watches over.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum GuardTarget {
    /// A spot on the map, in cells.
    Point(Vec2),
    /// An entity, followed wherever it goes.
    Entity(EntityId),
}

/// One order. See the [module documentation](self) for what each does.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Order {
    /// No order. Phase 5 lets an idle unit react (return fire, auto-acquire).
    Idle,
    /// Stand still on purpose: the unit never moves, not to chase and not to
    /// get closer. Phase 5 may fire from it at what is in range, but it never
    /// moves by itself. It never ends by itself and a queued order behind it
    /// waits until it is replaced or stopped.
    HoldPosition,
    /// Go to a point, in cells.
    Move(Vec2),
    /// Go for an entity until it is in weapon reach.
    Attack(EntityId),
    /// Go to a point, engaging what is met on the way (phase 5).
    AttackMove(Vec2),
    /// Watch over a point or an entity.
    Guard(GuardTarget),
    /// A short step in a deterministic direction.
    Scatter,
}

impl Order {
    /// Writes the order into a hash: a tag, then its fields. Stable.
    pub(crate) fn write_hash(self, h: &mut sf_math::hash::Fnv1a64) {
        match self {
            Order::Idle => h.write_u8(0),
            Order::Move(p) => {
                h.write_u8(1);
                h.write_vec2(p);
            }
            Order::Attack(id) => {
                h.write_u8(2);
                h.write_u32(id.index());
                h.write_u32(id.generation());
            }
            Order::AttackMove(p) => {
                h.write_u8(3);
                h.write_vec2(p);
            }
            Order::Guard(GuardTarget::Point(p)) => {
                h.write_u8(4);
                h.write_vec2(p);
            }
            Order::Guard(GuardTarget::Entity(id)) => {
                h.write_u8(5);
                h.write_u32(id.index());
                h.write_u32(id.generation());
            }
            Order::Scatter => h.write_u8(6),
            Order::HoldPosition => h.write_u8(7),
        }
    }
}

/// The orders waiting behind a unit's current one: a fixed-size ring.
///
/// Two queues are equal when they hold the same orders in the same order,
/// wherever the ring happens to start.
#[derive(Copy, Clone, Debug)]
pub(crate) struct OrderQueue {
    items: [Order; ORDER_QUEUE_CAP],
    head: u8,
    len: u8,
}

impl OrderQueue {
    pub(crate) const EMPTY: OrderQueue = OrderQueue {
        items: [Order::Idle; ORDER_QUEUE_CAP],
        head: 0,
        len: 0,
    };

    pub(crate) fn len(&self) -> usize {
        usize::from(self.len)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Appends an order; false (and nothing changes) if the ring is full.
    pub(crate) fn push_back(&mut self, order: Order) -> bool {
        if self.len() >= ORDER_QUEUE_CAP {
            return false;
        }
        let at = (usize::from(self.head) + self.len()) % ORDER_QUEUE_CAP;
        self.items[at] = order;
        self.len += 1;
        true
    }

    /// Takes the order at the front.
    pub(crate) fn pop_front(&mut self) -> Option<Order> {
        if self.is_empty() {
            return None;
        }
        let order = self.items[usize::from(self.head)];
        // A neutral value in the vacated place, so no stale order lingers.
        self.items[usize::from(self.head)] = Order::Idle;
        self.head = ((usize::from(self.head) + 1) % ORDER_QUEUE_CAP) as u8;
        self.len -= 1;
        Some(order)
    }

    /// The orders, front first.
    pub(crate) fn iter(&self) -> impl Iterator<Item = Order> + '_ {
        (0..self.len()).map(|k| self.items[(usize::from(self.head) + k) % ORDER_QUEUE_CAP])
    }
}

impl PartialEq for OrderQueue {
    fn eq(&self, other: &OrderQueue) -> bool {
        self.iter().eq(other.iter())
    }
}

impl Eq for OrderQueue {}

/// The step a scatter takes, from the unit's id and the tick it starts on: a
/// direction (one of 256) and a length between [`SCATTER_MIN`] and
/// [`SCATTER_MAX`]. A pure function: the same unit on the same tick always
/// scatters the same way, on every machine, and it consumes no randomness
/// from the unit's own stream.
pub fn scatter_offset(id: EntityId, tick: Tick) -> Vec2 {
    let unit = u64::from(id.index()) | (u64::from(id.generation()) << 32);
    // The salt keeps the finalizer away from its fixed point at zero (unit 0
    // on tick 0 would otherwise always take the shortest step east).
    let bits =
        mix64(mix64(unit ^ 0xA5A5_5A5A_C3C3_3C3C) ^ tick.0.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let facing = Facing::new((bits >> 56) as u8);
    let span = (SCATTER_MAX.raw() - SCATTER_MIN.raw()) as u64 + 1;
    let length = Fx::from_raw(SCATTER_MIN.raw() + ((bits & 0xFFFF_FFFF) % span) as i64);
    Vec2::from_facing(facing, length)
}

/// The point the unit steers at to get toward `destination`: the next
/// waypoint of its route. A straight line today, so the destination itself.
/// This is the one function pathfinding (WP4.2) replaces; it will then also
/// take the terrain and the route it keeps (its signature changes, its caller
/// `go_to` does not).
pub(crate) fn next_waypoint(_e: &Entities, _i: usize, destination: Vec2) -> Vec2 {
    destination
}

/// The hook for target selection (phase 5). Called every tick for a unit that
/// would look for a target: an attack-move on its way and a guard at its post.
/// It does nothing yet.
pub(crate) fn would_acquire_targets_here(_e: &Entities, _i: usize) {}

/// True if slot `j` holds the entity `id` names.
fn live_slot(e: &Entities, id: EntityId) -> Option<usize> {
    let j = id.index() as usize;
    (e.alive.get(j) == Some(&true) && e.generation[j] == id.generation()).then_some(j)
}

/// Half the width of what entity `j` is, in cells: the distance from its
/// middle to its edge, for a reach measured to the edge. For a structure the
/// shorter half side, which never overstates how close a unit is.
pub(crate) fn radius_of(e: &Entities, j: usize, units: &UnitTable) -> Fx {
    if e.def[j] == NO_DEF {
        return if e.cell[j] == crate::terrain::NO_CELL {
            Fx::ZERO
        } else {
            Fx::HALF
        };
    }
    let stats = units.at(e.def[j] as usize);
    match (stats.class(), stats.footprint) {
        (UnitClass::Structure, Some(fp)) => Fx::from_ratio(i64::from(fp.width.min(fp.height)), 2),
        _ => stats.size.div_int(2),
    }
}

/// The first weapon's range of entity `i`, zero if it has no entry or no weapon.
pub(crate) fn weapon_range_of(e: &Entities, i: usize, units: &UnitTable) -> Fx {
    if e.def[i] == NO_DEF {
        Fx::ZERO
    } else {
        units.at(e.def[i] as usize).weapon_range
    }
}

/// Starts `order` as the current order of unit `i` on tick `tick`. A scatter
/// turns into the move to the spot it picks; the spot is kept inside
/// `bounds` (the smallest and largest valid position).
pub(crate) fn begin(e: &mut Entities, i: usize, order: Order, tick: Tick, bounds: (Vec2, Vec2)) {
    let order = match order {
        Order::Scatter => {
            let id = EntityId::new(i as u32, e.generation[i]);
            Order::Move((e.pos[i] + scatter_offset(id, tick)).clamp(bounds.0, bounds.1))
        }
        other => other,
    };
    e.order[i] = order;
    e.target[i] = None;
    e.guard_walking[i] = false;
    e.behavior[i] = if order == Order::Idle {
        crate::behavior::Behavior::Idle
    } else {
        crate::behavior::Behavior::Ordered
    };
}

/// Gives unit `i` an order: replaces the current one and empties the queue,
/// or, when `queued`, appends it (starting it at once if the unit is idle).
/// False (nothing changes) if the queue is full.
pub(crate) fn give(
    e: &mut Entities,
    i: usize,
    order: Order,
    queued: bool,
    tick: Tick,
    bounds: (Vec2, Vec2),
) -> bool {
    if queued && e.order[i] != Order::Idle {
        return e.queue[i].push_back(order);
    }
    e.queue[i].clear_all();
    begin(e, i, order, tick, bounds);
    true
}

/// Clears the current order and the queue: the unit stands still. The unit
/// keeps its facing and position.
pub(crate) fn clear(e: &mut Entities, i: usize) {
    e.order[i] = Order::Idle;
    e.queue[i] = OrderQueue::EMPTY;
    e.target[i] = None;
    e.guard_walking[i] = false;
    e.behavior[i] = crate::behavior::Behavior::Idle;
}

/// The current order is done: the next queued order starts, or the unit goes
/// idle.
fn finish(e: &mut Entities, i: usize, tick: Tick, bounds: (Vec2, Vec2)) {
    match e.queue[i].pop_front() {
        Some(next) => begin(e, i, next, tick, bounds),
        None => clear(e, i),
    }
}

/// One tick of walking toward `destination` until within `stop_at` of it.
/// True when the unit has arrived. Steers at the route's next waypoint and
/// keeps it in the `target` column; arrival is judged against the destination.
fn go_to(e: &mut Entities, i: usize, destination: Vec2, stop_at: Fx, m: &Motion) -> bool {
    if (destination - e.pos[i]).length_squared() <= stop_at.square() {
        e.target[i] = None;
        return true;
    }
    let waypoint = next_waypoint(e, i, destination);
    e.target[i] = Some(waypoint);
    let reached = walk(e, i, waypoint, m);
    if reached && waypoint == destination {
        e.target[i] = None;
        return true;
    }
    false
}

/// Runs one tick of the current order of unit `i`, which has
/// `Behavior::Ordered`. `m` is the unit's own motion.
pub(crate) fn run(e: &mut Entities, i: usize, m: &Motion, units: &UnitTable, tick: Tick) {
    let bounds = (m.pos_min, m.pos_max);
    match e.order[i] {
        // Not reachable through the invariant (a unit with no order is idle),
        // but a stray one settles instead of looping.
        Order::Idle | Order::Scatter => clear(e, i),
        // Never moves. Phase 5 may fire from here, never chase.
        Order::HoldPosition => would_acquire_targets_here(e, i),
        Order::Move(destination) => {
            if go_to(e, i, destination, ARRIVE_TOLERANCE, m) {
                finish(e, i, tick, bounds);
            }
        }
        Order::AttackMove(destination) => {
            would_acquire_targets_here(e, i);
            if go_to(e, i, destination, ARRIVE_TOLERANCE, m) {
                finish(e, i, tick, bounds);
            }
        }
        Order::Guard(GuardTarget::Point(post)) => {
            would_acquire_targets_here(e, i);
            // Never ends by itself; a unit that has been moved off its post
            // walks back.
            go_to(e, i, post, ARRIVE_TOLERANCE, m);
        }
        Order::Guard(GuardTarget::Entity(watched)) => {
            let Some(j) = live_slot(e, watched) else {
                finish(e, i, tick, bounds);
                return;
            };
            would_acquire_targets_here(e, i);
            let radius = radius_of(e, j, units);
            let (close, leash) = (GUARD_CLOSE + radius, GUARD_LEASH + radius);
            // Hysteresis: a walking guard walks on until it is close; a
            // standing one waits until it is far. The switch is its own
            // field, not the waypoint, which the route will take over.
            let start = if e.guard_walking[i] { close } else { leash };
            if (e.pos[j] - e.pos[i]).length_squared() > start.square() {
                let at = e.pos[j];
                let arrived = go_to(e, i, at, close, m);
                e.guard_walking[i] = !arrived;
            } else {
                e.guard_walking[i] = false;
                e.target[i] = None;
            }
        }
        Order::Attack(victim) => {
            let Some(j) = live_slot(e, victim) else {
                finish(e, i, tick, bounds);
                return;
            };
            let reach = weapon_range_of(e, i, units) + radius_of(e, j, units);
            let at = e.pos[j];
            if go_to(e, i, at, reach, m)
                && let Some(wanted) = (at - e.pos[i]).facing()
            {
                // In reach: face the target. Firing comes in phase five.
                e.facing[i] = e.facing[i].turn_toward(wanted, m.turn_per_tick);
            }
        }
    }
}

impl OrderQueue {
    /// Empties the ring.
    pub(crate) fn clear_all(&mut self) {
        *self = OrderQueue::EMPTY;
    }
}

#[cfg(test)]
mod tests;
