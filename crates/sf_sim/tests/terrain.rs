//! Terrain types and occupancy per cell (WP2.1), through the public API.

mod common;

use common::{Scenario, run, script};
use sf_math::Vec2;
use sf_sim::{
    Cell, Command, ConfigError, EntityId, Kind, PlayerId, Terrain, TerrainDef, TerrainFlags,
    TerrainId, Tick, World, WorldConfig,
};

const GRASS: TerrainId = TerrainId(0);
const ROAD: TerrainId = TerrainId(1);
const WATER: TerrainId = TerrainId(2);

const BLOCKED: TerrainFlags = TerrainFlags {
    buildable: false,
    harvestable: false,
    ground: false,
};

fn defs() -> Vec<TerrainDef> {
    vec![
        TerrainDef {
            id: GRASS,
            flags: TerrainFlags::OPEN,
        },
        // A road: crossable but nothing may be built on it.
        TerrainDef {
            id: ROAD,
            flags: TerrainFlags {
                buildable: false,
                ..TerrainFlags::OPEN
            },
        },
        TerrainDef {
            id: WATER,
            flags: BLOCKED,
        },
    ]
}

/// A `w` x `h` layer: grass, with a road along row 1 and water in column 3.
fn layer(w: u32, h: u32) -> Vec<u8> {
    (0..h)
        .flat_map(|y| {
            (0..w).map(move |x| {
                if x == 3 {
                    WATER.0
                } else if y == 1 {
                    ROAD.0
                } else {
                    GRASS.0
                }
            })
        })
        .collect()
}

fn world(w: u32, h: u32) -> World {
    let terrain = Terrain::from_layer(w, h, &layer(w, h), &defs()).unwrap();
    World::with_terrain(WorldConfig::new(5).with_map_size(w, h), terrain).unwrap()
}

fn place(kind: u16, x: u32, y: u32, owner: u8) -> Command {
    Command::Place {
        kind: Kind(kind),
        cell: Cell::new(x, y),
        owner: PlayerId(owner),
    }
}

fn at(w: &mut World, tick: u64, player: u8, command: Command) {
    w.schedule(Tick(tick), PlayerId(player), command).unwrap();
}

// --- terrain queries ---

#[test]
fn the_world_answers_the_terrain_type_of_every_cell() {
    let w = world(8, 4);
    assert_eq!(w.terrain_at(Cell::new(0, 0)), Some(GRASS));
    assert_eq!(w.terrain_at(Cell::new(0, 1)), Some(ROAD));
    assert_eq!(w.terrain_at(Cell::new(3, 0)), Some(WATER));
    assert_eq!(
        w.terrain_at(Cell::new(3, 1)),
        Some(WATER),
        "water over road"
    );
    assert_eq!(w.terrain_at(Cell::new(7, 3)), Some(GRASS));
    assert_eq!(w.terrain_at(Cell::new(8, 0)), None);
    assert_eq!(w.terrain_at(Cell::new(0, 4)), None);
    for y in 0..4 {
        for x in 0..8 {
            let id = w.terrain_at(Cell::new(x, y)).unwrap();
            assert_eq!(id.0, layer(8, 4)[(y * 8 + x) as usize]);
            assert!(w.terrain().flags_at(Cell::new(x, y)).is_some());
        }
    }
    assert!(w.terrain().has_layer());
}

#[test]
fn a_world_without_a_layer_is_open_ground() {
    let w = World::new(WorldConfig::new(5).with_map_size(8, 4)).unwrap();
    assert!(!w.terrain().has_layer());
    assert_eq!(w.terrain_at(Cell::new(7, 3)), Some(GRASS));
    let flags = w.terrain().flags_at(Cell::new(2, 2)).unwrap();
    assert_eq!(flags, TerrainFlags::OPEN);
    assert_eq!(w.occupied_cells(), 0);
}

#[test]
fn a_layer_of_the_wrong_size_is_refused() {
    let terrain = Terrain::from_layer(8, 4, &layer(8, 4), &defs()).unwrap();
    let err = World::with_terrain(WorldConfig::new(5).with_map_size(8, 5), terrain).unwrap_err();
    assert_eq!(err, ConfigError::TerrainSize);
    assert!(err.to_string().contains("terrain"));
}

// --- Place and occupancy ---

#[test]
fn placing_on_free_buildable_ground_occupies_the_cell() {
    let mut w = world(8, 4);
    at(&mut w, 0, 0, place(9, 2, 0, 1));
    let r = w.step();
    assert_eq!((r.commands_executed, r.commands_ignored), (1, 0));
    let id = w.occupant_at(Cell::new(2, 0)).expect("occupied");
    assert_eq!(id, EntityId::new(0, 0));
    assert_eq!(w.cell_of(id), Some(Cell::new(2, 0)));
    assert_eq!(w.occupied_cells(), 1);
    assert!(w.is_occupied(Cell::new(2, 0)));
    assert!(!w.is_occupied(Cell::new(1, 0)));
    // The entity is a normal one: it stands in the middle of the cell.
    let view = w.get(id).unwrap();
    assert_eq!((view.kind, view.owner), (Kind(9), PlayerId(1)));
    assert_eq!(
        view.pos,
        Vec2::new(sf_math::Fx::from_ratio(5, 2), sf_math::Fx::HALF)
    );
    // And it stays there.
    for _ in 0..50 {
        w.step();
    }
    assert_eq!(w.get(id).unwrap().pos, view.pos);
    assert_eq!(w.occupant_at(Cell::new(2, 0)), Some(id));
}

#[test]
fn a_cell_that_cannot_be_built_on_stays_free() {
    let mut w = world(8, 4);
    at(&mut w, 0, 0, place(1, 0, 1, 0)); // road: not buildable
    at(&mut w, 0, 0, place(1, 3, 0, 0)); // water: not buildable
    at(&mut w, 0, 0, place(1, 8, 0, 0)); // outside the map
    at(&mut w, 0, 0, place(1, 0, 4, 0)); // outside the map
    let r = w.step();
    assert_eq!((r.commands_executed, r.commands_ignored), (0, 4));
    assert_eq!(w.occupied_cells(), 0);
    assert_eq!(w.len(), 0, "a refused placement creates nothing");
    for cell in [Cell::new(0, 1), Cell::new(3, 0), Cell::new(8, 0)] {
        assert_eq!(w.occupant_at(cell), None);
    }
}

#[test]
fn an_occupied_cell_cannot_be_placed_on_and_the_lower_player_wins_a_tie() {
    let mut w = world(8, 4);
    // Same tick, same cell: commands run by player, so player 0 gets it.
    at(&mut w, 0, 2, place(2, 5, 2, 2));
    at(&mut w, 0, 0, place(1, 5, 2, 0));
    let r = w.step();
    assert_eq!((r.commands_executed, r.commands_ignored), (1, 1));
    let id = w.occupant_at(Cell::new(5, 2)).unwrap();
    assert_eq!(w.get(id).unwrap().kind, Kind(1));
    // A later attempt fails too.
    at(&mut w, 1, 1, place(3, 5, 2, 1));
    let r = w.step();
    assert_eq!((r.commands_executed, r.commands_ignored), (0, 1));
    assert_eq!(w.occupant_at(Cell::new(5, 2)), Some(id));
    assert_eq!(w.len(), 1);
}

#[test]
fn despawning_a_structure_frees_its_cell_and_the_slot_comes_back_with_a_new_generation() {
    let mut w = world(8, 4);
    at(&mut w, 0, 0, place(1, 4, 0, 0));
    w.step();
    let first = w.occupant_at(Cell::new(4, 0)).unwrap();
    at(&mut w, 1, 0, Command::Despawn { id: first });
    w.step();
    assert_eq!(w.occupant_at(Cell::new(4, 0)), None);
    assert_eq!(w.occupied_cells(), 0);
    assert_eq!(w.cell_of(first), None, "a stale id occupies nothing");
    // The same cell can be taken again, by the freed slot at generation 1.
    at(&mut w, 2, 3, place(2, 4, 0, 3));
    w.step();
    let second = w.occupant_at(Cell::new(4, 0)).unwrap();
    assert_eq!(second, EntityId::new(first.index(), 1));
    assert_ne!(second, first);
    assert_eq!(w.cell_of(second), Some(Cell::new(4, 0)));
    // Despawning with the stale id changes nothing.
    at(&mut w, 3, 0, Command::Despawn { id: first });
    let r = w.step();
    assert_eq!(r.commands_ignored, 1);
    assert_eq!(w.occupant_at(Cell::new(4, 0)), Some(second));
}

#[test]
fn a_structure_does_not_move() {
    let mut w = world(8, 4);
    at(&mut w, 0, 0, place(1, 6, 3, 0));
    w.step();
    let id = w.occupant_at(Cell::new(6, 3)).unwrap();
    let pos = w.get(id).unwrap().pos;
    at(
        &mut w,
        1,
        0,
        Command::MoveTo {
            ids: vec![id],
            target: Vec2::from_ints(0, 0),
        },
    );
    at(
        &mut w,
        1,
        0,
        Command::SetWanderArea {
            ids: vec![id],
            area: sf_sim::Area::from_cells(0, 0, 8, 4),
        },
    );
    let r = w.step();
    assert_eq!((r.commands_executed, r.commands_ignored), (0, 2));
    for _ in 0..100 {
        w.step();
    }
    assert_eq!(w.get(id).unwrap().pos, pos);
    assert_eq!(w.cell_of(id), Some(Cell::new(6, 3)));
}

#[test]
fn occupancy_and_the_entity_columns_agree_after_a_busy_script() {
    // Places, refusals, despawns and re-places on a 16 x 16 map, with
    // wanderers around.
    let (w_cells, h_cells) = (16, 16);
    let terrain = Terrain::from_layer(w_cells, h_cells, &layer(w_cells, h_cells), &defs()).unwrap();
    let mut w =
        World::with_terrain(WorldConfig::new(9).with_map_size(w_cells, h_cells), terrain).unwrap();
    for i in 0..40 {
        at(
            &mut w,
            0,
            0,
            Command::Spawn {
                kind: Kind(i % 3),
                pos: Vec2::from_ints(i32::from(i) % 16, i32::from(i) / 3 % 16),
                owner: PlayerId(0),
            },
        );
    }
    let mut ids_to_drop = Vec::new();
    for k in 0..60_u32 {
        // Cells wander over water and road too: many are refused.
        at(
            &mut w,
            1 + u64::from(k / 20),
            (k % 3) as u8,
            place(5, (k * 7) % 16, (k * 5) % 16, 1),
        );
    }
    for _ in 0..5 {
        w.step();
    }
    for y in 0..h_cells {
        for x in 0..w_cells {
            if let Some(id) = w.occupant_at(Cell::new(x, y)) {
                ids_to_drop.push(id);
            }
        }
    }
    assert!(!ids_to_drop.is_empty() && ids_to_drop.len() < 60);
    // Every occupant is alive, stands on the cell it occupies and says so.
    let consistent = |w: &World| {
        let mut occupied = 0;
        for y in 0..h_cells {
            for x in 0..w_cells {
                let cell = Cell::new(x, y);
                if let Some(id) = w.occupant_at(cell) {
                    occupied += 1;
                    assert!(w.is_alive(id));
                    assert_eq!(w.cell_of(id), Some(cell));
                    assert_eq!(w.cell_at(w.get(id).unwrap().pos), Some(cell));
                    assert!(w.terrain().flags_at(cell).unwrap().buildable);
                }
            }
        }
        assert_eq!(occupied, w.occupied_cells());
        // And the other way round: only structures have a cell.
        let structures = w
            .snapshot()
            .iter()
            .filter(|v| w.cell_of(v.id).is_some())
            .count();
        assert_eq!(structures, occupied);
    };
    consistent(&w);
    let now = w.tick().0;
    for id in ids_to_drop.iter().step_by(2) {
        at(&mut w, now, 0, Command::Despawn { id: *id });
    }
    for k in 0..30_u32 {
        at(&mut w, now + 1, 1, place(6, (k * 3) % 16, (k * 11) % 16, 2));
    }
    for _ in 0..4 {
        w.step();
        consistent(&w);
    }
}

#[test]
fn cell_at_floors_a_position_and_rejects_outside() {
    let w = world(8, 4);
    assert_eq!(w.cell_at(Vec2::from_ints(0, 0)), Some(Cell::new(0, 0)));
    assert_eq!(
        w.cell_at(Vec2::new(
            sf_math::Fx::from_ratio(79, 10),
            sf_math::Fx::from_ratio(39, 10)
        )),
        Some(Cell::new(7, 3))
    );
    assert_eq!(w.cell_at(Vec2::from_ints(8, 0)), None);
    assert_eq!(w.cell_at(Vec2::from_ints(-1, 0)), None);
    assert_eq!(w.cell_at(Vec2::from_ints(0, 4)), None);
}

// --- wanderers and the checksum ---

/// The terrain layer must not change what wanderers do: the same script gives
/// the same positions on a world without a layer and on one full of water,
/// tick by tick. Only the checksum differs, because the terrain is in it.
#[test]
fn terrain_does_not_change_wanderers() {
    let s = Scenario::small();
    let cmds = script(&s);
    let mut plain = World::new(WorldConfig::new(s.seed)).unwrap();
    let water_everywhere = Terrain::from_layer(
        256,
        256,
        &vec![WATER.0; 256 * 256],
        &[TerrainDef {
            id: WATER,
            flags: BLOCKED,
        }],
    )
    .unwrap();
    let mut wet = World::with_terrain(WorldConfig::new(s.seed), water_everywhere).unwrap();
    for (tick, player, cmd) in &cmds {
        plain.schedule(*tick, *player, cmd.clone()).unwrap();
        wet.schedule(*tick, *player, cmd.clone()).unwrap();
    }
    for t in 0..300 {
        plain.step();
        wet.step();
        let a: Vec<_> = plain.snapshot().iter().collect();
        let b: Vec<_> = wet.snapshot().iter().collect();
        assert_eq!(a, b, "tick {t}");
    }
    assert_ne!(plain.checksum(), wet.checksum());
    // The same wanderers keep the same checksums apart from the terrain: two
    // worlds on the same layer agree with each other tick for tick.
    let mut again = World::with_terrain(
        WorldConfig::new(s.seed),
        Terrain::from_layer(
            256,
            256,
            &vec![WATER.0; 256 * 256],
            &[TerrainDef {
                id: WATER,
                flags: BLOCKED,
            }],
        )
        .unwrap(),
    )
    .unwrap();
    for (tick, player, cmd) in &cmds {
        again.schedule(*tick, *player, cmd.clone()).unwrap();
    }
    let ticks = run(&mut again, 300);
    assert_eq!(ticks, wet.checksum_history());
}

#[test]
fn the_checksum_covers_the_layer_and_the_occupancy() {
    let build = |layer_id: u8, placements: &[(u32, u32)]| {
        let mut ids = layer(8, 4);
        ids[0] = layer_id;
        let terrain = Terrain::from_layer(8, 4, &ids, &defs()).unwrap();
        let mut w = World::with_terrain(WorldConfig::new(5).with_map_size(8, 4), terrain).unwrap();
        for &(x, y) in placements {
            at(&mut w, 0, 0, place(1, x, y, 0));
        }
        w.step();
        w.checksum()
    };
    let base = build(0, &[(5, 2)]);
    assert_eq!(base, build(0, &[(5, 2)]), "deterministic");
    assert_ne!(
        base,
        build(1, &[(5, 2)]),
        "another terrain type in one cell"
    );
    assert_ne!(base, build(0, &[(6, 2)]), "another cell occupied");
    assert_ne!(base, build(0, &[(5, 2), (6, 2)]), "one more occupant");
    // Two structures: the order they were placed in decides their slots, so
    // swapping them changes the state.
    assert_ne!(build(0, &[(5, 2), (6, 2)]), build(0, &[(6, 2), (5, 2)]));
}

#[test]
fn a_clone_of_a_world_with_structures_stays_in_step() {
    let mut a = world(8, 4);
    at(&mut a, 0, 0, place(1, 5, 2, 0));
    at(&mut a, 0, 0, place(1, 6, 2, 0));
    a.step();
    let mut b = a.clone();
    assert_eq!(a, b);
    let (id_a, id_b) = (
        a.occupant_at(Cell::new(5, 2)).unwrap(),
        b.occupant_at(Cell::new(5, 2)).unwrap(),
    );
    at(&mut a, 1, 0, Command::Despawn { id: id_a });
    at(&mut b, 1, 0, Command::Despawn { id: id_b });
    for _ in 0..20 {
        assert_eq!(a.step().checksum, b.step().checksum);
    }
    assert_eq!(a, b);
}
