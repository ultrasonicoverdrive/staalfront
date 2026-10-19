//! The `replan` scenario (WP4.7): a route that is blocked by a structure, a route blocked
//! by a unit on hold with a way round and with none, a target that walks away, one that
//! disappears, and a group whose goal is built over. See `assets/scenarios/replan.ron`
//! for who is who, and `docs/rules/pathfinding.md` ("Replanning").
//!
//! Asserts the outcomes and the replans each unit asked for, and that none of it is a
//! storm: the requests are few, each unit's are within the limit and a cooldown apart,
//! and the path service stays inside its budget on every tick.

mod common;

use common::scenario_path;
use sf_headless::scenario::Scenario;
use sf_math::{Fx, Vec2};
use sf_sim::{Cell, EntityId, Order, RouteStatus, World};

fn id(slot: u32) -> EntityId {
    EntityId::new(slot, 0)
}

fn cell(x: i32, y: i32) -> Vec2 {
    Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF)
}

/// Slots of the scenario.
const R1_TANK: u32 = 0;
const R2_TANK: u32 = 1;
const R2_HOLDER: u32 = 2;
const R3_TANK: u32 = 3;
const R3_HOLDER: u32 = 4;
const R4_SCOUT: u32 = 5;
const R4_HUNTER: u32 = 6;
const R5_TARGET: u32 = 7;
const R5_HUNTER_QUEUED: u32 = 8;
const R5_HUNTER: u32 = 9;
const R5_BYSTANDER: u32 = 10;
const R6: std::ops::Range<u32> = 11..23;

struct Run {
    world: World,
    history: Vec<u64>,
    /// `(tick, slot)` of every unit on ground its class cannot enter or on a structure.
    bad_cells: Vec<(u64, u32)>,
    /// The most replans each slot asked for in an order, and the ticks they went up.
    most: Vec<u8>,
    asked_at: Vec<Vec<u64>>,
    /// The route status of the R1 tank on the tick after the structure was built.
    r1_after_the_structure: Option<RouteStatus>,
    /// Where the units of R1 and R2 were at their nearest to the lower corridor.
    went_by_the_lower_corridor: [bool; 2],
    /// The positions of the holders and the bystander at the start.
    held: Vec<(u32, Vec2)>,
    moved_when_it_should_not: Vec<u32>,
    /// The hunters of R5 stopped where they were on the tick the target went.
    r5_stopped_at: Option<Vec2>,
    /// The ticks at which the target replans went up.
    target_replans_at: Vec<u64>,
    /// The slowest unit to be done in R6.
    r6_done_at: Option<u64>,
    peak_nodes: u32,
    budget: u32,
    order_of_r3: Vec<(u64, Option<Order>)>,
}

fn run() -> Run {
    let scenario = Scenario::load(&scenario_path("replan.ron")).unwrap();
    let mut world = scenario.build_world().unwrap();
    let slots = 23_u32;
    let mut r = Run {
        world: world.clone(),
        history: Vec::new(),
        bad_cells: Vec::new(),
        most: vec![0; slots as usize],
        asked_at: vec![Vec::new(); slots as usize],
        r1_after_the_structure: None,
        went_by_the_lower_corridor: [false; 2],
        held: Vec::new(),
        moved_when_it_should_not: Vec::new(),
        r5_stopped_at: None,
        target_replans_at: Vec::new(),
        r6_done_at: None,
        peak_nodes: 0,
        budget: world.path_stats().budget,
        order_of_r3: Vec::new(),
    };
    let mut last_target_replans = 0;
    let mut last_order_of_r3 = None;
    for _ in 0..scenario.ticks {
        let report = world.step();
        r.history.push(report.checksum);
        let tick = report.tick.0;
        if r.held.is_empty() {
            r.held = [R2_HOLDER, R3_HOLDER, R5_BYSTANDER]
                .iter()
                .map(|&s| (s, world.get(id(s)).unwrap().pos))
                .collect();
        }
        for s in 0..slots {
            let Some(view) = world.get(id(s)) else {
                continue;
            };
            let Some(class) = world.stats_of(id(s)).and_then(|st| st.movement_class) else {
                continue;
            };
            let c = world.cell_at(view.pos).unwrap();
            if !world.passable(class, c) || world.is_occupied(c) {
                r.bad_cells.push((tick, s));
            }
            let replans = world.replans_of(id(s)).unwrap();
            if replans > r.most[s as usize] {
                r.asked_at[s as usize].push(tick);
            }
            r.most[s as usize] = r.most[s as usize].max(replans);
        }
        for &(s, at) in &r.held {
            if world.get(id(s)).unwrap().pos != at && !r.moved_when_it_should_not.contains(&s) {
                r.moved_when_it_should_not.push(s);
            }
        }
        // R1: the structure is built on tick 90 and the tank has to look again at once, on that tick.
        if tick == 90 {
            r.r1_after_the_structure = Some(world.route_of(id(R1_TANK)).unwrap().status);
        }
        let p = world.get(id(R1_TANK)).unwrap().pos;
        if p.y > Fx::from_int(20) && p.x > Fx::from_int(8) && p.x < Fx::from_int(24) {
            r.went_by_the_lower_corridor[0] = true;
        }
        let p = world.get(id(R2_TANK)).unwrap().pos;
        if p.y > Fx::from_int(20) && p.x > Fx::from_int(40) && p.x < Fx::from_int(56) {
            r.went_by_the_lower_corridor[1] = true;
        }
        // R5: the target is removed on tick 40.
        if tick == 41 {
            r.r5_stopped_at = Some(world.get(id(R5_HUNTER)).unwrap().pos);
        }
        let stats = world.path_stats();
        for _ in last_target_replans..stats.target_replans {
            r.target_replans_at.push(tick);
        }
        last_target_replans = stats.target_replans;
        r.peak_nodes = r.peak_nodes.max(stats.nodes_last_tick);
        let o = world.order_of(id(R3_TANK));
        if o != last_order_of_r3 {
            r.order_of_r3.push((tick, o));
            last_order_of_r3 = o;
        }
        if r.r6_done_at.is_none()
            && tick > 5
            && R6
                .clone()
                .all(|s| world.order_of(id(s)) == Some(Order::Idle))
        {
            r.r6_done_at = Some(tick);
        }
    }
    r.world = world;
    r
}

fn near(w: &World, slot: u32, to: Vec2, within: i32) -> bool {
    (w.get(id(slot)).unwrap().pos - to).length() < Fx::from_int(within)
}

#[test]
fn nobody_is_ever_on_ground_they_cannot_enter_and_the_service_keeps_to_its_budget() {
    let r = run();
    assert!(
        r.bad_cells.is_empty(),
        "{:?}",
        &r.bad_cells[..r.bad_cells.len().min(5)]
    );
    assert!(r.peak_nodes <= r.budget, "{} > {}", r.peak_nodes, r.budget);
    // The holders and the bystander never move a hair.
    assert!(
        r.moved_when_it_should_not.is_empty(),
        "{:?}",
        r.moved_when_it_should_not
    );
    assert_eq!(r.world.order_of(id(R2_HOLDER)), Some(Order::HoldPosition));
    assert_eq!(r.world.order_of(id(R3_HOLDER)), Some(Order::HoldPosition));
    assert_eq!(r.world.order_of(id(R5_BYSTANDER)), Some(Order::Idle));
}

#[test]
fn a_structure_built_in_the_corridor_sends_the_tank_by_the_other_one_without_getting_stuck() {
    let r = run();
    assert!(r.world.is_occupied(Cell::new(18, 8)), "the tower stands");
    assert_eq!(
        r.r1_after_the_structure,
        Some(RouteStatus::Waiting),
        "the tank asked again on the tick the structure was built"
    );
    assert!(r.went_by_the_lower_corridor[0]);
    assert_eq!(r.world.order_of(id(R1_TANK)), Some(Order::Idle));
    assert!(near(&r.world, R1_TANK, cell(27, 8), 1));
    // It was not stuck: the graph knew the structure.
    assert_eq!(r.most[R1_TANK as usize], 0);
}

#[test]
fn a_tank_stuck_behind_a_unit_on_hold_goes_round_with_one_replan() {
    let r = run();
    assert!(r.went_by_the_lower_corridor[1]);
    assert_eq!(r.world.order_of(id(R2_TANK)), Some(Order::Idle));
    assert!(near(&r.world, R2_TANK, cell(59, 8), 1));
    assert_eq!(r.most[R2_TANK as usize], 1, "one replan did it");
    assert_eq!(r.asked_at[R2_TANK as usize].len(), 1);
}

#[test]
fn with_no_way_round_the_tank_gives_up_after_k_replans_and_the_queued_order_starts() {
    let r = run();
    let k = r.world.replan_config().max;
    let cooldown = u64::from(r.world.replan_config().cooldown);
    assert_eq!(r.most[R3_TANK as usize], k);
    let asked = &r.asked_at[R3_TANK as usize];
    assert_eq!(asked.len(), usize::from(k));
    for pair in asked.windows(2) {
        assert!(pair[1] - pair[0] >= cooldown, "{asked:?}");
    }
    // The order it had was the move east until the tick it gave up, then the queued move.
    let orders: Vec<Option<Order>> = r.order_of_r3.iter().map(|&(_, o)| o).collect();
    assert_eq!(
        orders,
        vec![
            Some(Order::Idle),
            Some(Order::Move(cell(91, 8))),
            Some(Order::Move(cell(67, 20))),
            Some(Order::Idle),
        ],
        "{:?}",
        r.order_of_r3
    );
    let gave_up_at = r.order_of_r3[2].0;
    assert!(
        gave_up_at > asked[usize::from(k) - 1],
        "not before the last replan: {gave_up_at} {asked:?}"
    );
    assert_eq!(r.world.crowd_stats().gave_up, 1);
    assert!(near(&r.world, R3_TANK, cell(67, 20), 1));
}

#[test]
fn the_hunter_of_a_target_that_walks_away_asks_again_for_a_route_no_faster_than_the_cooldown() {
    let r = run();
    let cooldown = u64::from(r.world.replan_config().retarget_cooldown);
    let at = &r.target_replans_at;
    assert!(at.len() >= 3, "the hunter followed: {at:?}");
    // Few: the scout walked about 60 cells, a search for every three of them at most.
    assert!(at.len() <= 25, "{at:?}");
    for pair in at.windows(2) {
        assert!(pair[1] - pair[0] >= cooldown, "{at:?}");
    }
    // The scout ended where it was sent, round the wall by the gap; the hunter is in reach.
    assert_eq!(r.world.order_of(id(R4_SCOUT)), Some(Order::Idle));
    assert!(near(&r.world, R4_SCOUT, cell(28, 35), 1));
    assert_eq!(
        r.world.order_of(id(R4_HUNTER)),
        Some(Order::Attack(id(R4_SCOUT)))
    );
    let reach = r.world.stats_of(id(R4_HUNTER)).unwrap().weapon_range + Fx::HALF;
    let d =
        (r.world.get(id(R4_HUNTER)).unwrap().pos - r.world.get(id(R4_SCOUT)).unwrap().pos).length();
    assert!(d <= reach, "{d:?} > {reach:?}");
    assert_eq!(r.most[R4_HUNTER as usize], 0, "never stuck");
}

#[test]
fn a_target_that_dies_ends_the_attacks_and_the_hunters_go_on_to_what_is_next() {
    let r = run();
    let w = &r.world;
    // The rifleman is gone, and the unit that took its slot is not what anybody hunts.
    assert!(!w.is_alive(id(R5_TARGET)));
    let newcomer = EntityId::new(R5_TARGET, 1);
    assert!(w.is_alive(newcomer));
    assert_eq!(w.order_of(newcomer), Some(Order::Idle));
    assert!(near(w, R5_BYSTANDER, cell(60, 60), 1));
    // The hunter with a move queued behind its attack does it; the other stands idle
    // where it was when the target went.
    assert_eq!(w.order_of(id(R5_HUNTER_QUEUED)), Some(Order::Idle));
    assert!(near(w, R5_HUNTER_QUEUED, cell(36, 36), 1));
    assert_eq!(w.order_of(id(R5_HUNTER)), Some(Order::Idle));
    let stopped = r.r5_stopped_at.unwrap();
    assert!((w.get(id(R5_HUNTER)).unwrap().pos - stopped).length() < Fx::from_ratio(1, 2));
    assert_eq!(w.route_of(id(R5_HUNTER)).unwrap().status, RouteStatus::None);
}

#[test]
fn a_group_whose_goal_is_built_over_ends_round_the_structure() {
    let r = run();
    let w = &r.world;
    assert!(r.r6_done_at.is_some(), "somebody never got there");
    // The radar covers columns 89 and 90, rows 47 and 48; the goal cell (90, 48) is in it.
    for x in 89..=90 {
        for y in 47..=48 {
            assert!(w.is_occupied(Cell::new(x, y)));
        }
    }
    for s in R6 {
        let pos = w.get(id(s)).unwrap().pos;
        assert_eq!(w.order_of(id(s)), Some(Order::Idle), "unit {s}");
        assert!(
            !w.is_occupied(w.cell_at(pos).unwrap()),
            "unit {s} is inside it"
        );
        let d = (pos - cell(90, 48)).length();
        assert!(d < Fx::from_int(5), "unit {s} ended {d:?} from the goal");
        assert!(r.most[s as usize] <= 1, "unit {s}");
    }
    // The field was built twice: for the order, and again with another cell for a goal.
    assert!(w.path_stats().fields_built >= 2);
}

#[test]
fn there_is_no_storm_the_replans_are_few_and_counted() {
    let r = run();
    let stats = r.world.path_stats();
    let k = u64::from(r.world.replan_config().max);
    // R2's tank asked once, R3's asked k times: the stuck requests, all with an overlay.
    assert_eq!(stats.stuck_replans, 1 + k);
    assert_eq!(stats.overlay_replans, 1 + k);
    // The k requests of R3 found nothing; the one of R2 found its way.
    assert_eq!(stats.detours_failed, k);
    // The structure of R1 sent two units to look again: the tank, and the tank of the
    // next room, whose route lay in a sector next to the one that changed.
    assert_eq!(stats.map_replans, 2);
    // Jobs done in all: a few dozen for 23 units over 3,000 ticks.
    assert!(stats.jobs_done <= 60, "{}", stats.jobs_done);
    for s in 0..23 {
        assert!(r.most[s] <= r.world.replan_config().max, "slot {s}");
    }
    assert_eq!(r.world.crowd_stats().gave_up, 1);
}

#[test]
fn the_run_is_reproducible() {
    let a = run();
    let b = run();
    assert_eq!(a.history, b.history);
}
