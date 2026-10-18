//! Flowfields for groups (WP4.3), through the public API: a single order that
//! sends enough units of one class to one place gets one field instead of a
//! route each, the units arrive spread round the goal, the build shares the
//! node budget with the routes, and a structure in the way rebuilds the field.
//! The rules are in `docs/rules/pathfinding.md`; the parts of the machinery
//! (integration, directions, corridor, slots) are tested in `src/path/`; the
//! headless scenario `bridge_200` has the big crossing.

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

// --- helpers of this file ---

/// `n` units on a block of open ground starting at `(x, y)`, `per_row` to a row.
fn block(w: &mut World, kind: Kind, n: i32, x: i32, y: i32, per_row: i32) -> Vec<EntityId> {
    (0..n)
        .map(|k| spawn(w, kind, x + k % per_row, y + k / per_row))
        .collect()
}

fn group_order(w: &mut World, ids: &[EntityId], to: (i32, i32)) {
    command(
        w,
        Command::Order {
            ids: ids.to_vec(),
            order: Order::Move(mid(to.0, to.1)),
            queued: false,
        },
    );
}

fn all_idle(w: &World, ids: &[EntityId]) -> bool {
    ids.iter().all(|id| w.order_of(*id) == Some(Order::Idle))
}

/// Steps until every unit has no order, at most `limit` ticks, asserting each
/// tick that the budget holds and that no unit ends it where its class cannot
/// be. Returns the ticks it took.
fn run_group(w: &mut World, ids: &[EntityId], limit: u32) -> u32 {
    let budget = w.path_stats().budget;
    for t in 0..limit {
        w.step();
        assert!(w.path_stats().nodes_last_tick <= budget, "tick {t}");
        for id in ids {
            let class = w.stats_of(*id).unwrap().movement_class.unwrap();
            assert!(w.passable(class, cell(w, *id)), "tick {t}: {id:?}");
        }
        if all_idle(w, ids) {
            return t + 1;
        }
    }
    panic!("not all arrived in {limit} ticks");
}

fn pos_all(w: &World, ids: &[EntityId]) -> Vec<Vec2> {
    ids.iter().map(|id| pos(w, *id)).collect()
}

fn fields_in_use(w: &World) -> u32 {
    w.path_stats().fields_live
}

fn between(a: Vec2, b: Vec2) -> Fx {
    (a - b).length()
}

/// A world with a path budget and, unless `None`, a flowfield threshold.
fn world_with(budget: u32, threshold: Option<u32>) -> World {
    let terrain = Terrain::from_layer(MAP.0, MAP.1, &layer(), &defs()).unwrap();
    let mut w = World::with_terrain(config(), terrain)
        .unwrap()
        .with_units(table())
        .unwrap()
        .with_movement(costs())
        .unwrap()
        .with_path_budget(budget)
        .unwrap();
    if let Some(t) = threshold {
        w = w.with_flowfield_threshold(t).unwrap();
    }
    w
}

// --- when a group gets a field ---

#[test]
fn eight_soldiers_get_a_field_and_seven_do_not() {
    let mut w = world();
    assert_eq!(w.flowfield_threshold(), sf_sim::DEFAULT_FLOWFIELD_THRESHOLD);
    let seven = block(&mut w, SOLDIER, 7, 4, 4, 4);
    group_order(&mut w, &seven, (20, 30));
    w.step();
    for id in &seven {
        assert!(!w.route_of(*id).unwrap().field);
    }
    assert_eq!(fields_in_use(&w), 0);
    let t = run_group(&mut w, &seven, 1500);
    assert!(t > 0);
    assert!(w.path_stats().jobs_done >= 7, "seven routes of their own");
    assert_eq!(w.path_stats().fields_built, 0);

    let mut w = world();
    let eight = block(&mut w, SOLDIER, 8, 4, 4, 4);
    group_order(&mut w, &eight, (20, 30));
    w.step();
    for id in &eight {
        let route = w.route_of(*id).unwrap();
        assert!(route.field && route.complete);
    }
    assert_eq!(fields_in_use(&w), 1);
    run_group(&mut w, &eight, 1500);
    assert_eq!(w.path_stats().fields_built, 1);
    assert_eq!(w.path_stats().jobs_done, 0, "not one route of their own");
}

#[test]
fn the_threshold_is_data_and_zero_turns_fields_off() {
    let mut w = world_with(4096, Some(3));
    let ids = block(&mut w, SOLDIER, 3, 4, 4, 3);
    group_order(&mut w, &ids, (20, 30));
    w.step();
    assert!(w.route_of(ids[0]).unwrap().field, "three make a group at 3");

    let mut w = world_with(4096, Some(0));
    let ids = block(&mut w, SOLDIER, 40, 4, 4, 8);
    group_order(&mut w, &ids, (20, 30));
    w.step();
    assert!(!w.route_of(ids[0]).unwrap().field, "off");
    assert_eq!(fields_in_use(&w), 0);
}

#[test]
fn the_threshold_is_checked_when_it_is_given() {
    let fresh = || world();
    assert_eq!(
        fresh().with_flowfield_threshold(1).err(),
        Some(ConfigError::FlowfieldThreshold)
    );
    assert!(fresh().with_flowfield_threshold(0).is_ok());
    assert!(fresh().with_flowfield_threshold(2).is_ok());
    let mut started = fresh();
    started.step();
    assert_eq!(
        started.with_flowfield_threshold(8).err(),
        Some(ConfigError::FlowfieldThreshold)
    );
}

#[test]
fn a_group_arrives_spread_round_the_goal_inside_five_cells() {
    let mut w = world();
    let ids = block(&mut w, SOLDIER, 60, 2, 2, 12);
    group_order(&mut w, &ids, (20, 38));
    let ticks = run_group(&mut w, &ids, 2500);
    let goal = mid(20, 38);
    let spots = pos_all(&w, &ids);
    // Everybody within five cells of the goal, and nobody on top of another
    // (no two units are in the same place).
    for p in &spots {
        assert!(between(*p, goal) <= Fx::from_int(5), "{p:?}");
    }
    let mut sorted: Vec<(i64, i64)> = spots.iter().map(|p| (p.x.raw(), p.y.raw())).collect();
    sorted.sort_unstable();
    sorted.dedup();
    assert!(sorted.len() >= 58, "{} distinct places of 60", sorted.len());
    assert_eq!(w.path_stats().jobs_done, 0);
    eprintln!("60 soldiers arrived after {ticks} ticks");
}

#[test]
fn units_are_given_their_slots_by_entity_id_whatever_the_order_of_the_command() {
    let finals = |reverse: bool| {
        let mut w = world();
        let ids = block(&mut w, SOLDIER, 12, 4, 4, 4);
        let mut listed = ids.clone();
        if reverse {
            listed.reverse();
        }
        group_order(&mut w, &listed, (20, 30));
        run_group(&mut w, &ids, 1500);
        pos_all(&w, &ids)
    };
    assert_eq!(finals(false), finals(true));
}

#[test]
fn mixed_classes_get_a_field_each_and_the_class_below_the_threshold_goes_alone() {
    let mut w = world();
    let mut ids = block(&mut w, SOLDIER, 10, 4, 4, 5);
    let ferries = block(&mut w, FERRY, 9, 4, 8, 5);
    let few = block(&mut w, SPRINTER, 3, 4, 12, 3);
    // SPRINTER is a walker too: it counts toward the walkers (13 of them).
    ids.extend(&ferries);
    ids.extend(&few);
    group_order(&mut w, &ids, (55, 5));
    w.step();
    assert_eq!(fields_in_use(&w), 2, "walkers and hover craft");
    for id in ids.iter() {
        assert!(w.route_of(*id).unwrap().field);
    }
    run_group(&mut w, &ids, 3000);
    assert_eq!(w.path_stats().fields_built, 2);
    assert_eq!(w.path_stats().jobs_done, 0);
    // The two classes stand in different places.
    let spots = pos_all(&w, &ids);
    let mut sorted: Vec<(i64, i64)> = spots.iter().map(|p| (p.x.raw(), p.y.raw())).collect();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "no two on one spot");
    for p in spots {
        assert!(between(p, mid(55, 5)) <= Fx::from_int(5));
    }

    // Three ferries are too few: they are routed on their own.
    let mut w = world();
    let walkers = block(&mut w, SOLDIER, 10, 4, 4, 5);
    let three = block(&mut w, FERRY, 3, 4, 8, 3);
    let mut all = walkers.clone();
    all.extend(&three);
    group_order(&mut w, &all, (55, 5));
    w.step();
    assert_eq!(fields_in_use(&w), 1);
    assert!(w.route_of(walkers[0]).unwrap().field);
    assert!(!w.route_of(three[0]).unwrap().field);
    run_group(&mut w, &all, 3000);
    assert!(w.path_stats().jobs_done > 0, "the ferries' own routes");
}

#[test]
fn which_orders_make_a_group() {
    // Attack-move to a point: yes. Guard of a point: no, it returns to its
    // post and a post is a point. A queued order that starts now: yes; one
    // behind a walking order: it starts later, on its own route.
    let attack_move = || {
        let mut w = world();
        let ids = block(&mut w, SOLDIER, 8, 4, 4, 4);
        command(
            &mut w,
            Command::Order {
                ids: ids.clone(),
                order: Order::AttackMove(mid(20, 30)),
                queued: false,
            },
        );
        w.step();
        (w, ids)
    };
    let (w, ids) = attack_move();
    assert!(w.route_of(ids[0]).unwrap().field);

    let mut w = world();
    let ids = block(&mut w, SOLDIER, 8, 4, 4, 4);
    command(
        &mut w,
        Command::Order {
            ids: ids.clone(),
            order: Order::Guard(sf_sim::GuardTarget::Point(mid(20, 30))),
            queued: false,
        },
    );
    w.step();
    assert!(!w.route_of(ids[0]).unwrap().field, "guard");

    let mut w = world();
    let ids = block(&mut w, SOLDIER, 8, 4, 4, 4);
    command(
        &mut w,
        Command::Order {
            ids: ids.clone(),
            order: Order::Move(mid(20, 30)),
            queued: true,
        },
    );
    w.step();
    assert!(w.route_of(ids[0]).unwrap().field, "queued on idle units");
    // Now they walk: a queued order behind it starts later.
    command(
        &mut w,
        Command::Order {
            ids: ids.clone(),
            order: Order::Move(mid(10, 40)),
            queued: true,
        },
    );
    w.step();
    assert_eq!(w.queued_orders_of(ids[0]).unwrap().len(), 1);
    run_group(&mut w, &ids[..0], 1);
    for _ in 0..1500 {
        w.step();
        if all_idle(&w, &ids) {
            break;
        }
    }
    assert!(all_idle(&w, &ids), "both legs done");
    assert!(
        between(pos(&w, ids[0]), mid(10, 40)) < Fx::from_int(1),
        "the second leg on its own route"
    );
}

#[test]
fn a_new_order_or_a_stop_ends_the_membership() {
    let mut w = world();
    let ids = block(&mut w, SOLDIER, 12, 4, 4, 4);
    group_order(&mut w, &ids, (20, 30));
    run(&mut w, 30);
    assert!(w.route_of(ids[0]).unwrap().field);
    command(&mut w, Command::Stop { ids: vec![ids[0]] });
    order(&mut w, ids[1], (4, 40));
    w.step();
    assert!(!w.route_of(ids[0]).unwrap().field);
    assert_eq!(w.route_of(ids[0]).unwrap().status, RouteStatus::None);
    assert!(!w.route_of(ids[1]).unwrap().field, "its own route now");
    assert!(w.route_of(ids[2]).unwrap().field, "the rest go on");
    run_group(&mut w, &ids[1..], 2500);
    assert_eq!(w.order_of(ids[0]), Some(Order::Idle));
}

// --- units the field cannot serve ---

#[test]
fn a_member_cut_off_from_the_goal_falls_back_to_the_nearest_reachable_rule() {
    let mut w = world();
    // Seven on the mainland and one on the island of the lake, which a walker
    // cannot leave.
    let mut ids = block(&mut w, SOLDIER, 7, 4, 4, 4);
    let island = spawn(&mut w, SOLDIER, 55, 31);
    ids.push(island);
    group_order(&mut w, &ids, (20, 30));
    w.step();
    assert!(w.route_of(ids[0]).unwrap().field, "the others are members");
    // The field is built in this one tick and has no cell for the island: the
    // soldier there is routed on its own from the start.
    assert!(!w.route_of(island).unwrap().field);
    for _ in 0..800 {
        w.step();
    }
    let route = w.route_of(island).unwrap();
    assert_eq!(route.status, RouteStatus::Stranded);
    assert!(route.unreachable);
    // The island's own shore, nearest the goal.
    assert!(cell(&w, island).x >= 54 && cell(&w, island).x <= 56);
    // The seven arrive at their slots as usual.
    assert!(all_idle(&w, &ids[..7]));
}

#[test]
fn a_goal_the_class_cannot_enter_gets_no_field_and_each_unit_goes_to_the_nearest_cell() {
    let mut w = world();
    let ids = block(&mut w, SOLDIER, 10, 33, 30, 5);
    // The middle of the lake.
    group_order(&mut w, &ids, (55, 28));
    w.step();
    assert_eq!(fields_in_use(&w), 0);
    for id in &ids {
        assert!(!w.route_of(*id).unwrap().field);
    }
    run(&mut w, 1200);
    for id in &ids {
        let route = w.route_of(*id).unwrap();
        assert!(route.unreachable);
        assert_eq!(route.status, RouteStatus::Stranded);
    }
}

// --- the budget ---

#[test]
fn the_field_and_single_routes_share_the_budget_and_both_get_served() {
    // The smallest budget; a group of ten, and twelve single orders asked in
    // the same tick.
    let mut w = world_with(MIN_NODE_BUDGET, None);
    let group = block(&mut w, SOLDIER, 10, 2, 2, 5);
    let singles = block(&mut w, GUNMAN, 12, 2, 10, 6);
    group_order(&mut w, &group, (20, 40));
    for (k, id) in singles.iter().enumerate() {
        order(&mut w, *id, (35 + (k as i32 % 6), 5 + k as i32));
    }
    let mut first_route = vec![None; singles.len()];
    let mut field_ready = None;
    let mut spent_while_waiting = 0;
    let mut ticks_waiting = 0;
    for t in 0..800_u64 {
        w.step();
        let s = w.path_stats();
        assert!(s.nodes_last_tick <= MIN_NODE_BUDGET, "tick {t}");
        let working = s.waiting > 0 || (s.fields_live > 0 && s.fields_built == 0);
        if working && t > 1 {
            ticks_waiting += 1;
            spent_while_waiting += u64::from(s.nodes_last_tick);
            // The one atom, a line of sight (60 nodes at most), is all that can
            // be left over; and some ticks start a stage that does not fit.
            assert!(
                s.nodes_last_tick >= MIN_NODE_BUDGET * 3 / 4,
                "tick {t}: only {} of {MIN_NODE_BUDGET} spent while work waits ({s:?})",
                s.nodes_last_tick
            );
        }
        if field_ready.is_none() && s.fields_built == 1 {
            field_ready = Some(t);
        }
        for (k, id) in singles.iter().enumerate() {
            if first_route[k].is_none() && w.route_of(*id).unwrap().status != RouteStatus::Waiting {
                first_route[k] = Some(t);
            }
        }
    }
    assert!(ticks_waiting > 5);
    eprintln!(
        "budget shared: {ticks_waiting} ticks with work waiting, {} nodes a tick on average; \
         field ready on tick {field_ready:?}, singles served by {:?}",
        spent_while_waiting / ticks_waiting.max(1),
        first_route.iter().flatten().max()
    );
    // The field was built, and the single requests were served in the order of
    // the ids, as without a field.
    assert!(field_ready.is_some());
    let served: Vec<u64> = first_route.into_iter().map(|t| t.unwrap()).collect();
    assert!(served.windows(2).all(|p| p[0] <= p[1]), "{served:?}");
    assert!(all_idle(&w, &group) || w.order_of(group[0]).is_some());
}

#[test]
fn the_budget_decides_when_the_group_arrives_and_never_where() {
    let finals = |budget: u32| {
        let mut w = world_with(budget, None);
        let ids = block(&mut w, SOLDIER, 20, 2, 2, 10);
        group_order(&mut w, &ids, (20, 40));
        let ticks = run_group(&mut w, &ids, 3000);
        (pos_all(&w, &ids), ticks)
    };
    let (small, t_small) = finals(MIN_NODE_BUDGET);
    let (large, t_large) = finals(8 * MIN_NODE_BUDGET);
    assert_eq!(small, large);
    assert!(t_small >= t_large);
}

// --- the pool ---

#[test]
fn more_groups_than_the_pool_holds_still_all_arrive() {
    // Ten groups of eight to ten different places: the pool keeps eight fields;
    // the units of the others go on alone.
    let mut w = world();
    let mut groups = Vec::new();
    for g in 0..10_i32 {
        let ids = block(&mut w, SOLDIER, 8, 2 + (g % 5) * 5, 2 + (g / 5) * 5, 4);
        groups.push(ids);
    }
    for (g, ids) in groups.iter().enumerate() {
        group_order(&mut w, ids, (6 + 2 * g as i32, 40));
    }
    w.step();
    assert!(fields_in_use(&w) as usize <= sf_sim::FIELD_POOL);
    let all: Vec<EntityId> = groups.iter().flatten().copied().collect();
    run_group(&mut w, &all, 4000);
    assert!(w.path_stats().fields_built >= 8);
    assert!(
        w.path_stats().jobs_done > 0,
        "the units of the evicted fields went on alone"
    );
}

#[test]
fn a_field_expires_when_its_group_has_arrived() {
    let mut w = world();
    let ids = block(&mut w, SOLDIER, 8, 4, 4, 4);
    group_order(&mut w, &ids, (20, 30));
    run_group(&mut w, &ids, 1500);
    assert_eq!(fields_in_use(&w), 1);
    run(&mut w, sf_sim::FIELD_IDLE_TICKS + 5);
    assert_eq!(fields_in_use(&w), 0);
}

// --- a change of the map ---

#[test]
fn a_structure_placed_across_the_way_rebuilds_the_field_and_nobody_steps_into_it() {
    let mut w = world();
    // Eight walkers cross the gap of the wall and the bridge to the far bank.
    let ids = block(&mut w, SOLDIER, 8, 2, 2, 4);
    group_order(&mut w, &ids, (55, 5));
    run(&mut w, 120);
    assert_eq!(w.path_stats().fields_built, 1);
    // A hut on the middle of the bridge's approach, in the region of the field.
    place_hut(&mut w, 38, 8);
    let mut hut_cells = Vec::new();
    for t in 0..2500_u32 {
        w.step();
        if hut_cells.is_empty() {
            hut_cells = (0..64)
                .flat_map(|x| (0..48).map(move |y| Cell::new(x, y)))
                .filter(|c| w.is_occupied(*c))
                .collect();
        }
        for id in &ids {
            let c = cell(&w, *id);
            assert!(w.passable(WALKER, c), "tick {t}: {id:?} in {c:?}");
            assert!(!w.is_occupied(c), "tick {t}: {id:?} in the hut at {c:?}");
        }
        if all_idle(&w, &ids) {
            break;
        }
    }
    assert!(all_idle(&w, &ids), "they arrived");
    assert!(!hut_cells.is_empty(), "the hut stands");
    assert!(
        w.path_stats().fields_built >= 2,
        "the field was built again: {:?}",
        w.path_stats()
    );
    for id in &ids {
        assert!(between(pos(&w, *id), mid(55, 5)) <= Fx::from_int(5));
    }
}

#[test]
fn a_structure_taken_away_builds_the_field_again_and_the_group_still_arrives() {
    let mut w = world();
    let ids = block(&mut w, SOLDIER, 8, 2, 2, 4);
    group_order(&mut w, &ids, (55, 5));
    let built = |w: &World| w.path_stats().fields_built;
    // Until the first field is there.
    for _ in 0..50 {
        if built(&w) == 1 {
            break;
        }
        w.step();
    }
    assert_eq!(built(&w), 1);
    // Place a hut in the region, in the way of the bridge; wait for the second.
    let hut = EntityId::new(w.slot_count() as u32, 0);
    place_hut(&mut w, 38, 8);
    for _ in 0..50 {
        w.step();
        if built(&w) == 2 {
            break;
        }
    }
    assert_eq!(built(&w), 2, "built again for the structure");
    assert!(w.is_occupied(Cell::new(38, 8)));
    assert!(!all_idle(&w, &ids), "still walking");
    // Take it away: a third.
    command(&mut w, Command::Despawn { id: hut });
    for _ in 0..50 {
        w.step();
        if built(&w) == 3 {
            break;
        }
    }
    assert_eq!(built(&w), 3, "built again for the removal");
    assert!(!w.is_occupied(Cell::new(38, 8)));
    run_group(&mut w, &ids, 2500);
    for id in &ids {
        assert!(between(pos(&w, *id), mid(55, 5)) <= Fx::from_int(5));
    }
}

// --- determinism ---

#[test]
fn the_run_is_reproducible_and_a_clone_goes_on_the_same() {
    let build = || {
        let mut w = world();
        let ids = block(&mut w, SOLDIER, 16, 2, 2, 8);
        group_order(&mut w, &ids, (55, 5));
        (w, ids)
    };
    let (mut a, ids) = build();
    let (mut b, _) = build();
    run(&mut a, 200);
    run(&mut b, 200);
    assert_eq!(a.checksum_history(), b.checksum_history());
    // A clone in the middle of the run (field built, units walking) goes on the
    // same.
    let mut c = a.clone();
    run(&mut a, 300);
    run(&mut c, 300);
    assert_eq!(a.checksum(), c.checksum());
    assert_eq!(pos_all(&a, &ids), pos_all(&c, &ids));
}

#[test]
fn worlds_without_a_group_order_hash_as_before_whatever_the_threshold() {
    let run_with = |threshold: Option<u32>| {
        let mut w = world_with(4096, threshold);
        let ids = block(&mut w, SOLDIER, 6, 4, 4, 3);
        for (k, id) in ids.iter().enumerate() {
            order(&mut w, *id, (20 + k as i32, 30));
        }
        run(&mut w, 200);
        w.checksum_history().to_vec()
    };
    assert_eq!(run_with(None), run_with(Some(0)));
    assert_eq!(run_with(None), run_with(Some(2)));
}
