//! WP7.3: the storage bar belongs to a *standing* storing structure. A refinery under construction
//! has its construction bar and no storage bar; it gets the storage bar on the tick it completes.
//! Read through `SimSnapshot::stores()`, the list `storage_view::draw_bars` draws from.

use sf_data::units::UnitDefs;
use sf_game::playfield::{with_buildings, with_harvest, with_storage};
use sf_game::sim_snapshot::SimSnapshot;
use sf_sim::{Cell, Command, Kind, PlayerId, World, WorldConfig};

const ME: PlayerId = PlayerId(0);

fn kind(defs: &UnitDefs, id: &str) -> Kind {
    Kind(defs.kind_of(id).unwrap())
}

fn place(world: &mut World, kind: Kind, cell: Cell) {
    world
        .schedule(
            world.tick(),
            ME,
            Command::PlaceStructure {
                kind,
                cell,
                owner: ME,
            },
        )
        .unwrap();
    world.step();
}

#[test]
fn a_refinery_under_construction_has_no_storage_bar_until_it_is_complete() {
    let defs = UnitDefs::embedded();
    let refinery = kind(&defs, "refinery");
    let world = with_buildings(
        World::new(WorldConfig::new(9).with_map_size(64, 64))
            .unwrap()
            .with_units(sf_bridge::unit_table(&defs).unwrap())
            .unwrap(),
        &defs,
    );
    let mut world = with_storage(with_harvest(world, &defs), &defs);
    place(
        &mut world,
        kind(&defs, "construction_center"),
        Cell::new(20, 20),
    );
    place(&mut world, kind(&defs, "power_plant"), Cell::new(26, 20));
    world
        .schedule(
            world.tick(),
            ME,
            Command::BuildStructure {
                kind: refinery,
                cell: Cell::new(20, 26),
            },
        )
        .unwrap();
    world.step();
    world.step();

    let snap = SimSnapshot::capture(&world);
    let id = snap
        .views()
        .iter()
        .find(|v| v.kind == refinery)
        .expect("the refinery is placed")
        .id;
    assert!(world.construction_of(id).is_some(), "under construction");
    assert!(
        snap.stores().is_empty(),
        "a refinery under construction carries no storage bar"
    );

    let mut ticks = 0;
    while !world.stands(id) {
        world.step();
        ticks += 1;
        assert!(ticks < 5000, "the refinery completes");
    }
    let snap = SimSnapshot::capture(&world);
    assert_eq!(
        snap.stores(),
        &[id],
        "the complete refinery carries the bar"
    );
}
