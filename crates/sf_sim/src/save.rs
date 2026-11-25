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
//! * Sections, each a tag byte and a `u32` length: `W` the world scalars, `E` the entities (with
//!   the weapon slots, the engagement memory and the kills, WP13.1c-1), `Q` the command queue, `X`
//!   the projectiles and `D` the wrecks and the tally (WP13.1c-1), `P` the path service (WP13.1b),
//!   `F` the flowfields, `M` the formations, then (WP13.1c-2) `O` the ore, `H` the harvesters,
//!   docks and stores, `C` the money spent, `U` the factories and `I` the power totals, then
//!   (WP13.1c-3a) `G` the match outcome, in that order. The construction state (`build_left`) and
//!   the repair and sale state (`repairing`, `sell_left`, WP13.1c-3a) are columns of `E`.
//!
//! # Not yet saved
//!
//! Vision comes in WP13.1c-3b; until then [`World::save`] refuses, with [`SaveError::Unsupported`]
//! naming the section, any world in which it holds something that is not its tick-0 state; the
//! exact test per section is in `docs/rules/save.md`. A world that saves therefore restores whole.
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
use crate::crowd::{Claim, CrowdState, HOME_WAIT, SHOVE_CALM, SHOVE_LIMIT, SIDESTEP_TICKS};
use crate::entities::{Entities, NO_DEF};
use crate::formation::{Member, MemberMode};
use crate::ids::{EntityId, Kind, PlayerId, Tick};
use crate::orders::{GuardTarget, ORDER_QUEUE_CAP, Order, OrderQueue};
use crate::targeting::Engage;
use crate::terrain::{Cell, NO_CELL, Occupancy};
use crate::units::UnitClass;
use crate::weapons::{REST, SlotState, WeaponState};
use crate::world::{World, footprint_cells};

/// The first bytes of every save file.
pub const MAGIC: [u8; 4] = *b"SFSV";

/// The layout of the bytes after the magic. Raised whenever that layout changes; an older save is
/// refused with [`SaveError::Version`] (no migration yet).
pub const FORMAT_VERSION: u32 = 5;

const SECTION_WORLD: u8 = b'W';
const SECTION_ENTITIES: u8 = b'E';
const SECTION_QUEUE: u8 = b'Q';
const SECTION_PROJECTILES: u8 = b'X';
const SECTION_DESTRUCTION: u8 = b'D';
const SECTION_PATHING: u8 = b'P';
const SECTION_FIELDS: u8 = b'F';
const SECTION_FORMATIONS: u8 = b'M';
const SECTION_ORE: u8 = b'O';
const SECTION_HARVEST: u8 = b'H';
const SECTION_MONEY: u8 = b'C';
const SECTION_PRODUCTION: u8 = b'U';
const SECTION_POWER: u8 = b'I';
const SECTION_OUTCOME: u8 = b'G';
/// The sections of this format, in file order. `D` comes before `P`: the path service's restore
/// check of the job's epoch counts the rubble of the last tick (WP13.1c-1). `O` comes before `H`:
/// a harvester's claimed cell must be an ore field. The sections of a rule set the world does not
/// have are empty (the rules digest says which it has).
const SECTIONS: [u8; 14] = [
    SECTION_WORLD,
    SECTION_ENTITIES,
    SECTION_QUEUE,
    SECTION_PROJECTILES,
    SECTION_DESTRUCTION,
    SECTION_PATHING,
    SECTION_FIELDS,
    SECTION_FORMATIONS,
    SECTION_ORE,
    SECTION_HARVEST,
    SECTION_MONEY,
    SECTION_PRODUCTION,
    SECTION_POWER,
    SECTION_OUTCOME,
];

/// What a world holds of the sections WP13.1b saves: for tests that save in the middle of a run
/// and want to show the state they claim to carry across is there (a route being planned, a
/// flowfield, a formation, a unit half way through a turn).
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct SaveContents {
    /// A route is being planned and is not finished: the path service's job is in flight.
    pub path_job: bool,
    /// Live slots whose route is not idle.
    pub routes: u32,
    /// Live slots whose route request carries an overlay (a stuck unit's detour, WP4.7).
    pub overlays: u32,
    /// Flowfields in the pool.
    pub fields: u32,
    /// A flowfield is being built.
    pub field_building: bool,
    /// Formations alive.
    pub formations: u32,
    /// Live units that carry part of a turn step to the next tick (`turn_carry != 0`).
    pub turning: u32,
    /// Projectiles in flight (WP13.1c-1).
    pub projectiles: u32,
    /// Weapon slots of live entities that are reloading (`wait > 0`) outside a burst.
    pub reloading: u32,
    /// Weapon slots of live entities in the middle of a burst (`burst_left > 0`).
    pub bursting: u32,
    /// Live units on a target they chose themselves (an engagement, `Pursue`).
    pub engaged: u32,
    /// Live armed structures with a target (a defence).
    pub defending: u32,
    /// Live entities whose engagement memory is not at rest (an engagement, a held anchor, a
    /// scan cooldown).
    pub engage_memory: u32,
    /// Wrecks and rubble lying on the map.
    pub wrecks: u32,
    /// Players with a tally.
    pub tallies: u32,
    /// Live entities with kills.
    pub killers: u32,
    /// The last tick's reports of the weapon stage are not empty (shots, impacts, attacks, damage
    /// records): derived, not saved (the next weapon stage clears them first).
    pub reports: bool,
    /// The job in flight is behind the path service's epoch (a structure was destroyed after the
    /// service ran).
    pub job_behind: bool,
    /// The ore changed since the start (WP13.1c-2).
    pub ore_changed: bool,
    /// Harvesters the harvest stage has registered.
    pub harvesters: u32,
    /// Harvesters driving to ore or mining.
    pub harvesting: u32,
    /// Harvesters driving to a refinery, called, queued or granted.
    pub returning: u32,
    /// Harvesters in a bay, being unloaded.
    pub unloading: u32,
    /// Harvesters parked or waiting for a refinery.
    pub harvest_waiting: u32,
    /// Harvesters with a load.
    pub carrying: u32,
    /// Harvesters in a dock queue.
    pub dock_queued: u32,
    /// Messages in dock inboxes.
    pub dock_inbox: u32,
    /// Dock bays granted and not yet reached.
    pub bay_reserved: u32,
    /// Some player has had ore delivered.
    pub delivered: bool,
    /// Some player has something in store.
    pub stored: bool,
    /// Some player has spent money.
    pub spent: bool,
    /// Structures under construction.
    pub constructing: u32,
    /// Factory records.
    pub factories: u32,
    /// Factories whose front item has progress.
    pub producing: u32,
    /// Factories whose queue is at its limit.
    pub queue_full: u32,
    /// Factories with a rally point.
    pub rallies: u32,
    /// Factories that repeat.
    pub repeating: u32,
    /// Factories with a free unit queued.
    pub free_items: u32,
    /// Players with less power than they use (a speed below full).
    pub power_low: u32,
    /// Players whose defences have no power.
    pub unpowered: u32,
    /// Players with power totals.
    pub power_players: u32,
    /// The last step's reports of construction, placement, production and repair are not empty
    /// (completions, stalled builds, refused placements, production and repair commands, units put
    /// out, repairs waiting for money, sales finished): derived, not saved.
    pub economy_reports: bool,
    /// Live structures being repaired (WP13.1c-3a).
    pub repairing: u32,
    /// Live structures being sold.
    pub selling: u32,
    /// The match has begun.
    pub match_begun: bool,
    /// Players defeated.
    pub defeats: u32,
    /// The match is decided (won or drawn).
    pub decided: bool,
}

impl World {
    /// What the world holds of the pathing, flowfield, formation and turning state
    /// ([`SaveContents`]).
    pub fn save_contents(&self) -> SaveContents {
        let e = &self.entities;
        let live = |i: &usize| e.alive[*i];
        let (fields, field_building) = self.pathing.field_counts();
        SaveContents {
            path_job: self.pathing.has_job(),
            routes: (0..e.len())
                .filter(live)
                .filter(|&i| e.route[i].state != crate::path::RouteState::Idle)
                .count() as u32,
            overlays: (0..e.len())
                .filter(live)
                .filter(|&i| !e.route[i].overlay.is_empty())
                .count() as u32,
            fields,
            field_building,
            formations: self.formations.count() as u32,
            turning: (0..e.len())
                .filter(live)
                .filter(|&i| e.turn_carry[i] != 0)
                .count() as u32,
            projectiles: self.arsenal.projectiles.len() as u32,
            reloading: (0..e.len())
                .filter(live)
                .flat_map(|i| e.weapon[i].iter())
                .filter(|s| s.burst_left == 0 && s.wait > 0)
                .count() as u32,
            bursting: (0..e.len())
                .filter(live)
                .flat_map(|i| e.weapon[i].iter())
                .filter(|s| s.burst_left > 0)
                .count() as u32,
            engaged: (0..e.len())
                .filter(live)
                .filter(|&i| matches!(e.order[i], Order::Pursue(_)))
                .count() as u32,
            defending: (0..e.len())
                .filter(live)
                .filter(|&i| {
                    e.def[i] != NO_DEF
                        && self.units.at(e.def[i] as usize).class() == UnitClass::Structure
                        && self.arsenal.is_armed(e.def[i])
                        && e.turret_target[i].is_some()
                })
                .count() as u32,
            engage_memory: (0..e.len())
                .filter(live)
                .filter(|&i| e.eng[i] != Engage::REST)
                .count() as u32,
            wrecks: self.destruction.wrecks.len() as u32,
            tallies: self.destruction.tally.len() as u32,
            killers: (0..e.len())
                .filter(live)
                .filter(|&i| e.kills[i] > 0)
                .count() as u32,
            reports: !self.arsenal.shots.is_empty()
                || !self.arsenal.impacts.is_empty()
                || !self.arsenal.attacks.is_empty()
                || !self.arsenal.hurt.is_empty(),
            job_behind: self.pathing.job_behind(),
            ..self.economy_contents()
        }
    }

    /// The part of [`SaveContents`] of WP13.1c-2 (the rest at its defaults).
    fn economy_contents(&self) -> SaveContents {
        let e = &self.entities;
        let h = self
            .harvest
            .as_deref()
            .map(|h| h.counts())
            .unwrap_or_default();
        let p = self.production.as_deref();
        let limit = p.map_or(0, |p| usize::from(p.rules.queue_limit));
        let factories = p.map_or(&[][..], |p| &p.factories[..]);
        let count = |f: &dyn Fn(&crate::production::Factory) -> bool| {
            factories.iter().filter(|x| f(x)).count() as u32
        };
        let players: Vec<PlayerId> = self
            .power
            .as_ref()
            .map_or_else(Vec::new, |p| p.totals.iter().map(|&(q, _)| q).collect());
        SaveContents {
            ore_changed: self.ore.as_ref().is_some_and(|o| o.version() != 0),
            harvesters: h.harvesters,
            harvesting: h.harvesting,
            returning: h.returning,
            unloading: h.unloading,
            harvest_waiting: h.waiting,
            carrying: h.carrying,
            dock_queued: h.queued,
            dock_inbox: h.inbox,
            bay_reserved: h.reserved,
            delivered: h.delivered,
            stored: h.stored,
            spent: self
                .money
                .as_ref()
                .is_some_and(|m| m.spent.iter().any(|&(_, n)| n > 0)),
            constructing: (0..e.len())
                .filter(|&i| e.alive[i] && e.build_left[i] > 0)
                .count() as u32,
            factories: factories.len() as u32,
            producing: count(&|f| f.done > 0),
            queue_full: count(&|f| f.items.len() >= limit),
            rallies: count(&|f| f.rally.is_some()),
            repeating: count(&|f| f.repeat),
            free_items: count(&|f| f.items.iter().any(|i| i.free)),
            power_low: players
                .iter()
                .filter(|&&q| self.power_speed_permille(q) < crate::production::FULL_SPEED_PERMILLE)
                .count() as u32,
            unpowered: players
                .iter()
                .filter(|&&q| !self.defences_powered(q))
                .count() as u32,
            power_players: players.len() as u32,
            economy_reports: !self.completions.is_empty()
                || !self.stalled.is_empty()
                || !self.placement.rejections.is_empty()
                || p.is_some_and(|p| !p.rejections.is_empty() || !p.produced.is_empty())
                || self.repair.as_deref().is_some_and(|r| {
                    !r.rejections.is_empty() || !r.waiting.is_empty() || !r.sold.is_empty()
                }),
            repairing: (0..e.len())
                .filter(|&i| e.alive[i] && e.repairing[i])
                .count() as u32,
            selling: (0..e.len())
                .filter(|&i| e.alive[i] && e.sell_left[i] > 0)
                .count() as u32,
            match_begun: self.match_begun(),
            defeats: self.defeats().len() as u32,
            decided: self.match_result().is_some(),
            ..SaveContents::default()
        }
    }

    /// The saved fields of the sections of WP13.1c-2 and WP13.1c-3a that hold something other than their default
    /// in this world, by name: the coverage record (a field no scenario ever saves non-default gets
    /// a targeted round trip of its own, `docs/rules/save.md`).
    #[doc(hidden)]
    pub fn save_coverage(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        let add = |out: &mut Vec<&'static str>, name: &'static str, on: bool| {
            if on && !out.contains(&name) {
                out.push(name);
            }
        };
        if let Some(o) = &self.ore {
            add(
                &mut out,
                "ore.amount",
                o.total() != o.rules().start as u64 * o.field_count() as u64,
            );
            add(&mut out, "ore.version", o.version() != 0);
        }
        if let Some(h) = self.harvest.as_deref() {
            h.coverage(&mut out);
        }
        if let Some(m) = &self.money {
            add(&mut out, "spent", m.spent.iter().any(|&(_, n)| n > 0));
        }
        let e = &self.entities;
        add(
            &mut out,
            "build_left",
            (0..e.len()).any(|i| e.alive[i] && e.build_left[i] > 0),
        );
        // WP13.1c-3a.
        add(
            &mut out,
            "repairing",
            (0..e.len()).any(|i| e.alive[i] && e.repairing[i]),
        );
        add(
            &mut out,
            "sell_left",
            (0..e.len()).any(|i| e.alive[i] && e.sell_left[i] > 0),
        );
        if let Some(o) = self.outcome.as_deref() {
            outcome::coverage(o, &mut out);
        }
        if let Some(p) = self.production.as_deref() {
            for f in &p.factories {
                add(&mut out, "factory", true);
                add(&mut out, "factory.rally", f.rally.is_some());
                add(&mut out, "factory.repeat", f.repeat);
                add(&mut out, "factory.items", !f.items.is_empty());
                add(
                    &mut out,
                    "factory.item.free",
                    f.items.iter().any(|i| i.free),
                );
                add(&mut out, "factory.done", f.done > 0);
                use crate::production::FactoryStatus as S;
                let status = match f.status {
                    S::Idle => "factory.status.idle",
                    S::Building => "factory.status.building",
                    S::WaitingForMoney => "factory.status.waiting_for_money",
                    S::WaitingForExit => "factory.status.waiting_for_exit",
                    S::MissingPrerequisite(_) => "factory.status.missing_prerequisite",
                    S::Stopped => "factory.status.stopped",
                    S::NotStanding => "factory.status.not_standing",
                };
                add(&mut out, status, true);
            }
        }
        if let Some(p) = &self.power {
            add(
                &mut out,
                "power.made",
                p.totals.iter().any(|t| t.1.made > 0),
            );
            add(
                &mut out,
                "power.used",
                p.totals.iter().any(|t| t.1.used > 0),
            );
        }
        out
    }
}

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

pub(crate) type SaveResult<T> = Result<T, SaveError>;

/// `Ok` when `ok`, else [`SaveError::Corrupt`] with `what`.
pub(crate) fn ensure(ok: bool, what: &'static str) -> SaveResult<()> {
    if ok {
        Ok(())
    } else {
        Err(SaveError::Corrupt(what))
    }
}

/// Whether every chain of parents ends: following `next` from any element reaches an element
/// with none (`None`) without going round. `next` returns indices into `parent`.
pub(crate) fn parents_end<T: Copy>(parent: &[T], next: impl Fn(T) -> Option<usize>) -> bool {
    // 0 not looked at, 1 on the chain being followed, 2 known to end.
    let mut mark = vec![0u8; parent.len()];
    let mut chain = Vec::new();
    for start in 0..parent.len() {
        let mut at = start;
        loop {
            match mark[at] {
                2 => break,
                1 => return false,
                _ => {}
            }
            mark[at] = 1;
            chain.push(at);
            match next(parent[at]) {
                Some(n) if n < parent.len() => at = n,
                Some(_) => return false,
                None => break,
            }
        }
        for &c in &chain {
            mark[c] = 2;
        }
        chain.clear();
    }
    true
}

/// A slice of `u32`s: the count, then each.
pub(crate) fn put_u32s(w: &mut ByteWriter, v: &[u32]) -> Result<(), ByteError> {
    w.usize(v.len())?;
    for &x in v {
        w.u32(x);
    }
    Ok(())
}

pub(crate) fn get_u32s(r: &mut ByteReader<'_>) -> SaveResult<Vec<u32>> {
    let n = r.count(4)?;
    (0..n).map(|_| Ok(r.u32()?)).collect()
}

/// A slice of `u64`s: the count, then each.
pub(crate) fn put_u64s(w: &mut ByteWriter, v: &[u64]) -> Result<(), ByteError> {
    w.usize(v.len())?;
    for &x in v {
        w.u64(x);
    }
    Ok(())
}

pub(crate) fn get_u64s(r: &mut ByteReader<'_>) -> SaveResult<Vec<u64>> {
    let n = r.count(8)?;
    (0..n).map(|_| Ok(r.u64()?)).collect()
}

/// Pairs of `u32`s (a node and a cost): the count, then each.
pub(crate) fn put_links(w: &mut ByteWriter, v: &[(u32, u32)]) -> Result<(), ByteError> {
    w.usize(v.len())?;
    for &(a, b) in v {
        w.u32(a);
        w.u32(b);
    }
    Ok(())
}

pub(crate) fn get_links(r: &mut ByteReader<'_>) -> SaveResult<Vec<(u32, u32)>> {
    let n = r.count(8)?;
    (0..n).map(|_| Ok((r.u32()?, r.u32()?))).collect()
}

/// A slice of bytes: the count, then each.
pub(crate) fn put_u8s(w: &mut ByteWriter, v: &[u8]) -> Result<(), ByteError> {
    w.bytes(v)
}

pub(crate) fn get_u8s(r: &mut ByteReader<'_>) -> SaveResult<Vec<u8>> {
    Ok(r.bytes()?.to_vec())
}

/// A slice of flags, one byte each.
pub(crate) fn put_bools(w: &mut ByteWriter, v: &[bool]) -> Result<(), ByteError> {
    w.usize(v.len())?;
    for &x in v {
        w.bool(x);
    }
    Ok(())
}

pub(crate) fn get_bools(r: &mut ByteReader<'_>) -> SaveResult<Vec<bool>> {
    let n = r.count(1)?;
    (0..n).map(|_| Ok(r.bool()?)).collect()
}

// ---- primitives of the simulation ---------------------------------------------------------------

/// Per-player totals, ascending by player (the money, store, delivery and power lists): a count,
/// then each player and number.
pub(crate) fn put_pairs(w: &mut ByteWriter, v: &[(PlayerId, u64)]) -> Result<(), ByteError> {
    w.usize(v.len())?;
    for &(player, n) in v {
        w.u8(player.0);
        w.u64(n);
    }
    Ok(())
}

/// Per-player totals; refused unless the players are strictly ascending (every such list is kept
/// sorted for a binary search, and a player twice would split a total).
pub(crate) fn get_pairs(r: &mut ByteReader<'_>) -> SaveResult<Vec<(PlayerId, u64)>> {
    let n = r.count(9)?;
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push((PlayerId(r.u8()?), r.u64()?));
    }
    ensure(
        v.windows(2).all(|p| p[0].0 < p[1].0),
        "player totals out of order",
    )?;
    Ok(v)
}

pub(crate) fn put_fx(w: &mut ByteWriter, v: Fx) {
    w.i64(v.raw());
}

pub(crate) fn get_fx(r: &mut ByteReader<'_>) -> SaveResult<Fx> {
    Ok(Fx::from_raw(r.i64()?))
}

pub(crate) fn put_vec2(w: &mut ByteWriter, v: Vec2) {
    put_fx(w, v.x);
    put_fx(w, v.y);
}

pub(crate) fn get_vec2(r: &mut ByteReader<'_>) -> SaveResult<Vec2> {
    Ok(Vec2 {
        x: get_fx(r)?,
        y: get_fx(r)?,
    })
}

pub(crate) fn put_opt_vec2(w: &mut ByteWriter, v: Option<Vec2>) {
    w.bool(v.is_some());
    if let Some(v) = v {
        put_vec2(w, v);
    }
}

pub(crate) fn get_opt_vec2(r: &mut ByteReader<'_>) -> SaveResult<Option<Vec2>> {
    Ok(if r.bool()? { Some(get_vec2(r)?) } else { None })
}

pub(crate) fn put_id(w: &mut ByteWriter, id: EntityId) {
    w.u32(id.index());
    w.u32(id.generation());
}

pub(crate) fn get_id(r: &mut ByteReader<'_>) -> SaveResult<EntityId> {
    Ok(EntityId::new(r.u32()?, r.u32()?))
}

pub(crate) fn put_opt_id(w: &mut ByteWriter, id: Option<EntityId>) {
    w.bool(id.is_some());
    if let Some(id) = id {
        put_id(w, id);
    }
}

pub(crate) fn get_opt_id(r: &mut ByteReader<'_>) -> SaveResult<Option<EntityId>> {
    Ok(if r.bool()? { Some(get_id(r)?) } else { None })
}

pub(crate) fn put_ids(w: &mut ByteWriter, ids: &[EntityId]) -> Result<(), ByteError> {
    w.usize(ids.len())?;
    for &id in ids {
        put_id(w, id);
    }
    Ok(())
}

pub(crate) fn get_ids(r: &mut ByteReader<'_>) -> SaveResult<Vec<EntityId>> {
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
pub(crate) fn put_order(w: &mut ByteWriter, o: Order) {
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

pub(crate) fn get_order(r: &mut ByteReader<'_>) -> SaveResult<Order> {
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

/// The point an order walks to, if it names one.
pub(crate) fn order_point(o: Order) -> Option<Vec2> {
    match o {
        Order::Move(p) | Order::AttackMove(p) | Order::Guard(GuardTarget::Point(p)) => Some(p),
        _ => None,
    }
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

/// A unit's formation memory, every field (the "worked out each tick" ones too, so that a
/// restored world is equal by `PartialEq` and a resave byte-equal). Destructured without `..`.
fn put_member(w: &mut ByteWriter, m: &Member) {
    let Member {
        serial,
        off,
        lag,
        far,
        mode,
        slot,
        step,
        heading,
    } = m;
    w.u32(*serial);
    put_vec2(w, *off);
    w.u16(*lag);
    put_fx(w, *far);
    w.u8(*mode as u8);
    put_vec2(w, *slot);
    put_fx(w, *step);
    w.u8(heading.raw());
}

fn get_member(r: &mut ByteReader<'_>) -> SaveResult<Member> {
    Ok(Member {
        serial: r.u32()?,
        off: get_vec2(r)?,
        lag: r.u16()?,
        far: get_fx(r)?,
        mode: match r.u8()? {
            0 => MemberMode::None,
            1 => MemberMode::Lead,
            2 => MemberMode::Direct,
            3 => MemberMode::Flow,
            4 => MemberMode::Wait,
            _ => return Err(SaveError::Corrupt("unknown formation member mode")),
        },
        slot: get_vec2(r)?,
        step: get_fx(r)?,
        heading: Facing::new(r.u8()?),
    })
}

/// The weapon slots of a unit (WP13.1c-1): per slot the reload count and the shots left of a
/// burst. Destructured without `..`.
fn put_weapon(w: &mut ByteWriter, state: &WeaponState) {
    for slot in state {
        let SlotState { wait, burst_left } = slot;
        w.u32(*wait);
        w.u8(*burst_left);
    }
}

fn get_weapon(r: &mut ByteReader<'_>) -> SaveResult<WeaponState> {
    let mut state = REST;
    for slot in &mut state {
        slot.wait = r.u32()?;
        slot.burst_left = r.u8()?;
    }
    Ok(state)
}

/// A unit's engagement memory (WP13.1c-1), every field. Destructured without `..`.
fn put_engage(w: &mut ByteWriter, eng: &Engage) {
    let Engage {
        resume,
        chase,
        anchor,
        held,
        rest,
        aim,
        reach,
        cool,
    } = eng;
    w.bool(resume.is_some());
    if let Some(o) = resume {
        put_order(w, *o);
    }
    w.bool(*chase);
    put_vec2(w, *anchor);
    w.bool(*held);
    w.u16(*rest);
    put_vec2(w, *aim);
    put_fx(w, *reach);
    w.u16(*cool);
}

fn get_engage(r: &mut ByteReader<'_>) -> SaveResult<Engage> {
    Ok(Engage {
        resume: if r.bool()? { Some(get_order(r)?) } else { None },
        chase: r.bool()?,
        anchor: get_vec2(r)?,
        held: r.bool()?,
        rest: r.u16()?,
        aim: get_vec2(r)?,
        reach: get_fx(r)?,
        cool: r.u16()?,
    })
}

/// What the weapon stage and the targeting stage can leave in the weapon and engagement columns of
/// a unit (WP13.1c-1, `docs/rules/save.md`).
struct Combat<'a> {
    arsenal: &'a crate::weapons::Arsenal,
    /// The targeting settings when units choose targets: settings and a weapon table.
    aim: Option<crate::targeting::TargetingConfig>,
    destruction: bool,
}

impl Combat<'_> {
    /// Whether every weapon slot of an entity of table position `def` holds a state the weapon
    /// stage leaves: a slot without a weapon at rest; a slot with weapon `w` fewer shots left of
    /// its burst than the burst has (the first shot is taken off at once), and a reload count of at
    /// most what the last shot set: `burst_delay_ticks` inside a burst, `tail_ticks` after it.
    fn weapon_fits(&self, def: u32, state: &WeaponState) -> bool {
        state.iter().enumerate().all(|(s, slot)| {
            let weapon = (def != NO_DEF && self.arsenal.is_armed(def))
                .then(|| self.arsenal.weapons_of(def).nth(s))
                .flatten();
            match weapon {
                None => *slot == REST[s],
                Some(w) => {
                    slot.burst_left < w.burst
                        && slot.wait
                            <= if slot.burst_left > 0 {
                                w.burst_delay_ticks
                            } else {
                                w.tail_ticks()
                            }
                }
            }
        })
    }

    /// Whether the engagement memory `eng` of an entity of table position `def` (with `class`),
    /// on current order `order`, is one the targeting stage leaves. See `docs/rules/save.md`.
    fn engage_fits(
        &self,
        eng: &Engage,
        def: u32,
        class: Option<UnitClass>,
        order: Order,
        inside: &impl Fn(Vec2) -> bool,
    ) -> bool {
        let pursuing = matches!(order, Order::Pursue(_));
        let armed = def != NO_DEF && self.arsenal.is_armed(def);
        let Some(config) = self.aim.filter(|_| armed) else {
            // Nothing runs the targeting stage for this entity: its memory stays at rest.
            return *eng == Engage::REST && !pursuing;
        };
        let reach_of_a_weapon = || self.arsenal.weapons_of(def).any(|w| w.range == eng.reach);
        let Engage {
            resume,
            chase,
            anchor,
            held,
            rest,
            aim,
            reach,
            cool,
        } = *eng;
        let common = cool <= config.scan_cooldown && inside(anchor) && inside(aim);
        match class {
            Some(UnitClass::Mobile) => {
                let engaged = match resume {
                    // An engagement: the order it goes back to is one a unit may scan from, the
                    // weapon that chose fixed the reach, the anchor binds and the unit is not at
                    // rest.
                    Some(o) => {
                        pursuing
                            && matches!(
                                o,
                                Order::Idle
                                    | Order::HoldPosition
                                    | Order::Guard(_)
                                    | Order::AttackMove(_)
                            )
                            && order_point(o).is_none_or(inside)
                            && reach_of_a_weapon()
                            && held
                            && rest == 0
                    }
                    // Not engaged: `forget` cleared what the engagement held.
                    None => !pursuing && !chase && aim == Vec2::ZERO && reach == Fx::ZERO,
                };
                // `rest` counts up to `pursuit_reset` and starts again there, only while `held`;
                // dropping `held` clears the anchor.
                let leash =
                    rest < config.pursuit_reset && (held || (rest == 0 && anchor == Vec2::ZERO));
                common && engaged && leash
            }
            Some(UnitClass::Structure) => {
                // A defence takes no order: its target is the turret target, its memory the scan
                // cooldown and where and with what reach it last chose.
                !pursuing
                    && resume.is_none()
                    && !chase
                    && !held
                    && rest == 0
                    && anchor == Vec2::ZERO
                    && (reach == Fx::ZERO || reach_of_a_weapon())
                    && common
            }
            _ => *eng == Engage::REST && !pursuing,
        }
    }
}

fn save_entities(w: &mut ByteWriter, e: &Entities) -> SaveResult<()> {
    // Guard: every column is named. A new column fails to compile until it is saved or derived.
    let Entities {
        generation,
        alive,
        kind,
        owner,
        pos,
        // Saved (WP13.1b): the position before the last step. Not hashed, but the formation pass
        // reads it (how far the leader moved) before the movement stage sets it again.
        prev_pos,
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
        // Saved in the pathing section (the length of the column is state of the service).
        route: _,
        crowd,
        turn_carry,
        turret,
        // Derived like `prev_pos`.
        prev_turret: _,
        turret_target,
        form,
        // Saved (WP13.1c-1).
        weapon,
        eng,
        kills,
        // Saved (WP13.1c-2): the construction ticks left.
        build_left,
        // Saved (WP13.1c-3a): the repair flag and the sale countdown.
        repairing,
        sell_left,
    } = e;
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
        put_vec2(w, prev_pos[i]);
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
        w.u16(turn_carry[i]);
        put_member(w, &form[i]);
        put_weapon(w, &weapon[i]);
        put_engage(w, &eng[i]);
        w.u16(kills[i]);
        w.u32(build_left[i]);
        w.bool(repairing[i]);
        w.u32(sell_left[i]);
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
    // The closed rectangle: the far corner of a wander area is the map corner itself.
    let inside = |p: Vec2| p.x >= Fx::ZERO && p.y >= Fx::ZERO && p.x <= max_x && p.y <= max_y;
    let replan = world.pathing.replan_config();
    let limits = CrowdLimits {
        tick: world.tick,
        stuck_ticks: world.crowd.config.stuck_ticks,
        stuck_attempts: world.crowd.config.stuck_attempts,
        replan_max: replan.max,
        replan_cooldown: replan.cooldown,
        retarget_cooldown: replan.retarget_cooldown,
    };
    let combat = Combat {
        arsenal: &world.arsenal,
        // As `World::step` runs the targeting stage: settings and a weapon table.
        aim: world.targeting.config.filter(|_| !world.arsenal.is_empty()),
        destruction: world.destruction.config.is_some(),
    };
    let sell_ticks = world.repair.as_ref().map(|r| r.rules.sell_ticks);
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
        e.prev_pos[i] = get_vec2(r)?;
        if !inside(e.prev_pos[i]) {
            return Err(SaveError::Corrupt("an entity stood outside the map"));
        }
        e.facing[i] = Facing::new(r.u8()?);
        e.prev_facing[i] = e.facing[i];
        e.behavior[i] = get_behavior(r)?;
        e.target[i] = get_opt_vec2(r)?;
        e.area[i] = get_area(r)?;
        if e.target[i].is_some_and(|t| !inside(t))
            || !inside(e.area[i].min)
            || !inside(e.area[i].max)
        {
            return Err(SaveError::Corrupt(
                "a target or wander area lies outside the map",
            ));
        }
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
        let stats = (e.def[i] != NO_DEF).then(|| *world.units.at(e.def[i] as usize));
        e.hp[i] = r.u32()?;
        if e.hp[i] > stats.map_or(0, |s| s.hp) {
            return Err(SaveError::Corrupt(
                "an entity has more hit points than its kind",
            ));
        }
        // A table structure stands on its anchor cell and nothing else of the table does.
        if let Some(s) = stats
            && s.footprint.is_some() != (e.cell[i] != NO_CELL)
        {
            return Err(SaveError::Corrupt(
                "a structure without a cell, or a cell without a structure",
            ));
        }
        e.order[i] = get_order(r)?;
        // `begin` turns a scatter into a move. (A pursuit is current only with an engagement:
        // checked with the engagement memory below.)
        if e.order[i] == Order::Scatter {
            return Err(SaveError::Corrupt(
                "a current order the simulation never holds",
            ));
        }
        let queued = usize::from(r.u8()?);
        if queued > ORDER_QUEUE_CAP {
            return Err(SaveError::Corrupt("an order queue is longer than the ring"));
        }
        let mut q = OrderQueue::EMPTY;
        for _ in 0..queued {
            q.push_back(get_order(r)?);
        }
        e.queue[i] = q;
        if core::iter::once(e.order[i])
            .chain(e.queue[i].iter())
            .any(|o| order_point(o).is_some_and(|p| !inside(p)))
        {
            return Err(SaveError::Corrupt("an order point lies outside the map"));
        }
        // What cannot move (a structure, an object) has no behaviour and no order.
        let immobile = e.cell[i] != NO_CELL || stats.is_some_and(|s| s.movement_class.is_none());
        if immobile
            && (e.behavior[i] != Behavior::Idle
                || e.order[i] != Order::Idle
                || !e.queue[i].is_empty())
        {
            return Err(SaveError::Corrupt(
                "an entity that cannot move has a behaviour or an order",
            ));
        }
        e.guard_walking[i] = r.bool()?;
        e.turret[i] = Facing::new(r.u8()?);
        e.prev_turret[i] = e.turret[i];
        e.turret_target[i] = get_opt_id(r)?;
        e.crowd[i] = get_crowd(r)?;
        check_crowd(&e.crowd[i], &limits, &inside)?;
        // Thousandths of a turn step, always below one whole step (`turning::turn_carrying`).
        e.turn_carry[i] = r.u16()?;
        if e.turn_carry[i] >= TURN_CARRY_LIMIT {
            return Err(SaveError::Corrupt("a turn carry of a whole step or more"));
        }
        e.form[i] = get_member(r)?;
        e.weapon[i] = get_weapon(r)?;
        if !combat.weapon_fits(e.def[i], &e.weapon[i]) {
            return Err(SaveError::Corrupt(
                "a weapon slot the weapon stage never leaves",
            ));
        }
        e.eng[i] = get_engage(r)?;
        let class = stats.map(|s| s.class());
        if !combat.engage_fits(&e.eng[i], e.def[i], class, e.order[i], &inside) {
            return Err(SaveError::Corrupt(
                "an engagement the targeting stage never leaves",
            ));
        }
        e.kills[i] = r.u16()?;
        // Kill credit goes to a shooter that stands (an armed entity) in a world with destruction.
        if e.kills[i] != 0
            && !(combat.destruction && e.def[i] != NO_DEF && combat.arsenal.is_armed(e.def[i]))
        {
            return Err(SaveError::Corrupt("kills without a weapon or destruction"));
        }
        // Construction (WP13.1c-2): a structure the player built starts at the build ticks of its
        // building entry (`apply_from`, after `place_structure` gave it a table entry and a cell)
        // and only counts down (`advance_construction`).
        e.build_left[i] = r.u32()?;
        let most = world
            .buildings
            .get(e.kind[i])
            .filter(|_| e.def[i] != NO_DEF && e.cell[i] != NO_CELL)
            .map_or(0, |b| b.build_ticks);
        if e.build_left[i] > most {
            return Err(SaveError::Corrupt(
                "a construction the construction stage never leaves",
            ));
        }
        // Repair and sale (WP13.1c-3a, `docs/rules/save.md`). Both start only through
        // `repair_target` (`repair_command`, `sell_command`): a live table structure (`def`, a
        // cell) of a world with repair rules, complete, not being sold, standing. Nothing makes a
        // structure incomplete again, so `build_left` is zero while either runs. The hit points may
        // be zero: in a world without destruction a shot of the same tick (or of a later one, for
        // a sale) takes them there and nothing removes the structure.
        e.repairing[i] = r.bool()?;
        e.sell_left[i] = r.u32()?;
        let target = sell_ticks.is_some()
            && e.def[i] != NO_DEF
            && e.cell[i] != NO_CELL
            && e.build_left[i] == 0;
        // `repair_command` sets the flag only below full hit points of a kind that can be damaged;
        // `repair_step` clears it when it reaches them, and nothing else raises hit points.
        if e.repairing[i] && !(target && stats.is_some_and(|s| e.hp[i] < s.hp)) {
            return Err(SaveError::Corrupt("a repair the repair stage never leaves"));
        }
        // `sell_command` sets `sell_ticks` and clears the flag (and `repair_command` refuses a
        // structure being sold); `advance_repair_sell` counts down at the start of the next step
        // and removes the structure at zero. A sale ordered in the step before the save is still
        // at `sell_ticks`.
        if e.sell_left[i] != 0
            && !(target && !e.repairing[i] && sell_ticks.is_some_and(|most| e.sell_left[i] <= most))
        {
            return Err(SaveError::Corrupt("a sale the repair stage never leaves"));
        }
    }
    Ok(())
}

/// Refuses kills of live entities that add up, per player, to more than the player's tally: each
/// kill is counted in both (`World::reap`), and only a death or a new entity in the slot clears the
/// entity's count. A tally at its maximum (saturated) is not compared. Assumes an entity never
/// changes owner: its kills are summed under its owner now, the player whose tally counted them
/// (nothing changes an owner today; a rule that does must move or clear the kills).
fn check_kills(world: &World) -> SaveResult<()> {
    let e = &world.entities;
    let mut sums: BTreeMap<PlayerId, u64> = BTreeMap::new();
    for i in (0..e.len()).filter(|&i| e.alive[i] && e.kills[i] != 0) {
        *sums.entry(e.owner[i]).or_default() += u64::from(e.kills[i]);
    }
    for (player, sum) in sums {
        let tally = world.destruction.kills_of(player);
        ensure(
            tally == u32::MAX || sum <= u64::from(tally),
            "more kills than the tally",
        )?;
    }
    Ok(())
}

/// The turn carry stays below one step, in thousandths (`turning::turn_carrying`).
const TURN_CARRY_LIMIT: u16 = 1000;

/// The bounds the crowd pass keeps its counters in (`crowd.rs`, `orders.rs`, `path.rs`): a
/// counter outside them is no state the simulation reaches, and the next step could overflow it.
struct CrowdLimits {
    tick: Tick,
    stuck_ticks: u16,
    stuck_attempts: u8,
    replan_max: u8,
    replan_cooldown: u16,
    retarget_cooldown: u16,
}

/// Refuses crowd memory the simulation cannot reach.
fn check_crowd(c: &CrowdState, l: &CrowdLimits, inside: &impl Fn(Vec2) -> bool) -> SaveResult<()> {
    let CrowdState {
        // `quiet` is set back to zero as soon as it reaches `stuck_ticks`.
        quiet,
        attempts,
        // A distance, or `Fx::MAX` when there is no waypoint.
        best,
        mark,
        side,
        shoved,
        calm,
        // Flags: every value is reachable.
        replan: _,
        replans,
        cool,
        retarget,
        give_up: _,
        gave_up_at,
        idle_hit: _,
        // Saturating: every value is reachable.
        pressed: _,
        // Looked up with `get`, never indexed: a claim on no field is no claim.
        claim: _,
        home,
        home_wait,
        home_rest: _,
        disc,
        settle: _,
        fresh: _,
        walking: _,
        refused: _,
        stranded: _,
    } = c;
    let counters = *quiet < l.stuck_ticks
        && *attempts <= l.stuck_attempts
        && *calm < SHOVE_CALM
        && *replans <= l.replan_max
        && *cool <= l.replan_cooldown
        && *retarget <= l.retarget_cooldown
        && *home_wait <= HOME_WAIT
        && side.is_none_or(|(_, n)| (1..=SIDESTEP_TICKS).contains(&n));
    let amounts = *best >= Fx::ZERO
        && *shoved >= Fx::ZERO
        // `apply` clamps a push to the room left, but `clamp_length` rounds each component, so the
        // length it adds can be one raw unit over: the most `shoved` reaches is the limit plus that.
        && *shoved <= SHOVE_LIMIT + Fx::EPSILON
        && *disc >= Fx::ZERO
        && gave_up_at.is_none_or(|t| t <= l.tick);
    let points =
        mark.is_none_or(inside) && home.is_none_or(inside) && side.is_none_or(|(p, _)| inside(p));
    if counters && amounts && points {
        Ok(())
    } else {
        Err(SaveError::Corrupt(
            "a crowd counter or point the simulation cannot reach",
        ))
    }
}

// ---- the world ----------------------------------------------------------------------------------

/// Refuses a world whose subsystem state outside the entity columns is not its tick-0 state.
fn check_unsaved_sections(w: &World) -> SaveResult<()> {
    // The arsenal, the destruction and the targeting are saved (WP13.1c-1); their fields are named
    // in `Arsenal::save_parts`, `Destruction::save` and `Targeting::has_state`.
    if w.targeting.has_state() {
        return Err(SaveError::Unsupported("targeting"));
    }
    if w.crowd.has_state() {
        return Err(SaveError::Unsupported("crowd"));
    }
    // The ore, the harvest state, the money, production and power are saved (WP13.1c-2); their
    // fields are named in `Ore::save`, `Harvest::save`, `economy::save_money`,
    // `economy::save_production` and `economy::save_power`.
    if let Some(r) = &w.repair {
        let crate::repair::RepairSell {
            // Rules: in the rules digest.
            rules: _,
            // Reports for the host: not hashed, and no step reads them. `rejections` is cleared at
            // the start of `step`, `waiting` and `sold` at the start of `advance_repair_sell`,
            // before anything in the step writes or reads them; the state is the entity columns.
            rejections: _,
            waiting: _,
            sold: _,
        } = &**r;
    }
    let crate::placement::PlacementState {
        // Rules: in the rules digest.
        players: _,
        // The report of the last step: cleared at the start of the next.
        rejections: _,
    } = &w.placement;
    if w.vision.is_some() {
        return Err(SaveError::Unsupported("vision"));
    }
    // The outcome is saved (WP13.1c-3a, section `G`); its fields are named in `outcome::save`.
    Ok(())
}

impl World {
    /// A digest of the rules the world was made with: the configuration, the terrain and every
    /// table and setting the checksum hashes as rules. Two worlds built from the same match and
    /// assets have the same digest; a save only restores onto a world with the digest it was made
    /// with. The path, flowfield, replan, formation and turning settings (WP13.1b) are in it, and
    /// the numbers of the rule sets WP13.1c saves; the vision numbers (WP13.1c-3b) and the AI
    /// profiles (WP13.1d) join it with them, and a world with vision rules does not save yet.
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
        h.write_u64(self.arsenal.table.rules_digest());
        h.write_u64(self.buildings.digest());
        h.write_u64(self.buildings.placement_digest());
        h.write_u64(self.placement.players.len() as u64);
        for &(player, bit) in &self.placement.players {
            h.write_u8(player.0);
            h.write_u8(bit);
        }
        // Destructured without `..`: a new setting fails to compile until it is hashed.
        let crate::crowd::CollisionConfig {
            enabled,
            wait_ticks,
            stuck_ticks,
            stuck_attempts,
        } = self.crowd.config;
        h.write_u8(u8::from(enabled));
        h.write_u32(u32::from(wait_ticks));
        h.write_u32(u32::from(stuck_ticks));
        h.write_u32(u32::from(stuck_attempts));
        let crate::formation::FormationConfig {
            enabled,
            min_units,
            max_units,
            spacing_permille,
            spread_permille,
            pace_permille,
            catchup_permille,
            reform_tenths,
            patience_ticks,
            ahead_tenths,
        } = self.formations.config;
        h.write_u8(u8::from(enabled));
        for v in [
            min_units,
            max_units,
            spacing_permille,
            spread_permille,
            pace_permille,
            catchup_permille,
            reform_tenths,
            patience_ticks,
            ahead_tenths,
        ] {
            h.write_u32(u32::from(v));
        }
        h.write_u32(self.pathing.budget());
        h.write_u32(self.pathing.flowfield_threshold());
        // Destructured without `..`: a new replan setting fails to compile until it is hashed.
        let crate::path::ReplanConfig {
            cooldown,
            max,
            retarget_cooldown,
        } = self.pathing.replan_config();
        h.write_u32(u32::from(cooldown));
        h.write_u32(u32::from(max));
        h.write_u32(u32::from(retarget_cooldown));
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
        // The optional rule sets that a world which saves may have: their numbers (WP13.1c-2: the
        // ore rules and the number of field cells, every harvest number, the harvester kinds, the
        // storage table and the price, the money, production and power rules), each destructured
        // without `..` by its `write_rules`. The start amounts follow from the ore rules and the
        // terrain; the starting credits are state (the stores in section `H`).
        match &self.ore {
            Some(ore) => {
                h.write_u8(1);
                ore.write_rules(&mut h);
            }
            None => h.write_u8(0),
        }
        match self.harvest.as_deref() {
            Some(hv) => {
                h.write_u8(1);
                hv.write_rules(&mut h);
            }
            None => h.write_u8(0),
        }
        match &self.money {
            Some(m) => {
                h.write_u8(1);
                economy::write_money_rules(m, &mut h);
            }
            None => h.write_u8(0),
        }
        match self.production.as_deref() {
            Some(p) => {
                h.write_u8(1);
                economy::write_production_rules(p, &mut h);
            }
            None => h.write_u8(0),
        }
        match &self.power {
            Some(p) => {
                h.write_u8(1);
                economy::write_power_rules(p, &mut h);
            }
            None => h.write_u8(0),
        }
        match &self.repair {
            Some(rs) => {
                // Destructured without `..`: a new repair rule fails to compile until it is hashed.
                let crate::repair::RepairRules {
                    hp_per_tick,
                    full_cost_permille,
                    sell_ticks,
                    refund_permille,
                } = rs.rules;
                h.write_u8(1);
                h.write_u32(u32::from(hp_per_tick));
                h.write_u32(full_cost_permille);
                h.write_u32(sell_ticks);
                h.write_u32(refund_permille);
            }
            None => h.write_u8(0),
        }
        // The match rules (WP13.1c-3a): the number of players and each player, ascending
        // (`Outcome::write_rules`).
        match self.outcome.as_deref() {
            Some(o) => {
                h.write_u8(1);
                o.write_rules(&mut h);
            }
            None => h.write_u8(0),
        }
        // Whether the world has vision rules: a world with them does not save yet; their numbers
        // join in WP13.1c-3b.
        h.write_u8(u8::from(self.vision.is_some()));
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
            // Saved in sections `P` and `F`; the graphs are derived (rebuilt from the structures)
            // and the settings rules. Its fields are named in `Pathing::save`.
            pathing,
            // Rules (settings and shapes) and scratch; the state is the entity column. Its fields
            // are named in `Crowd::has_state`.
            crowd: _,
            // Saved in section `M`; the settings are rules. Fields named in `Formations::save`.
            formations,
            // Saved in section `X` (the projectiles); the table and what is derived from it are
            // rules, the reports of the last tick derived. Its fields are named in
            // `Arsenal::save_parts`.
            arsenal,
            // Rules.
            buildings: _,
            // Saved in sections `O`, `H` and `C` (WP13.1c-2); the rules in the rules digest. Fields
            // named in `Ore::save`, `Harvest::save` and `economy::save_money`.
            ore,
            harvest,
            money,
            // A report of the last tick, not hashed: `advance_construction` clears it before
            // anything in the step reads it.
            stalled: _,
            // Saved in sections `U` and `I` (WP13.1c-2). Fields named in
            // `economy::save_production` and `economy::save_power`.
            production,
            power,
            // Rules and the reports of the last tick; the state is the entity columns.
            repair: _,
            // Gated (WP13.1c-3b).
            vision: _,
            // Saved in section `G` (WP13.1c-3a); the players are rules. Fields named in
            // `outcome::save`.
            outcome,
            // Rules (who is which faction) and the report of the last tick.
            placement: _,
            // A report of the last tick: `advance_construction` clears it before anything in the
            // step reads it (production reads the completions of its own step).
            completions: _,
            // Rules and scratch; the state is the entity column. Fields named in
            // `Targeting::has_state`.
            targeting: _,
            // Saved in section `D` (wrecks and tally); the settings are rules, the deaths the
            // report of the last tick. Fields named in `Destruction::save`. The fields of `ore`,
            // `money`, `repair` and `placement` are named in `check_unsaved_sections`.
            destruction,
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
        w.section(SECTION_PROJECTILES, |s| arsenal.save_parts(s))?;
        w.section(SECTION_DESTRUCTION, |s| destruction.save(s))?;
        w.section(SECTION_PATHING, |s| pathing.save(s, entities))?;
        w.section(SECTION_FIELDS, |s| pathing.save_fields(s))?;
        w.section(SECTION_FORMATIONS, |s| formations.save(s))?;
        w.section(SECTION_ORE, |s| match ore {
            Some(o) => o.save(s),
            None => Ok(()),
        })?;
        w.section(SECTION_HARVEST, |s| match harvest.as_deref() {
            Some(h) => h.save(s),
            None => Ok(()),
        })?;
        w.section(SECTION_MONEY, |s| match money {
            Some(m) => economy::save_money(s, m),
            None => Ok(()),
        })?;
        w.section(SECTION_PRODUCTION, |s| match production.as_deref() {
            Some(p) => economy::save_production(s, p),
            None => Ok(()),
        })?;
        w.section(SECTION_POWER, |s| match power {
            Some(p) => economy::save_power(s, p),
            None => Ok(()),
        })?;
        w.section(SECTION_OUTCOME, |s| match outcome.as_deref() {
            Some(o) => outcome::save(s, o),
            None => Ok(()),
        })?;
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
        // The tick counter must be able to advance (and command ticks to follow it).
        if tick.0 == u64::MAX {
            return Err(SaveError::Corrupt("the tick cannot advance"));
        }
        if r.usize()? != SECTIONS.len() {
            return Err(SaveError::Corrupt("wrong number of sections"));
        }

        let mut world = fresh;
        world.tick = tick;
        world.history_base = tick;

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

        let map = (world.config.width, world.config.height);
        let mut s = r.section(SECTION_PROJECTILES)?;
        world.arsenal.projectiles =
            crate::projectiles::Projectiles::restore(&mut s, &world.arsenal.table, map)?;
        s.end()?;

        let mut s = r.section(SECTION_DESTRUCTION)?;
        world.destruction.restore(&mut s, &world.units, tick, map)?;
        s.end()?;
        check_kills(&world)?;

        // A job is as many epochs behind the service as structures were destroyed in the last
        // tick (`Pathing::restore`).
        let lag = match tick.0.checked_sub(1) {
            Some(last) => world.destruction.rubble_born(Tick(last)),
            None => 0,
        };
        let bounds = world
            .pathing
            .bounds(&world.entities, world.config.width, world.config.height);
        let mut s = r.section(SECTION_PATHING)?;
        world
            .pathing
            .restore(&mut s, &mut world.entities, &bounds, lag)?;
        s.end()?;

        let mut s = r.section(SECTION_FIELDS)?;
        world.pathing.restore_fields(&mut s, &bounds)?;
        s.end()?;

        let mut s = r.section(SECTION_FORMATIONS)?;
        world.formations.restore(&mut s, &world.entities)?;
        s.end()?;

        // WP13.1c-2: each section is empty for a rule set the world does not have.
        let mut s = r.section(SECTION_ORE)?;
        if let Some(o) = &mut world.ore {
            o.restore(&mut s)?;
        }
        s.end()?;
        let mut s = r.section(SECTION_HARVEST)?;
        if let Some(mut h) = world.harvest.take() {
            let result = h.restore(&mut s, &world);
            world.harvest = Some(h);
            result?;
        }
        s.end()?;
        let mut s = r.section(SECTION_MONEY)?;
        if let Some(m) = &mut world.money {
            economy::restore_money(&mut s, m)?;
        }
        s.end()?;
        let mut s = r.section(SECTION_PRODUCTION)?;
        if let Some(p) = world.production.as_deref_mut() {
            economy::restore_production(&mut s, p, map)?;
        }
        s.end()?;
        let mut s = r.section(SECTION_POWER)?;
        if let Some(p) = &mut world.power {
            economy::restore_power(&mut s, p)?;
        }
        s.end()?;
        let mut s = r.section(SECTION_OUTCOME)?;
        if let Some(o) = world.outcome.as_deref_mut() {
            outcome::restore(&mut s, o, tick)?;
        }
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
        let World {
            terrain,
            movement,
            occupancy,
            pathing,
            ..
        } = &mut world;
        pathing.rebuild_graphs(terrain, movement, occupancy);
        world.pathing.check_in_flight(&world.entities)?;
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

#[cfg(test)]
mod path_tests;

#[cfg(test)]
mod combat_tests;

mod economy;

mod outcome;

#[cfg(test)]
mod outcome_tests;

#[cfg(test)]
mod economy_tests;
