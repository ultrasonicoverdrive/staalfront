//! A sweep over the size and shape of the group of `formation_move` (WP4.6, review rounds 1
//! to 3, `docs/rules/formations.md`, "Sizes").
//!
//! `formation_move` pins ONE group (16 units in a block of 4 wide). A formation that works for
//! that block and crawls for another is a formation that works for the test, so this sweep
//! builds the same march on the same map for every size from the smallest formation (3) up to
//! `formation_max_units` (19), for six depths of the block the group starts in (2, 3, 4, 5, 6
//! and 8 columns along the way, so from a line abreast to a deep column) and for three mixes of
//! unit kinds (`mixed`: the riflemen, tanks and transports of `formation_move`; `foot`: riflemen
//! only; `wheels`: tanks and transports only). Every run is made twice, with the formation and
//! as the group order the same units make with `formation: false`, and for **every** run up to
//! the limit, every width and every mix it asserts:
//!
//! - **the 1.4 times bound** the scenario holds itself to: every unit is idle (arrived) within
//!   1.4 times the time the slowest unit of the group needs alone to walk the same way, with
//!   no formation and no other body in the way (1.4 is what `formation_move` asserts, and
//!   nothing here is looser);
//! - **not much slower than without a formation**: the formation is idle within 1.3 times the
//!   tick the same group is idle as a group order (`TO_GROUP_ORDER`). A group of fast units has
//!   a short walk, and a delay of a few hundred ticks at the passage hides inside 1.4 times a
//!   rifleman's walk but not inside a tank's: this bound catches it for every mix;
//! - **no crawl**: in no window of `WINDOW` ticks, while the rear of the group is still short
//!   of the goal area, does the unit farthest west advance by less than a quarter of what the
//!   slowest unit alone would walk in that window. The leader and the fast units may wait for
//!   it, but it must be getting on.
//!
//! The limit is the largest size at which all three hold for every width and mix:
//! `sizes_above_the_limit_for_the_record` makes the same march as a formation for 20 to 24
//! units (the limit raised for the probe) and prints it, asserting nothing, so that what the
//! limit is measured against stays visible: groups of only tanks and transports miss the 1.4
//! times bound there in 8 of 30 runs (20 units in a block 2 deep is the first), while the mixes with
//! riflemen stay clean.
//!
//! The sweep is slow (a few hundred worlds, about half a minute in release), so it is
//! `#[ignore]`d: run it with `cargo test --release -p sf_headless --test formation_sweep --
//! --ignored --nocapture`. The numbers it prints are in `docs/progress.md`. Each run is printed
//! as `c<columns>=<arrival over the walk>x/<arrival over the group order's>o/r<least share of the
//! slowest's walk the rear made in a window>/m<the same for the mean>|<the group order's
//! arrival over the walk>`, and `!` marks a run outside the bounds.

mod common;

use common::scenario_path;
use sf_headless::scenario::{Action, IdSpec, Scenario, TimedCommand};
use sf_sim::{EntityId, FormationConfig, Order};

/// A rifleman, a light tank and a light transport of `pathing_defs`.
const RIFLEMAN: u16 = 12;
const TANK: u16 = 19;
const TRANSPORT: u16 = 24;
/// The slowest of each kind, in cells a tick (cells a second over 20).
const RIFLEMAN_PACE: f64 = 0.7 / 20.0;
const TANK_PACE: f64 = 2.0 / 20.0;
const GOAL: (i32, i32) = (100, 23);
/// The bound of `formation_move`.
const BOUND: f64 = 1.4;
/// The most a formation may take, as a share of the time the same group takes as a group order
/// without one (the same march with `formation: false`).
const TO_GROUP_ORDER: f64 = 1.3;
/// The window of the no-crawl check, and the share of the slowest unit's walk the rear has
/// to make in it.
const WINDOW: u64 = 600;
const CRAWL_SHARE: f64 = 0.25;
/// Where the rear of the group is "at the goal area": from here on, the group is arriving and
/// has no further to go.
const REAR_DONE: f64 = 90.0;

/// The kinds a mix cycles through, and the pace of its slowest unit.
struct Mix {
    name: &'static str,
    kinds: &'static [u16],
    slowest: f64,
}

const MIXES: [Mix; 3] = [
    Mix {
        name: "mixed",
        kinds: &[RIFLEMAN, TANK, RIFLEMAN, TRANSPORT],
        slowest: RIFLEMAN_PACE,
    },
    Mix {
        name: "foot",
        kinds: &[RIFLEMAN],
        slowest: RIFLEMAN_PACE,
    },
    Mix {
        name: "wheels",
        kinds: &[TANK, TRANSPORT],
        slowest: TANK_PACE,
    },
];

const WIDTHS: [u32; 6] = [2, 3, 4, 5, 6, 8];
/// The shipped `formation_max_units`: the sweep covers exactly the sizes that make formations.
const CAP: u32 = 19;
/// The sizes above the limit printed for the record, as formations (the limit raised to the
/// largest of them for the probe).
const ABOVE: std::ops::RangeInclusive<u32> = 20..=24;

struct Outcome {
    /// The tick every unit was idle, if they all were by the end of the run.
    idle_at: Option<u64>,
    /// The least share of the slowest unit's walk the group made in a window.
    worst_share: f64,
    /// The same for the rear of the group (the unit farthest west): a crawl is the slowest
    /// unit making no headway, however the leader and the fast ones wait for it.
    worst_rear: f64,
    /// Formation counters at the end.
    breaks: u64,
    expelled: u64,
}

fn build(n: u32, cols: u32, mix: &Mix, ticks: u64, formation: bool, cap: u32) -> Scenario {
    let mut s = Scenario::load(&scenario_path("formation_move.ron")).unwrap();
    s.formation = Some(formation);
    s.formation_max_units = Some(cap as u16);
    s.ticks = ticks;
    s.commands.clear();
    let rows = n.div_ceil(cols);
    let y0 = 23 - rows / 2;
    for i in 0..n {
        let (c, r) = (i % cols, i / cols);
        s.commands.push(TimedCommand {
            tick: 0,
            player: 0,
            command: Action::SpawnUnit {
                kind: mix.kinds[i as usize % mix.kinds.len()],
                owner: 0,
                cell: (6 + c, y0 + r),
            },
        });
    }
    s.commands.push(TimedCommand {
        tick: 1,
        player: 0,
        command: Action::OrderMove {
            ids: vec![IdSpec::Range(0, n)],
            target: GOAL,
            queued: false,
        },
    });
    s
}

/// The time the slowest unit needs alone to walk from the middle of the block to the goal.
fn walk_ticks(cols: u32, mix: &Mix) -> f64 {
    let start = 6.0 + (f64::from(cols) - 1.0) / 2.0;
    (f64::from(GOAL.0) - start) / mix.slowest
}

fn run(n: u32, cols: u32, mix: &Mix, ticks: u64, formation: bool, cap: u32) -> Outcome {
    let s = build(n, cols, mix, ticks, formation, cap);
    let mut w = s.build_world().unwrap();
    let ids: Vec<EntityId> = (0..n).map(|u| EntityId::new(u, 0)).collect();
    let mean = |w: &sf_sim::World| {
        ids.iter()
            .map(|&id| w.get(id).unwrap().pos.x.raw() as f64 / 65536.0)
            .sum::<f64>()
            / f64::from(n)
    };
    let rear = |w: &sf_sim::World| {
        ids.iter()
            .map(|&id| w.get(id).unwrap().pos.x.raw() as f64 / 65536.0)
            .fold(f64::MAX, f64::min)
    };
    let mut idle_at = None;
    let mut worst_share = f64::MAX;
    let mut worst_rear = f64::MAX;
    let mut window_start = (0u64, 0.0, 0.0);
    for t in 0..ticks {
        w.step();
        if t == 0 {
            // The units are placed on this tick.
            window_start = (1, mean(&w), rear(&w));
        }
        if t > 3 && ids.iter().all(|&id| w.order_of(id) == Some(Order::Idle)) {
            idle_at = Some(t);
            break;
        }
        if t + 1 - window_start.0 == WINDOW {
            let (now, edge) = (mean(&w), rear(&w));
            if edge < REAR_DONE {
                let walk = mix.slowest * WINDOW as f64;
                worst_share = worst_share.min((now - window_start.1) / walk);
                worst_rear = worst_rear.min((edge - window_start.2) / walk);
            }
            window_start = (t + 1, now, edge);
        }
    }
    let stats = w.formation_stats();
    Outcome {
        idle_at,
        worst_share,
        worst_rear,
        breaks: stats.breaks,
        expelled: stats.expelled,
    }
}

/// The worst of each ratio over the runs of one mix: arrival over the walk, with and without a
/// formation, the formation's time over the group order's, and the least share of the walk the
/// rear made in a window.
#[derive(Default)]
struct Worst {
    walk: (f64, f64),
    off_walk: (f64, f64),
    to_off: (f64, f64),
    rear: f64,
}

fn widen(r: &mut (f64, f64), x: f64) {
    if r.0 == 0.0 && r.1 == 0.0 {
        *r = (x, x);
    } else {
        *r = (r.0.min(x), r.1.max(x));
    }
}

/// Every size, width and mix; returns the lines that failed.
fn sweep(sizes: &[u32], cap: u32, assert_it: bool) -> Vec<String> {
    let mut failed = Vec::new();
    for mix in &MIXES {
        println!("mix {}", mix.name);
        let mut worst = Worst {
            rear: f64::MAX,
            ..Worst::default()
        };
        for &n in sizes {
            let mut line = format!("  n={n:2}:");
            for &cols in WIDTHS.iter().filter(|&&c| c <= n) {
                let walk = walk_ticks(cols, mix);
                let bound = walk * BOUND;
                // Run on to twice the bound so that a slow run shows its time.
                let limit = (bound * 2.0) as u64;
                let o = run(n, cols, mix, limit, true, cap);
                let off = run(n, cols, mix, limit, false, cap);
                let fmt = |x: f64| {
                    if x == f64::MAX {
                        "-".to_string()
                    } else {
                        format!("{x:.2}")
                    }
                };
                let ratio = o.idle_at.map(|t| t as f64 / walk);
                let off_ratio = off.idle_at.map(|t| t as f64 / walk);
                let to_off = o.idle_at.zip(off.idle_at).map(|(a, b)| a as f64 / b as f64);
                let show = |r: Option<f64>| r.map_or("never".to_string(), |r| format!("{r:.2}"));
                line += &format!(
                    " c{cols}={}x/{}o/r{}/m{}|{}",
                    show(ratio),
                    show(to_off),
                    fmt(o.worst_rear),
                    fmt(o.worst_share),
                    show(off_ratio)
                );
                if o.breaks > 0 || o.expelled > 0 {
                    line += &format!("(b{} e{})", o.breaks, o.expelled);
                }
                ratio.inspect(|&r| widen(&mut worst.walk, r));
                off_ratio.inspect(|&r| widen(&mut worst.off_walk, r));
                to_off.inspect(|&r| widen(&mut worst.to_off, r));
                worst.rear = worst.rear.min(o.worst_rear);
                let late = ratio.is_none_or(|r| r > BOUND);
                let slower = to_off.is_none_or(|r| r > TO_GROUP_ORDER);
                let crawl = o.worst_rear < CRAWL_SHARE;
                if late || slower || crawl {
                    failed.push(format!(
                        "{} n={n} cols={cols}: arrived at {:?} (bound {bound:.0}; without a \
                         formation {:?}, bound {TO_GROUP_ORDER} times that), worst share of the \
                         walk of the rear {} (need {CRAWL_SHARE})",
                        mix.name,
                        o.idle_at,
                        off.idle_at,
                        fmt(o.worst_rear)
                    ));
                    line += "!";
                }
            }
            println!("{line}");
        }
        println!(
            "  {}: arrival {:.2} to {:.2} times the walk (without a formation {:.2} to {:.2}); \
             {:.2} to {:.2} times the group order; the rear made at least {:.2} of the walk in a window",
            mix.name,
            worst.walk.0,
            worst.walk.1,
            worst.off_walk.0,
            worst.off_walk.1,
            worst.to_off.0,
            worst.to_off.1,
            worst.rear
        );
    }
    if assert_it {
        assert!(
            failed.is_empty(),
            "{} runs outside the bounds:\n{}",
            failed.len(),
            failed.join("\n")
        );
    }
    failed
}

#[test]
#[ignore = "slow: about three hundred worlds; run with --ignored in release"]
fn formations_up_to_the_limit_keep_every_bound_for_every_width_and_mix() {
    assert_eq!(
        u32::from(FormationConfig::DEFAULT.max_units),
        CAP,
        "the sweep must cover exactly the shipped limit"
    );
    let sizes: Vec<u32> = (3..=CAP).collect();
    sweep(&sizes, CAP, true);
}

#[test]
#[ignore = "slow: prints the sizes above the limit as formations, asserts nothing"]
fn sizes_above_the_limit_for_the_record() {
    // Above the limit a group is not a formation (it moves as a group order). This makes the
    // same march as a formation, with the limit raised for the probe, and prints it next to the
    // group order, so that the reason for the limit stays visible. It asserts nothing.
    let sizes: Vec<u32> = ABOVE.collect();
    let failed = sweep(&sizes, *ABOVE.end(), false);
    println!(
        "{} of the runs above the limit are outside the bounds",
        failed.len()
    );
}
