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
//! Since WP4.1 it also carries the terrain half: [`movement_table`] copies the
//! terrain cost of every movement class into a [`MovementTable`], and
//! [`terrain_defs`] turns the terrain types into the simulation's definitions,
//! with the one flag that is derived from those costs.
//!
//! ```
//! use sf_data::units::UnitDefs;
//! use sf_sim::{Kind, World, WorldConfig};
//!
//! let table = sf_bridge::unit_table(&UnitDefs::embedded()).unwrap();
//! let world = World::new(WorldConfig::new(1)).unwrap().with_units(table).unwrap();
//! assert!(world.unit_table().get(Kind(12)).is_some()); // the rifleman
//! ```

use sf_data::movement::MovementCosts;
use sf_data::sim_settings::SimSettings;
use sf_data::terrain::TerrainTypes;
use sf_data::units::{SimUnitDef, TerrainCost, TurnMode, TurningDef, UnitDefs};
use sf_sim::{
    ClassCosts, Cost, Footprint, Kind, MovementTable, TerrainDef, TerrainFlags, TerrainId,
    TurnMode as SimTurnMode, Turning, UnitStats, UnitTable,
};

/// What the rules read about one role, copied from its converted definition.
/// `defs` is where its first weapon is looked up (the reach of an attack order).
pub fn unit_stats_of(defs: &UnitDefs, role: &SimUnitDef) -> UnitStats {
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
        weapon_range: role
            .weapons
            .first()
            .and_then(|&kind| defs.sim_weapon(kind))
            .map(|weapon| weapon.range)
            .unwrap_or_default(),
        turret_per_tick: role.turret_per_tick,
    }
}

/// The stats of every role of `defs`, ascending by kind.
pub fn unit_stats(defs: &UnitDefs) -> Vec<UnitStats> {
    defs.sim_units()
        .iter()
        .map(|role| unit_stats_of(defs, role))
        .collect()
}

/// The simulation's unit table for `defs`, to hand to
/// [`World::with_units`](sf_sim::World::with_units). Fails with the
/// simulation's reason if the table is refused (the definitions' own checks
/// rule that out for any definitions that loaded).
pub fn unit_table(defs: &UnitDefs) -> Result<UnitTable, String> {
    UnitTable::new(unit_stats(defs)).map_err(|e| e.to_string())
}

/// One cost of the data, as the simulation has it.
pub fn sim_cost(cost: TerrainCost) -> Cost {
    match cost {
        TerrainCost::Impassable => Cost::Impassable,
        TerrainCost::Speed(permille) => Cost::Speed(permille),
    }
}

/// One turning profile of the data, as the simulation has it (WP4.5).
pub fn sim_turning(def: &TurningDef) -> Turning {
    Turning {
        mode: match def.mode {
            TurnMode::Pivot => SimTurnMode::Pivot,
            TurnMode::Arc => SimTurnMode::Arc,
            TurnMode::Instant => SimTurnMode::Instant,
        },
        drive_tolerance: def.drive_tolerance,
        min_radius: def.min_radius.to_fx(),
        pivot_permille: def.pivot_permille,
    }
}

/// The simulation's movement table for `costs`, to hand to
/// [`World::with_movement`](sf_sim::World::with_movement): the terrain costs
/// and, since WP4.5, how each class turns. Fails with the simulation's reason
/// if the table is refused (the data's own checks rule that out for any costs
/// that were built).
pub fn movement_table(costs: &MovementCosts) -> Result<MovementTable, String> {
    MovementTable::new(
        costs
            .classes()
            .iter()
            .map(|class| ClassCosts {
                class: class.kind,
                costs: class
                    .costs()
                    .iter()
                    .map(|&(terrain, cost)| (TerrainId(terrain), sim_cost(cost)))
                    .collect(),
            })
            .collect(),
    )
    .and_then(|table| {
        table.with_turning(
            costs
                .classes()
                .iter()
                .map(|class| (class.kind, sim_turning(&class.turning)))
                .collect(),
        )
    })
    .map_err(|e| e.to_string())
}

/// The node budget of the path service per tick, from the settings of
/// `sim.ron`, to hand to [`World::with_path_budget`](sf_sim::World::with_path_budget)
/// (which refuses one below [`MIN_NODE_BUDGET`](sf_sim::MIN_NODE_BUDGET)).
pub fn path_budget(settings: &SimSettings) -> u32 {
    settings.path_node_budget
}

/// The flowfield threshold from the settings of `sim.ron`, to hand to
/// [`World::with_flowfield_threshold`](sf_sim::World::with_flowfield_threshold)
/// (which refuses one).
pub fn flowfield_threshold(settings: &SimSettings) -> u32 {
    settings.flowfield_threshold
}

/// The collision settings from the settings of `sim.ron`, to hand to
/// [`World::with_collision`](sf_sim::World::with_collision) (which refuses a
/// combination that does not fit; `sf_data` has checked the same rules).
pub fn collision_config(settings: &SimSettings) -> sf_sim::CollisionConfig {
    sf_sim::CollisionConfig {
        enabled: settings.collision,
        wait_ticks: settings.wait_ticks,
        stuck_ticks: settings.stuck_ticks,
        stuck_attempts: settings.stuck_attempts,
    }
}

/// The replan settings from the settings of `sim.ron` (WP4.7), to hand to
/// [`World::with_replan`](sf_sim::World::with_replan) (which refuses a combination
/// that does not fit; `sf_data` has checked the same rules).
pub fn replan_config(settings: &SimSettings) -> sf_sim::ReplanConfig {
    sf_sim::ReplanConfig {
        cooldown: settings.replan_cooldown,
        max: settings.replan_max,
        retarget_cooldown: settings.retarget_cooldown,
    }
}

/// The formation settings from the settings of `sim.ron` (WP4.6), to hand to
/// [`World::with_formation`](sf_sim::World::with_formation) (which refuses a combination
/// that does not fit; `sf_data` has checked the same rules).
pub fn formation_config(settings: &SimSettings) -> sf_sim::FormationConfig {
    sf_sim::FormationConfig {
        enabled: settings.formation,
        min_units: settings.formation_min_units,
        max_units: settings.formation_max_units,
        spacing_permille: settings.formation_spacing_permille,
        spread_permille: settings.formation_spread_permille,
        pace_permille: settings.formation_pace_permille,
        catchup_permille: settings.formation_catchup_permille,
        reform_tenths: settings.formation_reform_tenths,
        patience_ticks: settings.formation_patience_ticks,
    }
}

/// The simulation's table of terrain types: ids and flags, no names. The
/// `ground` flag (open ground for foot, wheeled and tracked units) is derived
/// from the movement costs; it is part of the terrain digest, see
/// [`TerrainFlags::ground`].
pub fn terrain_defs(types: &TerrainTypes, costs: &MovementCosts) -> Vec<TerrainDef> {
    types
        .iter()
        .map(|t| TerrainDef {
            id: TerrainId(t.id),
            flags: TerrainFlags {
                buildable: t.buildable,
                harvestable: t.harvestable,
                ground: costs.ground_passable(t.id),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_sim::{UnitClass, World, WorldConfig};

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
            assert_eq!(stats.turret_per_tick, role.turret_per_tick);
            assert_eq!(stats.size, role.size);
            assert_eq!(stats.footprint.map(|f| (f.width, f.height)), role.footprint);
        }
    }

    #[test]
    fn the_reach_is_the_first_weapons_range() {
        let defs = UnitDefs::embedded();
        let table = unit_table(&defs).unwrap();
        let reach = |id: &str| {
            table
                .get(Kind(defs.kind_of(id).unwrap()))
                .unwrap()
                .weapon_range
        };
        // The rifleman's rifle and the siege artillery's howitzer.
        let weapon = |id: &str| {
            defs.sim_weapon(defs.weapon(id).unwrap().kind)
                .unwrap()
                .range
        };
        assert_eq!(reach("rifleman"), weapon("rifle"));
        assert_eq!(reach("siege_artillery"), weapon("howitzer"));
        assert!(reach("rifleman") > reach("harvester"));
        // No weapon, no reach.
        assert_eq!(reach("harvester"), Default::default());
        assert_eq!(reach("refinery"), Default::default());
        for role in defs.sim_units() {
            let first = role
                .weapons
                .first()
                .map(|&k| defs.sim_weapon(k).unwrap().range)
                .unwrap_or_default();
            assert_eq!(table.get(Kind(role.kind)).unwrap().weapon_range, first);
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
    fn every_class_arrives_with_its_costs() {
        let defs = UnitDefs::embedded();
        let types = TerrainTypes::embedded();
        let costs = MovementCosts::from_defs(&defs, &types).unwrap();
        let table = movement_table(&costs).unwrap();
        assert_eq!(table.len(), defs.classes().movement_classes.len());
        for class in &defs.classes().movement_classes {
            for t in types.iter() {
                let want = sim_cost(class.cost_on(&t.name).unwrap());
                assert_eq!(
                    table.get(class.kind, TerrainId(t.id)),
                    Some(Some(want)),
                    "{} on {}",
                    class.name,
                    t.name
                );
            }
        }
        // Built from the same data, in any order, it is the same table.
        assert_eq!(table, movement_table(&MovementCosts::embedded()).unwrap());
    }

    #[test]
    fn every_class_arrives_with_the_way_it_turns() {
        let defs = UnitDefs::embedded();
        let costs = MovementCosts::from_defs(&defs, &TerrainTypes::embedded()).unwrap();
        let table = movement_table(&costs).unwrap();
        let by_name = |name: &str| table.turning(defs.classes().movement_kind(name).unwrap());
        // Infantry turn at once, tanks pivot, wheeled vehicles must move to turn.
        assert_eq!(by_name("foot").mode, SimTurnMode::Instant);
        assert_eq!(by_name("tracked").mode, SimTurnMode::Pivot);
        let wheeled = by_name("wheeled");
        assert_eq!(wheeled.mode, SimTurnMode::Arc);
        // The radius is the data's, in cells, as fixed point.
        let data = defs.classes().movement_class(1).unwrap().turning;
        assert_eq!(wheeled.min_radius, data.min_radius.to_fx());
        assert_eq!(wheeled.pivot_permille, data.pivot_permille);
        assert_eq!(wheeled.drive_tolerance, data.drive_tolerance);
        // Every class of the data has its profile, and it is a valid one.
        for class in &defs.classes().movement_classes {
            assert_eq!(table.turning(class.kind), sim_turning(&class.turning));
            assert_eq!(table.turning(class.kind).check(), Ok(()));
        }
    }

    #[test]
    fn every_unit_has_a_class_the_table_knows_and_the_unit_table_carries_it() {
        let defs = UnitDefs::embedded();
        let units = unit_table(&defs).unwrap();
        let movement = movement_table(&MovementCosts::embedded()).unwrap();
        let mut mobile = 0;
        for role in defs.units() {
            let stats = units.get(Kind(role.kind)).unwrap();
            // The class of the stats is the class the definition names.
            let named = role
                .movement
                .as_deref()
                .map(|name| defs.classes().movement_kind(name).unwrap());
            assert_eq!(stats.movement_class, named, "{}", role.id);
            if let Some(class) = stats.movement_class {
                mobile += 1;
                assert!(movement.has_class(class), "{}: class {class}", role.id);
            }
        }
        assert!(mobile > 10);
    }

    #[test]
    fn the_terrain_flags_come_from_the_types_and_the_ground_flag_from_the_costs() {
        let types = TerrainTypes::embedded();
        let costs = MovementCosts::embedded();
        let defs = terrain_defs(&types, &costs);
        assert_eq!(defs.len(), types.len());
        let flags = |name: &str| {
            let id = types.id_of(name).unwrap();
            defs.iter().find(|d| d.id.0 == id).unwrap().flags
        };
        for name in ["grass", "road", "ore", "bridge"] {
            assert!(flags(name).ground, "{name}");
        }
        for name in ["water", "rock"] {
            assert!(!flags(name).ground, "{name}");
        }
        assert!(flags("grass").buildable && !flags("road").buildable);
        assert!(flags("ore").harvestable && !flags("grass").harvestable);
    }

    #[test]
    fn the_shipped_path_budget_is_one_a_world_takes_and_the_simulations_default() {
        let budget = path_budget(&SimSettings::embedded());
        assert_eq!(budget, sf_sim::DEFAULT_NODE_BUDGET);
        assert!(budget >= sf_sim::MIN_NODE_BUDGET);
        assert!(
            World::new(WorldConfig::new(1))
                .unwrap()
                .with_path_budget(budget)
                .is_ok()
        );
        // And a budget the world refuses comes back as its error, not a panic here.
        let too_small = SimSettings {
            path_node_budget: sf_sim::MIN_NODE_BUDGET - 1,
            ..SimSettings::embedded()
        };
        assert!(
            World::new(WorldConfig::new(1))
                .unwrap()
                .with_path_budget(path_budget(&too_small))
                .is_err()
        );
    }

    #[test]
    fn the_shipped_flowfield_threshold_is_one_a_world_takes_and_the_simulations_default() {
        let threshold = flowfield_threshold(&SimSettings::embedded());
        assert_eq!(threshold, sf_sim::DEFAULT_FLOWFIELD_THRESHOLD);
        let world = World::new(WorldConfig::new(1))
            .unwrap()
            .with_flowfield_threshold(threshold)
            .unwrap();
        assert_eq!(world.flowfield_threshold(), threshold);
        // One is no group; the world says so instead of the bridge deciding.
        assert!(
            World::new(WorldConfig::new(1))
                .unwrap()
                .with_flowfield_threshold(1)
                .is_err()
        );
    }

    #[test]
    fn the_shipped_collision_settings_are_the_simulations_defaults_and_a_world_takes_them() {
        let config = collision_config(&SimSettings::embedded());
        assert_eq!(config, sf_sim::CollisionConfig::DEFAULT);
        let world = World::new(WorldConfig::new(1))
            .unwrap()
            .with_collision(config)
            .unwrap();
        assert_eq!(world.collision_config(), config);
        // A combination that does not fit comes back as the world's error.
        let bad = sf_sim::CollisionConfig {
            wait_ticks: 0,
            ..config
        };
        assert!(
            World::new(WorldConfig::new(1))
                .unwrap()
                .with_collision(bad)
                .is_err()
        );
    }

    #[test]
    fn the_shipped_replan_settings_are_the_simulations_defaults_and_a_world_takes_them() {
        let config = replan_config(&SimSettings::embedded());
        assert_eq!(config, sf_sim::ReplanConfig::DEFAULT);
        let world = World::new(WorldConfig::new(1))
            .unwrap()
            .with_replan(config)
            .unwrap();
        assert_eq!(world.replan_config(), config);
        let bad = sf_sim::ReplanConfig { max: 0, ..config };
        assert!(
            World::new(WorldConfig::new(1))
                .unwrap()
                .with_replan(bad)
                .is_err()
        );
    }

    #[test]
    fn the_shipped_formation_settings_are_the_simulations_defaults_and_a_world_takes_them() {
        let config = formation_config(&SimSettings::embedded());
        assert_eq!(config, sf_sim::FormationConfig::DEFAULT);
        let world = World::new(WorldConfig::new(1))
            .unwrap()
            .with_formation(config)
            .unwrap();
        assert_eq!(world.formation_config(), config);
        // A combination that does not fit comes back as the world's error.
        let bad = sf_sim::FormationConfig {
            pace_permille: 1200,
            ..config
        };
        assert!(
            World::new(WorldConfig::new(1))
                .unwrap()
                .with_formation(bad)
                .is_err()
        );
        // The settings of the file arrive one by one.
        let settings = SimSettings {
            formation: false,
            formation_max_units: 20,
            formation_reform_tenths: 45,
            ..SimSettings::embedded()
        };
        let changed = formation_config(&settings);
        assert!(!changed.enabled);
        assert_eq!((changed.max_units, changed.reform_tenths), (20, 45));
    }

    #[test]
    fn the_stats_come_in_kind_order() {
        let stats = unit_stats(&UnitDefs::embedded());
        assert!(stats.windows(2).all(|w| w[0].kind < w[1].kind));
    }
}
