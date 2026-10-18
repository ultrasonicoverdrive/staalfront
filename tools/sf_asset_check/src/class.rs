//! File names, factions and budget classes.
//!
//! A model file is `<faction>_<role>.glb`. The faction decides whether the
//! `team` material is required (factions) or forbidden (neutral objects); the
//! role decides the budget class. The role is looked up in `unit_defs.ron`
//! (its category: infantry, vehicle, vessel, structure, object); roles that
//! are not units are decor (trees, rocks) or wrecks, which both get the decor
//! budget.

use sf_data::units::{Category, Faction as DefFaction, UnitDefs};

/// The faction prefixes a file name may start with.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Faction {
    Meridian,
    Veyran,
    Neutral,
    Network,
}

impl Faction {
    const ALL: [Self; 4] = [Self::Meridian, Self::Veyran, Self::Neutral, Self::Network];

    /// The prefix in the file name, without the underscore.
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Meridian => "meridian",
            Self::Veyran => "veyran",
            Self::Neutral => "neutral",
            Self::Network => "network",
        }
    }

    /// Faction models carry the `team` material; neutral objects do not.
    pub const fn needs_team(self) -> bool {
        !matches!(self, Self::Neutral)
    }

    /// The faction of `unit_defs.ron` this prefix stands for, if it has one
    /// (`network` is not a faction of the unit definitions).
    fn def_faction(self) -> Option<DefFaction> {
        match self {
            Self::Meridian => Some(DefFaction::Meridian),
            Self::Veyran => Some(DefFaction::Veyran),
            Self::Neutral => Some(DefFaction::Neutral),
            Self::Network => None,
        }
    }
}

/// A file name taken apart: `<faction>_<role>.glb`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelName {
    pub faction: Faction,
    pub role: String,
    /// The file name without `.glb`; the LOD roots are named `<stem>_LOD<n>`.
    pub stem: String,
}

/// Take a file name apart, or say what is wrong with it.
pub fn parse_file_name(file_name: &str) -> Result<ModelName, String> {
    let stem = file_name
        .strip_suffix(".glb")
        .ok_or_else(|| format!("{file_name:?} does not end in lower-case .glb"))?;
    if stem.is_empty()
        || !stem
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(format!(
            "{file_name:?}: only a-z, 0-9 and _ are allowed before .glb"
        ));
    }
    let faction = Faction::ALL
        .into_iter()
        .find(|f| {
            stem.strip_prefix(f.prefix())
                .is_some_and(|rest| rest.starts_with('_'))
        })
        .ok_or_else(|| {
            format!(
                "{file_name:?} must start with meridian_, veyran_, neutral_ or network_ \
                 (<faction>_<role>.glb)"
            )
        })?;
    let role = &stem[faction.prefix().len() + 1..];
    if role.is_empty() || role.starts_with('_') || role.ends_with('_') || role.contains("__") {
        return Err(format!(
            "{file_name:?}: the role after the faction is empty or has stray underscores"
        ));
    }
    Ok(ModelName {
        faction,
        role: role.to_owned(),
        stem: stem.to_owned(),
    })
}

/// The budget class of a model.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Class {
    Infantry,
    /// Land vehicle or vessel.
    Vehicle,
    Building,
    /// Trees, rocks, ore fields and other objects of the world.
    Decor,
    /// A destroyed vehicle or vessel. Budget of decor, node rules of vehicles.
    Wreck,
}

impl Class {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Infantry => "infantry",
            Self::Vehicle => "vehicle",
            Self::Building => "building",
            Self::Decor => "decor",
            Self::Wreck => "wreck",
        }
    }
}

/// Triangle budget per LOD (LOD0, LOD1, LOD2). `pair` = two figures in one file.
pub const fn budget(class: Class, pair: bool) -> [u32; 3] {
    match class {
        Class::Infantry if pair => [3000, 500, 120],
        Class::Infantry => [1500, 250, 60],
        Class::Vehicle | Class::Decor | Class::Wreck => [1500, 500, 150],
        Class::Building => [5000, 1500, 300],
    }
}

/// Roles that are not in `unit_defs.ron` but are decor, by their first word.
const DECOR_WORDS: [&str; 2] = ["tree", "rock"];

/// Roles that are two figures in one file; they need the `_2` nodes even if
/// the file forgot them.
pub const TWO_FIGURE_ROLES: [&str; 1] = ["demolition_team"];

/// What the role is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Classified {
    pub class: Class,
    /// Vessel or vessel wreck: Y = 0 is the waterline, not the ground.
    pub vessel: bool,
    /// The factions `unit_defs.ron` gives this role, if it is a unit there.
    pub unit_factions: Option<Vec<DefFaction>>,
}

/// Which class `role` has, from `unit_defs.ron` (exact id, or the longest id
/// followed by `_`, so `ore_field_stage_01` is an `ore_field`), else from the
/// name (`wreck_*`, `tree_*`, `rock_*`).
pub fn classify(role: &str, defs: &UnitDefs) -> Result<Classified, String> {
    let words: Vec<&str> = role.split('_').collect();
    if words[0] == "wreck" {
        return Ok(Classified {
            class: Class::Wreck,
            vessel: words.contains(&"vessel"),
            unit_factions: None,
        });
    }
    let def = defs.unit(role).or_else(|| {
        defs.units()
            .iter()
            .filter(|u| {
                role.strip_prefix(u.id.as_str())
                    .is_some_and(|rest| rest.starts_with('_'))
            })
            .max_by_key(|u| u.id.len())
    });
    if let Some(def) = def {
        let class = match def.category {
            Category::Infantry => Class::Infantry,
            Category::Vehicle | Category::Vessel => Class::Vehicle,
            Category::Structure => Class::Building,
            Category::Object => Class::Decor,
        };
        return Ok(Classified {
            class,
            vessel: def.category == Category::Vessel,
            unit_factions: Some(def.factions.clone()),
        });
    }
    if DECOR_WORDS.contains(&words[0]) {
        return Ok(Classified {
            class: Class::Decor,
            vessel: false,
            unit_factions: None,
        });
    }
    Err(format!(
        "cannot tell the budget class of role {role:?}: it is not a role in unit_defs.ron \
         and not wreck_*, tree_* or rock_*"
    ))
}

impl Classified {
    /// Whether the file's faction prefix is one `unit_defs.ron` gives the
    /// role. `None` when the role is not a unit there or the prefix has no
    /// counterpart.
    pub fn faction_listed(&self, faction: Faction) -> Option<bool> {
        let listed = self.unit_factions.as_ref()?;
        Some(listed.contains(&faction.def_faction()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defs() -> UnitDefs {
        UnitDefs::embedded()
    }

    #[test]
    fn file_names_are_taken_apart() {
        let n = parse_file_name("veyran_combat_engineer.glb").unwrap();
        assert_eq!(n.faction, Faction::Veyran);
        assert_eq!(n.role, "combat_engineer");
        assert_eq!(n.stem, "veyran_combat_engineer");
        assert_eq!(
            parse_file_name("network_relay.glb").unwrap().faction,
            Faction::Network
        );
    }

    #[test]
    fn bad_file_names_are_refused_with_a_reason() {
        for bad in [
            "rifleman.glb",
            "Meridian_rifleman.glb",
            "meridian_rifleman.GLB",
            "meridian_rifleman.gltf",
            "meridian_.glb",
            "meridian.glb",
            "meridianrifleman.glb",
            "meridian_rifle man.glb",
            "meridian__rifleman.glb",
            "meridian_rifleman_.glb",
            ".glb",
        ] {
            assert!(parse_file_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn roles_get_the_class_of_their_unit_category() {
        let d = defs();
        let class = |r: &str| classify(r, &d).unwrap();
        assert_eq!(class("rifleman").class, Class::Infantry);
        assert_eq!(class("civilian").class, Class::Infantry);
        assert_eq!(class("light_tank").class, Class::Vehicle);
        assert_eq!(class("power_plant").class, Class::Building);
        let gunboat = class("gunboat");
        assert_eq!((gunboat.class, gunboat.vessel), (Class::Vehicle, true));
        // A stage of an ore field is the ore_field object: decor.
        assert_eq!(class("ore_field_stage_01").class, Class::Decor);
        // Longest id wins: combat_engineer is not an engineer variant.
        assert_eq!(class("combat_engineer").class, Class::Infantry);
    }

    #[test]
    fn decor_and_wrecks_come_from_the_name() {
        let d = defs();
        assert_eq!(classify("tree_pine_burnt", &d).unwrap().class, Class::Decor);
        assert_eq!(classify("rock_large", &d).unwrap().class, Class::Decor);
        let wreck = classify("wreck_light_vehicle", &d).unwrap();
        assert_eq!((wreck.class, wreck.vessel), (Class::Wreck, false));
        let vessel = classify("wreck_vessel", &d).unwrap();
        assert_eq!((vessel.class, vessel.vessel), (Class::Wreck, true));
        assert!(classify("spaceship", &d).is_err());
    }

    #[test]
    fn the_unit_defs_say_which_factions_may_have_a_role() {
        let d = defs();
        let sniper = classify("sniper", &d).unwrap();
        assert_eq!(sniper.faction_listed(Faction::Meridian), Some(true));
        assert_eq!(sniper.faction_listed(Faction::Veyran), Some(false));
        assert_eq!(sniper.faction_listed(Faction::Network), None);
        assert_eq!(
            classify("tree_pine", &d)
                .unwrap()
                .faction_listed(Faction::Neutral),
            None
        );
    }

    #[test]
    fn budgets_per_class() {
        assert_eq!(budget(Class::Infantry, false), [1500, 250, 60]);
        assert_eq!(budget(Class::Infantry, true), [3000, 500, 120]);
        assert_eq!(budget(Class::Vehicle, false), [1500, 500, 150]);
        assert_eq!(budget(Class::Decor, false), budget(Class::Vehicle, false));
        assert_eq!(budget(Class::Wreck, false), budget(Class::Decor, false));
        assert_eq!(budget(Class::Building, false), [5000, 1500, 300]);
    }
}
