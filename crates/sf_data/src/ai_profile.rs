//! The computer opponent's profile (WP9.1a, `assets/ai/<profile>.ron`, `docs/rules/ai.md`): the build order
//! as a goal list, the unit mix, and the numbers of the economy, the army and the think schedule.
//!
//! The file is data and not code (PLAN section 5 rule 6). It is checked when it is read
//! ([`AiProfile::from_ron`]: the shape and every number) and against the unit definitions
//! ([`AiProfile::check`]: every role exists, is open to the faction, is the right sort of thing, and every
//! prerequisite of a build entry is earlier in the list). `sf_ai::Profile` turns the roles into kinds.
//!
//! One file holds a section per faction, because what a faction may build differs. The order is a **goal
//! list**: `(role, count)` means "have at least `count` of `role`"; the AI walks it from the top and builds
//! the first entry it does not satisfy. One rule gives the opening, the tech choice and the rebuilding.
//!
//! ```
//! use sf_data::ai_profile::AiProfile;
//!
//! let normal = AiProfile::embedded();
//! assert_eq!(normal.think_interval, 10);
//! ```

use std::collections::BTreeSet;
use std::fmt;

use serde::Deserialize;

use crate::units::{BuildingFunction, Category, Faction, UnitDefs, produced_by};

/// The shipped profile "normal", compiled in.
pub const EMBEDDED_NORMAL: &str = include_str!("../../../assets/ai/normal.ron");

/// The most players that must be able to think on different ticks (the simulation's match limit).
pub const MAX_PLAYERS: u32 = 8;
/// The longest think interval, in ticks (30 seconds).
pub const MAX_THINK_INTERVAL: u32 = 600;
/// The largest count of one build entry.
pub const MAX_COUNT: u16 = 32;
/// The largest weight of one unit mix entry.
pub const MAX_WEIGHT: u16 = 1000;
/// The longest list of build entries or unit mix entries of a faction.
pub const MAX_ENTRIES: usize = 64;
/// The largest value of a tick count in the rules (one hour at 20 ticks per second).
pub const MAX_TICKS: u32 = 72_000;

/// Why a profile was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiProfileError(pub String);

impl fmt::Display for AiProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ai profile: {}", self.0)
    }
}

impl std::error::Error for AiProfileError {}

fn err<T>(message: impl Into<String>) -> Result<T, AiProfileError> {
    Err(AiProfileError(message.into()))
}

/// The numbers of the economy rules (WP9.1b).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EconomyRules {
    /// Spare power kept above what the structures use, 0 to 1000.
    pub power_margin: u32,
    /// Harvesters wanted per refinery, 1 to 8.
    pub harvesters_per_refinery: u8,
    /// Credits kept back before units are queued, at most 1,000,000.
    pub reserve: u32,
    /// Structures built at once, 1 to 4.
    pub max_parallel_builds: u8,
    /// A silo is built when the ore store is this many percent full, 1 to 100.
    pub silo_percent: u8,
    /// A structure below this many percent of its hit points is repaired, 1 to 100.
    pub repair_below_percent: u8,
    /// A role with no place found is skipped for this many ticks, 1 to [`MAX_TICKS`].
    pub blocked_ticks: u32,
    /// The most placement previews one think asks for, 1 to 65,536.
    pub placement_cap: u32,
}

/// The numbers of the army rules (WP9.1c).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmyRules {
    /// The army value (sum of unit costs) at which it attacks, 1 to 1,000,000.
    pub attack_value: u32,
    /// Ticks since the last attack after which it attacks with at least `min_units`, 1 to [`MAX_TICKS`].
    pub max_wait: u32,
    /// The fewest units of an attack, 1 to 1000.
    pub min_units: u16,
    /// No attack before this tick, at most [`MAX_TICKS`].
    pub first_attack_tick: u32,
    /// Ticks the army defends after a structure was hit, 1 to [`MAX_TICKS`].
    pub defend_ticks: u32,
    /// Cells from the base towards the map centre where the army gathers, 1 to 64.
    pub gather_distance: u8,
}

/// What one faction builds and makes.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactionPlan {
    /// The faction this section is for (not `Neutral`).
    pub faction: Faction,
    /// The goal list: `(role, count)`, structures only.
    pub build_order: Vec<(String, u16)>,
    /// `(role, weight)`: the share of the army each role aims at.
    pub unit_mix: Vec<(String, u16)>,
}

/// The file as written.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiProfile {
    /// A short name for logs.
    pub name: String,
    /// A think runs when `(tick + stagger * player) % think_interval == 0`, 1 to [`MAX_THINK_INTERVAL`].
    pub think_interval: u32,
    /// The offset per player. `think_interval / gcd(think_interval, stagger)` must be at least
    /// [`MAX_PLAYERS`], so that no two players of a match think on the same tick.
    pub stagger: u32,
    /// The economy numbers.
    pub economy: EconomyRules,
    /// The army numbers.
    pub army: ArmyRules,
    /// One section per faction that plays, ascending by faction, none twice.
    pub factions: Vec<FactionPlan>,
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn range<T: PartialOrd + fmt::Display>(
    what: &str,
    v: T,
    lo: T,
    hi: T,
) -> Result<(), AiProfileError> {
    if v < lo || v > hi {
        return err(format!("{what} is {v}, it must be between {lo} and {hi}"));
    }
    Ok(())
}

impl AiProfile {
    /// Reads and checks the shape and the numbers of a profile. The error says what is wrong.
    pub fn from_ron(text: &str) -> Result<AiProfile, AiProfileError> {
        let p: AiProfile = ron::from_str(text).map_err(|e| AiProfileError(e.to_string()))?;
        if p.name.trim().is_empty() {
            return err("name must not be empty");
        }
        range("think_interval", p.think_interval, 1, MAX_THINK_INTERVAL)?;
        if p.stagger == 0 || p.think_interval / gcd(p.think_interval, p.stagger) < MAX_PLAYERS {
            return err(format!(
                "stagger {} with think_interval {} lets fewer than {MAX_PLAYERS} players think on different ticks (interval / gcd(interval, stagger) must be at least {MAX_PLAYERS})",
                p.stagger, p.think_interval
            ));
        }
        let e = &p.economy;
        range("economy.power_margin", e.power_margin, 0, 1000)?;
        range(
            "economy.harvesters_per_refinery",
            e.harvesters_per_refinery,
            1,
            8,
        )?;
        range("economy.reserve", e.reserve, 0, 1_000_000)?;
        range("economy.max_parallel_builds", e.max_parallel_builds, 1, 4)?;
        range("economy.silo_percent", e.silo_percent, 1, 100)?;
        range(
            "economy.repair_below_percent",
            e.repair_below_percent,
            1,
            100,
        )?;
        range("economy.blocked_ticks", e.blocked_ticks, 1, MAX_TICKS)?;
        range("economy.placement_cap", e.placement_cap, 1, 65_536)?;
        let a = &p.army;
        range("army.attack_value", a.attack_value, 1, 1_000_000)?;
        range("army.max_wait", a.max_wait, 1, MAX_TICKS)?;
        range("army.min_units", a.min_units, 1, 1000)?;
        range("army.first_attack_tick", a.first_attack_tick, 0, MAX_TICKS)?;
        range("army.defend_ticks", a.defend_ticks, 1, MAX_TICKS)?;
        range("army.gather_distance", a.gather_distance, 1, 64)?;
        if p.factions.is_empty() {
            return err("no faction section");
        }
        let mut last: Option<Faction> = None;
        for f in &p.factions {
            if f.faction == Faction::Neutral {
                return err("a section for Neutral: nobody plays it");
            }
            if let Some(prev) = last {
                if prev == f.faction {
                    return err(format!("faction {:?} has two sections", f.faction));
                }
                if prev > f.faction {
                    return err("the faction sections must be in ascending faction order");
                }
            }
            last = Some(f.faction);
            let at = format!("{:?}", f.faction);
            if f.build_order.is_empty() || f.build_order.len() > MAX_ENTRIES {
                return err(format!(
                    "{at}: build_order has {} entries, it needs 1 to {MAX_ENTRIES}",
                    f.build_order.len()
                ));
            }
            if f.unit_mix.is_empty() || f.unit_mix.len() > MAX_ENTRIES {
                return err(format!(
                    "{at}: unit_mix has {} entries, it needs 1 to {MAX_ENTRIES}",
                    f.unit_mix.len()
                ));
            }
            for (role, count) in &f.build_order {
                range(
                    &format!("{at}: build_order {role} count"),
                    *count,
                    1,
                    MAX_COUNT,
                )?;
            }
            for (role, weight) in &f.unit_mix {
                range(
                    &format!("{at}: unit_mix {role} weight"),
                    *weight,
                    1,
                    MAX_WEIGHT,
                )?;
            }
        }
        Ok(p)
    }

    /// The shipped profile "normal". Panics if the shipped file is wrong (a test reads it).
    pub fn embedded() -> AiProfile {
        AiProfile::from_ron(EMBEDDED_NORMAL).unwrap_or_else(|e| panic!("assets/ai/normal.ron: {e}"))
    }

    /// The section of `faction`, if the profile has one.
    pub fn plan(&self, faction: Faction) -> Option<&FactionPlan> {
        self.factions.iter().find(|f| f.faction == faction)
    }

    /// Checks the lists against the unit definitions: every role exists, is open to the faction of its
    /// section, a build entry is a buildable structure whose prerequisites are earlier in the list (the
    /// construction centre, the anchor, is always there), a role repeated in the build list asks for more
    /// each time, a unit is a mobile infantry or vehicle role that is not a harvester (the economy makes
    /// those) and has a cost, and the factory that makes it and every structure it requires is in the
    /// build list.
    pub fn check(&self, defs: &UnitDefs) -> Result<(), AiProfileError> {
        for f in &self.factions {
            let at = format!("{:?}", f.faction);
            let mut earlier: BTreeSet<&str> = BTreeSet::new();
            let mut functions: BTreeSet<BuildingFunction> = BTreeSet::new();
            let mut counts: Vec<(&str, u16)> = Vec::new();
            for (i, (role, count)) in f.build_order.iter().enumerate() {
                let here = format!("{at}: build_order entry {i} ({role})");
                let Some(def) = defs.unit(role) else {
                    return err(format!("{here}: no such role in unit_defs.ron"));
                };
                if !def.factions.contains(&f.faction) {
                    return err(format!("{here}: the role is not open to {:?}", f.faction));
                }
                if def.category != Category::Structure {
                    return err(format!("{here}: not a structure"));
                }
                let Some(building) = &def.building else {
                    return err(format!("{here}: a structure without a building block"));
                };
                if def.cost.is_none() || building.function == BuildingFunction::Anchor {
                    return err(format!("{here}: it cannot be built"));
                }
                for need in &building.requires {
                    let anchor = defs.unit(need).is_some_and(|d| {
                        d.building
                            .as_ref()
                            .is_some_and(|b| b.function == BuildingFunction::Anchor)
                    });
                    if !anchor && !earlier.contains(need.as_str()) {
                        return err(format!(
                            "{here}: its prerequisite \"{need}\" is not earlier in the list"
                        ));
                    }
                }
                if let Some((_, last)) = counts.iter().rev().find(|(r, _)| r == role)
                    && count <= last
                {
                    return err(format!(
                        "{here}: the role comes again with count {count}, which must be above the {last} before"
                    ));
                }
                counts.push((role.as_str(), *count));
                earlier.insert(role.as_str());
                functions.insert(building.function);
            }
            let mut seen: BTreeSet<&str> = BTreeSet::new();
            for (i, (role, _)) in f.unit_mix.iter().enumerate() {
                let here = format!("{at}: unit_mix entry {i} ({role})");
                let Some(def) = defs.unit(role) else {
                    return err(format!("{here}: no such role in unit_defs.ron"));
                };
                if !seen.insert(role.as_str()) {
                    return err(format!("{here}: listed twice"));
                }
                if !def.factions.contains(&f.faction) {
                    return err(format!("{here}: the role is not open to {:?}", f.faction));
                }
                if !matches!(def.category, Category::Infantry | Category::Vehicle) {
                    return err(format!("{here}: only infantry and vehicles are in the mix"));
                }
                let Some(factory) = produced_by(def.category) else {
                    return err(format!("{here}: no factory makes this sort of role"));
                };
                if def.flags.harvests {
                    return err(format!(
                        "{here}: harvesters are the economy's, not part of the mix"
                    ));
                }
                if def.cost.is_none() {
                    return err(format!("{here}: it cannot be built"));
                }
                if !functions.contains(&factory) {
                    return err(format!(
                        "{here}: no structure of the build list can make it ({factory:?} missing)"
                    ));
                }
                for need in &def.requires {
                    if !earlier.contains(need.as_str()) {
                        return err(format!(
                            "{here}: it requires \"{need}\", which is not in the build list"
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
