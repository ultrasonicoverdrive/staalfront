//! The `terrain_256` scenario (WP2.1, phase 2 "done when"): a 256 by 256 cell
//! playfield loads its terrain layer, structures are placed on it, and the
//! terrain type and the occupancy of the cells are asserted.
//!
//! The terrain types are those of `assets/config/terrain_types.ron` (ids 0 to
//! 5: grass, road, water, rock, ore, bridge); the layer is
//! `assets/maps/playfield256/terrain.png`.

mod common;

use common::scenario_path;
use sf_data::map::Map;
use sf_data::map::generate::generate_cells;
use sf_data::terrain::TerrainTypes;
use sf_headless::scenario::{Scenario, ScenarioError};
use sf_sim::{Cell, EntityId, Kind, PlayerId, TerrainId, World};

fn id_of(name: &str) -> TerrainId {
    TerrainId(TerrainTypes::embedded().id_of(name).unwrap())
}

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("terrain_256.ron")).unwrap()
}

/// The world at tick 0, before anything ran, and the same world after `ticks`.
fn after(ticks: u64) -> World {
    let mut world = scenario().build_world().unwrap();
    for _ in 0..ticks {
        world.step();
    }
    world
}

fn cell(x: u32, y: u32) -> Cell {
    Cell::new(x, y)
}

/// The cells the scenario names, with the type each must have.
const NAMED: [((u32, u32), &str); 11] = [
    ((120, 126), "grass"),
    ((121, 126), "grass"),
    ((126, 124), "grass"),
    ((130, 125), "grass"),
    ((125, 126), "road"),
    ((126, 126), "bridge"),
    ((126, 127), "water"),
    ((47, 0), "rock"),
    ((199, 0), "ore"),
    ((0, 51), "road"),
    ((255, 255), "grass"),
];

#[test]
fn the_scenario_is_a_256_by_256_cell_playfield_with_a_terrain_layer() {
    let s = scenario();
    assert_eq!((s.map.width, s.map.height), (256, 256));
    assert_eq!(s.map_name.as_deref(), Some("playfield256"));
    let terrain = s.terrain.as_ref().expect("the layer was read from the map");
    assert!(terrain.has_layer());
    assert_eq!((terrain.width(), terrain.height()), (256, 256));
    let world = s.build_world().unwrap();
    assert_eq!(world.map_size(), (256, 256));
    assert_eq!(world.terrain().ids().len(), 65_536);
}

#[test]
fn every_cell_has_the_terrain_type_of_the_map_file() {
    let world = after(0);
    let file = Map::load(&scenario_path("../maps/playfield256")).unwrap();
    let layer = file.terrain().unwrap();
    // The playfield is the generator's layer for seed 1.
    assert_eq!(layer, generate_cells(256, 256, 1).terrain().unwrap());
    for y in 0..256 {
        for x in 0..256 {
            assert_eq!(
                world.terrain_at(cell(x, y)),
                layer.get(x, y).map(TerrainId),
                "cell ({x}, {y})"
            );
        }
    }
    assert_eq!(world.terrain_at(cell(256, 0)), None);
    // All six types occur, and nothing else.
    let h = layer.histogram();
    assert!(h[..6].iter().all(|&n| n > 0), "{:?}", &h[..6]);
    assert_eq!(h[..6].iter().sum::<u32>(), 65_536);
}

#[test]
fn the_named_cells_have_the_types_the_scenario_relies_on() {
    let world = after(0);
    for ((x, y), name) in NAMED {
        assert_eq!(
            world.terrain_at(cell(x, y)),
            Some(id_of(name)),
            "({x}, {y})"
        );
    }
    // What the types allow: the placements are refused or accepted by these.
    let buildable = |x, y| world.terrain().flags_at(cell(x, y)).unwrap().buildable;
    assert!(buildable(120, 126));
    for (x, y) in [(126, 126), (126, 127), (125, 126), (47, 0), (199, 0)] {
        assert!(!buildable(x, y), "({x}, {y})");
    }
    // Water and rock are not open ground for the ground classes, a bridge and
    // a road are: the `ground` summary of the terrain, derived from the movement
    // costs (docs/rules/movement.md). It is also what keeps this scenario's
    // terrain digest, and so its checksum, what it was before WP4.1.
    let flags = |name: &str| world.terrain().flags_of(id_of(name)).unwrap();
    for blocked in ["water", "rock"] {
        assert!(!flags(blocked).ground, "{blocked}");
    }
    for open in ["road", "bridge", "grass", "ore"] {
        assert!(flags(open).ground, "{open}");
    }
    assert!(flags("ore").harvestable && !flags("grass").harvestable);
}

#[test]
fn no_cell_is_occupied_before_the_structures_are_placed() {
    let world = after(0);
    assert_eq!(world.occupied_cells(), 0);
    // Tick 0 spawns the wanderers: they occupy nothing.
    let world = after(1);
    assert_eq!(world.len(), 300);
    assert_eq!(world.occupied_cells(), 0);
}

#[test]
fn placed_structures_occupy_their_cells_and_refused_ones_do_not() {
    let world = after(2); // the placements ran at tick 1
    // Four stand, in execution order.
    let standing = [
        ((120, 126), 300, 10, 0),
        ((121, 126), 301, 11, 1),
        ((126, 124), 302, 10, 0),
        ((130, 125), 303, 12, 2),
    ];
    for ((x, y), slot, kind, owner) in standing {
        let id = world.occupant_at(cell(x, y)).unwrap_or_else(|| {
            panic!("({x}, {y}) should be occupied");
        });
        assert_eq!(id, EntityId::new(slot, 0), "({x}, {y})");
        let view = world.get(id).unwrap();
        assert_eq!((view.kind, view.owner), (Kind(kind), PlayerId(owner)));
        assert_eq!(world.cell_of(id), Some(cell(x, y)));
        assert_eq!(world.cell_at(view.pos), Some(cell(x, y)));
    }
    assert_eq!(world.occupied_cells(), 4);
    // The six refused ones left nothing behind: not on the bridge, the water,
    // the road, the rock or the ore, and the second one on (120, 126) did not
    // replace the first.
    for (x, y) in [(126, 126), (126, 127), (125, 126), (47, 0), (199, 0)] {
        assert_eq!(world.occupant_at(cell(x, y)), None, "({x}, {y})");
    }
    assert_eq!(world.get(EntityId::new(300, 0)).unwrap().kind, Kind(10));
    assert_eq!(world.len(), 304);
    // Every other cell is free: 4 of 65,536 are occupied.
    let occupied = (0..256)
        .flat_map(|y| (0..256).map(move |x| cell(x, y)))
        .filter(|&c| world.is_occupied(c))
        .count();
    assert_eq!(occupied, 4);
}

#[test]
fn a_freed_cell_is_taken_again_by_the_reused_slot() {
    // Tick 100 despawns the structure on (121, 126).
    let world = after(101);
    assert_eq!(world.occupant_at(cell(121, 126)), None);
    assert_eq!(world.occupied_cells(), 3);
    assert!(!world.is_alive(EntityId::new(301, 0)));
    // The neighbours are untouched.
    assert_eq!(
        world.occupant_at(cell(120, 126)),
        Some(EntityId::new(300, 0))
    );

    // Tick 150: player 3 places on the freed cell; the freed slot comes back.
    let world = after(151);
    let id = world.occupant_at(cell(121, 126)).unwrap();
    assert_eq!(id, EntityId::new(301, 1));
    assert_eq!(world.get(id).unwrap().kind, Kind(13));
    assert_eq!(world.get(id).unwrap().owner, PlayerId(3));
    assert_eq!(world.occupied_cells(), 4);
    // The old id names nothing any more.
    assert_eq!(world.get(EntityId::new(301, 0)), None);
    assert_eq!(world.cell_of(EntityId::new(301, 0)), None);
}

#[test]
fn the_final_occupancy_and_the_command_counts() {
    let mut world = scenario().build_world().unwrap();
    let (mut executed, mut ignored) = (0, 0);
    for _ in 0..400 {
        let r = world.step();
        executed += r.commands_executed;
        ignored += r.commands_ignored;
    }
    // 300 spawns, 4 placements, 1 despawn, 1 placement.
    assert_eq!(executed, 306);
    // 6 refused placements at tick 1, the move order to a structure at tick
    // 200, and the placement on the occupied cell at tick 250.
    assert_eq!(ignored, 8);
    assert_eq!(world.len(), 304);
    assert_eq!(world.occupied_cells(), 4);
    let expect = [
        ((120, 126), EntityId::new(300, 0)),
        ((121, 126), EntityId::new(301, 1)),
        ((126, 124), EntityId::new(302, 0)),
        ((130, 125), EntityId::new(303, 0)),
    ];
    for ((x, y), id) in expect {
        assert_eq!(world.occupant_at(cell(x, y)), Some(id), "({x}, {y})");
    }
    // The structure that was told to move is where it was put (the middle of
    // its cell).
    let view = world.get(EntityId::new(300, 0)).unwrap();
    assert_eq!(
        view.pos,
        sf_math::Vec2::new(
            sf_math::Fx::from_int(120) + sf_math::Fx::HALF,
            sf_math::Fx::from_int(126) + sf_math::Fx::HALF,
        )
    );
    // The wanderers were not stopped by the terrain: they have moved around
    // (some stand on water).
    let on_water = world
        .snapshot()
        .iter()
        .filter(|v| world.cell_of(v.id).is_none())
        .filter(|v| {
            world
                .cell_at(v.pos)
                .and_then(|c| world.terrain_at(c))
                .is_some_and(|t| t == id_of("water") || t == id_of("rock"))
        })
        .count();
    assert!(on_water > 0, "wanderers ignore terrain until WP4.1");
}

/// The same placements on a world without a terrain layer: everything is open
/// ground, so the cells the layer refuses are accepted.
#[test]
fn without_a_layer_every_cell_accepts_a_structure() {
    let mut s = scenario();
    s.map_name = None;
    s.terrain = None;
    let mut world = s.build_world().unwrap();
    assert!(!world.terrain().has_layer());
    for _ in 0..2 {
        world.step();
    }
    // 5 refused placements are now accepted; only the one on a taken cell is not.
    assert_eq!(world.occupied_cells(), 9);
    for (x, y) in [(126, 126), (126, 127), (125, 126), (47, 0), (199, 0)] {
        assert!(world.is_occupied(cell(x, y)), "({x}, {y})");
    }
}

#[test]
fn a_scenario_that_names_a_map_cannot_be_built_from_text_alone() {
    let text = std::fs::read_to_string(scenario_path("terrain_256.ron")).unwrap();
    let s = Scenario::from_ron_str(&text).unwrap();
    assert!(s.terrain.is_none());
    let err = s.build_world().unwrap_err();
    assert!(
        matches!(err, ScenarioError::Invalid { .. }) && err.to_string().contains("Scenario::load"),
        "{err}"
    );
}
