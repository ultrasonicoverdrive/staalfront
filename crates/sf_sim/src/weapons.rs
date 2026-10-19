//! Weapons (WP5.1): what a unit carries, the reload that counts down in it, and a
//! placeholder shot that exercises both.
//!
//! `sf_data` reads and checks `assets/config/weapon_defs.ron` and converts it to plain
//! integers and fixed point; this crate may not depend on it, so the caller
//! (`sf_bridge::weapon_table`) copies the numbers into a [`WeaponTable`] and hands it
//! over once, with [`World::with_weapons`](crate::World::with_weapons), after the unit
//! table. The world has no weapons until then: it fires nothing, hashes nothing about
//! weapons, and every checksum made before WP5.1 is what it was. `docs/rules/weapons.md`
//! has the rules in full.
//!
//! # The table
//!
//! A [`WeaponStats`] is one weapon: reach (maximum and minimum, in cells, measured to the
//! edge of the target), the reload and the burst in ticks, the damage and its type, the
//! projectile (kind, speed per tick, height, steering), the splash radius, where it is
//! mounted and how wide its firing arc is. A [`Loadout`] says which weapons a unit kind
//! carries, in slot order, at most [`MAX_WEAPON_SLOTS`]. The weapon of the first slot has the
//! range that the unit table calls `weapon_range`, the reach of an attack order, and
//! [`World::with_weapons`](crate::World::with_weapons) refuses a table where the two disagree.
//!
//! # The state
//!
//! Every entity has, per slot, a [`SlotState`]: the ticks to wait before the weapon may fire
//! again and the shots still to come in a running burst. It lives in the entity, not in its
//! order: giving a unit a new order does not reload its weapons. All of it is hashed in the
//! checksum while any entity is not at rest (section `W`).
//!
//! The reload counts from the **first** shot of a burst to the first of the next. A burst of
//! `n` shots with `d` ticks between them takes `(n - 1) * d` ticks, and the wait that follows the
//! last shot is the rest of the reload. A burst that cannot go on (the target is gone, out of
//! reach or not aimed at) is dropped, and the weapon waits the same rest.
//!
//! # The placeholder shot
//!
//! Phase 5 replaces this part in steps: projectiles as entities (WP5.2), armor and splash
//! (WP5.3), choosing targets (WP5.4). Until then a unit whose current order is
//! [`Order::Attack`] fires each of its weapons at the victim, as an instant hit, whenever
//! the weapon is reloaded, the victim is within the weapon's reach (the same distance the
//! order walks to) and not within its minimum range, and the hull or turret points at it
//! within the firing arc. The hit takes `damage` off the victim's hit points, saturating at
//! zero, whatever the projectile kind, the damage type or the splash. A victim at zero hit points
//! is not shot at, and a shooter at zero fires no more. Nothing is removed (WP5.5). Every shot is recorded in [`Shot`]s that
//! [`World::shots`](crate::World::shots) lists for the last tick; they are not hashed (the
//! hit points are).

use core::fmt;

use sf_math::Fx;
use sf_math::hash::Fnv1a64;

use crate::entities::{Entities, NO_DEF};
use crate::ids::{EntityId, Kind, Tick};
use crate::orders::{Order, live_slot, radius_of};
use crate::units::UnitTable;

/// Most weapons one unit kind carries.
pub const MAX_WEAPON_SLOTS: usize = 4;

/// Written first into the digest; bump when its layout changes.
const DIGEST_TAG: &[u8] = b"sf_sim/weapons/v1";

/// The widest firing arc: half a turn to either side is all round.
pub const ALL_ROUND: u8 = 128;

/// An empty slot in a table of weapon positions.
const NONE: u16 = u16::MAX;

/// How the projectile of a weapon travels. Only the placeholder shot reads it so far:
/// it ignores the kind (WP5.2).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ProjectileKind {
    /// Hits in the tick of the shot; no projectile exists.
    Instant,
    /// Flies straight at the target.
    Straight,
    /// Lobbed on an arc.
    Arc,
    /// Steers toward its target in flight.
    Guided,
    /// Drops from above onto the target cell.
    Falling,
}

/// What a weapon aims with.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Mount {
    /// The hull: the whole unit has to face the target.
    Hull,
    /// The turret, which turns apart from the hull (WP4.5).
    Turret,
}

/// One weapon, in the sim's units: cells as [`Fx`], times in ticks, names as numbers.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct WeaponStats {
    /// The permanent number of the weapon.
    pub kind: u16,
    /// Longest distance it can hit, in cells, measured to the edge of the target.
    pub range: Fx,
    /// Shortest distance it can hit, in cells, zero for none; below `range`.
    pub min_range: Fx,
    /// Ticks from the first shot of a burst to the first of the next, at least one.
    pub reload_ticks: u32,
    /// Shots in a burst, at least one.
    pub burst: u8,
    /// Ticks between the shots of a burst; zero exactly when `burst` is one. Over the whole
    /// burst it is shorter than `reload_ticks`.
    pub burst_delay_ticks: u32,
    /// Hit points of damage per shot, before armor.
    pub damage: u32,
    /// Kind number of the damage type (the armor table of WP5.3 keys on it).
    pub damage_type: u16,
    /// How the projectile travels.
    pub projectile: ProjectileKind,
    /// Distance per tick of the projectile; zero for an instant hit.
    pub projectile_speed_per_tick: Fx,
    /// Height of the arc or the drop, in cells; presentation.
    pub flight_height: Fx,
    /// Facing steps (of 256) per tick a guided projectile steers; zero for none.
    pub guide_turn_per_tick: u8,
    /// Radius in cells in which a hit also damages (WP5.3); zero for none.
    pub splash_radius: Fx,
    /// What it aims with.
    pub mount: Mount,
    /// Half the width of the firing arc in facing steps (of 256), at most [`ALL_ROUND`].
    pub fire_arc: u8,
}

impl WeaponStats {
    /// The ticks that remain of the reload after the last shot of a burst.
    pub const fn tail_ticks(&self) -> u32 {
        self.reload_ticks - (self.burst as u32 - 1) * self.burst_delay_ticks
    }
}

/// The weapons a unit kind carries, in slot order.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Loadout {
    /// The unit kind.
    pub unit: Kind,
    /// Kind numbers of its weapons, one to [`MAX_WEAPON_SLOTS`].
    pub weapons: Vec<u16>,
}

/// Why a weapon table was refused.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum WeaponsError {
    /// Two weapons have the same kind.
    DuplicateWeapon(u16),
    /// A weapon has a reach of zero, or a minimum reach that is not below it.
    BadRange(u16),
    /// A weapon has a reload of zero ticks.
    NoReload(u16),
    /// A weapon's burst is empty, has a delay without a second shot (or none with
    /// one), or is not shorter than the reload.
    BadBurst(u16),
    /// A firing arc is above half a turn.
    BadArc(u16),
    /// Two loadouts are for the same unit kind.
    DuplicateLoadout(Kind),
    /// A loadout has no weapons or more than [`MAX_WEAPON_SLOTS`].
    BadLoadout(Kind),
    /// A loadout names a weapon the table does not have.
    UnknownWeapon {
        /// The unit kind.
        unit: Kind,
        /// The weapon kind.
        weapon: u16,
    },
    /// A loadout is for a unit kind the world's unit table does not have.
    UnknownUnit(Kind),
    /// The range of the first weapon of a unit is not the `weapon_range` of its stats.
    ReachMismatch(Kind),
    /// A turret-mounted weapon on a unit kind without a turret.
    TurretWithoutTurret {
        /// The unit kind.
        unit: Kind,
        /// The weapon kind.
        weapon: u16,
    },
}

impl fmt::Display for WeaponsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WeaponsError::DuplicateWeapon(k) => write!(f, "weapon {k} is listed twice"),
            WeaponsError::BadRange(k) => write!(f, "weapon {k} has no valid range"),
            WeaponsError::NoReload(k) => write!(f, "weapon {k} has a reload of zero ticks"),
            WeaponsError::BadBurst(k) => write!(f, "weapon {k} has a burst that does not fit"),
            WeaponsError::BadArc(k) => write!(f, "weapon {k} has a firing arc above half a turn"),
            WeaponsError::DuplicateLoadout(k) => {
                write!(f, "unit kind {} has two loadouts", k.0)
            }
            WeaponsError::BadLoadout(k) => {
                write!(f, "unit kind {} has no weapons or too many", k.0)
            }
            WeaponsError::UnknownWeapon { unit, weapon } => {
                write!(f, "unit kind {} carries unknown weapon {weapon}", unit.0)
            }
            WeaponsError::UnknownUnit(k) => {
                write!(
                    f,
                    "a loadout is for kind {}, which the unit table lacks",
                    k.0
                )
            }
            WeaponsError::ReachMismatch(k) => write!(
                f,
                "unit kind {}: the range of its first weapon is not its weapon_range",
                k.0
            ),
            WeaponsError::TurretWithoutTurret { unit, weapon } => write!(
                f,
                "unit kind {} has no turret for weapon {weapon}, which is turret-mounted",
                unit.0
            ),
        }
    }
}

impl std::error::Error for WeaponsError {}

/// Every weapon and every loadout of a world. Immutable once built.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct WeaponTable {
    /// Sorted by kind.
    weapons: Vec<WeaponStats>,
    /// Sorted by unit kind.
    loadouts: Vec<Loadout>,
    /// Hash of everything above; zero when empty.
    digest: u64,
}

impl fmt::Debug for WeaponTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WeaponTable")
            .field("weapons", &self.weapons.len())
            .field("armed_kinds", &self.loadouts.len())
            .field("digest", &format_args!("{:#018x}", self.digest))
            .finish()
    }
}

impl WeaponTable {
    /// The table of a world without weapons.
    pub fn empty() -> WeaponTable {
        WeaponTable::default()
    }

    /// Checks and sorts `weapons` and `loadouts`. The order of the input does not matter:
    /// any order gives an equal table with an equal digest.
    pub fn new(
        mut weapons: Vec<WeaponStats>,
        mut loadouts: Vec<Loadout>,
    ) -> Result<WeaponTable, WeaponsError> {
        weapons.sort_by_key(|w| w.kind);
        loadouts.sort_by_key(|l| l.unit);
        for pair in weapons.windows(2) {
            if pair[0].kind == pair[1].kind {
                return Err(WeaponsError::DuplicateWeapon(pair[0].kind));
            }
        }
        for w in &weapons {
            let k = w.kind;
            if w.range <= Fx::ZERO || w.min_range.is_negative() || w.min_range >= w.range {
                return Err(WeaponsError::BadRange(k));
            }
            if w.reload_ticks == 0 {
                return Err(WeaponsError::NoReload(k));
            }
            let span = u64::from(w.burst.saturating_sub(1)) * u64::from(w.burst_delay_ticks);
            let one_shot = w.burst == 1 && w.burst_delay_ticks == 0;
            let several = w.burst > 1 && w.burst_delay_ticks > 0;
            if !(one_shot || several) || span >= u64::from(w.reload_ticks) {
                return Err(WeaponsError::BadBurst(k));
            }
            if w.fire_arc > ALL_ROUND {
                return Err(WeaponsError::BadArc(k));
            }
        }
        for pair in loadouts.windows(2) {
            if pair[0].unit == pair[1].unit {
                return Err(WeaponsError::DuplicateLoadout(pair[0].unit));
            }
        }
        for l in &loadouts {
            if l.weapons.is_empty() || l.weapons.len() > MAX_WEAPON_SLOTS {
                return Err(WeaponsError::BadLoadout(l.unit));
            }
            for &w in &l.weapons {
                if weapons.binary_search_by_key(&w, |s| s.kind).is_err() {
                    return Err(WeaponsError::UnknownWeapon {
                        unit: l.unit,
                        weapon: w,
                    });
                }
            }
        }
        let digest = if weapons.is_empty() && loadouts.is_empty() {
            0
        } else {
            digest_of(&weapons, &loadouts)
        };
        Ok(WeaponTable {
            weapons,
            loadouts,
            digest,
        })
    }

    /// True if there are no weapons and no loadouts.
    pub fn is_empty(&self) -> bool {
        self.weapons.is_empty() && self.loadouts.is_empty()
    }

    /// Every weapon, ascending by kind.
    pub fn weapons(&self) -> &[WeaponStats] {
        &self.weapons
    }

    /// The weapon with this kind number.
    pub fn weapon(&self, kind: u16) -> Option<&WeaponStats> {
        self.weapons
            .binary_search_by_key(&kind, |w| w.kind)
            .ok()
            .map(|i| &self.weapons[i])
    }

    /// The kind numbers of the weapons a unit kind carries, in slot order; empty for an
    /// unarmed kind.
    pub fn loadout(&self, unit: Kind) -> &[u16] {
        self.loadouts
            .binary_search_by_key(&unit, |l| l.unit)
            .ok()
            .map_or(&[], |i| &self.loadouts[i].weapons)
    }

    /// A hash of every weapon and loadout; zero for an empty table. Part of the weapons
    /// section of the world checksum.
    pub fn digest(&self) -> u64 {
        self.digest
    }
}

fn digest_of(weapons: &[WeaponStats], loadouts: &[Loadout]) -> u64 {
    let mut h = Fnv1a64::new();
    h.write_bytes(DIGEST_TAG);
    h.write_u64(u64::try_from(weapons.len()).unwrap_or(u64::MAX));
    for w in weapons {
        h.write_bytes(&w.kind.to_le_bytes());
        h.write_fx(w.range);
        h.write_fx(w.min_range);
        h.write_u32(w.reload_ticks);
        h.write_u8(w.burst);
        h.write_u32(w.burst_delay_ticks);
        h.write_u32(w.damage);
        h.write_bytes(&w.damage_type.to_le_bytes());
        h.write_u8(match w.projectile {
            ProjectileKind::Instant => 0,
            ProjectileKind::Straight => 1,
            ProjectileKind::Arc => 2,
            ProjectileKind::Guided => 3,
            ProjectileKind::Falling => 4,
        });
        h.write_fx(w.projectile_speed_per_tick);
        h.write_fx(w.flight_height);
        h.write_u8(w.guide_turn_per_tick);
        h.write_fx(w.splash_radius);
        h.write_u8(match w.mount {
            Mount::Hull => 0,
            Mount::Turret => 1,
        });
        h.write_u8(w.fire_arc);
    }
    h.write_u64(u64::try_from(loadouts.len()).unwrap_or(u64::MAX));
    for l in loadouts {
        h.write_bytes(&l.unit.0.to_le_bytes());
        h.write_u8(l.weapons.len() as u8);
        for w in &l.weapons {
            h.write_bytes(&w.to_le_bytes());
        }
    }
    h.finish()
}

/// What one weapon slot of one entity remembers.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct SlotState {
    /// Ticks until the weapon may fire again; zero when it is ready.
    pub wait: u32,
    /// Shots still to come in the running burst, not counting one that is due now; zero
    /// when no burst runs.
    pub burst_left: u8,
}

/// The state of every slot of one entity.
pub(crate) type WeaponState = [SlotState; MAX_WEAPON_SLOTS];

/// The state of a weapon that has not fired, or has finished reloading.
pub(crate) const REST: WeaponState = [SlotState {
    wait: 0,
    burst_left: 0,
}; MAX_WEAPON_SLOTS];

/// Whether every slot is at rest: ready, no burst running.
pub(crate) fn at_rest(state: &WeaponState) -> bool {
    *state == REST
}

/// One shot of the placeholder: an instant hit, recorded for the tick it was fired in.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Shot {
    /// The tick of the shot.
    pub tick: Tick,
    /// Who fired.
    pub attacker: EntityId,
    /// Who was hit.
    pub target: EntityId,
    /// The kind number of the weapon.
    pub weapon: u16,
    /// Which weapon slot of the attacker fired.
    pub slot: u8,
    /// The damage of the weapon, before it was taken off the target's hit points.
    pub damage: u32,
    /// The target's hit points after the hit.
    pub hp_after: u32,
}

/// The weapons of a world: the table, where each unit kind's slots point into it, and the
/// shots of the last tick. Empty (no weapons, nothing fires) until the caller hands a table
/// over.
#[derive(Clone, PartialEq, Eq, Default)]
pub(crate) struct Arsenal {
    pub table: WeaponTable,
    /// Per position in the unit table: the index into `table.weapons` of each slot, or
    /// [`NONE`].
    by_def: Vec<[u16; MAX_WEAPON_SLOTS]>,
    /// The shots of the last tick. Derived; not hashed.
    pub shots: Vec<Shot>,
}

impl Arsenal {
    /// Checks `table` against the unit table and builds the lookup from unit-table position
    /// to weapon slots.
    pub fn new(table: WeaponTable, units: &UnitTable) -> Result<Arsenal, WeaponsError> {
        let mut by_def = vec![[NONE; MAX_WEAPON_SLOTS]; units.len()];
        for l in &table.loadouts {
            let Some(def) = units.index_of(l.unit) else {
                return Err(WeaponsError::UnknownUnit(l.unit));
            };
            let stats = units.at(def);
            for (slot, &w) in l.weapons.iter().enumerate() {
                let Ok(at) = table.weapons.binary_search_by_key(&w, |s| s.kind) else {
                    return Err(WeaponsError::UnknownWeapon {
                        unit: l.unit,
                        weapon: w,
                    });
                };
                let weapon = &table.weapons[at];
                if slot == 0 && weapon.range != stats.weapon_range {
                    return Err(WeaponsError::ReachMismatch(l.unit));
                }
                if weapon.mount == Mount::Turret && !stats.has_turret() {
                    return Err(WeaponsError::TurretWithoutTurret {
                        unit: l.unit,
                        weapon: w,
                    });
                }
                by_def[def][slot] = at as u16;
            }
        }
        // A unit kind that has a weapon_range in the unit table but no loadout would walk
        // up to its target and never fire: a table that disagrees with itself.
        for (def, slots) in by_def.iter().enumerate() {
            if slots[0] == NONE && !units.at(def).weapon_range.is_zero() {
                return Err(WeaponsError::ReachMismatch(units.at(def).kind));
            }
        }
        Ok(Arsenal {
            table,
            by_def,
            shots: Vec::new(),
        })
    }

    /// True while the world has no weapons.
    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }
}

/// Runs one tick of every weapon, in ascending slot order, after the units have moved and
/// the turrets turned: the reloads count down and the placeholder shots are fired.
pub(crate) fn update_all(e: &mut Entities, units: &UnitTable, arsenal: &mut Arsenal, tick: Tick) {
    arsenal.shots.clear();
    if arsenal.is_empty() {
        return;
    }
    let Arsenal {
        table,
        by_def,
        shots,
    } = arsenal;
    for i in 0..e.len() {
        if !e.alive[i] || e.def[i] == NO_DEF {
            continue;
        }
        let slots = by_def[e.def[i] as usize];
        if slots[0] == NONE {
            continue;
        }
        // A unit that has been shot down (no hit points left, though its kind has some) fires
        // no more.
        if e.hp[i] == 0 && units.at(e.def[i] as usize).hp > 0 {
            continue;
        }
        let victim = match e.order[i] {
            Order::Attack(id) => live_slot(e, id).filter(|&j| j != i),
            _ => None,
        };
        for (s, &at) in slots.iter().enumerate() {
            if at == NONE {
                break;
            }
            let weapon = &table.weapons[at as usize];
            let state = &mut e.weapon[i][s];
            state.wait = state.wait.saturating_sub(1);
            if state.wait > 0 {
                continue;
            }
            let Some(j) = victim.filter(|&j| can_fire(e, units, i, j, weapon)) else {
                let state = &mut e.weapon[i][s];
                if state.burst_left > 0 {
                    // The burst cannot go on: drop it, and wait out the rest of the reload.
                    state.burst_left = 0;
                    state.wait = weapon.tail_ticks();
                }
                continue;
            };
            let hp_after = e.hp[j].saturating_sub(weapon.damage);
            e.hp[j] = hp_after;
            let state = &mut e.weapon[i][s];
            // Shots left in this burst, this one included.
            let left = if state.burst_left > 0 {
                state.burst_left
            } else {
                weapon.burst
            };
            let remaining = left - 1;
            state.burst_left = remaining;
            state.wait = if remaining > 0 {
                weapon.burst_delay_ticks
            } else {
                weapon.tail_ticks()
            };
            shots.push(Shot {
                tick,
                attacker: EntityId::new(i as u32, e.generation[i]),
                target: EntityId::new(j as u32, e.generation[j]),
                weapon: weapon.kind,
                slot: s as u8,
                damage: weapon.damage,
                hp_after,
            });
        }
    }
}

/// Whether the weapon of unit `i` may fire at unit `j` now: `j` can still be hurt, is
/// within the weapon's reach and not within its minimum range (both measured to the edge of
/// `j`, the way an attack order measures its arrival), and the weapon points at it.
fn can_fire(e: &Entities, units: &UnitTable, i: usize, j: usize, w: &WeaponStats) -> bool {
    if e.hp[j] == 0 {
        return false;
    }
    let between = e.pos[j] - e.pos[i];
    let d2 = between.length_squared();
    let edge = radius_of(e, j, units);
    if d2 > (w.range + edge).square() {
        return false;
    }
    if !w.min_range.is_zero() && d2 < (w.min_range + edge).square() {
        return false;
    }
    // On top of each other there is no direction: it counts as aimed.
    let Some(wanted) = between.facing() else {
        return true;
    };
    let pointing = match w.mount {
        Mount::Hull => e.facing[i],
        Mount::Turret => e.turret[i],
    };
    pointing.abs_diff(wanted) <= w.fire_arc
}

#[cfg(test)]
mod tests;
