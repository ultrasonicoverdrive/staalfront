//! What the simulation gets from the definitions: plain integers and `sf_math`
//! fixed point, no text and no floats (this file is checked for float types by
//! a test).
//!
//! Everything is already in the sim's own units. Distances and sizes are
//! [`Fx`] cells, speeds are [`Fx`] cells per game second (the sim divides by 20
//! with `sf_sim::per_tick`), times are whole ticks at 20 Hz and a turn rate is
//! facing steps per tick. Names became indices: the kind of a unit is its
//! position in `unit_defs.ron`, and a weapon, movement class, armor type or
//! damage type is its position in its own list.
//!
//! The types live here, in `sf_data`, rather than in `sf_sim`: `sf_sim` does not
//! use them until WP3.2, and the dependency direction (`sf_sim` reading
//! `sf_data` or the plain structs moving) is for that WP to settle. Nothing
//! about them is serialized, so moving them later changes no file.

use sf_math::Fx;

use super::classes::ClassTables;
use super::decimal::Decimal;
use super::unit::{Category, Faction, Flags, UnitDef};
use super::weapon::{Projectile, WeaponDef};

/// A weapon for the simulation.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SimWeaponDef {
    /// Longest distance it can hit, in cells.
    pub range: Fx,
    /// Shortest distance it can hit, in cells (zero for none).
    pub min_range: Fx,
    /// Ticks between two shots, at least 1.
    pub reload_ticks: u32,
    /// Damage of one hit, before armor.
    pub damage: u32,
    /// Index into the damage types of `unit_classes.ron`.
    pub damage_type: u8,
    /// How the projectile travels.
    pub projectile: Projectile,
    /// Splash radius in cells (zero for none).
    pub splash_radius: Fx,
}

/// The abilities of a role, with references turned into kind numbers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimFlags {
    /// Captures structures.
    pub can_capture: bool,
    /// Repairs structures.
    pub repairs_structures: bool,
    /// Places demolition charges.
    pub places_charges: bool,
    /// Used up when it destroys a structure.
    pub consumed_on_use: bool,
    /// Harvests resources.
    pub harvests: bool,
    /// Deploys to fire.
    pub deploys: bool,
    /// Infantry carried.
    pub transports: u8,
    /// Vehicles carried.
    pub transports_vehicles: u8,
    /// Garrison size.
    pub garrison: u8,
    /// Healing radius in cells, zero for none.
    pub heal_radius: Fx,
    /// Repair radius in cells, zero for none.
    pub repair_radius: Fx,
    /// Invisible while still in cover.
    pub stealth_in_cover: bool,
    /// Kinds it can build in the field.
    pub builds: Vec<u16>,
    /// Kind it looks like to the enemy AI.
    pub disguise_as: Option<u16>,
    /// Kind that comes free with it.
    pub comes_with: Option<u16>,
    /// Coastal placement only.
    pub needs_water: bool,
    /// Build radius in cells, zero for none.
    pub build_radius: Fx,
    /// Enables the minimap.
    pub minimap: bool,
    /// Death explosion: radius in cells and damage.
    pub death_blast: Option<(Fx, u32)>,
    /// Picked up by walking onto it.
    pub pickup: bool,
    /// Fires at everyone.
    pub fires_at_all: bool,
}

/// A role for the simulation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimUnitDef {
    /// The kind number: the position of the role in `unit_defs.ron`.
    pub kind: u16,
    /// What sort of thing it is.
    pub category: Category,
    /// Bit mask of the factions that can use it ([`Faction::bit`]).
    pub factions: u8,
    /// Price in credits; `None` if it cannot be bought.
    pub cost: Option<u32>,
    /// Build time in ticks (zero if it cannot be built).
    pub build_ticks: u32,
    /// Hit points (zero: cannot be damaged).
    pub hp: u32,
    /// Index into the armor types.
    pub armor_type: u8,
    /// Power produced (positive) or used (negative).
    pub power: i32,
    /// Index into the movement classes; `None` for things that never move.
    pub movement_class: Option<u8>,
    /// Speed in cells per game second.
    pub speed: Fx,
    /// Speed on water in cells per game second (equal to `speed` unless the
    /// role says otherwise).
    pub water_speed: Fx,
    /// Facing steps (of 256) per tick.
    pub turn_per_tick: u8,
    /// Diameter in cells (zero for structures).
    pub size: Fx,
    /// Footprint `(width, height)` in cells; structures only.
    pub footprint: Option<(u8, u8)>,
    /// Sight in cells.
    pub sight: Fx,
    /// Indices into the weapon list.
    pub weapons: Vec<u16>,
    /// Abilities.
    pub flags: SimFlags,
}

impl SimUnitDef {
    /// Whether the faction can use this role.
    pub const fn available_to(&self, faction: Faction) -> bool {
        self.factions & faction.bit() != 0
    }
}

/// Converts the weapons. Validation has already checked every number, so a
/// failure here is a bug and reported as text.
pub(crate) fn convert_weapons(
    weapons: &[WeaponDef],
    classes: &ClassTables,
) -> Result<Vec<SimWeaponDef>, String> {
    weapons
        .iter()
        .map(|w| {
            Ok(SimWeaponDef {
                range: w.range.to_fx(),
                min_range: w.min_range.to_fx(),
                reload_ticks: ticks(w.reload, &w.id)?,
                damage: w.damage,
                damage_type: classes
                    .damage_index(&w.damage_type)
                    .ok_or_else(|| format!("weapon {}: damage type", w.id))?,
                projectile: w.projectile,
                splash_radius: w.splash_radius.to_fx(),
            })
        })
        .collect()
}

/// Converts the units; `weapons` is the list the units refer to.
pub(crate) fn convert_units(
    units: &[UnitDef],
    weapons: &[WeaponDef],
    classes: &ClassTables,
) -> Result<Vec<SimUnitDef>, String> {
    let kind_of = |id: &str| -> Result<u16, String> {
        units
            .iter()
            .position(|u| u.id == id)
            .and_then(|i| u16::try_from(i).ok())
            .ok_or_else(|| format!("unknown unit {id}"))
    };
    units
        .iter()
        .enumerate()
        .map(|(i, u)| {
            let id = &u.id;
            let weapon_ids = u
                .weapons
                .iter()
                .map(|w| {
                    weapons
                        .iter()
                        .position(|d| d.id == *w)
                        .and_then(|k| u16::try_from(k).ok())
                        .ok_or_else(|| format!("unit {id}: weapon {w}"))
                })
                .collect::<Result<Vec<u16>, String>>()?;
            let movement_class = u
                .movement
                .as_deref()
                .map(|m| {
                    classes
                        .movement_index(m)
                        .ok_or_else(|| format!("unit {id}: movement class {m}"))
                })
                .transpose()?;
            let speed = u.speed.to_fx();
            Ok(SimUnitDef {
                kind: u16::try_from(i).map_err(|_| "too many units".to_string())?,
                category: u.category,
                factions: u.factions.iter().fold(0, |m, f| m | f.bit()),
                cost: u.cost,
                build_ticks: ticks(u.build_seconds, id)?,
                hp: u.hp,
                armor_type: classes
                    .armor_index(&u.armor)
                    .ok_or_else(|| format!("unit {id}: armor type"))?,
                power: u.power,
                movement_class,
                speed,
                water_speed: u.water_speed.map_or(speed, Decimal::to_fx),
                turn_per_tick: u.turn_rate,
                size: u.size.to_fx(),
                footprint: u.footprint.map(|f| (f.width, f.height)),
                sight: u.sight.to_fx(),
                weapons: weapon_ids,
                flags: convert_flags(&u.flags, &kind_of)?,
            })
        })
        .collect()
}

fn ticks(d: Decimal, id: &str) -> Result<u32, String> {
    d.to_ticks()
        .ok_or_else(|| format!("{id}: {d} s is not a whole number of ticks"))
}

fn convert_flags(
    f: &Flags,
    kind_of: &dyn Fn(&str) -> Result<u16, String>,
) -> Result<SimFlags, String> {
    Ok(SimFlags {
        can_capture: f.can_capture,
        repairs_structures: f.repairs_structures,
        places_charges: f.places_charges,
        consumed_on_use: f.consumed_on_use,
        harvests: f.harvests,
        deploys: f.deploys,
        transports: f.transports,
        transports_vehicles: f.transports_vehicles,
        garrison: f.garrison,
        heal_radius: f.heal_radius.to_fx(),
        repair_radius: f.repair_radius.to_fx(),
        stealth_in_cover: f.stealth_in_cover,
        builds: f
            .builds
            .iter()
            .map(|id| kind_of(id))
            .collect::<Result<_, _>>()?,
        disguise_as: f.disguise_as.as_deref().map(kind_of).transpose()?,
        comes_with: f.comes_with.as_deref().map(kind_of).transpose()?,
        needs_water: f.needs_water,
        build_radius: f.build_radius.to_fx(),
        minimap: f.minimap,
        death_blast: f.death_blast.map(|b| (b.radius.to_fx(), b.damage)),
        pickup: f.pickup,
        fires_at_all: f.fires_at_all,
    })
}
