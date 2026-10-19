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
//! is how it gets there, and it is not part of the order: the unit has a
//! [`Route`](crate::path) of its own (WP4.2), reset whenever its order changes or
//! ends. The unit keeps the point it is steering at (the next waypoint) in its
//! `target` column, and `Pathing::steer` is the only place that decides it. A
//! unit that terrain binds asks for a route on the first tick of a walk, stands
//! still until it has one, and then follows the waypoints of its ring; aircraft
//! and the legacy walks steer straight at the destination. Arrival is always
//! judged against the destination, never against the waypoint, and a destination
//! that cannot be reached leaves the order as it is: the route ends in the
//! nearest cell that can be reached and says so (`World::route_of`).
//!
//! # Terrain (WP4.1)
//!
//! The walk toward the waypoint is bound by the unit's movement class
//! ([`crate::movement`]): it goes at the speed of the cell it stands in and
//! does not enter a cell the class cannot cross. A routed unit goes round such
//! cells; a step the terrain still refuses (a smoothed route that grazed a
//! corner) makes it ask for a route cell by cell, and a refusal on that leaves
//! it **standing with the order kept**: nothing ends the order and the queue
//! behind it waits, until a new order replaces it. Noticing that a route is
//! blocked later is the replan's (WP4.7, below).
//!
//! # Crowds (WP4.4)
//!
//! The walk of an order is the unit's own step; the bodies of the units are dealt
//! with afterwards by the crowd pass ([`crate::crowd`]). Two things happen here,
//! in [`go_to`], because they change what the order does: a unit that is blocked
//! by units that have arrived, near where it is going, counts as **arrived**
//! (the crowd is standing on its spot), and a unit that is **stuck** replaces its
//! walk for a few ticks by a step to a free neighbouring spot. A new order, or the
//! end of one, forgets all of it (`CrowdState::reset_progress`).
//!
//! # Replanning (WP4.7)
//!
//! The destination stays in the order; the route is made again when it stops being the
//! way (`docs/rules/pathfinding.md`, "Replanning"). A change of the map near the route and
//! a target that moved are noticed by [`Pathing::steer`] (the second behind a cooldown); a
//! unit that is stuck asks for a new route round the bodies that stand still, from the crowd
//! pass; and a unit that has asked `ReplanConfig::max` times in this order and is stuck
//! still **gives the order up** (`CrowdState::give_up`, carried out at the top of [`run`]):
//! it ends as if it were done, the next queued order starts or the unit is idle where it
//! stands. A target that is gone ends an `Attack` or a `Guard` of an entity by the
//! generation of its id ([`live_slot`]). A unit in reach of its target keeps the route it
//! has, so that a target that steps out of reach again and again is not a request for a
//! route each time.
//!
//! # Turning (WP4.5)
//!
//! The walk toward the waypoint turns the hull the way the unit's movement class does
//! ([`crate::turning`]): infantry at once, tanks on the spot, wheeled vehicles, hovercraft and
//! vessels only while they move. A waypoint that is not the last counts as reached a quarter of a
//! cell short for the classes that have to move to turn, which cannot land within a step of a
//! point off their line. An [`Order::Attack`] in reach turns the hull of a unit without a turret
//! toward the victim, and leaves the hull of a unit with one to the turret
//! ([`crate::turret`]), which aims at the victim from the tick of the order.
//!
//! # Formations (WP4.6)
//!
//! A unit of a formation ([`crate::formation`]) has a `Move` order like any other, but [`go_to`]
//! asks the formation first what it does this tick: walk straight to its slot, stand, or go on with
//! the order's own walk (the leader at the pace of the formation; a unit that cannot see its slot).
//! Any new order or the end of one takes the unit out of its formation (`begin`, `clear`).
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

use crate::behavior::{Motion, Walk, walk};
use crate::crowd::{
    CROWD_ARRIVE, CROWD_ARRIVE_ALONE, CROWD_ARRIVE_DISC, CROWD_ARRIVE_STEP, Claim, WAYPOINT_CROWD,
};
use crate::entities::{Entities, NO_DEF};
use crate::formation::{self, Member};
use crate::ids::{EntityId, Tick};
use crate::movement::Ground;
use crate::path::{Aim, Pathing, Steer};
use crate::turning::{self, Turning};
use crate::turret;
use crate::units::{UnitClass, UnitTable};
use crate::world::mix64;

/// How many orders wait behind the current one. A queue append beyond it is
/// refused. A constant, not data: it sizes every unit's storage.
pub const ORDER_QUEUE_CAP: usize = 8;

/// How near to its destination a unit counts as arrived, in cells: an eighth
/// of a cell is half a metre. A unit that is within one step of the
/// destination snaps onto it, as the legacy `MoveTo` does.
pub const ARRIVE_TOLERANCE: Fx = Fx::from_ratio(1, 8);

/// Ticks a unit must have been held up by units that have arrived, all told, to count
/// as one more sidestep when it is judged to have arrived in the crowd (the sidesteps
/// themselves come every `stuck_ticks`, 60 as shipped).
const PRESSED_PER_STEP: u32 = 60;

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

/// The hook for target selection (phase 5). Called every tick for a unit that
/// would look for a target: an attack-move on its way and a guard at its post.
/// It does nothing yet.
pub(crate) fn would_acquire_targets_here(_e: &Entities, _i: usize) {}

/// Where the current order of unit `i` wants the unit to be: the point of a move, an
/// attack-move or a guard of a point, the live position of the entity of an attack or
/// a guard of an entity; `None` for an order that does not walk, or whose entity is gone.
pub(crate) fn destination_of(e: &Entities, i: usize) -> Option<Vec2> {
    match e.order[i] {
        Order::Move(p) | Order::AttackMove(p) | Order::Guard(GuardTarget::Point(p)) => Some(p),
        Order::Attack(id) | Order::Guard(GuardTarget::Entity(id)) => {
            live_slot(e, id).map(|j| e.pos[j])
        }
        Order::Idle | Order::HoldPosition | Order::Scatter => None,
    }
}

/// True if slot `j` holds the entity `id` names.
pub(crate) fn live_slot(e: &Entities, id: EntityId) -> Option<usize> {
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
    e.crowd[i].reset_progress();
    // A new order lets go of the arrival slot the unit stood on (WP4.4b).
    e.crowd[i].claim = Claim::default();
    e.crowd[i].disc = Fx::ZERO;
    e.crowd[i].home = None;
    e.crowd[i].home_wait = 0;
    e.crowd[i].home_rest = false;
    // A new order takes the unit out of its formation (WP4.6).
    e.form[i] = Member::NONE;
    e.route.reset(i);
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
    forget_give_up(e, i);
    if queued && e.order[i] != Order::Idle {
        return e.queue[i].push_back(order);
    }
    e.queue[i].clear_all();
    begin(e, i, order, tick, bounds);
    true
}

/// A command gives the unit an order (or stops it): the record that its last order was
/// given up (WP4.7, `World::gave_up_at`) is over.
pub(crate) fn forget_give_up(e: &mut Entities, i: usize) {
    e.crowd[i].gave_up_at = None;
}

/// Clears the current order and the queue: the unit stands still. The unit
/// keeps its facing and position.
pub(crate) fn clear(e: &mut Entities, i: usize) {
    e.order[i] = Order::Idle;
    e.queue[i] = OrderQueue::EMPTY;
    e.target[i] = None;
    e.guard_walking[i] = false;
    e.crowd[i].reset_progress();
    e.form[i] = Member::NONE;
    e.route.reset(i);
    e.behavior[i] = crate::behavior::Behavior::Idle;
}

/// The current order is done: the next queued order starts, or the unit goes
/// idle.
fn finish(e: &mut Entities, i: usize, tick: Tick, bounds: (Vec2, Vec2)) {
    match e.queue[i].pop_front() {
        Some(next) => begin(e, i, next, tick, bounds),
        None => {
            clear(e, i);
            e.crowd[i].settle = true;
        }
    }
}

/// How far from the point a unit held up by the crowd on its destination counts as
/// arrived, in cells (WP4.4, WP4.4b): `ring` (how far its own slot lies from the point,
/// zero without one) plus [`CROWD_ARRIVE`] and [`CROWD_ARRIVE_STEP`] more for each of
/// the `steps` it has sidestepped or been held up for. The growth is bounded by the
/// disc the group's bodies fill (`disc`, [`CrowdState::disc`](crate::crowd::CrowdState),
/// zero when the unit was not ordered with enough others to have one:
/// [`CROWD_ARRIVE_ALONE`] then): nobody counts as arrived farther out than
/// [`CROWD_ARRIVE_DISC`] times its radius, however long it has pressed. The slot itself
/// and the base slack round it count whatever the disc.
fn crowd_arrival_radius(ring: Fx, steps: u32, disc: Fx) -> Fx {
    let slack = CROWD_ARRIVE + CROWD_ARRIVE_STEP.mul_int(i64::from(steps));
    let bound = if disc.is_zero() {
        CROWD_ARRIVE_ALONE
    } else {
        disc.saturating_mul(CROWD_ARRIVE_DISC)
    };
    (ring + slack).min(bound.max(ring + CROWD_ARRIVE))
}

/// One tick of walking toward `destination` until within `stop_at` of it.
/// True when the unit has arrived. Steers at the next waypoint of the route
/// ([`Pathing::steer`]) and keeps it in the `target` column; arrival is judged
/// against the destination. A unit whose route is not there yet waits where it
/// stands; one whose route ended short of the destination (it cannot be
/// reached) stands at the end of it; neither is an arrival and the order is
/// kept.
fn go_to(
    e: &mut Entities,
    i: usize,
    destination: Vec2,
    stop_at: Fx,
    m: &Motion,
    ground: &Ground<'_>,
    path: &mut Pathing,
) -> bool {
    // The cooldowns of the replan run down while the unit walks its order (WP4.7).
    let c = &mut e.crowd[i];
    c.cool = c.cool.saturating_sub(1);
    c.retarget = c.retarget.saturating_sub(1);
    // A unit in a formation walks to its slot, stands, or (the leader, and a unit that
    // cannot see its slot) goes on with the order's walk, the leader at the pace of the
    // formation (WP4.6).
    let held;
    let m = if e.form[i].serial != 0 {
        match formation::drive(e, i, m, ground) {
            Some(motion) => {
                held = motion;
                &held
            }
            None => return false,
        }
    } else {
        m
    };
    // A member of a group with a flowfield stops at its own arrival slot, not
    // at the order's point (WP4.3).
    let at_slot = path.arrival_point(e, i);
    let arrival = at_slot.unwrap_or(destination);
    let off = (arrival - e.pos[i]).length_squared();
    if off <= stop_at.square() {
        e.target[i] = None;
        // A move is over and its route with it. A unit in reach of a target (an attack, a
        // guard of an entity) keeps the route it has (WP4.7): the target steps out of
        // reach again and again, and a new request each time is a replan storm; with the
        // route kept the unit steers on after it at once, and asks again for a route only
        // when the target has moved far from where the route was asked for, once in the
        // cooldown.
        if stop_at <= ARRIVE_TOLERANCE {
            e.route.reset(i);
        }
        e.crowd[i].home = at_slot.map(|_| destination);
        return true;
    }
    // A crowd stands on the spot (WP4.4): a unit that has been blocked for a while by
    // units that have arrived (an idle friend was in its way last tick), within a cell
    // and a half of where it is going, has arrived, and two cells more
    // for each sidestep it has tried since. A member of a group (it is going to its
    // slot, which lies some distance from the order's point) has arrived as soon as
    // it is as near the point as its slot is, plus the same: the crowd has filled
    // the ground between. Only for a plain move, not for a reach.
    if m.crowd_wait > 0
        && stop_at <= ARRIVE_TOLERANCE
        && e.crowd[i].quiet >= m.crowd_wait
        && e.crowd[i].idle_hit
    {
        let steps =
            u32::from(e.crowd[i].attempts).max(u32::from(e.crowd[i].pressed) / PRESSED_PER_STEP);
        // A unit that holds a slot has arrived when it is near that slot. One that
        // does not (it shares a slot, or none was handed out yet) has arrived when
        // it is as near the point as its slot is, plus the slack (WP4.4).
        let ring = (arrival - destination).length();
        let radius = crowd_arrival_radius(ring, steps, e.crowd[i].disc);
        if (destination - e.pos[i]).length_squared() <= radius.square() {
            e.target[i] = None;
            e.route.reset(i);
            e.crowd[i].home = at_slot.map(|_| destination);
            return true;
        }
    }
    // A sidestep to a free neighbouring spot (WP4.4) replaces the walk for a few
    // ticks.
    if let Some((spot, left)) = e.crowd[i].side {
        e.target[i] = Some(spot);
        let walked = walk(e, i, spot, m, ground);
        e.crowd[i].side = (walked == Walk::Moving && left > 1).then_some((spot, left - 1));
        return false;
    }
    match path.steer(e, i, destination, m.class) {
        Steer::Wait => {
            e.target[i] = None;
            false
        }
        Steer::Stranded => {
            e.target[i] = None;
            // A route that ends short because the goal cannot be reached is where the
            // unit waits (not stuck); one that was stranded by a refused step, or by a
            // request that found no way round a body (WP4.7), is stuck.
            e.crowd[i].stranded = !e.route[i].unreachable;
            false
        }
        Steer::Go(waypoint, aim) => {
            e.target[i] = Some(waypoint);
            // A unit that has been blocked for a while within a cell of a waypoint of
            // its route takes it as reached (WP4.4): the waypoints are the middles of
            // cells and bodies are as wide as one, so the unit in front is standing
            // on it and the rest would wait for it to leave. Only when the route has
            // another waypoint to go on to: the one a flowfield gives is chosen again
            // from where the unit stands, and would be this one.
            if aim == Aim::Waypoint
                && m.crowd_wait > 0
                && e.crowd[i].quiet >= m.crowd_wait
                && e.route[i].ring_len() >= 2
                && (waypoint - e.pos[i]).length_squared() <= WAYPOINT_CROWD.square()
            {
                path.reached(e, i);
                return false;
            }
            // A unit that has to move to turn cannot be told to land within a step of a
            // waypoint that lies off its line (it would circle it): a waypoint is a point
            // to pass, and it counts as reached a quarter of a cell short, the margin the
            // routes are smoothed with (WP4.5). Not the last waypoint of the ring (see above).
            if aim == Aim::Waypoint
                && e.route[i].ring_len() >= 2
                && m.class.is_some_and(|class| {
                    ground.movement.turning(class).mode == turning::TurnMode::Arc
                })
                && (waypoint - e.pos[i]).length_squared() <= turning::ARC_CAPTURE.square()
            {
                path.reached(e, i);
                return false;
            }
            e.crowd[i].walking = true;
            match walk(e, i, waypoint, m, ground) {
                Walk::Arrived if aim == Aim::Destination => {
                    e.target[i] = None;
                    e.crowd[i].home = at_slot.map(|_| destination);
                    true
                }
                Walk::Arrived => {
                    path.reached(e, i);
                    false
                }
                Walk::Moving => false,
                Walk::Blocked => {
                    e.crowd[i].refused = true;
                    path.blocked(e, i);
                    false
                }
            }
        }
    }
}

/// Runs one tick of the current order of unit `i`, which has
/// `Behavior::Ordered`. `m` is the unit's own motion.
pub(crate) fn run(
    e: &mut Entities,
    i: usize,
    m: &Motion,
    units: &UnitTable,
    ground: &Ground<'_>,
    path: &mut Pathing,
    tick: Tick,
) {
    let bounds = (m.pos_min, m.pos_max);
    // A unit that was stuck after all the replans it may ask for gives its order up
    // (WP4.7): the next order of its queue starts, or it stands idle where it is.
    if e.crowd[i].give_up {
        finish(e, i, tick, bounds);
        // Kept for the presentation until the unit is given an order (below).
        e.crowd[i].gave_up_at = Some(tick);
        return;
    }
    match e.order[i] {
        // Not reachable through the invariant (a unit with no order is idle),
        // but a stray one settles instead of looping.
        Order::Idle | Order::Scatter => clear(e, i),
        // Never moves. Phase 5 may fire from here, never chase.
        Order::HoldPosition => would_acquire_targets_here(e, i),
        Order::Move(destination) => {
            if go_to(e, i, destination, ARRIVE_TOLERANCE, m, ground, path) {
                finish(e, i, tick, bounds);
            }
        }
        Order::AttackMove(destination) => {
            would_acquire_targets_here(e, i);
            if go_to(e, i, destination, ARRIVE_TOLERANCE, m, ground, path) {
                finish(e, i, tick, bounds);
            }
        }
        Order::Guard(GuardTarget::Point(post)) => {
            would_acquire_targets_here(e, i);
            // Never ends by itself; a unit that has been moved off its post
            // walks back.
            go_to(e, i, post, ARRIVE_TOLERANCE, m, ground, path);
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
                let arrived = go_to(e, i, at, close, m, ground, path);
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
            if go_to(e, i, at, reach, m, ground, path)
                && let Some(wanted) = (at - e.pos[i]).facing()
                && !turret::has_turret(e, i, units)
            {
                // In reach: face the target the way the class turns on the spot
                // (WP4.5). A unit with a turret leaves that to the turret and
                // keeps its hull as it is. Firing comes in phase five.
                let turning = m
                    .class
                    .map_or(Turning::LEGACY, |class| ground.movement.turning(class));
                let (facing, carry) = turning::turn_in_place(
                    &turning,
                    e.facing[i],
                    e.turn_carry[i],
                    wanted,
                    m.turn_per_tick,
                );
                e.facing[i] = facing;
                e.turn_carry[i] = carry;
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
