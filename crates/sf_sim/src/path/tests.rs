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

#[test]
fn routes_stay_close_to_the_best_on_random_maps() {
    let mut s: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    // The ratio of the walked length to the best walk along the grid, per
    // thousand, of every route; unreachable pairs are checked for what they are.
    let mut ratios: Vec<i64> = Vec::new();
    for round in 0..22 {
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
        let t = terrain(&refs);
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
    // A route that cuts corners is shorter than the best walk along the grid,
    // so most are under a thousand. The few that are not are the ones where the
    // abstract graph chose the wrong one of two ways around an obstacle, by a
    // few cells. Measured on 1 200 routes the worst was 1103; the bound has
    // room above that.
    let (median, p95, worst) = (ratios[n / 2], ratios[n * 95 / 100], ratios[n - 1]);
    assert!(median <= 1000, "median {median}");
    assert!(p95 <= 1040, "95th percentile {p95}");
    assert!(worst <= 1150, "worst {worst}");
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
