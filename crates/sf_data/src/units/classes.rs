//! The three small tables that units and weapons refer to by name: movement
//! classes, armor types and damage types (`assets/config/unit_classes.ron`).
//!
//! They are tables, not Rust enums, so that a new class is a data change. Each
//! entry is a struct with just a `name` today; the work packages that give the
//! tables meaning add their fields next to it:
//!
//! * **Movement classes** (WP4.1): the terrain cost of each terrain type
//!   (`terrain_costs`), which replaces the three `passable` flags of
//!   `terrain_types.ron`. Nothing about terrain costs is decided here.
//! * **Armor types** and **damage types** (WP5.3): the damage-times-armor
//!   percentage table and the splash falloff.

use serde::Deserialize;

use super::error::{DefError, Source};

/// Most entries one table may have (an index is stored in a `u8`).
const MAX_ENTRIES: usize = 64;

/// How a unit moves: which terrain it can use. Foot, wheeled, tracked, hover,
/// water or air in the shipped table.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MovementClass {
    /// Unique snake_case name that units refer to.
    pub name: String,
    // WP4.1: `terrain_costs` goes here (a cost per terrain type name, with
    // `#[serde(default)]` so older files keep loading).
}

/// What kind of protection a unit has against damage types.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmorType {
    /// Unique snake_case name that units refer to.
    pub name: String,
    // WP5.3: the percentage taken per damage type goes here or in a separate
    // damage table next to the lists in the file.
}

/// What kind of damage a weapon deals.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DamageType {
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
}

/// The validated tables. A name's position in its list is its index (what the
/// simulation gets as a `u8`), so entries are only ever appended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassTables {
    /// Foot, wheeled, tracked, hover, water, air.
    pub movement_classes: Vec<MovementClass>,
    /// Armor types.
    pub armor_types: Vec<ArmorType>,
    /// Damage types.
    pub damage_types: Vec<DamageType>,
}

impl ClassTables {
    /// Parses and validates the tables.
    pub(crate) fn parse(src: Source<'_>) -> Result<Self, DefError> {
        let file: ClassesFile = ron::from_str(src.text).map_err(|e| src.ron_error(&e))?;
        check_names(
            src,
            "movement class",
            file.movement_classes.iter().map(|c| &c.name),
        )?;
        check_names(src, "armor type", file.armor_types.iter().map(|c| &c.name))?;
        check_names(
            src,
            "damage type",
            file.damage_types.iter().map(|c| &c.name),
        )?;
        Ok(ClassTables {
            movement_classes: file.movement_classes,
            armor_types: file.armor_types,
            damage_types: file.damage_types,
        })
    }

    /// Index of the movement class with this name.
    pub fn movement_index(&self, name: &str) -> Option<u8> {
        index_of(self.movement_classes.iter().map(|c| c.name.as_str()), name)
    }

    /// Index of the armor type with this name.
    pub fn armor_index(&self, name: &str) -> Option<u8> {
        index_of(self.armor_types.iter().map(|c| c.name.as_str()), name)
    }

    /// Index of the damage type with this name.
    pub fn damage_index(&self, name: &str) -> Option<u8> {
        index_of(self.damage_types.iter().map(|c| c.name.as_str()), name)
    }
}

fn index_of<'a>(mut names: impl Iterator<Item = &'a str>, name: &str) -> Option<u8> {
    names
        .position(|n| n == name)
        .and_then(|i| u8::try_from(i).ok())
}

/// A name that tools and scenario files can type: lowercase letters, digits and
/// underscores, starting with a letter, at most 48 characters.
pub(crate) fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && name.len() <= 48
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn check_names<'a>(
    src: Source<'_>,
    what: &str,
    names: impl Iterator<Item = &'a String>,
) -> Result<(), DefError> {
    let names: Vec<&String> = names.collect();
    if names.is_empty() {
        return Err(src.error(format!("the list of {what}s is empty")));
    }
    if names.len() > MAX_ENTRIES {
        return Err(src.error(format!("more than {MAX_ENTRIES} {what}s")));
    }
    for (i, name) in names.iter().enumerate() {
        if !is_valid_name(name) {
            return Err(src.error_at_record(
                "name",
                name,
                0,
                format!("{what} name \"{name}\" is not snake_case (a-z, 0-9, _)"),
            ));
        }
        if names[..i].contains(name) {
            let nth = names[..i].iter().filter(|n| **n == *name).count();
            return Err(src.error_at_record(
                "name",
                name,
                nth,
                format!("{what} \"{name}\" appears twice"),
            ));
        }
    }
    Ok(())
}
