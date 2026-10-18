//! The `pathing` and `pathing_256` scenarios (WP4.2, `docs/rules/pathfinding.md`).
//!
//! `pathing` (`assets/scenarios/pathing.ron`, map `assets/maps/pathing`): a
//! maze of rock and water, a river with one bridge, an island that cannot be
//! reached, a structure that closes a gap in a wall, and 24 requests at once.
//! The tests check what the path finder promises, against plain searches of the
//! whole grid that they run themselves:
//!
//! * everything that can be reached is reached, and nothing that cannot is;
//! * no unit ever ends a tick on ground its class cannot enter, or on a cell a
//!   structure occupies;
//! * the distance a unit walked is at most 1.1 times that of the best any-angle
//!   route by the game's own line of sight (the walk is smoothed, so the reference
//!   is too);
//! * an unreachable goal ends in the reachable cell nearest to it, marked;
//! * a structure placed on the route sends the unit another way, and taking it
//!   away restores the routes exactly;
//! * the service never spends more than its budget, and serves in the order of
//!   the entity ids.
//!
//! `pathing_256` is the load test: 200 requests on the 256 by 256 playfield.

mod common;

use std::collections::BinaryHeap;

use common::scenario_path;
use sf_headless::scenario::Scenario;
use sf_math::{Fx, Vec2};
use sf_sim::{Cell, EntityId, Order, RouteStatus, TurnMode, World};

const TRACKED: u16 = 2;

const W: u32 = 96;
const H: u32 = 64;

const MAZE_TANK: u32 = 0;
const MAZE_HOVER: u32 = 1;
const BRIDGE_FOOT: u32 = 2;
const BRIDGE_WHEELED: u32 = 3;
const HAUL_TANK: u32 = 4;
const ISLAND_FOOT: u32 = 5;
const ISLAND_TANK: u32 = 6;
const ISLAND_HOVER: u32 = 7;
const BOAT_DOWN: u32 = 8;
const BOAT_LAND: u32 = 9;
const CROWD: std::ops::Range<u32> = 10..34;
const STRUCTURE: EntityId = EntityId::new(34, 0);

/// The tick by which every one of the 200 requests of `pathing_256` has its first
/// waypoints, at the default budget: 25 % above what is measured.
const SERVE_ALL_BY: u64 = 120;
const PLACED_AT: u64 = 650;
const REMOVED_AT: u64 = 1500;

fn id(slot: u32) -> EntityId {
    EntityId::new(slot, 0)
}

fn mid(x: u32, y: u32) -> Vec2 {
    Vec2::new(
        Fx::from_i64(i64::from(x)) + Fx::HALF,
        Fx::from_i64(i64::from(y)) + Fx::HALF,
    )
}

fn cell_of(world: &World, slot: u32) -> Cell {
    world.cell_at(world.get(id(slot)).unwrap().pos).unwrap()
}

fn class_of(world: &World, slot: u32) -> u16 {
    world.stats_of(id(slot)).unwrap().movement_class.unwrap()
}

// --- the references: plain searches of the whole grid ----------------------------

/// Which cells a class may be in, now: the terrain's answer, and for a class the
/// path finder serves, not a cell a structure occupies.
struct Passable {
    w: u32,
    h: u32,
    open: Vec<bool>,
}

impl Passable {
    fn of(world: &World, class: u16) -> Passable {
        let (w, h) = world.map_size();
        let blocked_by_structures = world.paths_class(class);
        let open = (0..w * h)
            .map(|i| {
                let c = Cell::new(i % w, i / w);
                world.passable(class, c) && !(blocked_by_structures && world.is_occupied(c))
            })
            .collect();
        Passable { w, h, open }
    }

    /// The terrain alone: structures are not looked at.
    fn terrain_only(world: &World, class: u16) -> Passable {
        let (w, h) = world.map_size();
        let open = (0..w * h)
            .map(|i| world.passable(class, Cell::new(i % w, i / w)))
            .collect();
        Passable { w, h, open }
    }

    fn at(&self, x: i64, y: i64) -> bool {
        x >= 0
            && y >= 0
            && x < i64::from(self.w)
            && y < i64::from(self.h)
            && self.open[(y as u32 * self.w + x as u32) as usize]
    }
}

const ORTH: i64 = 65_536;
const DIAG: i64 = 92_682;

/// The length of the shortest walk from `from` to every cell (eight neighbours,
/// no cutting a corner), in raw fixed point where a cell is 65,536; `i64::MAX`
/// where there is none.
fn distances(p: &Passable, from: Cell) -> Vec<i64> {
    let mut dist = vec![i64::MAX; (p.w * p.h) as usize];
    let start = (from.y * p.w + from.x) as usize;
    dist[start] = 0;
    let mut heap = BinaryHeap::new();
    heap.push(std::cmp::Reverse((0_i64, start)));
    while let Some(std::cmp::Reverse((d, at))) = heap.pop() {
        if d > dist[at] {
            continue;
        }
        let (x, y) = ((at as u32 % p.w) as i64, (at as u32 / p.w) as i64);
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
            if !p.at(nx, ny) || (dx != 0 && dy != 0 && !(p.at(nx, y) && p.at(x, ny))) {
                continue;
            }
            let step = if dx != 0 && dy != 0 { DIAG } else { ORTH };
            let n = (ny as u32 * p.w + nx as u32) as usize;
            if d + step < dist[n] {
                dist[n] = d + step;
                heap.push(std::cmp::Reverse((d + step, n)));
            }
        }
    }
    dist
}

/// The shortest route a unit could walk by the rules of the game, from the
/// middle of one cell to the middle of another, at any angle: Theta* over the
/// cells (a node takes its parent's parent when that one sees it, by the thick
/// line of sight the path finder smooths with, `World::sight_clear`) with the
/// straight distance as the heuristic. Raw fixed point; `i64::MAX` if there is
/// no way. The fair reference for a smoothed route: [`distances`] is the best
/// walk along the grid, which cannot cut a corner.
fn any_angle_best(world: &World, class: u16, p: &Passable, from: Cell, to: Cell) -> i64 {
    use std::cmp::Reverse;
    let w = p.w;
    let at = |i: usize| mid(i as u32 % w, i as u32 / w);
    let n = (p.w * p.h) as usize;
    let (start, goal) = ((from.y * w + from.x) as usize, (to.y * w + to.x) as usize);
    let mut dist = vec![i64::MAX; n];
    let mut parent = vec![usize::MAX; n];
    let mut done = vec![false; n];
    dist[start] = 0;
    parent[start] = start;
    let mut heap = BinaryHeap::new();
    heap.push(Reverse((at(start).distance(at(goal)).raw(), start)));
    while let Some(Reverse((_, s))) = heap.pop() {
        if std::mem::replace(&mut done[s], true) {
            continue;
        }
        if s == goal {
            return dist[s];
        }
        let (x, y) = ((s as u32 % w) as i64, (s as u32 / w) as i64);
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
            if !p.at(nx, ny) || (dx != 0 && dy != 0 && !(p.at(nx, y) && p.at(x, ny))) {
                continue;
            }
            let m = (ny as u32 * w + nx as u32) as usize;
            if done[m] {
                continue;
            }
            let q = parent[s];
            let (via, d) = if q != s && world.sight_clear(class, at(q), at(m)) {
                (q, dist[q] + at(q).distance(at(m)).raw())
            } else {
                (s, dist[s] + at(s).distance(at(m)).raw())
            };
            if d < dist[m] {
                dist[m] = d;
                parent[m] = via;
                heap.push(Reverse((d + at(m).distance(at(goal)).raw(), m)));
            }
        }
    }
    i64::MAX
}

fn octile(a: Cell, b: Cell) -> i64 {
    let (dx, dy) = (i64::from(a.x.abs_diff(b.x)), i64::from(a.y.abs_diff(b.y)));
    ORTH * (dx.max(dy) - dx.min(dy)) + DIAG * dx.min(dy)
}

/// The smallest octile distance to `goal` of any cell reachable from `from`.
fn nearest_distance(p: &Passable, from: Cell, goal: Cell) -> i64 {
    let dist = distances(p, from);
    (0..p.w * p.h)
        .filter(|&i| dist[i as usize] != i64::MAX)
        .map(|i| octile(Cell::new(i % p.w, i / p.w), goal))
        .min()
        .unwrap()
}

// --- the run -------------------------------------------------------------------------

/// Everything the scenario does, recorded tick by tick.
struct Run {
    world: World,
    /// The first tick at which each of the first 34 units had no order any more.
    arrived: Vec<Option<u64>>,
    /// The distance each walked, in raw fixed point.
    walked: Vec<i64>,
    /// The cells each stood in at the end of every tick.
    cells: Vec<Vec<Cell>>,
    /// The first tick (from 2 on) at which each had a route to follow, or none.
    routed: Vec<Option<u64>>,
    /// The most nodes the path service spent in any tick, and the budget.
    peak: u32,
    budget: u32,
    /// The distance the tank of the structure walked up to the tick it was
    /// placed, where it stood then, and the grids before and after.
    haul_before: i64,
    haul_cell_then: Cell,
    before: Option<Vec<bool>>,
    after: Option<Vec<bool>>,
    /// Routes planned for a tracked unit across the wall: before the structure,
    /// with it, and after it is gone.
    plan_before: Option<sf_sim::PlannedRoute>,
    plan_with: Option<sf_sim::PlannedRoute>,
    plan_after: Option<sf_sim::PlannedRoute>,
    /// The checksums after every tick.
    history: Vec<u64>,
}

fn crossing_plan(world: &World) -> Option<sf_sim::PlannedRoute> {
    world.plan_route(TRACKED, mid(66, 46), mid(80, 46))
}

fn run() -> Run {
    let scenario = Scenario::load(&scenario_path("pathing.ron")).unwrap();
    let mut world = scenario.build_world().unwrap();
    let units = 34;
    let mut r = Run {
        arrived: vec![None; units],
        walked: vec![0; units],
        cells: vec![Vec::new(); units],
        routed: vec![None; units],
        peak: 0,
        budget: world.path_stats().budget,
        haul_before: 0,
        haul_cell_then: Cell::new(0, 0),
        before: None,
        after: None,
        plan_before: None,
        plan_with: None,
        plan_after: None,
        history: Vec::new(),
        world: world.clone(),
    };
    let mut last: Vec<Vec2> = Vec::new();
    for tick in 0..scenario.ticks {
        if tick == PLACED_AT {
            r.haul_before = r.walked[HAUL_TANK as usize];
            r.haul_cell_then = cell_of(&world, HAUL_TANK);
            r.plan_before = crossing_plan(&world);
            r.before = Some(Passable::of(&world, TRACKED).open);
        }
        let report = world.step();
        r.history.push(report.checksum);
        let stats = world.path_stats();
        assert!(
            stats.nodes_last_tick <= stats.budget,
            "tick {tick}: the path service spent {} nodes, the budget is {}",
            stats.nodes_last_tick,
            stats.budget
        );
        r.peak = r.peak.max(stats.nodes_last_tick);
        if tick == PLACED_AT {
            r.plan_with = crossing_plan(&world);
            r.after = Some(Passable::of(&world, TRACKED).open);
        }
        if tick == REMOVED_AT {
            r.plan_after = crossing_plan(&world);
        }
        let now: Vec<Vec2> = (0..units as u32)
            .map(|s| world.get(id(s)).unwrap().pos)
            .collect();
        for s in 0..units {
            if !last.is_empty() {
                r.walked[s] += last[s].distance(now[s]).raw();
            }
            r.cells[s].push(world.cell_at(now[s]).unwrap());
            if report.tick.0 >= 2
                && r.arrived[s].is_none()
                && world.order_of(id(s as u32)) == Some(Order::Idle)
            {
                r.arrived[s] = Some(report.tick.0);
            }
            if report.tick.0 >= 2
                && r.routed[s].is_none()
                && world.route_of(id(s as u32)).is_some_and(|v| {
                    v.status != RouteStatus::Waiting && v.status != RouteStatus::None
                })
            {
                r.routed[s] = Some(report.tick.0);
            }
        }
        last = now;
    }
    r.world = world;
    r
}

impl Run {
    fn start_of(&self, slot: u32) -> Cell {
        self.cells[slot as usize][0]
    }

    fn end_of(&self, slot: u32) -> Cell {
        cell_of(&self.world, slot)
    }
}

// --- the tests -----------------------------------------------------------------------

#[test]
fn every_reachable_unit_arrives_and_every_unreachable_one_says_so() {
    let r = run();
    for slot in (0..34).filter(|s| ![ISLAND_FOOT, ISLAND_TANK, BOAT_LAND].contains(s)) {
        assert_eq!(
            r.world.order_of(id(slot)),
            Some(Order::Idle),
            "unit {slot} did not arrive"
        );
    }
    let goals = [
        (MAZE_TANK, (41, 25)),
        (MAZE_HOVER, (41, 25)),
        (BRIDGE_FOOT, (60, 58)),
        (BRIDGE_WHEELED, (60, 60)),
        (HAUL_TANK, (90, 46)),
        (ISLAND_HOVER, (72, 17)),
        (BOAT_DOWN, (46, 60)),
    ];
    for (slot, (x, y)) in goals {
        assert_eq!(r.end_of(slot), Cell::new(x, y), "unit {slot}");
    }
    for slot in CROWD {
        assert_eq!(r.end_of(slot), Cell::new(60, 62), "crowd {slot}");
    }
    // Nothing is left over.
    for slot in 0..34 {
        let view = r.world.route_of(id(slot)).unwrap();
        let unreachable = [ISLAND_FOOT, ISLAND_TANK, BOAT_LAND].contains(&slot);
        assert_eq!(view.unreachable, unreachable, "unit {slot}");
        assert_eq!(
            view.status,
            if unreachable {
                RouteStatus::Stranded
            } else {
                RouteStatus::None
            },
            "unit {slot}"
        );
    }
}

#[test]
fn no_unit_ever_ends_a_tick_where_its_class_cannot_be() {
    let r = run();
    for slot in 0..34_u32 {
        let class = class_of(&r.world, slot);
        for (tick, c) in r.cells[slot as usize].iter().enumerate() {
            assert!(
                r.world.passable(class, *c),
                "unit {slot} stood on {c} (class {class}) after tick {tick}"
            );
            // The structure stood from tick 650 to tick 1500.
            let structure = (PLACED_AT as usize..REMOVED_AT as usize).contains(&tick)
                && (72..=75).contains(&c.x)
                && (44..=47).contains(&c.y);
            assert!(
                !structure,
                "unit {slot} stood in the structure after tick {tick}"
            );
        }
    }
}

/// How far each unit walked against the best route there is, measured like with
/// like: the walk of a unit is smoothed and at any angle, so the reference is the
/// best any-angle route by the same line-of-sight rule ([`any_angle_best`]). The
/// best walk along the grid is printed beside it: a walk that cuts corners is
/// shorter than that.
#[test]
fn routes_are_at_most_ten_percent_longer_than_the_best() {
    let r = run();
    let (mut worst, mut worst_grid) = (0, 0);
    // The terrain only: the structure of the scenario is not on these units'
    // way (the tank that it is on the way of is checked apart).
    for slot in
        (0..34_u32).filter(|s| ![ISLAND_FOOT, ISLAND_TANK, BOAT_LAND, HAUL_TANK].contains(s))
    {
        let class = class_of(&r.world, slot);
        let p = Passable::terrain_only(&r.world, class);
        let (start, end) = (r.start_of(slot), r.end_of(slot));
        let best = any_angle_best(&r.world, class, &p, start, end);
        assert_ne!(best, i64::MAX, "unit {slot} has no way");
        let grid = distances(&p, start)[(end.y * W + end.x) as usize];
        let walked = r.walked[slot as usize];
        // The unit starts in the middle of its cell and ends within a tolerance of
        // the middle of the goal: a little under or over the cell-to-cell length.
        let ratio = walked * 1000 / best;
        worst = worst.max(ratio);
        worst_grid = worst_grid.max(walked * 1000 / grid);
        // A unit that has to move to turn (WP4.5) swings wide round the corners of a
        // route and may not get as close to the line as one that pivots: a quarter
        // more than the best route, a tenth for the others.
        let arc = r.world.movement_table().turning(class).mode == TurnMode::Arc;
        let allowed = if arc { 125 } else { 110 };
        assert!(
            walked * 100 <= best * allowed,
            "unit {slot}: walked {walked}, the best any-angle route is {best} ({ratio} per thousand, at most {allowed}0 allowed)"
        );
    }
    eprintln!(
        "pathing: the longest walk relative to the best any-angle route is {worst} per thousand \
         ({worst_grid} relative to the best walk along the grid)"
    );
}

#[test]
fn an_unreachable_goal_ends_in_the_nearest_reachable_cell() {
    let r = run();
    for (slot, goal) in [
        (ISLAND_FOOT, Cell::new(72, 17)),
        (ISLAND_TANK, Cell::new(72, 17)),
        (BOAT_LAND, Cell::new(30, 50)),
    ] {
        let p = Passable::terrain_only(&r.world, class_of(&r.world, slot));
        let end = r.end_of(slot);
        let want = nearest_distance(&p, r.start_of(slot), goal);
        assert_eq!(
            octile(end, goal),
            want,
            "unit {slot} ended in {end}, not as near to {goal} as it could get"
        );
        let view = r.world.route_of(id(slot)).unwrap();
        assert!(view.unreachable && view.status == RouteStatus::Stranded);
        assert_eq!(view.goal, Some(end));
        // The order is kept, standing in the middle of the cell.
        assert!(matches!(r.world.order_of(id(slot)), Some(Order::Move(_))));
        assert_eq!(r.world.get(id(slot)).unwrap().pos, mid(end.x, end.y));
    }
    // Land and tracks stop on the shore of the lake; the hover craft crosses it.
    assert_eq!(r.end_of(ISLAND_FOOT).y, 27, "the south shore");
    assert_eq!(r.end_of(ISLAND_HOVER), Cell::new(72, 17));
    // The gunboat sent onto the land stops at the edge of the river.
    assert_eq!(r.end_of(BOAT_LAND), Cell::new(44, 50));
}

#[test]
fn the_bridge_is_the_only_way_across_the_river() {
    let r = run();
    for slot in [BRIDGE_FOOT, BRIDGE_WHEELED, HAUL_TANK]
        .into_iter()
        .chain(CROWD)
    {
        let cells = &r.cells[slot as usize];
        // Every cell it stood in inside the river is on the bridge row.
        assert!(
            cells.iter().all(|c| !(44..=49).contains(&c.x) || c.y == 40),
            "unit {slot} was in the river"
        );
        assert!(
            cells.iter().any(|c| (44..=49).contains(&c.x) && c.y == 40),
            "unit {slot} never used the bridge"
        );
    }
    // The hover craft does not need it and the gunboat sails under it.
    assert!(r.cells[BOAT_DOWN as usize].contains(&Cell::new(46, 40)));
    assert!(
        r.cells[ISLAND_HOVER as usize]
            .iter()
            .all(|c| !(44..=49).contains(&c.x) || c.y != 40),
        "the hover craft needs no bridge"
    );
}

#[test]
fn the_maze_is_snaked_through_by_land_and_crossed_by_hover() {
    let r = run();
    let (tank, hover) = (MAZE_TANK as usize, MAZE_HOVER as usize);
    // The tank passes every gap of the walls (y 7, 12, 17, 22), at the end the
    // gap is in; the hover craft crosses the water walls where it likes.
    for (row, columns) in [(7, 34..=36), (12, 3..=5), (17, 34..=36), (22, 3..=5)] {
        assert!(
            r.cells[tank]
                .iter()
                .any(|c| c.y == row && columns.contains(&c.x)),
            "the tank never passed the gap in row {row}"
        );
    }
    assert!(
        r.cells[hover]
            .iter()
            .any(|c| c.y == 12 && !(3..=5).contains(&c.x)),
        "the hover craft crossed the water wall in row 12 outside the gap"
    );
    // The tank walks some four times as far as the hover craft.
    assert!(r.walked[tank] > 2 * r.walked[hover]);
    // And through the one-cell door of the room.
    assert!(r.cells[tank].contains(&Cell::new(38, 25)));
}

#[test]
fn a_structure_in_front_of_a_gap_sends_the_unit_through_the_other() {
    let r = run();
    let tank = HAUL_TANK as usize;
    // Up to tick 650 the tank had not reached the wall; it was on its way.
    assert!(r.haul_cell_then.x < 70, "{:?}", r.haul_cell_then);
    let seen = &r.cells[tank];
    let after: Vec<&Cell> = seen[PLACED_AT as usize..].iter().collect();
    assert!(
        after
            .iter()
            .any(|c| (70..=71).contains(&c.x) && (56..=59).contains(&c.y)),
        "it never went through the second gap"
    );
    assert!(
        after
            .iter()
            .all(|c| !((70..=71).contains(&c.x) && (44..=47).contains(&c.y))),
        "it went into the closed gap"
    );
    assert_eq!(r.end_of(HAUL_TANK), Cell::new(90, 46));

    // What it walked: its way so far, and then the best route from where it
    // stood, on the grid with the structure on it.
    let after_grid = Passable {
        w: W,
        h: H,
        open: r.after.clone().unwrap(),
    };
    let rest = distances(&after_grid, r.haul_cell_then)[(46 * W + 90) as usize];
    assert_ne!(rest, i64::MAX);
    let total = r.walked[tank];
    assert!(
        total * 10 <= (r.haul_before + rest) * 11,
        "walked {total}: {} before the structure and a best route of {rest} after",
        r.haul_before
    );
}

#[test]
fn the_graph_follows_the_structure_and_comes_back_exactly() {
    let r = run();
    let (before, with, after) = (
        r.plan_before.clone().unwrap(),
        r.plan_with.clone().unwrap(),
        r.plan_after.clone().unwrap(),
    );
    // The row in which the route crosses the middle of the wall, x = 71.
    let crossing = |plan: &sf_sim::PlannedRoute| {
        let mut at = mid(66, 46);
        let wall = Fx::from_int(71);
        for &w in &plan.waypoints {
            if (at.x < wall) != (w.x < wall) {
                let t = (wall - at.x).raw() * 1000 / (w.x - at.x).raw();
                let y = at.y.raw() + (w.y - at.y).raw() * t / 1000;
                return Some(y.div_euclid(65_536));
            }
            at = w;
        }
        None
    };
    // Before: the first gap. With the structure in front of it: the second, a
    // longer way. After it is gone: the first again, and the plan is the one
    // made before, waypoint for waypoint.
    assert!(
        matches!(crossing(&before), Some(44..=47)),
        "{:?}",
        crossing(&before)
    );
    assert!(
        matches!(crossing(&with), Some(56..=59)),
        "{:?}",
        crossing(&with)
    );
    assert_eq!(before, after);
    let length = |p: &sf_sim::PlannedRoute| {
        let mut at = mid(66, 46);
        p.waypoints.iter().fold(0_i64, |sum, w| {
            let d = at.distance(*w).raw();
            at = *w;
            sum + d
        })
    };
    assert!(length(&with) > length(&before));
    // No waypoint of any of them is in the structure's cells.
    assert!(!with.waypoints.iter().any(|w| {
        let (x, y) = w.cell();
        (72..=75).contains(&(x as u32)) && (44..=47).contains(&(y as u32))
    }));
    // The structure is gone at the end.
    assert!(!r.world.is_alive(STRUCTURE));
}

#[test]
fn the_service_stays_inside_its_budget_and_serves_in_the_order_of_the_ids() {
    let r = run();
    assert!(
        r.peak > 0 && r.peak <= r.budget,
        "peak {} budget {}",
        r.peak,
        r.budget
    );
    // The crowd asked on the same tick with the same request: the first tick at
    // which each had its route never goes down with the slot.
    let served: Vec<u64> = CROWD
        .map(|s| r.routed[s as usize].unwrap_or_else(|| panic!("crowd {s} was never served")))
        .collect();
    assert!(
        served.windows(2).all(|p| p[0] <= p[1]),
        "served out of order: {served:?}"
    );
    // The budget mattered: they did not all get theirs on the same tick.
    assert!(served.last() > served.first(), "{served:?}");
}

#[test]
fn the_run_is_reproducible() {
    let (a, b) = (run(), run());
    assert_eq!(a.history, b.history);
    assert_eq!(a.walked, b.walked);
    assert_eq!(a.arrived, b.arrived);
}

#[test]
fn the_budget_decides_when_and_not_where() {
    // The same scenario on the smallest budget and on a large one: every unit
    // ends in the same cell with the same order; only the times differ.
    let scenario = Scenario::load(&scenario_path("pathing.ron")).unwrap();
    let ends = |budget: u32| {
        let mut s = scenario.clone();
        s.path_budget = Some(budget);
        let mut world = s.build_world().unwrap();
        for _ in 0..s.ticks {
            world.step();
        }
        (0..34)
            .map(|slot| (cell_of(&world, slot), world.order_of(id(slot))))
            .collect::<Vec<_>>()
    };
    let small = ends(sf_sim::MIN_NODE_BUDGET);
    let large = ends(sf_sim::DEFAULT_NODE_BUDGET * 4);
    assert_eq!(small, large);
}

#[test]
fn two_hundred_requests_on_the_256_map_are_served_inside_the_budget() {
    let scenario = Scenario::load(&scenario_path("pathing_256.ron")).unwrap();
    let mut world = scenario.build_world().unwrap();
    let budget = world.path_stats().budget;
    let mut first_route: Vec<Option<u64>> = vec![None; 200];
    for _ in 0..scenario.ticks {
        let report = world.step();
        let stats = world.path_stats();
        assert!(stats.nodes_last_tick <= budget, "tick {}", report.tick.0);
        for (slot, served) in first_route.iter_mut().enumerate() {
            if served.is_none()
                && report.tick.0 >= 2
                && world.route_of(id(slot as u32)).is_some_and(|v| {
                    matches!(v.status, RouteStatus::Following | RouteStatus::Stranded)
                })
            {
                *served = Some(report.tick.0);
            }
        }
    }
    let served: Vec<u64> = first_route
        .iter()
        .enumerate()
        .map(|(slot, t)| t.unwrap_or_else(|| panic!("unit {slot} was never served")))
        .collect();
    // Asked on tick 1, so the requests are queued from tick 2: lowest id first.
    assert!(
        served.windows(2).all(|p| p[0] <= p[1]),
        "served out of the order of the ids"
    );
    let last = *served.last().unwrap();
    let stats = world.path_stats();
    eprintln!(
        "pathing_256: 200 requests served after {last} ticks ({} s of game time), \
         budget {budget} nodes per tick, the busiest tick spent {}, {} jobs in all",
        last / 20,
        stats.peak_nodes,
        stats.jobs_done
    );
    // The first request is served on the first tick it can be.
    assert_eq!(served[0], 2);
    // The last of 200 single-unit requests gets its first waypoints within
    // SERVE_ALL_BY ticks (measured: 85, about 600 jobs of 750 to 1,500 nodes
    // against 4,096 nodes a tick). A group order this size is WP4.3's flowfield,
    // not 200 searches (`docs/rules/pathfinding.md`); the limit is here so that a
    // slowdown of the search shows as a failing test and not as a bigger number
    // on a screen.
    assert!(
        last <= SERVE_ALL_BY,
        "the last of 200 requests was served on tick {last}, the limit is {SERVE_ALL_BY}"
    );
    assert!(last < scenario.ticks, "served within the run");
}
