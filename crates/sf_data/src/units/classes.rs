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
//!   percentage table (`armor_table`, [`ClassTables::armor_percent`]). Every
//!   unit names one armor type (`armor` in `unit_defs.ron`) and every weapon one
//!   damage type; the table says how much of the weapon's damage the armor
//!   takes. It is optional in the file, so the frozen copies of the definitions
//!   that predate it still load (without a table every hit does the weapon's
//!   damage); when it is there it has to be complete. The rules are in
//!   `docs/rules/armour.md`.

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

/// The most one armor type may take of a damage type, in percent (four times the damage). The
/// simulation's own bound (`sf_sim::MAX_ARMOR_PERCENT`) is the same number.
pub const MAX_ARMOR_PERCENT: u16 = 400;

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
    // The percentage it takes of each damage type is in the file's `armor_table`
    // (WP5.3), not on the type: one table to read for the balance.
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

/// One row of the `armor_table` as written in the file: the percentage that each armor type takes of
/// one damage type, in a list of pairs so that a name written twice is an error and not the last one.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArmorRowDef {
    /// Name of a damage type.
    damage: String,
    /// `(armor type name, percent)` for every armor type. A signed number, so that a negative one is
    /// refused with a message and not as a syntax error.
    percent: Vec<(String, i32)>,
}

/// The RON file.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassesFile {
    movement_classes: Vec<MovementClass>,
    armor_types: Vec<ArmorType>,
    damage_types: Vec<DamageType>,
    /// Damage times armor (WP5.3). Left out: no table, every hit does the weapon's damage. Written
    /// but empty (`armor_table: []`): refused, as it says neither.
    #[serde(default, deserialize_with = "present")]
    armor_table: Option<Vec<ArmorRowDef>>,
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
    /// The armor table as `(damage kind, armor kind, percent)`, one entry for every pair of a damage
    /// type and an armor type, sorted; empty when the file has no `armor_table` (WP5.3).
    pub armor_percents: Vec<(u16, u16, u16)>,
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
        let armor_percents = check_armor_table(src, &file)?;
        file.movement_classes.sort_by_key(|c| c.kind);
        file.armor_types.sort_by_key(|c| c.kind);
        file.damage_types.sort_by_key(|c| c.kind);
        Ok(ClassTables {
            movement_classes: file.movement_classes,
            armor_types: file.armor_types,
            damage_types: file.damage_types,
            armor_percents,
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

    /// Whether the file has an `armor_table`.
    pub fn has_armor_table(&self) -> bool {
        !self.armor_percents.is_empty()
    }

    /// The percentage of damage type `damage` (by kind number) that armor type `armor` takes; `None`
    /// when the file has no `armor_table` or either number is not in it.
    pub fn armor_percent(&self, damage: u16, armor: u16) -> Option<u16> {
        self.armor_percents
            .binary_search_by_key(&(damage, armor), |&(d, a, _)| (d, a))
            .ok()
            .map(|i| self.armor_percents[i].2)
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

/// A field that is written is `Some`, even when it is an empty list; one that is left out is `None`
/// (through `#[serde(default)]`). The file writes the list itself, not `Some([...])`.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// The checks of the `armor_table` (WP5.3): every row names a damage type that exists, once; every
/// row has every armor type that exists, once, with a percentage from 0 to [`MAX_ARMOR_PERCENT`];
/// every damage type has a row. A file without a table passes (and gives an empty list); an empty
/// table (`armor_table: []`) is refused. Returns the cells as `(damage kind, armor kind, percent)`,
/// sorted.
fn check_armor_table(
    src: Source<'_>,
    file: &ClassesFile,
) -> Result<Vec<(u16, u16, u16)>, DefError> {
    let Some(table) = &file.armor_table else {
        return Ok(Vec::new());
    };
    if table.is_empty() {
        return Err(src.error(
            "armor_table is empty: leave the field out for no table, or give every damage type a row",
        ));
    }
    let damage_kind = |name: &str| {
        file.damage_types
            .iter()
            .find(|d| d.name == name)
            .map(|d| d.kind)
    };
    let armor_kind = |name: &str| {
        file.armor_types
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.kind)
    };
    let mut cells = Vec::new();
    for (i, row) in table.iter().enumerate() {
        let name = row.damage.as_str();
        let nth = table[..i].iter().filter(|r| r.damage == row.damage).count();
        let here = |message: String| {
            src.error_at_record(
                "damage",
                name,
                nth,
                format!("armor_table, row \"{name}\": {message}"),
            )
        };
        let Some(damage) = damage_kind(name) else {
            return Err(src.error_at_record(
                "damage",
                name,
                nth,
                format!("armor_table: unknown damage type \"{name}\" (not in damage_types)"),
            ));
        };
        if nth > 0 {
            return Err(src.error_at_record(
                "damage",
                name,
                nth,
                format!("armor_table: damage type \"{name}\" has two rows"),
            ));
        }
        for (j, (armor_name, percent)) in row.percent.iter().enumerate() {
            let in_row = |message: String| {
                src.error_in_record("damage", name, armor_name, message_in(name, message))
            };
            let Some(armor) = armor_kind(armor_name) else {
                return Err(in_row(format!(
                    "unknown armor type \"{armor_name}\" (not in armor_types)"
                )));
            };
            if row.percent[..j].iter().any(|(n, _)| n == armor_name) {
                return Err(in_row(format!(
                    "armor type \"{armor_name}\" is listed twice"
                )));
            }
            let Ok(percent) = u16::try_from(*percent) else {
                return Err(in_row(format!(
                    "\"{armor_name}\" is {percent} percent, which must be 0 to {MAX_ARMOR_PERCENT}"
                )));
            };
            if percent > MAX_ARMOR_PERCENT {
                return Err(in_row(format!(
                    "\"{armor_name}\" is {percent} percent, which must be 0 to {MAX_ARMOR_PERCENT}"
                )));
            }
            cells.push((damage, armor, percent));
        }
        for armor in &file.armor_types {
            if !row.percent.iter().any(|(n, _)| *n == armor.name) {
                return Err(here(format!(
                    "armor type \"{}\" has no percentage (the table has to be complete)",
                    armor.name
                )));
            }
        }
    }
    for damage in &file.damage_types {
        if !table.iter().any(|r| r.damage == damage.name) {
            return Err(src.error(format!(
                "armor_table: damage type \"{}\" has no row (the table has to be complete)",
                damage.name
            )));
        }
    }
    cells.sort_unstable();
    Ok(cells)
}

/// The start of every message about one cell of a row.
fn message_in(damage: &str, message: String) -> String {
    format!("armor_table, row \"{damage}\": {message}")
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
