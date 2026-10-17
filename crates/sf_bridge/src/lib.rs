//! The bridge between the unit definitions and the simulation (WP3.2).
//!
//! `sf_data` reads, checks and converts `assets/config/unit_defs.ron` (to
//! `SimUnitDef`: integers and fixed point in the simulation's units). `sf_sim`
//! takes its own, smaller [`UnitStats`] in a [`UnitTable`] and may not depend on
//! `sf_data`; `sf_data` may not depend on `sf_sim`. This crate is the one
//! function that copies the first into the second, so the game (`sf_game`) and
//! the headless runner (`sf_headless`) share it instead of each carrying a copy.
//! It has no logic of its own to get wrong: every field is a move or a rename.
//!
//! Why a crate and not a module of one of the consumers: the game and the
//! headless runner must not depend on each other (the runner may not pull in
//! rendering, and the game has no business linking a tool), and the other
//! crates that see both sides (`sf_ai`) are about something else.
//!
//! ```
//! use sf_data::units::UnitDefs;
//! use sf_sim::{Kind, World, WorldConfig};
//!
//! let table = sf_bridge::unit_table(&UnitDefs::embedded()).unwrap();
//! let world = World::new(WorldConfig::new(1)).unwrap().with_units(table).unwrap();
//! assert!(world.unit_table().get(Kind(12)).is_some()); // the rifleman
//! ```

use sf_data::units::{SimUnitDef, UnitDefs};
use sf_sim::{Footprint, Kind, UnitStats, UnitTable};

/// What the rules read about one role, copied from its converted definition.
pub fn unit_stats_of(role: &SimUnitDef) -> UnitStats {
    UnitStats {
        kind: Kind(role.kind),
        hp: role.hp,
        movement_class: role.movement_class,
        speed_per_tick: role.speed_per_tick,
        turn_per_tick: role.turn_per_tick,
        size: role.size,
        footprint: role
            .footprint
            .map(|(width, height)| Footprint { width, height }),
    }
}

/// The stats of every role of `defs`, ascending by kind.
pub fn unit_stats(defs: &UnitDefs) -> Vec<UnitStats> {
    defs.sim_units().iter().map(unit_stats_of).collect()
}

/// The simulation's unit table for `defs`, to hand to
/// [`World::with_units`](sf_sim::World::with_units). Fails with the
/// simulation's reason if the table is refused (the definitions' own checks
/// rule that out for any definitions that loaded).
pub fn unit_table(defs: &UnitDefs) -> Result<UnitTable, String> {
    UnitTable::new(unit_stats(defs)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_sim::UnitClass;

    #[test]
    fn every_role_arrives_with_its_numbers() {
        let defs = UnitDefs::embedded();
        let table = unit_table(&defs).unwrap();
        assert_eq!(table.len(), defs.units().len());
        for role in defs.sim_units() {
            let stats = table
                .get(Kind(role.kind))
                .expect("every role is in the table");
            assert_eq!(stats.hp, role.hp, "kind {}", role.kind);
            assert_eq!(stats.movement_class, role.movement_class);
            assert_eq!(stats.speed_per_tick, role.speed_per_tick);
            assert_eq!(stats.turn_per_tick, role.turn_per_tick);
            assert_eq!(stats.size, role.size);
            assert_eq!(stats.footprint.map(|f| (f.width, f.height)), role.footprint);
        }
    }

    #[test]
    fn structures_units_and_objects_keep_their_class() {
        let defs = UnitDefs::embedded();
        let table = unit_table(&defs).unwrap();
        let class = |id: &str| table.get(Kind(defs.kind_of(id).unwrap())).unwrap().class();
        assert_eq!(class("rifleman"), UnitClass::Mobile);
        assert_eq!(class("gunboat"), UnitClass::Mobile);
        assert_eq!(class("refinery"), UnitClass::Structure);
        assert_eq!(class("evidence_crate"), UnitClass::Object);
        assert_eq!(class("ore_field"), UnitClass::Object);
    }

    #[test]
    fn the_stats_come_in_kind_order() {
        let stats = unit_stats(&UnitDefs::embedded());
        assert!(stats.windows(2).all(|w| w[0].kind < w[1].kind));
    }
}
