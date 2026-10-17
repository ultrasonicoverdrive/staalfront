//! The three small tables that units and weapons refer to by name: movement
//! classes, armor types and damage types (`assets/config/unit_classes.ron`).
//!
//! They are tables, not Rust enums, so that a new class is a data change. Each
//! entry has a permanent `kind` number (the number the simulation uses, see
//! [`super::kinds`]) and a `name`; the work packages that give the tables
//! meaning add their fields next to them:
//!
//! * **Movement classes** (WP4.1): the terrain cost of each terrain type
//!   (`terrain_costs`), which replaces the three `passable` flags of
//!   `terrain_types.ron`. Nothing about terrain costs is decided here.
//! * **Armor types** and **damage types** (WP5.3): the damage-times-armor
//!   percentage table and the splash falloff.

use serde::Deserialize;

use super::error::{DefError, Source};
use super::kinds::{self, Live, Retired};

/// Most entries one table may have.
const MAX_ENTRIES: usize = 64;

/// Largest `kind` of an entry of these tables (the simulation may store it in
/// a byte).
pub const MAX_CLASS_KIND: u16 = 255;

/// How a unit moves: which terrain it can use. Foot, wheeled, tracked, hover,
/// water or air in the shipped table.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MovementClass {
    /// Permanent number, unique among movement classes and tombstones.
    pub kind: u16,
    /// Unique snake_case name that units refer to.
    pub name: String,
    // WP4.1: `terrain_costs` goes here (a cost per terrain type name, with
    // `#[serde(default)]` so older files keep loading).
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
