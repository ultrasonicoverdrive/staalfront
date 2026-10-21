//! Formations in a real base (WP4.6b): the sandbox of `sf_game` on the map `assets/maps/formation`,
//! the twelve mobile Meridian units box-selected and given ONE `Move`.
//!
//! WP4.6 proved formations on a field with one passage and no structures. In the viewer the
//! same order looked broken: the structures of the bases made the formation "loose" (a narrow
//! place), and a loose formation used to mean "everybody at their own speed", so the tanks
//! raced ahead of the riflemen and no shape was visible. The world here is built the way the
//! viewer builds it (`playfield::make_world`, the unit table, `playfield::with_movement`,
//! `sandbox::schedule`, the viewer's world speed), and the order is the one a right-click makes
//! (`commands::move_command` over the mobile units of the selection), so a regression of the
//! base start shows here and not first on somebody's screen.
//!
//! Asserted for four goals (south, south-east, north-east and far east of the bases):
//!
//! * one formation forms, of the eleven ground units; the amphibious transport is a family of
//!   one and forms none, and does not take the shared fields of the others away;
//! * the **pace**: the leader never steps faster than the slowest member, loose or not, and in
//!   the stretches where the formation is loose the members that keep up with their slots do
//!   not run at the speeds they have of their own (that was the bug);
//! * the **shape**: how many members are near their slots. The honest bound is a low one:
//!   the structures of the sandbox keep the formation loose for 40 to 70 % of the march
//!   (they stay "narrow places", `docs/rules/formations.md`), two heavy tracked units are
//!   stuck among them for a long time with or without a formation (the paths are made for
//!   bodies of one cell), and a formation waits for them. What is asserted is what holds;
//!   the numbers of every run are printed (`--nocapture`);
//! * everybody arrives, nobody is stuck, bodies overlap within the bounds of phase 4, and the
//!   run is reproducible.
//!
//! `FORMATION_BASE_TRACE=<goal index>` prints a line per 100 ticks of that goal.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use sf_data::map::Map;
use sf_data::units::UnitDefs;
use sf_game::commands::move_command;
use sf_game::{playfield, sandbox};
use sf_math::{Fx, Vec2};
use sf_sim::{EntityId, MemberMode, Order, PlayerId, UnitClass, World, WorldConfig};

/// The viewer's world speed (`runner::DEMO_SPEED`); data-driven units move at their own speed,
/// but the world is built with it, so it is here too.
const DEMO_SPEED: Fx = Fx::from_ratio(3, 2);

/// The goals (cells): south of the bases, south-east, north-east and far east.
const GOALS: [(i32, i32); 4] = [(56, 44), (90, 44), (90, 10), (100, 40)];

/// Ticks a run may take: the riflemen walk 0.035 cells a tick, the longest goal is ~70 cells.
const MAX_TICKS: u64 = 5000;
/// Ticks measured after the last unit has stopped (the end overlap is read then).
const SETTLE_TICKS: u64 = 100;

/// How far from its slot a member may be and still count as in the shape: the re-form distance
/// of the shipped settings.
const NEAR: f64 = 3.0;
/// The window, in ticks, over which a member's speed is measured (one second).
const WINDOW: usize = 20;
/// The part of the slowest member's speed a member may use over a window and still keep the pace.
const PACE_BOUND: f64 = 1.2;
/// How far behind its slot (cells, along the heading) a member is catching up, not keeping up:
/// it goes at its own speed on purpose, so it is not measured.
const CATCHING_UP: f64 = 1.0;

const ORTH: i64 = 65_536;

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn cells(f: Fx) -> f64 {
    f.raw() as f64 / ORTH as f64
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

/// The sandbox of the viewer on the formation map, one tick in (the units stand), and the ids
/// of the mobile Meridian units, as a box-select would take them.
fn base_start() -> (World, Vec<EntityId>) {
    let assets = assets();
    let map = Map::load(&assets.join("maps/formation")).expect("the formation map loads");
    let size = map.size_in_cells();
    let config = WorldConfig {
        speed: DEMO_SPEED,
        ..WorldConfig::new(1).with_map_size(size.0, size.1)
    };
    let defs = UnitDefs::load(&assets.join("config"), &assets).expect("unit definitions load");
    let world = playfield::make_world(config, &map).expect("the map makes a world");
    let world = world
        .with_units(sf_bridge::unit_table(&defs).expect("the unit table"))
        .expect("units before the start");
    let mut world = playfield::with_movement(world, &defs);
    let plan = sandbox::schedule(&mut world, &defs);
    assert_eq!(plan.skipped, 0, "the whole sandbox stands on this map");
    world.step();
    let ids: Vec<EntityId> = world
        .snapshot()
        .iter()
        .filter(|v| v.owner == PlayerId(sandbox::MERIDIAN))
        .filter(|v| {
            world
                .stats_of(v.id)
                .is_some_and(|s| s.class() == UnitClass::Mobile)
        })
        .map(|v| v.id)
        .collect();
    (world, ids)
}

/// What a run saw.
#[derive(Debug, Default)]
struct Report {
    goal: (i32, i32),
    units: usize,
    /// Formations made, and the members of the first.
    formed: u64,
    members: usize,
    /// Units that follow a shared flowfield one tick after the order: the members of the
    /// formation, and the units outside it.
    fields_of_members: usize,
    fields_of_others: usize,
    /// The checksum of the world at the end.
    checksum: u64,
    /// Ticks a formation of the group existed, and of them loose.
    with_formation: u64,
    loose: u64,
    /// Ticks every member was near its slot; the same on the ticks that were not loose.
    in_shape: u64,
    open: u64,
    open_in_shape: u64,
    /// Member-ticks near their slots, of all member-ticks.
    member_near: u64,
    member_ticks: u64,
    /// The farthest any member was from its slot, on ticks the formation existed.
    worst_deviation: f64,
    /// The fastest the leader went over a window of twenty ticks that it led all through, and
    /// the speed of the slowest member, in cells a tick.
    leader_top: f64,
    slowest: f64,
    /// Windows of twenty ticks in which a member kept up with its slot, and of them the ones in
    /// which it went above `PACE_BOUND` times the slowest's speed; the same for the windows in
    /// which the formation was loose all through.
    windows: u64,
    fast_windows: u64,
    loose_windows: u64,
    loose_fast: u64,
    /// The tick the last unit had stopped (`None` if not by `MAX_TICKS`).
    all_idle: Option<u64>,
    stuck: Vec<usize>,
    far_from_goal: f64,
    worst_overlap: (i64, u64),
    end_overlap: i64,
    expelled: u64,
}

impl Report {
    fn share(part: u64, whole: u64) -> f64 {
        part as f64 / whole.max(1) as f64
    }
    fn loose_share(&self) -> f64 {
        Self::share(self.loose, self.with_formation)
    }
    fn shape_share(&self) -> f64 {
        Self::share(self.in_shape, self.with_formation)
    }
    fn open_shape_share(&self) -> f64 {
        Self::share(self.open_in_shape, self.open)
    }
    fn member_share(&self) -> f64 {
        Self::share(self.member_near, self.member_ticks)
    }
    fn fast_share(&self) -> f64 {
        Self::share(self.fast_windows, self.windows)
    }
    fn loose_fast_share(&self) -> f64 {
        Self::share(self.loose_fast, self.loose_windows)
    }
    fn print(&self) {
        eprintln!(
            "formation_base goal {:?}: {} units, {} formed, {} members; formation on {} ticks, \
             loose {:.0} %; all members within {NEAR} cells of their slots {:.0} % of the ticks \
             ({:.0} % of the {} not loose), members near their slots {:.0} %, worst {:.1} cells \
             off; leader top {:.4} cells a tick (slowest {:.4}); members above {PACE_BOUND}x the \
             slowest over {WINDOW} ticks in {:.0} % of the windows ({:.0} % of the {} loose ones); \
             idle by {:?}, {} stuck, farthest {:.1} from the goal; overlap {:.3} at tick {}, end \
             {:.3}; expelled {}",
            self.goal,
            self.units,
            self.formed,
            self.members,
            self.with_formation,
            100.0 * self.loose_share(),
            100.0 * self.shape_share(),
            100.0 * self.open_shape_share(),
            self.open,
            100.0 * self.member_share(),
            self.worst_deviation,
            self.leader_top,
            self.slowest,
            100.0 * self.fast_share(),
            100.0 * self.loose_fast_share(),
            self.loose_windows,
            self.all_idle,
            self.stuck.len(),
            self.far_from_goal,
            self.worst_overlap.0 as f64 / ORTH as f64,
            self.worst_overlap.1,
            self.end_overlap as f64 / ORTH as f64,
            self.expelled,
        );
    }
}

/// One right-click on the ground at `GOALS[goal_index]` with the twelve selected.
fn run(goal_index: usize) -> Report {
    let goal_cell = GOALS[goal_index];
    let trace = std::env::var("FORMATION_BASE_TRACE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        == Some(goal_index);
    let (mut world, ids) = base_start();
    let goal = Vec2::new(
        Fx::from_int(goal_cell.0) + Fx::HALF,
        Fx::from_int(goal_cell.1) + Fx::HALF,
    );
    let command = move_command(&ids, goal, false).expect("twelve units take an order");
    world
        .schedule(world.tick(), PlayerId(sandbox::MERIDIAN), command)
        .expect("the order is for the tick about to run");
    let n = ids.len();
    let radius: Vec<Fx> = ids
        .iter()
        .map(|&id| world.collision_radius_of(id).unwrap())
        .collect();

    let mut r = Report {
        goal: goal_cell,
        units: n,
        ..Report::default()
    };
    // Per tick: positions, whether the unit was in the formation of the group, how far ahead of
    // its slot along the heading it was, and whether the formation was loose.
    let mut history: Vec<Vec<Vec2>> = Vec::new();
    let mut member_at: Vec<Vec<bool>> = Vec::new();
    let mut along_hist: Vec<Vec<f64>> = Vec::new();
    let mut loose_hist: Vec<bool> = Vec::new();
    let mut serial = 0_u32;
    let mut slowest = f64::MAX;
    let mut last_leader: Option<usize> = None;
    let mut lead_run = 0_usize;
    let mut idle_since: Option<u64> = None;
    for step in 0..MAX_TICKS + SETTLE_TICKS {
        let report = world.step();
        let tick = report.tick.0;
        if step == 0 {
            // The order has begun: who follows a shared flowfield?
            for &id in &ids {
                let has_field = world.route_of(id).is_some_and(|v| v.field);
                let formed = world.formation_of(id).is_some();
                r.fields_of_members += usize::from(has_field && formed);
                r.fields_of_others += usize::from(has_field && !formed);
            }
        }
        let pos: Vec<Vec2> = ids.iter().map(|&id| world.get(id).unwrap().pos).collect();
        let views: Vec<_> = ids.iter().map(|&id| world.formation_of(id)).collect();
        if serial == 0 {
            serial = views.iter().flatten().map(|v| v.serial).min().unwrap_or(0);
        }
        let in_group: Vec<bool> = views
            .iter()
            .map(|v| v.is_some_and(|v| v.serial == serial && serial != 0))
            .collect();
        if r.members == 0 && in_group.iter().any(|&m| m) {
            r.members = in_group.iter().filter(|&&m| m).count();
            slowest = (0..n)
                .filter(|&k| in_group[k])
                .map(|k| cells(world.stats_of(ids[k]).unwrap().speed_per_tick))
                .fold(f64::MAX, f64::min);
            r.slowest = slowest;
        }
        // The formation of the group, as it stands this tick.
        let lead = views
            .iter()
            .enumerate()
            .find_map(|(k, v)| v.filter(|v| v.serial == serial && v.leader).map(|v| (k, v)));
        if let Some((l, lead)) = lead {
            r.with_formation += 1;
            r.loose += u64::from(lead.loose);
            let (mut worst, mut near, mut counted): (f64, u64, u64) = (0.0, 0, 0);
            for k in 0..n {
                let Some(v) = views[k].filter(|_| in_group[k]) else {
                    continue;
                };
                let off = cells((pos[k] - v.slot).length());
                worst = worst.max(off);
                counted += 1;
                near += u64::from(off <= NEAR);
            }
            r.worst_deviation = r.worst_deviation.max(worst);
            r.member_near += near;
            r.member_ticks += counted;
            if worst <= NEAR {
                r.in_shape += 1;
            }
            if !lead.loose {
                r.open += 1;
                r.open_in_shape += u64::from(worst <= NEAR);
            }
            // The leader's speed over a window it led all through.
            if last_leader == Some(l) && views[l].is_some_and(|v| v.mode == MemberMode::Lead) {
                lead_run += 1;
            } else {
                lead_run = 0;
            }
            if lead_run >= WINDOW && history.len() >= WINDOW {
                let then = &history[history.len() - WINDOW];
                let moved = cells((pos[l] - then[l]).length()) / WINDOW as f64;
                r.leader_top = r.leader_top.max(moved);
            }
            last_leader = Some(l);
        }
        let along_now: Vec<f64> = (0..n)
            .map(|k| match (views[k], lead) {
                (Some(v), Some((_, lead))) if in_group[k] => {
                    cells((pos[k] - v.slot).dot(lead.heading.to_vec2()))
                }
                _ => 0.0,
            })
            .collect();
        along_hist.push(along_now.clone());
        loose_hist.push(lead.is_some_and(|(_, l)| l.loose));
        history.push(pos.clone());
        member_at.push(in_group.clone());
        // The speed of every member that keeps up with its slot over the last window.
        if r.members > 0 && history.len() > WINDOW && lead.is_some() {
            let then = &history[history.len() - 1 - WINDOW];
            let before = &member_at[member_at.len() - 1 - WINDOW];
            let along_then = &along_hist[along_hist.len() - 1 - WINDOW];
            let all_loose = loose_hist[loose_hist.len() - 1 - WINDOW..]
                .iter()
                .all(|&x| x);
            for k in 0..n {
                if in_group[k]
                    && before[k]
                    && along_then[k] > -CATCHING_UP
                    && along_now[k] > -CATCHING_UP
                {
                    let ratio = cells((pos[k] - then[k]).length()) / (WINDOW as f64 * slowest);
                    let fast = u64::from(ratio > PACE_BOUND);
                    r.windows += 1;
                    r.fast_windows += fast;
                    if all_loose {
                        r.loose_windows += 1;
                        r.loose_fast += fast;
                    }
                }
            }
        }
        if trace && step % 100 == 99 {
            let mut line = format!("t{tick:5}");
            for k in 0..n {
                let before = &history[history.len().saturating_sub(100)];
                line += &format!(
                    " {}:{:.2}",
                    ids[k].index(),
                    cells((pos[k] - before[k]).length()) / 100.0 * 20.0
                );
                if let Some(v) = views[k] {
                    let mode = match v.mode {
                        MemberMode::Lead => 'L',
                        MemberMode::Direct => 'D',
                        MemberMode::Flow => 'F',
                        MemberMode::Wait => 'W',
                        MemberMode::None => '-',
                    };
                    line += &format!("{mode}{:.0}", cells((pos[k] - v.slot).length()));
                }
            }
            if let Some((_, l)) = lead {
                line += &format!(" | loose {} wait {} {:?}", l.loose, l.waiting, l.state);
            }
            eprintln!("{line}");
        }
        // Overlap, and when everybody is done.
        if tick >= 2 {
            for a in 0..n {
                for b in a + 1..n {
                    let overlap = (keep(radius[a], radius[b]) - pos[a].distance(pos[b])).raw();
                    if overlap > r.worst_overlap.0 {
                        r.worst_overlap = (overlap, tick);
                    }
                }
            }
        }
        let all_idle = ids
            .iter()
            .all(|&id| world.order_of(id) == Some(Order::Idle));
        if all_idle && tick > 2 {
            r.all_idle.get_or_insert(tick);
            idle_since.get_or_insert(tick);
        }
        if idle_since.is_some_and(|t| tick >= t + SETTLE_TICKS) {
            break;
        }
    }
    r.formed = world.formation_stats().formed;
    r.expelled = world.formation_stats().expelled;
    r.checksum = world.checksum();
    // The end.
    let last = &history[history.len() - 1];
    for a in 0..n {
        for b in a + 1..n {
            r.end_overlap = r
                .end_overlap
                .max((keep(radius[a], radius[b]) - last[a].distance(last[b])).raw());
        }
        if world.stuck_ticks_of(ids[a]) != Some(0) || world.replan_requested(ids[a]) != Some(false)
        {
            r.stuck.push(a);
        }
        r.far_from_goal = r.far_from_goal.max(cells((last[a] - goal).length()));
    }
    r
}

fn reports() -> &'static Vec<Report> {
    static REPORTS: OnceLock<Vec<Report>> = OnceLock::new();
    REPORTS.get_or_init(|| {
        let reports: Vec<Report> = (0..GOALS.len()).map(run).collect();
        for r in &reports {
            r.print();
        }
        reports
    })
}

#[test]
fn the_sandbox_has_twelve_mobile_meridian_units() {
    let (world, ids) = base_start();
    assert_eq!(ids.len(), 12);
    // Foot, tracked, wheeled and the amphibious transport's own class.
    let classes: std::collections::BTreeSet<u16> = ids
        .iter()
        .map(|&id| world.stats_of(id).unwrap().movement_class.unwrap())
        .collect();
    assert_eq!(classes.len(), 4, "{classes:?}");
}

#[test]
fn one_formation_forms_of_the_ground_units_and_the_amphibious_transport_is_left_out() {
    for r in reports() {
        assert_eq!(r.formed, 1, "goal {:?}", r.goal);
        assert_eq!(
            r.members,
            r.units - 1,
            "goal {:?}: every unit but the amphibious transport",
            r.goal
        );
    }
}

#[test]
fn the_unit_left_out_does_not_switch_off_the_shared_fields_of_the_classes_that_formed() {
    for r in reports() {
        // The foot, the tracked and the wheeled units are 5, 4 and 2 of an order of 12, below
        // the threshold of a field (8): they have one because each class is wholly in the
        // formation. The amphibious transport routes itself.
        assert_eq!(r.fields_of_members, r.members, "goal {:?}", r.goal);
        assert_eq!(r.fields_of_others, 0, "goal {:?}", r.goal);
    }
}

#[test]
fn a_loose_formation_keeps_the_pace() {
    let (mut loose_windows, mut loose_fast) = (0, 0);
    for r in reports() {
        // The leader never steps faster than the slowest member (a hair for the pushes of
        // the crowd pass).
        assert!(
            r.leader_top <= r.slowest * 1.15,
            "goal {:?}: the leader went {:.4} cells a tick over {WINDOW}, the slowest {:.4}",
            r.goal,
            r.leader_top,
            r.slowest
        );
        assert!(
            r.loose_windows > 300,
            "goal {:?}: {}",
            r.goal,
            r.loose_windows
        );
        // A member that keeps up with its slot runs ahead for a moment (up to the bound of
        // 2 cells) and goes no faster than the formation beyond it: over the loose stretches
        // of one goal at most 45 % of the windows are above 1.2 times the slowest's speed.
        assert!(
            r.loose_fast_share() <= 0.45,
            "goal {:?}: {:.0} % of the windows in loose stretches",
            r.goal,
            100.0 * r.loose_fast_share()
        );
        loose_windows += r.loose_windows;
        loose_fast += r.loose_fast;
    }
    // Over the four goals together it is the exception: before WP4.6b it was 29 %, a loose
    // formation being everybody at their own speed.
    let share = loose_fast as f64 / loose_windows as f64;
    assert!(
        share <= 0.20,
        "{:.1} % of the windows in loose stretches",
        100.0 * share
    );
}

#[test]
fn the_shape_holds_as_far_as_the_base_lets_it() {
    for r in reports() {
        // Loose for a good part of the march (the structures are narrow places), but not the
        // whole of it: the formation forms up again between them.
        assert!(
            r.open > 150,
            "goal {:?}: {} ticks not loose",
            r.goal,
            r.open
        );
        // The members near their slots, over all the ticks of the formation; the bound that
        // holds is half (it is 59 to 92 % with the shipped settings; the two heavy tracked
        // units that are stuck among the structures account for most of what is missing).
        assert!(
            r.member_share() >= 0.50,
            "goal {:?}: {:.0} % of the members near their slots",
            r.goal,
            100.0 * r.member_share()
        );
        // All of them at once, on the ticks that are not loose: 18 to 74 % as shipped.
        assert!(
            r.open_shape_share() >= 0.10,
            "goal {:?}: {:.0} % of the ticks that are not loose",
            r.goal,
            100.0 * r.open_shape_share()
        );
        // Nobody wanders off: the farthest any member was from its slot is the farthest a
        // stuck heavy unit was, far less than the map.
        assert!(r.worst_deviation < 40.0, "goal {:?}", r.goal);
    }
}

#[test]
fn everybody_arrives_nobody_is_stuck_and_bodies_do_not_overlap_beyond_phase_4() {
    for r in reports() {
        assert!(
            r.all_idle.is_some(),
            "goal {:?}: not everybody arrived",
            r.goal
        );
        assert!(r.stuck.is_empty(), "goal {:?}: stuck {:?}", r.goal, r.stuck);
        assert!(
            r.far_from_goal < 6.0,
            "goal {:?}: a unit ended {:.1} cells from the goal",
            r.goal,
            r.far_from_goal
        );
        // A quarter of a cell during the run, a sixteenth at the end.
        assert!(
            r.worst_overlap.0 < ORTH / 4,
            "goal {:?}: overlap {} at tick {}",
            r.goal,
            r.worst_overlap.0,
            r.worst_overlap.1
        );
        assert!(
            r.end_overlap < ORTH / 16,
            "goal {:?}: {}",
            r.goal,
            r.end_overlap
        );
    }
}

#[test]
fn the_run_is_reproducible() {
    let again = run(0);
    assert_eq!(again.checksum, reports()[0].checksum);
    assert_eq!(again.all_idle, reports()[0].all_idle);
}
