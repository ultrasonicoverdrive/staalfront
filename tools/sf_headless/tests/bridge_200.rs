//! The `bridge_200` scenario (WP4.3, WP4.4, WP4.4b, `docs/rules/pathfinding.md`,
//! `docs/rules/collision.md`): 200 mixed units cross one bridge to a goal area on one
//! order.
//!
//! This is the "done when" of phase 4 in PLAN.md, with bodies (WP4.4): all of them
//! arrive, none is stuck at the end, the bodies do not overlap beyond the slack, none
//! is ever on ground its class cannot enter, and the path service stays inside its
//! budget while it builds the fields. The crossing is a real bottleneck now (the
//! bridge is three cells wide and the crowd is wider than that, and so is the gap in
//! the wall behind it): it takes about twice as long as it did when the units went
//! through each other.
//!
//! **Five cells cannot hold this crowd.** The phase asked for at most 1 % of the units
//! outside 5 cells (20 m) of the goal, written when units were points. With bodies of
//! the shipped sizes (`size` of the definitions: a tank is 1.2 cells across) the 110
//! vehicles need about 150 cells of ground and a disc of five cells has 78, so about
//! half of them must stand further out however well they are packed. By the
//! orchestrator decision of 18-10-2026 (PLAN.md, phase 4) the criterion is: all
//! arrive, none stuck; at most 1 % end further from the goal than 1.3 times the radius
//! of a disc whose area is the sum of the units' `spacing * spacing` (`packing_radius`,
//! 7.37 cells here); no two bodies overlap by more than a quarter of a cell during the
//! run (every tick after the first step; see `worst_overlap`) or 1/16 of a cell at the end. The test
//! also prints how many are outside five cells. See `docs/rules/collision.md`,
//! "Arrival".
//!
//! **The criterion does not depend on the turning radius of the wheeled class** (WP4.4b).
//! After WP4.5 it held at 1.25 cells and failed at 1.5, 1.6, 1.75 and 2.0, which showed
//! that it had held by the luck of the crowd, not by the arrival logic. The tests named
//! `phase_four_holds_at_a_wheeled_radius_of_*` run the scenario with the radius replaced
//! (`scenario_with_wheeled_radius`, a test-only knob: the frozen definitions keep the
//! shipped radius) and ask `Run::misses` for every way the run falls short of the
//! criterion, one case for each radius the reviewers swept. The shipped radius is 1.5.
//!
//! The same crowd without flowfields (`flowfield_threshold: Some(0)`) is run as
//! the comparison: 195 single requests instead of three fields.

mod common;

use std::collections::BinaryHeap;
use std::sync::OnceLock;

use common::scenario_path;
use sf_headless::scenario::Scenario;
use sf_math::{Fx, Vec2};
use sf_sim::{Cell, EntityId, Order, TurnMode, World};

const UNITS: u32 = 200;
const GOAL: (u32, u32) = (80, 32);
const W: u32 = 96;
const H: u32 = 64;

/// The tick by which every unit has arrived. The measured last arrival is between 3,200
/// and 3,800 for the radii the sweep tried (it depends on how the queue at the gap in the
/// wall happens to fall), so this is a bound with room, not a prediction: the criterion
/// is that everybody arrives, not when.
const ARRIVE_BY: u64 = 4400;

/// The tick the crossing took when the units had no bodies (the slowest being a
/// rifleman at 0.7 cells a second, 60 cells and a bridge away).
const WITHOUT_BODIES: u64 = 2137;

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
    /// The worst overlap of two bodies at the end of any tick of the run (the
    /// first step on), in raw fixed point, and when. The state the scenario spawns
    /// them in is not part of the run: the units are placed one to a cell and the
    /// tanks are wider than that (0.35 of a cell over at tick 0, before the crowd
    /// pass has had a tick).
    worst_overlap: (i64, u64),
    /// The longest a unit stood (moved less than a hundredth of a cell a tick) with
    /// its order not done, in ticks.
    longest_still: u32,
    /// The tick by which every unit had crossed the river.
    all_across: Option<u64>,
}

/// The least distance the bodies of two units keep, the rule of
/// `docs/rules/collision.md`.
fn keep(ra: Fx, rb: Fx) -> Fx {
    let small = |r: Fx| r <= Fx::from_ratio(3, 8);
    if small(ra) && small(rb) {
        (ra + rb).mul_int(3).div_int(4)
    } else {
        ra + rb
    }
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
        worst_overlap: (0, 0),
        longest_still: 0,
        all_across: None,
    };
    let mut last: Vec<Vec2> = Vec::new();
    let mut moved = vec![false; UNITS as usize];
    let mut still = vec![0_u32; UNITS as usize];
    let mut across = vec![false; UNITS as usize];
    let mut radius: Vec<Fx> = Vec::new();
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
            radius = (0..UNITS)
                .map(|s| world.collision_radius_of(id(s)).unwrap())
                .collect();
        }
        let mut on_bridge = 0;
        let mut cautious = 0;
        let mut positions: Vec<Vec2> = Vec::with_capacity(UNITS as usize);
        for s in 0..UNITS {
            let unit = id(s);
            let pos = world.get(unit).unwrap().pos;
            positions.push(pos);
            let class = world.stats_of(unit).unwrap().movement_class.unwrap();
            let cell = world.cell_at(pos).unwrap();
            if !world.passable(class, cell) {
                r.bad_cells.push((tick, s, cell));
            }
            if !last.is_empty() {
                let step = last[s as usize].distance(pos);
                r.walked[s as usize] += step.raw();
                if pos != r.start[s as usize] {
                    moved[s as usize] = true;
                    r.class_first_move[usize::from(class)].get_or_insert(tick);
                }
                // Standing still with something left to do.
                if r.arrived[s as usize].is_none() && tick >= 20 {
                    if step < Fx::from_ratio(1, 100) {
                        still[s as usize] += 1;
                        r.longest_still = r.longest_still.max(still[s as usize]);
                    } else {
                        still[s as usize] = 0;
                    }
                }
                last[s as usize] = pos;
            }
            if (44..=49).contains(&cell.x) && (31..=33).contains(&cell.y) {
                on_bridge += 1;
            }
            if cell.x >= 50 {
                across[s as usize] = true;
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
        if r.all_across.is_none() && across.iter().all(|&a| a) {
            r.all_across = Some(tick);
        }
        if tick >= 1 {
            for a in 0..UNITS as usize {
                for b in a + 1..UNITS as usize {
                    let (pa, pb) = (positions[a], positions[b]);
                    if (pa.x - pb.x).abs() > Fx::from_int(2)
                        || (pa.y - pb.y).abs() > Fx::from_int(2)
                    {
                        continue;
                    }
                    let overlap = (keep(radius[a], radius[b]) - pa.distance(pb)).raw();
                    if overlap > r.worst_overlap.0 {
                        r.worst_overlap = (overlap, tick);
                    }
                }
            }
        }
    }
    r.world = world;
    r
}

/// The shared run of the shipped scenario: it is long, so the tests that only read
/// it share one.
fn run() -> &'static Run {
    static RUN: OnceLock<Run> = OnceLock::new();
    RUN.get_or_init(|| run_scenario(&Scenario::load(&scenario_path("bridge_200.ron")).unwrap()))
}

/// The scenario with the turning radius of the wheeled class replaced by
/// `hundredths` of a cell (150 is 1.5 cells). A test-only knob: the frozen
/// definitions keep the shipped radius, and the movement table the scenario was
/// loaded with is rebuilt with that one profile changed.
fn scenario_with_wheeled_radius(hundredths: i64) -> Scenario {
    let mut scenario = Scenario::load(&scenario_path("bridge_200.ron")).unwrap();
    let table = scenario.movement_table.take().unwrap();
    let mut profile = table.turning(WHEELED);
    assert_eq!(profile.mode, TurnMode::Arc, "the wheeled class is an arc");
    profile.min_radius = Fx::from_ratio(hundredths, 100);
    scenario.movement_table = Some(table.with_turning(vec![(WHEELED, profile)]).unwrap());
    scenario
}

impl Run {
    fn last_arrival(&self) -> u64 {
        self.arrived.iter().flatten().max().copied().unwrap_or(0)
    }

    fn outside(&self, cells: Fx) -> Vec<u32> {
        (0..UNITS)
            .filter(|&s| (self.world.get(id(s)).unwrap().pos - goal()).length() > cells)
            .collect()
    }

    /// The radius of a disc with the area of all the bodies as they are packed on a
    /// square lattice (one `distance * distance` of ground to each, the distance
    /// being what its body keeps from its neighbours), in cells, as a float for the
    /// comparison only.
    fn packing_radius(&self) -> f64 {
        let area: f64 = (0..UNITS)
            .map(|s| {
                let r = self.world.collision_radius_of(id(s)).unwrap();
                let d = keep(r, r).raw() as f64 / ORTH as f64;
                d * d
            })
            .sum();
        (area / std::f64::consts::PI).sqrt()
    }
}

impl Run {
    /// The worst overlap of two bodies at the end, in raw fixed point.
    fn worst_end_overlap(&self) -> i64 {
        let mut worst = 0;
        for a in 0..UNITS {
            for b in a + 1..UNITS {
                let (ra, rb) = (
                    self.world.collision_radius_of(id(a)).unwrap(),
                    self.world.collision_radius_of(id(b)).unwrap(),
                );
                let d = (self.world.get(id(a)).unwrap().pos - self.world.get(id(b)).unwrap().pos)
                    .length();
                worst = worst.max((keep(ra, rb) - d).raw());
            }
        }
        worst
    }

    fn farthest(&self) -> Fx {
        (0..UNITS)
            .map(|s| (self.world.get(id(s)).unwrap().pos - goal()).length())
            .max()
            .unwrap()
    }

    /// Every way this run misses the "done when" of phase 4 (PLAN.md), one line
    /// each; empty when it meets it. All arrive and none is stuck; at most 1 %
    /// end further than 1.3 times the packing radius; no overlap beyond a quarter
    /// of a cell on the way or a sixteenth at the end; nobody on ground its class
    /// cannot enter; nobody stood still for half a minute with its order not done.
    fn misses(&self) -> Vec<String> {
        let mut out = Vec::new();
        let never: Vec<u32> = (0..UNITS)
            .filter(|&s| self.arrived[s as usize].is_none())
            .collect();
        if !never.is_empty() {
            out.push(format!("{} never arrive: {never:?}", never.len()));
        }
        let last = self.last_arrival();
        if last > ARRIVE_BY {
            out.push(format!(
                "the last arrival is on tick {last}, after {ARRIVE_BY}"
            ));
        }
        let stuck: Vec<u32> = (0..UNITS)
            .filter(|&s| {
                let u = id(s);
                self.world.stuck_ticks_of(u) != Some(0)
                    || self.world.replan_requested(u) != Some(false)
                    || self.world.order_of(u) != Some(Order::Idle)
            })
            .collect();
        if !stuck.is_empty() {
            out.push(format!(
                "{} stuck or still walking at the end: {stuck:?}",
                stuck.len()
            ));
        }
        if self.longest_still > 600 {
            out.push(format!(
                "a unit stood still for {} ticks",
                self.longest_still
            ));
        }
        let limit = Fx::from_ratio((self.packing_radius() * 1.3 * 1000.0) as i64, 1000);
        let outside = self.outside(limit);
        if outside.len() * 100 > UNITS as usize {
            out.push(format!(
                "{} end outside {limit:?} cells: {outside:?}",
                outside.len()
            ));
        }
        if self.worst_overlap.0 > ORTH / 4 {
            out.push(format!(
                "overlap {:.3} cells at tick {}",
                self.worst_overlap.0 as f64 / ORTH as f64,
                self.worst_overlap.1
            ));
        }
        let end = self.worst_end_overlap();
        if end > ORTH / 16 {
            out.push(format!(
                "overlap {:.3} cells at the end",
                end as f64 / ORTH as f64
            ));
        }
        if !self.bad_cells.is_empty() {
            out.push(format!(
                "{} ticks on ground the class cannot enter",
                self.bad_cells.len()
            ));
        }
        out
    }

    /// One line of numbers for a sweep.
    fn summary(&self) -> String {
        let stats = self.world.crowd_stats();
        let limit = Fx::from_ratio((self.packing_radius() * 1.3 * 1000.0) as i64, 1000);
        format!(
            "last arrival {}, outside {:?}: {}, farthest {:?}, worst overlap {:.3} (tick {}), end \
             overlap {:.3}, longest still {}, sidesteps {}, replans {}, shoves {}",
            self.last_arrival(),
            limit,
            self.outside(limit).len(),
            self.farthest(),
            self.worst_overlap.0 as f64 / ORTH as f64,
            self.worst_overlap.1,
            self.worst_end_overlap() as f64 / ORTH as f64,
            self.longest_still,
            stats.sidesteps,
            stats.replans,
            stats.shoves,
        )
    }
}

// --- the criterion at every wheeled radius ---------------------------------------------

/// A case: the scenario with the wheeled turning radius replaced, run, and every way
/// it falls short of the criterion reported at once.
fn phase_four_holds_at(hundredths: i64) -> Run {
    let r = run_scenario(&scenario_with_wheeled_radius(hundredths));
    eprintln!(
        "bridge_200 at a wheeled radius of {}.{:02} cells: {}",
        hundredths / 100,
        hundredths % 100,
        r.summary()
    );
    let misses = r.misses();
    assert!(
        misses.is_empty(),
        "wheeled radius {hundredths}/100 cells: {misses:?}"
    );
    r
}

#[test]
fn phase_four_holds_at_a_wheeled_radius_of_1_25() {
    phase_four_holds_at(125);
}

/// The shipped radius: the run with the radius replaced by the one it already has is
/// the shipped run, bit for bit (the knob changes nothing else).
#[test]
fn phase_four_holds_at_a_wheeled_radius_of_1_5_and_it_is_the_shipped_run() {
    let r = phase_four_holds_at(150);
    assert_eq!(r.history, run().history);
}

#[test]
fn phase_four_holds_at_a_wheeled_radius_of_1_6() {
    phase_four_holds_at(160);
}

#[test]
fn phase_four_holds_at_a_wheeled_radius_of_1_75() {
    phase_four_holds_at(175);
}

#[test]
fn phase_four_holds_at_a_wheeled_radius_of_2() {
    phase_four_holds_at(200);
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
fn all_two_hundred_units_arrive_within_the_limit_and_the_bridge_is_a_real_bottleneck() {
    let r = run();
    let n = r.arrived.iter().flatten().count();
    assert_eq!(n, UNITS as usize, "units that never arrived: {:?}", {
        (0..UNITS)
            .filter(|&s| r.arrived[s as usize].is_none())
            .collect::<Vec<_>>()
    });
    let last = r.last_arrival();
    let stats = r.world.crowd_stats();
    eprintln!(
        "bridge_200: all {UNITS} arrived by tick {last} ({} s of game time; {} s when the units \
         had no bodies), the last crossed the river on tick {:?}; first move per class (foot, \
         wheeled, tracked, hover) {:?}, every unit moving by tick {:?}; fields finished on ticks \
         {:?}; the path service spent at most {} nodes in a tick ({} of them on fields) of {}, \
         {} in all, {} jobs of single routes; the crowd pass: {} ticks with a push, {} idle units \
         shoved, {} sidesteps, {} requests for a new route; longest a unit stood still with its \
         order not done: {} ticks",
        last / 20,
        WITHOUT_BODIES / 20,
        r.all_across,
        r.class_first_move,
        r.all_moving,
        r.field_done,
        r.peak_nodes,
        r.peak_field_nodes,
        r.budget,
        r.total_nodes,
        r.world.path_stats().jobs_done,
        stats.busy_ticks,
        stats.shoves,
        stats.sidesteps,
        stats.replans,
        r.longest_still,
    );
    assert!(last <= ARRIVE_BY, "the last unit arrived on tick {last}");
    // The bridge is three cells wide; the crowd that crosses it has to queue.
    assert!(
        last > WITHOUT_BODIES,
        "with bodies the crossing is slower than without: {last}"
    );
    for s in 0..UNITS {
        assert_eq!(r.world.order_of(id(s)), Some(Order::Idle));
        assert!(!r.world.route_of(id(s)).unwrap().field, "route over");
    }
}

#[test]
fn nobody_is_stuck_at_the_end_or_stood_still_for_long_on_the_way() {
    let r = run();
    for s in 0..UNITS {
        let unit = id(s);
        assert_eq!(
            r.world.stuck_ticks_of(unit),
            Some(0),
            "unit {s} has not come closer for a while at the end"
        );
        assert_eq!(r.world.replan_requested(unit), Some(false), "unit {s}");
        assert_eq!(r.world.order_of(unit), Some(Order::Idle), "unit {s}");
    }
    // In a queue for a bridge three cells wide a unit may stand a while (the
    // measured longest is printed above); nobody stands for half a minute.
    assert!(
        r.longest_still <= 600,
        "a unit stood still for {} ticks with its order not done",
        r.longest_still
    );
}

#[test]
fn at_most_one_percent_end_outside_the_disc_that_holds_the_bodies() {
    let r = run();
    let packing = r.packing_radius();
    let limit = Fx::from_ratio((packing * 1.3 * 1000.0) as i64, 1000);
    let outside = r.outside(limit);
    let farthest = (0..UNITS)
        .map(|s| (r.world.get(id(s)).unwrap().pos - goal()).length())
        .max()
        .unwrap();
    eprintln!(
        "bridge_200: the bodies fill a disc of {packing:.1} cells; {} of {UNITS} are outside \
         {:.1} cells of the goal, {} outside the 5 cells the phase asked for before units had \
         bodies; the farthest {:?} cells; at most {} on the bridge at once, at most {} stepping \
         cell by cell at once",
        outside.len(),
        packing * 1.3,
        r.outside(Fx::from_int(5)).len(),
        farthest,
        r.on_bridge,
        r.cautious
    );
    // Five cells could not hold them (see the module documentation)...
    assert!(
        packing > 5.0,
        "the bodies of 200 units of these sizes fill a disc of {packing:.1} cells"
    );
    // ...but the group is gathered round the goal.
    assert!(
        outside.len() * 100 <= UNITS as usize,
        "outside {:.1} cells: {outside:?}",
        packing * 1.3
    );
}

#[test]
fn bodies_do_not_overlap_beyond_the_slack_on_the_way_or_at_the_end() {
    let r = run();
    let (worst, at) = r.worst_overlap;
    eprintln!(
        "bridge_200: the worst overlap of two bodies in the run is {:.3} cells (tick {at})",
        worst as f64 / ORTH as f64
    );
    // The phase's bound: never more than a quarter of a cell during the run. (The
    // slack is a thirty-second; a packed crowd squeezed against the crowd that has
    // arrived goes over it for a moment.)
    assert!(worst <= ORTH / 4, "worst overlap {worst} at tick {at}");
    // And at the end the crowd has settled: at most a sixteenth of a cell.
    let end = r.history.len() as u64;
    let mut worst_end = 0;
    for a in 0..UNITS {
        for b in a + 1..UNITS {
            let (ra, rb) = (
                r.world.collision_radius_of(id(a)).unwrap(),
                r.world.collision_radius_of(id(b)).unwrap(),
            );
            let d = (r.world.get(id(a)).unwrap().pos - r.world.get(id(b)).unwrap().pos).length();
            worst_end = worst_end.max((keep(ra, rb) - d).raw());
        }
    }
    assert!(
        worst_end <= ORTH / 16,
        "at tick {end} two bodies overlap by {worst_end}"
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
    assert!(r.all_moving.unwrap() <= 40, "{:?}", r.all_moving);
    assert!(
        *r.field_done.last().unwrap() <= 12,
        "fields done: {:?}",
        r.field_done
    );
    // The budget held every tick (asserted in the run); the builder used it.
    assert!(r.peak_field_nodes > 0 && r.peak_nodes <= r.budget);
    // Not one route of its own for the 195 units of a field: the five hover craft
    // are all that was asked of the single-route service (and the crowd pass asking
    // again for a few that were stuck).
    assert!(
        stats.jobs_done < 40 + r.world.crowd_stats().replans,
        "{} jobs for the five hover craft",
        stats.jobs_done
    );
}

#[test]
fn the_units_take_about_the_shortest_way() {
    let r = run();
    let by_class = [FOOT, WHEELED, TRACKED, HOVER].map(|c| (c, grid_distances(&r.world, c)));
    // How much longer than the grid-best walk each unit walked.
    let mut ratios: Vec<(f64, u32, u16)> = Vec::new();
    for s in 0..UNITS {
        let class = r.world.stats_of(id(s)).unwrap().movement_class.unwrap();
        let dist = &by_class.iter().find(|(c, _)| *c == class).unwrap().1;
        let start = r.world.cell_at(r.start[s as usize]).unwrap();
        let best = dist[(start.y * W + start.x) as usize];
        assert_ne!(best, i64::MAX);
        ratios.push((r.walked[s as usize] as f64 / best as f64, s, class));
    }
    ratios.sort_by(|a, b| b.0.total_cmp(&a.0));
    let median = ratios[ratios.len() / 2].0;
    eprintln!(
        "bridge_200: the walks are {median:.2} of the grid-best walk at the median, the longest \
         {:.2} (unit {}, class {}), {:.2} at the 95th percentile",
        ratios[0].0,
        ratios[0].1,
        ratios[0].2,
        ratios[ratios.len() / 20].0
    );
    // A crowd walks further than a single unit (measured: 1.2 at the median): it is pushed, it waits, it goes round
    // and sidesteps, and a body shuffling in a queue adds up. Not twice as far at
    // the median, and not three times as far for anybody.
    assert!(median < 1.5, "median {median:.2}");
    assert!(
        ratios[0].0 < 3.0,
        "unit {} (class {}) walked {:.2} times the grid-best",
        ratios[0].1,
        ratios[0].2,
        ratios[0].0
    );
}

#[test]
fn the_run_is_reproducible() {
    let a = run();
    let b = run_scenario(&Scenario::load(&scenario_path("bridge_200.ron")).unwrap());
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
