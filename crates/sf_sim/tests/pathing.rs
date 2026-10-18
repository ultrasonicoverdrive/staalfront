//! Hierarchical path finding (WP4.2), through the public API: a move finds its
//! way round what a class cannot cross, the route is asked for and followed
//! as the rules in `docs/rules/pathfinding.md` say, the graph follows the
//! structures, and the service stays inside its budget. The headless scenario
//! `pathing` (`tools/sf_headless/tests/pathing.rs`) has the routes measured
//! against a plain search of the grid; the planner's own tests are in
//! `src/path/`.

use sf_math::{Fx, Vec2};
use sf_sim::{
    Cell, Command, ConfigError, Cost, EntityId, Footprint, Kind, MIN_NODE_BUDGET, MovementTable,
    Order, PlayerId, RouteStatus, Terrain, TerrainDef, TerrainFlags, TerrainId, UnitStats,
    UnitTable, World, WorldConfig, per_tick,
};

const GRASS: TerrainId = TerrainId(0);
const WATER: TerrainId = TerrainId(2);
const ROCK: TerrainId = TerrainId(3);
const BRIDGE: TerrainId = TerrainId(5);

// Movement classes (permanent kind numbers).
const WALKER: u16 = 0;
const BOAT: u16 = 1;
const FLYER: u16 = 2;
const HOVER: u16 = 3;

// Unit kinds.
const SOLDIER: Kind = Kind(1);
const GUNMAN: Kind = Kind(2);
const PLANE: Kind = Kind(3);
const FERRY: Kind = Kind(4);
const SPRINTER: Kind = Kind(5);
const GUNBOAT: Kind = Kind(6);
const HUT: Kind = Kind(7);

const MAP: (u32, u32) = (64, 48);

/// Grass, with a wall of rock in columns 30 and 31 that has a gap in rows 20 to
/// 23, a river in columns 44 to 47 bridged in row 10, and a lake in columns 50
/// to 60, rows 26 to 38, with an island of grass (columns 54 to 56, rows 30 to
/// 32) in it.
fn layer() -> Vec<u8> {
    (0..MAP.1)
        .flat_map(|y| {
            (0..MAP.0).map(move |x| {
                match (x, y) {
                    (30..=31, 20..=23) => GRASS,
                    (30..=31, _) => ROCK,
                    (44..=47, 10) => BRIDGE,
                    (44..=47, _) => WATER,
                    (54..=56, 30..=32) => GRASS,
                    (50..=60, 26..=38) => WATER,
                    _ => GRASS,
                }
                .0
            })
        })
        .collect()
}

fn defs() -> Vec<TerrainDef> {
    let open = |id| TerrainDef {
        id,
        flags: TerrainFlags::OPEN,
    };
    let blocked = |id| TerrainDef {
        id,
        flags: TerrainFlags::default(),
    };
    vec![
        open(GRASS),
        open(TerrainId(1)),
        blocked(WATER),
        blocked(ROCK),
        open(TerrainId(4)),
        open(BRIDGE),
    ]
}

fn costs() -> MovementTable {
    use sf_sim::ClassCosts;
    let class = |class, list: [Cost; 6]| ClassCosts {
        class,
        costs: (0_u8..6).map(TerrainId).zip(list).collect(),
    };
    let x = Cost::Impassable;
    let s = Cost::Speed;
    MovementTable::new(vec![
        class(WALKER, [s(1000), s(1000), x, x, s(1000), s(1000)]),
        class(BOAT, [x, x, s(1000), x, x, s(1000)]),
        class(FLYER, [s(1000); 6]),
        class(HOVER, [s(1000), s(1000), s(800), x, s(1000), s(1000)]),
    ])
    .unwrap()
}

fn table() -> UnitTable {
    let mobile = |kind: Kind, class: u16, speed_tenths: i64, turn: u8, range: i64| UnitStats {
        kind,
        hp: 100,
        movement_class: Some(class),
        speed_per_tick: per_tick(Fx::from_ratio(speed_tenths, 10)),
        turn_per_tick: turn,
        size: Fx::from_ratio(4, 10),
        footprint: None,
        weapon_range: Fx::from_int(range as i32),
    };
    UnitTable::new(vec![
        mobile(SOLDIER, WALKER, 30, 255, 0),
        mobile(GUNMAN, WALKER, 30, 255, 5),
        mobile(PLANE, FLYER, 60, 255, 0),
        mobile(FERRY, HOVER, 30, 255, 0),
        // A walker that covers 0.4 of a cell per tick and turns slowly: it
        // overshoots corners.
        mobile(SPRINTER, WALKER, 160, 6, 0),
        mobile(GUNBOAT, BOAT, 30, 255, 0),
        UnitStats {
            kind: HUT,
            hp: 100,
            movement_class: None,
            speed_per_tick: Fx::ZERO,
            turn_per_tick: 0,
            size: Fx::ZERO,
            footprint: Some(Footprint {
                width: 3,
                height: 3,
            }),
            weapon_range: Fx::ZERO,
        },
    ])
    .unwrap()
}

fn config() -> WorldConfig {
    WorldConfig::new(11).with_map_size(MAP.0, MAP.1)
}

fn world() -> World {
    let terrain = Terrain::from_layer(MAP.0, MAP.1, &layer(), &defs()).unwrap();
    World::with_terrain(config(), terrain)
        .unwrap()
        .with_units(table())
        .unwrap()
        .with_movement(costs())
        .unwrap()
}

fn mid(x: i32, y: i32) -> Vec2 {
    Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF)
}

fn spawn(w: &mut World, kind: Kind, x: i32, y: i32) -> EntityId {
    let before = w.slot_count();
    let tick = w.tick();
    w.schedule(
        tick,
        PlayerId(0),
        Command::SpawnUnit {
            kind,
            pos: mid(x, y),
            owner: PlayerId(0),
        },
    )
    .unwrap();
    let report = w.step();
    assert_eq!(report.commands_ignored, 0, "{kind:?} at ({x}, {y})");
    EntityId::new(before as u32, 0)
}

fn command(w: &mut World, c: Command) {
    let tick = w.tick();
    w.schedule(tick, PlayerId(0), c).unwrap();
}

fn order(w: &mut World, id: EntityId, to: (i32, i32)) {
    command(
        w,
        Command::Order {
            ids: vec![id],
            order: Order::Move(mid(to.0, to.1)),
            queued: false,
        },
    );
}

fn place_hut(w: &mut World, x: u32, y: u32) {
    command(
        w,
        Command::PlaceStructure {
            kind: HUT,
            cell: Cell::new(x, y),
            owner: PlayerId(0),
        },
    );
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

/// Steps until the unit has no order, at most `limit` ticks; the cells it stood
/// in after every tick.
fn walk(w: &mut World, id: EntityId, limit: u32) -> Vec<Cell> {
    let mut seen = Vec::new();
    for _ in 0..limit {
        w.step();
        seen.push(cell(w, id));
        if w.order_of(id) == Some(Order::Idle) {
            break;
        }
    }
    seen
}

// --- the basics ---

#[test]
fn a_move_goes_through_the_gap_of_a_wall_and_arrives() {
    let mut w = world();
    let id = spawn(&mut w, SOLDIER, 5, 5);
    order(&mut w, id, (55, 5));
    let seen = walk(&mut w, id, 1500);
    assert_eq!(w.order_of(id), Some(Order::Idle), "it arrived");
    // The wall (columns 30 and 31) is open in rows 20 to 23 only; the river is
    // crossed on the bridge in row 10.
    assert!(
        seen.iter()
            .any(|c| (30..=31).contains(&c.x) && (20..=23).contains(&c.y))
    );
    assert!(seen.contains(&Cell::new(45, 10)));
    assert!(seen.iter().all(|c| w.passable(WALKER, *c)));
    assert_eq!(cell(&w, id), Cell::new(55, 5));
    assert_eq!(w.route_of(id).unwrap().status, RouteStatus::None);
}

#[test]
fn the_route_is_asked_for_on_the_tick_of_the_order_and_followed_from_the_next() {
    let mut w = world();
    let id = spawn(&mut w, SOLDIER, 5, 5);
    let goal = mid(55, 5);
    order(&mut w, id, (55, 5));
    // The tick the order runs: the unit asks, and waits where it stands.
    w.step();
    assert_eq!(w.route_of(id).unwrap().status, RouteStatus::Waiting);
    assert_eq!(pos(&w, id), mid(5, 5));
    assert_eq!(w.waypoint_of(id), None);
    // The next tick the service gives it its route and it moves.
    w.step();
    let route = w.route_of(id).unwrap();
    assert_eq!(route.status, RouteStatus::Following);
    assert!(route.waypoints > 0 && !route.unreachable);
    assert_ne!(pos(&w, id), mid(5, 5));
    // It steers at a waypoint; the order keeps the destination.
    let waypoint = w.waypoint_of(id).unwrap();
    assert_ne!(waypoint, goal);
    assert_eq!(w.order_of(id), Some(Order::Move(goal)));
}

#[test]
fn a_destination_on_the_far_bank_of_the_lake_is_unreachable_on_foot_and_marked() {
    let mut w = world();
    let id = spawn(&mut w, SOLDIER, 40, 32);
    let goal = mid(55, 31);
    order(&mut w, id, (55, 31));
    run(&mut w, 600);
    let route = w.route_of(id).unwrap();
    assert_eq!(route.status, RouteStatus::Stranded);
    assert!(route.unreachable);
    // The nearest cell of the lake's shore to the island, by the straight
    // distance: the west shore, column 49, in the row of the goal.
    assert_eq!(cell(&w, id), Cell::new(49, 31));
    assert_eq!(route.goal, Some(Cell::new(49, 31)));
    assert_eq!(w.order_of(id), Some(Order::Move(goal)));
    // The hover craft crosses the lake and reaches the island.
    let ferry = spawn(&mut w, FERRY, 40, 32);
    order(&mut w, ferry, (55, 31));
    let seen = walk(&mut w, ferry, 800);
    assert_eq!(w.order_of(ferry), Some(Order::Idle));
    assert_eq!(*seen.last().unwrap(), Cell::new(55, 31));
    // A new order for the foot soldier replaces the route and the mark.
    order(&mut w, id, (40, 40));
    walk(&mut w, id, 800);
    assert_eq!(cell(&w, id), Cell::new(40, 40));
    assert!(!w.route_of(id).unwrap().unreachable);
}

#[test]
fn stop_and_a_new_order_end_the_route() {
    let mut w = world();
    let id = spawn(&mut w, SOLDIER, 5, 5);
    order(&mut w, id, (55, 5));
    run(&mut w, 30);
    assert_eq!(w.route_of(id).unwrap().status, RouteStatus::Following);
    command(&mut w, Command::Stop { ids: vec![id] });
    w.step();
    assert_eq!(w.route_of(id).unwrap().status, RouteStatus::None);
    let stopped = pos(&w, id);
    run(&mut w, 30);
    assert_eq!(pos(&w, id), stopped);
    // And the route of a new order starts from where it stands.
    order(&mut w, id, (5, 40));
    walk(&mut w, id, 800);
    assert_eq!(cell(&w, id), Cell::new(5, 40));
}

#[test]
fn a_queued_move_asks_for_its_own_route_when_it_starts() {
    let mut w = world();
    let id = spawn(&mut w, SOLDIER, 5, 5);
    let tick = w.tick();
    for (to, queued) in [((20, 5), false), ((20, 40), true), ((5, 40), true)] {
        w.schedule(
            tick,
            PlayerId(0),
            Command::Order {
                ids: vec![id],
                order: Order::Move(mid(to.0, to.1)),
                queued,
            },
        )
        .unwrap();
    }
    run(&mut w, 1500);
    assert_eq!(w.order_of(id), Some(Order::Idle));
    assert_eq!(cell(&w, id), Cell::new(5, 40));
}

#[test]
fn a_destination_between_the_middle_and_the_edge_of_its_cell_is_reached_where_it_is() {
    let mut w = world();
    let id = spawn(&mut w, SOLDIER, 5, 5);
    let goal = Vec2::new(
        Fx::from_int(55) + Fx::from_ratio(9, 10),
        Fx::from_int(5) + Fx::from_ratio(1, 10),
    );
    command(
        &mut w,
        Command::Order {
            ids: vec![id],
            order: Order::Move(goal),
            queued: false,
        },
    );
    walk(&mut w, id, 1500);
    assert_eq!(w.order_of(id), Some(Order::Idle));
    // Within the arrival tolerance of the very spot.
    assert!((pos(&w, id) - goal).length_squared() <= sf_sim::ARRIVE_TOLERANCE.square());
}

// --- what ignores the path finder ---

#[test]
fn aircraft_and_the_legacy_walkers_take_no_route() {
    let mut w = world();
    let plane = spawn(&mut w, PLANE, 5, 5);
    assert!(!w.paths_class(FLYER));
    assert!(w.paths_class(WALKER) && w.paths_class(HOVER) && w.paths_class(BOAT));
    order(&mut w, plane, (55, 5));
    w.step();
    // It moves on the very tick of its order, straight at the destination.
    assert_ne!(pos(&w, plane), mid(5, 5));
    assert_eq!(w.waypoint_of(plane), Some(mid(55, 5)));
    assert_eq!(w.route_of(plane).unwrap().status, RouteStatus::None);
    // A hut in its way does not stop it.
    place_hut(&mut w, 26, 4);
    run(&mut w, 600);
    assert_eq!(w.order_of(plane), Some(Order::Idle));

    // The legacy MoveTo goes in a straight line, into the wall, and stands.
    let soldier = spawn(&mut w, SOLDIER, 20, 5);
    command(
        &mut w,
        Command::MoveTo {
            ids: vec![soldier],
            target: mid(40, 5),
        },
    );
    run(&mut w, 600);
    assert_eq!(w.route_of(soldier).unwrap().status, RouteStatus::None);
    // The hut placed above stands in row 5 (columns 26 to 28): the soldier
    // stops at its edge, as it stops at the edge of the wall.
    assert_eq!(cell(&w, soldier), Cell::new(25, 5));
}

#[test]
fn a_world_without_a_movement_table_has_no_routes() {
    let terrain = Terrain::from_layer(MAP.0, MAP.1, &layer(), &defs()).unwrap();
    let mut w = World::with_terrain(config(), terrain)
        .unwrap()
        .with_units(table())
        .unwrap();
    assert!(!w.paths_class(WALKER));
    let id = spawn(&mut w, SOLDIER, 5, 5);
    order(&mut w, id, (55, 5));
    w.step();
    assert_eq!(w.route_of(id).unwrap().status, RouteStatus::None);
    assert_eq!(w.waypoint_of(id), Some(mid(55, 5)));
    assert_eq!(w.path_stats().peak_nodes, 0);
}

// --- structures ---

#[test]
fn a_structure_on_the_way_sends_the_unit_another_way_and_taking_it_away_restores_the_graph() {
    let mut w = world();
    // The soldier's best way east of the wall is the gap in rows 20 to 23. A hut
    // on 3 x 3 cells fills the gap's east side: the cells (32, 20) to (34, 22);
    // row 23 stays open, so the gap is a one-cell passage.
    let before = w.plan_route(WALKER, mid(20, 21), mid(40, 21)).unwrap();
    place_hut(&mut w, 32, 20);
    w.step();
    let with = w.plan_route(WALKER, mid(20, 21), mid(40, 21)).unwrap();
    assert_ne!(before, with);
    for p in &with.waypoints {
        let (x, y) = p.cell();
        assert!(
            !((32..=34).contains(&x) && (20..=22).contains(&y)),
            "a waypoint in the hut"
        );
    }
    // The structure blocks the ground classes, the boat and the hover craft
    // included (they are bound by terrain), but not the plane.
    assert!(w.is_occupied(Cell::new(33, 21)));
    // Despawn it: the graph is what it was, the plan the same.
    let hut = w.occupant_at(Cell::new(33, 21)).unwrap();
    command(&mut w, Command::Despawn { id: hut });
    w.step();
    assert!(!w.is_occupied(Cell::new(33, 21)));
    let after = w.plan_route(WALKER, mid(20, 21), mid(40, 21)).unwrap();
    assert_eq!(before, after);
}

#[test]
fn a_walker_on_its_way_is_sent_round_a_structure_placed_in_front_of_it() {
    let mut w = world();
    let id = spawn(&mut w, SOLDIER, 5, 21);
    order(&mut w, id, (50, 21));
    run(&mut w, 100);
    assert!(cell(&w, id).x < 28);
    // A hut closes most of the gap in the wall: the gap is columns 30 and 31,
    // rows 20 to 23, and the hut covers rows 20 to 22 of it. Row 23 stays open.
    place_hut(&mut w, 30, 20);
    w.step();
    assert!(w.is_occupied(Cell::new(30, 20)));
    let seen = walk(&mut w, id, 2000);
    assert_eq!(w.order_of(id), Some(Order::Idle), "it arrived");
    assert_eq!(cell(&w, id), Cell::new(50, 21));
    // It went through the one cell that was left, never through the hut.
    assert!(
        seen.iter()
            .all(|c| !((30..=32).contains(&c.x) && (20..=22).contains(&c.y)))
    );
    assert!(seen.contains(&Cell::new(30, 23)) || seen.contains(&Cell::new(31, 23)));
}

/// The repro of the review of PR 50: a hunter chases a prey that keeps moving,
/// so its route is asked for again and again; the request service is kept
/// busy by 40 other units, so the hunter waits for each new route while it
/// still follows the waypoints of the old one. A hut is placed in the gap of
/// the wall in the meantime. The hunter must never stand in the hut.
#[test]
fn a_hunter_waiting_for_a_new_route_does_not_walk_through_a_structure_placed_meanwhile() {
    let terrain = Terrain::from_layer(MAP.0, MAP.1, &layer(), &defs()).unwrap();
    let mut w = World::with_terrain(config(), terrain)
        .unwrap()
        .with_units(table())
        .unwrap()
        .with_movement(costs())
        .unwrap()
        .with_path_budget(MIN_NODE_BUDGET)
        .unwrap();
    // The busy ones first: the lowest slot is served first.
    let busy: Vec<EntityId> = (0..40)
        .map(|k| spawn(&mut w, SOLDIER, 2 + (k % 8), 30 + (k / 8)))
        .collect();
    let hunter = spawn(&mut w, GUNMAN, 4, 21);
    let prey = spawn(&mut w, SOLDIER, 20, 21);
    command(
        &mut w,
        Command::Order {
            ids: vec![hunter],
            order: Order::Attack(prey),
            queued: false,
        },
    );
    order(&mut w, prey, (60, 21));
    let hut = |c: Cell| (30..=32).contains(&c.x) && (20..=22).contains(&c.y);
    let mut placed = false;
    let mut waited_with_waypoints = 0;
    for t in 0..400_u32 {
        // Keep the service busy: new orders to far places every few ticks.
        for (k, &id) in busy.iter().enumerate() {
            if t >= 30 && (t + k as u32).is_multiple_of(10) {
                let far = if (t / 10 + k as u32).is_multiple_of(2) {
                    (58, 44)
                } else {
                    (3, 40)
                };
                order(&mut w, id, far);
            }
        }
        let mut just_placed = false;
        if !placed && cell(&w, hunter).x >= 22 && w.route_of(hunter).unwrap().waypoints > 0 {
            place_hut(&mut w, 30, 20);
            placed = true;
            just_placed = true;
        }
        w.step();
        if just_placed {
            // The old waypoints lead to the gap: they are gone on the tick the
            // map changed, not only when the hunter would step into the hut.
            assert_eq!(w.route_of(hunter).unwrap().waypoints, 0);
        }
        if placed {
            let view = w.route_of(hunter).unwrap();
            if view.status == RouteStatus::Waiting || view.waypoints > 0 {
                waited_with_waypoints += 1;
            }
            assert!(
                !hut(cell(&w, hunter)),
                "tick {t}: the hunter stands in the hut at {:?}",
                cell(&w, hunter)
            );
        }
    }
    assert!(placed, "the hunter never got near the gap");
    assert!(waited_with_waypoints > 0);
    assert!(
        w.is_occupied(Cell::new(31, 21)),
        "the hut stands in the gap"
    );
}

#[test]
fn a_structure_far_from_a_route_does_not_disturb_it() {
    let mut w = world();
    let id = spawn(&mut w, SOLDIER, 5, 5);
    order(&mut w, id, (25, 5));
    run(&mut w, 20);
    assert_eq!(w.route_of(id).unwrap().status, RouteStatus::Following);
    // A hut in the south-east corner, nowhere near.
    place_hut(&mut w, 60, 44);
    for _ in 0..300 {
        w.step();
        let status = w.route_of(id).unwrap().status;
        assert!(
            status != RouteStatus::Waiting,
            "the route was asked for again"
        );
        if w.order_of(id) == Some(Order::Idle) {
            break;
        }
    }
    assert_eq!(w.order_of(id), Some(Order::Idle));
}

#[test]
fn a_unit_with_a_structure_placed_on_it_walks_out_of_it() {
    let mut w = world();
    let id = spawn(&mut w, SOLDIER, 10, 10);
    // The hut covers (10, 10) to (12, 12), the soldier's cell, its corner,
    // included.
    place_hut(&mut w, 10, 10);
    w.step();
    assert!(w.is_occupied(Cell::new(10, 10)));
    order(&mut w, id, (20, 10));
    walk(&mut w, id, 800);
    assert_eq!(w.order_of(id), Some(Order::Idle));
    assert_eq!(cell(&w, id), Cell::new(20, 10));
}

#[test]
fn a_unit_walled_in_by_a_structure_stands_where_it_is_with_its_order() {
    // The middle of a 3 x 3 hut has no open neighbour: nothing can be reached
    // from there, the nearest reachable cell is the unit's own. Getting it out is
    // the collision work package's (a stuck unit goes to a free neighbour cell).
    let mut w = world();
    let id = spawn(&mut w, SOLDIER, 10, 10);
    place_hut(&mut w, 9, 9);
    w.step();
    order(&mut w, id, (20, 10));
    run(&mut w, 100);
    let route = w.route_of(id).unwrap();
    assert_eq!(route.status, RouteStatus::Stranded);
    assert!(route.unreachable);
    assert_eq!(cell(&w, id), Cell::new(10, 10));
    assert_eq!(w.order_of(id), Some(Order::Move(mid(20, 10))));
}

#[test]
fn structures_that_come_and_go_away_from_a_route_change_no_plan_and_a_unit_on_it_goes_on() {
    // The graph is rebuilt by the tick when a structure is placed or removed, and
    // there is no other way to make it: no public call changes it.
    let mut w = world();
    let id = spawn(&mut w, SOLDIER, 5, 5);
    let plan = w.plan_route(WALKER, mid(5, 5), mid(55, 5)).unwrap();
    for (x, y) in [(0, 40), (50, 42), (58, 44)] {
        place_hut(&mut w, x, y);
        w.step();
        let hut = w.occupant_at(Cell::new(x, y)).unwrap();
        command(&mut w, Command::Despawn { id: hut });
        w.step();
    }
    assert_eq!(w.plan_route(WALKER, mid(5, 5), mid(55, 5)).unwrap(), plan);
    assert!(w.path_stats().sectors_rebuilt >= 6);
    // A unit mid-route when a hut is placed in its sector asks again and arrives.
    order(&mut w, id, (55, 5));
    run(&mut w, 60);
    place_hut(&mut w, 10, 10);
    walk(&mut w, id, 1500);
    assert_eq!(w.order_of(id), Some(Order::Idle));
    assert_eq!(cell(&w, id), Cell::new(55, 5));
}

// --- the budget ---

#[test]
fn the_service_serves_in_the_order_of_the_ids_and_never_exceeds_its_budget() {
    let terrain = Terrain::from_layer(MAP.0, MAP.1, &layer(), &defs()).unwrap();
    let mut w = World::with_terrain(config(), terrain)
        .unwrap()
        .with_units(table())
        .unwrap()
        .with_movement(costs())
        .unwrap()
        .with_path_budget(MIN_NODE_BUDGET)
        .unwrap()
        // This is about the queue of single requests: no flowfield for the crowd.
        .with_flowfield_threshold(0)
        .unwrap();
    let ids: Vec<EntityId> = (0..24)
        .map(|k| spawn(&mut w, SOLDIER, 4 + k % 4, 4))
        .collect();
    // All of them to the island's shore on the far side of the wall and river.
    let tick = w.tick();
    w.schedule(
        tick,
        PlayerId(0),
        Command::Order {
            ids: ids.clone(),
            order: Order::Move(mid(58, 42)),
            queued: false,
        },
    )
    .unwrap();
    let mut first_route = vec![None; ids.len()];
    for t in 0..400_u64 {
        w.step();
        let stats = w.path_stats();
        assert!(stats.nodes_last_tick <= MIN_NODE_BUDGET, "tick {t}");
        for (k, id) in ids.iter().enumerate() {
            if first_route[k].is_none() && w.route_of(*id).unwrap().status == RouteStatus::Following
            {
                first_route[k] = Some(t);
            }
        }
    }
    let served: Vec<u64> = first_route.into_iter().map(|t| t.unwrap()).collect();
    assert!(served.windows(2).all(|p| p[0] <= p[1]), "{served:?}");
    assert!(
        served[23] > served[0],
        "the budget made them queue: {served:?}"
    );
    assert!(w.path_stats().peak_nodes > 0);
}

#[test]
fn the_budget_is_checked_when_it_is_given() {
    let terrain = || Terrain::from_layer(MAP.0, MAP.1, &layer(), &defs()).unwrap();
    let fresh = || World::with_terrain(config(), terrain()).unwrap();
    assert_eq!(
        fresh().with_path_budget(MIN_NODE_BUDGET - 1).err(),
        Some(ConfigError::PathBudget)
    );
    assert!(fresh().with_path_budget(MIN_NODE_BUDGET).is_ok());
    let mut started = fresh();
    started.step();
    assert_eq!(
        started.with_path_budget(5000).err(),
        Some(ConfigError::PathBudget)
    );
}

#[test]
fn a_unit_removed_while_its_route_is_being_made_does_not_stall_the_others() {
    let terrain = Terrain::from_layer(MAP.0, MAP.1, &layer(), &defs()).unwrap();
    let mut w = World::with_terrain(config(), terrain)
        .unwrap()
        .with_units(table())
        .unwrap()
        .with_movement(costs())
        .unwrap()
        .with_path_budget(MIN_NODE_BUDGET)
        .unwrap();
    let a = spawn(&mut w, SOLDIER, 4, 4);
    let b = spawn(&mut w, SOLDIER, 5, 4);
    let tick = w.tick();
    w.schedule(
        tick,
        PlayerId(0),
        Command::Order {
            ids: vec![a, b],
            order: Order::Move(mid(58, 42)),
            queued: false,
        },
    )
    .unwrap();
    // The first tick asks; the next serve a, which needs several ticks.
    run(&mut w, 2);
    command(&mut w, Command::Despawn { id: a });
    // A new unit takes the slot at once.
    run(&mut w, 1);
    let tick = w.tick();
    w.schedule(
        tick,
        PlayerId(0),
        Command::SpawnUnit {
            kind: SOLDIER,
            pos: mid(6, 4),
            owner: PlayerId(0),
        },
    )
    .unwrap();
    w.step();
    let c = EntityId::new(a.index(), a.generation() + 1);
    assert!(w.is_alive(c) && !w.is_alive(a));
    assert_eq!(w.route_of(c).unwrap().status, RouteStatus::None);
    order(&mut w, c, (6, 20));
    walk(&mut w, c, 800);
    assert_eq!(cell(&w, c), Cell::new(6, 20));
    walk(&mut w, b, 2500);
    assert_eq!(w.order_of(b), Some(Order::Idle));
    assert_eq!(cell(&w, b), Cell::new(58, 42));
}

// --- more of the rules ---

#[test]
fn a_clone_in_the_middle_of_a_run_goes_on_exactly_as_the_original() {
    let mut w = world();
    let ids: Vec<EntityId> = (0..6).map(|k| spawn(&mut w, SOLDIER, 4 + k, 5)).collect();
    for (k, id) in ids.iter().enumerate() {
        order(&mut w, *id, (50 + k as i32, 40));
    }
    run(&mut w, 37);
    place_hut(&mut w, 33, 21);
    run(&mut w, 11);
    let mut copy = w.clone();
    assert!(copy == w);
    for _ in 0..300 {
        let (a, b) = (w.step(), copy.step());
        assert_eq!(a.checksum, b.checksum);
    }
    assert!(copy == w);
}

#[test]
fn a_chase_follows_a_target_across_the_bridge() {
    let mut w = world();
    let hunter = spawn(&mut w, GUNMAN, 38, 6);
    let prey = spawn(&mut w, SOLDIER, 40, 12);
    // The prey walks to the east bank over the bridge (legacy straight walk to
    // the bridge row first: it just walks east along row 10).
    command(
        &mut w,
        Command::MoveTo {
            ids: vec![prey],
            target: mid(43, 10),
        },
    );
    command(
        &mut w,
        Command::Order {
            ids: vec![hunter],
            order: Order::Attack(prey),
            queued: false,
        },
    );
    run(&mut w, 400);
    // The hunter ends within its reach (5 cells and a half of the prey's size)
    // of the prey, which stands still on the west bank.
    let gap = (pos(&w, hunter) - pos(&w, prey)).length();
    assert!(gap <= Fx::from_ratio(53, 10), "{gap:?}");
    // Now the prey is across the river: ordered there by a route over the bridge.
    order(&mut w, prey, (52, 5));
    run(&mut w, 1200);
    let gap = (pos(&w, hunter) - pos(&w, prey)).length();
    assert!(gap <= Fx::from_ratio(53, 10), "{gap:?}");
    assert!(cell(&w, hunter).x >= 44, "{:?}", cell(&w, hunter));
}

#[test]
fn a_fast_unit_that_turns_slowly_still_arrives_and_never_stands_in_rock() {
    // 0.4 of a cell per tick and 6 of 256 facing steps of turning: at a corner
    // it overshoots by more than the margin a smoothed route keeps from rock.
    // A step refused on a smoothed route makes the route cell by cell; a step
    // refused on that stands the unit still. The unit must arrive, not stand.
    let mut w = world();
    let id = spawn(&mut w, SPRINTER, 5, 5);
    order(&mut w, id, (55, 5));
    let seen = walk(&mut w, id, 3000);
    assert!(seen.iter().all(|c| w.passable(WALKER, *c)));
    assert_eq!(w.order_of(id), Some(Order::Idle), "it arrived");
    assert_eq!(cell(&w, id), Cell::new(55, 5));
}

#[test]
fn a_gunboat_sails_the_river_under_the_bridge_and_the_route_ends_with_the_order() {
    let mut w = world();
    let id = spawn(&mut w, GUNBOAT, 45, 2);
    order(&mut w, id, (46, 45));
    let seen = walk(&mut w, id, 1500);
    assert_eq!(w.order_of(id), Some(Order::Idle));
    assert!(seen.contains(&Cell::new(45, 10)), "under the bridge");
    assert_eq!(cell(&w, id), Cell::new(46, 45));
    assert_eq!(w.route_of(id).unwrap().status, RouteStatus::None);
}

#[test]
fn the_budget_and_the_routes_are_in_the_checksum() {
    // The same crowd on two budgets: the routes arrive on other ticks, and the
    // checksums say so from the tick where they first differ.
    let make = |budget: u32| {
        let terrain = Terrain::from_layer(MAP.0, MAP.1, &layer(), &defs()).unwrap();
        let mut w = World::with_terrain(config(), terrain)
            .unwrap()
            .with_units(table())
            .unwrap()
            .with_movement(costs())
            .unwrap()
            .with_path_budget(budget)
            .unwrap();
        let ids: Vec<EntityId> = (0..8).map(|k| spawn(&mut w, SOLDIER, 4 + k, 4)).collect();
        let tick = w.tick();
        w.schedule(
            tick,
            PlayerId(0),
            Command::Order {
                ids,
                order: Order::Move(mid(58, 42)),
                queued: false,
            },
        )
        .unwrap();
        run(&mut w, 40);
        w
    };
    let (small, large) = (make(MIN_NODE_BUDGET), make(4 * MIN_NODE_BUDGET));
    assert_ne!(small.checksum(), large.checksum());
    // And the same budget gives the same history.
    assert_eq!(
        make(MIN_NODE_BUDGET).checksum_history(),
        small.checksum_history()
    );
}

// --- a crowd on a random map ---

/// A 72 by 56 map of clumps of rock and water, a random crowd with random orders
/// (some to places that cannot be reached), structures placed and removed at
/// random, on a small budget: the service never overspends, never leaves a unit
/// waiting for ever, no unit ends a tick on ground its class cannot enter, and the
/// same run twice is the same run.
fn crowd(seed: u64) -> (Vec<u64>, u32) {
    use sf_math::Rng;
    let (w, h) = (72_u32, 56_u32);
    let mut rng = Rng::new(seed);
    let mut ids = vec![GRASS.0; (w * h) as usize];
    for _ in 0..70 {
        let (x, y) = (rng.below(w), rng.below(h));
        let (cw, ch) = (1 + rng.below(5), 1 + rng.below(5));
        let kind = if rng.chance(1, 4) { WATER.0 } else { ROCK.0 };
        for dy in 0..ch {
            for dx in 0..cw {
                if x + dx < w && y + dy < h {
                    ids[((y + dy) * w + x + dx) as usize] = kind;
                }
            }
        }
    }
    let terrain = Terrain::from_layer(w, h, &ids, &defs()).unwrap();
    let mut world = World::with_terrain(WorldConfig::new(seed).with_map_size(w, h), terrain)
        .unwrap()
        .with_units(table())
        .unwrap()
        .with_movement(costs())
        .unwrap()
        .with_path_budget(MIN_NODE_BUDGET)
        .unwrap();
    let open = |c: Cell| ids[(c.y * w + c.x) as usize] == GRASS.0;
    let mut units = Vec::new();
    while units.len() < 30 {
        let (x, y) = (rng.below(w), rng.below(h));
        if open(Cell::new(x, y)) {
            let kind = if units.len() % 6 == 5 { FERRY } else { SOLDIER };
            units.push(spawn(&mut world, kind, x as i32, y as i32));
        }
    }
    let mut waiting_for = vec![0_u32; units.len()];
    let mut worst_wait = 0;
    let mut huts: Vec<EntityId> = Vec::new();
    for t in 0..1200_u32 {
        if t % 40 == 0 {
            // A few units get an order to a random cell, reachable or not.
            for id in &units {
                if rng.chance(1, 4) {
                    order(&mut world, *id, (rng.below(w) as i32, rng.below(h) as i32));
                }
            }
        }
        if t % 90 == 30 {
            place_hut(&mut world, rng.below(w - 3), rng.below(h - 3));
        }
        if t % 90 == 75
            && let Some(hut) = huts.pop()
        {
            command(&mut world, Command::Despawn { id: hut });
        }
        let report = world.step();
        let _ = report;
        // Collect the huts that stand (the last slots), for later removal.
        huts = (0..world.slot_count() as u32)
            .map(|k| EntityId::new(k, 0))
            .filter(|id| world.footprint_of(*id).is_some())
            .collect();
        assert!(world.path_stats().nodes_last_tick <= MIN_NODE_BUDGET);
        // Whenever a request is left waiting, the tick's budget was spent: what a
        // tick leaves unused is at most one line of sight, which is not cut in two.
        let stats = world.path_stats();
        if stats.waiting > 0 {
            assert!(
                u64::from(stats.nodes_last_tick) * 100 >= u64::from(MIN_NODE_BUDGET) * 80,
                "seed {seed}, tick {t}: {} units waiting and only {} of {} nodes used",
                stats.waiting,
                stats.nodes_last_tick,
                MIN_NODE_BUDGET
            );
        }
        for (k, id) in units.iter().enumerate() {
            let class = world.stats_of(*id).unwrap().movement_class.unwrap();
            let c = cell(&world, *id);
            // A unit that a hut was placed on stands in a blocked cell; any other
            // is where its class may be.
            assert!(
                world.passable(class, c) || world.is_occupied(c),
                "unit {k} stands on {c} (class {class}) after tick {t}"
            );
            let view = world.route_of(*id).unwrap();
            waiting_for[k] = if view.status == RouteStatus::Waiting {
                waiting_for[k] + 1
            } else {
                0
            };
            worst_wait = worst_wait.max(waiting_for[k]);
        }
    }
    (world.checksum_history().to_vec(), worst_wait)
}

#[test]
fn a_crowd_on_a_random_map_with_structures_comes_and_goes_without_stalling() {
    for seed in [1, 2, 3] {
        let (history, worst_wait) = crowd(seed);
        // 30 units on the smallest budget: service goes round the slots, so
        // nobody waits for the whole queue twice, and the budget is spent to the
        // line of sight while anyone waits. Measured: 32, 33 and 36 ticks (the
        // strict-id queue before this gave 130, 189 and 351).
        assert!(
            worst_wait < 60,
            "seed {seed}: a unit waited {worst_wait} ticks"
        );
        // And the same run again is the same run.
        assert_eq!(crowd(seed).0, history, "seed {seed}");
    }
}
