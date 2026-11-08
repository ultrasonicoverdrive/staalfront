//! The Repair and Sell modes (WP6.6): what a click on an entity does and which cursor goes with it,
//! read from real snapshots of a world with the shipped definitions and repair rules.

use super::*;
use sf_data::units::UnitDefs;
use sf_sim::{Cell, Kind, World, WorldConfig};

const ME: PlayerId = PlayerId(0);
const THEM: PlayerId = PlayerId(1);

struct Rig {
    world: World,
    defs: UnitDefs,
}

fn rig() -> Rig {
    let defs = UnitDefs::embedded();
    let world = World::new(WorldConfig::new(7).with_map_size(64, 64))
        .unwrap()
        .with_units(sf_bridge::unit_table(&defs).unwrap())
        .unwrap();
    let world = crate::playfield::with_buildings(world, &defs);
    let world = crate::playfield::with_repair(world);
    Rig { world, defs }
}

impl Rig {
    fn kind(&self, id: &str) -> Kind {
        Kind(self.defs.kind_of(id).unwrap())
    }

    fn place(&mut self, id: &str, cell: (u32, u32), owner: PlayerId) -> EntityId {
        let kind = self.kind(id);
        self.world
            .schedule(
                self.world.tick(),
                owner,
                Command::PlaceStructure {
                    kind,
                    cell: Cell::new(cell.0, cell.1),
                    owner,
                },
            )
            .unwrap();
        self.world.step();
        self.world.occupant_at(Cell::new(cell.0, cell.1)).unwrap()
    }

    fn snap(&self) -> SimSnapshot {
        SimSnapshot::capture(&self.world)
    }
}

#[test]
fn the_modes_have_their_own_buttons_and_cursors() {
    assert_eq!(ToolMode::ALL, [ToolMode::Repair, ToolMode::Sell]);
    assert_eq!(ToolMode::Repair.index(), 0);
    assert_eq!(ToolMode::Sell.index(), 1);
    assert_eq!(ToolMode::Repair.cursor(), CursorKind::Repair);
    assert_eq!(ToolMode::Sell.cursor(), CursorKind::Sell);
}

#[test]
fn a_hurt_own_structure_is_offered_a_repair_and_a_whole_one_is_not() {
    let mut r = rig();
    let plant = r.place("power_plant", (10, 10), ME);
    let mut snap = r.snap();
    let max = snap.max_hp(&snap.get(plant).unwrap());
    assert_eq!(max, 600);
    // At full hit points there is nothing to repair; a sale is always offered.
    assert_eq!(
        target(&snap, ToolMode::Repair, Some(plant), ME),
        Target::Nothing
    );
    assert_eq!(target(&snap, ToolMode::Sell, Some(plant), ME), Target::Sell);
    snap.set_hp_for_test(plant, 599);
    assert_eq!(
        target(&snap, ToolMode::Repair, Some(plant), ME),
        Target::StartRepair
    );
    assert_eq!(
        click(&snap, ToolMode::Repair, Some(plant), ME),
        Some(Command::Repair {
            structure: plant,
            on: true
        })
    );
    assert_eq!(
        click(&snap, ToolMode::Sell, Some(plant), ME),
        Some(Command::Sell { structure: plant })
    );
}

#[test]
fn nothing_else_is_offered_anything() {
    let mut r = rig();
    let mine = r.place("power_plant", (10, 10), ME);
    let enemy = r.place("power_plant", (30, 30), THEM);
    let mut snap = r.snap();
    snap.set_hp_for_test(mine, 100);
    snap.set_hp_for_test(enemy, 100);
    for mode in ToolMode::ALL {
        // An enemy structure, nothing under the cursor and a stale id.
        for entity in [Some(enemy), None, Some(EntityId::new(500, 0))] {
            assert_eq!(target(&snap, mode, entity, ME), Target::Nothing);
            assert_eq!(click(&snap, mode, entity, ME), None);
            assert_eq!(cursor(&snap, mode, entity, ME), CursorKind::NoGo);
        }
        assert_eq!(cursor(&snap, mode, Some(mine), ME), mode.cursor());
    }
    // A structure at 0 hit points is a wreck: neither mode acts on it.
    snap.set_hp_for_test(mine, 0);
    for mode in ToolMode::ALL {
        assert_eq!(target(&snap, mode, Some(mine), ME), Target::Nothing);
    }
}

#[test]
fn a_unit_is_no_target() {
    let mut r = rig();
    let kind = r.kind("rifleman");
    r.world
        .schedule(
            r.world.tick(),
            ME,
            Command::SpawnUnit {
                kind,
                pos: sf_math::Vec2::new(sf_math::Fx::from_int(20), sf_math::Fx::from_int(20)),
                owner: ME,
            },
        )
        .unwrap();
    r.world.step();
    let snap = r.snap();
    let unit = snap.views().iter().find(|v| v.kind == kind).unwrap().id;
    for mode in ToolMode::ALL {
        assert_eq!(target(&snap, mode, Some(unit), ME), Target::Nothing);
    }
}

#[test]
fn a_structure_going_up_or_being_sold_is_no_target_and_a_repairing_one_is_stopped() {
    let mut r = rig();
    let centre = r.place("construction_center", (20, 20), ME);
    let plant_kind = r.kind("power_plant");
    r.world
        .schedule(
            r.world.tick(),
            ME,
            Command::BuildStructure {
                kind: plant_kind,
                cell: Cell::new(26, 20),
            },
        )
        .unwrap();
    r.world.step();
    let going_up = r.world.occupant_at(Cell::new(26, 20)).unwrap();
    let sold = r.place("power_plant", (10, 10), ME);
    let repairing = r.place("power_plant", (10, 30), ME);
    // Sell one, and let the simulation start a repair on another (its hit points set in the world's
    // own column through a real damaged snapshot is not possible here: the sale is the real state).
    r.world
        .schedule(r.world.tick(), ME, Command::Sell { structure: sold })
        .unwrap();
    r.world.step();
    let mut snap = r.snap();
    assert!(snap.sale_of(sold).is_some());
    snap.set_hp_for_test(going_up, 10);
    for mode in ToolMode::ALL {
        assert_eq!(target(&snap, mode, Some(going_up), ME), Target::Nothing);
        assert_eq!(target(&snap, mode, Some(sold), ME), Target::Nothing);
    }
    // The anchor is a structure like the others and can be sold.
    assert_eq!(
        target(&snap, ToolMode::Sell, Some(centre), ME),
        Target::Sell
    );
    // A repair in progress is stopped by the next click (the state is the simulation's).
    snap.set_repair_for_test(repairing, sf_sim::RepairStatus::Repairing);
    snap.set_hp_for_test(repairing, 100);
    assert_eq!(
        target(&snap, ToolMode::Repair, Some(repairing), ME),
        Target::StopRepair
    );
    assert_eq!(
        click(&snap, ToolMode::Repair, Some(repairing), ME),
        Some(Command::Repair {
            structure: repairing,
            on: false
        })
    );
}

#[test]
fn what_a_click_sends_the_simulation_accepts() {
    // The click's command, scheduled in the world, is taken: a sale starts. (A repair of a hurt
    // structure is accepted in `sf_sim`'s tests, where hit points can be set.)
    let mut r = rig();
    let plant = r.place("power_plant", (10, 10), ME);
    let snap = r.snap();
    let command = click(&snap, ToolMode::Sell, Some(plant), ME).unwrap();
    r.world.schedule(r.world.tick(), ME, command).unwrap();
    r.world.step();
    assert!(r.world.sale_of(plant).is_some());
    assert!(r.world.repair_rejections().is_empty());
    // The player who is not the owner is refused by the simulation even if a click got through.
    let other = r.place("power_plant", (30, 30), THEM);
    r.world
        .schedule(r.world.tick(), ME, Command::Sell { structure: other })
        .unwrap();
    r.world.step();
    assert_eq!(r.world.repair_rejections().len(), 1);
    assert!(r.world.sale_of(other).is_none());
}
