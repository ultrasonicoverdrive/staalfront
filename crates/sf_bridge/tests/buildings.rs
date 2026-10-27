//! WP6.1: the building definitions become the simulation's building table.
//!
//! The data (`sf_data`) is checked in its own tests; here the table that reaches the
//! simulation: every structure of the shipped definitions, in fixed point and ticks, a world
//! that takes the table only in the right place, and a checksum that sees it.

use sf_bridge::{building_table, unit_table};
use sf_data::units::UnitDefs;
use sf_sim::{
    BuildingFunction, BuildingStats, BuildingTable, BuildingsError, Command, ConfigError, Kind,
    Side, World, WorldConfig,
};

fn defs() -> UnitDefs {
    UnitDefs::embedded()
}

fn kind(defs: &UnitDefs, id: &str) -> Kind {
    Kind(defs.kind_of(id).unwrap())
}

fn world(defs: &UnitDefs) -> World {
    World::new(WorldConfig::new(7))
        .unwrap()
        .with_units(unit_table(defs).unwrap())
        .unwrap()
}

fn with_buildings(defs: &UnitDefs) -> World {
    world(defs)
        .with_buildings(building_table(defs).unwrap())
        .unwrap()
}

#[test]
fn the_shipped_structures_reach_the_sim_in_ticks_and_cells() {
    let defs = defs();
    let table = building_table(&defs).unwrap();
    assert_eq!(table.len(), 19);
    let yard = table.get(kind(&defs, "vehicle_factory")).unwrap();
    assert_eq!(yard.function, BuildingFunction::VehicleFactory);
    assert_eq!(yard.exit_side, Some(Side::PosX));
    assert_eq!((yard.footprint.width, yard.footprint.height), (3, 4));
    assert_eq!(yard.build_ticks, 1000); // 50 seconds at 20 ticks per second
    assert_eq!(yard.cost, Some(1500));
    assert_eq!(yard.power, -40);
    assert_eq!(yard.requires, vec![kind(&defs, "refinery")]);
    let plant = table.get(kind(&defs, "power_plant")).unwrap();
    assert_eq!(plant.function, BuildingFunction::PowerPlant);
    assert_eq!(plant.power, 100);
    let anchor = table.get(kind(&defs, "construction_center")).unwrap();
    assert_eq!(anchor.function, BuildingFunction::Anchor);
    assert_eq!((anchor.cost, anchor.build_ticks), (None, 0));
    // Only structures: no unit has an entry.
    assert!(table.get(kind(&defs, "rifleman")).is_none());
    // Every entry agrees with the unit table about its footprint.
    let units = unit_table(&defs).unwrap();
    for b in table.iter() {
        assert_eq!(units.get(b.kind).unwrap().footprint, Some(b.footprint));
    }
}

#[test]
fn the_world_takes_the_table_after_the_units_and_before_it_starts() {
    let defs = defs();
    let table = building_table(&defs).unwrap();
    let w = with_buildings(&defs);
    assert_eq!(w.building_table().len(), 19);
    assert_eq!(w.building_table().digest(), table.digest());
    assert!(world(&defs).building_table().is_empty());

    // A tick has run.
    let mut started = world(&defs);
    started.step();
    assert_eq!(
        started.with_buildings(table.clone()).unwrap_err(),
        ConfigError::BuildingsAfterStart
    );
    // A command is queued.
    let mut queued = world(&defs);
    queued
        .schedule(
            sf_sim::Tick::ZERO,
            sf_sim::PlayerId(0),
            Command::PlaceStructure {
                kind: kind(&defs, "power_plant"),
                cell: sf_sim::Cell::new(2, 2),
                owner: sf_sim::PlayerId(0),
            },
        )
        .unwrap();
    assert_eq!(
        queued.with_buildings(table.clone()).unwrap_err(),
        ConfigError::BuildingsAfterStart
    );
}

fn first(defs: &UnitDefs) -> BuildingStats {
    building_table(defs).unwrap().iter().next().unwrap().clone()
}

#[test]
fn an_entry_that_does_not_fit_the_unit_table_is_refused() {
    let defs = defs();
    // A kind the unit table does not have.
    let mut unknown = first(&defs);
    unknown.kind = Kind(999);
    unknown.requires.clear();
    let t = BuildingTable::new(vec![unknown]).unwrap();
    assert_eq!(
        world(&defs).with_buildings(t).unwrap_err(),
        ConfigError::Buildings(BuildingsError::NotAStructure(Kind(999)))
    );
    // A kind that is a unit, not a structure.
    let mut unit = first(&defs);
    unit.kind = kind(&defs, "rifleman");
    unit.requires.clear();
    let t = BuildingTable::new(vec![unit]).unwrap();
    assert_eq!(
        world(&defs).with_buildings(t).unwrap_err(),
        ConfigError::Buildings(BuildingsError::NotAStructure(kind(&defs, "rifleman")))
    );
    // A structure with another footprint than the unit table says.
    let mut wrong = first(&defs);
    wrong.requires.clear();
    wrong.footprint.width += 1;
    let t = BuildingTable::new(vec![wrong.clone()]).unwrap();
    assert_eq!(
        world(&defs).with_buildings(t).unwrap_err(),
        ConfigError::Buildings(BuildingsError::FootprintMismatch(wrong.kind))
    );
}

#[test]
fn the_checksum_sees_the_table_and_only_when_there_is_one() {
    let defs = defs();
    let plain = world(&defs);
    let with = with_buildings(&defs);
    assert_ne!(plain.checksum(), with.checksum());
    // The same table in the same world gives the same checksum, tick after tick.
    let (mut a, mut b) = (with_buildings(&defs), with_buildings(&defs));
    for _ in 0..5 {
        assert_eq!(a.step().checksum, b.step().checksum);
    }
    // One changed field (the exit side of the barracks) is another checksum from the first tick.
    let mut stats: Vec<BuildingStats> = building_table(&defs).unwrap().iter().cloned().collect();
    let barracks = kind(&defs, "barracks");
    for s in stats.iter_mut().filter(|s| s.kind == barracks) {
        s.exit_side = Some(Side::NegY);
    }
    let changed = world(&defs)
        .with_buildings(BuildingTable::new(stats).unwrap())
        .unwrap();
    assert_ne!(changed.checksum(), with.checksum());
}

#[test]
fn placing_a_structure_is_the_same_with_or_without_the_table() {
    // Nothing acts on the table yet (placement is WP6.2): the world's state, apart from the
    // table's own section, is the same, so the structure stands where it is put.
    let defs = defs();
    for mut w in [world(&defs), with_buildings(&defs)] {
        let kind = kind(&defs, "barracks");
        w.schedule(
            sf_sim::Tick::ZERO,
            sf_sim::PlayerId(0),
            Command::PlaceStructure {
                kind,
                cell: sf_sim::Cell::new(4, 4),
                owner: sf_sim::PlayerId(0),
            },
        )
        .unwrap();
        w.step();
        let id = w
            .occupant_at(sf_sim::Cell::new(5, 6))
            .expect("a footprint of 2 x 3 cells");
        assert_eq!(
            w.footprint_of(id).map(|f| (f.width, f.height)),
            Some((2, 3))
        );
    }
}

/// Every data function, by an exhaustive match: a new variant does not compile until it is
/// added here (and so pinned below).
fn all_data_functions() -> Vec<sf_data::units::BuildingFunction> {
    use sf_data::units::BuildingFunction as F;
    let all = [
        F::None,
        F::Anchor,
        F::PowerPlant,
        F::HarvesterDock,
        F::InfantryFactory,
        F::VehicleFactory,
        F::VesselFactory,
        F::Radar,
        F::Defence,
        F::Decoy,
    ];
    for f in all {
        match f {
            F::None
            | F::Anchor
            | F::PowerPlant
            | F::HarvesterDock
            | F::InfantryFactory
            | F::VehicleFactory
            | F::VesselFactory
            | F::Radar
            | F::Defence
            | F::Decoy => {}
        }
    }
    all.to_vec()
}

fn all_data_sides() -> Vec<sf_data::units::Side> {
    use sf_data::units::Side as S;
    let all = [S::PosX, S::NegX, S::PosY, S::NegY];
    for s in all {
        match s {
            S::PosX | S::NegX | S::PosY | S::NegY => {}
        }
    }
    all.to_vec()
}

#[test]
fn sim_numbers_equal_the_data_numbers_for_every_variant() {
    use sf_bridge::{building_function, building_side};
    // The hashed numbers, written out: a renumbering on either side fails here.
    let functions: [(&str, u8); 10] = [
        ("None", 0),
        ("Anchor", 1),
        ("PowerPlant", 2),
        ("HarvesterDock", 3),
        ("InfantryFactory", 4),
        ("VehicleFactory", 5),
        ("VesselFactory", 6),
        ("Radar", 7),
        ("Defence", 8),
        ("Decoy", 9),
    ];
    let data = all_data_functions();
    assert_eq!(data.len(), functions.len());
    for (d, (name, n)) in data.into_iter().zip(functions) {
        let s = building_function(d);
        assert_eq!(format!("{d:?}"), name);
        assert_eq!(format!("{s:?}"), name, "the bridge maps {d:?} to {s:?}");
        assert_eq!(d.number(), n, "data number of {name}");
        assert_eq!(s.number(), n, "sim number of {name}");
    }
    let sides: [(&str, u8); 4] = [("PosX", 0), ("NegX", 1), ("PosY", 2), ("NegY", 3)];
    let data = all_data_sides();
    assert_eq!(data.len(), sides.len());
    for (d, (name, n)) in data.into_iter().zip(sides) {
        let s = building_side(d);
        assert_eq!(format!("{d:?}"), name);
        assert_eq!(format!("{s:?}"), name, "the bridge maps {d:?} to {s:?}");
        assert_eq!(d.number(), n, "data number of {name}");
        assert_eq!(s.number(), n, "sim number of {name}");
    }
}

/// The digest of the shipped building table (sf_data -> building_table -> World::with_buildings).
/// It moves with any change of the shipped structures or of a hashed number; a move needs a
/// deliberate data change and a note in `docs/rules/buildings.md`.
const SHIPPED_BUILDINGS_DIGEST: u64 = 0x53ae_b53a_b441_57d1;

#[test]
fn the_shipped_building_table_digest_is_pinned() {
    let defs = defs();
    let table = building_table(&defs).unwrap();
    let digest = table.digest();
    // The table the world takes is the same one.
    let w = world(&defs).with_buildings(table).unwrap();
    assert_eq!(w.buildings().digest(), digest);
    assert_eq!(digest, SHIPPED_BUILDINGS_DIGEST, "digest {digest:#018x}");
}

#[test]
fn f1_m05_start_state_takes_an_enemy_decoy_and_the_players_base() {
    // F1-M05 places 8 decoy_emplacement for Enemy 1 (Veyran) although the decoy is
    // `factions: [Meridian]`. `PlaceStructure` is the placement of scenario setup and mission
    // objects and checks no faction or prerequisite (WP6.2: only `BuildStructure` does,
    // `tests/placement.rs`), so a pre-placed decoy of another owner is accepted.
    let defs = defs();
    let mut w = with_buildings(&defs);
    let player = sf_sim::PlayerId(0);
    let enemy = sf_sim::PlayerId(1);
    let place = [
        ("construction_center", player, 2, 2),
        ("power_plant", player, 8, 2),
        ("barracks", player, 12, 2),
        ("vehicle_factory", player, 16, 2),
        ("decoy_emplacement", enemy, 30, 30),
    ];
    for (id, owner, x, y) in place {
        w.schedule(
            sf_sim::Tick::ZERO,
            owner,
            Command::PlaceStructure {
                kind: kind(&defs, id),
                cell: sf_sim::Cell::new(x, y),
                owner,
            },
        )
        .unwrap();
    }
    w.step();
    for (id, _, x, y) in place {
        assert!(
            w.occupant_at(sf_sim::Cell::new(x, y)).is_some(),
            "{id} stands"
        );
    }
    // The decoy's prerequisites are all in the player's starting base.
    let table = w.buildings();
    let decoy = table.get(kind(&defs, "decoy_emplacement")).unwrap();
    for r in &decoy.requires {
        assert!(
            place[..4].iter().any(|&(id, ..)| kind(&defs, id) == *r),
            "decoy needs {r:?}"
        );
    }
}
