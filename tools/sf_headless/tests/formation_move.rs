//! The `formation_move` scenario (WP4.6, `docs/rules/formations.md`): a mixed group of 16
//! marches across open ground, through a narrow passage and on to a goal behind it, as one
//! formation.
//!
//! Asserts what the work package asked for: the shape is held in the open field (every
//! member within the re-form distance of its slot for nearly all the ticks the formation is
//! on open ground), the pace is the slowest member's (the group takes as long as the riflemen
//! alone would, and is never strung out along its heading by more than its own width), the
//! formation breaks up at the passage and forms up again behind it, everybody arrives and
//! none is stuck, nobody leaves the formation on the way, and the bodies do not overlap beyond
//! the bounds of phase 4.

mod common;

use common::scenario_path;
use sf_headless::scenario::Scenario;
use sf_math::{Fx, Vec2};
use sf_sim::{EntityId, MemberMode, Order, World};

const UNITS: u32 = 16;
const GOAL: (i32, i32) = (100, 23);
/// Columns of the wall: the passage is in rows 22 to 24 and eight cells long.
const WALL: std::ops::RangeInclusive<i32> = 40..=47;

/// How far from its slot a member may be and still count as in the shape: the re-form distance
/// of the shipped settings (30 tenths of a cell).
const NEAR: f64 = 3.0;
/// The share of the ticks on open ground that every member has to be near its slot.
const IN_SHAPE: f64 = 0.95;
/// The slowest member of the group, a rifleman, in cells a tick.
const SLOWEST: f64 = 0.7 / 20.0;

fn id(slot: u32) -> EntityId {
    EntityId::new(slot, 0)
}

fn cells(f: Fx) -> f64 {
    f.raw() as f64 / 65536.0
}

fn cell(x: i32, y: i32) -> Vec2 {
    Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF)
}

/// The least distance the bodies of two units keep (`docs/rules/collision.md`).
fn keep(ra: Fx, rb: Fx) -> Fx {
    let small = |r: Fx| r <= Fx::from_ratio(3, 8);
    if small(ra) && small(rb) {
        (ra + rb).mul_int(3).div_int(4)
    } else {
        ra + rb
    }
}

const ORTH: i64 = 65_536;

/// What a run saw.
struct Run {
    world: World,
    history: Vec<u64>,
    /// Ticks on open ground (the formation exists, is not loose, the leader is in a field
    /// and not at the end of the march) and of them the ticks every member was near its slot.
    open_ticks: u32,
    in_shape_ticks: u32,
    /// The farthest any member was from its slot on those ticks.
    worst_deviation: f64,
    /// The widest the group was along its heading, on those ticks.
    widest_spread: f64,
    /// The widest the formation's own lattice was, from the leader.
    extent: f64,
    /// The leader's position on the first and last tick of the stretch of open ground in the
    /// west field that was measured, and the ticks between them.
    west_stretch: Option<(f64, f64, u32)>,
    /// Ticks the formation was loose, and members in `Flow` at once (most).
    loose_ticks: u32,
    most_flowing: usize,
    /// The first tick, after the passage, on which the whole group was in shape again.
    formed_again: Option<u64>,
    best_after: f64,
    /// The tick the formation was let go.
    released: Option<u64>,
    bad_cells: Vec<(u64, u32)>,
    worst_overlap: (i64, u64),
    all_idle: Option<u64>,
}

fn run() -> Run {
    let scenario = Scenario::load(&scenario_path("formation_move.ron")).unwrap();
    let mut world = scenario.build_world().unwrap();
    let mut r = Run {
        world: world.clone(),
        history: Vec::new(),
        open_ticks: 0,
        in_shape_ticks: 0,
        worst_deviation: 0.0,
        widest_spread: 0.0,
        extent: 0.0,
        west_stretch: None,
        loose_ticks: 0,
        most_flowing: 0,
        formed_again: None,
        best_after: f64::MAX,
        released: None,
        bad_cells: Vec::new(),
        worst_overlap: (0, 0),
        all_idle: None,
    };
    let mut radius: Vec<Fx> = Vec::new();
    let mut west: Option<(f64, u32, f64, u32)> = None;
    for _ in 0..scenario.ticks {
        let report = world.step();
        r.history.push(report.checksum);
        let tick = report.tick.0;
        if radius.is_empty() && world.len() == UNITS as usize {
            radius = (0..UNITS)
                .map(|s| world.collision_radius_of(id(s)).unwrap())
                .collect();
        }
        let pos: Vec<Vec2> = (0..UNITS).map(|s| world.get(id(s)).unwrap().pos).collect();
        for s in 0..UNITS {
            let class = world.stats_of(id(s)).unwrap().movement_class.unwrap();
            if !world.passable(class, world.cell_at(pos[s as usize]).unwrap()) {
                r.bad_cells.push((tick, s));
            }
        }
        let views: Vec<_> = (0..UNITS).map(|s| world.formation_of(id(s))).collect();
        if r.released.is_none() && tick > 2 && views.iter().all(Option::is_none) {
            r.released = Some(tick);
        }
        if let Some(lead) = views.iter().flatten().find(|v| v.leader) {
            let leader = lead.leader_id.index() as usize;
            let lead_x = cells(pos[leader].x);
            r.extent = r.extent.max(cells(lead.extent));
            let before_wall = lead_x < f64::from(*WALL.start()) - 8.0;
            let after_wall = lead_x > f64::from(*WALL.end()) + 8.0;
            if lead.loose {
                r.loose_ticks += 1;
            }
            let flowing = views
                .iter()
                .flatten()
                .filter(|v| v.mode == MemberMode::Flow)
                .count();
            r.most_flowing = r.most_flowing.max(flowing);
            // Where the heading points: the group's spread along it.
            let h = lead.heading.to_vec2();
            let along: Vec<f64> = (0..UNITS as usize)
                .filter(|&s| views[s].is_some())
                .map(|s| cells((pos[s] - pos[leader]).dot(h)))
                .collect();
            let spread = along.iter().cloned().fold(f64::MIN, f64::max)
                - along.iter().cloned().fold(f64::MAX, f64::min);
            let in_shape = (0..UNITS as usize)
                .filter(|&s| views[s].is_some())
                .all(|s| cells((pos[s] - views[s].unwrap().slot).length()) <= NEAR);
            let deviation = (0..UNITS as usize)
                .filter_map(|s| views[s].map(|v| cells((pos[s] - v.slot).length())))
                .fold(0.0, f64::max);
            if !lead.loose && (before_wall || after_wall) && tick > 60 {
                r.open_ticks += 1;
                r.in_shape_ticks += u32::from(in_shape);
                r.worst_deviation = r.worst_deviation.max(deviation);
                r.widest_spread = r.widest_spread.max(spread);
                if before_wall && lead_x > 14.0 {
                    let w = west.get_or_insert((lead_x, tick as u32, lead_x, tick as u32));
                    w.2 = lead_x;
                    w.3 = tick as u32;
                }
            }
            if lead_x > f64::from(*WALL.end()) + 6.0 && !lead.loose {
                r.best_after = r.best_after.min(deviation);
            }
            if r.formed_again.is_none()
                && lead_x > f64::from(*WALL.end()) + 6.0
                && !lead.loose
                && lead.members == UNITS as usize
                // Formed up: every member walks straight to its slot (none follows the order's
                // way, none stands ahead of it) and is within a cell of it.
                && (0..UNITS as usize).all(|s| {
                    views[s].is_some_and(|v| {
                        (v.mode == MemberMode::Direct || v.leader)
                            && cells((pos[s] - v.slot).length()) < 1.0
                    })
                })
            {
                r.formed_again = Some(tick);
            }
        }
        if r.all_idle.is_none()
            && tick > 2
            && (0..UNITS).all(|s| world.order_of(id(s)) == Some(Order::Idle))
        {
            r.all_idle = Some(tick);
        }
        if tick >= 2 && !radius.is_empty() {
            for a in 0..UNITS as usize {
                for b in a + 1..UNITS as usize {
                    let overlap = (keep(radius[a], radius[b]) - pos[a].distance(pos[b])).raw();
                    if overlap > r.worst_overlap.0 {
                        r.worst_overlap = (overlap, tick);
                    }
                }
            }
        }
    }
    r.west_stretch = west.map(|(x0, t0, x1, t1)| (x0, x1, t1 - t0));
    r.world = world;
    r
}

fn shared() -> &'static Run {
    use std::sync::OnceLock;
    static RUN: OnceLock<Run> = OnceLock::new();
    RUN.get_or_init(run)
}

#[test]
fn the_shape_is_held_in_the_open_field() {
    let r = shared();
    eprintln!(
        "formation_move: {} ticks on open ground, {} of them with every member within {NEAR} cells \
         of its slot ({:.1} %); the worst deviation {:.2} cells; the lattice reaches {:.2} cells from the leader",
        r.open_ticks,
        r.in_shape_ticks,
        100.0 * f64::from(r.in_shape_ticks) / f64::from(r.open_ticks.max(1)),
        r.worst_deviation,
        r.extent
    );
    assert!(r.open_ticks > 1200, "{} ticks on open ground", r.open_ticks);
    assert!(
        f64::from(r.in_shape_ticks) >= IN_SHAPE * f64::from(r.open_ticks),
        "{} of {} ticks in shape",
        r.in_shape_ticks,
        r.open_ticks
    );
    // No member ever strayed far: a lattice of this group is 4.5 cells wide at most, and two
    // of them is the most the shape lets any member be off.
    assert!(r.worst_deviation <= 2.0 * r.extent, "{}", r.worst_deviation);
}

#[test]
fn the_slowest_sets_the_pace_and_the_group_is_not_strung_out() {
    let r = shared();
    let (x0, x1, ticks) = r
        .west_stretch
        .expect("a stretch of the west field was measured");
    let speed = (x1 - x0) / f64::from(ticks);
    eprintln!(
        "formation_move: the leader went {:.1} cells in {ticks} ticks in the west field: {:.4} cells \
         a tick, the slowest member's top speed is {SLOWEST:.4}; the group was at most {:.1} cells \
         long along its heading",
        x1 - x0,
        speed,
        r.widest_spread
    );
    // At the pace of the rifleman (a tenth is kept back so that it can keep up), never faster.
    assert!(speed <= SLOWEST * 1.001, "{speed}");
    assert!(speed >= SLOWEST * 0.85, "{speed}");
    // Strung out: the tanks and trucks could be 50 cells ahead of the riflemen by now.
    assert!(
        r.widest_spread <= 2.0 * r.extent + 3.0,
        "{}",
        r.widest_spread
    );
}

#[test]
fn the_formation_breaks_up_at_the_passage_and_forms_up_again_behind_it() {
    let r = shared();
    eprintln!(
        "formation_move: loose on {} ticks, at most {} members following the order's own way at \
         once, whole again on tick {:?}, let go on tick {:?}; {:?}",
        r.loose_ticks,
        r.most_flowing,
        (r.formed_again, r.best_after),
        r.released,
        r.world.formation_stats()
    );
    assert!(
        r.loose_ticks > 20,
        "the formation never let go at the passage"
    );
    assert!(r.most_flowing >= 3, "{}", r.most_flowing);
    let again = r
        .formed_again
        .expect("the formation formed up behind the passage");
    let released = r.released.expect("the formation was let go");
    assert!(again < released);
    // It re-formed on open ground, a good way before the goal.
    let stats = r.world.formation_stats();
    assert_eq!(stats.formed, 1);
    assert_eq!(stats.released, 1);
    assert_eq!(stats.expelled, 0, "nobody left the formation on the way");
    assert_eq!(stats.dissolved, 0);
}

#[test]
fn everybody_arrives_and_nobody_is_stuck() {
    let r = shared();
    let idle = r.all_idle.expect("every order was done");
    eprintln!("formation_move: everybody had arrived by tick {idle}");
    for s in 0..UNITS {
        let u = id(s);
        assert_eq!(r.world.order_of(u), Some(Order::Idle), "unit {s}");
        assert_eq!(r.world.stuck_ticks_of(u), Some(0), "unit {s}");
        assert_eq!(r.world.replan_requested(u), Some(false), "unit {s}");
        let d = (r.world.get(u).unwrap().pos - cell(GOAL.0, GOAL.1)).length();
        assert!(d < Fx::from_int(6), "unit {s} ended {d:?} from the goal");
    }
    assert!(
        r.bad_cells.is_empty(),
        "{:?}",
        &r.bad_cells[..r.bad_cells.len().min(5)]
    );
    // The march took as long as the riflemen need, a tenth more for the part kept back and
    // the waits at the passage, not twice as long.
    let walk = f64::from(GOAL.0 - 8) / SLOWEST;
    assert!(
        idle as f64 >= walk,
        "arrived at tick {idle}, the riflemen alone need {walk}"
    );
    assert!(
        (idle as f64) < walk * 1.4,
        "arrived at tick {idle}, the riflemen alone need {walk}"
    );
}

#[test]
fn bodies_do_not_overlap_beyond_the_slack_on_the_way_or_at_the_end() {
    let r = shared();
    assert!(
        r.worst_overlap.0 < ORTH / 4,
        "worst overlap {} at tick {}",
        r.worst_overlap.0,
        r.worst_overlap.1
    );
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
    eprintln!(
        "formation_move: the worst overlap during the run is {:.3} cells (tick {}), at the end {:.3}",
        r.worst_overlap.0 as f64 / ORTH as f64,
        r.worst_overlap.1,
        worst_end as f64 / ORTH as f64
    );
    assert!(
        worst_end < ORTH / 16,
        "at the end two bodies overlap by {worst_end}"
    );
}

#[test]
fn the_run_is_reproducible() {
    let a = shared();
    let b = run();
    assert_eq!(a.history, b.history);
}
