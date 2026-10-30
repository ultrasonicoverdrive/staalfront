//! Harvesters (WP7.2) on the shipped data: the `harvest` scenario (a refinery, three harvesters of
//! one player queueing on it, one of another player without a refinery, on the 256 by 256 cell
//! playfield) with its checksum pinned, and a harvester that gets stuck in traffic and asks for a
//! new route. See `assets/scenarios/harvest.ron` for who is who, and `docs/rules/economy.md`
//! ("Harvesters").

mod common;

use common::scenario_path;
use sf_data::movement::MovementCosts;
use sf_data::sim_settings::SimSettings;
use sf_data::terrain::TerrainTypes;
use sf_data::units::UnitDefs;
use sf_headless::cli::parse_checksum;
use sf_headless::run::{RunOptions, run};
use sf_headless::scenario::{Action, Scenario, TimedCommand};
use sf_math::{Fx, Vec2};
use sf_sim::{
    Cell, Command, EntityId, HarvestStage, Kind, Order, PlayerId, Terrain, Tick, World, WorldConfig,
};
use std::time::Instant;

const EXPECTED: &str = include_str!("../../../assets/scenarios/harvest.expected");

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn id(slot: u32) -> EntityId {
    EntityId::new(slot, 0)
}

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("harvest.ron")).unwrap()
}

/// What a run of the scenario recorded, tick by tick.
struct Log {
    world: World,
    /// `(tick, slot, stage)` every time a harvester changes stage.
    stages: Vec<(u64, u32, HarvestStage)>,
    /// `(tick, slot, units)` every time the load of a harvester in the bay dropped.
    unload_bites: Vec<(u64, u32, u16)>,
    /// The most harvesters that were in the bay (unloading) in one tick.
    most_in_bay: usize,
    /// The longest queue seen at the refinery.
    longest_queue: usize,
    /// Per tick: ore left in the field cells plus the loads carried plus the ore delivered.
    totals: Vec<u64>,
}

fn run_scenario() -> Log {
    let mut world = scenario().build_world().unwrap();
    let mut log = Log {
        world: world.clone(),
        stages: Vec::new(),
        unload_bites: Vec::new(),
        most_in_bay: 0,
        longest_queue: 0,
        totals: Vec::new(),
    };
    let mut last: [Option<HarvestStage>; 5] = [None; 5];
    let mut loads = [0u16; 5];
    for _ in 0..4500 {
        world.step();
        let tick = world.tick().0;
        let mut in_bay = 0;
        let mut carried = 0u64;
        for slot in 1..5u32 {
            let v = world.harvester_of(id(slot));
            let stage = v.map(|v| v.stage);
            let was_in_bay = last[slot as usize] == Some(HarvestStage::Unloading);
            if stage != last[slot as usize] {
                if let Some(s) = stage {
                    log.stages.push((tick, slot, s));
                }
                last[slot as usize] = stage;
            }
            let load = v.map_or(0, |v| v.load);
            carried += u64::from(load);
            // A bite of the dock: the load drops while the harvester is in the bay (the last bite
            // is the tick it is released, so it is no longer in the bay in the same tick).
            if load < loads[slot as usize] && was_in_bay {
                log.unload_bites
                    .push((tick, slot, loads[slot as usize] - load));
            }
            loads[slot as usize] = load;
            in_bay += usize::from(stage == Some(HarvestStage::Unloading));
        }
        log.most_in_bay = log.most_in_bay.max(in_bay);
        if let Some(d) = world.dock_of(id(0)) {
            log.longest_queue = log.longest_queue.max(d.queue.len());
        }
        let delivered = world.ore_delivered(P0) + world.ore_delivered(P1);
        log.totals
            .push(world.ore().unwrap().total() + carried + delivered);
    }
    log.world = world;
    log
}

#[test]
fn the_final_checksum_is_the_pinned_value() {
    let expected = parse_checksum(EXPECTED).unwrap();
    let s = scenario();
    let outcome = run(&s, &RunOptions::for_scenario(&s)).unwrap();
    assert_eq!(outcome.final_checksum, expected);
}

#[test]
fn every_harvester_runs_whole_cycles() {
    let log = run_scenario();
    for slot in [1u32, 2, 4] {
        // Player 0's three harvesters each unloaded at least twice.
        let mine: Vec<HarvestStage> = log
            .stages
            .iter()
            .filter(|&&(_, s, _)| s == slot)
            .map(|&(_, _, st)| st)
            .collect();
        let unloads = mine
            .iter()
            .filter(|&&st| st == HarvestStage::Unloading)
            .count();
        assert!(unloads >= 2, "slot {slot} unloaded {unloads} times");
        // And between two unloadings they went through the whole loop, in order.
        let first = mine
            .iter()
            .position(|&s| s == HarvestStage::Unloading)
            .unwrap();
        let second = first
            + 1
            + mine[first + 1..]
                .iter()
                .position(|&s| s == HarvestStage::Unloading)
                .unwrap();
        let between = &mine[first + 1..second];
        let order = |s: HarvestStage| {
            between
                .iter()
                .position(|&x| x == s)
                .unwrap_or_else(|| panic!("slot {slot}: {s:?} missing in {between:?}"))
        };
        assert!(order(HarvestStage::ToOre) < order(HarvestStage::Mining));
        assert!(order(HarvestStage::Mining) < order(HarvestStage::ToRefinery));
        assert!(order(HarvestStage::ToRefinery) < order(HarvestStage::Granted));
    }
}

#[test]
fn the_delivered_total_is_asserted_and_nothing_is_lost_or_made() {
    let log = run_scenario();
    let w = &log.world;
    let delivered = w.ore_delivered(P0);
    // Player 1 has no refinery: it mines a load and holds it.
    assert_eq!(w.ore_delivered(P1), 0);
    let held = w.harvester_of(id(3)).unwrap();
    assert_eq!(held.stage, HarvestStage::NoRefinery);
    assert_eq!(held.load, 200);
    // The total that was unloaded is the sum of the unloading bites seen.
    let bites: u64 = log.unload_bites.iter().map(|&(_, _, n)| u64::from(n)).sum();
    assert_eq!(delivered, bites);
    // Eight whole loads and 80 units of the ninth (the bay is mid-unload at the last tick).
    assert_eq!(delivered, 1680);
    // Conservation: the ore left in the cells plus the loads carried plus the ore delivered only
    // ever rises by the regrowth, and never falls. The tick's growth is the number of field
    // cells whose turn it is, one unit each: at most `215 / 200 + 1` cells per tick here.
    for pair in log.totals.windows(2) {
        assert!(
            pair[1] >= pair[0],
            "ore vanished: {} to {}",
            pair[0],
            pair[1]
        );
        assert!(
            pair[1] - pair[0] <= 2,
            "ore appeared: {} to {}",
            pair[0],
            pair[1]
        );
    }
}

#[test]
fn harvesters_take_turns_on_one_bay_and_the_second_one_queues() {
    let log = run_scenario();
    // Never two in the bay at once.
    assert_eq!(log.most_in_bay, 1);
    // Somebody queued, and stood while it did.
    assert!(log.longest_queue >= 1, "nobody ever queued");
    assert!(
        log.stages
            .iter()
            .any(|&(_, _, st)| st == HarvestStage::Queued)
    );
    // Every harvester of player 0 got its turn more than once.
    let entries = |slot: u32| {
        log.stages
            .iter()
            .filter(|&&(_, s, st)| s == slot && st == HarvestStage::Unloading)
            .count()
    };
    assert!(entries(1) >= 2 && entries(2) >= 2 && entries(4) >= 2);
}

/// A harvester that drives to an ore cell stops and mines within one cell of it, so two harvesters
/// at neighbouring cells do not hold each other off: no stretch of `ToOre` is longer than the drive
/// from the refinery plus a margin. (Without the stop, harvesters stood against each other in
/// `ToOre` for thousands of ticks.)
#[test]
fn no_harvester_is_stuck_driving_to_ore() {
    let log = run_scenario();
    let mut longest = 0u64;
    for slot in [1u32, 2, 3, 4] {
        let mine: Vec<(u64, HarvestStage)> = log
            .stages
            .iter()
            .filter(|&&(_, s, _)| s == slot)
            .map(|&(t, _, st)| (t, st))
            .collect();
        for pair in mine.windows(2) {
            if pair[0].1 == HarvestStage::ToOre {
                longest = longest.max(pair[1].0 - pair[0].0);
            }
        }
    }
    println!("longest ToOre stretch: {longest} ticks");
    assert!(longest < LONGEST_TO_ORE, "{longest} ticks in ToOre");
}

/// The longest stretch seen is 686 ticks (the first drive, round the rock patch); the jam without
/// the stop lasted 1900.
const LONGEST_TO_ORE: u64 = 1000;

#[test]
fn the_player_without_a_refinery_gets_nothing_and_does_not_clog_the_bay() {
    let log = run_scenario();
    assert!(
        log.stages
            .iter()
            .any(|&(_, s, st)| s == 3 && st == HarvestStage::NoRefinery)
    );
    assert!(
        !log.stages
            .iter()
            .any(|&(_, s, st)| s == 3 && st == HarvestStage::Unloading)
    );
}

// ---- a harvester stuck in traffic ----------------------------------------------------------

/// A 64 by 32 map: a rock wall across x 20 and 21 with two openings one cell wide, at y 10 and y 24;
/// the refinery on the left at (4, 10) (exit cell (7, 12)); a field of 5 by 5 ore cells on the
/// right at x 40 to 44, y 10 to 14. With a holder, a heavy tank of the other player stands on hold
/// in the upper opening: the way the harvester's route takes is shut by a body the route knows
/// nothing about.
fn jam_world(with_holder: bool) -> World {
    let types = TerrainTypes::embedded();
    let defs = UnitDefs::embedded();
    let costs = MovementCosts::from_defs(&defs, &types).unwrap();
    let grass = types.id_of("grass").unwrap();
    let rock = types.id_of("rock").unwrap();
    let ore = types.id_of("ore").unwrap();
    let (w, h) = (64u32, 32u32);
    let mut ids = vec![grass; (w * h) as usize];
    for y in 0..h {
        for x in 20..22 {
            if y != 10 && y != 24 {
                ids[(y * w + x) as usize] = rock;
            }
        }
    }
    for y in 10..15 {
        for x in 40..45 {
            ids[(y * w + x) as usize] = ore;
        }
    }
    let terrain =
        Terrain::from_layer(w, h, &ids, &sf_bridge::terrain_defs(&types, &costs)).unwrap();
    let settings = SimSettings::embedded();
    let mut world = World::with_terrain(WorldConfig::new(5).with_map_size(w, h), terrain)
        .unwrap()
        .with_ore(sf_bridge::ore_rules(&sf_data::ore::OreSettings::embedded()))
        .unwrap()
        .with_units(sf_bridge::unit_table(&defs).unwrap())
        .unwrap()
        .with_movement(sf_bridge::movement_table(&costs).unwrap())
        .unwrap()
        .with_path_budget(sf_bridge::path_budget(&settings))
        .unwrap()
        .with_flowfield_threshold(sf_bridge::flowfield_threshold(&settings))
        .unwrap()
        .with_collision(sf_bridge::collision_config(&settings))
        .unwrap()
        .with_replan(sf_bridge::replan_config(&settings))
        .unwrap()
        .with_formation(sf_bridge::formation_config(&settings))
        .unwrap()
        .with_buildings(sf_bridge::building_table(&defs).unwrap())
        .unwrap()
        .with_harvest(
            sf_bridge::harvest_rules(&sf_data::harvest::HarvestSettings::embedded()),
            &sf_bridge::harvester_kinds(&defs),
        )
        .unwrap();
    let kind = |name: &str| Kind(defs.kind_of(name).unwrap());
    let at = |c: i32, r: i32| Vec2::new(Fx::from_int(c) + Fx::HALF, Fx::from_int(r) + Fx::HALF);
    let place = Command::PlaceStructure {
        kind: kind("refinery"),
        cell: Cell::new(4, 10),
        owner: P0,
    };
    world.schedule(Tick(0), P0, place).unwrap();
    let spawn = Command::SpawnUnit {
        kind: kind("harvester"),
        pos: at(10, 12),
        owner: P0,
    };
    world.schedule(Tick(0), P0, spawn).unwrap();
    if with_holder {
        let tank = Command::SpawnUnit {
            kind: kind("heavy_tank"),
            pos: at(20, 10),
            owner: P1,
        };
        world.schedule(Tick(0), P1, tank).unwrap();
        let hold = Command::Order {
            ids: vec![id(2)],
            order: Order::HoldPosition,
            queued: false,
        };
        world.schedule(Tick(1), P1, hold).unwrap();
    }
    world
}

/// Runs the jam world and reports `(most replans asked in one order, crossed the wall by the lower
/// opening, crossed it by the upper one)`. A crossing is a change of side of the wall (columns 20
/// and 21; its openings are the rows 10 and 24), judged by the row it was in last inside the wall.
fn drive_through_the_wall(w: &mut World) -> (u8, bool, bool) {
    let harvester = id(1);
    let mut most = 0u8;
    let (mut lower, mut upper) = (false, false);
    // -1 left of the wall, +1 right of it.
    let mut side = -1i32;
    let mut wall_row = 0i32;
    for _ in 0..9000 {
        w.step();
        if let Some(n) = w.replans_of(harvester) {
            most = most.max(n);
        }
        let (x, y) = w.get(harvester).unwrap().pos.cell();
        if (20..22).contains(&x) {
            wall_row = y;
        } else {
            let now = if x < 20 { -1 } else { 1 };
            if now != side {
                side = now;
                if wall_row >= 17 {
                    lower = true;
                } else {
                    upper = true;
                }
            }
        }
    }
    (most, lower, upper)
}

#[test]
fn a_harvester_stuck_behind_a_holder_asks_for_a_new_route_and_still_delivers() {
    let mut w = jam_world(true);
    let (most, lower, upper) = drive_through_the_wall(&mut w);
    // It asked for new routes round the body that stood in the way...
    assert!(most >= 1, "it never replanned");
    // ... went through the lower opening, and never through the upper one (the tank holds it)...
    assert!(lower, "it never used the lower opening");
    assert!(!upper, "it went through the opening the holder shut");
    // ... and carried on with its work: it delivered whole loads.
    assert!(
        w.ore_delivered(P0) >= 200,
        "delivered {}",
        w.ore_delivered(P0)
    );
}

#[test]
fn without_the_holder_it_takes_the_upper_opening_and_never_replans() {
    let mut w = jam_world(false);
    let (most, _lower, upper) = drive_through_the_wall(&mut w);
    assert_eq!(most, 0);
    assert!(upper);
    assert!(w.ore_delivered(P0) >= 200);
}

// ---- cost ---------------------------------------------------------------------------------------

/// The `harvest` scenario with 50 harvesters of player 0 (a lattice of 54 places east of the rock
/// patch, on open grass) and three refineries, over `ticks` ticks; with `harvest` off the same units
/// stand where they are put (no harvest rules).
fn crowd_scenario(harvest: bool, ticks: u64) -> Scenario {
    let mut s = scenario();
    s.ticks = ticks;
    if !harvest {
        s.harvest = None;
    }
    s.commands.clear();
    let mut push = |command: Action| {
        s.commands.push(TimedCommand {
            tick: 0,
            player: 0,
            command,
        })
    };
    for cell in [(96, 86), (108, 76), (90, 80)] {
        push(Action::PlaceStructure {
            kind: 2,
            owner: 0,
            cell,
        });
    }
    for y in (94..=108).step_by(2) {
        for x in (106..=116).step_by(2) {
            push(Action::SpawnUnit {
                kind: 11,
                owner: 0,
                cell: (x, y),
            });
        }
    }
    s
}

/// The crowd scenario with only the first `n` of its harvesters (the WP7.2 review's measurement).
fn crowd_of(n: usize, ticks: u64) -> Scenario {
    let mut s = crowd_scenario(true, ticks);
    let mut seen = 0;
    s.commands.retain(|c| match c.command {
        Action::SpawnUnit { .. } => {
            seen += 1;
            seen <= n
        }
        _ => true,
    });
    s
}

/// Ore delivered to player 0 by the end of `scenario`.
fn delivered_by(scenario: &Scenario) -> u64 {
    let mut world = scenario.build_world().unwrap();
    for _ in 0..scenario.ticks {
        world.step();
    }
    world.ore_delivered(P0)
}

/// Prints the delivery of 3, 6, 12, 24 and 48 harvesters on three refineries in 4000 ticks.
/// `cargo test --release -p sf_headless --test harvest -- --ignored --nocapture throughput_table`.
#[test]
#[ignore]
fn throughput_table() {
    for n in [3, 6, 12, 24, 48] {
        println!("{n:2} harvesters: {} ore", delivered_by(&crowd_of(n, 4000)));
    }
}

/// Milliseconds a run of `scenario` takes, and the harvesters alive at the end.
fn timed(scenario: &Scenario) -> (f64, usize, u64) {
    let mut world = scenario.build_world().unwrap();
    let started = Instant::now();
    for _ in 0..scenario.ticks {
        world.step();
    }
    let ms = started.elapsed().as_secs_f64() * 1e3;
    (ms, world.len() - 3, world.ore_delivered(P0))
}

/// Prints the relative cost of the harvest stage with 50 harvesters: alternating runs of the same
/// scenario with the harvest rules on and off. Run it in release:
/// `cargo test --release -p sf_headless --test harvest -- --ignored --nocapture harvest_cost`.
#[test]
#[ignore]
fn harvest_cost() {
    let ticks = 4000;
    let (on, off) = (crowd_scenario(true, ticks), crowd_scenario(false, ticks));
    let mut rows = Vec::new();
    for round in 0..5 {
        let (with, n, delivered) = timed(&on);
        let (without, _, _) = timed(&off);
        println!(
            "round {round}: {with:8.1} ms with the harvest rules, {without:8.1} ms without \
             ({n} harvesters, {delivered} ore delivered in {ticks} ticks)"
        );
        rows.push((with, without));
    }
    let mean = |f: fn(&(f64, f64)) -> f64| rows.iter().map(f).sum::<f64>() / rows.len() as f64;
    let (with, without) = (mean(|r| r.0), mean(|r| r.1));
    println!(
        "mean: {with:.1} ms with, {without:.1} ms without: {:.1} microseconds per tick more with \
         harvesting ({:.1} microseconds per tick in all)",
        (with - without) * 1e3 / f64::from(ticks as u32),
        with * 1e3 / f64::from(ticks as u32)
    );
}

/// Prints the relative cost of storage (WP7.3) with 50 harvesters: alternating runs of the crowd
/// scenario with the harvest rules, with and without storage rules (1,000,000 per refinery, so the
/// store never fills and both runs do the same work apart from the bookkeeping). Run it in release:
/// `cargo test --release -p sf_headless --test harvest -- --ignored --nocapture storage_cost`.
#[test]
#[ignore]
fn storage_cost() {
    let ticks = 4000;
    let plain = crowd_scenario(true, ticks);
    let mut stored = plain.clone();
    stored.storage = Some(vec![(2, 1_000_000)]);
    let mut rows = Vec::new();
    for round in 0..7 {
        let (with, _, d1) = timed(&stored);
        let (without, _, d0) = timed(&plain);
        assert_eq!(
            d0, d1,
            "the store never fills, so the runs deliver the same"
        );
        println!("round {round}: {with:8.1} ms with storage, {without:8.1} ms without");
        rows.push((with, without));
    }
    let mean = |f: fn(&(f64, f64)) -> f64| rows.iter().map(f).sum::<f64>() / rows.len() as f64;
    let (with, without) = (mean(|r| r.0), mean(|r| r.1));
    println!(
        "mean: {with:.1} ms with, {without:.1} ms without: {:.2} microseconds per tick more",
        (with - without) * 1e3 / f64::from(ticks as u32)
    );
}

/// Prints the relative cost of money (WP7.4) with 50 harvesters and twelve power plants going up
/// at once (three construction centres, four plants each): alternating runs of the crowd scenario
/// with storage (1,000,000 per refinery) with and without money rules. Player 0 starts with
/// 1,000,000 credits, so nothing ever waits and both runs do the same work apart from the
/// bookkeeping (the harvesters deliver the same, the plants are the same). Two lengths: 4000 ticks
/// (the plants are done after 300), and 300 ticks (the plants are going up the whole time).
/// Run it in release:
/// `cargo test --release -p sf_headless --test harvest -- --ignored --nocapture money_cost`.
#[test]
#[ignore]
fn money_cost() {
    for ticks in [4000u64, 300] {
        let mut plain = crowd_scenario(true, ticks);
        plain.storage = Some(vec![(2, 1_000_000)]);
        for (x, y) in [(60, 60), (76, 60), (60, 44)] {
            plain.commands.push(TimedCommand {
                tick: 0,
                player: 0,
                command: Action::PlaceStructure {
                    kind: 0,
                    owner: 0,
                    cell: (x, y),
                },
            });
            for cell in [(x + 5, y), (x + 5, y + 3), (x - 3, y), (x - 3, y + 3)] {
                plain.commands.push(TimedCommand {
                    tick: 1,
                    player: 0,
                    command: Action::BuildStructure { kind: 1, cell },
                });
            }
        }
        let mut money = plain.clone();
        money.money = Some(sf_headless::scenario::MoneySpec {
            credits_per_ore: 1,
            starting: vec![(0, 1_000_000)],
        });
        let mut probe = money.build_world().unwrap();
        for _ in 0..2 {
            probe.step();
        }
        println!(
            "under construction: {}, refused: {:?}",
            probe.under_construction(),
            probe
                .placement_rejections()
                .iter()
                .map(|r| (r.cell, r.reason))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            probe.under_construction(),
            12,
            "all twelve plants are accepted"
        );
        let mut rows = Vec::new();
        for round in 0..7 {
            let (with, _, d1) = timed(&money);
            let (without, _, d0) = timed(&plain);
            assert_eq!(d0, d1, "nothing waits, so the runs deliver the same");
            println!(
                "{ticks} ticks, round {round}: {with:8.1} ms with money, {without:8.1} ms without"
            );
            rows.push((with, without));
        }
        let mean = |f: fn(&(f64, f64)) -> f64| rows.iter().map(f).sum::<f64>() / rows.len() as f64;
        let (with, without) = (mean(|r| r.0), mean(|r| r.1));
        println!(
            "{ticks} ticks, mean: {with:.1} ms with, {without:.1} ms without: {:.2} microseconds per tick more",
            (with - without) * 1e3 / ticks as f64
        );
        if ticks != 300 {
            continue;
        }
        // The same without the harvesters: the plants and the refineries alone, where the payments
        // are not drowned in the crowd's routing. 40 alternating rounds.
        let mut plants_money = money.clone();
        let mut plants_plain = plain.clone();
        for s in [&mut plants_money, &mut plants_plain] {
            s.commands
                .retain(|c| !matches!(c.command, Action::SpawnUnit { .. }));
        }
        let mut rows = Vec::new();
        for _ in 0..40 {
            let (with, _, _) = timed(&plants_money);
            let (without, _, _) = timed(&plants_plain);
            rows.push((with, without));
        }
        let n = rows.len() as f64;
        let (with, without) = (
            rows.iter().map(|r| r.0).sum::<f64>() / n,
            rows.iter().map(|r| r.1).sum::<f64>() / n,
        );
        println!(
            "plants only, 300 ticks, 40 rounds, mean: {with:.2} ms with, {without:.2} ms without: {:.3} microseconds per tick more",
            (with - without) * 1e3 / ticks as f64
        );
    }
}
