//! The rules of the AI's construction turn where each one decides (WP9.1b-1, `docs/rules/ai.md`, "The base"):
//! the emergencies that start unfunded, the harvester reserve, the cap of a build's savings at what the store
//! holds, the budget of work already promised, the question cap and the resumption of the placement search,
//! the refusals it remembers, and the exit cell of the placement fence. Each test is the `ai_base` scenario
//! (or its world) changed so that the one rule makes the difference, and runs the production code: the
//! runner's own driver (`run_watched`), `Observation::build`, `Lens` and `placement::search`.

mod common;

use std::collections::BTreeMap;

use common::scenario_path;
use sf_ai::placement::search;
use sf_ai::{Ai, Lens, Observation};
use sf_data::match_def::MatchDef;
use sf_data::units::UnitDefs;
use sf_headless::run::{Finished, RunOptions, run_full, run_watched};
use sf_headless::scenario::{Action, Scenario, TimedCommand};
use sf_sim::{Cell, Command, Kind, PlacementError, PlayerId, Tick, Visibility, World, tick_cost};

const P0: PlayerId = PlayerId(0);
const PLANT: Kind = Kind(1);
const REFINERY: Kind = Kind(2);
const BARRACKS: Kind = Kind(3);
const FACTORY: Kind = Kind(4);

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("ai_base.ron")).unwrap()
}

fn go(s: &Scenario, ticks: u64) -> Finished {
    run_full(s, &RunOptions { ticks, every: 100 }).unwrap()
}

/// `s` with its match file changed by `change`; the starting objects are made again from it the way
/// `Scenario::load` makes them.
fn with_match(mut s: Scenario, change: impl FnOnce(&mut MatchDef)) -> Scenario {
    let mut def = s.match_def.clone().unwrap();
    change(&mut def);
    let defs = UnitDefs::load(&scenario_path("ai_base_defs"), &scenario_path("..")).unwrap();
    s.match_commands = sf_bridge::match_commands(&def, &defs).unwrap();
    s.match_def = Some(def);
    s
}

/// Player 0 starts with `credits` and, besides its construction centre, the objects `(role, cell)`.
fn start(credits: u64, objects: &[(&str, (u32, u32))]) -> Scenario {
    with_match(scenario(), |def| {
        let p = &mut def.players[0];
        p.credits = credits;
        for &(role, cell) in objects {
            let mut o = p.objects[0].clone();
            o.role = role.to_string();
            o.cell = cell;
            p.objects.push(o);
        }
    })
}

/// The builds the AI sent: `(tick, kind)`.
fn builds(f: &Finished) -> Vec<(u64, Kind)> {
    f.ai_commands
        .iter()
        .filter_map(|(t, _, c)| match c {
            Command::BuildStructure { kind, .. } => Some((*t, *kind)),
            _ => None,
        })
        .collect()
}

// ---- the money rule -----------------------------------------------------------------------------

#[test]
fn a_refinery_starts_unfunded_when_none_stands() {
    // 400 credits: the power plant (300) at tick 10, then the refinery (1200) with 100 free, because a player
    // without a refinery has no income at all. A normal build would wait for its full price.
    let f = go(&start(400, &[]), 600);
    let b = builds(&f);
    assert_eq!(b[0], (10, PLANT));
    assert_eq!(b[1].1, REFINERY, "{b:?}");
    assert!(f.world.credits(P0) < 1200);
}

#[test]
fn a_power_plant_starts_unfunded_when_power_is_short() {
    // A barracks (uses 20) and no power plant: less power made than used. With 100 credits the plant (300)
    // is sent all the same.
    let f = go(&start(100, &[("barracks", (17, 69))]), 100);
    assert_eq!(builds(&f), vec![(10, PLANT)]);
}

#[test]
fn a_normal_build_waits_for_its_full_price() {
    // The control of the two above: the barracks (400) after a standing plant and refinery, with 300 credits
    // and a harvester (so no reserve), is not sent.
    let f = go(
        &start(
            300,
            &[
                ("power_plant", (9, 68)),
                ("refinery", (17, 64)),
                ("harvester", (22, 66)),
            ],
        ),
        100,
    );
    assert!(builds(&f).is_empty(), "{:?}", builds(&f));
}

#[test]
fn without_a_harvester_a_build_also_keeps_a_harvesters_price() {
    // A plant and a refinery stand, no harvester exists or is queued (the refinery was placed, so it brought
    // none). The barracks costs 400, a harvester 1100: with 1400 credits it waits, with 1500 it is sent.
    let objects = [("power_plant", (9, 68)), ("refinery", (17, 64))];
    let poor = go(&start(1400, &objects), 1000);
    let harvesters = |f: &Finished| {
        f.world
            .snapshot()
            .iter()
            .filter(|v| v.owner == P0 && v.kind == Kind(11))
            .count()
    };
    assert_eq!(harvesters(&poor), 0);
    assert!(builds(&poor).is_empty(), "{:?}", builds(&poor));
    let rich = go(&start(1500, &objects), 100);
    assert_eq!(builds(&rich), vec![(10, BARRACKS)]);
}

#[test]
fn a_build_saves_up_no_more_than_the_store_can_hold() {
    // Refineries that store 1000: with one refinery the store holds 1000, so the vehicle factory (1500) is
    // started once 900 are free (90 percent, `silo_percent`). Waiting for 1500 would wait for ever.
    let mut s = start(2000, &[]);
    s.storage = Some(vec![(2, 1000), (43, 1500)]);
    let mut credits = BTreeMap::new();
    let f = run_watched(
        &s,
        &RunOptions {
            ticks: 8000,
            every: 100,
        },
        |w| {
            credits.insert(w.tick().0, w.credits(P0));
        },
    )
    .unwrap();
    let b = builds(&f);
    let &(at, _) = b
        .iter()
        .find(|&&(_, k)| k == FACTORY)
        .unwrap_or_else(|| panic!("no vehicle factory: {b:?}"));
    // The credits before the step the build ran in.
    let had = credits[&at];
    assert!((900..1500).contains(&had), "sent at tick {at} with {had}");
    assert!(f.world.storage_capacity(P0) <= 2000);
}

#[test]
fn the_budget_counts_what_a_structure_going_up_still_costs() {
    // Two builds at once (`max_parallel_builds` 2). With 650 credits the barracks (400) is sent at tick 10;
    // at the next turn (tick 50) the second plant (300) would fit the credits but not what is free of them:
    // 650 minus what the barracks still has to pay leaves less than 300. So nothing is sent at tick 50, and
    // nothing ever waits for money.
    let mut s = start(
        650,
        &[
            ("power_plant", (9, 68)),
            ("refinery", (17, 64)),
            ("harvester", (22, 66)),
        ],
    );
    let old = s.ais[0].state().clone();
    let mut profile = old.profile.clone();
    profile.economy.max_parallel_builds = 2;
    s.ais = vec![Ai::new(P0, profile, 1, old.start_sites.clone())];
    let mut stalled = false;
    let f = run_watched(
        &s,
        &RunOptions {
            ticks: 600,
            every: 100,
        },
        |w| {
            stalled |= !w.stalled().is_empty();
        },
    )
    .unwrap();
    let b = builds(&f);
    assert_eq!(b[0], (10, BARRACKS));
    assert!(b.get(1).is_none_or(|&(t, _)| t > 50), "{b:?}");
    assert!(!stalled);

    // The budget against the simulation's own payment rule: at tick 100 the barracks owes the price of every
    // tick still to build (`tick_cost`).
    let f = go(&s, 100);
    let obs = Observation::build(&f.world, P0, &BTreeMap::new());
    let barracks = obs.own.iter().find(|o| o.kind == BARRACKS).unwrap();
    let c = barracks.building.unwrap();
    let cost = f
        .world
        .building_table()
        .get(BARRACKS)
        .unwrap()
        .cost
        .unwrap();
    let owed: u64 = (c.total - c.left..c.total)
        .map(|done| tick_cost(cost, c.total, done))
        .sum();
    assert!(owed > 0);
    let lens = Lens::new(&f.world, P0);
    assert_eq!(sf_ai::budget::committed(&obs, &lens), owed);
    assert_eq!(sf_ai::budget::free(&obs, &lens), obs.credits - owed);
}

// ---- the placement search -----------------------------------------------------------------------

/// The world of `s` after one tick, its observation for player 0 and a fresh copy of the AI's state.
fn first_tick(s: &Scenario) -> (Finished, Observation) {
    let f = go(s, 1);
    let obs = Observation::build(&f.world, P0, &BTreeMap::new());
    (f, obs)
}

/// The best `n` places of a power plant on the world of `ai_base`, in the order the search tries them.
fn ranked(n: usize) -> Vec<Cell> {
    let (f, obs) = first_tick(&scenario());
    let mut state = f.ais[0].state().clone();
    let mut out = Vec::new();
    for _ in 0..n {
        let mut lens = Lens::new(&f.world, P0);
        let c = search(&mut state.clone(), &obs, &mut lens, PLANT).unwrap();
        state.refused.insert((PLANT, c), Tick(1_000_000));
        out.push(c);
    }
    out
}

#[test]
fn the_search_asks_at_most_the_cap_remembers_the_refusals_and_resumes_behind_them() {
    // Repair trucks of player 0 stand on the best two places of a plant; the AI is not told about them (they
    // are taken out of its observation), so its mask lets those places through and only the simulation says
    // no. With a cap of 2 the first search asks exactly those two, gets two refusals and finds nothing. The
    // next search, after the block and the refusals have expired, must not ask the same two again: it
    // resumes behind them and finds the third place.
    let best = ranked(3);
    let mut s = scenario();
    for c in &best[..2] {
        s.commands.push(TimedCommand {
            tick: 0,
            player: 0,
            command: Action::SpawnUnit {
                kind: 22,
                owner: 0,
                cell: (c.x, c.y),
            },
        });
    }
    let (f, mut obs) = first_tick(&s);
    obs.own.retain(|o| o.footprint.is_some());
    let mut state = f.ais[0].state().clone();
    state.profile.economy.placement_cap = 2;
    for c in &best[..2] {
        assert_eq!(
            f.world.check_placement(PLANT, *c, P0),
            Err(PlacementError::UnitInTheWay)
        );
    }

    let mut lens = Lens::new(&f.world, P0);
    assert_eq!(search(&mut state, &obs, &mut lens, PLANT), None);
    assert_eq!(lens.previews(), 2, "the cap");
    let blocked = Tick(obs.tick.0 + u64::from(state.profile.economy.blocked_ticks));
    for c in &best[..2] {
        assert_eq!(state.refused.get(&(PLANT, *c)), Some(&blocked), "{c:?}");
    }

    // The next search, when the refusals have expired as the construction turn drops them.
    obs.tick = blocked;
    state.refused.retain(|_, until| *until > blocked);
    assert!(state.refused.is_empty());
    let mut lens = Lens::new(&f.world, P0);
    assert_eq!(search(&mut state, &obs, &mut lens, PLANT), Some(best[2]));
    assert_eq!(lens.previews(), 1);
    assert!(state.resume.is_empty());
}

#[test]
fn the_lens_does_not_ask_when_the_exit_cell_is_hidden() {
    // A barracks (exit side +x) whose footprint is visible and whose exit cell is in the shroud, with a
    // structure of player 1 hidden on that exit cell: the simulation would say `ExitBlocked` and so give the
    // structure away. The lens does not ask.
    let mut s = start(6000, &[("power_plant", (9, 68))]);
    let (probe, _) = first_tick(&s);
    let w: &World = &probe.world;
    let seen = |x: u32, y: u32| w.visibility(P0, Cell::new(x, y)) == Visibility::Visible;
    // The first anchor east of the centre whose 2 x 3 footprint is visible and whose exit cell (x + 2, y + 1)
    // is not, within the build reach.
    let anchor = (60..80)
        .flat_map(|y| (16..24).map(move |x| (x, y)))
        .find(|&(x, y)| {
            (0..3).all(|dy| (0..2).all(|dx| seen(x + dx, y + dy))) && !seen(x + 2, y + 1)
        })
        .expect("a footprint at the edge of sight");
    let exit = (anchor.0 + 2, anchor.1 + 1);
    s.commands.push(TimedCommand {
        tick: 0,
        player: 1,
        command: Action::PlaceStructure {
            kind: 6,
            owner: 1,
            cell: exit,
        },
    });
    let (f, _) = first_tick(&s);
    let at = Cell::new(anchor.0, anchor.1);
    assert_eq!(
        f.world.check_placement(BARRACKS, at, P0),
        Err(PlacementError::ExitBlocked)
    );
    let mut lens = Lens::new(&f.world, P0);
    assert_eq!(lens.placement(BARRACKS, at), None);
    assert_eq!(lens.previews(), 0);
}
