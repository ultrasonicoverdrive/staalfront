//! The 24-unit crowd of the `pathing` scenario on a flowfield (WP4.3).
//!
//! `pathing.ron` turns flowfields off (`flowfield_threshold: Some(0)`) because
//! its crowd of 24 light transports, sent to one place by one order, is the test
//! of the queue and the budget of single requests, and its checksum is pinned on
//! that. Since WP4.3 such an order is a group order; this runs the same scenario
//! with the shipped threshold and checks what a flowfield promises on that map (a
//! maze, a river with one bridge, a wall with gaps, a structure placed and taken
//! away): the crowd arrives round its goal, nobody is ever on ground its class
//! cannot enter, the other units are served as before, and the budget holds.

mod common;

use common::scenario_path;
use sf_headless::cli::parse_checksum;
use sf_headless::scenario::Scenario;
use sf_math::{Fx, Vec2};
use sf_sim::{EntityId, Order, RouteStatus};

const CROWD: std::ops::Range<u32> = 10..34;
const GOAL: (u32, u32) = (60, 62);

fn id(slot: u32) -> EntityId {
    EntityId::new(slot, 0)
}

#[test]
fn the_crowd_of_the_pathing_scenario_arrives_on_a_flowfield() {
    let mut scenario = Scenario::load(&scenario_path("pathing.ron")).unwrap();
    assert_eq!(
        scenario.flowfield_threshold,
        Some(0),
        "pinned without fields"
    );
    scenario.flowfield_threshold = None;
    let mut world = scenario.build_world().unwrap();
    assert_eq!(
        world.flowfield_threshold(),
        sf_sim::DEFAULT_FLOWFIELD_THRESHOLD
    );
    let budget = world.path_stats().budget;
    let mut arrived = vec![None; CROWD.len()];
    let mut first_move = None;
    for _ in 0..scenario.ticks {
        let report = world.step();
        let tick = report.tick.0;
        let stats = world.path_stats();
        assert!(stats.nodes_last_tick <= budget, "tick {tick}");
        for s in 0..34 {
            let unit = id(s);
            let Some(view) = world.get(unit) else {
                continue;
            };
            let class = world.stats_of(unit).unwrap().movement_class.unwrap();
            let cell = world.cell_at(view.pos).unwrap();
            assert!(
                world.passable(class, cell),
                "tick {tick}: unit {s} in {cell:?}"
            );
        }
        for (k, s) in CROWD.enumerate() {
            if tick >= 2 && arrived[k].is_none() && world.order_of(id(s)) == Some(Order::Idle) {
                arrived[k] = Some(tick);
            }
            if first_move.is_none()
                && world
                    .route_of(id(s))
                    .is_some_and(|v| v.field && v.status == RouteStatus::Following)
            {
                first_move = Some(tick);
            }
        }
    }
    let goal = Vec2::new(
        Fx::from_i64(i64::from(GOAL.0)) + Fx::HALF,
        Fx::from_i64(i64::from(GOAL.1)) + Fx::HALF,
    );
    for (k, s) in CROWD.enumerate() {
        let pos = world.get(id(s)).unwrap().pos;
        assert!(arrived[k].is_some(), "crowd unit {s} never arrived");
        assert!(
            (pos - goal).length() <= Fx::from_int(5),
            "crowd unit {s} stands at {pos:?}"
        );
    }
    let stats = world.path_stats();
    eprintln!(
        "fields built {}, arrivals {:?}",
        stats.fields_built, arrived
    );
    assert!(
        stats.fields_built >= 1,
        "a field for the 24 light transports"
    );
    let last = arrived.iter().flatten().max().unwrap();
    // The checksum this gives, next to the pinned one without the field: the
    // difference is the whole of what the scenario's opt-out keeps pinned.
    let pinned =
        parse_checksum(include_str!("../../../assets/scenarios/pathing.expected")).unwrap();
    let with_field = *world.checksum_history().last().unwrap();
    eprintln!(
        "pathing on a flowfield: the 24 arrive by tick {last}, first moving on tick {first_move:?}; \
         final checksum {with_field:#018x}, pinned without fields {pinned:#018x}; \
         {} single-route jobs",
        stats.jobs_done
    );
    assert_ne!(
        with_field, pinned,
        "a different behaviour is a different value"
    );
}

#[test]
fn the_other_units_of_the_pathing_scenario_are_served_as_before_with_the_crowd_on_a_field() {
    // The ten units with their own orders still get their routes from the
    // single-route service, in the order of the ids, and the island and the
    // water ones say what they said.
    let mut scenario = Scenario::load(&scenario_path("pathing.ron")).unwrap();
    scenario.flowfield_threshold = None;
    let mut world = scenario.build_world().unwrap();
    for _ in 0..scenario.ticks {
        world.step();
    }
    for s in [0, 1, 2, 3, 4, 8] {
        assert_eq!(world.order_of(id(s)), Some(Order::Idle), "unit {s} arrived");
    }
    for s in [5, 6, 9] {
        let v = world.route_of(id(s)).unwrap();
        assert!(
            v.unreachable && v.status == RouteStatus::Stranded,
            "unit {s}"
        );
    }
}
