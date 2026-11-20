//! Destruction (WP5.5, `docs/rules/destruction.md`): what happens to a unit or a structure whose hit
//! points run out.
//!
//! **Death.** It happens at one defined point of the tick: [`World::reap`], right after the weapon stage
//! has applied all the damage of the tick (the two-pass rule of `docs/rules/projectiles.md` is kept: no
//! decision of the tick has seen a death) and before the return fire. An entity made from the unit
//! table whose kind has hit points and whose hit points are now zero is **removed from the world**
//! (the same `despawn` a `Despawn` command uses, so the slot's generation goes up, every id issued for it
//! goes stale, a structure frees its cells and the routes and flowfields over them are refreshed). That
//! also ends the ambiguity of "zero hit points": in a world with destruction an entity at zero is a kind
//! that cannot be damaged (a crate), never one that was shot down.
//!
//! **Wrecks.** What dies may leave a [`Wreck`]: a vehicle (a mobile unit whose body is at least
//! [`DestructionConfig::wreck_min_size`] across) a wreck, a structure rubble the size of its footprint.
//! A wreck is not an entity: it has no slot, takes no orders, cannot be shot and **blocks nothing**; it
//! is a record with a position that the presentation draws and that expires after a number of ticks
//! given by the settings. Wrecks are kept sorted by a total key, so the order the dead were found in
//! (the slot order) is not visible anywhere.
//!
//! **Kill credit.** Settled from the damage the victim took **in the tick it died**, per shooter, and
//! never from the order of the shots or the `hp_after` reports. Only damage of the enemy counts (another
//! owner): the shooter with the most damage is the killer, the others who hurt it are assists, and a
//! victim that only its own side (or itself) hurt has no killer. Ties are broken by where the shooter
//! stood, `(y, x)`, not by its slot; a shooter that is already gone (a shell that outlived its gunner) comes
//! after those that are there.

use sf_math::bytes::{ByteReader, ByteWriter};
use sf_math::hash::Fnv1a64;
use sf_math::{Facing, Fx, Vec2};

use crate::entities::{Entities, NO_DEF};
use crate::ids::{EntityId, Kind, PlayerId, Tick};
use crate::orders::live_slot;
use crate::save::{SaveError, SaveResult, ensure, get_fx, get_vec2, put_fx, put_vec2};
use crate::terrain::Cell;
use crate::units::{Footprint, UnitClass, UnitTable};
use crate::world::World;

/// Ticks a vehicle wreck lasts unless the settings say otherwise: half a minute.
pub const DEFAULT_WRECK_TICKS: u32 = 600;
/// Ticks rubble lasts unless the settings say otherwise: a minute.
pub const DEFAULT_RUBBLE_TICKS: u32 = 1200;
/// The smallest body, in tenths of a cell, that leaves a wreck unless the settings say otherwise:
/// the infantry (4 to 6 tenths of a cell) leave nothing, the vehicles (12 tenths and up) do.
pub const DEFAULT_WRECK_MIN_SIZE_TENTHS: u16 = 10;
/// The most ticks a wreck or rubble may last (an hour).
pub const MAX_WRECK_TICKS: u32 = 72_000;
/// The most tenths of a cell `wreck_min_size_tenths` may be.
pub const MAX_WRECK_MIN_SIZE_TENTHS: u16 = 80;

/// Written first into the hash of the settings; bump when the layout changes.
const SETTINGS_TAG: &[u8] = b"sf_sim/destruction/v1";

/// The settings of destruction (`sim.ron`, `docs/rules/destruction.md`).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct DestructionConfig {
    /// Ticks a vehicle wreck stays; 1 to [`MAX_WRECK_TICKS`].
    pub wreck_ticks: u32,
    /// Ticks rubble stays; 1 to [`MAX_WRECK_TICKS`].
    pub rubble_ticks: u32,
    /// The smallest body (a diameter, in cells) of a mobile unit that leaves a wreck. Zero: every
    /// mobile unit does. At most `MAX_WRECK_MIN_SIZE_TENTHS` tenths.
    pub wreck_min_size: Fx,
}

impl DestructionConfig {
    /// The numbers of the shipped `sim.ron`, which a test keeps equal.
    pub const DEFAULT: DestructionConfig = DestructionConfig::from_tenths(
        DEFAULT_WRECK_TICKS,
        DEFAULT_RUBBLE_TICKS,
        DEFAULT_WRECK_MIN_SIZE_TENTHS,
    );

    /// Settings with the minimum size in tenths of a cell, as `sim.ron` gives it.
    pub const fn from_tenths(
        wreck_ticks: u32,
        rubble_ticks: u32,
        min_size_tenths: u16,
    ) -> DestructionConfig {
        DestructionConfig {
            wreck_ticks,
            rubble_ticks,
            wreck_min_size: Fx::from_ratio(min_size_tenths as i64, 10),
        }
    }

    /// Whether the numbers are in their ranges.
    pub fn is_valid(&self) -> bool {
        (1..=MAX_WRECK_TICKS).contains(&self.wreck_ticks)
            && (1..=MAX_WRECK_TICKS).contains(&self.rubble_ticks)
            && !self.wreck_min_size.is_negative()
            && self.wreck_min_size <= Fx::from_ratio(MAX_WRECK_MIN_SIZE_TENTHS as i64, 10)
    }

    /// In the checksum and in the rules digest of a save. Destructured without `..`: a new
    /// setting fails to compile until it is hashed.
    pub(crate) fn write_hash(&self, h: &mut Fnv1a64) {
        let DestructionConfig {
            wreck_ticks,
            rubble_ticks,
            wreck_min_size,
        } = self;
        h.write_bytes(SETTINGS_TAG);
        h.write_u32(*wreck_ticks);
        h.write_u32(*rubble_ticks);
        h.write_fx(*wreck_min_size);
    }
}

impl Default for DestructionConfig {
    fn default() -> DestructionConfig {
        DestructionConfig::DEFAULT
    }
}

/// What is left of something that was destroyed.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum WreckKind {
    /// The hulk of a vehicle: a body's size across.
    Vehicle,
    /// The rubble of a structure: the size of its footprint.
    Rubble,
}

/// A wreck or a pile of rubble. Not an entity (no slot, no id, no orders); it blocks nothing.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Wreck {
    /// Vehicle wreck or rubble.
    pub kind: WreckKind,
    /// The kind number of the unit or structure it was.
    pub unit: Kind,
    /// Who owned it.
    pub owner: PlayerId,
    /// Where it lies: the middle of the vehicle, or of the structure's footprint.
    pub pos: Vec2,
    /// The way the vehicle faced (zero for rubble).
    pub facing: Facing,
    /// The diameter of a vehicle in cells; zero for rubble.
    pub size: Fx,
    /// The cells rubble covers, counted from `anchor`; `None` for a vehicle wreck.
    pub footprint: Option<Footprint>,
    /// The corner cell with the smallest x and y of the rubble; `None` for a vehicle wreck.
    pub anchor: Option<Cell>,
    /// The tick it was made.
    pub born: Tick,
    /// The tick on which it is gone: it lies there for `expires - born` ticks.
    pub expires: Tick,
}

impl Wreck {
    /// The total order wrecks are kept in: where they lie, then what they are.
    fn key(&self) -> (i64, i64, WreckKind, u16, u8, u64, u64) {
        (
            self.pos.y.raw(),
            self.pos.x.raw(),
            self.kind,
            self.unit.0,
            self.owner.0,
            self.born.0,
            self.expires.0,
        )
    }

    /// How many more states of the world it is listed in, counting the one just after the tick that
    /// ran, as of `now` (the next tick to run): `lifetime` right after it was made, one in the last state
    /// it is in, so the next step removes it. For a presentation that fades it over its last ticks.
    pub fn ticks_left(&self, now: Tick) -> u64 {
        self.expires.0.saturating_sub(now.0.saturating_sub(1))
    }
}

/// One shooter's part in a death.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Credit {
    /// The shooter; stale if it was gone or died on the same tick.
    pub shooter: EntityId,
    /// The shooter's player.
    pub owner: PlayerId,
    /// The damage it did to the victim in the tick of the death (after the armor, before the hit
    /// points saturated at zero).
    pub damage: u32,
}

/// A unit or structure that was destroyed in a tick: what the presentation draws an explosion from,
/// and what score and veterancy read.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Death {
    /// The tick it died.
    pub tick: Tick,
    /// The victim's id, stale by now.
    pub victim: EntityId,
    /// What it was.
    pub kind: Kind,
    /// Who owned it.
    pub owner: PlayerId,
    /// Mobile or structure.
    pub class: UnitClass,
    /// Where it was (the middle of the footprint for a structure).
    pub pos: Vec2,
    /// The way it faced.
    pub facing: Facing,
    /// The diameter in cells of a mobile unit; zero for a structure.
    pub size: Fx,
    /// The cells of a structure; `None` for a mobile unit.
    pub footprint: Option<Footprint>,
    /// What it left behind, if anything.
    pub wreck: Option<WreckKind>,
    /// The enemy that did most damage in that tick; `None` if only its own side or itself hurt it.
    pub killer: Option<Credit>,
    /// The other enemies that hurt it in that tick, in the order of the credit key (most damage first).
    pub assists: Vec<Credit>,
}

/// What a player has done and lost since the world began. State: hashed.
#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
pub struct Tally {
    /// Enemies this player's units killed (the largest share of the killing tick's damage).
    pub kills: u32,
    /// Enemies they hurt in the tick they died, without being the killer.
    pub assists: u32,
    /// Units and structures of this player that were destroyed, whoever did it.
    pub losses: u32,
}

/// One entry of the damage the weapon stage did: filled only in a world with destruction.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct Hurt {
    pub victim: u32,
    pub damage: u32,
    pub shooter: EntityId,
    pub player: PlayerId,
}

/// The world's part: the settings (none: nothing is ever destroyed), the wrecks and the scores.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct Destruction {
    pub config: Option<DestructionConfig>,
    /// Sorted by [`Wreck::key`].
    pub wrecks: Vec<Wreck>,
    /// Per player, ascending; only players that have a non-zero tally.
    pub tally: Vec<(PlayerId, Tally)>,
    /// The deaths of the last tick. Derived; hashed through nothing (a report).
    pub deaths: Vec<Death>,
}

impl Destruction {
    /// Writes the wrecks and the tally (section `D` of a save, WP13.1c-1). The deaths are the
    /// report of the last tick: `reap` clears them before it reads anything, so they are not
    /// saved. Destructured without `..`.
    pub(crate) fn save(&self, w: &mut ByteWriter) -> SaveResult<()> {
        let Destruction {
            // Rules: in the rules digest.
            config: _,
            wrecks,
            tally,
            // The report of the last tick, cleared by the next `reap` before anything reads it.
            deaths: _,
        } = self;
        w.usize(wrecks.len())?;
        for wreck in wrecks {
            let Wreck {
                kind,
                unit,
                owner,
                pos,
                facing,
                size,
                footprint,
                anchor,
                born,
                expires,
            } = wreck;
            w.u8(match kind {
                WreckKind::Vehicle => 0,
                WreckKind::Rubble => 1,
            });
            w.u16(unit.0);
            w.u8(owner.0);
            put_vec2(w, *pos);
            w.u8(facing.raw());
            put_fx(w, *size);
            w.bool(footprint.is_some());
            if let Some(f) = footprint {
                w.u8(f.width);
                w.u8(f.height);
            }
            w.bool(anchor.is_some());
            if let Some(a) = anchor {
                w.u32(a.x);
                w.u32(a.y);
            }
            w.u64(born.0);
            w.u64(expires.0);
        }
        w.usize(tally.len())?;
        for (player, t) in tally {
            let Tally {
                kills,
                assists,
                losses,
            } = t;
            w.u8(player.0);
            w.u32(*kills);
            w.u32(*assists);
            w.u32(*losses);
        }
        Ok(())
    }

    /// Reads what [`Destruction::save`] wrote into the part of a fresh world. Refused as
    /// `Corrupt`: wrecks or a tally in a world without destruction settings, wrecks out of their
    /// order, a wreck the death stage never leaves (see `docs/rules/save.md`), a tally out of
    /// player order, twice or all zero.
    pub(crate) fn restore(
        &mut self,
        r: &mut ByteReader<'_>,
        units: &UnitTable,
        now: Tick,
        map: (u32, u32),
    ) -> SaveResult<()> {
        let n = r.count(36)?;
        let mut wrecks = Vec::with_capacity(n);
        for _ in 0..n {
            let kind = match r.u8()? {
                0 => WreckKind::Vehicle,
                1 => WreckKind::Rubble,
                _ => return Err(SaveError::Corrupt("unknown wreck kind")),
            };
            let unit = Kind(r.u16()?);
            let owner = PlayerId(r.u8()?);
            let pos = get_vec2(r)?;
            let facing = Facing::new(r.u8()?);
            let size = get_fx(r)?;
            let footprint = if r.bool()? {
                Some(Footprint {
                    width: r.u8()?,
                    height: r.u8()?,
                })
            } else {
                None
            };
            let anchor = if r.bool()? {
                Some(Cell::new(r.u32()?, r.u32()?))
            } else {
                None
            };
            let born = Tick(r.u64()?);
            let expires = Tick(r.u64()?);
            let wreck = Wreck {
                kind,
                unit,
                owner,
                pos,
                facing,
                size,
                footprint,
                anchor,
                born,
                expires,
            };
            ensure(
                self.config
                    .is_some_and(|c| wreck_reachable(&wreck, &c, units, now, map)),
                "a wreck the death stage never leaves",
            )?;
            ensure(
                wrecks.last().is_none_or(|l: &Wreck| l.key() <= wreck.key()),
                "the wrecks are out of order",
            )?;
            wrecks.push(wreck);
        }
        let n = r.count(13)?;
        let mut tally: Vec<(PlayerId, Tally)> = Vec::with_capacity(n);
        for _ in 0..n {
            let player = PlayerId(r.u8()?);
            let t = Tally {
                kills: r.u32()?,
                assists: r.u32()?,
                losses: r.u32()?,
            };
            ensure(
                self.config.is_some()
                    && t != Tally::default()
                    && tally.last().is_none_or(|&(p, _)| p < player),
                "a tally out of order, all zero or without destruction",
            )?;
            tally.push((player, t));
        }
        self.wrecks = wrecks;
        self.tally = tally;
        Ok(())
    }

    /// The kills a player has in the tally (for the restore check of the kills column).
    pub(crate) fn kills_of(&self, player: PlayerId) -> u32 {
        self.tally
            .binary_search_by_key(&player, |t| t.0)
            .map_or(0, |at| self.tally[at].1.kills)
    }

    /// The structures destroyed in tick `tick`: the rubble born then. Each one moved the path
    /// service's epoch once after the service ran (`World::despawn`), so a job saved after that
    /// tick is this many epochs behind (`Pathing::restore`).
    pub(crate) fn rubble_born(&self, tick: Tick) -> u32 {
        self.wrecks
            .iter()
            .filter(|w| w.kind == WreckKind::Rubble && w.born == tick)
            .count() as u32
    }

    fn tally_of(&mut self, player: PlayerId) -> &mut Tally {
        let at = match self.tally.binary_search_by_key(&player, |t| t.0) {
            Ok(at) => at,
            Err(at) => {
                self.tally.insert(at, (player, Tally::default()));
                at
            }
        };
        &mut self.tally[at].1
    }

    fn add_wreck(&mut self, wreck: Wreck) {
        let at = self.wrecks.partition_point(|w| w.key() <= wreck.key());
        self.wrecks.insert(at, wreck);
    }

    /// Removes what lies under a structure that is placed on `anchor` with `footprint`: wrecks whose
    /// middle is in the footprint and rubble that overlaps it.
    pub(crate) fn clear_under(&mut self, anchor: Cell, footprint: Footprint) {
        if self.wrecks.is_empty() {
            return;
        }
        let (x0, y0) = (anchor.x, anchor.y);
        let (x1, y1) = (
            x0 + u32::from(footprint.width),
            y0 + u32::from(footprint.height),
        );
        self.wrecks.retain(|w| match (w.anchor, w.footprint) {
            (Some(a), Some(f)) => {
                let (bx, by) = (a.x + u32::from(f.width), a.y + u32::from(f.height));
                !(a.x < x1 && bx > x0 && a.y < y1 && by > y0)
            }
            _ => {
                let cell = |v: Fx| u32::try_from(v.raw() >> sf_math::FRAC_BITS).unwrap_or(0);
                let (px, py) = (cell(w.pos.x), cell(w.pos.y));
                !(px >= x0 && px < x1 && py >= y0 && py < y1)
            }
        });
    }

    /// The destruction section of the checksum.
    pub(crate) fn write_hash(&self, h: &mut Fnv1a64, e: &Entities) {
        if let Some(config) = &self.config {
            config.write_hash(h);
        }
        h.write_u64(self.wrecks.len() as u64);
        for w in &self.wrecks {
            h.write_u8(w.kind as u8);
            h.write_bytes(&w.unit.0.to_le_bytes());
            h.write_u8(w.owner.0);
            h.write_vec2(w.pos);
            h.write_facing(w.facing);
            h.write_fx(w.size);
            match (w.footprint, w.anchor) {
                (Some(f), Some(a)) => {
                    h.write_u8(1);
                    h.write_u8(f.width);
                    h.write_u8(f.height);
                    h.write_u32(a.x);
                    h.write_u32(a.y);
                }
                _ => h.write_u8(0),
            }
            h.write_u64(w.born.0);
            h.write_u64(w.expires.0);
        }
        h.write_u64(self.tally.len() as u64);
        for (player, t) in &self.tally {
            h.write_u8(player.0);
            h.write_u32(t.kills);
            h.write_u32(t.assists);
            h.write_u32(t.losses);
        }
        let credited = |i: usize| e.alive[i] && e.kills[i] > 0;
        h.write_u64((0..e.len()).filter(|&i| credited(i)).count() as u64);
        for i in (0..e.len()).filter(|&i| credited(i)) {
            h.write_u32(i as u32);
            h.write_bytes(&e.kills[i].to_le_bytes());
        }
    }
}

/// Whether the death stage leaves `w` in a world saved before tick `now`: made in an earlier tick,
/// not expired (the stage removes a wreck on its `expires` tick), lasting exactly the setting of
/// its kind, of a kind of the unit table whose class leaves it (a mobile unit at least
/// `wreck_min_size` across leaves a vehicle wreck of its size and no cells; a structure rubble with
/// its footprint on an anchor of the map, size zero and facing zero), and lying on the map.
fn wreck_reachable(
    w: &Wreck,
    c: &DestructionConfig,
    units: &UnitTable,
    now: Tick,
    map: (u32, u32),
) -> bool {
    let Some(def) = units.index_of(w.unit) else {
        return false;
    };
    let stats = units.at(def);
    let (max_x, max_y) = (
        Fx::from_i64(i64::from(map.0)),
        Fx::from_i64(i64::from(map.1)),
    );
    let on_map =
        w.pos.x >= Fx::ZERO && w.pos.y >= Fx::ZERO && w.pos.x <= max_x && w.pos.y <= max_y;
    let ticks = match w.kind {
        WreckKind::Vehicle => c.wreck_ticks,
        WreckKind::Rubble => c.rubble_ticks,
    };
    let times = w.born < now
        && w.expires >= now
        && w.expires == w.born.saturating_add(u64::from(ticks));
    let shape = match w.kind {
        WreckKind::Vehicle => {
            stats.class() == UnitClass::Mobile
                && stats.size >= c.wreck_min_size
                && w.size == stats.size
                && w.footprint.is_none()
                && w.anchor.is_none()
        }
        WreckKind::Rubble => {
            stats.class() == UnitClass::Structure
                && w.size == Fx::ZERO
                && w.facing == Facing::default()
                && w.footprint == stats.footprint
                && match (w.anchor, w.footprint) {
                    (Some(a), Some(f)) => {
                        u64::from(a.x) + u64::from(f.width) <= u64::from(map.0)
                            && u64::from(a.y) + u64::from(f.height) <= u64::from(map.1)
                    }
                    _ => false,
                }
        }
    };
    on_map && times && shape
}

/// The order shooters are ranked in: most damage first, then the one that stood farthest north, then
/// farthest west, then by player and finally by the shooter's id (which only decides between two
/// shooters of one player that stood on exactly the same spot and did exactly the same damage). A
/// shooter that is gone has no place and comes after those that have one.
type CreditKey = (std::cmp::Reverse<u32>, bool, i64, i64, u8, u32, u32);

fn credit_key(c: &Credit, at: Option<Vec2>) -> CreditKey {
    let (y, x) = at.map_or((0, 0), |p| (p.y.raw(), p.x.raw()));
    (
        std::cmp::Reverse(c.damage),
        at.is_none(),
        y,
        x,
        c.owner.0,
        c.shooter.index(),
        c.shooter.generation(),
    )
}

/// The enemies that hurt `victim` (slot) in this tick, ranked: the killer first.
fn rank_credits(hurt: &[Hurt], victim: usize, victim_owner: PlayerId, e: &Entities) -> Vec<Credit> {
    // `hurt` is sorted by victim, then shooter, so the entries of one shooter are adjacent.
    let from = hurt.partition_point(|h| (h.victim as usize) < victim);
    let mut credits: Vec<Credit> = Vec::new();
    for h in hurt[from..]
        .iter()
        .take_while(|h| h.victim as usize == victim)
    {
        if h.player == victim_owner || h.damage == 0 {
            continue;
        }
        match credits.last_mut() {
            Some(c) if c.shooter == h.shooter => c.damage = c.damage.saturating_add(h.damage),
            _ => credits.push(Credit {
                shooter: h.shooter,
                owner: h.player,
                damage: h.damage,
            }),
        }
    }
    let place = |c: &Credit| live_slot(e, c.shooter).map(|j| e.pos[j]);
    credits.sort_by_key(|c| credit_key(c, place(c)));
    credits
}

impl World {
    /// The death stage of the tick (see the module documentation): expires the wrecks that are due,
    /// then removes everything whose hit points the weapon stage took to zero, settles the credit,
    /// leaves the wrecks and notes the deaths. A world without destruction settings does nothing, and a
    /// unit at zero then stays where it fell, as before WP5.5.
    pub(crate) fn reap(&mut self, tick: Tick) {
        self.destruction.deaths.clear();
        let Some(config) = self.destruction.config else {
            return;
        };
        self.destruction.wrecks.retain(|w| w.expires > tick);

        let mut hurt = std::mem::take(&mut self.arsenal.hurt);
        hurt.sort_unstable_by_key(|h| (h.victim, h.shooter.index(), h.shooter.generation()));

        // (a) Find the dead, in slot order, and work out everything from the state as it stands.
        let mut dead: Vec<usize> = Vec::new();
        for i in 0..self.entities.len() {
            if self.entities.alive[i]
                && self.entities.def[i] != NO_DEF
                && self.entities.hp[i] == 0
                && self.units.at(self.entities.def[i] as usize).hp > 0
            {
                dead.push(i);
            }
        }
        let mut deaths: Vec<Death> = Vec::with_capacity(dead.len());
        for &i in &dead {
            let e = &self.entities;
            let stats = *self.units.at(e.def[i] as usize);
            let class = stats.class();
            let (size, footprint, wreck) = match class {
                UnitClass::Structure => (Fx::ZERO, stats.footprint, Some(WreckKind::Rubble)),
                _ => (
                    stats.size,
                    None,
                    (stats.size >= config.wreck_min_size && class == UnitClass::Mobile)
                        .then_some(WreckKind::Vehicle),
                ),
            };
            let mut credits = rank_credits(&hurt, i, e.owner[i], e).into_iter();
            let killer = credits.next();
            deaths.push(Death {
                tick,
                victim: EntityId::new(i as u32, e.generation[i]),
                kind: e.kind[i],
                owner: e.owner[i],
                class,
                pos: e.pos[i],
                facing: e.facing[i],
                size,
                footprint,
                wreck,
                killer,
                assists: credits.collect(),
            });
        }
        // (b) The credit, while every shooter still stands in its slot: a kill count of a shooter that
        // dies on the same tick goes with it.
        for d in &deaths {
            // Saturating (WP13.1c-1): a restored tally at its maximum must not overflow; below it
            // the numbers are the same as before.
            let t = self.destruction.tally_of(d.owner);
            t.losses = t.losses.saturating_add(1);
            if let Some(k) = d.killer {
                let t = self.destruction.tally_of(k.owner);
                t.kills = t.kills.saturating_add(1);
                if let Some(j) = live_slot(&self.entities, k.shooter) {
                    self.entities.kills[j] = self.entities.kills[j].saturating_add(1);
                }
            }
            for a in &d.assists {
                let t = self.destruction.tally_of(a.owner);
                t.assists = t.assists.saturating_add(1);
            }
        }
        // (c) The wrecks, from the anchor of each structure before its cells are freed.
        for (&i, d) in dead.iter().zip(&deaths) {
            let Some(kind) = d.wreck else { continue };
            let anchor = (self.entities.cell[i] != crate::terrain::NO_CELL)
                .then(|| self.occupancy.cell(self.entities.cell[i]));
            let (ticks, size) = match kind {
                WreckKind::Vehicle => (config.wreck_ticks, d.size),
                WreckKind::Rubble => (config.rubble_ticks, Fx::ZERO),
            };
            self.destruction.add_wreck(Wreck {
                kind,
                unit: d.kind,
                owner: d.owner,
                pos: d.pos,
                facing: if kind == WreckKind::Vehicle {
                    d.facing
                } else {
                    Facing::default()
                },
                size,
                footprint: d.footprint,
                anchor,
                born: tick,
                expires: tick.saturating_add(u64::from(ticks)),
            });
        }
        // (d) Remove them. The cells of structures are freed and the routes over them refreshed by
        // `despawn`; the order does not matter, each refresh reads the occupancy as it then is and
        // the last one sees them all free.
        for d in &deaths {
            let removed = self.despawn(d.victim);
            debug_assert!(removed, "the victim was alive a moment ago");
        }
        // The report lists the dead by where they stood, not by their slots.
        deaths.sort_by_key(|d| {
            (
                d.pos.y.raw(),
                d.pos.x.raw(),
                d.kind.0,
                d.owner.0,
                d.victim.index(),
            )
        });
        self.destruction.deaths = deaths;
        hurt.clear();
        self.arsenal.hurt = hurt;
    }
}

#[cfg(test)]
mod tests;
