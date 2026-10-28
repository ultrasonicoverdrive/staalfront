//! What the simulation gets from the definitions: plain integers and `sf_math`
//! fixed point, no text and no floats (this file is checked for float types by
//! a test).
//!
//! Everything is already in the sim's own units. Distances and sizes are
//! [`Fx`] cells, times are whole ticks at 20 Hz and a turn rate is facing steps
//! per tick. A speed comes twice: `speed` in cells per game second (for tools
//! and display) and `speed_per_tick`, which the sim moves by. The per-tick value
//! is rounded once from the exact decimal; dividing `speed` by 20 afterwards
//! (`sf_sim::per_tick`) would round twice and is off by one raw unit for about
//! 2.5 % of all speeds. Names became **kind numbers**, the permanent `kind`
//! field of the records: of a unit, a weapon, a movement class, an armor type
//! and a damage type. The lists are sorted by kind and the numbers can have
//! gaps (tombstones), so look a number up with `UnitDefs::sim_unit` and
//! `UnitDefs::sim_weapon` instead of indexing the slices.
//!
//! The types live here, in `sf_data`, and `sf_sim` does not depend on this
//! crate (WP3.2 kept it that way, as for the terrain): `sf_sim` has its own
//! `UnitStats` with the few fields its rules read, and the small crate
//! `sf_bridge` copies a `SimUnitDef` into one (for `sf_game` and `sf_headless`). Nothing about these types is serialized, so
//! moving them later changes no file.

use sf_math::Fx;

use super::building::{BuildingFunction, Side};
use super::classes::ClassTables;
use super::decimal::Decimal;
use super::unit::{Category, Faction, Flags, UnitDef};
use super::weapon::{Projectile, WeaponDef, WeaponMount};

/// A weapon for the simulation.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SimWeaponDef {
    /// The permanent number of the weapon.
    pub kind: u16,
    /// Longest distance it can hit, in cells.
    pub range: Fx,
    /// Shortest distance it can hit, in cells (zero for none).
    pub min_range: Fx,
    /// Ticks between two shots, at least 1.
    pub reload_ticks: u32,
    /// Damage of one hit, before armor.
    pub damage: u32,
    /// Kind number of the damage type in `unit_classes.ron`.
    pub damage_type: u16,
    /// How the projectile travels.
    pub projectile: Projectile,
    /// Splash radius in cells (zero for none).
    pub splash_radius: Fx,
    /// Cells from the edge of a victim inside which the splash does its full
    /// damage (zero: it falls from the start).
    pub splash_inner: Fx,
    /// Whether the splash hurts the shooter's own side.
    pub friendly_fire: bool,
    /// Projectile speed in cells per game second (for tools and display);
    /// zero when the file gives none and for an `Instant` weapon.
    pub projectile_speed: Fx,
    /// Projectile speed per tick, rounded once from the exact decimal like a
    /// unit's `speed_per_tick`; what a projectile moves by (WP5.2).
    pub projectile_speed_per_tick: Fx,
    /// Height of the arc or the drop in cells; zero for none.
    pub flight_height: Fx,
    /// Facing steps (of 256) per tick a guided projectile steers; zero for
    /// none.
    pub guide_turn_per_tick: u8,
    /// Shots per burst, at least 1.
    pub burst: u8,
    /// Ticks between the shots of a burst; zero exactly when `burst` is 1.
    /// Over the whole burst it is shorter than the reload:
    /// `(burst - 1) * burst_delay_ticks < reload_ticks`.
    pub burst_delay_ticks: u32,
    /// What the weapon aims with.
    pub mount: WeaponMount,
    /// Half the width of the firing arc in facing steps (of 256), 0 to 128.
    pub fire_arc: u8,
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
    /// The permanent kind number of the role (`kind` in `unit_defs.ron`).
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
    /// Kind number of the armor type.
    pub armor_type: u16,
    /// Power produced (positive) or used (negative).
    pub power: i32,
    /// Kind number of the movement class; `None` for things that never move.
    pub movement_class: Option<u16>,
    /// Speed in cells per game second (for tools and display).
    pub speed: Fx,
    /// Speed on water in cells per game second (equal to `speed` unless the
    /// role says otherwise).
    pub water_speed: Fx,
    /// Distance per tick, rounded once from the exact decimal: what the
    /// simulation moves by.
    pub speed_per_tick: Fx,
    /// `water_speed` per tick, rounded once.
    pub water_speed_per_tick: Fx,
    /// Facing steps (of 256) per tick.
    pub turn_per_tick: u8,
    /// Facing steps (of 256) per tick the turret turns; zero for a unit without
    /// a turret that turns apart from the hull.
    pub turret_per_tick: u8,
    /// Diameter in cells (zero for structures).
    pub size: Fx,
    /// Footprint `(width, height)` in cells; structures only.
    pub footprint: Option<(u8, u8)>,
    /// Sight in cells.
    pub sight: Fx,
    /// Kind numbers of its weapons.
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

/// A structure for the simulation (WP6.1): what placement, power and production
/// will read. Integers only; the function and the side are the permanent
/// numbers of the enums.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimBuildingDef {
    /// The permanent kind number of the role.
    pub kind: u16,
    /// Bit mask of the factions that can use it ([`Faction::bit`]).
    pub factions: u8,
    /// Footprint `(width, height)` in cells.
    pub footprint: (u8, u8),
    /// What it does.
    pub function: BuildingFunction,
    /// The side units come out of or go in by; `Some` exactly for a factory or
    /// the harvester dock.
    pub exit_side: Option<Side>,
    /// Build time in ticks (zero if it cannot be built).
    pub build_ticks: u32,
    /// Price in credits; `None` if it cannot be bought (phase 7 reads it).
    pub cost: Option<u32>,
    /// Power produced (positive) or used (negative).
    pub power: i32,
    /// Kind numbers of the buildings it needs, ascending.
    pub requires: Vec<u16>,
    /// The `build_radius` flag: cells around the footprint in which the owner
    /// may build (the base anchor); zero for everything else.
    pub build_radius: Fx,
    /// Units of ore it stores for its owner while it stands (WP7.3); zero for most.
    pub storage: u32,
}

/// Converts the structures (the roles with a footprint).
pub(crate) fn convert_buildings(units: &[UnitDef]) -> Result<Vec<SimBuildingDef>, String> {
    let mut out = Vec::new();
    for u in units.iter().filter(|u| u.category == Category::Structure) {
        let fp = u
            .footprint
            .ok_or_else(|| format!("structure {} has no footprint", u.id))?;
        let b = u.building.clone().unwrap_or_default();
        let mut requires = b
            .requires
            .iter()
            .map(|r| {
                units
                    .iter()
                    .find(|d| d.id == *r)
                    .map(|d| d.kind)
                    .ok_or_else(|| format!("unit {}: requires {r}", u.id))
            })
            .collect::<Result<Vec<u16>, String>>()?;
        requires.sort_unstable();
        out.push(SimBuildingDef {
            kind: u.kind,
            factions: u.factions.iter().fold(0, |m, f| m | f.bit()),
            footprint: (fp.width, fp.height),
            function: b.function,
            exit_side: b.exit_side,
            build_ticks: ticks(u.build_seconds, &u.id)?,
            cost: u.cost,
            power: u.power,
            requires,
            build_radius: u.flags.build_radius.to_fx(),
            storage: b.storage,
        });
    }
    out.sort_by_key(|b| b.kind);
    Ok(out)
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
                kind: w.kind,
                range: w.range.to_fx(),
                min_range: w.min_range.to_fx(),
                reload_ticks: ticks(w.reload, &w.id)?,
                damage: w.damage,
                damage_type: classes
                    .damage_kind(&w.damage_type)
                    .ok_or_else(|| format!("weapon {}: damage type", w.id))?,
                projectile: w.projectile,
                splash_radius: w.splash_radius.to_fx(),
                splash_inner: w.splash_inner.to_fx(),
                friendly_fire: w.friendly_fire,
                projectile_speed: w.projectile_speed.to_fx(),
                projectile_speed_per_tick: w.projectile_speed.to_fx_per_tick(),
                flight_height: w.flight_height.to_fx(),
                guide_turn_per_tick: w.guide_turn,
                burst: w.burst,
                burst_delay_ticks: ticks(w.burst_delay, &w.id)?,
                mount: w.mount,
                fire_arc: w.fire_arc,
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
            .find(|u| u.id == id)
            .map(|u| u.kind)
            .ok_or_else(|| format!("unknown unit {id}"))
    };
    units
        .iter()
        .map(|u| {
            let id = &u.id;
            let weapon_ids = u
                .weapons
                .iter()
                .map(|w| {
                    weapons
                        .iter()
                        .find(|d| d.id == *w)
                        .map(|d| d.kind)
                        .ok_or_else(|| format!("unit {id}: weapon {w}"))
                })
                .collect::<Result<Vec<u16>, String>>()?;
            let movement_class = u
                .movement
                .as_deref()
                .map(|m| {
                    classes
                        .movement_kind(m)
                        .ok_or_else(|| format!("unit {id}: movement class {m}"))
                })
                .transpose()?;
            let speed = u.speed.to_fx();
            let speed_per_tick = u.speed.to_fx_per_tick();
            Ok(SimUnitDef {
                kind: u.kind,
                category: u.category,
                factions: u.factions.iter().fold(0, |m, f| m | f.bit()),
                cost: u.cost,
                build_ticks: ticks(u.build_seconds, id)?,
                hp: u.hp,
                armor_type: classes
                    .armor_kind(&u.armor)
                    .ok_or_else(|| format!("unit {id}: armor type"))?,
                power: u.power,
                movement_class,
                speed,
                water_speed: u.water_speed.map_or(speed, Decimal::to_fx),
                speed_per_tick,
                water_speed_per_tick: u
                    .water_speed
                    .map_or(speed_per_tick, Decimal::to_fx_per_tick),
                turn_per_tick: u.turn_rate,
                turret_per_tick: u.turret_rate,
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
