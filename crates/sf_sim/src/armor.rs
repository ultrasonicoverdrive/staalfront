//! The armor table (WP5.3): how much of a damage type an armor type takes.
//!
//! `sf_data` reads and checks the table in `assets/config/unit_classes.ron` and the armor type of
//! every unit; this crate may not depend on it, so the caller (`sf_bridge::weapon_table`) copies the
//! numbers into an [`ArmorTable`] and hands it over inside the [`WeaponTable`](crate::WeaponTable)
//! ([`WeaponTable::with_armor`](crate::WeaponTable::with_armor)). A world whose weapon table has no
//! armor table takes every hit at one hundred percent, which is what a hit did before WP5.3.
//! `docs/rules/armour.md` has the rules in full.
//!
//! # The table
//!
//! One whole number of percent for every pair of a damage type and an armor type, both given by their
//! permanent kind numbers: `100` takes the weapon's damage as it is, `0` takes none (the armor is
//! immune to that damage type), and up to [`MAX_PERCENT`] takes more. The table has to be **complete**:
//! a missing pair, a pair twice, a number above the bound or a name that is not in the lists is
//! refused when the table is built ([`ArmorError`]), so a hit never meets a hole. Besides the
//! percentages the table holds the armor type of every unit kind, because the unit table
//! ([`UnitStats`](crate::UnitStats)) is pinned by earlier scenarios and does not carry it.
//!
//! # The arithmetic
//!
//! [`scaled_damage`] is the one place where a damage is multiplied: the damage, the percentage of the
//! armor, and (for splash) the falloff as a fraction `num / den` of two integers, all in **one**
//! rational step with **one** rounding to the nearest whole hit point, a tie going up. A positive
//! damage that meets a positive percentage and a positive falloff always does at least one hit point
//! (rounding never turns a hit into nothing); only a percentage of zero, or a falloff of zero, does
//! nothing.

use core::fmt;

use sf_math::hash::Fnv1a64;

use crate::ids::Kind;

/// Written first into the digest; bump when its layout changes.
const DIGEST_TAG: &[u8] = b"sf_sim/armor/v1";

/// The largest percentage a table may hold: four times the weapon's damage.
pub const MAX_PERCENT: u16 = 400;

/// What a hit does after the armor and the falloff: `damage * percent / 100 * num / den`, rounded
/// **once**, to the nearest hit point with a tie going up, and never to nothing when every factor
/// is positive (a minimum of one hit point).
///
/// `percent` is the table's number for the damage type and the victim's armor, `num / den` the
/// falloff of a splash (`1 / 1` for a direct hit). The product is worked out in integers, so it is
/// the same on every machine. `den` must not be zero (it is a positive distance in the one caller).
///
/// ```
/// use sf_sim::armor::scaled_damage;
/// assert_eq!(scaled_damage(6, 100, 1, 1), 6); // as it is
/// assert_eq!(scaled_damage(6, 35, 1, 1), 2); // 21/10, to nearest
/// assert_eq!(scaled_damage(4, 10, 1, 1), 1); // 4/10 would round to 0, the minimum makes it 1
/// assert_eq!(scaled_damage(6, 0, 1, 1), 0); // immune: no minimum
/// assert_eq!(scaled_damage(90, 100, 1, 2), 45); // half way out
/// ```
pub fn scaled_damage(damage: u32, percent: u16, num: u64, den: u64) -> u32 {
    debug_assert!(den > 0, "a falloff needs a denominator");
    let product = u128::from(damage) * u128::from(percent) * u128::from(num);
    if product == 0 {
        return 0;
    }
    let scale = u128::from(den) * 100;
    // floor(product / scale + 1/2): to nearest, a tie up.
    let rounded = (product * 2 + scale) / (scale * 2);
    u32::try_from(rounded.max(1)).unwrap_or(u32::MAX)
}

/// Why an armor table was refused.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ArmorError {
    /// There are no damage types or no armor types.
    Empty,
    /// A damage type is listed twice.
    DuplicateDamageType(u16),
    /// An armor type is listed twice.
    DuplicateArmorType(u16),
    /// A percentage names a damage type that is not in the list.
    UnknownDamageType(u16),
    /// A percentage (or a unit) names an armor type that is not in the list.
    UnknownArmorType(u16),
    /// The pair has two percentages.
    DuplicatePercent {
        /// The damage type.
        damage_type: u16,
        /// The armor type.
        armor_type: u16,
    },
    /// The pair has none: the table is not complete.
    MissingPercent {
        /// The damage type.
        damage_type: u16,
        /// The armor type.
        armor_type: u16,
    },
    /// A percentage is above [`MAX_PERCENT`].
    PercentTooHigh {
        /// The damage type.
        damage_type: u16,
        /// The armor type.
        armor_type: u16,
        /// The number given.
        percent: u16,
    },
    /// A unit kind has two armor types.
    DuplicateUnit(Kind),
}

impl fmt::Display for ArmorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArmorError::Empty => write!(f, "the armor table has no damage types or no armor types"),
            ArmorError::DuplicateDamageType(k) => write!(f, "damage type {k} is listed twice"),
            ArmorError::DuplicateArmorType(k) => write!(f, "armor type {k} is listed twice"),
            ArmorError::UnknownDamageType(k) => {
                write!(f, "a percentage names damage type {k}, which is not listed")
            }
            ArmorError::UnknownArmorType(k) => {
                write!(f, "armor type {k} is named but not listed")
            }
            ArmorError::DuplicatePercent {
                damage_type,
                armor_type,
            } => write!(
                f,
                "damage type {damage_type} against armor type {armor_type} has two percentages"
            ),
            ArmorError::MissingPercent {
                damage_type,
                armor_type,
            } => write!(
                f,
                "damage type {damage_type} against armor type {armor_type} has no percentage"
            ),
            ArmorError::PercentTooHigh {
                damage_type,
                armor_type,
                percent,
            } => write!(
                f,
                "damage type {damage_type} against armor type {armor_type} is {percent} percent, \
                 above {MAX_PERCENT}"
            ),
            ArmorError::DuplicateUnit(k) => write!(f, "unit kind {} has two armor types", k.0),
        }
    }
}

impl std::error::Error for ArmorError {}

/// The percentages and the armor type of every unit kind. Immutable once built; part of the weapon
/// table and of its digest.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ArmorTable {
    /// Sorted ascending.
    damage_types: Vec<u16>,
    /// Sorted ascending.
    armor_types: Vec<u16>,
    /// Damage type major: the cell of damage type `d` and armor type `a` is
    /// `percent[d_index * armor_types.len() + a_index]`.
    percent: Vec<u16>,
    /// Sorted by unit kind: the armor type of each.
    units: Vec<(Kind, u16)>,
    digest: u64,
}

impl ArmorTable {
    /// Checks and sorts the lists. `cells` are `(damage type, armor type, percent)`, in any order;
    /// `units` are `(unit kind, armor type)`. Any order of the input gives an equal table.
    pub fn new(
        mut damage_types: Vec<u16>,
        mut armor_types: Vec<u16>,
        cells: Vec<(u16, u16, u16)>,
        mut units: Vec<(Kind, u16)>,
    ) -> Result<ArmorTable, ArmorError> {
        if damage_types.is_empty() || armor_types.is_empty() {
            return Err(ArmorError::Empty);
        }
        damage_types.sort_unstable();
        armor_types.sort_unstable();
        if let Some(pair) = damage_types.windows(2).find(|p| p[0] == p[1]) {
            return Err(ArmorError::DuplicateDamageType(pair[0]));
        }
        if let Some(pair) = armor_types.windows(2).find(|p| p[0] == p[1]) {
            return Err(ArmorError::DuplicateArmorType(pair[0]));
        }
        let width = armor_types.len();
        let mut percent: Vec<Option<u16>> = vec![None; damage_types.len() * width];
        for (damage_type, armor_type, p) in cells {
            let Ok(row) = damage_types.binary_search(&damage_type) else {
                return Err(ArmorError::UnknownDamageType(damage_type));
            };
            let Ok(column) = armor_types.binary_search(&armor_type) else {
                return Err(ArmorError::UnknownArmorType(armor_type));
            };
            if p > MAX_PERCENT {
                return Err(ArmorError::PercentTooHigh {
                    damage_type,
                    armor_type,
                    percent: p,
                });
            }
            let cell = &mut percent[row * width + column];
            if cell.is_some() {
                return Err(ArmorError::DuplicatePercent {
                    damage_type,
                    armor_type,
                });
            }
            *cell = Some(p);
        }
        let mut complete = Vec::with_capacity(percent.len());
        for (at, cell) in percent.iter().enumerate() {
            match cell {
                Some(p) => complete.push(*p),
                None => {
                    return Err(ArmorError::MissingPercent {
                        damage_type: damage_types[at / width],
                        armor_type: armor_types[at % width],
                    });
                }
            }
        }
        units.sort_unstable();
        if let Some(pair) = units.windows(2).find(|p| p[0].0 == p[1].0) {
            return Err(ArmorError::DuplicateUnit(pair[0].0));
        }
        for &(_, armor_type) in &units {
            if armor_types.binary_search(&armor_type).is_err() {
                return Err(ArmorError::UnknownArmorType(armor_type));
            }
        }
        let digest = digest_of(&damage_types, &armor_types, &complete, &units);
        Ok(ArmorTable {
            damage_types,
            armor_types,
            percent: complete,
            units,
            digest,
        })
    }

    /// The percentage of damage type `damage_type` that armor type `armor_type` takes; `None` if
    /// either is not in the table.
    pub fn percent(&self, damage_type: u16, armor_type: u16) -> Option<u16> {
        let row = self.damage_types.binary_search(&damage_type).ok()?;
        let column = self.armor_types.binary_search(&armor_type).ok()?;
        Some(self.percent[row * self.armor_types.len() + column])
    }

    /// The armor type of a unit kind; `None` if the table does not list the kind.
    pub fn armor_of(&self, unit: Kind) -> Option<u16> {
        self.units
            .binary_search_by_key(&unit, |&(k, _)| k)
            .ok()
            .map(|i| self.units[i].1)
    }

    /// The damage types of the table, ascending.
    pub fn damage_types(&self) -> &[u16] {
        &self.damage_types
    }

    /// The armor types of the table, ascending.
    pub fn armor_types(&self) -> &[u16] {
        &self.armor_types
    }

    /// The unit kinds the table gives an armor type, ascending.
    pub fn unit_kinds(&self) -> impl Iterator<Item = Kind> + '_ {
        self.units.iter().map(|&(k, _)| k)
    }

    /// A hash of the whole table. Part of the digest of the weapon table.
    pub fn digest(&self) -> u64 {
        self.digest
    }
}

fn digest_of(
    damage_types: &[u16],
    armor_types: &[u16],
    percent: &[u16],
    units: &[(Kind, u16)],
) -> u64 {
    let mut h = Fnv1a64::new();
    h.write_bytes(DIGEST_TAG);
    h.write_u64(damage_types.len() as u64);
    for d in damage_types {
        h.write_bytes(&d.to_le_bytes());
    }
    h.write_u64(armor_types.len() as u64);
    for a in armor_types {
        h.write_bytes(&a.to_le_bytes());
    }
    for p in percent {
        h.write_bytes(&p.to_le_bytes());
    }
    h.write_u64(units.len() as u64);
    for (kind, armor) in units {
        h.write_bytes(&kind.0.to_le_bytes());
        h.write_bytes(&armor.to_le_bytes());
    }
    h.finish()
}

#[cfg(test)]
mod tests;
