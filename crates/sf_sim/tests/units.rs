//! Units and structures from the unit table (WP3.2), through the public API:
//! stats at birth, footprints in the occupancy grid, refusals, per-unit speed,
//! and what the legacy `Spawn` and `Place` still do.

use sf_math::{Fx, Vec2};
use sf_sim::{
    Cell, Command, ConfigError, EntityId, Footprint, Kind, PlayerId, Terrain, TerrainDef,
    TerrainFlags, TerrainId, Tick, UnitClass, UnitStats, UnitTable, World, WorldConfig, per_tick,
};

const GRASS: TerrainId = TerrainId(0);
const ROAD: TerrainId = TerrainId(1);
const WATER: TerrainId = TerrainId(2);

const MAP: (u32, u32) = (16, 12);

// Kinds of this test. They are on purpose also kinds a legacy wanderer uses.
const RIFLEMAN: Kind = Kind(1);
const TANK: Kind = Kind(2);
const HOUSE: Kind = Kind(3); // structure 2 x 3
const TOWER: Kind = Kind(4); // structure 1 x 1
const CRATE: Kind = Kind(5); // object with hit points
const ORE: Kind = Kind(6); // object that cannot be damaged
const UNKNOWN: Kind = Kind(99);

fn table() -> UnitTable {
    let mobile = |kind: Kind, hp, class, speed_per_second: i64, turn, size_tenths: i64| UnitStats {
        kind,
        hp,
        movement_class: Some(class),
        speed_per_tick: per_tick(Fx::from_ratio(speed_per_second, 10)),
        turn_per_tick: turn,
        size: Fx::from_ratio(size_tenths, 10),
        footprint: None,
        weapon_range: Fx::from_int(4),
        turret_per_tick: 0,
    };
    let structure = |kind: Kind, hp, w, h| UnitStats {
        kind,
        hp,
        movement_class: None,
        speed_per_tick: Fx::ZERO,
        turn_per_tick: 0,
        size: Fx::ZERO,
        footprint: Some(Footprint {
            width: w,
            height: h,
        }),
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    };
    let object = |kind: Kind, hp| UnitStats {
        footprint: None,
        ..structure(kind, hp, 1, 1)
    };
    UnitTable::new(vec![
        mobile(RIFLEMAN, 50, 7, 7, 16, 4),
        mobile(TANK, 400, 3, 20, 10, 12),
        structure(HOUSE, 700, 2, 3),
        structure(TOWER, 600, 1, 1),
        object(CRATE, 100),
        object(ORE, 0),
    ])
    .unwrap()
}

fn defs() -> Vec<TerrainDef> {
    let blocked = TerrainFlags {
        buildable: false,
        harvestable: false,
        ground: false,
    };
    vec![
        TerrainDef {
            id: GRASS,
            flags: TerrainFlags::OPEN,
        },
        TerrainDef {
            id: ROAD,
            flags: TerrainFlags {
                buildable: false,
                ..TerrainFlags::OPEN
            },
        },
        TerrainDef {
            id: WATER,
            flags: blocked,
        },
    ]
}

/// Grass, with a road along row 9, a water cell at (10, 3) and a water cell at
/// (13, 6).
fn layer() -> Vec<u8> {
    (0..MAP.1)
        .flat_map(|y| {
            (0..MAP.0).map(move |x| match (x, y) {
                (10, 3) | (13, 6) => WATER.0,
                (_, 9) => ROAD.0,
                _ => GRASS.0,
            })
        })
        .collect()
}

fn world() -> World {
    let terrain = Terrain::from_layer(MAP.0, MAP.1, &layer(), &defs()).unwrap();
    World::with_terrain(WorldConfig::new(7).with_map_size(MAP.0, MAP.1), terrain)
        .unwrap()
        .with_units(table())
        .unwrap()
}

fn at(w: &mut World, tick: u64, player: u8, command: Command) {
    w.schedule(Tick(tick), PlayerId(player), command).unwrap();
}

fn unit(kind: Kind, x: i32, y: i32, owner: u8) -> Command {
    Command::SpawnUnit {
        kind,
        pos: Vec2::from_ints(x, y),
        owner: PlayerId(owner),
    }
}

fn structure(kind: Kind, x: u32, y: u32, owner: u8) -> Command {
    Command::PlaceStructure {
        kind,
        cell: Cell::new(x, y),
        owner: PlayerId(owner),
    }
}

/// Runs one tick with a single command and returns `(executed, ignored)`.
fn one(w: &mut World, command: Command) -> (u32, u32) {
    let tick = w.tick();
    at(w, tick.0, 0, command);
    let r = w.step();
    (r.commands_executed, r.commands_ignored)
}

fn cells_of_footprint(x: u32, y: u32, fp: Footprint) -> Vec<Cell> {
    let mut out = Vec::new();
    for dy in 0..u32::from(fp.height) {
        for dx in 0..u32::from(fp.width) {
            out.push(Cell::new(x + dx, y + dy));
        }
    }
    out
}

// --- the table ---

#[test]
fn the_table_is_part_of_how_a_world_is_made() {
    let mut w = world();
    assert_eq!(w.unit_table().len(), 6);
    let late = w.clone();
    one(&mut w, unit(RIFLEMAN, 1, 1, 0));
    // After a tick, or with a command pending, it is too late.
    assert_eq!(
        w.clone().with_units(table()).unwrap_err(),
        ConfigError::UnitsAfterStart
    );
    let mut pending = late;
    at(&mut pending, 5, 0, unit(RIFLEMAN, 1, 1, 0));
    assert_eq!(
        pending.with_units(table()).unwrap_err(),
        ConfigError::UnitsAfterStart
    );
    assert!(
        ConfigError::UnitsAfterStart
            .to_string()
            .contains("unit table")
    );
}

#[test]
fn stats_are_looked_up_by_kind_and_classified() {
    let w = world();
    let t = w.unit_table();
    assert_eq!(t.get(RIFLEMAN).unwrap().class(), UnitClass::Mobile);
    assert_eq!(t.get(HOUSE).unwrap().class(), UnitClass::Structure);
    assert_eq!(t.get(CRATE).unwrap().class(), UnitClass::Object);
    assert!(t.get(UNKNOWN).is_none());
}

// --- SpawnUnit ---

#[test]
fn a_unit_takes_its_stats_from_the_table() {
    let mut w = world();
    assert_eq!(one(&mut w, unit(TANK, 4, 5, 2)), (1, 0));
    let id = EntityId::new(0, 0);
    let view = w.get(id).unwrap();
    assert_eq!((view.kind, view.owner), (TANK, PlayerId(2)));
    assert!(view.has_stats);
    assert_eq!(view.hp, 400);
    assert_eq!(w.hp_of(id), Some(400));
    let stats = w.stats_of(id).unwrap();
    assert_eq!(stats.kind, TANK);
    assert_eq!(stats.movement_class, Some(3));
    assert_eq!(stats.turn_per_tick, 10);
    assert_eq!(stats.size, Fx::from_ratio(12, 10));
    assert_eq!(stats.speed_per_tick, per_tick(Fx::from_int(2)));
    assert_eq!(stats.footprint, None);
    // It stands where it was told.
    assert_eq!(view.pos, Vec2::from_ints(4, 5));
}

#[test]
fn a_unit_occupies_no_cell_and_stands_idle() {
    let mut w = world();
    // Three units on one cell are fine: collision is phase 4.
    for kind in [RIFLEMAN, RIFLEMAN, TANK] {
        assert_eq!(one(&mut w, unit(kind, 4, 5, 0)), (1, 0));
    }
    assert_eq!(w.occupied_cells(), 0);
    assert!(!w.is_occupied(Cell::new(4, 5)));
    assert_eq!(w.cell_of(EntityId::new(0, 0)), None);
    assert_eq!(w.footprint_of(EntityId::new(0, 0)), None);
    for _ in 0..200 {
        w.step();
    }
    // Idle: no wandering.
    for i in 0..3 {
        let v = w.get(EntityId::new(i, 0)).unwrap();
        assert_eq!(v.pos, Vec2::from_ints(4, 5));
        assert_eq!(v.pos, v.prev_pos);
    }
    assert_eq!(w.occupied_cells(), 0);
}

#[test]
fn an_object_is_spawned_like_a_unit_and_never_moves() {
    let mut w = world();
    assert_eq!(one(&mut w, unit(CRATE, 6, 6, 0)), (1, 0));
    assert_eq!(one(&mut w, unit(ORE, 7, 6, 0)), (1, 0));
    assert_eq!(w.hp_of(EntityId::new(0, 0)), Some(100));
    // hp zero: it cannot be damaged, and it is still a table entity.
    assert_eq!(w.hp_of(EntityId::new(1, 0)), Some(0));
    assert_eq!(w.occupied_cells(), 0);
    // Orders to move or wander change nothing.
    let ids = vec![EntityId::new(0, 0), EntityId::new(1, 0)];
    let moved = one(
        &mut w,
        Command::MoveTo {
            ids: ids.clone(),
            target: Vec2::from_ints(1, 1),
        },
    );
    assert_eq!(moved, (0, 1));
    let wander = one(
        &mut w,
        Command::SetWanderArea {
            ids,
            area: sf_sim::Area::from_cells(0, 0, 16, 12),
        },
    );
    assert_eq!(wander, (0, 1));
    for _ in 0..100 {
        w.step();
    }
    assert_eq!(
        w.get(EntityId::new(0, 0)).unwrap().pos,
        Vec2::from_ints(6, 6)
    );
    assert_eq!(
        w.get(EntityId::new(1, 0)).unwrap().pos,
        Vec2::from_ints(7, 6)
    );
}

#[test]
fn spawning_refuses_what_it_cannot_make() {
    let mut w = world();
    // A kind the table does not define.
    assert_eq!(one(&mut w, unit(UNKNOWN, 1, 1, 0)), (0, 1));
    // A structure goes through PlaceStructure.
    assert_eq!(one(&mut w, unit(HOUSE, 1, 1, 0)), (0, 1));
    // Outside the map: the far edge is exclusive, and so is below zero.
    assert_eq!(one(&mut w, unit(RIFLEMAN, 16, 1, 0)), (0, 1));
    assert_eq!(one(&mut w, unit(RIFLEMAN, 1, 12, 0)), (0, 1));
    assert_eq!(one(&mut w, unit(RIFLEMAN, -1, 1, 0)), (0, 1));
    // The last cell is fine.
    assert_eq!(one(&mut w, unit(RIFLEMAN, 15, 11, 0)), (1, 0));
    // Nothing else was created and no slot was spent on a refusal.
    assert_eq!(w.len(), 1);
    assert_eq!(w.slot_count(), 1);
    // A cell a structure occupies is not a place for a unit to appear.
    assert_eq!(one(&mut w, structure(HOUSE, 2, 2, 0)), (1, 0));
    assert_eq!(one(&mut w, unit(RIFLEMAN, 3, 4, 0)), (0, 1));
    assert_eq!(one(&mut w, unit(RIFLEMAN, 4, 2, 0)), (1, 0));
}

#[test]
fn a_unit_may_appear_on_any_terrain_until_movement_classes_exist() {
    // Terrain costs per movement class are WP4.1; today only the map edge and
    // structures stop a spawn. Written down so that WP4.1 changes it on purpose.
    let mut w = world();
    assert_eq!(one(&mut w, unit(RIFLEMAN, 10, 3, 0)), (1, 0));
    assert_eq!(w.terrain_at(Cell::new(10, 3)), Some(WATER));
}

#[test]
fn a_despawned_slot_comes_back_for_a_unit_with_a_new_generation() {
    let mut w = world();
    one(&mut w, unit(RIFLEMAN, 1, 1, 0));
    let id = EntityId::new(0, 0);
    assert_eq!(one(&mut w, Command::Despawn { id }), (1, 0));
    assert_eq!(one(&mut w, unit(TANK, 2, 2, 1)), (1, 0));
    let again = EntityId::new(0, 1);
    assert_eq!(w.get(again).unwrap().kind, TANK);
    assert!(w.get(id).is_none());
    assert_eq!(w.hp_of(id), None);
    assert_eq!(w.hp_of(again), Some(400));
}

// --- PlaceStructure ---

#[test]
fn a_structure_occupies_every_cell_of_its_footprint() {
    let mut w = world();
    assert_eq!(one(&mut w, structure(HOUSE, 3, 2, 1)), (1, 0));
    let id = EntityId::new(0, 0);
    let fp = Footprint {
        width: 2,
        height: 3,
    };
    assert_eq!(w.footprint_of(id), Some(fp));
    assert_eq!(w.cell_of(id), Some(Cell::new(3, 2)), "the anchor");
    assert_eq!(w.occupied_cells(), 6);
    for cell in cells_of_footprint(3, 2, fp) {
        assert_eq!(w.occupant_at(cell), Some(id), "cell {cell}");
    }
    // The cells around it are free.
    for cell in [
        Cell::new(2, 2),
        Cell::new(5, 2),
        Cell::new(3, 1),
        Cell::new(3, 5),
        Cell::new(4, 5),
    ] {
        assert!(!w.is_occupied(cell), "cell {cell}");
    }
    // Stands in the middle of the footprint: the anchor corner plus half its
    // size, facing zero, with the hit points of the table.
    let view = w.get(id).unwrap();
    assert_eq!(
        view.pos,
        Vec2::new(
            Fx::from_int(3) + Fx::from_int(1),
            Fx::from_int(2) + Fx::from_ratio(3, 2)
        )
    );
    assert_eq!(view.facing.raw(), 0);
    assert_eq!((view.hp, view.has_stats), (700, true));
    assert_eq!(view.owner, PlayerId(1));
}

#[test]
fn a_one_cell_structure_stands_in_the_middle_of_its_cell() {
    let mut w = world();
    assert_eq!(one(&mut w, structure(TOWER, 5, 4, 0)), (1, 0));
    let id = EntityId::new(0, 0);
    assert_eq!(
        w.get(id).unwrap().pos,
        Vec2::new(Fx::from_ratio(11, 2), Fx::from_ratio(9, 2))
    );
    assert_eq!(w.occupied_cells(), 1);
    assert_eq!(
        w.footprint_of(id),
        Some(Footprint {
            width: 1,
            height: 1
        })
    );
}

#[test]
fn a_placement_is_all_or_nothing() {
    let mut w = world();
    // 2 x 3 on (9, 2): the cells (9..11, 2..5) include the water at (10, 3).
    assert_eq!(one(&mut w, structure(HOUSE, 9, 2, 0)), (0, 1));
    // On the road (row 9): not buildable.
    assert_eq!(
        one(&mut w, structure(HOUSE, 5, 8, 0)),
        (0, 1),
        "row 9 is road"
    );
    // Off the east edge, off the south edge.
    assert_eq!(one(&mut w, structure(HOUSE, 15, 1, 0)), (0, 1));
    assert_eq!(one(&mut w, structure(HOUSE, 1, 10, 0)), (0, 1));
    // The anchor outside the map.
    assert_eq!(one(&mut w, structure(HOUSE, 16, 0, 0)), (0, 1));
    assert_eq!(one(&mut w, structure(HOUSE, 0, 12, 0)), (0, 1));
    // Nothing was made, claimed or spent.
    assert_eq!((w.len(), w.slot_count(), w.occupied_cells()), (0, 0, 0));
    // A unknown kind and a kind that is not a structure.
    assert_eq!(one(&mut w, structure(UNKNOWN, 1, 1, 0)), (0, 1));
    assert_eq!(one(&mut w, structure(RIFLEMAN, 1, 1, 0)), (0, 1));
    assert_eq!(one(&mut w, structure(CRATE, 1, 1, 0)), (0, 1));
    assert_eq!((w.len(), w.occupied_cells()), (0, 0));
    // The same footprint on free grass stands.
    assert_eq!(one(&mut w, structure(HOUSE, 11, 3, 0)), (1, 0));
}

#[test]
fn an_overlapping_placement_is_refused_and_leaves_the_first_alone() {
    let mut w = world();
    assert_eq!(one(&mut w, structure(HOUSE, 3, 2, 0)), (1, 0));
    let first = EntityId::new(0, 0);
    // Overlaps by one cell in each direction.
    for (x, y) in [(2, 1), (4, 4), (4, 2), (3, 4), (2, 3)] {
        assert_eq!(one(&mut w, structure(HOUSE, x, y, 1)), (0, 1), "({x}, {y})");
    }
    // The 1 x 1 tower at a corner that is covered is refused; free ones are not.
    assert_eq!(one(&mut w, structure(TOWER, 4, 4, 1)), (0, 1));
    assert_eq!(one(&mut w, structure(TOWER, 5, 4, 1)), (1, 0));
    assert_eq!(w.occupant_at(Cell::new(4, 4)), Some(first));
    assert_eq!(w.get(first).unwrap().owner, PlayerId(0));
}

#[test]
fn two_placements_in_one_tick_go_to_the_lower_player() {
    let mut w = world();
    at(&mut w, 0, 1, structure(HOUSE, 4, 3, 1));
    at(&mut w, 0, 0, structure(TOWER, 5, 4, 0));
    let r = w.step();
    assert_eq!((r.commands_executed, r.commands_ignored), (1, 1));
    assert_eq!(w.get(EntityId::new(0, 0)).unwrap().kind, TOWER);
    assert_eq!(w.occupied_cells(), 1);
}

#[test]
fn despawning_a_structure_frees_all_its_cells() {
    let mut w = world();
    one(&mut w, structure(HOUSE, 3, 2, 0));
    one(&mut w, structure(TOWER, 8, 8, 0));
    assert_eq!(w.occupied_cells(), 7);
    let house = EntityId::new(0, 0);
    assert_eq!(one(&mut w, Command::Despawn { id: house }), (1, 0));
    assert_eq!(w.occupied_cells(), 1, "only the tower is left");
    for cell in cells_of_footprint(
        3,
        2,
        Footprint {
            width: 2,
            height: 3,
        },
    ) {
        assert!(!w.is_occupied(cell), "cell {cell}");
    }
    assert!(w.is_occupied(Cell::new(8, 8)));
    // The cells can be built on again, with the slot back at generation 1.
    assert_eq!(one(&mut w, structure(HOUSE, 4, 2, 1)), (1, 0));
    let again = EntityId::new(0, 1);
    assert_eq!(w.occupant_at(Cell::new(5, 4)), Some(again));
    assert_eq!(w.get(again).unwrap().owner, PlayerId(1));
}

#[test]
fn a_structure_cannot_be_ordered_to_move() {
    let mut w = world();
    one(&mut w, structure(HOUSE, 3, 2, 0));
    let id = EntityId::new(0, 0);
    let before = w.get(id).unwrap().pos;
    let r = one(
        &mut w,
        Command::MoveTo {
            ids: vec![id],
            target: Vec2::from_ints(0, 0),
        },
    );
    assert_eq!(r, (0, 1));
    for _ in 0..100 {
        w.step();
    }
    assert_eq!(w.get(id).unwrap().pos, before);
}

// --- movement by the unit's own speed ---

#[test]
fn a_unit_moves_by_its_own_speed_and_turn_rate() {
    let mut w = world();
    // The slow rifleman and the fast tank to the same target, side by side.
    one(&mut w, unit(RIFLEMAN, 1, 1, 0));
    one(&mut w, unit(TANK, 1, 5, 0));
    let (r, t) = (EntityId::new(0, 0), EntityId::new(1, 0));
    let target = Vec2::from_ints(14, 3);
    one(
        &mut w,
        Command::MoveTo {
            ids: vec![r, t],
            target,
        },
    );
    let step_of = |w: &World, id| w.stats_of(id).unwrap().speed_per_tick;
    let (rifle_step, tank_step) = (step_of(&w, r), step_of(&w, t));
    assert!(tank_step.raw() > 2 * rifle_step.raw());
    let mut arrived = [None, None];
    for tick in 0..2000_u64 {
        w.step();
        for (n, id) in [r, t].into_iter().enumerate() {
            let v = w.get(id).unwrap();
            // Never faster than the unit's own speed per tick.
            let limit = step_of(&w, id) + Fx::from_raw(2);
            assert!((v.pos - v.prev_pos).length_squared() <= limit.square());
            if arrived[n].is_none() && v.pos == target {
                arrived[n] = Some(tick);
            }
        }
    }
    let (ra, ta) = (arrived[0].unwrap(), arrived[1].unwrap());
    assert!(
        ta < ra,
        "the tank ({ta}) arrives before the rifleman ({ra})"
    );
    // About the distance over the speed, plus the time it took to turn.
    let distance = (target - Vec2::from_ints(1, 1)).length_squared().sqrt();
    let expected = distance.raw() / rifle_step.raw();
    assert!(
        (ra as i64 - expected).abs() <= 40,
        "rifleman took {ra} ticks, {expected} expected"
    );
}

#[test]
fn a_wanderer_of_the_legacy_spawn_keeps_the_world_speed() {
    let mut w = world();
    // Kind 1 is also the rifleman's number: the legacy Spawn ignores the table.
    one(
        &mut w,
        Command::Spawn {
            kind: Kind(1),
            pos: Vec2::from_ints(8, 6),
            owner: PlayerId(0),
        },
    );
    let id = EntityId::new(0, 0);
    let view = w.get(id).unwrap();
    assert!(!view.has_stats);
    assert_eq!(view.hp, 0);
    assert_eq!((w.hp_of(id), w.stats_of(id)), (None, None));
    let mut max = Fx::ZERO;
    for _ in 0..200 {
        w.step();
        let v = w.get(id).unwrap();
        max = max.max((v.pos - v.prev_pos).length_squared());
    }
    assert!(max > Fx::ZERO, "it wanders");
    let world_step = per_tick(sf_sim::DEFAULT_SPEED);
    assert!(max <= (world_step + Fx::from_raw(2)).square());
    // And it is faster than the table's rifleman.
    let rifle = w.unit_table().get(RIFLEMAN).unwrap().speed_per_tick;
    assert!(world_step > rifle);
}

#[test]
fn a_legacy_place_still_takes_one_cell_whatever_the_table_says() {
    let mut w = world();
    let place = Command::Place {
        kind: HOUSE,
        cell: Cell::new(3, 2),
        owner: PlayerId(0),
    };
    assert_eq!(one(&mut w, place), (1, 0));
    assert_eq!(w.occupied_cells(), 1);
    let id = EntityId::new(0, 0);
    assert_eq!(w.hp_of(id), None);
    assert_eq!(
        w.footprint_of(id),
        Some(Footprint {
            width: 1,
            height: 1
        })
    );
    // Despawn frees that one cell.
    assert_eq!(one(&mut w, Command::Despawn { id }), (1, 0));
    assert_eq!(w.occupied_cells(), 0);
}

// --- the checksum ---

fn script(w: &mut World) {
    at(w, 0, 0, structure(HOUSE, 3, 2, 0));
    at(w, 0, 1, structure(TOWER, 9, 8, 1));
    at(w, 0, 0, unit(RIFLEMAN, 1, 1, 0));
    at(w, 0, 1, unit(TANK, 12, 10, 1));
    at(w, 1, 0, unit(CRATE, 6, 6, 0));
    at(w, 2, 0, structure(HOUSE, 4, 3, 0)); // refused: overlaps
    at(w, 3, 0, unit(UNKNOWN, 1, 1, 0)); // refused
    at(
        w,
        5,
        1,
        Command::MoveTo {
            ids: vec![EntityId::new(2, 0), EntityId::new(3, 0)],
            target: Vec2::from_ints(7, 7),
        },
    );
    at(
        w,
        40,
        0,
        Command::Despawn {
            id: EntityId::new(0, 0),
        },
    );
    at(w, 41, 1, structure(HOUSE, 3, 3, 1));
}

fn run(mut w: World, ticks: u64) -> (World, Vec<u64>) {
    script(&mut w);
    for _ in 0..ticks {
        w.step();
    }
    let history = w.checksum_history().to_vec();
    (w, history)
}

#[test]
fn the_same_script_gives_the_same_checksums() {
    let (a, ha) = run(world(), 300);
    let (b, hb) = run(world(), 300);
    assert_eq!(ha, hb);
    assert_eq!(a, b);
    assert_eq!(a.occupied_cells(), 1 + 6, "tower and the second house");
}

#[test]
fn a_clone_stays_in_step() {
    let mut w = world();
    script(&mut w);
    for _ in 0..60 {
        w.step();
    }
    let mut copy = w.clone();
    for _ in 0..100 {
        w.step();
        copy.step();
        assert_eq!(w.checksum(), copy.checksum());
    }
}

#[test]
fn the_order_the_table_is_listed_in_does_not_matter() {
    let mut stats: Vec<UnitStats> = table().iter().copied().collect();
    stats.reverse();
    let terrain = Terrain::from_layer(MAP.0, MAP.1, &layer(), &defs()).unwrap();
    let w = World::with_terrain(WorldConfig::new(7).with_map_size(MAP.0, MAP.1), terrain)
        .unwrap()
        .with_units(UnitTable::new(stats).unwrap())
        .unwrap();
    assert_eq!(w.checksum(), world().checksum());
    let (_, a) = run(w, 100);
    let (_, b) = run(world(), 100);
    assert_eq!(a, b);
}

#[test]
fn the_table_and_the_units_are_in_the_checksum() {
    let plain = {
        let terrain = Terrain::from_layer(MAP.0, MAP.1, &layer(), &defs()).unwrap();
        World::with_terrain(WorldConfig::new(7).with_map_size(MAP.0, MAP.1), terrain).unwrap()
    };
    // An empty table is no table: nothing is added to the hashed bytes.
    let empty = {
        let terrain = Terrain::from_layer(MAP.0, MAP.1, &layer(), &defs()).unwrap();
        World::with_terrain(WorldConfig::new(7).with_map_size(MAP.0, MAP.1), terrain)
            .unwrap()
            .with_units(UnitTable::empty())
            .unwrap()
    };
    assert_eq!(plain.checksum(), empty.checksum());
    assert_ne!(plain.checksum(), world().checksum(), "the table is hashed");

    // A different hit point value is a different world.
    let mut other: Vec<UnitStats> = table().iter().copied().collect();
    other[0].hp += 1;
    let terrain = Terrain::from_layer(MAP.0, MAP.1, &layer(), &defs()).unwrap();
    let changed = World::with_terrain(WorldConfig::new(7).with_map_size(MAP.0, MAP.1), terrain)
        .unwrap()
        .with_units(UnitTable::new(other).unwrap())
        .unwrap();
    assert_ne!(changed.checksum(), world().checksum());

    // The same kind made by SpawnUnit and by the legacy Spawn differs.
    let mut a = world();
    let mut b = world();
    one(&mut a, unit(RIFLEMAN, 4, 4, 0));
    one(
        &mut b,
        Command::Spawn {
            kind: RIFLEMAN,
            pos: Vec2::from_ints(4, 4),
            owner: PlayerId(0),
        },
    );
    assert_ne!(a.checksum(), b.checksum());
}

#[test]
fn commands_that_never_touch_the_table_hash_as_before_in_a_world_without_one() {
    // The legacy script on a table-less world is what every earlier checksum
    // is; the full pins are in golden.rs and the headless scenarios. Here: the
    // new commands are the only thing that can make a table-less world differ,
    // and they are ignored there (no kind is defined).
    let mut w = World::new(WorldConfig::new(7).with_map_size(MAP.0, MAP.1)).unwrap();
    assert_eq!(one(&mut w, unit(RIFLEMAN, 1, 1, 0)), (0, 1));
    assert_eq!(one(&mut w, structure(TOWER, 1, 1, 0)), (0, 1));
    assert_eq!(w.len(), 0);
}
