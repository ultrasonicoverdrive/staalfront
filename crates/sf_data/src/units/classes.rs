//! The three small tables that units and weapons refer to by name: movement
//! classes, armor types and damage types (`assets/config/unit_classes.ron`).
//!
//! They are tables, not Rust enums, so that a new class is a data change. Each
//! entry has a permanent `kind` number (the number the simulation uses, see
//! [`super::kinds`]) and a `name`; the work packages that give the tables
//! meaning add their fields next to them:
//!
//! * **Movement classes** (WP4.1): the terrain cost of each terrain type
//!   (`terrain_costs`), which replaced the three `passable` flags of
//!   `terrain_types.ron`. This module checks the list of one class on its own
//!   (names, duplicates, range); that **every class has a cost for every
//!   terrain type** needs the terrain table too and is checked by
//!   [`crate::movement::MovementCosts`].
//! * **Turning** (WP4.5): how the class turns its hull, a `turning` block per
//!   movement class ([`TurningDef`]; `docs/rules/turning.md`).
//! * **Armor types** and **damage types** (WP5.3): the damage-times-armor
//!   percentage table and the splash falloff.

use serde::Deserialize;

use super::decimal::Decimal;
use super::error::{DefError, Source};
use super::kinds::{self, Live, Retired};

/// Most entries one table may have.
const MAX_ENTRIES: usize = 64;

/// Largest `kind` of an entry of these tables (the simulation may store it in
/// a byte).
pub const MAX_CLASS_KIND: u16 = 255;

/// The slowest speed multiplier a terrain may give, in permille (one tenth).
pub const MIN_COST_PERMILLE: u16 = 100;

/// The fastest speed multiplier a terrain may give, in permille (double).
pub const MAX_COST_PERMILLE: u16 = 2000;

/// What one movement class does on one terrain type (WP4.1): it cannot enter
/// it, or it moves there at a fraction of its own speed.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum TerrainCost {
    /// The class cannot enter a cell of this type.
    Impassable,
    /// The class moves at this many thousandths of its speed: `1000` is its
    /// normal speed, `1200` a fifth faster, `500` half. Between
    /// [`MIN_COST_PERMILLE`] and [`MAX_COST_PERMILLE`].
    Speed(u16),
}

impl TerrainCost {
    /// The multiplier in permille; zero for [`TerrainCost::Impassable`].
    pub const fn permille(self) -> u16 {
        match self {
            TerrainCost::Impassable => 0,
            TerrainCost::Speed(p) => p,
        }
    }

    /// Whether a unit may enter.
    pub const fn is_passable(self) -> bool {
        !matches!(self, TerrainCost::Impassable)
    }
}

/// The longest drive tolerance a class may have, in facing steps: a quarter turn.
pub const MAX_DRIVE_TOLERANCE: u8 = 64;

/// The smallest turning radius of an arc class, in thousandths of a cell.
pub const MIN_ARC_RADIUS_MILLI: i64 = 250;

/// The largest turning radius of an arc class, in thousandths of a cell.
pub const MAX_ARC_RADIUS_MILLI: i64 = 32_000;

/// How a movement class turns the hull of its units (WP4.5).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum TurnMode {
    /// Turns in place at its turn rate, drives when it is within the tolerance.
    Pivot,
    /// Has to move to turn, in an arc of at least `min_radius`.
    Arc,
    /// The heading snaps onto the wanted one.
    Instant,
}

/// The `turning` block of a movement class (WP4.5, `docs/rules/turning.md`).
/// Every field has a default, and the defaults are the walk as it was before
/// WP4.5: pivot, drive within 16 steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TurningDef {
    /// How the hull turns.
    pub mode: TurnMode,
    /// The heading error, in facing steps (256 to a turn), up to which the unit
    /// drives; above it, it turns first (`Pivot`) or arcs (`Arc`). Not used by
    /// `Instant`. At most [`MAX_DRIVE_TOLERANCE`].
    pub drive_tolerance: u8,
    /// `Arc` only: the smallest radius of a turn, in cells.
    pub min_radius: Decimal,
    /// `Arc` only: the share of the unit's turn rate, in thousandths, that it
    /// has when it has to turn on the spot (a stand-in for backing and turning);
    /// 1 to 1000. Must be 1000 for the other modes.
    pub pivot_permille: u16,
}

impl Default for TurningDef {
    fn default() -> TurningDef {
        TurningDef {
            mode: TurnMode::Pivot,
            drive_tolerance: 16,
            min_radius: Decimal::ZERO,
            pivot_permille: 1000,
        }
    }
}

/// How a unit moves: which terrain it can use and how fast. Foot, wheeled,
/// tracked, hover, water or air in the shipped table.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MovementClass {
    /// Permanent number, unique among movement classes and tombstones.
    pub kind: u16,
    /// Unique snake_case name that units refer to.
    pub name: String,
    /// The cost of each terrain type, by the terrain type's name, in the order
    /// the file lists them (the order does not matter). A list and not a map so
    /// that a type written twice is an error and not silently the last one.
    pub terrain_costs: Vec<(String, TerrainCost)>,
    /// Whether this is a **ground class**: one of the classes whose ability to
    /// enter a terrain type makes it "open ground" for placement helpers and
    /// for the `ground` summary the terrain digest keeps (`foot`, `wheeled` and
    /// `tracked` in the shipped file). Defaults to false, so a file that does
    /// not say keeps loading.
    #[serde(default)]
    pub ground: bool,
    /// How the class turns its hull (WP4.5); left out it is the walk as it was
    /// before: pivot, drive within 16 steps.
    #[serde(default)]
    pub turning: TurningDef,
}

impl MovementClass {
    /// The cost on the terrain type called `terrain`.
    pub fn cost_on(&self, terrain: &str) -> Option<TerrainCost> {
        self.terrain_costs
            .iter()
            .find(|(name, _)| name == terrain)
            .map(|&(_, cost)| cost)
    }
}

/// What kind of protection a unit has against damage types.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmorType {
    /// Permanent number, unique among armor types and tombstones.
    pub kind: u16,
    /// Unique snake_case name that units refer to.
    pub name: String,
    // WP5.3: the percentage taken per damage type goes here or in a separate
    // damage table next to the lists in the file.
}

/// What kind of damage a weapon deals.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DamageType {
    /// Permanent number, unique among damage types and tombstones.
    pub kind: u16,
    /// Unique snake_case name that weapons refer to.
    pub name: String,
}

/// The RON file.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassesFile {
    movement_classes: Vec<MovementClass>,
    armor_types: Vec<ArmorType>,
    damage_types: Vec<DamageType>,
    #[serde(default)]
    retired_movement_classes: Vec<Retired>,
    #[serde(default)]
    retired_armor_types: Vec<Retired>,
    #[serde(default)]
    retired_damage_types: Vec<Retired>,
}

/// The validated tables, each sorted by `kind`. The order in the file does not
/// matter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassTables {
    /// Foot, wheeled, tracked, hover, water, air.
    pub movement_classes: Vec<MovementClass>,
    /// Armor types.
    pub armor_types: Vec<ArmorType>,
    /// Damage types.
    pub damage_types: Vec<DamageType>,
    /// Numbers of removed movement classes, never to be reused.
    pub retired_movement_classes: Vec<Retired>,
    /// Numbers of removed armor types.
    pub retired_armor_types: Vec<Retired>,
    /// Numbers of removed damage types.
    pub retired_damage_types: Vec<Retired>,
}

impl ClassTables {
    /// Parses and validates the text of a `unit_classes.ron`; `file` is the
    /// name that errors show.
    pub fn from_ron(file: &str, text: &str) -> Result<Self, DefError> {
        Self::parse(Source::new(file, text))
    }

    /// Parses and validates the tables.
    pub(crate) fn parse(src: Source<'_>) -> Result<Self, DefError> {
        src.check_numbers()?;
        let mut file: ClassesFile = ron::from_str(src.text).map_err(|e| src.ron_error(&e))?;
        check_list(
            src,
            "movement class",
            &names(file.movement_classes.iter().map(|c| (c.kind, &c.name))),
            &file.retired_movement_classes,
        )?;
        check_list(
            src,
            "armor type",
            &names(file.armor_types.iter().map(|c| (c.kind, &c.name))),
            &file.retired_armor_types,
        )?;
        check_list(
            src,
            "damage type",
            &names(file.damage_types.iter().map(|c| (c.kind, &c.name))),
            &file.retired_damage_types,
        )?;
        for class in &file.movement_classes {
            check_costs(src, class)?;
            check_turning(src, class)?;
        }
        file.movement_classes.sort_by_key(|c| c.kind);
        file.armor_types.sort_by_key(|c| c.kind);
        file.damage_types.sort_by_key(|c| c.kind);
        Ok(ClassTables {
            movement_classes: file.movement_classes,
            armor_types: file.armor_types,
            damage_types: file.damage_types,
            retired_movement_classes: file.retired_movement_classes,
            retired_armor_types: file.retired_armor_types,
            retired_damage_types: file.retired_damage_types,
        })
    }

    /// The movement class with this number.
    pub fn movement_class(&self, kind: u16) -> Option<&MovementClass> {
        self.movement_classes
            .binary_search_by_key(&kind, |c| c.kind)
            .ok()
            .map(|i| &self.movement_classes[i])
    }

    /// The number of the movement class with this name.
    pub fn movement_kind(&self, name: &str) -> Option<u16> {
        self.movement_classes
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.kind)
    }

    /// The number of the armor type with this name.
    pub fn armor_kind(&self, name: &str) -> Option<u16> {
        self.armor_types
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.kind)
    }

    /// The number of the damage type with this name.
    pub fn damage_kind(&self, name: &str) -> Option<u16> {
        self.damage_types
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.kind)
    }
}

/// A name that tools and scenario files can type: lowercase letters, digits and
/// underscores, starting with a letter, at most 48 characters.
pub(crate) fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && name.len() <= 48
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// The checks of one class's `terrain_costs` that need nothing but the class:
/// terrain names are snake_case and listed once, a speed is within range.
fn check_costs(src: Source<'_>, class: &MovementClass) -> Result<(), DefError> {
    let here = |message: String| src.error_at_record("name", &class.name, 0, message);
    for (i, (terrain, cost)) in class.terrain_costs.iter().enumerate() {
        if !is_valid_name(terrain) {
            return Err(here(format!(
                "movement class \"{}\": terrain name \"{terrain}\" is not snake_case (a-z, 0-9, _)",
                class.name
            )));
        }
        if class.terrain_costs[..i].iter().any(|(t, _)| t == terrain) {
            return Err(here(format!(
                "movement class \"{}\": terrain \"{terrain}\" is listed twice",
                class.name
            )));
        }
        if let TerrainCost::Speed(p) = *cost
            && !(MIN_COST_PERMILLE..=MAX_COST_PERMILLE).contains(&p)
        {
            return Err(here(format!(
                "movement class \"{}\": speed {p} on \"{terrain}\" must be between \
                 {MIN_COST_PERMILLE} and {MAX_COST_PERMILLE} thousandths (use Impassable to forbid it)",
                class.name
            )));
        }
    }
    Ok(())
}

/// The checks of one class's `turning` block (WP4.5).
fn check_turning(src: Source<'_>, class: &MovementClass) -> Result<(), DefError> {
    let t = &class.turning;
    let here = |message: String| {
        src.error_at_record(
            "name",
            &class.name,
            0,
            format!("movement class \"{}\": turning: {message}", class.name),
        )
    };
    if t.drive_tolerance > MAX_DRIVE_TOLERANCE {
        return Err(here(format!(
            "drive_tolerance {} must be at most {MAX_DRIVE_TOLERANCE} steps (a quarter turn)",
            t.drive_tolerance
        )));
    }
    if t.pivot_permille == 0 || t.pivot_permille > 1000 {
        return Err(here(format!(
            "pivot_permille {} must be between 1 and 1000",
            t.pivot_permille
        )));
    }
    match t.mode {
        TurnMode::Arc => {
            if !(MIN_ARC_RADIUS_MILLI..=MAX_ARC_RADIUS_MILLI).contains(&t.min_radius.milli()) {
                return Err(here(format!(
                    "an Arc needs a min_radius between 0.25 and 32 cells, got {}",
                    t.min_radius
                )));
            }
        }
        TurnMode::Pivot | TurnMode::Instant => {
            if !t.min_radius.is_zero() {
                return Err(here("min_radius is for mode Arc only".into()));
            }
            if t.pivot_permille != 1000 {
                return Err(here("pivot_permille is for mode Arc only".into()));
            }
        }
    }
    Ok(())
}

fn names<'a>(items: impl Iterator<Item = (u16, &'a String)>) -> Vec<(u16, &'a str)> {
    items.map(|(k, n)| (k, n.as_str())).collect()
}

fn check_list(
    src: Source<'_>,
    what: &str,
    entries: &[(u16, &str)],
    retired: &[Retired],
) -> Result<(), DefError> {
    if entries.is_empty() {
        return Err(src.error(format!("the list of {what}s is empty")));
    }
    if entries.len() > MAX_ENTRIES {
        return Err(src.error(format!("more than {MAX_ENTRIES} {what}s")));
    }
    for (i, (_, name)) in entries.iter().enumerate() {
        if !is_valid_name(name) {
            return Err(src.error_at_record(
                "name",
                name,
                0,
                format!("{what} name \"{name}\" is not snake_case (a-z, 0-9, _)"),
            ));
        }
        if entries[..i].iter().any(|(_, n)| n == name) {
            let nth = entries[..i].iter().filter(|(_, n)| n == name).count();
            return Err(src.error_at_record(
                "name",
                name,
                nth,
                format!("{what} \"{name}\" appears twice"),
            ));
        }
    }
    let live: Vec<Live<'_>> = entries
        .iter()
        .map(|&(kind, name)| Live { kind, name })
        .collect();
    kinds::check(src, what, "name", MAX_CLASS_KIND, &live, retired)
}
