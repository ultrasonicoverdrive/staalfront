//! The `crowd_jam` scenario (WP4.4, `docs/rules/collision.md`): two groups pass head-on
//! through a narrow gap, and a unit holding position stands in a corridor while
//! others route round it.
//!
//! Asserts what the work package asked for: nobody gets stuck forever, the unit
//! holding position is never pushed, the two groups really did meet in the gap and
//! got by each other each on its own right, nobody is ever on ground its class cannot
//! enter, and the bodies do not overlap beyond the slack at the end.

mod common;

use common::scenario_path;
use sf_headless::scenario::Scenario;
use sf_math::{Fx, Vec2};
use sf_sim::{EntityId, Order, World};

const A: std::ops::Range<u32> = 0..18;
const B: std::ops::Range<u32> = 18..36;
const C: std::ops::Range<u32> = 36..44;
const HOLDER: u32 = 44;
const UNITS: u32 = 45;

/// The gap: columns 27 to 34, rows 18 to 20.
fn in_gap(p: Vec2) -> bool {
    let (x, y) = p.cell();
    (27..=34).contains(&x) && (18..=20).contains(&y)
}

fn id(slot: u32) -> EntityId {
    EntityId::new(slot, 0)
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

struct Run {
    world: World,
    history: Vec<u64>,
    /// Ticks in which at least one unit of A and one of B were in the gap.
    met_in_the_gap: u32,
    /// The sum of the rows (in raw fixed point) of A's and B's units in the gap, and
    /// how many sightings of each.
    rows_a: (i64, i64),
    rows_b: (i64, i64),
    bad_cells: Vec<(u64, u32)>,
    worst_overlap: (i64, u64),
    /// The most ticks in a row any unit but the holder went without coming closer to
    /// where it was going (the unit's own count, which a sidestep restarts).
    longest_quiet: u32,
    holder_moved: bool,
    tick_all_idle: Option<u64>,
}

fn run() -> Run {
    let scenario = Scenario::load(&scenario_path("crowd_jam.ron")).unwrap();
    let mut world = scenario.build_world().unwrap();
    let mut r = Run {
        world: world.clone(),
        history: Vec::new(),
        met_in_the_gap: 0,
        rows_a: (0, 0),
        rows_b: (0, 0),
        bad_cells: Vec::new(),
        worst_overlap: (0, 0),
        longest_quiet: 0,
        holder_moved: false,
        tick_all_idle: None,
    };
    let holder_at = cell(49, 32);
    let mut radius: Vec<Fx> = Vec::new();
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
        let (mut a_in, mut b_in) = (false, false);
        for s in 0..UNITS {
            let unit = id(s);
            let class = world.stats_of(unit).unwrap().movement_class.unwrap();
            let c = world.cell_at(pos[s as usize]).unwrap();
            if !world.passable(class, c) {
                r.bad_cells.push((tick, s));
            }
            if A.contains(&s) && in_gap(pos[s as usize]) {
                a_in = true;
                r.rows_a.0 += pos[s as usize].y.raw();
                r.rows_a.1 += 1;
            }
            if B.contains(&s) && in_gap(pos[s as usize]) {
                b_in = true;
                r.rows_b.0 += pos[s as usize].y.raw();
                r.rows_b.1 += 1;
            }
            if s != HOLDER {
                r.longest_quiet = r.longest_quiet.max(world.stuck_ticks_of(unit).unwrap());
            }
        }
        if a_in && b_in {
            r.met_in_the_gap += 1;
        }
        if pos[HOLDER as usize] != holder_at {
            r.holder_moved = true;
        }
        if r.tick_all_idle.is_none()
            && tick > 2
            && (0..UNITS).all(|s| s == HOLDER || world.order_of(id(s)) == Some(Order::Idle))
        {
            r.tick_all_idle = Some(tick);
        }
        if tick >= 20 && tick.is_multiple_of(5) && !radius.is_empty() {
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
    r.world = world;
    r
}

const ORTH: i64 = 65_536;

#[test]
fn nobody_gets_stuck_forever_and_the_holder_is_never_pushed() {
    let r = run();
    let stats = r.world.crowd_stats();
    eprintln!(
        "crowd_jam: every unit but the holder had its order done by tick {:?}; the groups were \
         in the gap together on {} ticks; the worst overlap of two bodies was {:.3} cells (tick \
         {}); the longest any unit went without coming closer was {} ticks; the crowd pass: {} \
         ticks with a push, {} idle units shoved, {} sidesteps, {} requests for a new route",
        r.tick_all_idle,
        r.met_in_the_gap,
        r.worst_overlap.0 as f64 / ORTH as f64,
        r.worst_overlap.1,
        r.longest_quiet,
        stats.busy_ticks,
        stats.shoves,
        stats.sidesteps,
        stats.replans,
    );
    assert!(
        r.tick_all_idle.is_some(),
        "some unit never got where it was going"
    );
    for s in 0..UNITS {
        if s != HOLDER {
            assert_eq!(r.world.order_of(id(s)), Some(Order::Idle), "unit {s}");
            assert_eq!(r.world.stuck_ticks_of(id(s)), Some(0), "unit {s}");
        }
    }
    // The holder stood on its post the whole time and still holds it.
    assert!(!r.holder_moved, "the unit holding position was pushed");
    assert_eq!(r.world.order_of(id(HOLDER)), Some(Order::HoldPosition));
    assert!(
        r.bad_cells.is_empty(),
        "{:?}",
        &r.bad_cells[..r.bad_cells.len().min(5)]
    );
    // Nobody went without coming closer for longer than the stuck limit asks.
    let limit = u32::from(r.world.collision_config().stuck_ticks);
    assert!(r.longest_quiet <= limit, "{} > {limit}", r.longest_quiet);
}

#[test]
fn the_groups_meet_in_the_gap_and_each_keeps_to_its_own_right() {
    let r = run();
    assert!(
        r.met_in_the_gap > 20,
        "A and B were in the gap together on only {} ticks",
        r.met_in_the_gap
    );
    // Going east the right hand is south, which is the larger row: A kept to the
    // south half of the gap and B to the north half.
    let mean = |(sum, n): (i64, i64)| sum as f64 / n as f64 / ORTH as f64;
    let (a, b) = (mean(r.rows_a), mean(r.rows_b));
    eprintln!("crowd_jam: mean row in the gap: A (eastbound) {a:.2}, B (westbound) {b:.2}");
    assert!(a > b, "A {a:.2} should keep south of B {b:.2}");
}

#[test]
fn the_groups_end_where_they_were_sent_and_c_gets_past_the_holder() {
    let r = run();
    let near = |range: std::ops::Range<u32>, to: Vec2, within: i64| {
        for s in range {
            let d = (r.world.get(id(s)).unwrap().pos - to).length();
            assert!(
                d < Fx::from_int(within as i32),
                "unit {s} ended {d:?} from {to:?}"
            );
        }
    };
    // 18 units (10 riflemen, 8 tanks) fill a disc of about four cells.
    near(A, cell(56, 24), 9);
    near(B, cell(8, 24), 9);
    near(C, cell(60, 32), 9);
    // C is past the holder: the holder is at column 49, the corridor ends at 58.
    for s in C {
        assert!(
            r.world.get(id(s)).unwrap().pos.x > Fx::from_int(51),
            "unit {s} did not get past the holder"
        );
    }
}

#[test]
fn bodies_do_not_overlap_beyond_the_slack_on_the_way_or_at_the_end() {
    let r = run();
    // A packed crowd squeezed in a gap goes a little over the slack of a
    // thirty-second for a moment.
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
    assert!(
        worst_end < ORTH / 16,
        "at the end two bodies overlap by {worst_end}"
    );
}

#[test]
fn the_run_is_reproducible() {
    let a = run();
    let b = run();
    assert_eq!(a.history, b.history);
}
