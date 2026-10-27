//! Weapons (WP5.1): what a unit carries, the reload that counts down in it, and the shot
//! (an instant hit, or a projectile that flies, WP5.2).
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
//! # A tick of shooting, in two passes
//!
//! Since WP5.2 a weapon either hits at once (`Instant`) or launches a projectile that flies and
//! detonates later ([`crate::projectiles`]). Since WP5.3 what a hit does is [`damage_of`]: the weapon's
//! `damage` goes through the armor table ([`crate::armor`]) for the victim it hit, and a weapon with a
//! splash radius also hurts everything else near the detonation, less the farther out it stands
//! (`docs/rules/armour.md`). Since WP5.4 a unit can also choose its target ([`crate::targeting`]).
//!
//! A tick of weapons runs in two passes, and **no hit point changes in the first**:
//!
//! * *Decide.* In ascending slot order the projectiles in flight move and decide whether they detonate
//!   (and on whom); then, in ascending slot order, every weapon slot of every entity counts its wait
//!   down and decides whether it fires: an `Instant` weapon decides a hit, any other launches a
//!   projectile. Every decision reads the hit points and positions of the start of the weapon stage.
//! * *Apply.* Every hit of the tick, the instant ones and the detonations, is turned into damage by
//!   [`damage_of`] (the direct victim and everyone in a splash), summed **per victim**, and taken off its
//!   hit points once, saturating at zero. Detonated projectiles are removed and the new ones enter the
//!   store.
//!
//! So the outcome of a tick does not depend on the order of the entities or of the projectiles: two
//! units that kill each other both die, two shooters that overkill one victim both fire and both spend
//! their reload, a unit that is shot down on a tick still fires on it, and a shell that lands on a tick
//! on a victim that another shell kills on that same tick still hurts it. Only the order of the
//! *reports* ([`Shot`], [`Impact`]) follows the slots.
//!
//! A shooter at zero hit points fires no more, and a victim at zero is not shot at; a projectile
//! already in flight does not care whether its shooter is alive. Nothing is removed when a victim
//! reaches zero (WP5.5). Every trigger pull is recorded in a [`Shot`], every detonation of a projectile
//! in an [`Impact`]; [`World::shots`](crate::World::shots) and
//! [`World::impacts`](crate::World::impacts) list them for the last tick. They are reports, not state,
//! and are not hashed (the hit points and the projectiles are).

use core::fmt;

use sf_math::hash::Fnv1a64;
use sf_math::{Fx, Vec2};

use crate::armor::{ArmorTable, scaled_damage};
use crate::crowd::Index;
use crate::entities::{Entities, NO_DEF};
use crate::ids::{EntityId, Kind, PlayerId, Tick};
use crate::orders::{Order, live_slot, radius_of, radius_of_stats};
use crate::projectiles::{self, ImpactCause, Projectile, Projectiles};
use crate::units::UnitTable;

/// Most weapons one unit kind carries.
pub const MAX_WEAPON_SLOTS: usize = 4;

/// Written first into the digest; bump when its layout changes.
const DIGEST_TAG: &[u8] = b"sf_sim/weapons/v2";

/// Cells to a bucket of the index a splash looks its victims up in.
const SPLASH_BUCKET: i64 = 4;

/// The widest firing arc: half a turn to either side is all round.
pub const ALL_ROUND: u8 = 128;

/// An empty slot in a table of weapon positions.
const NONE: u16 = u16::MAX;

/// How the projectile of a weapon travels (`docs/rules/projectiles.md`).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ProjectileKind {
    /// Hits in the tick of the shot; no projectile exists.
    Instant,
    /// Flies at constant speed toward where the target stood, and goes off when its path touches the
    /// target.
    Straight,
    /// Lobbed over everything between: goes off where it lands.
    Arc,
    /// Steers toward its target in flight.
    Guided,
    /// Drops from above onto the place where the target stood.
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
    /// Distance per tick of the projectile; zero for an instant hit, positive for every other kind
    /// ([`WeaponsError::NoSpeed`]). For a falling projectile it is the speed of the drop.
    pub projectile_speed_per_tick: Fx,
    /// Height of the top of an arc, or the height a falling projectile drops from, in cells; positive
    /// for a falling one ([`WeaponsError::NoHeight`]). Presentation, apart from the time a drop takes.
    pub flight_height: Fx,
    /// Facing steps (of 256) per tick a guided projectile steers; zero for none.
    pub guide_turn_per_tick: u8,
    /// Radius in cells in which a detonation also hurts what is near it (WP5.3); zero for none. Measured
    /// from the point where it goes off to the **edge** of the victim.
    pub splash_radius: Fx,
    /// Inside this distance from the edge of a victim the splash does its full damage; from there it falls
    /// away in a straight line to nothing at `splash_radius`. Zero: it starts to fall at once. Below
    /// `splash_radius` (and zero when that is zero).
    pub splash_inner: Fx,
    /// Whether the splash hurts the shooter's own player's units too. The direct victim is hurt whoever
    /// it belongs to.
    pub friendly_fire: bool,
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
    /// A full-damage splash radius that is negative, or not below the splash radius (or given without
    /// one).
    BadSplash(u16),
    /// A projectile that flies (any kind but `Instant`) has no speed.
    NoSpeed(u16),
    /// A falling projectile has no height to drop from.
    NoHeight(u16),
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
    /// A loadout, or an entry of the armor table, is for a unit kind the world's unit table does not
    /// have.
    UnknownUnit(Kind),
    /// A weapon's damage type is not in the armor table.
    UnknownDamageType {
        /// The weapon kind.
        weapon: u16,
        /// The damage type.
        damage_type: u16,
    },
    /// The armor table gives no armor type to a unit kind of the unit table.
    NoArmor(Kind),
    /// The range of the first weapon of a unit is not the `weapon_range` of its stats.
    ReachMismatch(Kind),
    /// A turret-mounted weapon on a unit kind without a turret.
    TurretWithoutTurret {
        /// The unit kind.
        unit: Kind,
        /// The weapon kind.
        weapon: u16,
    },
    /// A hull-mounted weapon on a unit kind that has a turret (WP5.4): the turret aims, the hull does
    /// not follow it for a weapon, so the weapon would never be aimed.
    HullWeaponOnTurret {
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
            WeaponsError::BadSplash(k) => write!(
                f,
                "weapon {k} has a full-damage splash radius that is not below its splash radius"
            ),
            WeaponsError::UnknownDamageType {
                weapon,
                damage_type,
            } => write!(
                f,
                "weapon {weapon} has damage type {damage_type}, which the armor table lacks"
            ),
            WeaponsError::NoArmor(k) => {
                write!(f, "unit kind {} has no armor type in the armor table", k.0)
            }
            WeaponsError::NoSpeed(k) => {
                write!(f, "weapon {k} has a projectile that flies but no speed")
            }
            WeaponsError::NoHeight(k) => {
                write!(
                    f,
                    "weapon {k} has a falling projectile but no height to drop from"
                )
            }
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
                    "a loadout or an armor entry is for kind {}, which the unit table lacks",
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
            WeaponsError::HullWeaponOnTurret { unit, weapon } => write!(
                f,
                "unit kind {} has a turret, and weapon {weapon} is hull-mounted: it would never be aimed",
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
    /// Damage times armor, as percentages (WP5.3); `None` takes every hit at one hundred percent.
    armor: Option<ArmorTable>,
    /// Hash of everything above; zero when empty.
    digest: u64,
}

impl fmt::Debug for WeaponTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WeaponTable")
            .field("weapons", &self.weapons.len())
            .field("armed_kinds", &self.loadouts.len())
            .field("armor", &self.armor.is_some())
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
            if w.splash_radius.is_negative()
                || w.splash_inner.is_negative()
                || (w.splash_inner >= w.splash_radius && !w.splash_inner.is_zero())
            {
                return Err(WeaponsError::BadSplash(k));
            }
            if w.projectile != ProjectileKind::Instant && w.projectile_speed_per_tick <= Fx::ZERO {
                return Err(WeaponsError::NoSpeed(k));
            }
            if w.projectile == ProjectileKind::Falling && w.flight_height <= Fx::ZERO {
                return Err(WeaponsError::NoHeight(k));
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
            digest_of(&weapons, &loadouts, None)
        };
        Ok(WeaponTable {
            weapons,
            loadouts,
            armor: None,
            digest,
        })
    }

    /// The same table with an armor table (WP5.3): every hit then takes the percentage of its damage
    /// type against the victim's armor type. Every weapon's damage type has to be in the armor table.
    /// (That every unit kind of the world has an armor type is checked when the world is given the
    /// table, which knows the unit table.)
    pub fn with_armor(mut self, armor: ArmorTable) -> Result<WeaponTable, WeaponsError> {
        for w in &self.weapons {
            if armor.damage_types().binary_search(&w.damage_type).is_err() {
                return Err(WeaponsError::UnknownDamageType {
                    weapon: w.kind,
                    damage_type: w.damage_type,
                });
            }
        }
        self.digest = digest_of(&self.weapons, &self.loadouts, Some(&armor));
        self.armor = Some(armor);
        Ok(self)
    }

    /// The armor table, if the table has one.
    pub fn armor(&self) -> Option<&ArmorTable> {
        self.armor.as_ref()
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

fn digest_of(weapons: &[WeaponStats], loadouts: &[Loadout], armor: Option<&ArmorTable>) -> u64 {
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
        h.write_fx(w.splash_inner);
        h.write_u8(u8::from(w.friendly_fire));
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
    match armor {
        None => h.write_u8(0),
        Some(armor) => {
            h.write_u8(1);
            h.write_u64(armor.digest());
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

/// One pull of a trigger: recorded for the tick it was fired in, whether the shot hits at once (an
/// `Instant` weapon) or launches a projectile (any other kind; its detonation is an [`Impact`] some ticks
/// later).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Shot {
    /// The tick of the shot.
    pub tick: Tick,
    /// Who fired.
    pub attacker: EntityId,
    /// Who it was fired at.
    pub target: EntityId,
    /// The kind number of the weapon.
    pub weapon: u16,
    /// Which weapon slot of the attacker fired.
    pub slot: u8,
    /// The damage of the weapon, before the armor and before it is taken off the target's hit points. For a
    /// projectile this is what it will do if it hits, not what has been done (see [`Impact::damage`] for
    /// what a hit did after the armor).
    pub damage: u32,
    /// The target's hit points at the **end of the tick** of the shot: after every hit of that tick, from
    /// every shooter. All shots at one target in one tick report the same number, whatever the slots, so
    /// it does not depend on the order of the attackers. For an instant weapon that is what is left of
    /// the target after the hits; for a projectile weapon the shot has not landed yet and this is only
    /// the target's hit points at the time of the launch.
    ///
    /// A report, not state and not hashed. It says what was left, not who took it: **kill credit
    /// (WP5.5) must not be read from it**, nor from the order of [`Shot`]s or [`Impact`]s in a tick, which
    /// follows the slots. Credit has to be settled from the set of the tick's hits.
    pub hp_after: u32,
}

/// A projectile that went off: where, why, and the entity it counted as a direct hit on, if any.
/// Recorded for the tick of the detonation (`docs/rules/projectiles.md`).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Impact {
    /// The tick it went off.
    pub tick: Tick,
    /// The shooter, which may be gone or dead by now: a projectile lands anyway.
    pub owner: EntityId,
    /// The kind number of the weapon.
    pub weapon: u16,
    /// Where it went off.
    pub at: Vec2,
    /// Why.
    pub cause: ImpactCause,
    /// The entity it hit, if it hit one (a direct hit; the splash around it is `splash_victims`).
    pub victim: Option<EntityId>,
    /// The damage it did to the direct victim **after the armor**, before the sum of the tick saturated
    /// at zero: zero for a miss, and for a victim whose armor is immune to its damage type.
    pub damage: u32,
    /// How many other entities its splash hurt (a victim that took nothing, because the falloff or
    /// the armor made it zero, is not counted). Zero for a weapon without a splash.
    pub splash_victims: u32,
    /// The victim's hit points at the **end of the tick** (zero if there is no victim). A report, with
    /// the same caveat as [`Shot::hp_after`]: not for kill credit.
    pub hp_after: u32,
}

/// The weapons of a world: the table, where each unit kind's slots point into it, the projectiles in
/// flight, and the shots and impacts of the last tick. Empty (no weapons, nothing fires) until the caller
/// hands a table over.
#[derive(Clone, PartialEq, Eq, Default)]
pub(crate) struct Arsenal {
    pub table: WeaponTable,
    /// Per position in the unit table: the index into `table.weapons` of each slot, or
    /// [`NONE`].
    by_def: Vec<[u16; MAX_WEAPON_SLOTS]>,
    /// Per position in the unit table: the armor type of the kind, when the table has an armor table.
    armor_of_def: Vec<Option<u16>>,
    /// The widest radius of any unit kind that can be hurt: how far beyond the splash radius the middle of
    /// a victim can lie while its edge is inside it.
    widest: Fx,
    /// Where the splash looks its victims up. Scratch, not state.
    splash: SplashIndex,
    /// The projectiles in flight (WP5.2). State: hashed.
    pub projectiles: Projectiles,
    /// The shots of the last tick. Derived; not hashed.
    pub shots: Vec<Shot>,
    /// The detonations of the last tick. Derived; not hashed.
    pub impacts: Vec<Impact>,
    /// Who hurt whom in the last tick, as (victim slot, attacker), for the return fire of
    /// [`crate::targeting`] (WP5.4). Filled only in a world with targeting; derived, not hashed.
    pub attacks: Vec<(u32, EntityId)>,
    /// Every piece of damage of the last tick with its shooter, for the kill credit of
    /// [`crate::destruction`] (WP5.5). Filled only in a world with destruction; derived, not hashed.
    pub hurt: Vec<crate::destruction::Hurt>,
}

/// The spatial index of a splash's victims (the crowd pass's grid, built here from whatever can be hurt)
/// with the map it was configured for. Scratch memory: two arsenals are equal whatever it holds.
#[derive(Clone, Default)]
struct SplashIndex {
    index: Index,
    map: Option<(u32, u32)>,
}

impl PartialEq for SplashIndex {
    fn eq(&self, _: &SplashIndex) -> bool {
        true
    }
}

impl Eq for SplashIndex {}

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
                if weapon.mount == Mount::Hull && stats.has_turret() {
                    return Err(WeaponsError::HullWeaponOnTurret {
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
        let mut armor_of_def = vec![None; units.len()];
        if let Some(armor) = &table.armor {
            if let Some(kind) = armor.unit_kinds().find(|&k| units.index_of(k).is_none()) {
                return Err(WeaponsError::UnknownUnit(kind));
            }
            for (def, slot) in armor_of_def.iter_mut().enumerate() {
                let kind = units.at(def).kind;
                *slot = Some(armor.armor_of(kind).ok_or(WeaponsError::NoArmor(kind))?);
            }
        }
        let widest = (0..units.len())
            .map(|def| units.at(def))
            .filter(|stats| stats.hp > 0)
            .map(radius_of_stats)
            .max()
            .unwrap_or(Fx::ZERO);
        Ok(Arsenal {
            table,
            by_def,
            armor_of_def,
            widest,
            splash: SplashIndex::default(),
            projectiles: Projectiles::default(),
            shots: Vec::new(),
            impacts: Vec::new(),
            attacks: Vec::new(),
            hurt: Vec::new(),
        })
    }

    /// True while the world has no weapons.
    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    /// Whether the unit kind at table position `def` carries a weapon.
    pub fn is_armed(&self, def: u32) -> bool {
        self.by_def
            .get(def as usize)
            .is_some_and(|slots| slots[0] != NONE)
    }

    /// The weapons of the unit kind at table position `def`, in slot order.
    pub fn weapons_of(&self, def: u32) -> impl Iterator<Item = &WeaponStats> + '_ {
        self.by_def[def as usize]
            .iter()
            .take_while(|&&at| at != NONE)
            .map(move |&at| &self.table.weapons[at as usize])
    }

    /// How much of `weapon`'s damage the armor of the unit kind at table position `def` takes, in
    /// percent: one hundred when the world has no armor table (a hit then does what the weapon says).
    pub fn percent(&self, weapon: &WeaponStats, def: u32) -> u16 {
        percent_of(
            self.table.armor.as_ref(),
            &self.armor_of_def,
            weapon.damage_type,
            def,
        )
    }

    /// The widest radius of any kind that can be hurt: how far beyond a distance the middle of a
    /// victim can lie while its edge is inside it.
    pub fn widest(&self) -> Fx {
        self.widest
    }

    /// The weapon of the unit kind at table position `def` that does the most against the unit
    /// kind at `target_def`, standing `d2` (squared, middle to middle) from it with `edge` the
    /// radius of the target. A weapon that does nothing to it (zero percent) is never chosen. With
    /// `in_reach_only` only a weapon that could fire at it now counts (within its range and not within
    /// its minimum range, both measured to the edge, as [`can_fire`] does); without, the distance is
    /// ignored. The first of equally good weapons in slot order wins.
    pub fn best_against(
        &self,
        def: u32,
        target_def: u32,
        d2: Fx,
        edge: Fx,
        in_reach_only: bool,
    ) -> Option<Pick> {
        let mut best: Option<Pick> = None;
        for w in self.weapons_of(def) {
            let percent = self.percent(w, target_def);
            if percent == 0 || (in_reach_only && !in_reach(w, d2, edge)) {
                continue;
            }
            let score = weapon_score(w, percent);
            if best.is_none_or(|b| score > b.score) {
                best = Some(Pick {
                    score,
                    reach: w.range,
                });
            }
        }
        best
    }
}

/// What a weapon is worth against one kind of target, for choosing targets (WP5.4): the best weapon's
/// score and its range.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Pick {
    /// The damage per second the weapon does to that target, in percent thousandths (see
    /// [`weapon_score`]).
    pub score: u64,
    /// The range of the weapon, in cells.
    pub reach: Fx,
}

/// The expected damage of a weapon against an armor that takes `percent` of it, per second of
/// reloads, as one integer: `damage * burst * percent * 1000 / reload ticks`. It is the armor
/// table's opinion (WP5.3) scaled by how hard and how often the weapon hits, so a weapon prefers
/// what its damage type does most to, and between two weapons of one unit the one that does more
/// wins. Nothing is rounded to a hit point: it only has to order targets. The most it can be is
/// 10^6 * 16 * 400 * 1000, which fits a `u64` with room.
pub fn weapon_score(w: &WeaponStats, percent: u16) -> u64 {
    u64::from(w.damage) * u64::from(w.burst) * u64::from(percent) * 1000 / u64::from(w.reload_ticks)
}

/// The percent of damage of `damage_type` that the armor of the unit kind at table position `def`
/// takes: one hundred when there is no armor table, and for a kind without an armor type (an entity
/// that is not made from a unit kind has `NO_DEF`, which no position of the table is).
fn percent_of(
    armor: Option<&ArmorTable>,
    armor_of_def: &[Option<u16>],
    damage_type: u16,
    def: u32,
) -> u16 {
    let Some(armor) = armor else {
        return 100;
    };
    match armor_of_def.get(def as usize) {
        Some(&Some(kind)) => armor.percent(damage_type, kind).unwrap_or(100),
        _ => 100,
    }
}

/// How a hit came about: an instant weapon's, or a projectile's detonation.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum HitKind {
    /// An instant shot: no projectile.
    Instant,
    /// A projectile that went off, for this reason.
    Projectile(ImpactCause),
}

/// A hit decided in the first pass: a weapon went off at a point, with this victim if it counts as a
/// direct hit. Nothing has been taken from anyone yet.
#[derive(Copy, Clone, Debug)]
struct Hit {
    /// Index into the weapon table.
    weapon: usize,
    /// The shooter.
    owner: EntityId,
    /// The shooter's player, as it was when the shot was fired (a splash that spares its own side asks).
    player: PlayerId,
    /// Where it went off (the victim's middle for an instant shot).
    at: Vec2,
    /// The direct victim's slot.
    victim: Option<usize>,
    kind: HitKind,
}

/// What a hit did, for the reports: the damage to the direct victim after the armor, and how many others
/// the splash hurt.
#[derive(Copy, Clone, Debug, Default)]
struct Dealt {
    direct: u32,
    splash_victims: u32,
}

/// What [`damage_of`] reads: the entities as they were at the start of the weapon stage, the armor table,
/// and the index of what a splash can reach. Nothing in it changes while the hits are turned into damage.
struct Blast<'a> {
    e: &'a Entities,
    armor: Option<&'a ArmorTable>,
    armor_of_def: &'a [Option<u16>],
    index: &'a Index,
    widest: Fx,
}

impl Blast<'_> {
    /// The percent of `damage_type` that the armor of entity `j` takes: one hundred when the world has no
    /// armor table (a hit then does what the weapon says), and for an entity that has no unit kind.
    fn percent(&self, j: usize, damage_type: u16) -> u16 {
        percent_of(self.armor, self.armor_of_def, damage_type, self.e.def[j])
    }
}

/// **What a hit does to the world** (WP5.3): pushes `(victim slot, damage)` for everyone it hurts.
///
/// * The **direct victim** (if the hit has one) takes the weapon's `damage` times the armor's percentage
///   for its damage type, at full strength: a direct hit is the centre of its own splash and takes it
///   once, not twice.
/// * With a `splash_radius`, **every other** entity that can be hurt (alive, with hit points at the start
///   of the weapon stage) whose edge is nearer than the radius to `hit.at` takes the damage times the
///   falloff times its armor's percentage. The falloff is one up to `splash_inner` from the edge and then
///   falls in a straight line to nothing at `splash_radius`. The shooter is no exception, and a weapon
///   without `friendly_fire` spares the units of the shooter's own player.
/// * The number is worked out once by [`scaled_damage`]: one rounding, and at least one hit point for any
///   positive damage, percentage and falloff.
///
/// It reads the state of the start of the tick only, and the order in which it pushes does not matter:
/// the pushes are summed per victim afterwards.
fn damage_of(blast: &Blast<'_>, hit: &Hit, w: &WeaponStats, out: &mut Vec<(u32, u32)>) -> Dealt {
    let mut dealt = Dealt::default();
    if let Some(j) = hit.victim {
        dealt.direct = scaled_damage(w.damage, blast.percent(j, w.damage_type), 1, 1);
        out.push((j as u32, dealt.direct));
    }
    if w.splash_radius.is_zero() {
        return dealt;
    }
    // A victim whose edge is inside the radius has its middle inside the radius plus its own.
    let reach = Vec2::splat(w.splash_radius + blast.widest);
    let width = (w.splash_radius - w.splash_inner).raw().unsigned_abs();
    for stretch in blast.index.in_rect(hit.at - reach, hit.at + reach) {
        for near in stretch {
            let j = near.id as usize;
            if hit.victim == Some(j) {
                continue;
            }
            if !w.friendly_fire && blast.e.owner[j] == hit.player {
                continue;
            }
            let edge = ((near.at - hit.at).length() - near.radius).max(Fx::ZERO);
            if edge >= w.splash_radius {
                continue;
            }
            let (num, den) = if edge <= w.splash_inner {
                (1, 1)
            } else {
                ((w.splash_radius - edge).raw().unsigned_abs(), width)
            };
            let damage = scaled_damage(w.damage, blast.percent(j, w.damage_type), num, den);
            if damage > 0 {
                out.push((j as u32, damage));
                dealt.splash_victims += 1;
            }
        }
    }
    dealt
}

/// Runs one tick of every weapon after the units have moved and the turrets turned, in two passes
/// (`docs/rules/projectiles.md`). The first decides everything from the state at the start of the
/// tick: the projectiles in flight move and decide whether they go off, then the weapons count down and
/// decide whether they fire (an instant hit, or a launch). The second applies all the damage at once.
/// `map` is the size of the world's map in cells (the splash's index covers it).
///
/// `record_attacks` (a world with targeting, WP5.4) lists who hurt whom this tick in
/// [`Arsenal::attacks`], for the return fire that follows.
pub(crate) fn update_all(
    e: &mut Entities,
    units: &UnitTable,
    arsenal: &mut Arsenal,
    tick: Tick,
    map: (u32, u32),
    record_attacks: bool,
    record_hurt: bool,
) {
    arsenal.shots.clear();
    arsenal.impacts.clear();
    arsenal.attacks.clear();
    arsenal.hurt.clear();
    if arsenal.is_empty() {
        return;
    }
    let Arsenal {
        table,
        by_def,
        armor_of_def,
        widest,
        splash,
        projectiles,
        shots,
        impacts,
        attacks,
        hurt: hurt_list,
    } = arsenal;
    let mut hits: Vec<Hit> = Vec::new();
    let mut spent: Vec<u32> = Vec::new();
    let mut launches: Vec<Projectile> = Vec::new();

    // Decide, 1: the projectiles in flight move (those launched this tick are not in the store yet,
    // so they start flying next tick) and some go off. A projectile reads the entities and itself, never
    // another projectile.
    for index in 0..projectiles.slot_count() {
        let Some(p) = projectiles.row_mut(index) else {
            continue;
        };
        let at = table
            .weapons
            .binary_search_by_key(&p.weapon, |w| w.kind)
            .ok();
        // A projectile of a weapon the table lacks cannot happen (the table is fixed); it would
        // vanish rather than fly forever.
        let burst = at.and_then(|at| projectiles::fly(p, &table.weapons[at], e, units));
        if burst.is_some() || at.is_none() {
            spent.push(index as u32);
        }
        if let (Some(burst), Some(at)) = (burst, at) {
            hits.push(Hit {
                weapon: at,
                owner: p.owner,
                player: p.player,
                at: burst.at,
                victim: burst.victim,
                kind: HitKind::Projectile(burst.cause),
            });
        }
    }

    // Decide, 2: the weapons count down and fire.
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
        // The victim of an attack order, or the target the unit chose itself (WP5.4, `Pursue`). A unit
        // that chose its target does not fire a weapon that does nothing to it (zero percent in the
        // armor table); an attack order fires all weapons that can reach, as before.
        let (victim, chosen) = match e.order[i] {
            Order::Attack(id) => (live_slot(e, id).filter(|&j| j != i), false),
            Order::Pursue(id) => (live_slot(e, id).filter(|&j| j != i), true),
            _ => (None, false),
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
            let Some(j) = victim.filter(|&j| {
                can_fire(e, units, i, j, weapon)
                    && (!chosen
                        || percent_of(
                            table.armor.as_ref(),
                            armor_of_def,
                            weapon.damage_type,
                            e.def[j],
                        ) > 0)
            }) else {
                let state = &mut e.weapon[i][s];
                if state.burst_left > 0 {
                    // The burst cannot go on: drop it, and wait out the rest of the reload.
                    state.burst_left = 0;
                    state.wait = weapon.tail_ticks();
                }
                continue;
            };
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
            let owner = EntityId::new(i as u32, e.generation[i]);
            let target = EntityId::new(j as u32, e.generation[j]);
            if weapon.projectile == ProjectileKind::Instant {
                hits.push(Hit {
                    weapon: at as usize,
                    owner,
                    player: e.owner[i],
                    at: e.pos[j],
                    victim: Some(j),
                    kind: HitKind::Instant,
                });
            } else {
                let pointing = match weapon.mount {
                    Mount::Hull => e.facing[i],
                    Mount::Turret => e.turret[i],
                };
                launches.push(projectiles::launch(
                    weapon, owner, e.owner[i], e.pos[i], pointing, target, e.pos[j],
                ));
            }
            shots.push(Shot {
                tick,
                attacker: owner,
                target,
                weapon: weapon.kind,
                slot: s as u8,
                damage: weapon.damage,
                hp_after: 0,
            });
        }
    }

    // Apply. Nothing above changed a hit point. The projectiles that went off leave the store and the
    // launched ones enter it; every hit is turned into damage, summed per victim and taken off once,
    // saturating at zero.
    for index in spent {
        projectiles.remove(index);
    }
    for p in launches {
        projectiles.insert(p);
    }
    // The splash looks its victims up in the crowd pass's grid, built here from everything that can be
    // hurt, and only on a tick in which some hit has a splash.
    if hits
        .iter()
        .any(|h| !table.weapons[h.weapon].splash_radius.is_zero())
    {
        if splash.map != Some(map) {
            splash.index.configure(SPLASH_BUCKET, map.0, map.1);
            splash.map = Some(map);
        }
        let hurtable = |i: usize| e.alive[i] && e.def[i] != NO_DEF && e.hp[i] > 0;
        splash.index.build_with(e, e.len(), |i| {
            hurtable(i).then(|| (false, radius_of_stats(units.at(e.def[i] as usize))))
        });
    }
    let mut damage: Vec<(u32, u32)> = Vec::new();
    let mut dealt: Vec<Dealt> = Vec::with_capacity(hits.len());
    {
        let blast = Blast {
            e,
            armor: table.armor.as_ref(),
            armor_of_def,
            index: &splash.index,
            widest: *widest,
        };
        for hit in &hits {
            let from = damage.len();
            dealt.push(damage_of(
                &blast,
                hit,
                &table.weapons[hit.weapon],
                &mut damage,
            ));
            if record_hurt {
                for &(victim, damage) in &damage[from..] {
                    hurt_list.push(crate::destruction::Hurt {
                        victim,
                        damage,
                        shooter: hit.owner,
                        player: hit.player,
                    });
                }
            }
            if record_attacks {
                // Whoever the hit hurt, apart from the shooter and its own side (a splash can reach
                // both), was attacked by the shooter.
                for &(victim, hurt) in &damage[from..] {
                    let own_side = blast.e.owner[victim as usize] == hit.player;
                    if hurt > 0 && victim != hit.owner.index() && !own_side {
                        attacks.push((victim, hit.owner));
                    }
                }
            }
        }
    }
    damage.sort_unstable();
    let mut rest = damage.as_slice();
    while let Some(&(victim, _)) = rest.first() {
        let n = rest.iter().take_while(|d| d.0 == victim).count();
        let sum: u64 = rest[..n].iter().map(|d| u64::from(d.1)).sum();
        let j = victim as usize;
        e.hp[j] = e.hp[j].saturating_sub(u32::try_from(sum).unwrap_or(u32::MAX));
        rest = &rest[n..];
    }

    // Reports, from the state at the end of the tick.
    for shot in shots.iter_mut() {
        shot.hp_after = e.hp[shot.target.index() as usize];
    }
    for (hit, dealt) in hits.iter().zip(&dealt) {
        let HitKind::Projectile(cause) = hit.kind else {
            continue;
        };
        impacts.push(Impact {
            tick,
            owner: hit.owner,
            weapon: table.weapons[hit.weapon].kind,
            at: hit.at,
            cause,
            victim: hit.victim.map(|j| EntityId::new(j as u32, e.generation[j])),
            damage: dealt.direct,
            splash_victims: dealt.splash_victims,
            hp_after: hit.victim.map_or(0, |j| e.hp[j]),
        });
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
    if !in_reach(w, d2, edge) {
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

/// Whether a target whose middle is `d2` (squared) away and whose edge is `edge` from its middle is
/// within the reach of weapon `w`: not farther than `range` and not nearer than `min_range`, both to the
/// edge (the test by which an attack order counts as arrived).
pub(crate) fn in_reach(w: &WeaponStats, d2: Fx, edge: Fx) -> bool {
    d2 <= (w.range + edge).square()
        && (w.min_range.is_zero() || d2 >= (w.min_range + edge).square())
}

#[cfg(test)]
mod tests;
