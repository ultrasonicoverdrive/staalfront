//! Formations (WP4.6): a group that moves together keeps its shape, at the pace of its
//! slowest member.
//!
//! One [`Order::Move`](crate::Order::Move) that sends at least
//! [`FormationConfig::min_units`] units that cross the same ground to one point makes
//! them a **formation**. The rules and the reasons are in `docs/rules/formations.md`;
//! this is the code.
//!
//! # The shape
//!
//! When the order is given the group's *current* layout is turned into a lattice
//! ([`layout`]): the unit nearest the middle of the group becomes the **leader**, the
//! others get an **offset** from it in a frame that points the way the group is going
//! (forward, and to the right of that), kept as the group stands (a line stays a line, a
//! block a block) but compressed and snapped:
//!
//! * the lattice is as wide as the widest body of the group needs, times
//!   [`FormationConfig::spacing_permille`] (`path/slots.rs`, the same spacing the arrival
//!   slots are made with, so that bodies in their places never overlap);
//! * a scattered group is pulled in: the offsets are scaled so that no unit is farther
//!   from the leader than [`FormationConfig::spread_permille`] times the radius of a disc
//!   that holds the bodies;
//! * every unit takes the lattice point nearest to its own spot, nearest-to-the-middle first,
//!   and the next free one when that is taken.
//!
//! The offsets rotate with the **heading** of the formation, which follows the way the leader
//! goes, one facing step at a time and no faster than the outermost slot can be followed at
//! the pace of the formation. The **slot** of a unit is the leader's position plus its offset
//! turned by the heading.
//!
//! # The pace
//!
//! The leader follows the order's route as any unit does (a flowfield or a route of its own,
//! `path/`), but never faster than [`FormationConfig::pace_permille`] of the speed of the
//! slowest member *as it stands now* (terrain included). The others walk to their slots as
//! fast as [`FormationConfig::catchup_permille`] of their own speed allows: a unit that falls
//! behind catches up, one that is ahead of its slot waits on it, because its slot comes to it
//! at the pace of the formation.
//!
//! # Narrow places and re-forming
//!
//! A member that cannot see its slot (a thick line of sight, `path/los.rs`: a wall, a gap, the
//! river) follows the flowfield or route of the order as it always did and is not held back,
//! except that a unit that is ahead of its slot by more than [`FormationConfig::reform_tenths`]
//! waits where it stands if the ground round it is open. When the leader is on open ground
//! and some member is farther from its slot than that, the **leader waits** for it, for at most
//! [`FormationConfig::patience_ticks`] ticks in all; then the formation is *broken*: nobody
//! waits any more until the stragglers are back (at most a tenth of the group away). A member
//! that has been away from its slot for twice the patience leaves the formation and goes on
//! as a unit of its own. So a bridge or a gap breaks the formation up, the units file through,
//! and it forms up again on the other side; and nothing can wait for ever.
//!
//! # The end
//!
//! A formation ends when its leader comes within the extent of the formation and two cells of
//! the goal: the members are released and go on as every unit of a group order does, to the
//! arrival slots of the goal's lattice, which is turned across the direction the group came
//! in (`path/slots.rs`, [`Layout::turn`](crate::path)), claimed on approach (`path/claim.rs`)
//! so that a slot that is blocked is simply not taken. Any other order, a death or the end of
//! a unit's order takes it out of the formation; with fewer than two members left the formation
//! is gone. A leader that goes picks a new one.
//!
//! # State and checksum
//!
//! The formations and the members' offsets are in the checksum (section `F`, written only
//! while a formation exists). What a tick works out for each member (its slot, what it does)
//! is derived from that state and not hashed.

use std::collections::{BTreeMap, BTreeSet};

use sf_math::hash::Fnv1a64;
use sf_math::{Facing, Fx, Vec2};

use crate::behavior::{Motion, Walk, walk};
use crate::entities::{Entities, NO_DEF};
use crate::movement::{Ground, scale_step};
use crate::path::{Pathing, RouteState};
use crate::turning;
use crate::units::UnitTable;

/// The most units one formation holds (the arrival slots of a field are as many).
pub const MAX_MEMBERS: usize = 256;

/// The most formations a world keeps at once; an order beyond that moves its group as a
/// group order always did.
pub const MAX_FORMATIONS: usize = 64;

/// How far inside the extent of the formation the leader is from the goal when the
/// formation is let go, in cells.
const RELEASE_MARGIN: Fx = Fx::from_int(2);

/// How much more than the two radii a member may be from the one in front of it and be
/// "in its way", in cells.
const BLOCK_MARGIN: Fx = Fx::from_ratio(1, 2);

/// How often, in ticks, the slots are handed out again while some member is stalled.
const REASSIGN_EVERY: u32 = 100;

/// The ticks in a window in which a straggler has to come closer to its slot, and how much
/// closer, in cells: a unit that does not is stalled, and the leader's patience runs out
/// while it is. Counted over a window and not tick by tick so that a unit jostled by a
/// crowd, which gains and loses a little all the time, does not look as if it were on its way.
const WINDOW: u32 = 20;
const PROGRESS: Fx = Fx::from_ratio(1, 32);

/// While every straggler is coming closer the leader's patience runs down one tick in this
/// many, so that it waits that many times as long as for stragglers that do not.
const CLOSING_SLOWDOWN: u32 = 3;

/// A member this near its slot (in cells) is on it; it does not have to come closer.
const SETTLE: Fx = Fx::from_ratio(1, 2);

/// For how many ticks a formation stays loose after the ground round its leader was last
/// not open: the leader does not stand about at the mouth of a gap it has just left.
const LOOSE_TICKS: u8 = 20;

/// The angle of one facing step in radians, in thousandths (`2 pi / 256`): what the
/// slowest turn of the heading is worked out with.
const RADIANS_PER_STEP_MILLI: i64 = 25;

/// The longest, in ticks, between two steps of the heading.
const MAX_TURN_PERIOD: i64 = 40;

/// The fewest units a formation may be made of (a pair; `sf_data::sim_settings` checks the same).
pub const MIN_UNITS_LIMIT: u16 = 2;

/// The shipped numbers: see [`FormationConfig`] for what each does.
pub const DEFAULT_MIN_UNITS: u16 = 3;
/// See [`FormationConfig::max_units`].
pub const DEFAULT_MAX_UNITS: u16 = 48;
/// See [`FormationConfig::spacing_permille`].
pub const DEFAULT_SPACING_PERMILLE: u16 = 1250;
/// See [`FormationConfig::spread_permille`].
pub const DEFAULT_SPREAD_PERMILLE: u16 = 1500;
/// See [`FormationConfig::pace_permille`].
pub const DEFAULT_PACE_PERMILLE: u16 = 900;
/// See [`FormationConfig::catchup_permille`].
pub const DEFAULT_CATCHUP_PERMILLE: u16 = 1000;
/// See [`FormationConfig::reform_tenths`].
pub const DEFAULT_REFORM_TENTHS: u16 = 30;
/// See [`FormationConfig::patience_ticks`].
pub const DEFAULT_PATIENCE_TICKS: u16 = 300;

/// The formation settings of a world: game data (`assets/config/sim.ron`); a world that is
/// given none uses [`FormationConfig::DEFAULT`], the same numbers.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct FormationConfig {
    /// False turns formations off: a group order moves its units as a group order did before
    /// WP4.6 (flowfields and arrival slots, nothing else).
    pub enabled: bool,
    /// How many units that cross the same ground one move order must send to one place to
    /// form a formation. At least 2, at most [`MAX_MEMBERS`].
    pub min_units: u16,
    /// The most units a formation holds: a bigger group (a crowd of two hundred) is not a
    /// formation but a mob, which a lattice that wide cannot keep, and it moves as a group
    /// order does. At least `min_units`, at most [`MAX_MEMBERS`].
    pub max_units: u16,
    /// The spacing of the lattice, as thousandths of what the widest body of the group needs
    /// (1000 puts bodies side by side). 1000 to 3000.
    pub spacing_permille: u16,
    /// The most a formation may spread, as thousandths of the radius of a disc that holds the
    /// bodies of the group: a scattered group is pulled in to this. 1000 to 4000.
    pub spread_permille: u16,
    /// The pace of the formation, as thousandths of the current speed of its slowest member
    /// (the part it keeps back so that the slowest can keep up). 500 to 1000.
    pub pace_permille: u16,
    /// The most speed a member may use to catch up with its slot, as thousandths of its own
    /// speed. Not less than `pace_permille`, at most 1000.
    pub catchup_permille: u16,
    /// How far from its slot a member may be and still count as in formation, in tenths of a
    /// cell: farther and the leader waits for it (on open ground), and a unit ahead of its slot
    /// by more than this waits. 5 to 100.
    pub reform_tenths: u16,
    /// For how long, in ticks, the leader may wait for stragglers before the formation counts
    /// as broken; a member away from its slot for twice that leaves. 20 to 2000.
    pub patience_ticks: u16,
}

impl FormationConfig {
    /// The numbers of the shipped `sim.ron`.
    pub const DEFAULT: FormationConfig = FormationConfig {
        enabled: true,
        min_units: DEFAULT_MIN_UNITS,
        max_units: DEFAULT_MAX_UNITS,
        spacing_permille: DEFAULT_SPACING_PERMILLE,
        spread_permille: DEFAULT_SPREAD_PERMILLE,
        pace_permille: DEFAULT_PACE_PERMILLE,
        catchup_permille: DEFAULT_CATCHUP_PERMILLE,
        reform_tenths: DEFAULT_REFORM_TENTHS,
        patience_ticks: DEFAULT_PATIENCE_TICKS,
    };

    /// The shipped numbers with formations turned off: a world where group orders move
    /// as they did before WP4.6.
    pub const OFF: FormationConfig = FormationConfig {
        enabled: false,
        ..FormationConfig::DEFAULT
    };

    /// Whether a world takes these numbers (the ranges of the fields).
    pub const fn is_valid(&self) -> bool {
        self.min_units >= MIN_UNITS_LIMIT
            && self.max_units >= self.min_units
            && self.max_units as usize <= MAX_MEMBERS
            && self.spacing_permille >= 1000
            && self.spacing_permille <= 3000
            && self.spread_permille >= 1000
            && self.spread_permille <= 4000
            && self.pace_permille >= 500
            && self.pace_permille <= 1000
            && self.catchup_permille >= self.pace_permille
            && self.catchup_permille <= 1000
            && self.reform_tenths >= 5
            && self.reform_tenths <= 100
            && self.patience_ticks >= 20
            && self.patience_ticks <= 2000
    }

    fn reform(&self) -> Fx {
        Fx::from_ratio(i64::from(self.reform_tenths), 10)
    }
}

impl Default for FormationConfig {
    fn default() -> FormationConfig {
        FormationConfig::DEFAULT
    }
}

/// What the formations did so far; for tests and the debug panel. Not part of the checksum.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct FormationStats {
    /// Formations made.
    pub formed: u64,
    /// Formations that ended because their leader came to the goal.
    pub released: u64,
    /// Formations that ended for want of members.
    pub dissolved: u64,
    /// Members that left because they were away from their slot for too long.
    pub expelled: u64,
    /// Ticks the leader of a formation waited for stragglers (summed over formations).
    pub waits: u64,
    /// Times a formation was broken: its patience ran out.
    pub breaks: u64,
    /// Times a broken formation was whole again.
    pub reforms: u64,
}

/// What a member does this tick.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum MemberMode {
    /// Not in a formation, or nothing worked out yet.
    #[default]
    None,
    /// The leader: follows the order's route at the pace of the formation.
    Lead,
    /// Sees its slot and walks straight to it.
    Direct,
    /// Cannot see its slot: follows the order's flowfield or route as a unit of a group does.
    Flow,
    /// Stands: the leader waiting for stragglers, or a unit ahead of its slot waiting for it.
    Wait,
}

/// What a unit remembers about its formation: a column of the entity store. The first
/// three fields are state (hashed); the rest is worked out afresh every tick by
/// [`Formations::update`].
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct Member {
    /// The formation, zero for none.
    pub serial: u32,
    /// Where the unit stands in the lattice, from the middle of the layout the formation was
    /// made from: forward and to the right of the heading when it was made.
    pub off: Vec2,
    /// Ticks the unit has been farther from its slot than the re-form distance without coming
    /// closer (counted in whole windows of [`WINDOW`] ticks): zero while it closes in.
    pub lag: u16,
    /// The nearest to its slot the unit has come since it fell off it (most when on it).
    pub far: Fx,
    // ---- worked out each tick ----
    pub mode: MemberMode,
    /// The world position of the slot.
    pub slot: Vec2,
    /// The step the unit may take this tick before the ground scales it: the pace of the
    /// formation for the leader, the catch-up speed for the others.
    pub step: Fx,
    /// The heading of the formation, for a unit that stands in its slot.
    pub heading: Facing,
}

impl Member {
    /// A unit that is in no formation.
    pub(crate) const NONE: Member = Member {
        serial: 0,
        off: Vec2::ZERO,
        lag: 0,
        far: Fx::MAX,
        mode: MemberMode::None,
        slot: Vec2::ZERO,
        step: Fx::ZERO,
        heading: Facing::PLUS_X,
    };
}

/// Where a formation is in its life.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum FormationState {
    /// Moving as one: the leader waits for stragglers (on open ground, for as long as its
    /// patience lasts).
    Whole,
    /// Its patience ran out: the leader goes on whoever is behind, until they are back.
    Broken,
}

/// One formation.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Formation {
    pub serial: u32,
    /// The point the order sends the group to.
    pub goal: Vec2,
    /// The slot index of the leader.
    pub leader: u32,
    pub heading: Facing,
    /// Ticks until the heading may turn one step again.
    pub turn_wait: u8,
    pub state: FormationState,
    /// Ticks of waiting the leader has left.
    pub patience: u16,
    /// The leader is waiting this tick.
    pub waiting: bool,
    /// Ticks since the formation was made: the windows of the progress of the stragglers.
    pub age: u32,
    /// Ticks the formation stays loose: the ground round the leader was not open that long ago.
    pub loose: u8,
    /// The members' slot indexes, ascending.
    pub members: Vec<u32>,
    // ---- worked out each tick ----
    /// The farthest a member's offset lies from the leader's.
    pub extent: Fx,
    /// The pace of the formation this tick, a step after the ground has scaled it.
    pub pace: Fx,
}

/// A read-only view of the formation a unit is in, for tests and the presentation.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct FormationView {
    /// The formation's number.
    pub serial: u32,
    /// The unit is the leader.
    pub leader: bool,
    /// The leader's entity.
    pub leader_id: crate::ids::EntityId,
    /// Members now.
    pub members: usize,
    /// What the unit does this tick.
    pub mode: MemberMode,
    /// Where its slot is.
    pub slot: Vec2,
    /// Its offset in the lattice, forward and to the right of the formation's first heading.
    pub offset: Vec2,
    /// The heading of the formation.
    pub heading: Facing,
    /// Whether the formation is whole or broken.
    pub state: FormationState,
    /// The leader is waiting for stragglers.
    pub waiting: bool,
    /// The formation is loose: the ground round the leader is not open, everybody follows the
    /// order's way.
    pub loose: bool,
    /// The farthest a member lies from the leader in the lattice, in cells.
    pub extent: Fx,
    /// The pace of the formation this tick, in cells per tick.
    pub pace: Fx,
    /// Ticks the unit has been away from its slot without coming closer.
    pub lag: u16,
}

/// All the formations of a world.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Formations {
    pub config: FormationConfig,
    list: Vec<Formation>,
    next_serial: u32,
    pub stats: FormationStats,
}

/// The eight ways a leader (or a unit that would wait) looks at to see whether the ground
/// round it is open, and how far each is looked at, in cells.
const PROBES: [Vec2; 8] = [
    Vec2::from_ints(3, 0),
    Vec2::from_ints(-3, 0),
    Vec2::from_ints(0, 3),
    Vec2::from_ints(0, -3),
    Vec2::from_ints(2, 2),
    Vec2::from_ints(-2, 2),
    Vec2::from_ints(2, -2),
    Vec2::from_ints(-2, -2),
];

/// Whether a unit of `class` at `at` has open ground round it: no rock, water or structure
/// within three cells in any of eight directions. A bridge, a gap in a wall and a funnel
/// are not: the leader does not stand and wait in them, where it would hold the others up.
fn open_ground(path: &Pathing, class: u16, at: Vec2) -> bool {
    PROBES.iter().all(|&d| path.sight_clear(class, at, at + d))
}

/// The inverse of a facing: the one that turns back what `f` turned.
fn inverse(f: Facing) -> Facing {
    Facing::new(0_u8.wrapping_sub(f.raw()))
}

impl Formations {
    pub(crate) fn new() -> Formations {
        Formations {
            config: FormationConfig::DEFAULT,
            list: Vec::new(),
            next_serial: 1,
            stats: FormationStats::default(),
        }
    }

    /// Whether the world has a formation (the checksum has a section then).
    pub(crate) fn any(&self) -> bool {
        !self.list.is_empty()
    }

    /// The formation `serial`.
    fn get(&self, serial: u32) -> Option<&Formation> {
        self.list.iter().find(|f| f.serial == serial)
    }

    /// A view of the formation unit `i` is in.
    pub(crate) fn view(&self, e: &Entities, i: usize) -> Option<FormationView> {
        let m = e.form[i];
        let f = self.get(m.serial).filter(|_| m.serial != 0)?;
        let leader = f.leader as usize;
        Some(FormationView {
            serial: f.serial,
            leader: leader == i,
            leader_id: crate::ids::EntityId::new(f.leader, e.generation[leader]),
            members: f.members.len(),
            mode: m.mode,
            slot: m.slot,
            offset: m.off,
            heading: f.heading,
            state: f.state,
            waiting: f.waiting,
            loose: f.loose > 0,
            extent: f.extent,
            pace: f.pace,
            lag: m.lag,
        })
    }

    // ---- making formations ----------------------------------------------------------

    /// One `Order::Move` to `goal` was just given to the units in `started` (ascending
    /// slots, each once, each starting the order now). The ones that cross the same ground
    /// and are at least [`FormationConfig::min_units`] (and at most `max_units`) form a
    /// formation each. Returns the units that are in one with the heading of their formation,
    /// ascending by slot: what the arrival slots of the goal are turned by.
    pub(crate) fn create(
        &mut self,
        e: &mut Entities,
        units: &UnitTable,
        ground: &Ground<'_>,
        path: &Pathing,
        started: &[usize],
        goal: Vec2,
    ) -> Vec<(usize, Facing)> {
        let cfg = self.config;
        let mut formed: Vec<(usize, Facing)> = Vec::new();
        if !cfg.enabled || !path.is_active() {
            return formed;
        }
        let Some(goal_cell) = ground.cell_at(goal) else {
            return formed;
        };
        // The units that can form up, by the ground they cross.
        let mut families: BTreeMap<[u64; 4], Vec<usize>> = BTreeMap::new();
        for &i in started {
            if !e.alive[i] || e.def[i] == NO_DEF {
                continue;
            }
            let Some(class) = units.at(e.def[i] as usize).movement_class else {
                continue;
            };
            let Some(family) = ground.movement.terrain_family(class) else {
                continue;
            };
            if !path.paths_class(class) || !ground.cost(class, goal_cell).is_passable() {
                continue;
            }
            families.entry(family).or_default().push(i);
        }
        for (_, members) in families {
            if members.len() < usize::from(cfg.min_units)
                || members.len() > usize::from(cfg.max_units)
                || self.list.len() >= MAX_FORMATIONS
            {
                continue;
            }
            let serial = self.next_serial;
            self.next_serial = self.next_serial.checked_add(1).unwrap_or(1);
            let lattice = layout(e, units, &members, goal, &cfg);
            for (k, &i) in members.iter().enumerate() {
                e.form[i] = Member {
                    serial,
                    off: lattice.offsets[k],
                    ..Member::NONE
                };
            }
            self.list.push(Formation {
                serial,
                goal,
                leader: members[lattice.leader] as u32,
                heading: lattice.heading,
                turn_wait: 0,
                state: FormationState::Whole,
                patience: cfg.patience_ticks,
                waiting: false,
                age: 0,
                loose: 0,
                members: members.iter().map(|&i| i as u32).collect(),
                extent: Fx::ZERO,
                pace: Fx::ZERO,
            });
            self.stats.formed += 1;
            formed.extend(members.iter().map(|&i| (i, lattice.heading)));
        }
        formed.sort_unstable_by_key(|&(i, _)| i);
        formed
    }

    // ---- the tick -------------------------------------------------------------------

    /// Works out, for every member of every formation, what it does this tick. Runs after
    /// the path service has served and before the units take their steps.
    pub(crate) fn update(
        &mut self,
        e: &mut Entities,
        units: &UnitTable,
        ground: &Ground<'_>,
        path: &mut Pathing,
    ) {
        let cfg = self.config;
        let mut k = 0;
        while k < self.list.len() {
            let mut f = std::mem::replace(&mut self.list[k], Formation::EMPTY);
            let verdict = update_one(&mut f, e, units, ground, path, &cfg, &mut self.stats);
            match verdict {
                Verdict::Keep => {
                    self.list[k] = f;
                    k += 1;
                }
                Verdict::Released | Verdict::Dissolved => {
                    for &m in &f.members {
                        let m = m as usize;
                        if e.form[m].serial == f.serial {
                            let was = e.form[m].mode;
                            e.form[m] = Member::NONE;
                            if matches!(was, MemberMode::Direct | MemberMode::Wait) {
                                forget_way(e, m, units, path, f.goal);
                            }
                        }
                    }
                    if verdict == Verdict::Released {
                        self.stats.released += 1;
                    } else {
                        self.stats.dissolved += 1;
                    }
                    self.list.remove(k);
                }
            }
        }
    }

    // ---- the checksum ---------------------------------------------------------------

    /// Writes the formation section of the checksum.
    pub(crate) fn write_hash(&self, h: &mut Fnv1a64, e: &Entities) {
        h.write_u32(self.next_serial);
        h.write_u32(self.list.len() as u32);
        for f in &self.list {
            h.write_u32(f.serial);
            h.write_vec2(f.goal);
            h.write_u32(f.leader);
            h.write_u8(f.heading.raw());
            h.write_u8(f.turn_wait);
            h.write_u8(match f.state {
                FormationState::Whole => 0,
                FormationState::Broken => 1,
            });
            h.write_u32(u32::from(f.patience));
            h.write_u8(f.loose);
            h.write_u32(f.age);
            h.write_u32(f.members.len() as u32);
            for &m in &f.members {
                let member = e.form[m as usize];
                h.write_u32(m);
                h.write_vec2(member.off);
                h.write_u32(u32::from(member.lag));
                h.write_fx(member.far);
                // What the member did last tick: a unit that goes back to the order's own way
                // finds its route again only if it was walking to a slot before.
                h.write_u8(member.mode as u8);
            }
        }
    }
}

impl Formation {
    /// A placeholder while a formation is being updated.
    const EMPTY: Formation = Formation {
        serial: 0,
        goal: Vec2::ZERO,
        leader: 0,
        heading: Facing::PLUS_X,
        turn_wait: 0,
        state: FormationState::Whole,
        patience: 0,
        waiting: false,
        age: 0,
        loose: 0,
        members: Vec::new(),
        extent: Fx::ZERO,
        pace: Fx::ZERO,
    };
}

/// What became of a formation in a tick.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Verdict {
    Keep,
    /// The leader is at the goal.
    Released,
    /// Fewer than two members are left.
    Dissolved,
}

/// One tick of one formation.
fn update_one(
    f: &mut Formation,
    e: &mut Entities,
    units: &UnitTable,
    ground: &Ground<'_>,
    path: &mut Pathing,
    cfg: &FormationConfig,
    stats: &mut FormationStats,
) -> Verdict {
    // Who is still in it: alive and in this formation (any other order takes a unit out,
    // `orders::begin` and `clear`).
    f.members
        .retain(|&m| e.alive[m as usize] && e.form[m as usize].serial == f.serial);
    if f.members.len() < 2 {
        return Verdict::Dissolved;
    }
    // A leader that is gone is replaced by the member nearest the middle of those left.
    if f.members.binary_search(&f.leader).is_err() {
        f.leader = nearest_to_middle(e, &f.members);
        f.waiting = false;
    }
    let leader = f.leader as usize;
    let lead_at = e.pos[leader];
    let lead_off = e.form[leader].off;

    // The extent of the lattice, and the pace: the slowest member as it stands now.
    let mut extent2 = Fx::ZERO;
    let mut slowest: Option<Fx> = None;
    for &m in &f.members {
        let i = m as usize;
        extent2 = extent2.max((e.form[i].off - lead_off).length_squared());
        let step = scale_step(
            units.at(e.def[i] as usize).speed_per_tick,
            ground.permille_at(class_of(e, units, i), e.pos[i]),
        );
        slowest = Some(slowest.map_or(step, |s| s.min(step)));
    }
    f.extent = extent2.sqrt();
    let pace = slowest
        .unwrap_or(Fx::ZERO)
        .mul_int(i64::from(cfg.pace_permille))
        .div_int(1000)
        .max(Fx::from_raw(1));
    f.pace = pace;

    // The end: the leader is within the extent and a margin of the goal, or within the disc
    // the whole order's bodies fill round it (the units of other families that were sent
    // with the formation stand there, and its slots would lie among them); or its goal is
    // out of reach and it has come to the end of the way.
    let stranded = e.route[leader].state == RouteState::Stranded && e.route[leader].unreachable;
    let ends = f.extent.max(e.crowd[leader].disc) + RELEASE_MARGIN;
    if stranded || (f.goal - lead_at).length() <= ends {
        return Verdict::Released;
    }

    // The heading follows where the leader goes, a step at a time, no faster than the
    // outermost slot can be followed at the pace of the formation.
    f.turn_wait = f.turn_wait.saturating_sub(1);
    let moved = e.pos[leader] - e.prev_pos[leader];
    if f.turn_wait == 0
        && moved.length_squared() >= pace.div_int(4).square()
        && let Some(want) = moved.facing()
        && f.heading != want
    {
        f.heading = f.heading.turn_toward(want, 1);
        let period = f
            .extent
            .mul_int(RADIANS_PER_STEP_MILLI)
            .div_int(1000)
            .saturating_div(pace)
            .ceil_int()
            .clamp(1, MAX_TURN_PERIOD);
        f.turn_wait = (period - 1) as u8;
    }

    // Is the ground round the leader open? Near a wall, the water or a gap the formation
    // lets go for a while: everybody follows the order's way at full speed, nobody waits
    // for anybody, and the units file through. It forms up again on open ground.
    let open = open_ground(path, class_of(e, units, leader), lead_at);
    let was_loose = f.loose > 0;
    f.loose = if open {
        f.loose.saturating_sub(1)
    } else {
        LOOSE_TICKS
    };
    let loose = f.loose > 0;
    // The units came through a gap in another order than they went in, and two that must
    // pass each other to reach their slots would stand in each other's way: when the
    // formation forms up again, and from time to time while some member does not get to
    // its slot, the slots go to the units nearest them.
    let stalled = f.members.iter().any(|&m| e.form[m as usize].lag > 0);
    if !loose && ((was_loose && f.age > 0) || (stalled && f.age.is_multiple_of(REASSIGN_EVERY))) {
        reassign(f, e, lead_at, lead_off);
    }

    // A member that does not get to its slot because another stands in the way, on its own
    // slot or on its way to it, swaps places with that one: the pair walks on without having
    // to pass each other.
    if !loose && f.age.is_multiple_of(WINDOW) {
        swap_blocked(f, e, units, lead_at, lead_off);
    }

    // Every member: its slot, and how far it is from it.
    let reform = cfg.reform();
    let mut lagging = 0_usize;
    let mut stalled_far = false;
    let mut expelled: Vec<u32> = Vec::new();
    f.age = f.age.wrapping_add(1);
    for &m in &f.members {
        let i = m as usize;
        let mut member = e.form[i];
        let was = member.mode;
        member.heading = f.heading;
        member.slot = lead_at + (member.off - lead_off).rotate(f.heading);
        let own = units.at(e.def[i] as usize).speed_per_tick;
        if i == leader {
            member.mode = MemberMode::Lead;
            member.lag = 0;
            if matches!(was, MemberMode::Direct | MemberMode::Wait) {
                forget_way(e, i, units, path, f.goal);
            }
            // At the pace of the formation, unless the formation is loose.
            let permille = ground.permille_at(class_of(e, units, i), e.pos[i]).max(1);
            member.step = if loose {
                own
            } else {
                pace.mul_int(1000)
                    .div_int(i64::from(permille))
                    .min(own)
                    .max(Fx::from_raw(1))
            };
            e.form[i] = member;
            continue;
        }
        let class = class_of(e, units, i);
        let lag = (member.slot - e.pos[i]).length();
        member.step = own.mul_int(i64::from(cfg.catchup_permille)).div_int(1000);
        member.mode = if loose || !path.sight_clear(class, e.pos[i], member.slot) {
            MemberMode::Flow
        } else {
            MemberMode::Direct
        };
        // A unit that cannot see its slot and is ahead of it waits for it on open ground:
        // the slot comes to it at the pace of the formation. It is not a straggler.
        let ahead = member.mode == MemberMode::Flow
            && !loose
            && (f.goal - e.pos[i]).length() + reform < (f.goal - member.slot).length()
            && open_ground(path, class, e.pos[i]);
        if ahead {
            member.mode = MemberMode::Wait;
            member.lag = 0;
        } else if loose || lag <= SETTLE {
            member.lag = 0;
            member.far = Fx::MAX;
        } else {
            // Off its slot by more than a body: it has to come closer by a little in every
            // window, or it is stalled.
            if lag > reform {
                lagging += 1;
            }
            if f.age.is_multiple_of(WINDOW) {
                if lag + PROGRESS < member.far {
                    member.lag = 0;
                    member.far = lag;
                } else {
                    member.lag = member.lag.saturating_add(WINDOW as u16);
                }
            }
            stalled_far |= lag > reform && member.lag > 0;
        }
        if lag > reform && member.lag > cfg.patience_ticks {
            expelled.push(m);
            continue;
        }
        // A unit that goes back to following the order's way has to find it again from
        // where it stands: the waypoints it has are from where it was when it left them.
        if member.mode == MemberMode::Flow && matches!(was, MemberMode::Direct | MemberMode::Wait) {
            forget_way(e, i, units, path, f.goal);
        }
        e.form[i] = member;
    }
    for m in expelled {
        if matches!(
            e.form[m as usize].mode,
            MemberMode::Direct | MemberMode::Wait
        ) {
            forget_way(e, m as usize, units, path, f.goal);
        }
        e.form[m as usize] = Member::NONE;
        f.members.retain(|&x| x != m);
        stats.expelled += 1;
    }
    if f.members.len() < 2 {
        return Verdict::Dissolved;
    }

    // The leader waits for stragglers where it can do so without holding anybody up.
    let n = f.members.len();
    match f.state {
        FormationState::Whole => {
            f.waiting = open && !loose && lagging > 0 && f.patience > 0;
            if f.waiting {
                // The patience runs out at full speed while some straggler is not coming
                // closer and at a third of it while they all are: it is waited for longer,
                // but not for ever.
                if stalled_far || f.age.is_multiple_of(CLOSING_SLOWDOWN) {
                    f.patience -= 1;
                }
                stats.waits += 1;
            } else if lagging == 0 && !loose {
                f.patience = cfg.patience_ticks;
            }
            if f.patience == 0 && lagging > 0 {
                f.state = FormationState::Broken;
                f.waiting = false;
                stats.breaks += 1;
            }
        }
        FormationState::Broken => {
            f.waiting = false;
            if !loose && lagging * 10 <= n {
                f.state = FormationState::Whole;
                f.patience = cfg.patience_ticks;
                stats.reforms += 1;
            }
        }
    }
    if f.waiting {
        e.form[leader].mode = MemberMode::Wait;
    }
    // The fields the members will go back to when the formation ends stay alive.
    for &m in &f.members {
        let r = &e.route[m as usize];
        if r.field_serial != 0 && r.state != RouteState::Stranded {
            path.keep_field(r.field_serial);
        }
    }
    Verdict::Keep
}

/// Members that are stalled (not coming closer to their slot for a window) swap slots with the
/// member that stands in their way: another member within the sum of the two radii and a
/// little more of the unit, in the direction of its slot, that is not itself stalled. Each
/// member swaps at most once per call, the lowest slot first.
fn swap_blocked(f: &Formation, e: &mut Entities, units: &UnitTable, lead_at: Vec2, lead_off: Vec2) {
    let mut moved = vec![false; e.len()];
    for &m in &f.members {
        let i = m as usize;
        if i == f.leader as usize || moved[i] || e.form[i].lag == 0 {
            continue;
        }
        let slot = lead_at + (e.form[i].off - lead_off).rotate(f.heading);
        let want = slot - e.pos[i];
        if want.is_zero() {
            continue;
        }
        let radius_i = units.at(e.def[i] as usize).size.div_int(2);
        // The nearest member ahead of it, toward the slot, close enough to be in the way.
        let mut best: Option<(Fx, usize)> = None;
        for &o in &f.members {
            let j = o as usize;
            if j == i || j == f.leader as usize || moved[j] {
                continue;
            }
            let apart = e.pos[j] - e.pos[i];
            let reach = radius_i + units.at(e.def[j] as usize).size.div_int(2) + BLOCK_MARGIN;
            if apart.dot(want) <= Fx::ZERO || apart.length_squared() > reach.square() {
                continue;
            }
            // The other must itself be where it belongs, or the swap only passes the problem on.
            if e.form[j].lag != 0 {
                continue;
            }
            let key = apart.length_squared();
            if best.is_none_or(|(b, k)| (key, j) < (b, k)) {
                best = Some((key, j));
            }
        }
        if let Some((_, j)) = best {
            let (a, b) = (e.form[i].off, e.form[j].off);
            e.form[i].off = b;
            e.form[j].off = a;
            for k in [i, j] {
                e.form[k].lag = 0;
                e.form[k].far = Fx::MAX;
                moved[k] = true;
            }
        }
    }
}

/// Hands the slots of the lattice out again, each to the unit nearest it: the pairs of a unit
/// and a slot, nearest first, ties to the lower unit and then the lower slot, each unit and
/// each slot used once. The leader keeps its place (it is where the lattice hangs from).
/// A unit that already stands at a slot keeps it, as that pair is the nearest there is.
fn reassign(f: &Formation, e: &mut Entities, lead_at: Vec2, lead_off: Vec2) {
    let movers: Vec<usize> = f
        .members
        .iter()
        .map(|&m| m as usize)
        .filter(|&i| i != f.leader as usize)
        .collect();
    let offsets: Vec<Vec2> = movers.iter().map(|&i| e.form[i].off).collect();
    let mut pairs: Vec<(Fx, usize, usize)> = Vec::with_capacity(movers.len() * movers.len());
    for (a, &i) in movers.iter().enumerate() {
        for (k, off) in offsets.iter().enumerate() {
            let slot = lead_at + (*off - lead_off).rotate(f.heading);
            pairs.push(((slot - e.pos[i]).length_squared(), a, k));
        }
    }
    pairs.sort_unstable();
    let mut unit_done = vec![false; movers.len()];
    let mut slot_done = vec![false; movers.len()];
    let mut handed = vec![Vec2::ZERO; movers.len()];
    for (_, a, k) in pairs {
        if unit_done[a] || slot_done[k] {
            continue;
        }
        unit_done[a] = true;
        slot_done[k] = true;
        handed[a] = offsets[k];
    }
    for (a, &i) in movers.iter().enumerate() {
        if e.form[i].off != handed[a] {
            e.form[i].off = handed[a];
            // The unit has a new place to get to: its progress starts again.
            e.form[i].lag = 0;
            e.form[i].far = Fx::MAX;
        }
    }
}

/// Unit `i` goes back to the order's own way (a route of its own or the flowfield of its
/// group) after it walked to a slot or stood on one. A member of a flowfield chooses its
/// leg again from the cell it stands in. A route of its own is kept if it still leads the
/// way the unit is going and made again if the unit has left it behind
/// ([`Pathing::way_is_stale`]), which keeps a unit that goes in and out of sight of its slot
/// from asking for a new route each time.
fn forget_way(e: &mut Entities, i: usize, units: &UnitTable, path: &mut Pathing, goal: Vec2) {
    e.target[i] = None;
    if e.route[i].field_serial != 0 {
        let r = &mut e.route[i];
        r.ring_clear();
        r.final_leg = false;
        if r.state == RouteState::Stranded {
            r.state = RouteState::Active;
            r.cautious = false;
        }
    } else if path.way_is_stale(e, i, class_of(e, units, i), goal) {
        e.route.reset(i);
    }
}

/// The movement class of unit `i` (a formation's members all have one).
fn class_of(e: &Entities, units: &UnitTable, i: usize) -> u16 {
    units.at(e.def[i] as usize).movement_class.unwrap_or(0)
}

/// The member nearest the middle of `members` (ties to the lower slot).
fn nearest_to_middle(e: &Entities, members: &[u32]) -> u32 {
    let mut sum = Vec2::ZERO;
    for &m in members {
        sum += e.pos[m as usize];
    }
    let middle = sum.div_int(members.len() as i64);
    members
        .iter()
        .copied()
        .min_by_key(|&m| ((e.pos[m as usize] - middle).length_squared(), m))
        .unwrap_or(members[0])
}

/// What a unit in a formation does with this tick's walk. `None`: the formation took the
/// tick (the unit walked to its slot or stands) and the order's own walk is not run.
/// `Some(motion)`: run the order's walk with this motion, which is the unit's own, or, for
/// the leader, held to the pace of the formation.
pub(crate) fn drive(e: &mut Entities, i: usize, m: &Motion, ground: &Ground<'_>) -> Option<Motion> {
    let member = e.form[i];
    let capped = |step: Fx| Motion {
        step,
        step_sq: step.square(),
        ..*m
    };
    match member.mode {
        MemberMode::None | MemberMode::Flow => Some(*m),
        MemberMode::Lead => Some(capped(m.step.min(member.step))),
        MemberMode::Wait => {
            e.target[i] = None;
            None
        }
        MemberMode::Direct => {
            e.target[i] = Some(member.slot);
            let walked = walk(e, i, member.slot, &capped(m.step.min(member.step)), ground);
            if walked == Walk::Arrived {
                // On its slot: face the way the formation goes, the way the class turns.
                let turning = m
                    .class
                    .map_or(turning::Turning::LEGACY, |c| ground.movement.turning(c));
                let (facing, carry) = turning::turn_in_place(
                    &turning,
                    e.facing[i],
                    e.turn_carry[i],
                    member.heading,
                    m.turn_per_tick,
                );
                e.facing[i] = facing;
                e.turn_carry[i] = carry;
            }
            None
        }
    }
}

// ---- the lattice --------------------------------------------------------------------

/// The lattice of a new formation.
pub(crate) struct Lattice {
    /// Which of the members (by place in the list) leads.
    pub leader: usize,
    /// The way the group goes.
    pub heading: Facing,
    /// The offset of each member, in the order of the list.
    pub offsets: Vec<Vec2>,
}

/// Turns the layout of the group into a lattice (see the module documentation).
///
/// `members` are slots, ascending. The offsets are in the frame of the heading from the
/// leader to `goal`: `x` forward, `y` to the right.
pub(crate) fn layout(
    e: &Entities,
    units: &UnitTable,
    members: &[usize],
    goal: Vec2,
    cfg: &FormationConfig,
) -> Lattice {
    let n = members.len();
    // The leader: the unit nearest the middle.
    let mut sum = Vec2::ZERO;
    for &i in members {
        sum += e.pos[i];
    }
    let middle = sum.div_int(n as i64);
    let leader = (0..n)
        .min_by_key(|&k| ((e.pos[members[k]] - middle).length_squared(), members[k]))
        .unwrap_or(0);
    let lead_at = e.pos[members[leader]];
    let heading = (goal - lead_at).facing().unwrap_or(Facing::PLUS_X);
    // The widest body decides the lattice (as the arrival slots are spaced), and the
    // area the bodies need decides how far the group may spread.
    let spacing_of = |i: usize| crate::path::spacing_for(units.at(e.def[i] as usize).size);
    let widest = members
        .iter()
        .map(|&i| spacing_of(i))
        .max()
        .unwrap_or(Fx::ONE);
    let cell = widest
        .mul_int(i64::from(cfg.spacing_permille))
        .div_int(1000);
    let mut area = Fx::ZERO;
    for &i in members {
        area += spacing_of(i).square();
    }
    let disc = area.mul_div(Fx::from_int(113), Fx::from_int(355)).sqrt();
    let reach = disc
        .mul_int(i64::from(cfg.spread_permille))
        .div_int(1000)
        .max(cell);
    // Where each unit stands in the frame of the heading, pulled in if the group is wider
    // than it may be.
    let back = inverse(heading);
    let mut at: Vec<Vec2> = members
        .iter()
        .map(|&i| (e.pos[i] - lead_at).rotate(back))
        .collect();
    let far = at.iter().map(|p| p.length()).max().unwrap_or(Fx::ZERO);
    if far > reach {
        for p in &mut at {
            *p = Vec2::new(p.x.mul_div(reach, far), p.y.mul_div(reach, far));
        }
    }
    // Each unit takes the lattice point nearest its spot, the ones nearest the middle first.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&k| (at[k].length_squared(), members[k]));
    let mut taken: BTreeSet<(i64, i64)> = BTreeSet::new();
    let mut offsets = vec![Vec2::ZERO; n];
    for k in order {
        let want = (
            at[k].x.saturating_div(cell).round_int(),
            at[k].y.saturating_div(cell).round_int(),
        );
        let point = nearest_free(&taken, want, at[k], cell);
        taken.insert(point);
        offsets[k] = Vec2::new(cell.mul_int(point.0), cell.mul_int(point.1));
    }
    Lattice {
        leader,
        heading,
        offsets,
    }
}

/// The free lattice point nearest `spot` (in cells), searching outwards from `want`, the
/// point nearest to it: ties go to the lower point.
fn nearest_free(
    taken: &BTreeSet<(i64, i64)>,
    want: (i64, i64),
    spot: Vec2,
    cell: Fx,
) -> (i64, i64) {
    if !taken.contains(&want) {
        return want;
    }
    let dist =
        |p: (i64, i64)| (Vec2::new(cell.mul_int(p.0), cell.mul_int(p.1)) - spot).length_squared();
    // The best point so far, by (distance, point), and the ring the first free one was in.
    let mut best: Option<(Fx, (i64, i64))> = None;
    let mut found_in: Option<i64> = None;
    for ring in 1_i64.. {
        // A point of the next ring can be nearer than a corner of this one, so one more
        // ring is looked at after the first free point is found.
        if found_in.is_some_and(|found| ring > found + 1) {
            break;
        }
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs().max(dy.abs()) != ring {
                    continue;
                }
                let p = (want.0 + dx, want.1 + dy);
                if taken.contains(&p) {
                    continue;
                }
                let key = (dist(p), p);
                if best.is_none_or(|b| key < b) {
                    best = Some(key);
                }
                found_in.get_or_insert(ring);
            }
        }
    }
    best.map_or(want, |(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_numbers_are_valid_and_the_ranges_are_checked() {
        assert!(FormationConfig::DEFAULT.is_valid());
        assert!(FormationConfig::OFF.is_valid());
        let c = FormationConfig::DEFAULT;
        assert!(!FormationConfig { min_units: 1, ..c }.is_valid());
        assert!(
            !FormationConfig { min_units: 49, ..c }.is_valid(),
            "above max_units"
        );
        assert!(
            !FormationConfig {
                max_units: 257,
                ..c
            }
            .is_valid()
        );
        assert!(
            !FormationConfig {
                spacing_permille: 999,
                ..c
            }
            .is_valid()
        );
        assert!(
            !FormationConfig {
                spread_permille: 4001,
                ..c
            }
            .is_valid()
        );
        assert!(
            !FormationConfig {
                pace_permille: 499,
                ..c
            }
            .is_valid()
        );
        assert!(
            !FormationConfig {
                catchup_permille: 899,
                ..c
            }
            .is_valid()
        );
        assert!(
            !FormationConfig {
                catchup_permille: 1001,
                ..c
            }
            .is_valid()
        );
        assert!(
            !FormationConfig {
                reform_tenths: 4,
                ..c
            }
            .is_valid()
        );
        assert!(
            !FormationConfig {
                patience_ticks: 19,
                ..c
            }
            .is_valid()
        );
    }

    #[test]
    fn the_inverse_of_a_facing_turns_it_back() {
        for raw in 0..=255_u8 {
            let f = Facing::new(raw);
            let v = Vec2::from_ints(7, -3).rotate(f).rotate(inverse(f));
            let d = (v - Vec2::from_ints(7, -3)).length();
            assert!(d < Fx::from_ratio(1, 100), "{raw}: {d:?}");
        }
    }

    #[test]
    fn a_taken_point_gives_the_nearest_free_one() {
        let cell = Fx::ONE;
        let mut taken = BTreeSet::new();
        taken.insert((0, 0));
        // A unit just ahead of the middle point, but a little to the east: the east
        // neighbour is the nearest free point.
        let spot = Vec2::new(Fx::from_ratio(2, 5), Fx::from_ratio(1, 10));
        assert_eq!(nearest_free(&taken, (0, 0), spot, cell), (1, 0));
        // A free point is taken as it is.
        assert_eq!(nearest_free(&taken, (3, 3), spot, cell), (3, 3));
        // Many taken: still finds one, and never a taken one.
        for x in -3..=3 {
            for y in -3..=3 {
                taken.insert((x, y));
            }
        }
        let p = nearest_free(&taken, (0, 0), spot, cell);
        assert!(!taken.contains(&p));
        assert_eq!(p.0.abs().max(p.1.abs()), 4);
    }
}
