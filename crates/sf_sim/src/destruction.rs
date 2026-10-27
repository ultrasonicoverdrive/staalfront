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

use sf_math::hash::Fnv1a64;
use sf_math::{Facing, Fx, Vec2};

use crate::entities::{Entities, NO_DEF};
use crate::ids::{EntityId, Kind, PlayerId, Tick};
use crate::orders::live_slot;
use crate::terrain::Cell;
use crate::units::{Footprint, UnitClass};
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

    fn write_hash(&self, h: &mut Fnv1a64) {
        h.write_bytes(SETTINGS_TAG);
        h.write_u32(self.wreck_ticks);
        h.write_u32(self.rubble_ticks);
        h.write_fx(self.wreck_min_size);
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
            self.destruction.tally_of(d.owner).losses += 1;
            if let Some(k) = d.killer {
                self.destruction.tally_of(k.owner).kills += 1;
                if let Some(j) = live_slot(&self.entities, k.shooter) {
                    self.entities.kills[j] = self.entities.kills[j].saturating_add(1);
                }
            }
            for a in &d.assists {
                self.destruction.tally_of(a.owner).assists += 1;
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
