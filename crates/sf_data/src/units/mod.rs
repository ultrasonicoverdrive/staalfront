//! Unit, structure and weapon definitions (WP3.1).
//!
//! Game data in RON, loaded and checked here, so that adding a role is a data
//! change and no Rust change (PLAN.md section 5, rule 6). Three files:
//!
//! | File | Holds |
//! |---|---|
//! | `assets/config/unit_classes.ron` | movement classes, armor types, damage types (names that the other files use) |
//! | `assets/config/weapon_defs.ron` | weapons: range, reload, damage, damage type, projectile kind and its parameters, burst, mount and firing arc, minimum range, splash (`docs/rules/weapons.md`) |
//! | `assets/config/unit_defs.ron` | one record per role of `content/units.md`: cost, hit points, armor, movement, speed, weapons, model, icon, abilities |
//!
//! ([`UnitDefs::load`] reads the three; `units.ron` next to them is the
//! unrelated placeholder look of WP1.6.)
//!
//! **Numbers are permanent.** Every unit, weapon, movement class, armor type and
//! damage type has a `kind` number written into its record. The simulation,
//! scenarios, saves and replays use that number; it never changes, the order of
//! the records in the file is irrelevant, and a removed record leaves a
//! tombstone (`retired_*`) so the number is never reused.
//!
//! Loading checks the files and reports the first problem as a [`DefError`]
//! with the file and the line (`assets/config/unit_defs.ron:41:16: unit
//! "light_tank": unknown weapon "cannon2" (not in weapon_defs.ron)`): ids are
//! unique snake_case names, every weapon, movement class, armor type, damage
//! type and referenced unit exists, model and icon files exist (a glTF model
//! also lies in a pack of `config/models.ron`, for the generic `model` and for
//! each faction's entry of `models`), numbers are in range, and a structure has a footprint while a mobile unit has a movement
//! class and a speed. Numbers with decimals are read as thousandths
//! ([`Decimal`]) and never become floats; durations must be whole ticks.
//!
//! The result, [`UnitDefs`], also holds the conversion for the simulation:
//! [`SimUnitDef`] and [`SimWeaponDef`], integers and `sf_math` fixed point in
//! the sim's own units. `docs/rules/units.md` describes every field.
//!
//! ```
//! use sf_data::units::UnitDefs;
//! use sf_math::Fx;
//!
//! let defs = UnitDefs::embedded();
//! let kind = defs.kind_of("light_tank").unwrap();
//! let tank = defs.sim_unit(kind).unwrap();
//! assert_eq!(tank.hp, 400);
//! assert_eq!(tank.speed, Fx::from_int(2)); // cells per game second
//! assert_eq!(tank.speed_per_tick, Fx::from_ratio(1, 10)); // what the sim moves by
//! ```

mod classes;
mod decimal;
mod error;
mod kinds;
mod sim;
mod unit;
mod weapon;

use std::path::Path;

pub use classes::{
    ArmorType, ClassTables, DamageType, MAX_CLASS_KIND, MAX_COST_PERMILLE, MAX_DRIVE_TOLERANCE,
    MIN_COST_PERMILLE, MovementClass, TerrainCost, TurnMode, TurningDef,
};
pub use decimal::{Decimal, TICK_MILLIS, TICKS_PER_SECOND};
pub use error::DefError;
pub use kinds::Retired;
pub use sim::{SimFlags, SimUnitDef, SimWeaponDef};
pub use unit::{
    Category, DeathBlast, Faction, Flags, Footprint, IconShape, MAX_UNIT_KIND, MAX_UNITS, Model,
    ModelShape, UnitDef,
};
pub use weapon::{
    ALL_ROUND, DEFAULT_FIRE_ARC, MAX_BURST, MAX_GUIDE_TURN, MAX_WEAPON_KIND, Projectile, WeaponDef,
    WeaponMount,
};

use error::Source;

/// File names of the three definition files, relative to a config directory.
pub const CLASSES_FILE: &str = "unit_classes.ron";
/// See [`CLASSES_FILE`].
pub const WEAPONS_FILE: &str = "weapon_defs.ron";
/// See [`CLASSES_FILE`].
pub const UNITS_FILE: &str = "unit_defs.ron";
/// The model packs file, relative to the assets directory. Only the `dir` of
/// each pack is read here (to check that a role's model lies in a pack);
/// `sf_render` reads the rest.
pub const MODELS_FILE: &str = "config/models.ron";

/// The shipped `unit_classes.ron`, compiled in.
pub const EMBEDDED_CLASSES: &str = include_str!("../../../../assets/config/unit_classes.ron");
/// The shipped `weapon_defs.ron`, compiled in.
pub const EMBEDDED_WEAPONS: &str = include_str!("../../../../assets/config/weapon_defs.ron");
/// The shipped `unit_defs.ron`, compiled in.
pub const EMBEDDED_UNITS: &str = include_str!("../../../../assets/config/unit_defs.ron");

/// One definition file as text, with the name that errors should show.
#[derive(Copy, Clone, Debug)]
pub struct DefFile<'a> {
    /// Shown in error messages, normally the path.
    pub name: &'a str,
    /// The RON text.
    pub text: &'a str,
}

/// The three files, as text.
#[derive(Copy, Clone, Debug)]
pub struct DefSources<'a> {
    /// `unit_classes.ron`.
    pub classes: DefFile<'a>,
    /// `weapon_defs.ron`.
    pub weapons: DefFile<'a>,
    /// `unit_defs.ron`.
    pub units: DefFile<'a>,
}

impl DefSources<'static> {
    /// The shipped files, compiled in.
    pub fn embedded() -> Self {
        DefSources {
            classes: DefFile {
                name: "assets/config/unit_classes.ron",
                text: EMBEDDED_CLASSES,
            },
            weapons: DefFile {
                name: "assets/config/weapon_defs.ron",
                text: EMBEDDED_WEAPONS,
            },
            units: DefFile {
                name: "assets/config/unit_defs.ron",
                text: EMBEDDED_UNITS,
            },
        }
    }
}

/// The directories of the packs in `models.ron` under `assets`; `None` if the
/// file is not there (a tool with a partial assets directory).
fn pack_dirs(assets: &Path) -> Result<Option<Vec<String>>, DefError> {
    #[derive(serde::Deserialize)]
    struct Packs {
        packs: std::collections::BTreeMap<String, Dir>,
    }
    #[derive(serde::Deserialize)]
    struct Dir {
        dir: String,
    }
    let path = assets.join(MODELS_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let file: Packs = ron::from_str(&text)
        .map_err(|e| DefError::file(path.display().to_string(), format!("cannot parse: {e}")))?;
    Ok(Some(file.packs.into_values().map(|d| d.dir).collect()))
}

/// The validated definitions: tables, weapons and roles, and their conversion
/// for the simulation.
#[derive(Clone, Debug)]
pub struct UnitDefs {
    classes: ClassTables,
    weapons: Vec<WeaponDef>,
    units: Vec<UnitDef>,
    retired_weapons: Vec<Retired>,
    retired_units: Vec<Retired>,
    sim_weapons: Vec<SimWeaponDef>,
    sim_units: Vec<SimUnitDef>,
}

impl UnitDefs {
    /// Parses and validates the three files.
    ///
    /// With `assets_dir` set, every model and icon path must name an existing
    /// file under it; with `None` only the shape of the paths is checked (for
    /// tests and tools that have no assets at hand).
    pub fn from_sources(
        sources: &DefSources<'_>,
        assets_dir: Option<&Path>,
    ) -> Result<Self, DefError> {
        let classes_src = Source::new(sources.classes.name, sources.classes.text);
        let weapons_src = Source::new(sources.weapons.name, sources.weapons.text);
        let units_src = Source::new(sources.units.name, sources.units.text);

        let classes = ClassTables::parse(classes_src)?;
        let (weapons, retired_weapons) = weapon::parse(weapons_src, &classes)?;
        let packs = assets_dir.map(pack_dirs).transpose()?.flatten();
        let (units, retired_units) = unit::parse(
            units_src,
            &classes,
            &weapons,
            unit::Assets {
                dir: assets_dir,
                packs: packs.as_deref(),
            },
        )?;
        let sim_weapons = sim::convert_weapons(&weapons, &classes)
            .map_err(|e| units_src.error(format!("internal: converting weapons: {e}")))?;
        let sim_units = sim::convert_units(&units, &weapons, &classes)
            .map_err(|e| units_src.error(format!("internal: converting units: {e}")))?;
        Ok(UnitDefs {
            classes,
            weapons,
            units,
            retired_weapons,
            retired_units,
            sim_weapons,
            sim_units,
        })
    }

    /// Reads `unit_classes.ron`, `weapon_defs.ron` and `unit_defs.ron` from
    /// `config_dir` (normally `assets/config`) and checks the model and icon
    /// files against `assets_dir` (normally `assets`).
    pub fn load(config_dir: &Path, assets_dir: &Path) -> Result<Self, DefError> {
        let read = |file: &str| -> Result<(String, String), DefError> {
            let path = config_dir.join(file);
            let name = path.display().to_string();
            std::fs::read_to_string(&path)
                .map(|text| (name.clone(), text))
                .map_err(|e| DefError::file(name, format!("cannot read: {e}")))
        };
        let (cn, ct) = read(CLASSES_FILE)?;
        let (wn, wt) = read(WEAPONS_FILE)?;
        let (un, ut) = read(UNITS_FILE)?;
        let sources = DefSources {
            classes: DefFile {
                name: &cn,
                text: &ct,
            },
            weapons: DefFile {
                name: &wn,
                text: &wt,
            },
            units: DefFile {
                name: &un,
                text: &ut,
            },
        };
        Self::from_sources(&sources, Some(assets_dir))
    }

    /// The shipped definitions, compiled in. Model and icon files are not
    /// looked up (the binary may run from anywhere); `tests/units.rs` checks
    /// them against the files in the repository.
    ///
    /// # Panics
    ///
    /// If the shipped files are invalid, which a test rules out.
    pub fn embedded() -> Self {
        Self::from_sources(&DefSources::embedded(), None)
            .unwrap_or_else(|e| panic!("the embedded unit definitions are valid: {e}"))
    }

    /// The movement class, armor type and damage type tables.
    pub fn classes(&self) -> &ClassTables {
        &self.classes
    }

    /// Every weapon, sorted by `kind`.
    pub fn weapons(&self) -> &[WeaponDef] {
        &self.weapons
    }

    /// Every role, sorted by `kind` (not in file order: that does not matter).
    pub fn units(&self) -> &[UnitDef] {
        &self.units
    }

    /// The role with this id.
    pub fn unit(&self, id: &str) -> Option<&UnitDef> {
        self.units.iter().find(|u| u.id == id)
    }

    /// The role with this kind number.
    pub fn unit_by_kind(&self, kind: u16) -> Option<&UnitDef> {
        self.units
            .binary_search_by_key(&kind, |u| u.kind)
            .ok()
            .map(|i| &self.units[i])
    }

    /// The kind number of the role with this id.
    pub fn kind_of(&self, id: &str) -> Option<u16> {
        self.unit(id).map(|u| u.kind)
    }

    /// The weapon with this id.
    pub fn weapon(&self, id: &str) -> Option<&WeaponDef> {
        self.weapons.iter().find(|w| w.id == id)
    }

    /// The weapon with this kind number.
    pub fn weapon_by_kind(&self, kind: u16) -> Option<&WeaponDef> {
        self.weapons
            .binary_search_by_key(&kind, |w| w.kind)
            .ok()
            .map(|i| &self.weapons[i])
    }

    /// Tombstones of removed roles: their numbers stay taken.
    pub fn retired_units(&self) -> &[Retired] {
        &self.retired_units
    }

    /// Tombstones of removed weapons.
    pub fn retired_weapons(&self) -> &[Retired] {
        &self.retired_weapons
    }

    /// The roles converted for the simulation, sorted by kind. The numbers can
    /// have gaps: look a role up with [`UnitDefs::sim_unit`].
    pub fn sim_units(&self) -> &[SimUnitDef] {
        &self.sim_units
    }

    /// The converted role with this kind number.
    pub fn sim_unit(&self, kind: u16) -> Option<&SimUnitDef> {
        self.sim_units
            .binary_search_by_key(&kind, |u| u.kind)
            .ok()
            .map(|i| &self.sim_units[i])
    }

    /// The weapons converted for the simulation, sorted by kind.
    pub fn sim_weapons(&self) -> &[SimWeaponDef] {
        &self.sim_weapons
    }

    /// The converted weapon with this kind number (what
    /// [`SimUnitDef::weapons`] holds).
    pub fn sim_weapon(&self, kind: u16) -> Option<&SimWeaponDef> {
        self.sim_weapons
            .binary_search_by_key(&kind, |w| w.kind)
            .ok()
            .map(|i| &self.sim_weapons[i])
    }
}
