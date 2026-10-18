//! Tests of the planner through `Pathing`: whole routes on small maps, checked
//! against a plain grid search.

use sf_math::{Fx, Vec2};

use super::*;
use crate::movement::ClassCosts;
use crate::terrain::{TerrainDef, TerrainFlags, TerrainId};

const GRASS: TerrainId = TerrainId(0);
const ROAD: TerrainId = TerrainId(1);
const WATER: TerrainId = TerrainId(2);
const ROCK: TerrainId = TerrainId(3);
const BRIDGE: TerrainId = TerrainId(5);

const FOOT: u16 = 0;
const BOAT: u16 = 1;
const FLYER: u16 = 2;

fn defs() -> Vec<TerrainDef> {
    let open = TerrainDef {
        id: GRASS,
        flags: TerrainFlags::OPEN,
    };
    let blocked = TerrainFlags::default();
    vec![
        open,
        TerrainDef {
            id: ROAD,
            flags: TerrainFlags::OPEN,
        },
        TerrainDef {
            id: WATER,
            flags: blocked,
        },
        TerrainDef {
            id: ROCK,
            flags: blocked,
        },
        TerrainDef {
            id: TerrainId(4),
            flags: TerrainFlags::OPEN,
        },
        TerrainDef {
            id: BRIDGE,
            flags: TerrainFlags::OPEN,
        },
    ]
}

fn movement() -> MovementTable {
    let x = Cost::Impassable;
    let s = Cost::Speed;
    let class = |class, list: [Cost; 6]| ClassCosts {
        class,
        costs: [0_u8, 1, 2, 3, 4, 5]
            .into_iter()
            .map(TerrainId)
            .zip(list)
            .collect(),
    };
    MovementTable::new(vec![
        class(FOOT, [s(1000), s(1500), x, x, s(500), s(1200)]),
        class(BOAT, [x, x, s(1000), x, x, s(1000)]),
        class(FLYER, [s(1000); 6]),
    ])
    .unwrap()
}

/// A map from rows of text: `.` grass, `r` road, `~` water, `#` rock, `=` bridge.
fn terrain(rows: &[&str]) -> Terrain {
    let (w, h) = (rows[0].len() as u32, rows.len() as u32);
    let ids: Vec<u8> = rows
        .iter()
        .flat_map(|r| r.bytes())
        .map(|c| match c {
            b'.' => GRASS.0,
            b'r' => ROAD.0,
            b'~' => WATER.0,
            b'#' => ROCK.0,
            b'=' => BRIDGE.0,
            other => panic!("unknown map character {}", other as char),
        })
        .collect();
    Terrain::from_layer(w, h, &ids, &defs()).unwrap()
}

fn pathing_for(terrain: &Terrain) -> Pathing {
    let occupancy = Occupancy::new(terrain.width(), terrain.height());
    Pathing::build(terrain, &movement(), &occupancy, DEFAULT_NODE_BUDGET)
}

fn mid(x: u32, y: u32) -> Vec2 {
    Vec2::new(
        Fx::from_i64(i64::from(x)) + Fx::HALF,
        Fx::from_i64(i64::from(y)) + Fx::HALF,
    )
}

/// The length of a polyline in raw fixed point.
fn length(from: Vec2, waypoints: &[Vec2]) -> i64 {
    let mut at = from;
    let mut total = 0;
    for &w in waypoints {
        total += at.distance(w).raw();
        at = w;
    }
    total
}

const ORTH: i64 = 65_536;
const DIAG: i64 = 92_682;

/// The cheapest walk between two cells on a grid of uniform speed, by plain
/// Dijkstra over the eight neighbours with the movement rule; `None` if there
/// is none. In raw fixed point, where one cell is 65 536 units of it.
fn reference(terrain: &Terrain, from: (u32, u32), to: (u32, u32)) -> Option<i64> {
    let (w, h) = (terrain.width() as i64, terrain.height() as i64);
    let table = movement();
    let open = |x: i64, y: i64| {
        x >= 0
            && y >= 0
            && x < w
            && y < h
            && table
                .get(FOOT, terrain.id_at(Cell::new(x as u32, y as u32)).unwrap())
                .flatten()
                .is_some_and(Cost::is_passable)
    };
    let mut dist = vec![i64::MAX; (w * h) as usize];
    let start = (from.1 as i64 * w + from.0 as i64) as usize;
    dist[start] = 0;
    let mut heap = std::collections::BinaryHeap::new();
    heap.push(std::cmp::Reverse((0_i64, start)));
    while let Some(std::cmp::Reverse((d, b))) = heap.pop() {
        if d > dist[b] {
            continue;
        }
        let (x, y) = (b as i64 % w, b as i64 / w);
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
            let n = (ny * w + nx) as usize;
            if d + step < dist[n] {
                dist[n] = d + step;
                heap.push(std::cmp::Reverse((d + step, n)));
            }
        }
    }
    let d = dist[(to.1 as i64 * w + to.0 as i64) as usize];
    (d != i64::MAX).then_some(d)
}

fn cell_of(p: Vec2) -> (u32, u32) {
    let (x, y) = p.cell();
    (x as u32, y as u32)
}

#[test]
fn open_ground_is_a_straight_line_in_a_few_waypoints() {
    let t = terrain(&vec![&".".repeat(80)[..]; 60]);
    let p = pathing_for(&t);
    let (from, to) = (mid(2, 3), mid(75, 52));
    let plan = p.plan(FOOT, from, to).unwrap();
    assert!(!plan.unreachable);
    assert_eq!(plan.waypoints.last(), Some(&to));
    let walked = length(from, &plan.waypoints);
    // Not longer than the best walk along the grid (which cannot cut across
    // cells), and nowhere near one waypoint per cell.
    let best = reference(&t, (2, 3), (75, 52)).unwrap();
    assert!(walked <= best, "{walked} against {best}");
    assert!(walked * 100 <= from.distance(to).raw() * 106);
    assert!(
        plan.waypoints.len() <= 14,
        "{} waypoints",
        plan.waypoints.len()
    );
}

#[test]
fn a_wall_with_a_gap_is_walked_around_through_the_gap() {
    let mut rows = vec![".".repeat(64); 64];
    for (y, row) in rows.iter_mut().enumerate() {
        if y != 40 {
            row.replace_range(32..34, "##");
        }
    }
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    let t = terrain(&refs);
    let p = pathing_for(&t);
    let plan = p.plan(FOOT, mid(5, 10), mid(58, 12)).unwrap();
    assert!(!plan.unreachable);
    // Every waypoint is on open ground, and the gap is passed.
    for w in &plan.waypoints {
        let c = cell_of(*w);
        assert_ne!(t.id_at(Cell::new(c.0, c.1)), Some(ROCK), "{c:?}");
    }
    assert!(
        plan.waypoints
            .iter()
            .any(|w| (32..34).contains(&cell_of(*w).0) || cell_of(*w).1 == 40),
        "no waypoint at the gap: {:?}",
        plan.waypoints
    );
    let best = reference(&t, (5, 10), (58, 12)).unwrap();
    let walked = length(mid(5, 10), &plan.waypoints);
    assert!(walked * 10 <= best * 11, "walked {walked}, best {best}");
}

#[test]
fn an_island_is_unreachable_and_the_route_leads_to_the_nearest_shore() {
    let mut rows = vec![".".repeat(64); 40];
    // A lake across the whole map with an island of grass in it.
    for row in rows.iter_mut().take(30).skip(10) {
        row.replace_range(0..64, &"~".repeat(64));
    }
    for row in rows.iter_mut().take(24).skip(18) {
        row.replace_range(40..48, "........");
    }
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    let t = terrain(&refs);
    let p = pathing_for(&t);
    let plan = p.plan(FOOT, mid(44, 5), mid(44, 21)).unwrap();
    assert!(plan.unreachable);
    // The north shore, straight above the island (column 44, row 9).
    assert_eq!(plan.goal, Cell::new(44, 9));
    let last = *plan.waypoints.last().unwrap();
    assert_eq!(cell_of(last), (44, 9));
}

#[test]
fn a_goal_in_blocked_ground_leads_to_the_nearest_open_cell() {
    let t = terrain(&[
        "................",
        "......###.......",
        "......###.......",
        "................",
    ]);
    let p = pathing_for(&t);
    let plan = p.plan(FOOT, mid(1, 1), mid(7, 1)).unwrap();
    assert!(plan.unreachable);
    // (7, 1) is rock: the nearest open cells are (5, 1) and (9, 1) are 2 away;
    // (7, 0) is one away.
    assert_eq!(plan.goal, Cell::new(7, 0));
}

#[test]
fn a_class_the_terrain_does_not_restrict_has_no_graph() {
    let t = terrain(&["....", "....", "....", "...."]);
    let p = pathing_for(&t);
    assert!(p.paths_class(FOOT));
    assert!(p.paths_class(BOAT));
    assert!(!p.paths_class(FLYER));
    assert!(p.plan(FLYER, mid(0, 0), mid(3, 3)).is_none());
}

#[test]
fn a_bridge_is_the_only_way_across_a_river_for_foot_and_the_boat_goes_under_it() {
    let mut rows = vec![".".repeat(48); 32];
    for row in rows.iter_mut() {
        row.replace_range(20..24, "~~~~");
    }
    rows[9].replace_range(20..24, "====");
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    let t = terrain(&refs);
    let p = pathing_for(&t);
    let plan = p.plan(FOOT, mid(4, 25), mid(43, 25)).unwrap();
    assert!(!plan.unreachable);
    // It goes up to the bridge row and across it.
    assert!(plan.waypoints.iter().any(|w| cell_of(*w).1 <= 10));
    let best = reference(&t, (4, 25), (43, 25)).unwrap();
    let walked = length(mid(4, 25), &plan.waypoints);
    assert!(walked * 10 <= best * 11, "walked {walked}, best {best}");
    // The boat sails the river from one end to the other, under the bridge.
    let boat = p.plan(BOAT, mid(21, 1), mid(22, 30)).unwrap();
    assert!(!boat.unreachable);
    assert_eq!(boat.waypoints.last(), Some(&mid(22, 30)));
}

/// A deterministic generator for the quality tests.
fn xorshift(seed: u64) -> impl FnMut() -> u64 {
    let mut s = seed;
    move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    }
}

/// A map of 70 by 55 cells with clumps of rock of three densities (by `round`),
/// and its rock cells.
fn random_rock_map(round: usize, next: &mut impl FnMut() -> u64) -> (Terrain, Vec<bool>) {
    let (w, h) = (70_u32, 55_u32);
    // Clumps of rock: a seed per cell, then each seed grows a few cells.
    let mut rock = vec![false; (w * h) as usize];
    let density = [18, 24, 30][round % 3];
    for _ in 0..(w * h * density / 100 / 5) {
        let (x, y) = (next() as u32 % w, next() as u32 % h);
        let (cw, ch) = (1 + next() as u32 % 4, 1 + next() as u32 % 4);
        for dy in 0..ch {
            for dx in 0..cw {
                if x + dx < w && y + dy < h {
                    rock[((y + dy) * w + x + dx) as usize] = true;
                }
            }
        }
    }
    let rows: Vec<String> = (0..h)
        .map(|y| {
            (0..w)
                .map(|x| if rock[(y * w + x) as usize] { '#' } else { '.' })
                .collect()
        })
        .collect();
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    (terrain(&refs), rock)
}

/// The shortest route a unit could walk by the rules of the game, from the
/// middle of one cell to the middle of another: any angle, every segment clear
/// by the thick line of sight and at most [`los::LOS_MAX_CELLS`] long (longer
/// stretches are made of several, which costs nothing on a straight line).
/// Theta*: A* over the cells in which a node may take its parent's parent when
/// that one sees it, with the straight distance as the heuristic. `None` if
/// the goal cannot be reached. In raw fixed point.
fn any_angle_reference(grid: &Grid, from: (u32, u32), to: (u32, u32)) -> Option<i64> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let w = grid.width();
    let at = |i: usize| mid(i as u32 % w, i as u32 / w);
    let sees = |a: usize, b: usize| {
        let (p, q) = (at(a), at(b));
        los::short_enough(p, q) && los::thick_clear(grid, p, q).0
    };
    let n = (w * grid.height()) as usize;
    let (start, goal) = ((from.1 * w + from.0) as usize, (to.1 * w + to.0) as usize);
    let mut dist = vec![i64::MAX; n];
    let mut parent = vec![usize::MAX; n];
    let mut done = vec![false; n];
    dist[start] = 0;
    parent[start] = start;
    let mut heap = BinaryHeap::new();
    heap.push(Reverse((at(start).distance(at(goal)).raw(), start)));
    while let Some(Reverse((_, s))) = heap.pop() {
        if done[s] {
            continue;
        }
        done[s] = true;
        if s == goal {
            return Some(dist[s]);
        }
        let (sx, sy) = (s as u32 % w, s as u32 / w);
        for (dx, dy) in grid::DIRS {
            if !grid.can_step(sx, sy, dx, dy) {
                continue;
            }
            let m = ((i64::from(sy) + dy) as u32 * w + (i64::from(sx) + dx) as u32) as usize;
            if done[m] {
                continue;
            }
            let p = parent[s];
            let (via, d) = if p != s && sees(p, m) {
                (p, dist[p] + at(p).distance(at(m)).raw())
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
    None
}

#[test]
fn routes_stay_close_to_the_best_on_random_maps() {
    let mut next = xorshift(0x2545_F491_4F6C_DD1D);
    // The ratio of the walked length to the best walk along the grid, per
    // thousand, of every route; unreachable pairs are checked for what they are.
    let mut ratios: Vec<i64> = Vec::new();
    for round in 0..22 {
        let (t, rock) = random_rock_map(round, &mut next);
        let (w, h) = (t.width(), t.height());
        let p = pathing_for(&t);
        for _ in 0..40 {
            let a = (next() as u32 % w, next() as u32 % h);
            let b = (next() as u32 % w, next() as u32 % h);
            if rock[(a.1 * w + a.0) as usize] || rock[(b.1 * w + b.0) as usize] {
                continue;
            }
            if a.0.abs_diff(b.0).max(a.1.abs_diff(b.1)) < 20 {
                continue;
            }
            let plan = p.plan(FOOT, mid(a.0, a.1), mid(b.0, b.1)).unwrap();
            match reference(&t, a, b) {
                Some(best) => {
                    assert!(!plan.unreachable, "{a:?} to {b:?} is reachable");
                    let walked = length(mid(a.0, a.1), &plan.waypoints);
                    ratios.push(walked * 1000 / best);
                    // Never a waypoint on rock.
                    for w in &plan.waypoints {
                        let c = cell_of(*w);
                        assert!(!rock[(c.1 * 70 + c.0) as usize], "{c:?} is rock");
                    }
                }
                None => assert!(plan.unreachable, "{a:?} to {b:?} is not reachable"),
            }
        }
    }
    ratios.sort_unstable();
    let n = ratios.len();
    assert!(n > 200, "{n} routes checked");
    // This compares an any-angle walk with the best walk along the grid, which
    // cannot cut corners: most routes are under a thousand, and the bound is no
    // statement about the quality of the search. The test below is: it compares
    // like with like.
    let (median, p95, worst) = (ratios[n / 2], ratios[n * 95 / 100], ratios[n - 1]);
    assert!(median <= 1000, "median {median}");
    assert!(p95 <= 1040, "95th percentile {p95}");
    assert!(worst <= 1150, "worst {worst}");
}

/// The quality of the search, measured like with like: the length of the route
/// a unit walks (smoothed, any angle) against the best any-angle route there is
/// by the same line-of-sight rule ([`any_angle_reference`]). Three seeds of 22
/// maps each, about 1,100 routes of at least 20 cells.
///
/// Measured, per thousand of the best: median 1006, 99th percentile 1050, worst
/// 1075; what is left over
/// is the abstract search choosing the longer of two ways round an obstacle
/// when the portals make them look alike; with fewer portals per run (two for a
/// run of 6 to 11 positions, three from 12) the worst case was 1150 in all.
#[test]
fn routes_are_within_ten_percent_of_the_best_any_angle_route() {
    let mut ratios: Vec<(i64, String)> = Vec::new();
    for seed in [0x2545_F491_4F6C_DD1D_u64, 11, 12_345] {
        let mut next = xorshift(seed);
        for round in 0..22 {
            let (t, rock) = random_rock_map(round, &mut next);
            let (w, h) = (t.width(), t.height());
            let p = pathing_for(&t);
            let g = &p.graphs[p.graph_index(FOOT).unwrap()];
            for _ in 0..40 {
                let a = (next() as u32 % w, next() as u32 % h);
                let b = (next() as u32 % w, next() as u32 % h);
                if rock[(a.1 * w + a.0) as usize] || rock[(b.1 * w + b.0) as usize] {
                    continue;
                }
                if a.0.abs_diff(b.0).max(a.1.abs_diff(b.1)) < 20 {
                    continue;
                }
                let plan = p.plan(FOOT, mid(a.0, a.1), mid(b.0, b.1)).unwrap();
                let Some(best) = any_angle_reference(&g.grid, a, b) else {
                    assert!(plan.unreachable);
                    continue;
                };
                let walked = length(mid(a.0, a.1), &plan.waypoints);
                ratios.push((
                    walked * 1000 / best,
                    format!("seed {seed}, round {round}: {a:?} to {b:?}"),
                ));
            }
        }
    }
    ratios.sort();
    let n = ratios.len();
    assert!(n > 900, "{n} routes checked");
    let (median, p99) = (ratios[n / 2].0, ratios[n * 99 / 100].0);
    let (worst, which) = ratios.last().cloned().unwrap();
    assert!(median <= 1020, "median {median}");
    assert!(p99 <= 1060, "99th percentile {p99}");
    assert!(worst <= 1100, "worst {worst} ({which})");
}

#[test]
fn a_job_finishes_the_same_whatever_the_budget() {
    let mut rows = vec![".".repeat(96); 96];
    for (y, row) in rows.iter_mut().enumerate() {
        if y % 24 != 5 {
            row.replace_range(30..32, "##");
        }
        if y % 24 != 17 {
            row.replace_range(60..62, "##");
        }
    }
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    let t = terrain(&refs);
    let p = pathing_for(&t);
    let g = &p.graphs[p.graph_index(FOOT).unwrap()];
    let run = |slice: u32| {
        let mut work = Work::new(g.sectors.count());
        let mut route = Route::IDLE;
        route.state = RouteState::Wanted;
        route.want = g.grid.index(90, 80);
        route.ticket = 1;
        let mut job = Job::new(0, 1, 0, Mode::Fresh, 0, mid(3, 3), &route);
        let mut calls = 0;
        loop {
            let mut rem = slice;
            calls += 1;
            let before = rem;
            let progress = job.run(g, &mut work, &mut route, &mut rem, 0);
            assert!(before - rem <= slice);
            if progress == Progress::Done {
                break;
            }
            assert_eq!(progress, Progress::Yield);
        }
        (route, calls)
    };
    let (whole, calls_whole) = run(u32::MAX);
    let (sliced, calls_sliced) = run(MIN_NODE_BUDGET);
    assert_eq!(calls_whole, 1);
    assert!(calls_sliced > 1, "the budget must have mattered");
    assert_eq!(whole, sliced);
    assert!(whole.ring_len() > 0 && !whole.unreachable);
}

#[test]
fn planning_twice_gives_the_same_route() {
    let t = terrain(&["..........#..........................."; 45]);
    let p = pathing_for(&t);
    let a = p.plan(FOOT, mid(1, 1), mid(37, 40));
    let b = p.plan(FOOT, mid(1, 1), mid(37, 40));
    assert_eq!(a, b);
}
