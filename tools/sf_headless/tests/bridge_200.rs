//! The `bridge_200` scenario (WP4.3, `docs/rules/pathfinding.md`, "Flowfields for
//! groups"): 200 mixed units cross one bridge to a goal area on one order.
//!
//! This is the "done when" of phase 4 in PLAN.md, as far as it can be before
//! collision (WP4.4): all of them arrive, at most 1 % end outside 5 cells (20 m)
//! of the goal, none is ever on ground its class cannot enter, and the path
//! service stays inside its budget while it builds the fields. The units overlap
//! on the bridge and at the goal; that is expected until WP4.4, and the test
//! prints how much instead of asserting it away.
//!
//! The same crowd without flowfields (`flowfield_threshold: Some(0)`) is run as
//! the comparison: 195 single requests instead of three fields.

mod common;

use std::collections::BinaryHeap;

use common::scenario_path;
use sf_headless::scenario::Scenario;
use sf_math::{Fx, Vec2};
use sf_sim::{Cell, EntityId, Order, World};

const UNITS: u32 = 200;
const GOAL: (u32, u32) = (80, 32);
const W: u32 = 96;
const H: u32 = 64;

/// The tick by which every unit has arrived (measured: 2,137 for the slowest, a
/// rifleman at 0.7 cells a second, about 60 cells and a bridge away).
const ARRIVE_BY: u64 = 2400;

/// The classes, as the permanent kind numbers of `unit_classes.ron`.
const FOOT: u16 = 0;
const WHEELED: u16 = 1;
const TRACKED: u16 = 2;
const HOVER: u16 = 3;

fn id(slot: u32) -> EntityId {
    EntityId::new(slot, 0)
}

fn goal() -> Vec2 {
    Vec2::new(
        Fx::from_i64(i64::from(GOAL.0)) + Fx::HALF,
        Fx::from_i64(i64::from(GOAL.1)) + Fx::HALF,
    )
}

/// What a run of the scenario saw.
struct Run {
    world: World,
    /// The number of the tick on which each unit's order ended.
    arrived: Vec<Option<u64>>,
    /// The first tick any unit of each class had moved, by class number.
    class_first_move: [Option<u64>; 4],
    /// The tick by which every unit had moved.
    all_moving: Option<u64>,
    /// The most nodes the path service spent in a tick, and of them on fields.
    peak_nodes: u32,
    peak_field_nodes: u32,
    /// All the nodes it spent.
    total_nodes: u64,
    /// The tick each field was finished on.
    field_done: Vec<u64>,
    /// Ticks in which a unit ended on ground its class cannot enter.
    bad_cells: Vec<(u64, u32, Cell)>,
    /// The distance each unit walked, in raw fixed point.
    walked: Vec<i64>,
    /// The first position of each unit.
    start: Vec<Vec2>,
    /// The most units on the bridge's three rows at once, and the most that were
    /// refused a step (cautious) at once.
    on_bridge: usize,
    cautious: usize,
    history: Vec<u64>,
    budget: u32,
}

fn run_scenario(scenario: &Scenario) -> Run {
    let mut world = scenario.build_world().unwrap();
    let budget = world.path_stats().budget;
    let mut r = Run {
        world: world.clone(),
        arrived: vec![None; UNITS as usize],
        class_first_move: [None; 4],
        all_moving: None,
        peak_nodes: 0,
        peak_field_nodes: 0,
        total_nodes: 0,
        field_done: Vec::new(),
        bad_cells: Vec::new(),
        walked: vec![0; UNITS as usize],
        start: Vec::new(),
        on_bridge: 0,
        cautious: 0,
        history: Vec::new(),
        budget,
    };
    let mut last: Vec<Vec2> = Vec::new();
    let mut moved = vec![false; UNITS as usize];
    for _ in 0..scenario.ticks {
        let report = world.step();
        r.history.push(report.checksum);
        let tick = report.tick.0;
        let stats = world.path_stats();
        assert!(
            stats.nodes_last_tick <= budget,
            "tick {tick}: the path service spent {} nodes, the budget is {budget}",
            stats.nodes_last_tick
        );
        r.peak_nodes = r.peak_nodes.max(stats.nodes_last_tick);
        r.peak_field_nodes = r.peak_field_nodes.max(stats.field_nodes_last_tick);
        r.total_nodes += u64::from(stats.nodes_last_tick);
        while (r.field_done.len() as u64) < stats.fields_built {
            r.field_done.push(tick);
        }
        if r.start.is_empty() && world.len() == UNITS as usize {
            r.start = (0..UNITS).map(|s| world.get(id(s)).unwrap().pos).collect();
            last = r.start.clone();
        }
        let mut on_bridge = 0;
        let mut cautious = 0;
        for s in 0..UNITS {
            let unit = id(s);
            let pos = world.get(unit).unwrap().pos;
            let class = world.stats_of(unit).unwrap().movement_class.unwrap();
            let cell = world.cell_at(pos).unwrap();
            if !world.passable(class, cell) {
                r.bad_cells.push((tick, s, cell));
            }
            if !last.is_empty() {
                r.walked[s as usize] += last[s as usize].distance(pos).raw();
                last[s as usize] = pos;
                if pos != r.start[s as usize] {
                    moved[s as usize] = true;
                    r.class_first_move[usize::from(class)].get_or_insert(tick);
                }
            }
            if (44..=49).contains(&cell.x) && (31..=33).contains(&cell.y) {
                on_bridge += 1;
            }
            if world.route_of(unit).is_some_and(|v| v.cautious) {
                cautious += 1;
            }
            if tick >= 2
                && r.arrived[s as usize].is_none()
                && world.order_of(unit) == Some(Order::Idle)
            {
                r.arrived[s as usize] = Some(tick);
            }
        }
        r.on_bridge = r.on_bridge.max(on_bridge);
        r.cautious = r.cautious.max(cautious);
        if r.all_moving.is_none() && !last.is_empty() && moved.iter().all(|&m| m) {
            r.all_moving = Some(tick);
        }
    }
    r.world = world;
    r
}

fn run() -> Run {
    run_scenario(&Scenario::load(&scenario_path("bridge_200.ron")).unwrap())
}

impl Run {
    fn last_arrival(&self) -> u64 {
        self.arrived.iter().flatten().max().copied().unwrap_or(0)
    }

    fn outside_five_cells(&self) -> Vec<u32> {
        (0..UNITS)
            .filter(|&s| (self.world.get(id(s)).unwrap().pos - goal()).length() > Fx::from_int(5))
            .collect()
    }

    /// Pairs of units closer than a quarter of a cell at the end: the overlap
    /// that collision (WP4.4) will have to resolve.
    fn overlapping_pairs(&self) -> usize {
        let pos: Vec<Vec2> = (0..UNITS)
            .map(|s| self.world.get(id(s)).unwrap().pos)
            .collect();
        let near = Fx::from_ratio(1, 4);
        let mut pairs = 0;
        for a in 0..pos.len() {
            for b in a + 1..pos.len() {
                if (pos[a] - pos[b]).length() < near {
                    pairs += 1;
                }
            }
        }
        pairs
    }
}

// --- the references: a plain search of the whole grid --------------------------------

const ORTH: i64 = 65_536;
const DIAG: i64 = 92_682;

/// The length of the shortest walk along the grid (eight neighbours, no cutting
/// a corner) from the goal cell to every cell, for a class, in raw fixed point
/// where a cell is 65,536; `i64::MAX` where there is none.
fn grid_distances(world: &World, class: u16) -> Vec<i64> {
    let open = |x: i64, y: i64| {
        x >= 0
            && y >= 0
            && x < i64::from(W)
            && y < i64::from(H)
            && world.passable(class, Cell::new(x as u32, y as u32))
    };
    let mut dist = vec![i64::MAX; (W * H) as usize];
    let start = (GOAL.1 * W + GOAL.0) as usize;
    dist[start] = 0;
    let mut heap = BinaryHeap::new();
    heap.push(std::cmp::Reverse((0_i64, start)));
    while let Some(std::cmp::Reverse((d, at))) = heap.pop() {
        if d > dist[at] {
            continue;
        }
        let (x, y) = ((at as u32 % W) as i64, (at as u32 / W) as i64);
        for (dx, dy) in [
            (0, -1),
            (1, 0),
            (0, 1),
            (-1, 0),
            (1, -1),
            (1, 1),
            (-1, 1),
            (-1, -1),
        ] {
            let (nx, ny) = (x + dx, y + dy);
            if !open(nx, ny) || (dx != 0 && dy != 0 && !(open(nx, y) && open(x, ny))) {
                continue;
            }
            let step = if dx != 0 && dy != 0 { DIAG } else { ORTH };
            let n = (ny as u32 * W + nx as u32) as usize;
            if d + step < dist[n] {
                dist[n] = d + step;
                heap.push(std::cmp::Reverse((d + step, n)));
            }
        }
    }
    dist
}

// --- the tests -------------------------------------------------------------------------

#[test]
fn all_two_hundred_units_arrive_within_the_limit() {
    let r = run();
    let n = r.arrived.iter().flatten().count();
    assert_eq!(n, UNITS as usize, "units that never arrived: {:?}", {
        (0..UNITS)
            .filter(|&s| r.arrived[s as usize].is_none())
            .collect::<Vec<_>>()
    });
    let last = r.last_arrival();
    eprintln!(
        "bridge_200: all {UNITS} arrived by tick {last} ({} s of game time); \
         first move per class (foot, wheeled, tracked, hover) {:?}, every unit moving by tick {:?}; \
         fields finished on ticks {:?}; the path service spent at most {} nodes in a tick \
         ({} of them on fields) of {}, {} in all, {} jobs of single routes",
        last / 20,
        r.class_first_move,
        r.all_moving,
        r.field_done,
        r.peak_nodes,
        r.peak_field_nodes,
        r.budget,
        r.total_nodes,
        r.world.path_stats().jobs_done
    );
    assert!(last <= ARRIVE_BY, "the last unit arrived on tick {last}");
    for s in 0..UNITS {
        assert_eq!(r.world.order_of(id(s)), Some(Order::Idle));
        assert!(!r.world.route_of(id(s)).unwrap().field, "route over");
    }
}

#[test]
fn at_most_one_percent_end_outside_five_cells_of_the_goal() {
    let r = run();
    let outside = r.outside_five_cells();
    let farthest = (0..UNITS)
        .map(|s| (r.world.get(id(s)).unwrap().pos - goal()).length())
        .max()
        .unwrap();
    eprintln!(
        "bridge_200: {} of {UNITS} outside 5 cells of the goal; the farthest {:?} cells; \
         {} pairs of units closer than a quarter of a cell (collision is WP4.4), \
         at most {} on the bridge at once, at most {} stepping cell by cell at once",
        outside.len(),
        farthest,
        r.overlapping_pairs(),
        r.on_bridge,
        r.cautious
    );
    assert!(
        outside.len() * 100 <= UNITS as usize,
        "outside five cells: {outside:?}"
    );
}

#[test]
fn nobody_ever_ends_a_tick_on_ground_its_class_cannot_enter() {
    let r = run();
    assert!(
        r.bad_cells.is_empty(),
        "{:?}",
        &r.bad_cells[..r.bad_cells.len().min(5)]
    );
}

#[test]
fn the_groups_start_within_a_few_ticks_and_the_budget_holds_while_the_fields_are_built() {
    let r = run();
    // Three fields (foot, wheeled, tracked), the five hover craft on their own.
    let stats = r.world.path_stats();
    assert_eq!(stats.fields_built, 3);
    assert_eq!(r.field_done.len(), 3);
    // The first unit of each class that follows a field moves within a few ticks
    // of the order (given on tick 1), and everybody is moving soon after (the
    // slowest to turn takes a few ticks to face the way).
    for class in [FOOT, WHEELED, TRACKED, HOVER] {
        let first = r.class_first_move[usize::from(class)].unwrap();
        assert!(first <= 12, "class {class} moved first on tick {first}");
    }
    assert!(r.all_moving.unwrap() <= 30, "{:?}", r.all_moving);
    assert!(
        *r.field_done.last().unwrap() <= 12,
        "fields done: {:?}",
        r.field_done
    );
    // The budget held every tick (asserted in the run); the builder used it.
    assert!(r.peak_field_nodes > 0 && r.peak_nodes <= r.budget);
    // Not one route of its own for the 195 units of a field: the five hover craft
    // are all that was asked of the single-route service.
    assert!(
        stats.jobs_done < 40,
        "{} jobs for the five hover craft",
        stats.jobs_done
    );
}

#[test]
fn the_units_take_about_the_shortest_way() {
    let r = run();
    let by_class = [FOOT, WHEELED, TRACKED, HOVER].map(|c| (c, grid_distances(&r.world, c)));
    let mut worst = 0.0_f64;
    for s in 0..UNITS {
        let class = r.world.stats_of(id(s)).unwrap().movement_class.unwrap();
        let dist = &by_class.iter().find(|(c, _)| *c == class).unwrap().1;
        let start = r.world.cell_at(r.start[s as usize]).unwrap();
        let best = dist[(start.y * W + start.x) as usize];
        assert_ne!(best, i64::MAX);
        // The walk may end up to five cells from the goal's middle, in a slot, so
        // a little less or more than the walk to the middle.
        let slack = 5 * ORTH;
        let walked = r.walked[s as usize];
        assert!(
            walked <= best * 115 / 100 + slack,
            "unit {s} (class {class}) walked {} cells, the grid-best is {}",
            walked as f64 / ORTH as f64,
            best as f64 / ORTH as f64
        );
        worst = worst.max(walked as f64 / (best + slack) as f64);
    }
    eprintln!("bridge_200: longest walk is {worst:.3} of the grid-best walk plus five cells");
}

#[test]
fn the_run_is_reproducible() {
    let a = run();
    let b = run();
    assert_eq!(a.history, b.history);
}

/// The comparison: the same crowd with flowfields off is 195 requests for the
/// single-route service. Prints the numbers that show what the fields save and
/// asserts that they save most of it.
#[test]
fn without_fields_the_same_crowd_costs_many_times_the_search_work() {
    let with = run();
    let mut scenario = Scenario::load(&scenario_path("bridge_200.ron")).unwrap();
    scenario.flowfield_threshold = Some(0);
    let without = run_scenario(&scenario);
    let (a, b) = (with.world.path_stats(), without.world.path_stats());
    eprintln!(
        "bridge_200 with fields: {} nodes in all, {} jobs, every unit moving by tick {:?}, \
         all arrived by {}; without: {} nodes in all, {} jobs, every unit moving by tick {:?}, \
         all arrived by {}",
        with.total_nodes,
        a.jobs_done,
        with.all_moving,
        with.last_arrival(),
        without.total_nodes,
        b.jobs_done,
        without.all_moving,
        without.last_arrival()
    );
    assert_eq!(a.fields_built, 3);
    assert_eq!(b.fields_built, 0);
    assert!(with.total_nodes * 4 < without.total_nodes);
    assert!(without.bad_cells.is_empty());
}
