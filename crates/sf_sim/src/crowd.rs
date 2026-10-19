//! Collisions and local avoidance (WP4.4).
//!
//! Units have a body: a disc whose radius is half the `size` of their definition
//! ([`Shape`]). After every unit has taken the step its order asked for
//! ([`behavior::advance_all`](crate::behavior)), the **crowd pass** ([`Crowd::resolve`])
//! pushes the bodies that overlap apart, steers units that meet head-on to the same
//! side, and notices units that make no progress. The rules, the numbers and the
//! reasons are in `docs/rules/collision.md`; this is the code.
//!
//! # Who takes part
//!
//! A *participant* is a live entity made from the unit table, with a movement class
//! that terrain restricts (aircraft pass over everything), whose behaviour is
//! [`Behavior::Idle`] or [`Behavior::Ordered`]. Its **role** says how it gives way:
//!
//! | Role | Is | Gives way to |
//! |---|---|---|
//! | [`Role::Mover`] | an order that walks (move, attack-move, attack, guard, a scatter in the queue) | other movers, by mass |
//! | [`Role::Idle`] | no order | a *friendly* mover, up to [`IDLE_TICK_CAP`] a tick and [`SHOVE_LIMIT`] in all, and only while the spot it moves to is free |
//! | [`Role::Hold`] | `HoldPosition` | nobody: it is never pushed |
//!
//! The legacy walks (`Wander`, `MoveTo`) and everything without a table entry are not
//! participants at all: they neither push nor are pushed, which is why the pinned
//! wanderer scenarios did not move.
//!
//! # The pass
//!
//! Up to [`PASSES`] times: (1) rebuild the index of bodies from the positions as they
//! are; (2) for every overlapping pair that has a unit that moves (or was pushed in
//! the pass before), work out how the overlap is shared, by weight; (3) on the first
//! pass only, add the steering: a unit that has met another coming the other way
//! slips to its own right, and one that has waited long enough for something that
//! does not move does the same; (4) move every unit by the sum of what it was
//! asked, if the terrain, the map edge and the bodies that do not give way allow it.
//! After the last pass the units that make no progress are looked at: they wait,
//! detour, step to a free neighbouring spot, and in the end ask for a new route.
//!
//! # Order independence
//!
//! Every pass reads one snapshot of the positions and sums, for each unit, the push
//! of every overlapping pair (a function of the two units alone, each share rounded
//! on its own so that it does not matter which of the two is called first). Integer
//! addition does not care in which order the pairs come. Each unit is then judged
//! (may it go there?) against the same snapshot, and only when all are judged are
//! the moves written. So the result does not depend on the order the units, the
//! pairs or the buckets are visited in; `tests/collision.rs` spawns one crowd in
//! three orders and compares where every unit ends. A group order given with the
//! same ids listed in another order gives the same result as well (the world sorts
//! the ids by slot before it makes the groups). The one place a slot index is used is
//! two bodies on exactly the same point, where something has to say which way each
//! goes: units that start on exactly the same point are **split by slot index** (a
//! hash of the two indices picks the direction, the lower slot goes that way and the
//! higher the opposite). Since a slot index is the spawn order, a crowd that is
//! spawned on one point can end differently when it is spawned in another order;
//! units that start apart cannot.
//!
//! # Memory and time
//!
//! The scratch arrays are sized by the number of slots and reused; a uniform grid of
//! buckets, at least as wide as the biggest pair distance, is rebuilt by a counting
//! sort in slot order before each pass. A pass looks at the nine buckets round each
//! active unit, as three stretches (the buckets of a row are one after the other), and
//! the position and size of every body is copied into the index, so that looking at a
//! neighbour is a read of what lies next to the last one; the cost of a tick is the
//! number of neighbours looked at, and most are told apart by the distance along one
//! axis (`apart_on_an_axis`) before any product of wide numbers. [`PASSES`] passes at
//! most, and none when nothing overlaps; nothing at all when no unit walks an order
//! and no idle unit is still settling.

use sf_math::hash::Fnv1a64;
use sf_math::{Facing, Fx, Vec2};

use crate::behavior::Behavior;
use crate::entities::{Entities, NO_DEF};
use crate::movement::{Ground, MovementTable};
use crate::orders::{self, Order};
use crate::path::{OVERLAY_RANGE, Overlay, Pathing};
use crate::units::UnitTable;
use crate::world::mix64;

/// Ticks a unit that cannot get on waits before it detours (the `wait_ticks` of
/// `sim.ron`).
pub const DEFAULT_WAIT_TICKS: u16 = 6;

/// Ticks without progress after which a unit steps to a free neighbouring spot
/// (the `stuck_ticks` of `sim.ron`).
pub const DEFAULT_STUCK_TICKS: u16 = 60;

/// Sidesteps a stuck unit tries before it asks for a new route (the
/// `stuck_attempts` of `sim.ron`).
pub const DEFAULT_STUCK_ATTEMPTS: u8 = 3;

/// The most `stuck_ticks` a world takes.
pub const MAX_STUCK_TICKS: u16 = 1000;

/// The collision settings of a world. Game data (`assets/config/sim.ron`); a world
/// that is given none uses [`CollisionConfig::DEFAULT`], the same numbers.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct CollisionConfig {
    /// False turns the whole crowd pass off: units pass through each other (what
    /// the simulation did before WP4.4).
    pub enabled: bool,
    /// Ticks without progress after which a unit detours round what blocks it.
    pub wait_ticks: u16,
    /// Ticks without progress after which a unit steps to a free neighbouring spot.
    pub stuck_ticks: u16,
    /// Sidesteps in a row after which it asks for a new route.
    pub stuck_attempts: u8,
}

impl CollisionConfig {
    /// The numbers of the shipped `sim.ron`.
    pub const DEFAULT: CollisionConfig = CollisionConfig {
        enabled: true,
        wait_ticks: DEFAULT_WAIT_TICKS,
        stuck_ticks: DEFAULT_STUCK_TICKS,
        stuck_attempts: DEFAULT_STUCK_ATTEMPTS,
    };

    /// Whether a world takes these numbers: at least one tick to wait, a stuck
    /// limit above it (and at most [`MAX_STUCK_TICKS`]) and at least one attempt.
    pub const fn is_valid(&self) -> bool {
        self.wait_ticks >= 1
            && self.stuck_ticks > self.wait_ticks
            && self.stuck_ticks <= MAX_STUCK_TICKS
            && self.stuck_attempts >= 1
    }
}

impl Default for CollisionConfig {
    fn default() -> CollisionConfig {
        CollisionConfig::DEFAULT
    }
}

/// What the crowd pass did; for tests and the debug panel. Not part of the
/// checksum.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct CrowdStats {
    /// Ticks in which at least one pair was pushed apart.
    pub busy_ticks: u64,
    /// Idle units pushed aside (counted once per unit and tick).
    pub shoves: u64,
    /// Sidesteps to a free neighbouring spot.
    pub sidesteps: u64,
    /// Requests for a new route after the sidesteps did not help.
    pub replans: u64,
    /// Orders given up because the unit was stuck after its replans (WP4.7).
    pub gave_up: u64,
    /// Most participants in one tick.
    pub peak_bodies: u32,
}

// ---- the numbers ----------------------------------------------------------------

/// Bodies may overlap by this much before they are pushed apart, in cells: the
/// slack that keeps a settled crowd from twitching.
pub const SLOP: Fx = Fx::from_ratio(1, 32);

/// Units this small (diameter, in cells) are infantry-sized: two of them may stand
/// [`SOFT_NUM`]/[`SOFT_DEN`] of the sum of their radii apart.
pub const SOFT_SIZE: Fx = Fx::from_ratio(3, 4);

/// The softer distance between two small bodies, as a fraction.
pub const SOFT_NUM: i64 = 3;
/// See [`SOFT_NUM`].
pub const SOFT_DEN: i64 = 4;

/// Passes of the pushing per tick.
pub const PASSES: usize = 6;

/// The most a moving unit is moved by pushes in one pass, in cells.
pub const MOVER_PASS_CAP: Fx = Fx::from_ratio(1, 4);

/// The most an idle unit is pushed in one tick, in cells (about the step of a
/// light tank).
pub const IDLE_TICK_CAP: Fx = Fx::from_ratio(1, 8);

/// The most an idle unit is pushed in all before it has been left alone for
/// [`SHOVE_CALM`] ticks, in cells.
pub const SHOVE_LIMIT: Fx = Fx::from_int(3);

/// Ticks an idle unit must be left alone before its push allowance is back.
pub const SHOVE_CALM: u8 = 40;

/// How far an idle unit may be shoved into another idle unit that could give way,
/// beyond the slack, in cells (WP4.4b): a chain of shoved units must not squeeze
/// together more than this, so the crowd stays inside the overlap bound whatever
/// presses on it. A shove that would go further is refused and the mover waits.
pub const COMPRESS: Fx = Fx::from_ratio(1, 8);

/// The deepest a mover may end a tick in another body, in cells (WP4.4b). The
/// pushes usually bring a pair apart, but a mover pressed into a crowd that cannot
/// give way (everybody round it is squeezed too) is not pushed out in six passes.
/// Such a mover goes back to where it was at the start of the tick, as far as that is
/// less deep, and waits. This keeps the bodies inside the overlap bound whatever
/// walks into the crowd.
pub const MOVER_OVERLAP: Fx = Fx::from_ratio(3, 16);

/// Ticks a settled unit must be left alone before it closes up.
pub const HOME_WAIT: u8 = 20;

/// How far a settled unit closes up toward the point in a tick, in cells: slower
/// than anything that walks, so that it never catches up with a unit that is
/// shoving it.
pub const HOME_STEP: Fx = Fx::from_ratio(1, 16);

/// A settled unit this near the point its group was sent to has closed up.
pub const HOME_EPS: Fx = Fx::from_ratio(1, 16);

/// How far across a settled unit may slide when straight at the point is taken: the
/// sideways part of the direction against the part toward the point, so that the
/// unit turns away from the point by about 35, 63, 76 and 83 degrees. Every one of
/// them still brings it nearer.
pub const HOME_SLIDE: [Fx; 4] = [
    Fx::from_ratio(7, 10),
    Fx::from_int(2),
    Fx::from_int(4),
    Fx::from_int(8),
];

/// A resting unit this near a unit that moved gets another try, in cells.
const WAKE_RANGE: Fx = Fx::from_int(3);

/// The deepest two bodies may end a tick in each other, in cells (WP4.4b): what the
/// pushes and the allowances leave beyond it is pushed apart by `separate`. Below the
/// quarter of a cell the bound of the phase asks for.
pub const HARD_OVERLAP: Fx = Fx::from_ratio(3, 16);

/// How deep two settled bodies may rest in each other before the pass pushes them apart, in
/// cells (WP4.4b): less than the sixteenth of a cell the end of a run may overlap by, more than
/// the slack. `separate` only acts above [`HARD_OVERLAP`] and the passes only look at units that
/// move or were pushed this tick, so a pair that came to rest between the two (a shove that
/// squeezed an idle friend by [`COMPRESS`] beyond the slack, and then nothing touched either)
/// stayed so for good.
pub const REST_OVERLAP: Fx = Fx::from_ratio(3, 64);

/// The most pairs `separate` takes in a tick: what is left is for the next.
const DEEP_MAX: usize = 4096;

/// The most `separate` moves a unit, in cells.
pub const SEPARATE_CAP: Fx = Fx::from_ratio(1, 4);

/// How many times `separate` runs on the tick units are placed (WP4.4b): the units of a
/// scenario or a spawn are put down on a grid of cells, and bodies wider than a cell
/// overlap their neighbours all through the block. One round only moves the units at
/// the edge of it, since the pushes of a neighbour on each side cancel; the next
/// brings the layer inside it, and so on, so the block spreads out until it is within
/// [`HARD_OVERLAP`] on the tick it is placed. Later ticks run it once.
const SPAWN_ROUNDS: usize = 64;

/// A unit that has come this near a new best distance to its waypoint has made
/// progress.
pub const PROGRESS_EPS: Fx = Fx::from_ratio(1, 32);

/// A step shorter than this is not a heading.
const HEADING_MIN: Fx = Fx::from_ratio(1, 256);

/// Head-on means the headings are more than 120 degrees apart: their dot product
/// is below minus one half.
const OPPOSED: Fx = Fx::from_ratio(-1, 2);

/// Moving the same way means the headings are less than 60 degrees apart: their
/// dot product is above one half.
const ALIGNED: Fx = Fx::from_ratio(1, 2);

/// A unit that is blocked and within this many cells of the point it is walking to
/// has arrived (the crowd is standing on its slot).
pub const CROWD_ARRIVE: Fx = Fx::from_ratio(3, 2);

/// Each sidestep a unit has tried without getting anywhere widens that radius by
/// this many cells: a crowd sent to one point has no room for everybody on it.
pub const CROWD_ARRIVE_STEP: Fx = Fx::from_int(2);

/// A crowd held up on its destination counts as arrived no farther out than this part
/// (13 tenths) of the radius of the disc its bodies fill (`Pathing::group_disc`): the
/// slack that grows with each sidestep must not let a unit count as arrived at any
/// distance. The unit's own slot, and the slack of [`CROWD_ARRIVE`] round it, always
/// count.
pub const CROWD_ARRIVE_DISC: Fx = Fx::from_ratio(13, 10);

/// The same bound for a group that has no field to measure a disc by (fewer than the
/// units that get one): a few bodies fill little ground, in cells.
pub const CROWD_ARRIVE_ALONE: Fx = Fx::from_int(4);

/// A unit that has been blocked for a while and is within this many cells of a
/// waypoint of its route takes the waypoint as reached.
pub const WAYPOINT_CROWD: Fx = Fx::ONE;

/// Ticks a sidestep may last at most (it ends when the spot is reached).
const SIDESTEP_TICKS: u8 = 40;

/// The weight of a unit that is moved easily (an idle friend of the mover).
const W_EASY: i64 = 1000;

/// The weight a mover of mass one has; heavier movers divide it.
const W_MOVER: i64 = 12;

// ---- per-unit state -------------------------------------------------------------

/// A sidestep in progress: the spot and the ticks left to get there.
pub(crate) type Sidestep = (Vec2, u8);

/// The arrival slot a member of a group has taken (WP4.4b, `collision.md`,
/// "Arrival"): the field it follows and the place in that field's slots. It is
/// kept while the unit stands on its slot after the order is done, and let go
/// when the unit gets a new order (`orders::begin`) or is removed. Not part of
/// "at rest": it is hashed with the field's table of holders.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct Claim {
    /// The serial of the field; zero for no claim.
    pub serial: u32,
    /// The place in the field's slots, or [`Claim::SHARED`] when every slot was
    /// taken and the unit stands on the one its rank shares.
    pub slot: u16,
}

impl Claim {
    /// No slot of its own was left.
    pub const SHARED: u16 = u16::MAX;
}

/// What a unit remembers about being in a crowd. The first group lives from tick to
/// tick and is hashed while it is not at rest; the second is scratch within one
/// tick: set while the unit carries out its order, read by the crowd pass, and
/// cleared when the unit's next turn begins.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct CrowdState {
    /// Ticks since the unit last came closer to its waypoint.
    pub quiet: u16,
    /// Sidesteps tried since the unit last made real progress.
    pub attempts: u8,
    /// The best distance to the waypoint `mark` since `quiet` was last zero.
    pub best: Fx,
    /// The waypoint `best` is about.
    pub mark: Option<Vec2>,
    /// The sidestep in progress.
    pub side: Option<Sidestep>,
    /// How far this idle unit has been pushed since it was last left alone.
    pub shoved: Fx,
    /// Ticks since it was last pushed.
    pub calm: u8,
    /// The unit has asked for a new route (`World::replan_requested`).
    pub replan: bool,
    /// Replans this order has asked for because the unit was stuck (WP4.7); the order
    /// is given up when it would ask once more than `ReplanConfig::max`.
    pub replans: u8,
    /// Ticks until the unit may ask for a replan again (`ReplanConfig::cooldown`).
    pub cool: u16,
    /// Ticks until the unit may search again for a target that moved
    /// (`ReplanConfig::retarget_cooldown`).
    pub retarget: u16,
    /// The unit has used up its replans: its order ends on its next turn (WP4.7).
    pub give_up: bool,
    /// The tick at which the unit last gave an order up (WP4.7), for the presentation
    /// (`World::gave_up_at`). Unlike the rest of the replan bookkeeping it outlives the
    /// order that was given up: a new order given to the unit (a command) clears it
    /// (`orders::forget_give_up`), `reset_progress` does not.
    pub gave_up_at: Option<crate::ids::Tick>,
    /// Last tick a body that has arrived (an idle friend) was in the way: only then
    /// does a blocked unit count as arrived for being crowded out of its spot. A
    /// unit stuck in a queue of other walkers has not arrived.
    pub idle_hit: bool,
    /// Ticks the unit has been held up by units that have arrived, all told (a new
    /// waypoint does not start it again): the longer it presses on the crowd that
    /// stands on its destination, the farther from it it counts as arrived
    /// (`orders::go_to`). Cleared by the next order.
    pub pressed: u16,
    /// The arrival slot the unit has taken.
    pub claim: Claim,
    /// Where the unit gravitates once it has arrived: the point its group was sent
    /// to. A settled unit that has been left alone (`HOME_WAIT`) closes up toward it
    /// as far as the way is free, so a crowd that was spread by what pushed through
    /// it gathers again. Set when the order of a group member ends, cleared by the
    /// next order.
    pub home: Option<Vec2>,
    /// Ticks since this settled unit was last touched or overlapped.
    pub home_wait: u8,
    /// The settled unit cannot close up any further: it is left alone until something
    /// touches it or moves next to it.
    pub home_rest: bool,
    /// The radius of the disc the bodies of the group the unit was ordered with fill,
    /// in cells: zero for a unit that was not ordered with enough others to be a group
    /// (`Pathing::group_order`). Bounds how far out the slack of a crowd held up on its
    /// destination lets the unit count as arrived (`orders::go_to`). Cleared by the next
    /// order.
    pub disc: Fx,
    /// The unit has just stopped (its order ended): the crowd pass looks at it once,
    /// so that a unit that came to rest overlapping a neighbour is pushed clear of it
    /// instead of staying so for good. Set by `orders`, cleared by the pass.
    pub settle: bool,
    /// The unit was placed this tick (set by `Entities::fill`, cleared by the pass,
    /// which then separates the whole block of units placed with it, `SPAWN_ROUNDS`).
    pub fresh: bool,
    // ---- scratch within a tick ----
    /// The order walked this tick (a waypoint to steer at).
    pub walking: bool,
    /// The terrain refused the step.
    pub refused: bool,
    /// The route is stranded on a spot that is not an unreachable goal.
    pub stranded: bool,
}

impl CrowdState {
    /// Nothing remembered between ticks.
    pub(crate) fn at_rest(&self) -> bool {
        self.quiet == 0
            && self.attempts == 0
            && self.side.is_none()
            && self.shoved == Fx::ZERO
            && self.calm == 0
            && self.pressed == 0
            && !self.replan
            && !self.give_up
            && self.gave_up_at.is_none()
            && self.replans == 0
            && self.cool == 0
            && self.retarget == 0
            && !self.idle_hit
            && self.home.is_none()
    }

    /// The order changed or ended: forget the progress.
    pub(crate) fn reset_progress(&mut self) {
        self.quiet = 0;
        self.pressed = 0;
        self.attempts = 0;
        self.best = Fx::ZERO;
        self.mark = None;
        self.side = None;
        self.replan = false;
        self.replans = 0;
        self.cool = 0;
        self.retarget = 0;
        self.give_up = false;
        self.walking = false;
        self.refused = false;
        self.stranded = false;
    }

    pub(crate) fn write_hash(&self, h: &mut Fnv1a64) {
        h.write_u32(u32::from(self.quiet) | u32::from(self.attempts) << 16);
        if self.pressed > 0 {
            h.write_u8(0xC3);
            h.write_u32(u32::from(self.pressed));
        }
        if self.quiet > 0 {
            h.write_fx(self.best);
            match self.mark {
                Some(m) => {
                    h.write_u8(1);
                    h.write_vec2(m);
                }
                None => h.write_u8(0),
            }
        }
        match self.side {
            Some((spot, left)) => {
                h.write_u8(1);
                h.write_vec2(spot);
                h.write_u8(left);
            }
            None => h.write_u8(0),
        }
        h.write_fx(self.shoved);
        h.write_u8(self.calm);
        h.write_u8(
            u8::from(self.replan) | u8::from(self.idle_hit) << 1 | u8::from(self.give_up) << 2,
        );
        // The replan bookkeeping (WP4.7); nothing for a unit that has none.
        if self.replans != 0 || self.cool != 0 || self.retarget != 0 {
            h.write_u8(0xC5);
            h.write_u8(self.replans);
            h.write_u32(u32::from(self.cool) | u32::from(self.retarget) << 16);
        }
        if let Some(at) = self.gave_up_at {
            h.write_u8(0xC6);
            h.write_u64(at.0);
        }
        // Nothing for a unit that never took a slot, so a world without groups
        // hashes as before.
        if self.claim.serial != 0 {
            h.write_u8(0xC1);
            h.write_u32(self.claim.serial);
            h.write_u32(u32::from(self.claim.slot));
        }
        if self.disc != Fx::ZERO {
            h.write_u8(0xC4);
            h.write_fx(self.disc);
        }
        if let Some(home) = self.home {
            h.write_u8(0xC2 | u8::from(self.home_rest) << 1);
            h.write_vec2(home);
            h.write_u8(self.home_wait);
        }
    }
}

// ---- shapes -----------------------------------------------------------------------

/// The body of a unit kind.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct Shape {
    /// Whether units of this kind take part at all.
    pub collides: bool,
    /// Half the size of the definition.
    pub radius: Fx,
    /// Infantry-sized: its pairs with other small bodies are softer.
    pub small: bool,
    /// One for a body up to a cell wide, up to four for the biggest.
    pub mass: u8,
    /// The movement class (a participant has one).
    pub class: u16,
}

impl Shape {
    const NONE: Shape = Shape {
        collides: false,
        radius: Fx::ZERO,
        small: false,
        mass: 1,
        class: 0,
    };
}

/// The shapes of every kind of the unit table, in table order. A kind takes part
/// when it has a movement class that the movement table either does not know or
/// restricts (aircraft do not collide).
fn shapes_of(units: &UnitTable, movement: &MovementTable) -> Vec<Shape> {
    (0..units.len())
        .map(|k| {
            let s = units.at(k);
            let Some(class) = s.movement_class else {
                return Shape::NONE;
            };
            let flies = movement.has_class(class) && !movement.restricts(class);
            let radius = s.size.div_int(2);
            Shape {
                collides: !flies && radius > Fx::ZERO,
                radius,
                small: s.size <= SOFT_SIZE,
                mass: s.size.ceil_int().clamp(1, 4) as u8,
                class,
            }
        })
        .collect()
}

// ---- roles and bodies -------------------------------------------------------------

/// How a participant gives way.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Role {
    /// Does not take part.
    None,
    /// Walks an order.
    Mover,
    /// Has no order.
    Idle,
    /// `HoldPosition`.
    Hold,
}

#[derive(Copy, Clone, Debug)]
struct Body {
    role: Role,
    radius: Fx,
    small: bool,
    mass: u8,
    class: u16,
    owner: u8,
}

impl Body {
    const NONE: Body = Body {
        role: Role::None,
        radius: Fx::ZERO,
        small: false,
        mass: 1,
        class: 0,
        owner: 0,
    };
}

/// The smallest distance between the middles of two bodies.
fn min_distance(a: &Body, b: &Body) -> Fx {
    distance_between(a.radius, a.small, b.radius, b.small)
}

/// [`min_distance`] from the radii and the sizes of the two bodies.
#[inline(always)]
fn distance_between(ra: Fx, small_a: bool, rb: Fx, small_b: bool) -> Fx {
    let sum = ra + rb;
    if small_a && small_b {
        sum.mul_int(SOFT_NUM).div_int(SOFT_DEN)
    } else {
        sum
    }
}

// ---- the spatial index --------------------------------------------------------------

/// A uniform grid of buckets over the map: a counting sort of the participants by
/// bucket, in slot order, so what a bucket holds is ascending and the same on every
/// machine. Fixed arrays, rebuilt before each pass.
#[derive(Clone, Debug, Default)]
struct Index {
    /// Cells to a bucket side.
    side: i64,
    across: u32,
    down: u32,
    start: Vec<u32>,
    fill: Vec<u32>,
    items: Vec<u32>,
    /// What the passes ask of every neighbour, in the order of `items`: copied into the
    /// index so that a stretch of a bucket row is read in one go instead of by a
    /// lookup of the position and the body of each slot (the cost of a pass is the
    /// number of neighbours it looks at). Valid as long as the index is: it is built
    /// from the positions at that moment.
    packed: Vec<Near>,
}

/// A body as the index holds it: its slot, its position and what its distance to
/// another body depends on.
#[derive(Copy, Clone, Debug)]
struct Near {
    id: u32,
    small: bool,
    radius: Fx,
    at: Vec2,
}

impl Near {
    const EMPTY: Near = Near {
        id: 0,
        small: false,
        radius: Fx::ZERO,
        at: Vec2::ZERO,
    };
}

impl Index {
    fn configure(&mut self, side: i64, width: u32, height: u32) {
        self.side = side.max(1);
        self.across = (i64::from(width) / self.side + 1) as u32;
        self.down = (i64::from(height) / self.side + 1) as u32;
        let n = (self.across * self.down) as usize;
        self.start = vec![0; n + 1];
        self.fill = vec![0; n];
    }

    fn bucket(&self, p: Vec2) -> (u32, u32) {
        let bx = (p.x.floor_int().max(0) / self.side) as u32;
        let by = (p.y.floor_int().max(0) / self.side) as u32;
        (bx.min(self.across - 1), by.min(self.down - 1))
    }

    fn build(&mut self, e: &Entities, bodies: &[Body]) {
        self.start.iter_mut().for_each(|c| *c = 0);
        let mut count = 0;
        for (i, b) in bodies.iter().enumerate() {
            if b.role != Role::None {
                let (bx, by) = self.bucket(e.pos[i]);
                self.start[(by * self.across + bx) as usize + 1] += 1;
                count += 1;
            }
        }
        for k in 1..self.start.len() {
            self.start[k] += self.start[k - 1];
        }
        let n = self.fill.len();
        self.fill.copy_from_slice(&self.start[..n]);
        self.items.clear();
        self.items.resize(count, 0);
        self.packed.clear();
        self.packed.resize(count, Near::EMPTY);
        for (i, b) in bodies.iter().enumerate() {
            if b.role != Role::None {
                let (bx, by) = self.bucket(e.pos[i]);
                let at = (by * self.across + bx) as usize;
                let place = self.fill[at] as usize;
                self.items[place] = i as u32;
                self.packed[place] = Near {
                    id: i as u32,
                    small: b.small,
                    radius: b.radius,
                    at: e.pos[i],
                };
                self.fill[at] += 1;
            }
        }
    }

    /// The participants in the nine buckets round `p`, as up to three stretches, one for
    /// each row of buckets and ascending within a bucket. The buckets of a row lie one
    /// after the other in `items`, so the three of a row are one stretch of it, in the
    /// same order as taking them one by one.
    fn rows(&self, p: Vec2) -> impl Iterator<Item = &[u32]> + '_ {
        let (bx, by) = self.bucket(p);
        let (x0, x1) = (bx.saturating_sub(1), (bx + 1).min(self.across - 1));
        let (y0, y1) = (by.saturating_sub(1), (by + 1).min(self.down - 1));
        (y0..=y1).map(move |y| {
            let row = (y * self.across) as usize;
            let from = self.start[row + x0 as usize] as usize;
            let to = self.start[row + x1 as usize + 1] as usize;
            &self.items[from..to]
        })
    }

    /// [`Index::rows`] with the position and the body of each participant.
    fn stretches(&self, p: Vec2) -> impl Iterator<Item = &[Near]> + '_ {
        let (bx, by) = self.bucket(p);
        let (x0, x1) = (bx.saturating_sub(1), (bx + 1).min(self.across - 1));
        let (y0, y1) = (by.saturating_sub(1), (by + 1).min(self.down - 1));
        (y0..=y1).map(move |y| {
            let row = (y * self.across) as usize;
            let from = self.start[row + x0 as usize] as usize;
            let to = self.start[row + x1 as usize + 1] as usize;
            &self.packed[from..to]
        })
    }

    /// The participants in the nine buckets round `p`, ascending within a bucket.
    fn near(&self, p: Vec2) -> impl Iterator<Item = u32> + '_ {
        self.rows(p).flat_map(|row| row.iter().copied())
    }
}

// ---- the world's part ------------------------------------------------------------------

/// Scratch arrays of the pass, sized by the number of slots and reused.
#[derive(Clone, Debug, Default)]
struct Scratch {
    index: Index,
    bodies: Vec<Body>,
    delta: Vec<Vec2>,
    delta_mark: Vec<u32>,
    /// The sum of the lengths of the pushes on a unit this pass: what it was asked
    /// to give, as against what is left of them once they cancel.
    demand: Vec<Fx>,
    active_mark: Vec<u32>,
    room: Vec<Fx>,
    frozen: Vec<bool>,
    heading: Vec<Vec2>,
    step_len: Vec<Fx>,
    movers: Vec<u32>,
    /// Idle units that were pushed lately and are still settling.
    settling: Vec<u32>,
    active: Vec<u32>,
    dirty: Vec<u32>,
    touched: Vec<u32>,
    touched_flag: Vec<bool>,
    /// Idle units a body overlapped this tick, pushed or not: they are not calm.
    contact: Vec<bool>,
    /// Bodies that overlapped another beyond the slack in some pass of this tick,
    /// pushed or not: the ones the checks after the passes look at.
    lapped: Vec<bool>,
    /// A unit with a body was placed this tick.
    fresh: bool,
    /// The passes ended because a look at every pair found none overlapping beyond the
    /// slack: nothing has moved since, so no pair is deeper than the bounds of
    /// `hold_back` and `separate` and they are not run.
    passes_clear: bool,
    /// Movers that an idle friend was in the way of this tick.
    idle_hit: Vec<bool>,
    /// Movers that gave way to one ahead of them going the same way this tick: a
    /// unit waiting in line is not stuck.
    in_line: Vec<bool>,
    /// The neighbours of the unit being looked at.
    near: Vec<u32>,
    /// Pairs of bodies that overlap too deeply (`separate`).
    deep: Vec<(u32, u32)>,
    /// The moves of one pass, written after all are decided.
    moves: Vec<(u32, Vec2, Fx)>,
    /// The idle units whose push was refused in one pass.
    stuck: Vec<u32>,
    epoch: u32,
}

impl Scratch {
    fn prepare(&mut self, slots: usize) {
        if self.bodies.len() < slots {
            self.bodies.resize(slots, Body::NONE);
            self.delta.resize(slots, Vec2::ZERO);
            self.delta_mark.resize(slots, 0);
            self.demand.resize(slots, Fx::ZERO);
            self.active_mark.resize(slots, 0);
            self.room.resize(slots, Fx::ZERO);
            self.frozen.resize(slots, false);
            self.heading.resize(slots, Vec2::ZERO);
            self.step_len.resize(slots, Fx::ZERO);
            self.touched_flag.resize(slots, false);
            self.contact.resize(slots, false);
            self.lapped.resize(slots, false);
            self.idle_hit.resize(slots, false);
            self.in_line.resize(slots, false);
        }
        self.movers.clear();
        self.fresh = false;
        self.passes_clear = false;
        self.settling.clear();
        self.active.clear();
        self.dirty.clear();
        self.touched.clear();
    }
}

/// Whether two bodies `diff` apart are clear of each other, told by the cheap test: at
/// least `dmin` apart along an axis. Exact for what it says (the squared distance of such
/// a pair is at least `dmin` squared, whatever the rounding), so every pair it answers
/// is one the squared test would have answered the same; the pairs it does not answer
/// are looked at by the full test. Almost all the neighbours of a body in the nine
/// buckets round it are clear this way, without a multiplication of wide numbers.
#[inline(always)]
fn apart_on_an_axis(diff: Vec2, dmin: Fx) -> bool {
    diff.x.abs() >= dmin || diff.y.abs() >= dmin
}

/// The crowd pass of a world: its settings, the shapes of the unit kinds, the
/// scratch memory and the statistics.
#[derive(Clone, Debug)]
pub(crate) struct Crowd {
    pub config: CollisionConfig,
    shapes: Vec<Shape>,
    side: i64,
    scratch: Scratch,
    pub stats: CrowdStats,
}

impl PartialEq for Crowd {
    /// The scratch memory is not state.
    fn eq(&self, other: &Crowd) -> bool {
        self.config == other.config && self.shapes == other.shapes && self.stats == other.stats
    }
}

impl Eq for Crowd {}

impl Crowd {
    pub(crate) fn new() -> Crowd {
        Crowd {
            config: CollisionConfig::DEFAULT,
            shapes: Vec::new(),
            side: 2,
            scratch: Scratch::default(),
            stats: CrowdStats::default(),
        }
    }

    /// Reads the shapes again from the tables (when either changes).
    pub(crate) fn set_tables(
        &mut self,
        units: &UnitTable,
        movement: &MovementTable,
        map: (u32, u32),
    ) {
        self.shapes = shapes_of(units, movement);
        let widest = self
            .shapes
            .iter()
            .filter(|s| s.collides)
            .map(|s| s.radius.mul_int(2))
            .max()
            .unwrap_or(Fx::ONE);
        self.side = widest.ceil_int().max(1);
        self.scratch.index.configure(self.side, map.0, map.1);
        self.scratch.bodies.clear();
    }

    /// The radius of the body of the unit kind at table position `def`; zero for a
    /// kind that does not take part.
    pub(crate) fn radius_of_def(&self, def: usize) -> Fx {
        self.shapes
            .get(def)
            .filter(|s| s.collides)
            .map_or(Fx::ZERO, |s| s.radius)
    }

    /// Whether any unit kind takes part: with none there is nothing to do.
    pub(crate) fn any_shapes(&self) -> bool {
        self.shapes.iter().any(|s| s.collides)
    }

    /// The role of the entity in slot `i`.
    fn body_of(&self, e: &Entities, i: usize) -> Body {
        if !e.alive[i] || e.def[i] == NO_DEF {
            return Body::NONE;
        }
        let Some(sh) = self.shapes.get(e.def[i] as usize).filter(|s| s.collides) else {
            return Body::NONE;
        };
        let role = match e.behavior[i] {
            Behavior::Idle => Role::Idle,
            Behavior::Ordered => match e.order[i] {
                Order::HoldPosition => Role::Hold,
                Order::Idle | Order::Scatter => Role::Idle,
                _ => Role::Mover,
            },
            Behavior::Wander | Behavior::MoveTo => return Body::NONE,
        };
        Body {
            role,
            radius: sh.radius,
            small: sh.small,
            mass: sh.mass,
            class: sh.class,
            owner: e.owner[i].0,
        }
    }

    /// The crowd pass of one tick: pushes the overlapping bodies apart, steers the
    /// ones that meet head-on, and looks after the units that do not get on. Runs
    /// after every unit has taken its step.
    pub(crate) fn resolve(
        &mut self,
        e: &mut Entities,
        ground: &Ground<'_>,
        bounds: (Vec2, Vec2),
        path: &mut Pathing,
    ) {
        if !self.config.enabled || !self.any_shapes() {
            return;
        }
        let slots = e.len();
        self.scratch.prepare(slots);
        // Who takes part, and who walks.
        let mut bodies = std::mem::take(&mut self.scratch.bodies);
        let mut count = 0_u32;
        for (i, body) in bodies.iter_mut().enumerate().take(slots) {
            *body = self.body_of(e, i);
            if body.role != Role::None {
                count += 1;
            }
            if body.role == Role::Mover {
                self.scratch.movers.push(i as u32);
            } else if body.role == Role::Idle
                && (e.crowd[i].shoved > Fx::ZERO
                    || e.crowd[i].settle
                    || (!e.crowd[i].home_rest
                        && e.crowd[i]
                            .home
                            .is_some_and(|h| (h - e.pos[i]).length_squared() > HOME_EPS.square())))
            {
                self.scratch.settling.push(i as u32);
            }
            if e.crowd[i].settle {
                // A unit that has just stopped has its whole allowance back: if it
                // came to rest against a neighbour that cannot give way any more, it is
                // the one that moves clear.
                if body.role == Role::Idle {
                    e.crowd[i].shoved = Fx::ZERO;
                    e.crowd[i].calm = 0;
                }
                e.crowd[i].settle = false;
            }
            if e.crowd[i].fresh {
                e.crowd[i].fresh = false;
                self.scratch.fresh |= body.role != Role::None;
            }
        }
        self.stats.peak_bodies = self.stats.peak_bodies.max(count);
        if self.scratch.movers.is_empty() && self.scratch.settling.is_empty() {
            self.scratch.bodies = bodies;
            return;
        }
        let cfg = self.config;
        let mut work = Work {
            cfg,
            bodies: &bodies,
            sc: &mut self.scratch,
            ground,
            bounds,
            stats: &mut self.stats,
        };
        work.prepare_movers(e);
        work.passes(e);
        work.bookkeeping(e, path);
        self.scratch.bodies = bodies;
    }
}

/// The working state of one tick's pass.
struct Work<'a, 'g> {
    cfg: CollisionConfig,
    bodies: &'a [Body],
    sc: &'a mut Scratch,
    ground: &'a Ground<'g>,
    bounds: (Vec2, Vec2),
    stats: &'a mut CrowdStats,
}

impl Work<'_, '_> {
    /// Headings and step lengths of the movers (from the step they just took), and
    /// the push allowance of the idle units.
    fn prepare_movers(&mut self, e: &Entities) {
        for k in 0..self.sc.movers.len() {
            let u = self.sc.movers[k] as usize;
            let step = e.pos[u] - e.prev_pos[u];
            let len = step.length();
            self.sc.step_len[u] = len;
            self.sc.heading[u] = if len >= HEADING_MIN {
                step.normalize_or_zero()
            } else {
                Vec2::ZERO
            };
        }
        for i in 0..self.bodies.len() {
            self.sc.touched_flag[i] = false;
            self.sc.contact[i] = false;
            self.sc.lapped[i] = false;
            self.sc.idle_hit[i] = false;
            self.sc.in_line[i] = false;
            self.sc.frozen[i] = false;
            if self.bodies[i].role == Role::Idle {
                self.sc.room[i] = IDLE_TICK_CAP.min(SHOVE_LIMIT - e.crowd[i].shoved);
                if self.sc.room[i] <= Fx::ZERO {
                    self.sc.room[i] = Fx::ZERO;
                    self.sc.frozen[i] = true;
                }
            }
        }
    }

    /// The weight of `a` against `b`: how willingly `a` gives way. Zero is a body
    /// that does not move.
    fn weight(&self, a: usize, b: usize) -> i64 {
        let (ba, bb) = (&self.bodies[a], &self.bodies[b]);
        match ba.role {
            Role::None | Role::Hold => 0,
            Role::Mover => W_MOVER / i64::from(ba.mass),
            Role::Idle => {
                if self.sc.frozen[a] || self.sc.room[a] <= Fx::ZERO || ba.owner != bb.owner {
                    0
                } else if bb.role == Role::Idle {
                    W_MOVER / i64::from(ba.mass)
                } else {
                    W_EASY
                }
            }
        }
    }

    /// Adds a push to the sum of unit `u` for this pass.
    /// `len` is the length of `v`, which the caller has at hand.
    fn add(&mut self, u: usize, v: Vec2, len: Fx, epoch: u32) {
        if self.sc.delta_mark[u] != epoch {
            self.sc.delta_mark[u] = epoch;
            self.sc.delta[u] = Vec2::ZERO;
            self.sc.demand[u] = Fx::ZERO;
            self.sc.dirty.push(u as u32);
        }
        self.sc.delta[u] += v;
        self.sc.demand[u] += len;
    }

    fn passes(&mut self, e: &mut Entities) {
        let mut busy = false;
        for pass in 0..PASSES {
            self.sc.index.build(e, self.bodies);
            self.sc.epoch = self.sc.epoch.wrapping_add(1).max(1);
            let epoch = self.sc.epoch;
            self.sc.active.clear();
            self.sc.dirty.clear();
            for k in 0..self.sc.movers.len() {
                let u = self.sc.movers[k];
                self.sc.active.push(u);
                self.sc.active_mark[u as usize] = epoch;
            }
            for k in 0..self.sc.settling.len() {
                let u = self.sc.settling[k];
                if self.sc.active_mark[u as usize] != epoch {
                    self.sc.active.push(u);
                    self.sc.active_mark[u as usize] = epoch;
                }
            }
            for k in 0..self.sc.touched.len() {
                let u = self.sc.touched[k];
                if self.sc.active_mark[u as usize] != epoch {
                    self.sc.active.push(u);
                    self.sc.active_mark[u as usize] = epoch;
                }
            }
            let mut contacts = 0_u32;
            for k in 0..self.sc.active.len() {
                let u = self.sc.active[k] as usize;
                let pu = e.pos[u];
                // The neighbours are copied out: the index is read while the sums
                // are written. A pair of two active units is done from the lower.
                let mut near = std::mem::take(&mut self.sc.near);
                near.clear();
                let (ru, su) = (self.bodies[u].radius, self.bodies[u].small);
                for row in self.sc.index.stretches(pu) {
                    for n in row {
                        let vi = n.id as usize;
                        if vi == u || (self.sc.active_mark[vi] == epoch && vi < u) {
                            continue;
                        }
                        let dmin = distance_between(ru, su, n.radius, n.small);
                        if apart_on_an_axis(pu - n.at, dmin) {
                            continue;
                        }
                        near.push(n.id);
                    }
                }
                for &v in &near {
                    if self.pair(e, u, v as usize, pass, epoch) {
                        contacts += 1;
                    }
                }
                self.sc.near = near;
            }
            if contacts == 0 {
                self.sc.passes_clear = true;
                break;
            }
            busy = true;
            self.apply(e);
        }
        if busy {
            self.stats.busy_ticks += 1;
        }
        self.stats.shoves += self.sc.touched.len() as u64;
    }

    /// One pair of bodies. True if they overlap.
    fn pair(&mut self, e: &Entities, u: usize, v: usize, pass: usize, epoch: u32) -> bool {
        let (bu, bv) = (&self.bodies[u], &self.bodies[v]);
        let dmin = min_distance(bu, bv);
        let diff = e.pos[u] - e.pos[v];
        if apart_on_an_axis(diff, dmin) || diff.length_squared() >= dmin.square() {
            return false;
        }
        let dist = diff.length();
        // Overlapping means closer than the slack allows; the push brings the
        // pair to half the slack, so the next look finds nothing to do.
        if dist + SLOP >= dmin {
            return false;
        }
        let (mut wu, mut wv) = (self.weight(u, v), self.weight(v, u));
        let pen = dmin - dist - SLOP.div_int(2);
        self.sc.lapped[u] = true;
        self.sc.lapped[v] = true;
        // An idle unit that a mover overlaps is not left alone, whether or not it can
        // give way.
        if self.bodies[v].role == Role::Mover {
            self.sc.contact[u] = true;
        }
        if self.bodies[u].role == Role::Mover {
            self.sc.contact[v] = true;
        }
        if self.bodies[u].role == Role::Mover
            && self.bodies[v].role == Role::Idle
            && self.bodies[u].owner == self.bodies[v].owner
        {
            self.sc.idle_hit[u] = true;
        }
        if self.bodies[v].role == Role::Mover
            && self.bodies[u].role == Role::Idle
            && self.bodies[u].owner == self.bodies[v].owner
        {
            self.sc.idle_hit[v] = true;
        }
        // Units that move the same way follow: the one behind gives way, the one
        // in front is not pushed on.
        if self.bodies[u].role == Role::Mover && self.bodies[v].role == Role::Mover {
            let (hu, hv) = (self.sc.heading[u], self.sc.heading[v]);
            if !hu.is_zero() && !hv.is_zero() && hu.dot(hv) > ALIGNED {
                let along = (-diff).dot(hu + hv);
                if along > dist {
                    wv = 0;
                    self.sc.in_line[u] = true;
                } else if along < -dist {
                    wu = 0;
                    self.sc.in_line[v] = true;
                }
            }
        }
        // The direction that takes `u` away from `v`.
        let away = if dist > Fx::ZERO {
            diff
        } else {
            let (lo, hi) = if u < v { (u, v) } else { (v, u) };
            let bits = mix64((lo as u64) << 32 | hi as u64);
            let f = Vec2::from_facing(Facing::new((bits >> 56) as u8), Fx::ONE);
            if u < v { f } else { -f }
        };
        if wu + wv > 0 {
            // Each share is rounded on its own, so that the pair gives the same
            // whichever of the two is called `u`.
            let on_u = pen.mul_int(wu).div_int(wu + wv);
            let on_v = pen.mul_int(wv).div_int(wu + wv);
            if on_u > Fx::ZERO {
                self.add(u, away.with_length(on_u), on_u, epoch);
            }
            if on_v > Fx::ZERO {
                self.add(v, (-away).with_length(on_v), on_v, epoch);
            }
        }
        if pass == 0 {
            self.keep_right(e, u, v, wv, away);
            self.keep_right(e, v, u, wu, -away);
        }
        true
    }

    /// The steering of mover `a` against body `b` (`away` points from `b` to `a`):
    /// when `b` is ahead and comes the other way, or stands in the way and `a` has
    /// waited long enough, `a` slips to its own right (alternating with its left on
    /// every other sidestep, so a unit that failed on one side tries the other).
    fn keep_right(&mut self, e: &Entities, a: usize, b: usize, wb: i64, away: Vec2) {
        if self.bodies[a].role != Role::Mover {
            return;
        }
        let h = self.sc.heading[a];
        if h.is_zero() || h.dot(away) >= Fx::ZERO {
            return;
        }
        let hb = self.sc.heading[b];
        let head_on = self.bodies[b].role == Role::Mover && h.dot(hb) < OPPOSED;
        let c = &e.crowd[a];
        let in_the_way = c.quiet >= self.cfg.wait_ticks && wb == 0;
        if !(head_on || in_the_way) {
            return;
        }
        let right = if c.attempts & 1 == 0 {
            h.perp()
        } else {
            -h.perp()
        };
        let push = right.scale(self.sc.step_len[a].div_int(2));
        // The sum is opened by `add` for pass zero only through a pair, so it is
        // always open here.
        let epoch = self.sc.epoch;
        let len = self.sc.step_len[a].div_int(2);
        self.add(a, push, len, epoch);
    }

    /// Moves every unit that was pushed by its sum, as far as the terrain, the
    /// map and the bodies that do not move allow.
    fn apply(&mut self, e: &mut Entities) {
        // Every unit is judged against the positions the pass began with, and only
        // then are the moves written: what one unit does cannot depend on whether
        // another was visited before it.
        let dirty = std::mem::take(&mut self.sc.dirty);
        let mut moves = std::mem::take(&mut self.sc.moves);
        let mut stuck = std::mem::take(&mut self.sc.stuck);
        moves.clear();
        stuck.clear();
        for &u in &dirty {
            let u = u as usize;
            let body = self.bodies[u];
            let cap = match body.role {
                Role::Mover => MOVER_PASS_CAP,
                _ => self.sc.room[u],
            };
            if body.role == Role::Hold || body.role == Role::None || cap <= Fx::ZERO {
                continue;
            }
            let d = self.sc.delta[u].clamp_length(cap);
            if d.is_zero() {
                continue;
            }
            let from = e.pos[u];
            // The whole push, then its two sides alone (a unit pressed against a
            // wall slides along it), then half.
            let tries = [
                d,
                Vec2::new(d.x, Fx::ZERO),
                Vec2::new(Fx::ZERO, d.y),
                d.div_int(2),
            ];
            let mut achieved = Fx::ZERO;
            for t in tries {
                if t.is_zero() {
                    continue;
                }
                let to = (from + t).clamp(self.bounds.0, self.bounds.1);
                if to == from || !self.ground.may_step(body.class, from, to) {
                    continue;
                }
                if body.role == Role::Idle && self.pinned_by_immovable(e, u, to) {
                    continue;
                }
                achieved = t.length();
                moves.push((u as u32, to, achieved));
                break;
            }
            // An idle unit that gets less than half of what it was asked, because
            // the terrain, a body that does not give way, or pushes from two sides
            // at once stop it, is treated as one that does not move in the passes
            // that follow, and the bodies pressing on it give way instead.
            let squeezed = self.sc.delta[u].length().mul_int(2) < self.sc.demand[u];
            let limited = achieved.mul_int(2) < d.length();
            if body.role == Role::Idle && (squeezed || limited) {
                stuck.push(u as u32);
            }
        }
        for &(u, to, len) in &moves {
            let u = u as usize;
            e.pos[u] = to;
            if self.bodies[u].role == Role::Idle {
                self.sc.room[u] = (self.sc.room[u] - len).max(Fx::ZERO);
                let c = &mut e.crowd[u];
                c.shoved += len;
                c.calm = 0;
                if !self.sc.touched_flag[u] {
                    self.sc.touched_flag[u] = true;
                    self.sc.touched.push(u as u32);
                }
            }
        }
        for &u in &stuck {
            self.sc.frozen[u as usize] = true;
        }
        self.sc.dirty = dirty;
        self.sc.moves = moves;
        self.sc.stuck = stuck;
    }

    /// Whether idle unit `u` at `to` would overlap a body that does not give way to
    /// it: a unit holding position, an enemy, or an idle unit that cannot move.
    fn pinned_by_immovable(&self, e: &Entities, u: usize, to: Vec2) -> bool {
        for v in self.sc.index.near(to) {
            let v = v as usize;
            if v == u {
                continue;
            }
            let immovable = self.bodies[v].role == Role::Hold
                || self.bodies[v].owner != self.bodies[u].owner
                || (self.bodies[v].role == Role::Idle && self.sc.frozen[v]);
            // A body that can give way may be pressed on a little, not squeezed.
            let leeway = if immovable { Fx::ZERO } else { COMPRESS };
            if !immovable && self.bodies[v].role == Role::Mover {
                continue;
            }
            let dmin = min_distance(&self.bodies[u], &self.bodies[v]) - SLOP - leeway;
            if (to - e.pos[v]).length_squared() < dmin.square() {
                return true;
            }
        }
        false
    }

    /// After the pushing: who is stuck, the sidesteps and the requests for a new
    /// route, and the idle units that have been left alone.
    fn bookkeeping(&mut self, e: &mut Entities, path: &mut Pathing) {
        let cfg = self.cfg;
        // The index of the final positions, for the free spots.
        self.sc.index.build(e, self.bodies);
        // When the last pass found no pair overlapping beyond the slack nothing has moved
        // since, so no mover is deeper than `MOVER_OVERLAP` and no pair than
        // `HARD_OVERLAP` (both are more than the slack): the two checks have nothing to find.
        let looked = !self.sc.passes_clear || self.sc.fresh;
        if looked && self.hold_back(e) {
            self.sc.index.build(e, self.bodies);
        }
        let rounds = if self.sc.fresh { SPAWN_ROUNDS } else { 1 };
        for _ in 0..rounds {
            if !looked || !self.separate(e) {
                break;
            }
            self.sc.index.build(e, self.bodies);
        }
        for k in 0..self.sc.movers.len() {
            let u = self.sc.movers[k] as usize;
            let c = &mut e.crowd[u];
            c.idle_hit = self.sc.idle_hit[u];
            let pos = e.pos[u];
            if c.walking {
                if let Some(target) = e.target[u] {
                    let d = (target - pos).length();
                    let tried = self.sc.step_len[u] > Fx::ZERO || c.refused;
                    if c.mark != Some(target) {
                        c.mark = Some(target);
                        c.best = d;
                        c.quiet = 0;
                        c.attempts = 0;
                    } else if !tried {
                        // Turning on the spot: neither progress nor blocked.
                    } else if d + PROGRESS_EPS <= c.best {
                        c.best = d;
                        c.quiet = 0;
                    } else if self.sc.in_line[u] {
                        // Waiting in line behind a unit that goes the same way: the one
                        // in front is the one that is stuck, if anybody is.
                    } else {
                        c.quiet = c.quiet.saturating_add(1);
                        if self.sc.idle_hit[u] {
                            c.pressed = c.pressed.saturating_add(1);
                        }
                    }
                }
            } else if c.stranded {
                c.quiet = c.quiet.saturating_add(1);
            }
            if c.quiet >= cfg.stuck_ticks {
                c.quiet = 0;
                // The watermark starts again from here; the waypoint is the same, so
                // the attempts are not forgotten.
                c.best = e.target[u].map_or(Fx::MAX, |t| (t - pos).length());
                let replans = path.replan_config();
                if c.attempts >= cfg.stuck_attempts && c.replans >= replans.max {
                    // The unit has asked for all the new routes it is allowed in this
                    // order and is stuck still: the order is given up on its next turn
                    // (WP4.7, `docs/rules/orders.md`).
                    c.attempts = 0;
                    c.give_up = true;
                    self.stats.gave_up += 1;
                } else if c.attempts >= cfg.stuck_attempts && c.cool == 0 {
                    c.attempts = 0;
                    c.replan = true;
                    c.replans += 1;
                    c.cool = replans.cooldown;
                    self.stats.replans += 1;
                    let class = self.bodies[u].class;
                    let overlay = self.overlay_for(e, path, u, class);
                    path.request_replan(e, u, class, overlay);
                } else {
                    // Another sidestep; also while the cooldown of the last replan
                    // runs, when the attempts stay at their limit so that the next
                    // stuck period asks.
                    let c = &mut e.crowd[u];
                    c.attempts = (c.attempts + 1).min(cfg.stuck_attempts);
                    self.stats.sidesteps += 1;
                    // The slot it was going to is walled off: it takes another one
                    // when it can (WP4.4b, `path/claim.rs`).
                    c.claim = Claim::default();
                    let spot = self.free_spot(e, u);
                    let c = &mut e.crowd[u];
                    c.side = spot.map(|s| (s, SIDESTEP_TICKS));
                }
            }
        }
        self.go_home(e);
        let closed_up = !self.sc.moves.is_empty();
        if closed_up {
            self.sc.index.build(e, self.bodies);
        }
        self.ease_apart(e);
        // Idle units that nothing has touched for a while get their allowance back.
        for i in 0..self.bodies.len() {
            if self.bodies[i].role == Role::Idle {
                let c = &mut e.crowd[i];
                if c.shoved > Fx::ZERO {
                    if self.sc.touched_flag[i] || self.sc.contact[i] {
                        c.calm = 0;
                    } else {
                        c.calm += 1;
                        if c.calm >= SHOVE_CALM {
                            c.shoved = Fx::ZERO;
                            c.calm = 0;
                        }
                    }
                }
            }
        }
    }

    /// The cells of the bodies near the stuck unit `u` that stand still (a unit holding
    /// position, an enemy with no order) that its request for a new route treats as
    /// blocked (WP4.7, `path/replan.rs`). Empty when none is near, or the unit has no
    /// destination.
    fn overlay_for(&self, e: &Entities, path: &Pathing, u: usize, class: u16) -> Overlay {
        let Some(dest) = orders::destination_of(e, u) else {
            return Overlay::EMPTY;
        };
        let me = self.bodies[u];
        let from = e.pos[u];
        let range2 = OVERLAY_RANGE.square();
        let blockers = (0..e.len().min(self.bodies.len()))
            .filter(|&v| v != u)
            .filter(|&v| {
                let b = &self.bodies[v];
                (b.role == Role::Hold || (b.role == Role::Idle && b.owner != me.owner))
                    && (e.pos[v] - from).length_squared() <= range2
            })
            .map(|v| (e.pos[v], self.bodies[v].radius));
        path.overlay_for(class, from, dest, me.radius, blockers)
    }

    /// Whether unit `a` may be moved by the hard separation for the sake of body `b`:
    /// not a unit holding position, and a unit with no order not by an enemy (the
    /// rules of "Who gives way" hold to the end).
    fn may_give_way(&self, a: usize, b: usize) -> bool {
        let (ba, bb) = (&self.bodies[a], &self.bodies[b]);
        match ba.role {
            Role::None | Role::Hold => false,
            Role::Idle => bb.role != Role::None && ba.owner == bb.owner,
            Role::Mover => bb.role != Role::None,
        }
    }

    /// The last resort for the overlap bound: two bodies of the units this tick has
    /// touched that still overlap by more than [`HARD_OVERLAP`] (the passes ran out, or
    /// the allowance of the units pressed together was used up) are pushed apart by
    /// the excess, a quarter of a cell at most for each, to where the terrain allows. A
    /// unit holding position is never moved: the other takes it all. Every unit is
    /// judged against the same positions and the moves are written after, so the order
    /// does not matter. True if any unit was moved.
    fn separate(&mut self, e: &mut Entities) -> bool {
        // The units the tick touched: movers, settling, pushed.
        self.sc.epoch = self.sc.epoch.wrapping_add(1).max(1);
        let epoch = self.sc.epoch;
        self.sc.active.clear();
        self.sc.dirty.clear();
        for k in 0..self.sc.movers.len() {
            let u = self.sc.movers[k];
            self.sc.active.push(u);
            self.sc.active_mark[u as usize] = epoch;
        }
        for list in [0, 1] {
            let len = if list == 0 {
                self.sc.settling.len()
            } else {
                self.sc.touched.len()
            };
            for k in 0..len {
                let u = if list == 0 {
                    self.sc.settling[k]
                } else {
                    self.sc.touched[k]
                };
                if self.sc.active_mark[u as usize] != epoch {
                    self.sc.active.push(u);
                    self.sc.active_mark[u as usize] = epoch;
                }
            }
        }
        // The sums use their own epoch.
        let sum = epoch.wrapping_add(1).max(1);
        self.sc.epoch = sum;
        // The pairs that are too deep, found without writing anything.
        let mut deep = std::mem::take(&mut self.sc.deep);
        deep.clear();
        for k in 0..self.sc.active.len() {
            let u = self.sc.active[k] as usize;
            if !self.sc.lapped[u] {
                continue;
            }
            let pu = e.pos[u];
            let (ru, su) = (self.bodies[u].radius, self.bodies[u].small);
            for row in self.sc.index.stretches(pu) {
                for n in row {
                    let v = n.id as usize;
                    if v == u {
                        continue;
                    }
                    let diff = pu - n.at;
                    let deep_at = distance_between(ru, su, n.radius, n.small) - HARD_OVERLAP;
                    // Almost every pair is clear of it: no square root for those.
                    if apart_on_an_axis(diff, deep_at) || diff.length_squared() >= deep_at.square()
                    {
                        continue;
                    }
                    if self.may_give_way(u, v) && deep.len() < DEEP_MAX {
                        deep.push((u as u32, v as u32));
                    }
                }
            }
        }
        for &(u, v) in &deep {
            let (u, v) = (u as usize, v as usize);
            let diff = e.pos[u] - e.pos[v];
            let dist = diff.length();
            if dist.is_zero() {
                continue;
            }
            let dmin = min_distance(&self.bodies[u], &self.bodies[v]);
            let excess = dmin - dist - HARD_OVERLAP + SLOP;
            // The other moves as well, when it is one of the units looked at and may
            // move.
            let shares = self.sc.active_mark[v] == epoch && self.may_give_way(v, u);
            let mine = if shares { excess.div_int(2) } else { excess };
            self.add(u, diff.with_length(mine), mine, sum);
        }
        self.sc.deep = deep;
        let dirty = std::mem::take(&mut self.sc.dirty);
        let mut moves = std::mem::take(&mut self.sc.moves);
        moves.clear();
        for &u in &dirty {
            let u = u as usize;
            let from = e.pos[u];
            let to = (from + self.sc.delta[u].clamp_length(SEPARATE_CAP))
                .clamp(self.bounds.0, self.bounds.1);
            if to != from && self.ground.may_step(self.bodies[u].class, from, to) {
                moves.push((u as u32, to, Fx::ZERO));
            }
        }
        for &(u, to, _) in &moves {
            e.pos[u as usize] = to;
        }
        let any = !moves.is_empty();
        self.sc.dirty = dirty;
        self.sc.moves = moves;
        any
    }

    /// The settled units that have been left alone for [`HOME_WAIT`] ticks close up
    /// toward the point their group was sent to: [`HOME_STEP`] a tick, straight at it
    /// or, when that is taken, a half turn to either side of straight (still nearer
    /// it), to a spot that overlaps no body beyond the slack. A unit that cannot move
    /// rests until something touches it or moves next to it. So the crowd that was
    /// spread by what pushed through it gathers again, whoever stands where; each step
    /// brings a unit nearer the point, so it always ends. Every unit is judged against
    /// the positions the tick ended with and the steps are written after, so the order
    /// does not matter.
    fn go_home(&mut self, e: &mut Entities) {
        let mut steps = std::mem::take(&mut self.sc.moves);
        steps.clear();
        for k in 0..self.sc.settling.len() {
            let u = self.sc.settling[k] as usize;
            if self.bodies[u].role != Role::Idle {
                continue;
            }
            let Some(home) = e.crowd[u].home else {
                continue;
            };
            if self.sc.touched_flag[u] || self.sc.contact[u] {
                e.crowd[u].home_wait = 0;
                e.crowd[u].home_rest = false;
                continue;
            }
            if e.crowd[u].home_rest {
                continue;
            }
            if e.crowd[u].home_wait < HOME_WAIT {
                e.crowd[u].home_wait += 1;
                continue;
            }
            let from = e.pos[u];
            let off = home - from;
            if off.length_squared() <= HOME_EPS.square() {
                continue;
            }
            let ahead = off.normalize_or_zero();
            let side = ahead.perp();
            // Straight, then turned away from it by more and more to one side and
            // the other (each still brings the unit nearer the point).
            let tries = [
                ahead,
                ahead + side.scale(HOME_SLIDE[0]),
                ahead - side.scale(HOME_SLIDE[0]),
                ahead + side.scale(HOME_SLIDE[1]),
                ahead - side.scale(HOME_SLIDE[1]),
                ahead + side.scale(HOME_SLIDE[2]),
                ahead - side.scale(HOME_SLIDE[2]),
                ahead + side.scale(HOME_SLIDE[3]),
                ahead - side.scale(HOME_SLIDE[3]),
            ];
            let mut moved = false;
            for dir in tries {
                let to = from + dir.with_length(HOME_STEP.min(off.length()));
                if to != to.clamp(self.bounds.0, self.bounds.1)
                    || !self.ground.may_step(self.bodies[u].class, from, to)
                {
                    continue;
                }
                let free = self.sc.index.near(to).all(|v| {
                    let v = v as usize;
                    if v == u {
                        return true;
                    }
                    let dmin = min_distance(&self.bodies[u], &self.bodies[v]) - SLOP.div_int(2);
                    let diff = to - e.pos[v];
                    apart_on_an_axis(diff, dmin) || diff.length_squared() >= dmin.square()
                });
                if free {
                    steps.push((u as u32, to, HOME_STEP));
                    moved = true;
                    break;
                }
            }
            if !moved {
                e.crowd[u].home_rest = true;
            }
        }
        // A unit that moved may have made room: the settled units next to its old and
        // new place try again.
        for &(u, to, _) in &steps {
            let u = u as usize;
            let was = e.pos[u];
            for at in [was, to] {
                for v in self.sc.index.near(at) {
                    let v = v as usize;
                    if v != u
                        && e.crowd[v].home_rest
                        && (e.pos[v] - was).length_squared() <= WAKE_RANGE.square()
                    {
                        e.crowd[v].home_rest = false;
                        e.crowd[v].home_wait = HOME_WAIT;
                    }
                }
            }
        }
        for &(u, to, _) in &steps {
            e.pos[u as usize] = to;
        }
        self.sc.moves = steps;
    }

    /// The deepest overlap of unit `u`, were it at `at`, with a body it may give way to
    /// (`may_give_way`), in cells, and the direction away from that body. Equal depths
    /// go to the body that comes first in the index, which does not depend on the order
    /// the units are visited in.
    fn deepest_to_give_way(&self, u: usize, at: Vec2) -> (Fx, Vec2) {
        let mut best = (Fx::ZERO, Vec2::ZERO);
        let (ru, su) = (self.bodies[u].radius, self.bodies[u].small);
        for row in self.sc.index.stretches(at) {
            for n in row {
                if n.id as usize == u {
                    continue;
                }
                let dmin = distance_between(ru, su, n.radius, n.small);
                let diff = at - n.at;
                if apart_on_an_axis(diff, dmin) || diff.length_squared() >= dmin.square() {
                    continue;
                }
                let depth = dmin - diff.length();
                if depth > best.0 && self.may_give_way(u, n.id as usize) {
                    best = (depth, diff);
                }
            }
        }
        best
    }

    /// The settled units that rest deeper than [`REST_OVERLAP`] in a body they may give way
    /// to step away from it, by half of what is over the slack (the other one, if it is a
    /// settled unit too, does the same: they part in one tick; if it is not, the next
    /// tick halves what is left, for as long as the unit is settling). A step is only taken
    /// when it leaves the unit less deep in every body than it was, and where the terrain
    /// allows. Every unit is judged against the positions the tick ended with and the
    /// steps are written after, so the order does not matter; nothing here pushes a unit
    /// that is being touched or overlapped by a mover, so it cannot start a fight with the
    /// passes, and a unit that is not settling is not looked at.
    fn ease_apart(&mut self, e: &mut Entities) {
        let mut steps = std::mem::take(&mut self.sc.moves);
        steps.clear();
        for k in 0..self.sc.settling.len() {
            let u = self.sc.settling[k] as usize;
            if self.bodies[u].role != Role::Idle || self.sc.touched_flag[u] || self.sc.contact[u] {
                continue;
            }
            let from = e.pos[u];
            let (depth, away) = self.deepest_to_give_way(u, from);
            if depth <= REST_OVERLAP || away.is_zero() {
                continue;
            }
            let step = (depth - SLOP.div_int(2)).div_int(2);
            let to = (from + away.with_length(step)).clamp(self.bounds.0, self.bounds.1);
            if to == from
                || !self.ground.may_step(self.bodies[u].class, from, to)
                || self.deepest(u, to) >= depth
            {
                continue;
            }
            steps.push((u as u32, to, step));
        }
        for &(u, to, _) in &steps {
            e.pos[u as usize] = to;
        }
        self.sc.moves = steps;
    }

    /// The deepest overlap of unit `u`, were it at `at`, with any other body, in
    /// cells (zero when it overlaps none).
    fn deepest(&self, u: usize, at: Vec2) -> Fx {
        let mut deepest = Fx::ZERO;
        let (ru, su) = (self.bodies[u].radius, self.bodies[u].small);
        for row in self.sc.index.stretches(at) {
            for n in row {
                if n.id as usize == u {
                    continue;
                }
                let dmin = distance_between(ru, su, n.radius, n.small);
                let diff = at - n.at;
                if !apart_on_an_axis(diff, dmin) && diff.length_squared() < dmin.square() {
                    deepest = deepest.max(dmin - diff.length());
                }
            }
        }
        deepest
    }

    /// Sends back the movers that the pushes left deeper in another body than
    /// [`MOVER_OVERLAP`]: to where they began the tick, when that is less deep.
    /// Every mover is judged against the same positions and the moves are written
    /// after, so the order of the movers does not matter. True if any went back.
    fn hold_back(&mut self, e: &mut Entities) -> bool {
        let mut back = std::mem::take(&mut self.sc.moves);
        back.clear();
        for k in 0..self.sc.movers.len() {
            let u = self.sc.movers[k] as usize;
            let before = e.prev_pos[u];
            if before == e.pos[u] || !self.sc.lapped[u] {
                continue;
            }
            let here = self.deepest(u, e.pos[u]);
            if here > MOVER_OVERLAP && self.deepest(u, before) < here {
                back.push((u as u32, before, here));
            }
        }
        let any = !back.is_empty();
        for &(u, before, _) in &back {
            e.pos[u as usize] = before;
        }
        self.sc.moves = back;
        any
    }

    /// A free spot next to unit `u`: the middle of a neighbouring cell its class may
    /// enter, with no other body in it. Tried right of its heading first (left on
    /// every other attempt), then the rest in a fixed order.
    fn free_spot(&self, e: &Entities, u: usize) -> Option<Vec2> {
        let body = self.bodies[u];
        let pos = e.pos[u];
        let toward = e.target[u].map_or(Vec2::ZERO, |t| (t - pos).normalize_or_zero());
        let toward = if toward.is_zero() {
            Vec2::new(Fx::ONE, Fx::ZERO)
        } else {
            toward
        };
        let right = if e.crowd[u].attempts & 1 == 1 {
            toward.perp()
        } else {
            -toward.perp()
        };
        // right, left, forward-right, forward-left, back-right, back-left, forward, back.
        let wanted = [
            right,
            -right,
            toward + right,
            toward - right,
            -toward + right,
            -toward - right,
            toward,
            -toward,
        ];
        let (cx, cy) = pos.cell();
        for dir in wanted {
            let probe = pos + dir.with_length(Fx::ONE);
            let (px, py) = probe.cell();
            let (dx, dy) = ((px - cx).clamp(-1, 1), (py - cy).clamp(-1, 1));
            if dx == 0 && dy == 0 {
                continue;
            }
            let (nx, ny) = (cx + dx, cy + dy);
            if nx < 0 || ny < 0 {
                continue;
            }
            let spot = Vec2::new(Fx::from_int(nx) + Fx::HALF, Fx::from_int(ny) + Fx::HALF);
            if !self.ground.may_step(body.class, pos, spot)
                || spot != spot.clamp(self.bounds.0, self.bounds.1)
            {
                continue;
            }
            let free = self.sc.index.near(spot).all(|v| {
                let v = v as usize;
                if v == u {
                    return true;
                }
                let dmin = min_distance(&body, &self.bodies[v]) - SLOP;
                (spot - e.pos[v]).length_squared() >= dmin.square()
            });
            if free {
                return Some(spot);
            }
        }
        None
    }
}

/// Writes the crowd section of the checksum: the settings, then every live unit
/// that remembers something, in slot order.
pub(crate) fn write_hash(h: &mut Fnv1a64, config: &CollisionConfig, e: &Entities) {
    h.write_u8(u8::from(config.enabled));
    h.write_u32(u32::from(config.wait_ticks));
    h.write_u32(u32::from(config.stuck_ticks));
    h.write_u32(u32::from(config.stuck_attempts));
    let remembering = || (0..e.len()).filter(|&i| e.alive[i] && !e.crowd[i].at_rest());
    h.write_u64(remembering().count() as u64);
    for i in remembering() {
        h.write_u32(i as u32);
        e.crowd[i].write_hash(h);
    }
}

/// Whether the checksum has a crowd section: some live unit remembers something.
pub(crate) fn any_state(e: &Entities) -> bool {
    (0..e.len()).any(|i| e.alive[i] && !e.crowd[i].at_rest())
}

#[cfg(test)]
mod tests;
