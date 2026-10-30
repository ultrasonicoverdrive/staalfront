//! WP7.4: what the HUD reads of the money from the snapshot: the credits of the local player, and
//! which structures wait for them. A world without money rules has no credits to show.

use sf_data::units::UnitDefs;
use sf_game::playfield::{with_buildings, with_harvest, with_money, with_storage};
use sf_game::sim_snapshot::SimSnapshot;
use sf_sim::{Cell, Command, Kind, PlayerId, World, WorldConfig};

const ME: PlayerId = PlayerId(0);
const THEM: PlayerId = PlayerId(1);

fn kind(defs: &UnitDefs, id: &str) -> Kind {
    Kind(defs.kind_of(id).unwrap())
}

fn base() -> (World, UnitDefs) {
    let defs = UnitDefs::embedded();
    let world = with_buildings(
        World::new(WorldConfig::new(9).with_map_size(64, 64))
            .unwrap()
            .with_units(sf_bridge::unit_table(&defs).unwrap())
            .unwrap(),
        &defs,
    );
    (with_storage(with_harvest(world, &defs), &defs), defs)
}

fn place(world: &mut World, owner: PlayerId, kind: Kind, cell: Cell) {
    world
        .schedule(
            world.tick(),
            owner,
            Command::PlaceStructure { kind, cell, owner },
        )
        .unwrap();
    world.step();
}

fn build(world: &mut World, owner: PlayerId, kind: Kind, cell: Cell) {
    world
        .schedule(world.tick(), owner, Command::BuildStructure { kind, cell })
        .unwrap();
    world.step();
}

#[test]
fn the_snapshot_has_the_credits_and_the_structures_that_wait_for_them() {
    let (world, defs) = base();
    let mut world = with_money(world);
    let (cc, plant) = (
        kind(&defs, "construction_center"),
        kind(&defs, "power_plant"),
    );
    place(&mut world, ME, cc, Cell::new(20, 20));
    place(&mut world, THEM, cc, Cell::new(40, 40));
    // The shipped start is 1000 credits for each seat.
    let snap = SimSnapshot::capture(&world);
    assert_eq!(snap.credits_of(ME), Some(1000));
    assert_eq!(snap.credits_of(THEM), Some(1000));
    assert!(snap.stalled().is_empty());
    assert!(!snap.waits_for_money(ME));

    // The power plant costs 300 over 300 ticks, 1 a tick: 1000 pay for it, and nothing waits.
    build(&mut world, ME, plant, Cell::new(26, 20));
    for _ in 0..10 {
        world.step();
    }
    let snap = SimSnapshot::capture(&world);
    assert!(snap.credits_of(ME).unwrap() < 1000);
    assert!(!snap.waits_for_money(ME));
}

#[test]
fn a_structure_that_cannot_be_paid_is_listed_for_its_owner_only() {
    let (world, defs) = base();
    // Money with nothing to start on: build the world by hand with 0 credits for both.
    let world = world
        .with_money(sf_sim::MoneyRules { credits_per_ore: 1 }, &[])
        .unwrap();
    let mut world = world;
    let (cc, plant) = (
        kind(&defs, "construction_center"),
        kind(&defs, "power_plant"),
    );
    place(&mut world, ME, cc, Cell::new(20, 20));
    place(&mut world, THEM, cc, Cell::new(40, 40));
    build(&mut world, ME, plant, Cell::new(26, 20));
    world.step();
    world.step();
    let snap = SimSnapshot::capture(&world);
    let id = snap
        .views()
        .iter()
        .find(|v| v.kind == plant)
        .expect("the plant is placed")
        .id;
    assert_eq!(snap.stalled(), &[id]);
    assert!(snap.waits_for_money(ME));
    assert!(!snap.waits_for_money(THEM));
    assert_eq!(snap.credits_of(ME), Some(0));
}

#[test]
fn a_world_without_money_rules_has_no_credits_to_show() {
    let (mut world, defs) = base();
    place(
        &mut world,
        ME,
        kind(&defs, "construction_center"),
        Cell::new(20, 20),
    );
    let snap = SimSnapshot::capture(&world);
    assert_eq!(snap.credits_of(ME), None);
    assert!(!snap.waits_for_money(ME));
}

#[test]
fn a_player_that_owns_nothing_still_shows_its_credits() {
    // A money world before any placement: no views at all, yet each seat holds the shipped 1000.
    let (world, _) = base();
    let world = with_money(world);
    assert_eq!(world.credits(ME), 1000);
    let snap = SimSnapshot::capture(&world);
    assert_eq!(snap.credits_of(ME), Some(1000));
    assert_eq!(snap.credits_of(THEM), Some(1000));
    // A seat the money rules do not know holds nothing.
    assert_eq!(snap.credits_of(PlayerId(7)), Some(0));
}
