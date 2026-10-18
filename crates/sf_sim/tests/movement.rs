//! Movement classes and terrain costs (WP4.1), through the public API: the
//! `cost`/`passable` queries, the speed a cell gives, a unit stopping at the
//! edge of a cell it cannot enter with its order kept, the corner rule, the
//! spawn refusal, wanderers that still ignore terrain, and the checksum.

use sf_math::{Fx, Vec2};
use sf_sim::{
    Cell, Command, ConfigError, Cost, EntityId, Kind, MAX_STEP, MovementTable, Order, PlayerId,
    Terrain, TerrainDef, TerrainFlags, TerrainId, UnitStats, UnitTable, World, WorldConfig,
    per_tick, scale_step,
};

const GRASS: TerrainId = TerrainId(0);
const ROAD: TerrainId = TerrainId(1);
const WATER: TerrainId = TerrainId(2);
const ROCK: TerrainId = TerrainId(3);
const ORE: TerrainId = TerrainId(4);
const BRIDGE: TerrainId = TerrainId(5);

// Movement classes of this test (permanent kind numbers).
const WALKER: u16 = 0;
const BOAT: u16 = 1;
const FLYER: u16 = 2;
const HOVER: u16 = 3;

// Unit kinds.
const SOLDIER: Kind = Kind(1);
const GUNBOAT: Kind = Kind(2);
const PLANE: Kind = Kind(3);
const AMPHIBIAN: Kind = Kind(4);
const CRATE: Kind = Kind(5);
const SPRINTER: Kind = Kind(6); // a walker fast enough to snap onto a point from afar

const MAP: (u32, u32) = (24, 12);

/// Grass, with a road in columns 4 to 7, ore in 8 and 9, a river in 12 to 14
/// that is bridged in row 5, and rock in 18 and 19.
fn layer() -> Vec<u8> {
    (0..MAP.1)
        .flat_map(|y| {
            (0..MAP.0).map(move |x| {
                match x {
                    4..=7 => ROAD,
                    8..=9 => ORE,
                    12..=14 if y == 5 => BRIDGE,
                    12..=14 => WATER,
                    18..=19 => ROCK,
                    _ => GRASS,
                }
                .0
            })
        })
        .collect()
}

fn defs() -> Vec<TerrainDef> {
    let open = |id, buildable| TerrainDef {
        id,
        flags: TerrainFlags {
            buildable,
            ..TerrainFlags::OPEN
        },
    };
    let blocked = |id| TerrainDef {
        id,
        flags: TerrainFlags::default(),
    };
    vec![
        open(GRASS, true),
        open(ROAD, false),
        blocked(WATER),
        blocked(ROCK),
        open(ORE, false),
        open(BRIDGE, false),
    ]
}

fn costs() -> MovementTable {
    use sf_sim::ClassCosts;
    let class = |class, list: [Cost; 6]| ClassCosts {
        class,
        costs: [GRASS, ROAD, WATER, ROCK, ORE, BRIDGE]
            .into_iter()
            .zip(list)
            .collect(),
    };
    let x = Cost::Impassable;
    let s = Cost::Speed;
    MovementTable::new(vec![
        class(WALKER, [s(1000), s(1500), x, x, s(500), s(1200)]),
        class(BOAT, [x, x, s(1000), x, x, s(1000)]),
        class(FLYER, [s(1000); 6]),
        class(HOVER, [s(1000), s(1000), s(500), x, s(1000), s(1000)]),
    ])
    .unwrap()
}

fn table() -> UnitTable {
    let mobile = |kind: Kind, class: u16, speed_tenths: i64| UnitStats {
        kind,
        hp: 100,
        movement_class: Some(class),
        speed_per_tick: per_tick(Fx::from_ratio(speed_tenths, 10)),
        // Full turns in one tick: a unit is lined up at once, so the first tick
        // of an order already moves it.
        turn_per_tick: 255,
        size: Fx::from_ratio(4, 10),
        footprint: None,
        weapon_range: Fx::ZERO,
    };
    UnitTable::new(vec![
        mobile(SOLDIER, WALKER, 20),
        mobile(GUNBOAT, BOAT, 30),
        mobile(PLANE, FLYER, 60),
        mobile(AMPHIBIAN, HOVER, 20),
        mobile(SPRINTER, WALKER, 60),
        UnitStats {
            kind: CRATE,
            hp: 10,
            movement_class: None,
            speed_per_tick: Fx::ZERO,
            turn_per_tick: 0,
            size: Fx::ONE,
            footprint: None,
            weapon_range: Fx::ZERO,
        },
    ])
    .unwrap()
}

fn world() -> World {
    let terrain = Terrain::from_layer(MAP.0, MAP.1, &layer(), &defs()).unwrap();
    World::with_terrain(WorldConfig::new(7).with_map_size(MAP.0, MAP.1), terrain)
        .unwrap()
        .with_units(table())
        .unwrap()
        .with_movement(costs())
        .unwrap()
}

fn middle(x: i32, y: i32) -> Vec2 {
    Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF)
}

/// Spawns `kind` in the middle of cell `(x, y)`; the id of the new unit.
fn spawn(w: &mut World, kind: Kind, x: i32, y: i32) -> EntityId {
    let before = w.slot_count();
    let tick = w.tick();
    w.schedule(
        tick,
        PlayerId(0),
        Command::SpawnUnit {
            kind,
            pos: middle(x, y),
            owner: PlayerId(0),
        },
    )
    .unwrap();
    let report = w.step();
    assert_eq!(report.commands_ignored, 0, "{kind:?} at ({x}, {y})");
    assert_eq!(w.slot_count(), before + 1);
    EntityId::new(before as u32, 0)
}

fn order(w: &mut World, id: EntityId, to: (i32, i32)) {
    let tick = w.tick();
    w.schedule(
        tick,
        PlayerId(0),
        Command::Order {
            ids: vec![id],
            order: Order::Move(middle(to.0, to.1)),
            queued: false,
        },
    )
    .unwrap();
}

fn pos(w: &World, id: EntityId) -> Vec2 {
    w.get(id).unwrap().pos
}

fn cell(w: &World, id: EntityId) -> Cell {
    w.cell_at(pos(w, id)).unwrap()
}

fn run(w: &mut World, ticks: u32) {
    for _ in 0..ticks {
        w.step();
    }
}

// --- the queries ---

#[test]
fn cost_and_passable_answer_per_class_and_cell() {
    let w = world();
    let c = |class, x, y| w.cost(class, Cell::new(x, y));
    // The table, read through the cell's terrain.
    assert_eq!(c(WALKER, 0, 0), Cost::Speed(1000)); // grass
    assert_eq!(c(WALKER, 5, 0), Cost::Speed(1500)); // road
    assert_eq!(c(WALKER, 8, 0), Cost::Speed(500)); // ore
    assert_eq!(c(WALKER, 13, 0), Cost::Impassable); // water
    assert_eq!(c(WALKER, 18, 0), Cost::Impassable); // rock
    assert_eq!(c(WALKER, 13, 5), Cost::Speed(1200)); // bridge
    assert_eq!(c(BOAT, 13, 0), Cost::Speed(1000));
    assert_eq!(c(BOAT, 13, 5), Cost::Speed(1000), "a boat passes under");
    assert_eq!(c(BOAT, 0, 0), Cost::Impassable);
    assert_eq!(c(HOVER, 13, 0), Cost::Speed(500));
    assert_eq!(c(HOVER, 18, 0), Cost::Impassable);
    assert_eq!(c(FLYER, 18, 0), Cost::Speed(1000), "air ignores terrain");
    assert!(w.passable(FLYER, Cell::new(13, 0)) && !w.passable(WALKER, Cell::new(13, 0)));
    // Off the map: nobody, not even a plane. An unknown class is never blocked.
    assert_eq!(c(FLYER, 24, 0), Cost::Impassable);
    assert_eq!(c(FLYER, 0, 12), Cost::Impassable);
    assert_eq!(c(99, 13, 0), Cost::OPEN);
    assert_eq!(w.movement_table().len(), 4);
}

#[test]
fn a_world_without_a_table_or_without_a_layer_is_open_ground() {
    let terrain = Terrain::from_layer(MAP.0, MAP.1, &layer(), &defs()).unwrap();
    let no_table =
        World::with_terrain(WorldConfig::new(1).with_map_size(MAP.0, MAP.1), terrain).unwrap();
    assert_eq!(no_table.cost(WALKER, Cell::new(13, 0)), Cost::OPEN);
    assert!(no_table.movement_table().is_empty());
    let no_layer = World::new(WorldConfig::new(1).with_map_size(MAP.0, MAP.1))
        .unwrap()
        .with_movement(costs())
        .unwrap();
    assert_eq!(no_layer.cost(BOAT, Cell::new(0, 0)), Cost::OPEN);
    assert_eq!(no_layer.cost(BOAT, Cell::new(24, 0)), Cost::Impassable);
}

#[test]
fn the_table_is_part_of_how_a_world_is_made_and_must_cover_the_map() {
    let mut w = world();
    spawn(&mut w, SOLDIER, 1, 1);
    // Too late once the world has started.
    assert_eq!(
        w.clone().with_movement(costs()).unwrap_err(),
        ConfigError::MovementAfterStart
    );
    // A class that leaves out a terrain type of the map is refused...
    let gap = MovementTable::new(vec![sf_sim::ClassCosts {
        class: WALKER,
        costs: vec![(GRASS, Cost::OPEN)],
    }])
    .unwrap();
    let terrain = Terrain::from_layer(MAP.0, MAP.1, &layer(), &defs()).unwrap();
    let fresh =
        World::with_terrain(WorldConfig::new(7).with_map_size(MAP.0, MAP.1), terrain).unwrap();
    let err = fresh.clone().with_movement(gap.clone()).unwrap_err();
    assert_eq!(
        err,
        ConfigError::MovementGap {
            class: WALKER,
            terrain: ROAD
        }
    );
    assert!(!err.to_string().is_empty());
    // ...but a world without a layer is open ground and takes any table.
    let open = World::new(WorldConfig::new(7).with_map_size(MAP.0, MAP.1)).unwrap();
    assert!(open.with_movement(gap).is_ok());
}

// --- the speed a cell gives ---

/// The distance a unit covers on each of the first `ticks` ticks of a move,
/// as raw fixed-point units of its x coordinate.
fn strides(w: &mut World, id: EntityId, ticks: u32) -> Vec<i64> {
    let mut out = Vec::new();
    let mut last = pos(w, id).x.raw();
    for _ in 0..ticks {
        w.step();
        let now = pos(w, id).x.raw();
        out.push(now - last);
        last = now;
    }
    out
}

#[test]
fn a_unit_moves_at_its_speed_times_the_multiplier_of_the_cell_it_stands_in() {
    let step = per_tick(Fx::from_int(2));
    for (x, y, permille) in [
        (1, 1, 1000_u16), // grass
        (4, 2, 1500),     // road (columns 4 to 7)
        (8, 3, 500),      // ore (columns 8 and 9)
    ] {
        let mut w = world();
        let id = spawn(&mut w, SOLDIER, x, y);
        // East, within the same strip of terrain: three ticks only.
        order(&mut w, id, (x + 1, y));
        let want = scale_step(step, permille).raw();
        for (k, got) in strides(&mut w, id, 3).into_iter().enumerate() {
            // The facing is a table lookup, a hair off a perfect east.
            assert!(
                (got - want).abs() <= 2,
                "cell ({x}, {y}) stride {k}: {got} raw units, expected {want} ({permille} permille)"
            );
        }
    }
    // The three speeds are in the ratio of the table.
    assert!(scale_step(step, 1500) > scale_step(step, 1000));
    assert!(scale_step(step, 1000) > scale_step(step, 500));
}

#[test]
fn the_multiplier_is_the_cell_the_unit_is_in_when_the_step_starts() {
    let mut w = world();
    // On the last road cell (column 7), heading into ore (column 8) and beyond.
    let id = spawn(&mut w, SOLDIER, 7, 2);
    order(&mut w, id, (11, 2));
    let step = per_tick(Fx::from_int(2));
    let (road, ore) = (scale_step(step, 1500).raw(), scale_step(step, 500).raw());
    let mut seen = Vec::new();
    for _ in 0..40 {
        let from = cell(&w, id).x;
        let before = pos(&w, id).x.raw();
        w.step();
        let stride = pos(&w, id).x.raw() - before;
        seen.push((from, stride));
    }
    for (from, stride) in seen {
        let want = match from {
            7 => road,
            8 | 9 => ore,
            _ => continue,
        };
        assert!(
            (stride - want).abs() <= 2,
            "stepping from column {from}: {stride}, expected {want}"
        );
    }
}

// --- stopping at the edge ---

#[test]
fn a_move_into_water_stops_at_the_edge_and_keeps_the_order() {
    let mut w = world();
    let id = spawn(&mut w, SOLDIER, 10, 2);
    let goal = middle(16, 2);
    order(&mut w, id, (16, 2));
    run(&mut w, 300);
    // Standing in the last grass cell before the river (columns 12 to 14), less
    // than one step from its edge.
    assert_eq!(cell(&w, id), Cell::new(11, 2));
    let step = per_tick(Fx::from_int(2));
    let edge = Fx::from_int(12);
    assert!(
        edge - pos(&w, id).x <= step,
        "x = {:?}, more than a step short of the edge",
        pos(&w, id).x
    );
    // The order is kept, and so is the waypoint.
    assert_eq!(w.order_of(id), Some(Order::Move(goal)));
    assert_eq!(w.waypoint_of(id), Some(goal));
    // It does not creep: 100 more ticks change nothing.
    let before = pos(&w, id);
    run(&mut w, 100);
    assert_eq!(pos(&w, id), before);
    // A new order replaces it and the unit goes.
    order(&mut w, id, (10, 8));
    run(&mut w, 300);
    assert_eq!(w.order_of(id), Some(Order::Idle));
    assert_eq!(cell(&w, id), Cell::new(10, 8));
}

#[test]
fn a_blocked_move_holds_back_the_queue_behind_it() {
    let mut w = world();
    let id = spawn(&mut w, SOLDIER, 10, 2);
    let tick = w.tick();
    for (to, queued) in [((16, 2), false), ((10, 8), true)] {
        w.schedule(
            tick,
            PlayerId(0),
            Command::Order {
                ids: vec![id],
                order: Order::Move(middle(to.0, to.1)),
                queued,
            },
        )
        .unwrap();
    }
    run(&mut w, 300);
    assert_eq!(cell(&w, id), Cell::new(11, 2));
    assert_eq!(w.order_of(id), Some(Order::Move(middle(16, 2))));
    assert_eq!(
        w.queued_orders_of(id).unwrap(),
        [Order::Move(middle(10, 8))]
    );
    // Stop clears both.
    let tick = w.tick();
    w.schedule(tick, PlayerId(0), Command::Stop { ids: vec![id] })
        .unwrap();
    run(&mut w, 5);
    assert_eq!(w.order_of(id), Some(Order::Idle));
    assert!(w.queued_orders_of(id).unwrap().is_empty());
}

#[test]
fn a_destination_inside_blocked_ground_is_approached_but_never_reached() {
    let mut w = world();
    let id = spawn(&mut w, SOLDIER, 16, 2);
    // The middle of a rock cell.
    order(&mut w, id, (18, 2));
    run(&mut w, 300);
    assert_eq!(cell(&w, id), Cell::new(17, 2));
    assert_eq!(w.order_of(id), Some(Order::Move(middle(18, 2))));
}

#[test]
fn a_unit_does_not_snap_onto_a_spot_in_blocked_ground() {
    // A unit that is within one step of its destination lands on it. A walker
    // fast enough to do that from outside the arrival tolerance (an eighth of a
    // cell) stands at the edge of the rock, and the spot just inside it is a
    // step like any other: refused.
    let mut w = world();
    let id = spawn(&mut w, SPRINTER, 16, 2);
    order(&mut w, id, (18, 2));
    run(&mut w, 100);
    assert_eq!(cell(&w, id), Cell::new(17, 2));
    let before = pos(&w, id);
    // The first point of the rock, on the row the unit stands on.
    let goal = Vec2::new(Fx::from_int(18), before.y);
    let step = per_tick(Fx::from_int(6));
    let distance = (goal - before).length_squared();
    assert!(distance <= step.square(), "within one step");
    assert!(
        distance > Fx::from_ratio(1, 8).square(),
        "outside the tolerance"
    );
    let tick = w.tick();
    w.schedule(
        tick,
        PlayerId(0),
        Command::Order {
            ids: vec![id],
            order: Order::Move(goal),
            queued: false,
        },
    )
    .unwrap();
    run(&mut w, 50);
    assert_eq!(pos(&w, id), before);
    assert_eq!(w.order_of(id), Some(Order::Move(goal)));
}

#[test]
fn the_legacy_move_stands_at_the_edge_too() {
    let mut w = world();
    let id = spawn(&mut w, SOLDIER, 10, 2);
    let tick = w.tick();
    w.schedule(
        tick,
        PlayerId(0),
        Command::MoveTo {
            ids: vec![id],
            target: middle(16, 2),
        },
    )
    .unwrap();
    run(&mut w, 300);
    assert_eq!(cell(&w, id), Cell::new(11, 2));
    assert_eq!(w.waypoint_of(id), Some(middle(16, 2)));
}

// --- who crosses what ---

#[test]
fn land_crosses_the_river_on_the_bridge_and_a_boat_passes_under_it() {
    let mut w = world();
    let soldier = spawn(&mut w, SOLDIER, 11, 5);
    let boat = spawn(&mut w, GUNBOAT, 13, 1);
    order(&mut w, soldier, (16, 5));
    order(&mut w, boat, (13, 9));
    run(&mut w, 400);
    // The soldier walked over the bridge row, the boat down the river
    // and under the bridge cell (13, 5).
    assert_eq!(w.order_of(soldier), Some(Order::Idle));
    assert_eq!(cell(&w, soldier), Cell::new(16, 5));
    assert_eq!(w.order_of(boat), Some(Order::Idle));
    assert_eq!(cell(&w, boat), Cell::new(13, 9));
}

#[test]
fn land_is_blocked_by_water_and_rock_and_a_boat_by_land() {
    let mut w = world();
    let soldier = spawn(&mut w, SOLDIER, 11, 7);
    let boat = spawn(&mut w, GUNBOAT, 13, 7);
    order(&mut w, soldier, (13, 7)); // into the river
    order(&mut w, boat, (10, 7)); // onto the bank
    run(&mut w, 300);
    assert_eq!(cell(&w, soldier), Cell::new(11, 7));
    assert_eq!(
        cell(&w, boat),
        Cell::new(12, 7),
        "the near edge of the river"
    );
    assert!(matches!(w.order_of(soldier), Some(Order::Move(_))));
    assert!(matches!(w.order_of(boat), Some(Order::Move(_))));

    let mut w = world();
    let soldier = spawn(&mut w, SOLDIER, 16, 3);
    order(&mut w, soldier, (21, 3)); // through the rock in columns 18 and 19
    run(&mut w, 400);
    assert_eq!(cell(&w, soldier), Cell::new(17, 3));
}

#[test]
fn air_crosses_everything_and_hover_crosses_water_at_half_speed() {
    let mut w = world();
    let plane = spawn(&mut w, PLANE, 1, 2);
    order(&mut w, plane, (22, 2)); // over the river and the rock
    run(&mut w, 400);
    assert_eq!(w.order_of(plane), Some(Order::Idle));
    assert_eq!(cell(&w, plane), Cell::new(22, 2));

    let mut w = world();
    let ferry = spawn(&mut w, AMPHIBIAN, 11, 8);
    order(&mut w, ferry, (16, 8));
    run(&mut w, 400);
    assert_eq!(w.order_of(ferry), Some(Order::Idle));
    assert_eq!(cell(&w, ferry), Cell::new(16, 8));
    // In the river it goes at half its speed.
    let mut w = world();
    let ferry = spawn(&mut w, AMPHIBIAN, 13, 8);
    order(&mut w, ferry, (13, 9));
    let step = per_tick(Fx::from_int(2));
    let before = pos(&w, ferry).y.raw();
    w.step();
    let stride = pos(&w, ferry).y.raw() - before;
    assert!(
        (stride - scale_step(step, 500).raw()).abs() <= 2,
        "stride {stride}"
    );
}

// --- the corner rule ---

#[test]
fn a_diagonal_step_does_not_slip_between_two_blocked_cells() {
    // A 8 x 8 field of grass with water at (4, 3) and (3, 4): the two cells
    // that touch at the corner (4, 4).
    let mut ids = vec![GRASS.0; 64];
    ids[3 * 8 + 4] = WATER.0;
    ids[4 * 8 + 3] = WATER.0;
    let make = |ids: &[u8]| {
        let terrain = Terrain::from_layer(8, 8, ids, &defs()).unwrap();
        World::with_terrain(WorldConfig::new(7).with_map_size(8, 8), terrain)
            .unwrap()
            .with_units(table())
            .unwrap()
            .with_movement(costs())
            .unwrap()
    };
    let mut w = make(&ids);
    let id = spawn(&mut w, SOLDIER, 3, 3);
    order(&mut w, id, (6, 6));
    run(&mut w, 300);
    assert_eq!(cell(&w, id), Cell::new(3, 3), "the corner is closed");

    // With neither cell blocked, the same walk arrives.
    ids[4 * 8 + 3] = GRASS.0;
    ids[3 * 8 + 4] = GRASS.0;
    let mut w = make(&ids);
    let id = spawn(&mut w, SOLDIER, 3, 3);
    order(&mut w, id, (6, 6));
    run(&mut w, 300);
    assert_eq!(cell(&w, id), Cell::new(6, 6));
}

// --- spawning ---

#[test]
fn a_unit_is_not_spawned_on_ground_its_class_cannot_enter() {
    let mut w = world();
    let at = |kind, x, y| Command::SpawnUnit {
        kind,
        pos: middle(x, y),
        owner: PlayerId(0),
    };
    let tick = w.tick();
    for command in [
        at(SOLDIER, 13, 1), // water: refused
        at(SOLDIER, 18, 1), // rock: refused
        at(GUNBOAT, 1, 1),  // grass: refused
        at(PLANE, 18, 1),   // rock: air goes anywhere
        at(GUNBOAT, 13, 1), // water
        at(SOLDIER, 13, 5), // the bridge
        at(AMPHIBIAN, 13, 2),
        at(CRATE, 13, 3), // an object has no class and stands where it is put
    ] {
        w.schedule(tick, PlayerId(0), command).unwrap();
    }
    let report = w.step();
    assert_eq!((report.commands_executed, report.commands_ignored), (5, 3));
    assert_eq!(w.len(), 5);
}

// --- what ignores terrain ---

#[test]
fn wanderers_of_the_legacy_spawn_still_cross_everything() {
    let run_wanderers = |with_table: bool| {
        let terrain = Terrain::from_layer(MAP.0, MAP.1, &layer(), &defs()).unwrap();
        let mut w =
            World::with_terrain(WorldConfig::new(5).with_map_size(MAP.0, MAP.1), terrain).unwrap();
        if with_table {
            w = w.with_movement(costs()).unwrap();
        }
        let tick = w.tick();
        for i in 0..40 {
            w.schedule(
                tick,
                PlayerId(0),
                Command::Spawn {
                    kind: Kind(1),
                    pos: middle(i % 20, i % 10),
                    owner: PlayerId(0),
                },
            )
            .unwrap();
        }
        let mut seen_in_water = false;
        let mut positions = Vec::new();
        for _ in 0..600 {
            w.step();
            for v in w.snapshot().iter() {
                let c = w.cell_at(v.pos).unwrap();
                seen_in_water |= w.terrain_at(c) == Some(WATER);
                positions.push(v.pos);
            }
        }
        (seen_in_water, positions)
    };
    let (water, with) = run_wanderers(true);
    let (_, without) = run_wanderers(false);
    assert!(water, "the wanderers walk through the river");
    assert_eq!(with, without, "a movement table does not move a wanderer");
}

#[test]
fn a_wandering_unit_never_ends_a_tick_on_ground_it_cannot_enter() {
    let mut w = world();
    let soldier = spawn(&mut w, SOLDIER, 1, 1);
    let boat = spawn(&mut w, GUNBOAT, 13, 1);
    let ferry = spawn(&mut w, AMPHIBIAN, 11, 8);
    let tick = w.tick();
    w.schedule(
        tick,
        PlayerId(0),
        Command::SetWanderArea {
            ids: vec![soldier, boat, ferry],
            area: sf_sim::Area::from_cells(0, 0, MAP.0 as i32, MAP.1 as i32),
        },
    )
    .unwrap();
    let mut moved = [false; 3];
    let start = [pos(&w, soldier), pos(&w, boat), pos(&w, ferry)];
    for _ in 0..4000 {
        w.step();
        for (k, (id, class)) in [(soldier, WALKER), (boat, BOAT), (ferry, HOVER)]
            .into_iter()
            .enumerate()
        {
            assert!(
                w.passable(class, cell(&w, id)),
                "class {class} stands on {}",
                cell(&w, id)
            );
            moved[k] |= pos(&w, id) != start[k];
        }
    }
    assert!(moved[0] && moved[2], "the walker and the ferry did wander");
}

// --- open ground changes nothing ---

/// What stands on the layer in `checksum_with`.
#[derive(Copy, Clone)]
enum Cast {
    /// Nothing.
    Empty,
    /// A crate: a unit-table entity with no movement class.
    Crate,
    /// A soldier who stands still.
    Standing,
    /// A soldier with an order.
    Ordered,
    /// A soldier sent by the legacy `MoveTo`.
    LegacyMove,
}

/// The checksum of a world on the layer with the given movement table (none:
/// open ground) and `cast` on it.
fn checksum_with(table: Option<MovementTable>, cast: Cast) -> u64 {
    let terrain = Terrain::from_layer(MAP.0, MAP.1, &layer(), &defs()).unwrap();
    let mut w = World::with_terrain(WorldConfig::new(7).with_map_size(MAP.0, MAP.1), terrain)
        .unwrap()
        .with_units(self::table())
        .unwrap();
    if let Some(table) = table {
        w = w.with_movement(table).unwrap();
    }
    match cast {
        Cast::Empty => {}
        Cast::Crate => {
            spawn(&mut w, CRATE, 1, 1);
        }
        Cast::Standing => {
            spawn(&mut w, SOLDIER, 1, 1);
        }
        Cast::Ordered => {
            let soldier = spawn(&mut w, SOLDIER, 1, 1);
            order(&mut w, soldier, (3, 1));
        }
        Cast::LegacyMove => {
            let soldier = spawn(&mut w, SOLDIER, 1, 1);
            let tick = w.tick();
            w.schedule(
                tick,
                PlayerId(0),
                Command::MoveTo {
                    ids: vec![soldier],
                    target: middle(3, 1),
                },
            )
            .unwrap();
        }
    }
    run(&mut w, 5);
    w.checksum()
}

#[test]
fn on_open_ground_a_table_changes_nothing() {
    // A world without a layer: identical checksums, tick for tick, with and
    // without the table, orders and all.
    let build = |with_table: bool| {
        let mut w = World::new(WorldConfig::new(11).with_map_size(MAP.0, MAP.1))
            .unwrap()
            .with_units(table())
            .unwrap();
        if with_table {
            w = w.with_movement(costs()).unwrap();
        }
        let a = spawn(&mut w, SOLDIER, 2, 2);
        let b = spawn(&mut w, GUNBOAT, 3, 3);
        order(&mut w, a, (20, 9));
        order(&mut w, b, (5, 1));
        run(&mut w, 400);
        w.checksum_history().to_vec()
    };
    assert_eq!(build(true), build(false));
}

#[test]
fn the_table_joins_the_checksum_on_a_map_with_terrain_once_a_unit_has_a_class() {
    // Nothing that has a movement class: the table is not in the checksum (so a
    // pinned world without such a unit stays where it was).
    for cast in [Cast::Empty, Cast::Crate] {
        assert_eq!(
            checksum_with(Some(costs()), cast),
            checksum_with(None, cast)
        );
    }
    // A unit with a class, whatever drives it, brings the table into the state:
    // standing, ordered, or sent by the legacy MoveTo.
    for cast in [Cast::Standing, Cast::Ordered, Cast::LegacyMove] {
        assert_ne!(
            checksum_with(Some(costs()), cast),
            checksum_with(None, cast)
        );
    }
}

#[test]
fn a_changed_cost_changes_the_checksum_and_the_order_of_the_classes_does_not() {
    let base = checksum_with(Some(costs()), Cast::Ordered);
    // Same classes, other order, each class's costs in another order.
    let mut classes: Vec<sf_sim::ClassCosts> = Vec::new();
    let ids = [GRASS, ROAD, WATER, ROCK, ORE, BRIDGE];
    for class in [HOVER, FLYER, BOAT, WALKER] {
        let list: Vec<(TerrainId, Cost)> = ids
            .iter()
            .rev()
            .map(|&id| (id, costs().get(class, id).unwrap().unwrap()))
            .collect();
        classes.push(sf_sim::ClassCosts { class, costs: list });
    }
    let shuffled = MovementTable::new(classes.clone()).unwrap();
    assert_eq!(checksum_with(Some(shuffled), Cast::Ordered), base);
    // A cost one thousandth different, on a terrain the soldier never touches:
    // the digest still sees it.
    classes[3].costs[5].1 = Cost::Speed(1001);
    let changed = MovementTable::new(classes).unwrap();
    assert_ne!(checksum_with(Some(changed), Cast::Ordered), base);
}

#[test]
fn a_step_never_covers_a_whole_cell() {
    assert!(MAX_STEP < Fx::ONE);
}
