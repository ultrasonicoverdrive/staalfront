//! Saving and restoring the world (WP13.1a, `docs/rules/save.md`).
//!
//! A save holds the dynamic state only. The rules (the configuration, the terrain, the unit,
//! movement, weapon and building tables, the settings) are not in the file: the host builds a fresh
//! tick-0 world from the same match and assets, exactly as for a new match, and
//! [`World::restore`] fills in the state after checking that the rules digest in the header
//! ([`World::rules_digest`]) is the fresh world's.
//!
//! # Format
//!
//! Everything is written with [`sf_math::bytes`]: little-endian, fixed width, a `usize` as a `u32`.
//!
//! * Header: the magic `SFSV`, `format_version` ([`FORMAT_VERSION`]), the rules digest, the tick
//!   and the number of sections.
//! * Sections, each a tag byte and a `u32` length: `W` the world scalars, `E` the entities, `Q` the
//!   command queue, in that order.
//!
//! # Not yet saved
//!
//! Pathing, flowfields, formations, turning, weapons, projectiles, targeting, destruction,
//! buildings (construction), economy, production, power, repair, vision and outcome come in WP13.1b
//! and WP13.1c. Until then [`World::save`] refuses, with [`SaveError::Unsupported`] naming the
//! section, any world in which one of them holds something that is not its tick-0 state; the exact
//! test per section is in `docs/rules/save.md`. A world that saves therefore restores whole.
//!
//! # Guards
//!
//! Every function here that saves a struct destructures it without `..`: a field added to
//! [`World`] or to the entity columns is a compile error until it is saved, rebuilt or gated here.

use core::fmt;
use std::collections::BTreeMap;

use sf_math::bytes::{ByteError, ByteReader, ByteWriter};
use sf_math::hash::Fnv1a64;
use sf_math::{Facing, Fx, Rng, RngState, Vec2};

use crate::behavior::{Area, Behavior};
use crate::command::{Command, CommandQueue};
use crate::crowd::{Claim, CrowdState};
use crate::entities::{Entities, NO_DEF};
use crate::formation::Member;
use crate::ids::{EntityId, Kind, PlayerId, Tick};
use crate::orders::{GuardTarget, ORDER_QUEUE_CAP, Order, OrderQueue};
use crate::path::Routes;
use crate::targeting::Engage;
use crate::terrain::{Cell, NO_CELL, Occupancy};
use crate::weapons::REST;
use crate::world::{World, footprint_cells};

/// The first bytes of every save file.
pub const MAGIC: [u8; 4] = *b"SFSV";

/// The layout of the bytes after the magic. Raised whenever that layout changes; an older save is
/// refused with [`SaveError::Version`] (no migration yet).
pub const FORMAT_VERSION: u32 = 1;

const SECTION_WORLD: u8 = b'W';
const SECTION_ENTITIES: u8 = b'E';
const SECTION_QUEUE: u8 = b'Q';
/// The sections of this format, in file order.
const SECTIONS: [u8; 3] = [SECTION_WORLD, SECTION_ENTITIES, SECTION_QUEUE];

/// Why a world could not be saved or restored.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SaveError {
    /// The bytes ended early.
    Truncated,
    /// The bytes do not start with [`MAGIC`].
    BadMagic,
    /// The file has another format version than [`FORMAT_VERSION`].
    Version(u32),
    /// The file was saved with other rules (assets, settings, map) than the fresh world has.
    Rules,
    /// The bytes are malformed or describe an impossible world; names what was wrong.
    Corrupt(&'static str),
    /// The world holds state of a section this format does not save yet; names the section.
    Unsupported(&'static str),
    /// The world given to [`World::restore`] is not at tick zero, or already has entities or
    /// commands.
    NotFresh,
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::Truncated => write!(f, "the save ends early"),
            SaveError::BadMagic => write!(f, "not a save file"),
            SaveError::Version(v) => {
                write!(f, "save format {v}, this build reads {FORMAT_VERSION}")
            }
            SaveError::Rules => write!(f, "the save was made with other rules or assets"),
            SaveError::Corrupt(what) => write!(f, "the save is corrupt: {what}"),
            SaveError::Unsupported(what) => {
                write!(f, "the {what} state cannot be saved yet")
            }
            SaveError::NotFresh => write!(f, "restore needs a world at tick zero"),
        }
    }
}

impl std::error::Error for SaveError {}

impl From<ByteError> for SaveError {
    fn from(e: ByteError) -> SaveError {
        match e {
            ByteError::Truncated => SaveError::Truncated,
            ByteError::TooLong => SaveError::Corrupt("a length does not fit 32 bits"),
            ByteError::BadBool => SaveError::Corrupt("a flag is neither 0 nor 1"),
            ByteError::BadSection => SaveError::Corrupt("a section has the wrong tag or length"),
        }
    }
}

type SaveResult<T> = Result<T, SaveError>;

// ---- primitives of the simulation ---------------------------------------------------------------

fn put_fx(w: &mut ByteWriter, v: Fx) {
    w.i64(v.raw());
}

fn get_fx(r: &mut ByteReader<'_>) -> SaveResult<Fx> {
    Ok(Fx::from_raw(r.i64()?))
}

fn put_vec2(w: &mut ByteWriter, v: Vec2) {
    put_fx(w, v.x);
    put_fx(w, v.y);
}

fn get_vec2(r: &mut ByteReader<'_>) -> SaveResult<Vec2> {
    Ok(Vec2 {
        x: get_fx(r)?,
        y: get_fx(r)?,
    })
}

fn put_opt_vec2(w: &mut ByteWriter, v: Option<Vec2>) {
    w.bool(v.is_some());
    if let Some(v) = v {
        put_vec2(w, v);
    }
}

fn get_opt_vec2(r: &mut ByteReader<'_>) -> SaveResult<Option<Vec2>> {
    Ok(if r.bool()? { Some(get_vec2(r)?) } else { None })
}

fn put_id(w: &mut ByteWriter, id: EntityId) {
    w.u32(id.index());
    w.u32(id.generation());
}

fn get_id(r: &mut ByteReader<'_>) -> SaveResult<EntityId> {
    Ok(EntityId::new(r.u32()?, r.u32()?))
}

fn put_opt_id(w: &mut ByteWriter, id: Option<EntityId>) {
    w.bool(id.is_some());
    if let Some(id) = id {
        put_id(w, id);
    }
}

fn get_opt_id(r: &mut ByteReader<'_>) -> SaveResult<Option<EntityId>> {
    Ok(if r.bool()? { Some(get_id(r)?) } else { None })
}

fn put_ids(w: &mut ByteWriter, ids: &[EntityId]) -> Result<(), ByteError> {
    w.usize(ids.len())?;
    for &id in ids {
        put_id(w, id);
    }
    Ok(())
}

fn get_ids(r: &mut ByteReader<'_>) -> SaveResult<Vec<EntityId>> {
    let n = r.count(8)?;
    (0..n).map(|_| get_id(r)).collect()
}

fn put_cell(w: &mut ByteWriter, c: Cell) {
    w.u32(c.x);
    w.u32(c.y);
}

fn get_cell(r: &mut ByteReader<'_>) -> SaveResult<Cell> {
    Ok(Cell::new(r.u32()?, r.u32()?))
}

fn put_area(w: &mut ByteWriter, a: Area) {
    put_vec2(w, a.min);
    put_vec2(w, a.max);
}

fn get_area(r: &mut ByteReader<'_>) -> SaveResult<Area> {
    // Field by field, not through `Area::new`: the corners come back exactly as they were.
    Ok(Area {
        min: get_vec2(r)?,
        max: get_vec2(r)?,
    })
}

fn put_rng(w: &mut ByteWriter, rng: &Rng) {
    let RngState { state, inc } = rng.state();
    w.u64(state);
    w.u64(inc);
}

fn get_rng(r: &mut ByteReader<'_>) -> SaveResult<Rng> {
    let state = r.u64()?;
    let inc = r.u64()?;
    // Every generator the world makes has an odd increment.
    if inc & 1 == 0 {
        return Err(SaveError::Corrupt("a random stream has an even increment"));
    }
    Ok(Rng::from_state(RngState { state, inc }))
}

/// The tag of an order in the file. The same numbers as the checksum's (`Order::write_hash`).
fn put_order(w: &mut ByteWriter, o: Order) {
    match o {
        Order::Idle => w.u8(0),
        Order::Move(p) => {
            w.u8(1);
            put_vec2(w, p);
        }
        Order::Attack(id) => {
            w.u8(2);
            put_id(w, id);
        }
        Order::AttackMove(p) => {
            w.u8(3);
            put_vec2(w, p);
        }
        Order::Guard(GuardTarget::Point(p)) => {
            w.u8(4);
            put_vec2(w, p);
        }
        Order::Guard(GuardTarget::Entity(id)) => {
            w.u8(5);
            put_id(w, id);
        }
        Order::Scatter => w.u8(6),
        Order::HoldPosition => w.u8(7),
        Order::Pursue(id) => {
            w.u8(8);
            put_id(w, id);
        }
    }
}

fn get_order(r: &mut ByteReader<'_>) -> SaveResult<Order> {
    Ok(match r.u8()? {
        0 => Order::Idle,
        1 => Order::Move(get_vec2(r)?),
        2 => Order::Attack(get_id(r)?),
        3 => Order::AttackMove(get_vec2(r)?),
        4 => Order::Guard(GuardTarget::Point(get_vec2(r)?)),
        5 => Order::Guard(GuardTarget::Entity(get_id(r)?)),
        6 => Order::Scatter,
        7 => Order::HoldPosition,
        8 => Order::Pursue(get_id(r)?),
        _ => return Err(SaveError::Corrupt("unknown order")),
    })
}

fn get_behavior(r: &mut ByteReader<'_>) -> SaveResult<Behavior> {
    Ok(match r.u8()? {
        0 => Behavior::Idle,
        1 => Behavior::Wander,
        2 => Behavior::MoveTo,
        3 => Behavior::Ordered,
        _ => return Err(SaveError::Corrupt("unknown behaviour")),
    })
}

// ---- commands -----------------------------------------------------------------------------------

/// The tag of each command variant in the file. Pinned by `command_tags_are_pinned`: a renumbering
/// makes every saved queue read back as other commands.
pub(crate) fn command_tag(c: &Command) -> u8 {
    match c {
        Command::Spawn { .. } => 0,
        Command::Place { .. } => 1,
        Command::SpawnUnit { .. } => 2,
        Command::PlaceStructure { .. } => 3,
        Command::BuildStructure { .. } => 4,
        Command::QueueUnit { .. } => 5,
        Command::CancelUnit { .. } => 6,
        Command::SetRally { .. } => 7,
        Command::SetRepeat { .. } => 8,
        Command::Repair { .. } => 9,
        Command::Sell { .. } => 10,
        Command::Order { .. } => 11,
        Command::Stop { .. } => 12,
        Command::AimTurret { .. } => 13,
        Command::TakeOre { .. } => 14,
        Command::Despawn { .. } => 15,
        Command::SetWanderArea { .. } => 16,
        Command::MoveTo { .. } => 17,
    }
}

/// Writes a command: its tag ([`command_tag`]), then its fields in declaration order.
pub fn write_command(w: &mut ByteWriter, c: &Command) -> Result<(), ByteError> {
    w.u8(command_tag(c));
    match c {
        Command::Spawn { kind, pos, owner } | Command::SpawnUnit { kind, pos, owner } => {
            w.u16(kind.0);
            put_vec2(w, *pos);
            w.u8(owner.0);
        }
        Command::Place { kind, cell, owner } | Command::PlaceStructure { kind, cell, owner } => {
            w.u16(kind.0);
            put_cell(w, *cell);
            w.u8(owner.0);
        }
        Command::BuildStructure { kind, cell } => {
            w.u16(kind.0);
            put_cell(w, *cell);
        }
        Command::QueueUnit { factory, kind } => {
            put_id(w, *factory);
            w.u16(kind.0);
        }
        Command::CancelUnit { factory, index } => {
            put_id(w, *factory);
            w.u8(*index);
        }
        Command::SetRally { factory, cell } => {
            put_id(w, *factory);
            w.bool(cell.is_some());
            if let Some(cell) = cell {
                put_cell(w, *cell);
            }
        }
        Command::SetRepeat { factory, on } => {
            put_id(w, *factory);
            w.bool(*on);
        }
        Command::Repair { structure, on } => {
            put_id(w, *structure);
            w.bool(*on);
        }
        Command::Sell { structure } => put_id(w, *structure),
        Command::Order { ids, order, queued } => {
            put_ids(w, ids)?;
            put_order(w, *order);
            w.bool(*queued);
        }
        Command::Stop { ids } => put_ids(w, ids)?,
        Command::AimTurret { ids, target } => {
            put_ids(w, ids)?;
            put_opt_id(w, *target);
        }
        Command::TakeOre { cell, amount } => {
            put_cell(w, *cell);
            w.u16(*amount);
        }
        Command::Despawn { id } => put_id(w, *id),
        Command::SetWanderArea { ids, area } => {
            put_ids(w, ids)?;
            put_area(w, *area);
        }
        Command::MoveTo { ids, target } => {
            put_ids(w, ids)?;
            put_vec2(w, *target);
        }
    }
    Ok(())
}

/// Reads a command written by [`write_command`].
pub fn read_command(r: &mut ByteReader<'_>) -> Result<Command, SaveError> {
    let tag = r.u8()?;
    Ok(match tag {
        0 | 2 => {
            let kind = Kind(r.u16()?);
            let pos = get_vec2(r)?;
            let owner = PlayerId(r.u8()?);
            if tag == 0 {
                Command::Spawn { kind, pos, owner }
            } else {
                Command::SpawnUnit { kind, pos, owner }
            }
        }
        1 | 3 => {
            let kind = Kind(r.u16()?);
            let cell = get_cell(r)?;
            let owner = PlayerId(r.u8()?);
            if tag == 1 {
                Command::Place { kind, cell, owner }
            } else {
                Command::PlaceStructure { kind, cell, owner }
            }
        }
        4 => Command::BuildStructure {
            kind: Kind(r.u16()?),
            cell: get_cell(r)?,
        },
        5 => Command::QueueUnit {
            factory: get_id(r)?,
            kind: Kind(r.u16()?),
        },
        6 => Command::CancelUnit {
            factory: get_id(r)?,
            index: r.u8()?,
        },
        7 => Command::SetRally {
            factory: get_id(r)?,
            cell: if r.bool()? { Some(get_cell(r)?) } else { None },
        },
        8 => Command::SetRepeat {
            factory: get_id(r)?,
            on: r.bool()?,
        },
        9 => Command::Repair {
            structure: get_id(r)?,
            on: r.bool()?,
        },
        10 => Command::Sell {
            structure: get_id(r)?,
        },
        11 => Command::Order {
            ids: get_ids(r)?,
            order: get_order(r)?,
            queued: r.bool()?,
        },
        12 => Command::Stop { ids: get_ids(r)? },
        13 => Command::AimTurret {
            ids: get_ids(r)?,
            target: get_opt_id(r)?,
        },
        14 => Command::TakeOre {
            cell: get_cell(r)?,
            amount: r.u16()?,
        },
        15 => Command::Despawn { id: get_id(r)? },
        16 => Command::SetWanderArea {
            ids: get_ids(r)?,
            area: get_area(r)?,
        },
        17 => Command::MoveTo {
            ids: get_ids(r)?,
            target: get_vec2(r)?,
        },
        _ => return Err(SaveError::Corrupt("unknown command")),
    })
}

fn save_queue(w: &mut ByteWriter, q: &CommandQueue) -> SaveResult<()> {
    let (pending, next_seq) = q.parts();
    w.usize(pending.len())?;
    for (&(tick, player, seq), command) in pending {
        w.u64(tick.0);
        w.u8(player.0);
        w.u64(seq);
        write_command(w, command)?;
    }
    w.usize(next_seq.len())?;
    for (&player, &seq) in next_seq {
        w.u8(player.0);
        w.u64(seq);
    }
    Ok(())
}

fn restore_queue(r: &mut ByteReader<'_>, now: Tick) -> SaveResult<CommandQueue> {
    let n = r.count(18)?;
    let mut pending = BTreeMap::new();
    let mut last: Option<(Tick, PlayerId, u64)> = None;
    for _ in 0..n {
        let key = (Tick(r.u64()?), PlayerId(r.u8()?), r.u64()?);
        let command = read_command(r)?;
        // Strictly ascending: no duplicate key, the file order is the execution order.
        if last.is_some_and(|l| l >= key) {
            return Err(SaveError::Corrupt("the command queue is out of order"));
        }
        if key.0 < now {
            return Err(SaveError::Corrupt("a pending command is for a past tick"));
        }
        if key.2 == u64::MAX {
            return Err(SaveError::Corrupt(
                "a pending command has the reserved number",
            ));
        }
        last = Some(key);
        pending.insert(key, command);
    }
    let n = r.count(9)?;
    let mut next_seq = BTreeMap::new();
    let mut last: Option<PlayerId> = None;
    for _ in 0..n {
        let player = PlayerId(r.u8()?);
        let seq = r.u64()?;
        if last.is_some_and(|l| l >= player) {
            return Err(SaveError::Corrupt("the sequence counters are out of order"));
        }
        last = Some(player);
        next_seq.insert(player, seq);
    }
    // Every pending command was numbered below its player's counter.
    for &(_, player, seq) in pending.keys() {
        if next_seq.get(&player).is_none_or(|&next| seq >= next) {
            return Err(SaveError::Corrupt("a command is numbered past its counter"));
        }
    }
    Ok(CommandQueue::from_parts(pending, next_seq))
}

// ---- entities -----------------------------------------------------------------------------------

fn put_opt_tick(w: &mut ByteWriter, t: Option<Tick>) {
    w.bool(t.is_some());
    if let Some(t) = t {
        w.u64(t.0);
    }
}

fn get_opt_tick(r: &mut ByteReader<'_>) -> SaveResult<Option<Tick>> {
    Ok(if r.bool()? {
        Some(Tick(r.u64()?))
    } else {
        None
    })
}

/// The crowd memory of a unit, every field (the "scratch within a tick" ones too: the crowd pass of
/// the next tick sets them before it reads them, but writing them keeps a resave byte-equal and the
/// restored world equal by `PartialEq`). Destructured without `..`.
fn put_crowd(w: &mut ByteWriter, c: &CrowdState) {
    let CrowdState {
        quiet,
        attempts,
        best,
        mark,
        side,
        shoved,
        calm,
        replan,
        replans,
        cool,
        retarget,
        give_up,
        gave_up_at,
        idle_hit,
        pressed,
        claim,
        home,
        home_wait,
        home_rest,
        disc,
        settle,
        fresh,
        walking,
        refused,
        stranded,
    } = c;
    w.u16(*quiet);
    w.u8(*attempts);
    put_fx(w, *best);
    put_opt_vec2(w, *mark);
    w.bool(side.is_some());
    if let Some((at, n)) = side {
        put_vec2(w, *at);
        w.u8(*n);
    }
    put_fx(w, *shoved);
    w.u8(*calm);
    w.bool(*replan);
    w.u8(*replans);
    w.u16(*cool);
    w.u16(*retarget);
    w.bool(*give_up);
    put_opt_tick(w, *gave_up_at);
    w.bool(*idle_hit);
    w.u16(*pressed);
    w.u32(claim.serial);
    w.u16(claim.slot);
    put_opt_vec2(w, *home);
    w.u8(*home_wait);
    w.bool(*home_rest);
    put_fx(w, *disc);
    w.bool(*settle);
    w.bool(*fresh);
    w.bool(*walking);
    w.bool(*refused);
    w.bool(*stranded);
}

fn get_crowd(r: &mut ByteReader<'_>) -> SaveResult<CrowdState> {
    Ok(CrowdState {
        quiet: r.u16()?,
        attempts: r.u8()?,
        best: get_fx(r)?,
        mark: get_opt_vec2(r)?,
        side: if r.bool()? {
            Some((get_vec2(r)?, r.u8()?))
        } else {
            None
        },
        shoved: get_fx(r)?,
        calm: r.u8()?,
        replan: r.bool()?,
        replans: r.u8()?,
        cool: r.u16()?,
        retarget: r.u16()?,
        give_up: r.bool()?,
        gave_up_at: get_opt_tick(r)?,
        idle_hit: r.bool()?,
        pressed: r.u16()?,
        claim: Claim {
            serial: r.u32()?,
            slot: r.u16()?,
        },
        home: get_opt_vec2(r)?,
        home_wait: r.u8()?,
        home_rest: r.bool()?,
        disc: get_fx(r)?,
        settle: r.bool()?,
        fresh: r.bool()?,
        walking: r.bool()?,
        refused: r.bool()?,
        stranded: r.bool()?,
    })
}

/// Refuses a world in which a column of a section this format does not save yet holds anything
/// that a freshly spawned (or freed) entity does not.
fn check_unsaved_columns(e: &Entities) -> SaveResult<()> {
    let idle_route = Routes::default();
    for i in 0..e.len() {
        if e.route[i] != idle_route[i] {
            return Err(SaveError::Unsupported("pathing"));
        }
        if e.turn_carry[i] != 0 {
            return Err(SaveError::Unsupported("turning"));
        }
        if e.form[i] != Member::NONE {
            return Err(SaveError::Unsupported("formations"));
        }
        if e.weapon[i] != REST {
            return Err(SaveError::Unsupported("weapons"));
        }
        if e.eng[i] != Engage::REST {
            return Err(SaveError::Unsupported("targeting"));
        }
        if e.build_left[i] != 0 {
            return Err(SaveError::Unsupported("buildings"));
        }
        if e.repairing[i] || e.sell_left[i] != 0 {
            return Err(SaveError::Unsupported("repair"));
        }
        if e.kills[i] != 0 {
            return Err(SaveError::Unsupported("destruction"));
        }
    }
    Ok(())
}

fn save_entities(w: &mut ByteWriter, e: &Entities) -> SaveResult<()> {
    // Guard: every column is named. A new column fails to compile until it is saved, derived or
    // gated in `check_unsaved_columns`.
    let Entities {
        generation,
        alive,
        kind,
        owner,
        pos,
        // Derived: the position before the last step, for render interpolation only; the next step
        // sets it before anything reads it. Restored equal to `pos`.
        prev_pos: _,
        facing,
        // Derived like `prev_pos`.
        prev_facing: _,
        behavior,
        target,
        area,
        rng,
        cell,
        // Saved although it looks derived: a legacy `Spawn` or `Place` entity has `NO_DEF` whatever
        // its kind, so the kind alone does not say which.
        def,
        hp,
        order,
        queue,
        guard_walking,
        // Gated (pathing, WP13.1b): idle on every slot of a world that saves.
        route: _,
        crowd,
        // Gated (turning, WP13.1b): zero.
        turn_carry: _,
        turret,
        // Derived like `prev_pos`.
        prev_turret: _,
        turret_target,
        // Gated (formations, WP13.1b): `Member::NONE`.
        form: _,
        // Gated (weapons, WP13.1c): at rest.
        weapon: _,
        // Gated (targeting, WP13.1c): at rest.
        eng: _,
        // Gated (buildings, WP13.1c): zero.
        build_left: _,
        // Gated (repair, WP13.1c): false and zero.
        repairing: _,
        sell_left: _,
        // Gated (destruction, WP13.1c): zero.
        kills: _,
    } = e;
    check_unsaved_columns(e)?;
    w.usize(e.len())?;
    for i in 0..e.len() {
        w.u32(generation[i]);
        w.bool(alive[i]);
        if !alive[i] {
            // A free slot holds the neutral values of `Entities::clear`; only its generation counts.
            continue;
        }
        w.u16(kind[i].0);
        w.u8(owner[i].0);
        put_vec2(w, pos[i]);
        w.u8(facing[i].raw());
        w.u8(behavior[i].tag());
        put_opt_vec2(w, target[i]);
        put_area(w, area[i]);
        put_rng(w, &rng[i]);
        w.u32(cell[i]);
        w.u32(def[i]);
        w.u32(hp[i]);
        put_order(w, order[i]);
        w.u8(queue[i].len() as u8);
        for o in queue[i].iter() {
            put_order(w, o);
        }
        w.bool(guard_walking[i]);
        w.u8(turret[i].raw());
        put_opt_id(w, turret_target[i]);
        put_crowd(w, &crowd[i]);
    }
    Ok(())
}

fn restore_entities(r: &mut ByteReader<'_>, world: &mut World) -> SaveResult<()> {
    let n = r.count(5)?;
    let (width, height) = (world.config.width, world.config.height);
    let cells = u64::from(width) * u64::from(height);
    let (max_x, max_y) = (
        Fx::from_i64(i64::from(width)),
        Fx::from_i64(i64::from(height)),
    );
    let inside = |p: Vec2| p.x >= Fx::ZERO && p.y >= Fx::ZERO && p.x <= max_x && p.y <= max_y;
    let e = &mut world.entities;
    for i in 0..n {
        e.push_free_slot();
        e.generation[i] = r.u32()?;
        if !r.bool()? {
            continue;
        }
        e.alive[i] = true;
        e.kind[i] = Kind(r.u16()?);
        e.owner[i] = PlayerId(r.u8()?);
        e.pos[i] = get_vec2(r)?;
        if !inside(e.pos[i]) {
            return Err(SaveError::Corrupt("an entity stands outside the map"));
        }
        e.prev_pos[i] = e.pos[i];
        e.facing[i] = Facing::new(r.u8()?);
        e.prev_facing[i] = e.facing[i];
        e.behavior[i] = get_behavior(r)?;
        e.target[i] = get_opt_vec2(r)?;
        e.area[i] = get_area(r)?;
        e.rng[i] = get_rng(r)?;
        e.cell[i] = r.u32()?;
        if e.cell[i] != NO_CELL && u64::from(e.cell[i]) >= cells {
            return Err(SaveError::Corrupt(
                "an entity occupies a cell outside the map",
            ));
        }
        e.def[i] = r.u32()?;
        if e.def[i] != NO_DEF {
            let def = e.def[i] as usize;
            if def >= world.units.len() || world.units.at(def).kind != e.kind[i] {
                return Err(SaveError::Corrupt(
                    "an entity has no unit table entry for its kind",
                ));
            }
        }
        e.hp[i] = r.u32()?;
        e.order[i] = get_order(r)?;
        let queued = usize::from(r.u8()?);
        if queued > ORDER_QUEUE_CAP {
            return Err(SaveError::Corrupt("an order queue is longer than the ring"));
        }
        let mut q = OrderQueue::EMPTY;
        for _ in 0..queued {
            q.push_back(get_order(r)?);
        }
        e.queue[i] = q;
        e.guard_walking[i] = r.bool()?;
        e.turret[i] = Facing::new(r.u8()?);
        e.prev_turret[i] = e.turret[i];
        e.turret_target[i] = get_opt_id(r)?;
        e.crowd[i] = get_crowd(r)?;
    }
    Ok(())
}

// ---- the world ----------------------------------------------------------------------------------

/// Refuses a world whose subsystem state outside the entity columns is not its tick-0 state.
fn check_unsaved_sections(w: &World) -> SaveResult<()> {
    if let Some(section) = w.pathing.unsaved_section(w.tick) {
        return Err(SaveError::Unsupported(section));
    }
    if !w.formations.at_start() {
        return Err(SaveError::Unsupported("formations"));
    }
    if !w.arsenal.projectiles_at_start() {
        return Err(SaveError::Unsupported("projectiles"));
    }
    if !w.destruction.wrecks.is_empty() || !w.destruction.tally.is_empty() {
        return Err(SaveError::Unsupported("destruction"));
    }
    if w.ore.as_ref().is_some_and(|o| o.version() != 0)
        || w.harvest.is_some()
        || w.money
            .as_ref()
            .is_some_and(|m| m.spent.iter().any(|&(_, s)| s != 0))
    {
        return Err(SaveError::Unsupported("economy"));
    }
    if w.production.is_some() {
        return Err(SaveError::Unsupported("production"));
    }
    if w.power.is_some() {
        return Err(SaveError::Unsupported("power"));
    }
    if w.vision.is_some() {
        return Err(SaveError::Unsupported("vision"));
    }
    if w.outcome.is_some() {
        return Err(SaveError::Unsupported("outcome"));
    }
    Ok(())
}

impl World {
    /// A digest of the rules the world was made with: the configuration, the terrain and every
    /// table and setting the checksum hashes as rules. Two worlds built from the same match and
    /// assets have the same digest; a save only restores onto a world with the digest it was made
    /// with. Rules of the sections that WP13.1b/c add (and the AI profiles, WP13.1d) join this
    /// digest with them; a world with those rules does not save yet.
    pub fn rules_digest(&self) -> u64 {
        let mut h = Fnv1a64::new();
        h.write_bytes(b"sf-rules-1");
        h.write_u64(self.config.seed);
        h.write_u32(self.config.width);
        h.write_u32(self.config.height);
        h.write_fx(self.config.speed);
        h.write_u8(self.config.turn_per_tick);
        h.write_u8(u8::from(self.terrain.has_layer()));
        h.write_u64(self.terrain.digest());
        h.write_u64(self.units.digest());
        h.write_u64(self.units.reach_digest());
        h.write_u64(self.units.turret_digest());
        h.write_u64(self.movement.digest());
        h.write_u64(self.arsenal.table.digest());
        h.write_u64(self.buildings.digest());
        h.write_u64(self.buildings.placement_digest());
        h.write_u64(self.placement.players.len() as u64);
        for &(player, bit) in &self.placement.players {
            h.write_u8(player.0);
            h.write_u8(bit);
        }
        let c = self.crowd.config;
        h.write_u8(u8::from(c.enabled));
        h.write_u32(u32::from(c.wait_ticks));
        h.write_u32(u32::from(c.stuck_ticks));
        h.write_u32(u32::from(c.stuck_attempts));
        let f = self.formations.config;
        h.write_u8(u8::from(f.enabled));
        h.write_u32(u32::from(f.min_units));
        h.write_u32(u32::from(f.max_units));
        h.write_u32(u32::from(f.spacing_permille));
        h.write_u32(u32::from(f.spread_permille));
        h.write_u32(self.pathing.budget());
        h.write_u32(self.pathing.flowfield_threshold());
        let rp = self.pathing.replan_config();
        h.write_u32(u32::from(rp.cooldown));
        h.write_u32(u32::from(rp.max));
        h.write_u32(u32::from(rp.retarget_cooldown));
        match &self.targeting.config {
            Some(t) => {
                h.write_u8(1);
                t.write_hash(&mut h);
            }
            None => h.write_u8(0),
        }
        match &self.destruction.config {
            Some(d) => {
                h.write_u8(1);
                d.write_hash(&mut h);
            }
            None => h.write_u8(0),
        }
        // Which of the optional rule sets the world has (their numbers join in WP13.1c).
        for present in [
            self.ore.is_some(),
            self.harvest.is_some(),
            self.money.is_some(),
            self.production.is_some(),
            self.power.is_some(),
            self.repair.is_some(),
            self.vision.is_some(),
            self.outcome.is_some(),
        ] {
            h.write_u8(u8::from(present));
        }
        h.finish()
    }

    /// The dynamic state of the world as bytes (`docs/rules/save.md`). Restore them with
    /// [`World::restore`] onto a fresh world built from the same match and assets.
    ///
    /// Refused with [`SaveError::Unsupported`] while a section this format does not save yet holds
    /// anything that is not its tick-0 state.
    pub fn save(&self) -> Result<Vec<u8>, SaveError> {
        // Guard: every field is named. A new field fails to compile until it is saved, rebuilt or
        // gated here.
        let World {
            // Rules: in the rules digest, rebuilt by the host.
            config: _,
            tick,
            entities,
            free,
            // Derived: the number of live slots.
            live: _,
            spawn_serial,
            // Rules: in the rules digest.
            terrain: _,
            // Derived: rebuilt from the structures and their footprints. Its count is written as a
            // corruption check.
            occupancy,
            // Rules.
            units: _,
            movement: _,
            // Gated in `check_unsaved_sections` (the graphs are rules).
            pathing: _,
            // Rules (settings and shapes) and scratch; the state is the entity column.
            crowd: _,
            // Gated.
            formations: _,
            // Rules (the table and what is derived from it) and the reports of the last tick;
            // projectiles gated.
            arsenal: _,
            // Rules.
            buildings: _,
            // Gated.
            ore: _,
            harvest: _,
            money: _,
            // A report of the last tick, not hashed: replaced by every step.
            stalled: _,
            // Gated.
            production: _,
            power: _,
            // Rules and the reports of the last tick; the state is the entity columns.
            repair: _,
            // Gated.
            vision: _,
            outcome: _,
            // Rules (who is which faction) and the report of the last tick.
            placement: _,
            // A report of the last tick.
            completions: _,
            // Rules and scratch; the state is the entity column.
            targeting: _,
            // Rules and the report of the last tick; wrecks and tally gated.
            destruction: _,
            queue,
            // Not saved: a restored world starts a new history at its tick.
            history: _,
            history_base: _,
        } = self;
        check_unsaved_sections(self)?;

        let mut w = ByteWriter::new();
        w.raw(&MAGIC);
        w.u32(FORMAT_VERSION);
        w.u64(self.rules_digest());
        w.u64(tick.0);
        w.usize(SECTIONS.len())?;
        w.section(SECTION_WORLD, |s| {
            s.u64(*spawn_serial);
            s.usize(free.len())?;
            for &index in free {
                s.u32(index);
            }
            s.usize(occupancy.count())?;
            Ok::<(), SaveError>(())
        })?;
        w.section(SECTION_ENTITIES, |s| save_entities(s, entities))?;
        w.section(SECTION_QUEUE, |s| save_queue(s, queue))?;
        Ok(w.finish())
    }

    /// The world `bytes` describe, built on `fresh`: the tick-0 world the host made from the same
    /// match and assets as the saved one. Refused when the bytes are not a save of this format
    /// ([`SaveError::BadMagic`], [`SaveError::Version`]), were made with other rules
    /// ([`SaveError::Rules`]), are cut short ([`SaveError::Truncated`]) or describe an impossible
    /// world ([`SaveError::Corrupt`]). Never panics, whatever the bytes.
    pub fn restore(fresh: World, bytes: &[u8]) -> Result<World, SaveError> {
        if fresh.tick != Tick::ZERO
            || fresh.entities.len() != 0
            || !fresh.queue.is_empty()
            || !fresh.history.is_empty()
        {
            return Err(SaveError::NotFresh);
        }
        let mut r = ByteReader::new(bytes);
        if r.take(MAGIC.len()).map_err(|_| SaveError::BadMagic)? != MAGIC {
            return Err(SaveError::BadMagic);
        }
        let version = r.u32()?;
        if version != FORMAT_VERSION {
            return Err(SaveError::Version(version));
        }
        if r.u64()? != fresh.rules_digest() {
            return Err(SaveError::Rules);
        }
        let tick = Tick(r.u64()?);
        if r.usize()? != SECTIONS.len() {
            return Err(SaveError::Corrupt("wrong number of sections"));
        }

        let mut world = fresh;
        world.tick = tick;
        world.history_base = tick;
        world.pathing.restore_clock(tick);

        let mut s = r.section(SECTION_WORLD)?;
        world.spawn_serial = s.u64()?;
        let n = s.count(4)?;
        let mut free = Vec::with_capacity(n);
        for _ in 0..n {
            free.push(s.u32()?);
        }
        let occupied = s.usize()?;
        s.end()?;

        let mut s = r.section(SECTION_ENTITIES)?;
        restore_entities(&mut s, &mut world)?;
        s.end()?;

        let mut s = r.section(SECTION_QUEUE)?;
        world.queue = restore_queue(&mut s, tick)?;
        s.end()?;
        r.end()?;

        // The free list: distinct free slots, and every free slot not on it retired for good.
        let slots = world.entities.len();
        let mut listed = vec![false; slots];
        for &index in &free {
            let i = index as usize;
            if i >= slots || world.entities.alive[i] || listed[i] {
                return Err(SaveError::Corrupt("the free list"));
            }
            listed[i] = true;
        }
        for (i, &on_list) in listed.iter().enumerate() {
            if !world.entities.alive[i] && !on_list && world.entities.generation[i] != u32::MAX {
                return Err(SaveError::Corrupt(
                    "a free slot is missing from the free list",
                ));
            }
        }
        world.free = free;
        world.live = world.entities.alive.iter().filter(|&&a| a).count() as u32;

        rebuild_occupancy(&mut world)?;
        if world.occupancy.count() != occupied {
            return Err(SaveError::Corrupt("the occupied cells do not add up"));
        }
        Ok(world)
    }
}

/// Claims the footprint of every structure again, in slot order.
fn rebuild_occupancy(world: &mut World) -> SaveResult<()> {
    let mut occupancy = Occupancy::new(world.config.width, world.config.height);
    for i in 0..world.entities.len() {
        if !world.entities.alive[i] || world.entities.cell[i] == NO_CELL {
            continue;
        }
        let anchor = occupancy.cell(world.entities.cell[i]);
        let footprint = world.footprint_at(i);
        for cell in footprint_cells(anchor, footprint) {
            let Some(index) = occupancy.index(cell) else {
                return Err(SaveError::Corrupt("a footprint leaves the map"));
            };
            if !occupancy.claim(index, i as u32) {
                return Err(SaveError::Corrupt("two structures on one cell"));
            }
        }
    }
    world.occupancy = occupancy;
    Ok(())
}

#[cfg(test)]
mod tests;
