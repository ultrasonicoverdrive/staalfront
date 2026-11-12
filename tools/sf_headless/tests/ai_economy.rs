//! The AI's economy manager (WP9.1b-2, `docs/rules/ai.md`, "The economy turn"): harvesters, the ore silo,
//! repair, and the rebuilding that falls out of the goal list. The scenarios `ai_economy` and `ai_rebuild`
//! with their checksums and AI digests pinned and the rules seen from outside (delivered ore, exactly one
//! silo, a lost harvester replaced, a lost refinery and barracks rebuilt, a damaged power plant repaired
//! with exactly one command), then each rule where it decides: the economy turn called on an observation
//! that is the world's own with the entities the test needs put in or changed (the way
//! `ai_base::the_search_keeps_off_the_apron_of_an_own_refinery` does), so every test runs the production
//! code (`sf_ai::economy::turn`, `Lens`, the runner's own driver).
//!
//! Player 0 is the AI (Meridian, a construction centre at (12, 68)) on the first match's map, player 1 a
//! lone static construction centre; both scenarios run on their own frozen copy of the definitions with a
//! goal list of four (power plant, refinery, barracks, vehicle factory).

mod common;

use std::collections::BTreeMap;

use common::scenario_path;
use sf_ai::economy::{self, REPAIRS_PER_TURN};
use sf_ai::{Lens, Observation, Own, OwnFactory};
use sf_headless::cli::parse_checksum;
use sf_headless::run::{Finished, RunOptions, run, run_full, run_watched};
use sf_headless::scenario::Scenario;
use sf_sim::{
    BuildingFunction, Command, Construction, EntityId, FactoryStatus, Footprint, Kind, Order,
    PlayerId, PowerTotals, Tick, World,
};

const P0: PlayerId = PlayerId(0);

const ECONOMY_EXPECTED: &str = include_str!("../../../assets/scenarios/ai_economy.expected");
const REBUILD_EXPECTED: &str = include_str!("../../../assets/scenarios/ai_rebuild.expected");
/// `Ai::digest` of player 0 at the end of each scenario.
const ECONOMY_DIGEST: u64 = 0xc79c_0cba_b7c6_ddb7;
const REBUILD_DIGEST: u64 = 0x15af_ee5a_c92b_e396;

/// The kinds of the frozen definitions.
const CENTRE: Kind = Kind(0);
const PLANT: Kind = Kind(1);
const REFINERY: Kind = Kind(2);
const BARRACKS: Kind = Kind(3);
const FACTORY: Kind = Kind(4);
const TOWER: Kind = Kind(6);
const HARVESTER: Kind = Kind(11);
const LIGHT_TANK: Kind = Kind(19);
const SILO: Kind = Kind(43);

fn load(name: &str) -> Scenario {
    Scenario::load(&scenario_path(&format!("{name}.ron"))).unwrap()
}

fn go(s: &Scenario, ticks: u64) -> Finished {
    run_full(s, &RunOptions { ticks, every: 100 }).unwrap()
}

#[test]
fn the_checksums_and_ai_digests_are_the_pinned_values() {
    for (name, expected, digest) in [
        ("ai_economy", ECONOMY_EXPECTED, ECONOMY_DIGEST),
        ("ai_rebuild", REBUILD_EXPECTED, REBUILD_DIGEST),
    ] {
        let s = load(name);
        let outcome = run(&s, &RunOptions::for_scenario(&s)).unwrap();
        assert_eq!(
            outcome.final_checksum,
            parse_checksum(expected).unwrap(),
            "{name}"
        );
        assert_eq!(outcome.ai_digests, vec![(0, digest)], "{name}");
        assert_eq!(outcome, run(&s, &RunOptions::for_scenario(&s)).unwrap());
    }
}

/// Player 0's units of `kind` in `world`.
fn units_of(world: &World, kind: Kind) -> usize {
    world
        .snapshot()
        .iter()
        .filter(|v| v.owner == P0 && v.kind == kind)
        .count()
}

/// The ticks at which each command of the AI that matches `pick` was sent.
fn sent(f: &Finished, pick: impl Fn(&Command) -> bool) -> Vec<u64> {
    f.ai_commands
        .iter()
        .filter(|(_, _, c)| pick(c))
        .map(|(t, _, _)| *t)
        .collect()
}

fn is_silo_build(c: &Command) -> bool {
    matches!(c, Command::BuildStructure { kind, .. } if *kind == SILO)
}

fn is_harvester_order(c: &Command) -> bool {
    matches!(c, Command::QueueUnit { kind, .. } if *kind == HARVESTER)
}

// ---- the scenarios -------------------------------------------------------------------------------

#[test]
fn the_delivered_ore_rises_in_every_window_of_2000_ticks() {
    let s = load("ai_economy");
    let mut delivered = vec![0u64];
    run_watched(&s, &RunOptions::for_scenario(&s), |w| {
        if w.tick().0 % 2000 == 0 {
            delivered.push(w.ore_delivered(P0));
        }
    })
    .unwrap();
    println!("delivered ore at 0, 2000, ...: {delivered:?}");
    assert_eq!(delivered.len(), 7, "{delivered:?}");
    for (i, pair) in delivered.windows(2).enumerate() {
        assert!(pair[1] > pair[0], "window {i}: {delivered:?}");
    }
    // Measured: 310 in the first window, 690 to 1,330 in the others; none is a trickle.
    assert!(delivered.windows(2).all(|p| p[1] - p[0] >= 250));
}

#[test]
fn exactly_one_silo_is_built_after_the_store_is_full() {
    let s = load("ai_economy");
    // Per tick: whether the store holds at least 90 percent of what it can (and can hold anything).
    let mut full = Vec::new();
    let mut silos_standing = Vec::new();
    run_watched(&s, &RunOptions::for_scenario(&s), |w| {
        let cap = w.storage_capacity(P0);
        full.push(cap > 0 && w.ore_stored(P0) * 100 >= cap * 90);
        if w.tick().0 % 100 == 0 {
            let n = w
                .snapshot()
                .iter()
                .filter(|v| v.owner == P0 && v.kind == SILO && w.stands(v.id))
                .count();
            silos_standing.push(n);
        }
    })
    .unwrap();
    let silo = sent(&go(&s, s.ticks), is_silo_build);
    assert_eq!(silo.len(), 1, "{silo:?}");
    // `full[i]` is the state after tick i + 1. The store was full at the start too (the credits the player
    // starts with), until the goal list spent them: the silo wait is counted from the full spell it ends.
    let mut full_at = silo[0] as usize;
    while full_at > 0 && full[full_at - 1] {
        full_at -= 1;
    }
    let full_at = full_at as u64 + 1;
    let before = full[..full_at as usize - 1].iter().filter(|&&f| f).count();
    println!(
        "the store is full from tick {full_at} (and was full for {before} ticks at the start); the silo is ordered at {silo:?}"
    );
    // Not before two economy turns (40 ticks apart, the first at or after the tick the store is full) and
    // the construction turn that follows (10 ticks later): at least 50 ticks, and not long after.
    assert!(
        silo[0] >= full_at + 50 && silo[0] <= full_at + 140,
        "full from {full_at}, silo at {silo:?}"
    );
    // Nothing was built while the base was still being built, although the store was full for 636 ticks
    // then, enough for two economy turns: the goal list came first (a goal was open).
    assert!(before >= 500, "{before}");
    // It stands from then on, and it is the only one: the second refinery of a base would call for a second.
    assert_eq!(*silos_standing.last().unwrap(), 1);
    assert!(silos_standing.iter().all(|&n| n <= 1), "{silos_standing:?}");
}

#[test]
fn a_harvester_lost_at_tick_t_is_replaced_and_only_once() {
    let s = load("ai_economy");
    // The scenario despawns player 0's free harvester (id 4) at tick 8001.
    let mut count = Vec::new();
    run_watched(&s, &RunOptions::for_scenario(&s), |w| {
        count.push((w.tick().0, units_of(w, HARVESTER)));
    })
    .unwrap();
    let at = |t: u64| count.iter().find(|c| c.0 == t).unwrap().1;
    assert_eq!((at(8000), at(8002)), (2, 1));
    let f = go(&s, s.ticks);
    let orders = sent(&f, is_harvester_order);
    println!("harvester orders at {orders:?}");
    // The first with the vehicle factory (tick 1998 stands, 2000 is the next economy turn), the second at the
    // first economy turn after the loss.
    assert_eq!(orders, [2000, 8040]);
    let back = count
        .iter()
        .find(|&&(t, n)| t > 8002 && n == 2)
        .map(|c| c.0)
        .unwrap();
    println!(
        "the harvester is lost at 8001, ordered at 8040 and out of the factory at tick {back}"
    );
    assert!(back <= 8040 + REPLACEMENT_TICKS, "back at {back}");
    assert_eq!(at(s.ticks), 2);
}

/// Measured: the harvester ordered at tick 8040 is out of the factory at tick 8440 (400 ticks); 20 percent more.
const REPLACEMENT_TICKS: u64 = 480;

#[test]
fn a_lost_refinery_and_a_lost_barracks_are_rebuilt() {
    let s = load("ai_rebuild");
    let mut stood: BTreeMap<EntityId, (Kind, u64)> = BTreeMap::new();
    run_watched(&s, &RunOptions::for_scenario(&s), |w| {
        for v in w.snapshot().iter().filter(|v| v.owner == P0) {
            if w.footprint_of(v.id).is_some() && w.stands(v.id) {
                stood.entry(v.id).or_insert((v.kind, w.tick().0));
            }
        }
    })
    .unwrap();
    let again = |kind: Kind, after: u64| {
        stood
            .values()
            .find(|&&(k, t)| k == kind && t > after)
            .map(|&(_, t)| t)
    };
    // The scenario removes the refinery at tick 3001 and the barracks at 6001.
    let refinery = again(REFINERY, 3001).unwrap();
    let barracks = again(BARRACKS, 6001).unwrap();
    println!(
        "T3: the refinery lost at 3001 stands again at {refinery}, the barracks lost at 6001 at {barracks}"
    );
    assert!(refinery <= 3001 + T3_REFINERY, "refinery at {refinery}");
    assert!(barracks <= 6001 + T3_BARRACKS, "barracks at {barracks}");
    // Each was ordered once, at the first construction turn after the loss.
    let go = go(&s, s.ticks);
    let orders: Vec<(u64, Kind)> = go
        .ai_commands
        .iter()
        .filter_map(|(t, _, c)| match c {
            Command::BuildStructure { kind, .. } if *t > 3000 && *kind != SILO => Some((*t, *kind)),
            _ => None,
        })
        .collect();
    assert_eq!(orders, [(3010, REFINERY), (6010, BARRACKS)]);
}

/// T3, measured: the refinery lost at tick 3001 stands again at 3545 (544 ticks: ordered at 3010, unfunded, and
/// built in 535) and the barracks lost at 6001 at 6278 (277 ticks); 20 percent more.
const T3_REFINERY: u64 = 660;
const T3_BARRACKS: u64 = 340;

#[test]
fn a_damaged_power_plant_is_repaired_to_full_with_exactly_one_command() {
    let s = load("ai_rebuild");
    let plant = EntityId::new(2, 0);
    let (mut lowest, mut full_again) = (u32::MAX, None);
    let f = run_watched(&s, &RunOptions::for_scenario(&s), |w| {
        let Some(hp) = w.snapshot().iter().find(|v| v.id == plant).map(|v| v.hp) else {
            return;
        };
        lowest = lowest.min(hp);
        if lowest < 420 && hp == 600 && full_again.is_none() {
            full_again = Some(w.tick().0);
        }
    })
    .unwrap();
    // 70 percent of 600 is 420: the shipped threshold is reached from above.
    assert!(lowest < 420, "lowest {lowest}");
    println!("the plant falls to {lowest} hit points and is full again at tick {full_again:?}");
    assert!(full_again.is_some());
    let go = go(&s, s.ticks);
    let repairs: Vec<(u64, Command)> = go
        .ai_commands
        .iter()
        .filter(|(_, _, c)| matches!(c, Command::Repair { .. }))
        .map(|(t, _, c)| (*t, c.clone()))
        .collect();
    assert_eq!(
        repairs,
        [(
            8200,
            Command::Repair {
                structure: plant,
                on: true
            }
        )]
    );
    assert_eq!(f.outcome.commands_ignored, 0);
}

#[test]
fn the_new_scenarios_send_only_what_a_player_may_send() {
    for name in ["ai_economy", "ai_rebuild"] {
        let s = load(name);
        let f = go(&s, s.ticks);
        assert!(!f.ai_commands.is_empty(), "{name}");
        for (tick, player, c) in &f.ai_commands {
            assert_eq!(*player, 0);
            assert!(
                matches!(
                    c,
                    Command::BuildStructure { .. }
                        | Command::QueueUnit { .. }
                        | Command::Repair { .. }
                        | Command::Sell { .. }
                ),
                "{name}: tick {tick}: {c:?}"
            );
        }
        // Nothing the AI sends is refused.
        assert_eq!(f.outcome.commands_ignored, 0, "{name}");
    }
}

#[test]
fn without_repair_rules_the_ai_asks_again_only_after_the_backoff() {
    // The same scenario in a world that has no repair rules: the simulation ignores `Repair` silently. The
    // plant stays damaged and the AI may ask again, but only once per `blocked_ticks`.
    let mut s = load("ai_rebuild");
    s.repair = None;
    let f = go(&s, s.ticks);
    let blocked = u64::from(f.ais[0].state().profile.economy.blocked_ticks);
    let repairs = sent(&f, |c| matches!(c, Command::Repair { .. }));
    println!("repair orders in a world without repair rules: {repairs:?}");
    assert!(repairs.len() >= 5, "{repairs:?}");
    for pair in repairs.windows(2) {
        assert!(pair[1] - pair[0] >= blocked, "{repairs:?}");
    }
    // Every one was ignored by the simulation.
    assert_eq!(f.outcome.commands_ignored, repairs.len() as u64);
}

// ---- the rules, each where it decides ------------------------------------------------------------

/// The world of `ai_economy` after one tick and the AI's state: the lens of every rule test below.
fn rig() -> (World, sf_ai::AiState) {
    let s = load("ai_economy");
    let f = go(&s, 1);
    (*f.world, f.ais[0].state().clone())
}

fn footprint(world: &World, kind: Kind) -> Option<Footprint> {
    world.building_table().get(kind).map(|b| b.footprint)
}

/// A structure of player 0 for the observation: complete and undamaged unless changed.
fn structure(world: &World, id: u32, kind: Kind) -> Own {
    Own {
        id: EntityId::new(id, 0),
        kind,
        cell: sf_sim::Cell::new(20 + id, 60),
        footprint: footprint(world, kind),
        hp: 1000,
        order: Order::Idle,
        standing: true,
        max_hp: 1000,
        building: None,
        repairing: false,
        factory: None,
        selling: false,
        armed: false,
        mobile: false,
    }
}

fn damaged(mut o: Own, hp: u32) -> Own {
    o.hp = hp;
    o
}

fn going_up(mut o: Own) -> Own {
    o.standing = false;
    o.building = Some(Construction {
        left: 500,
        total: 1000,
    });
    o
}

fn queued(mut o: Own, items: &[Kind]) -> Own {
    o.factory = Some(OwnFactory {
        queue: items.iter().map(|&k| (k, false)).collect(),
        front_paid: 0,
        status: FactoryStatus::Building,
        rally: None,
    });
    o
}

fn unit(id: u32, kind: Kind) -> Own {
    Own {
        id: EntityId::new(id, 0),
        kind,
        cell: sf_sim::Cell::new(30, 70),
        footprint: None,
        hp: 800,
        order: Order::Idle,
        standing: true,
        max_hp: 800,
        building: None,
        repairing: false,
        factory: None,
        selling: false,
        armed: false,
        mobile: true,
    }
}

/// The world's own observation at tick `tick` with `own` as the player's entities and `credits`.
fn observation(world: &World, tick: u64, credits: u64, own: Vec<Own>) -> Observation {
    let mut obs = Observation::build(world, P0, &BTreeMap::new());
    obs.tick = Tick(tick);
    obs.credits = credits;
    obs.power = Some(PowerTotals {
        made: 200,
        used: 100,
    });
    obs.own = own;
    obs
}

/// One economy turn: the commands it sends.
fn turn(world: &World, state: &mut sf_ai::AiState, obs: &Observation) -> Vec<Command> {
    let mut lens = Lens::new(world, P0);
    let mut out = Vec::new();
    economy::turn(state, obs, &mut lens, &mut out);
    out
}

/// A base with two refineries (four harvesters wanted), a vehicle factory (id 5) and `units` harvesters.
fn base_with(world: &World, units: u32) -> Vec<Own> {
    let mut own = vec![
        structure(world, 0, CENTRE),
        structure(world, 1, PLANT),
        structure(world, 2, REFINERY),
        structure(world, 3, REFINERY),
        structure(world, 5, FACTORY),
    ];
    own.extend((0..units).map(|i| unit(10 + i, HARVESTER)));
    own
}

#[test]
fn a_harvester_is_ordered_when_fewer_than_wanted_and_never_two_at_once() {
    let (world, mut state) = rig();
    assert_eq!(state.profile.economy.harvesters_per_refinery, 2);
    // Two refineries want four, there are two: one order, on the factory.
    let obs = observation(&world, 2000, 0, base_with(&world, 2));
    let order = Command::QueueUnit {
        factory: EntityId::new(5, 0),
        kind: HARVESTER,
    };
    assert_eq!(turn(&world, &mut state, &obs), [order]);
    assert_eq!(state.harvester_sent, Some((HARVESTER, Tick(2000))));
    // It is in the queue now: three of four, but one is on its way, so nothing more is ordered.
    let mut own = base_with(&world, 2);
    own[4] = queued(own[4].clone(), &[HARVESTER]);
    let obs = observation(&world, 2040, 0, own);
    assert_eq!(turn(&world, &mut state, &obs), []);
    assert_eq!(state.harvester_sent, None);
    assert!(state.blocked_until.is_empty());
    // Four of four: nothing.
    let obs = observation(&world, 2080, 0, base_with(&world, 4));
    assert_eq!(turn(&world, &mut state, &obs), []);
    // One harvester and one queued, two more wanted (four of four needs two): still one in flight at most.
    let mut own = base_with(&world, 1);
    own[4] = queued(own[4].clone(), &[HARVESTER]);
    let obs = observation(&world, 2120, 0, own);
    assert_eq!(turn(&world, &mut state, &obs), []);
}

#[test]
fn a_harvester_order_the_simulation_refused_is_not_repeated_before_the_backoff() {
    let (world, mut state) = rig();
    let blocked = u64::from(state.profile.economy.blocked_ticks);
    let obs = observation(&world, 2000, 0, base_with(&world, 2));
    assert_eq!(turn(&world, &mut state, &obs).len(), 1);
    // The next turn finds no harvester in any queue: the order was refused.
    let obs = observation(&world, 2040, 0, base_with(&world, 2));
    assert_eq!(turn(&world, &mut state, &obs), []);
    assert_eq!(
        state.blocked_until.get(&HARVESTER),
        Some(&Tick(2040 + blocked))
    );
    // Silent until the block ends, then it tries again.
    let obs = observation(&world, 2080, 0, base_with(&world, 2));
    assert_eq!(turn(&world, &mut state, &obs), []);
    let obs = observation(&world, 2040 + blocked, 0, base_with(&world, 2));
    assert_eq!(turn(&world, &mut state, &obs).len(), 1);
}

#[test]
fn a_harvester_goes_on_the_factory_with_the_shortest_queue_the_lowest_id_among_equals() {
    let (world, mut state) = rig();
    let mut own = base_with(&world, 2);
    // Factory 5 has a tank queued, factory 4 is empty and has the lower id; 6 is empty too.
    own[4] = queued(own[4].clone(), &[LIGHT_TANK]);
    own.push(structure(&world, 4, FACTORY));
    own.push(structure(&world, 6, FACTORY));
    own.sort_by_key(|o| o.id);
    let obs = observation(&world, 2000, 0, own.clone());
    assert_eq!(
        turn(&world, &mut state, &obs),
        [Command::QueueUnit {
            factory: EntityId::new(4, 0),
            kind: HARVESTER
        }]
    );
    // Factory 4 has two items, 5 has one, 6 none: 6, the shortest queue, although 4 and 5 have lower ids.
    let (_, mut state) = rig();
    let busy = own
        .into_iter()
        .map(|o| match o.id.index() {
            4 => queued(o, &[LIGHT_TANK, LIGHT_TANK]),
            _ => o,
        })
        .collect();
    let obs = observation(&world, 2000, 0, busy);
    assert_eq!(
        turn(&world, &mut state, &obs),
        [Command::QueueUnit {
            factory: EntityId::new(6, 0),
            kind: HARVESTER
        }]
    );
}

#[test]
fn no_harvester_without_a_refinery_a_factory_or_while_an_emergency_build_waits_for_money() {
    let (world, mut state) = rig();
    // No refinery standing: nothing wanted.
    let own = vec![structure(&world, 0, CENTRE), structure(&world, 5, FACTORY)];
    assert_eq!(
        turn(&world, &mut state, &observation(&world, 2000, 0, own)),
        []
    );
    // A refinery but no vehicle factory: nowhere to order it.
    let own = vec![structure(&world, 0, CENTRE), structure(&world, 2, REFINERY)];
    assert_eq!(
        turn(&world, &mut state, &observation(&world, 2040, 0, own)),
        []
    );
    // A power plant going up while less is made than used, and nothing to pay it with: the money is its.
    let mut own = base_with(&world, 2);
    own.push(going_up(structure(&world, 7, PLANT)));
    let mut obs = observation(&world, 2080, 0, own.clone());
    obs.power = Some(PowerTotals { made: 0, used: 50 });
    assert!(economy::emergency_stalled(&obs, &Lens::new(&world, P0)));
    assert_eq!(turn(&world, &mut state, &obs), []);
    // The same with credits: free money, so the order goes.
    let mut obs = observation(&world, 2120, 5000, own.clone());
    obs.power = Some(PowerTotals { made: 0, used: 50 });
    assert!(!economy::emergency_stalled(&obs, &Lens::new(&world, P0)));
    assert_eq!(turn(&world, &mut state, &obs).len(), 1);
    // A power plant going up with power to spare is no emergency, even with no credits.
    let mut state = rig().1;
    let obs = observation(&world, 2160, 0, own);
    assert!(!economy::emergency_stalled(&obs, &Lens::new(&world, P0)));
    assert_eq!(turn(&world, &mut state, &obs).len(), 1);
}

/// A state whose goal list is `[(plant, 1), (refinery, 1)]`, so that a base of those two closes it.
fn short_list(state: &mut sf_ai::AiState) {
    state.profile.build_order = vec![(PLANT, 1), (REFINERY, 1)];
}

fn full_store(obs: &mut Observation) {
    obs.storage_capacity = 2000;
    obs.ore_stored = 1800;
}

#[test]
fn the_silo_is_wanted_after_two_full_turns_with_no_goal_open() {
    let (world, mut state) = rig();
    short_list(&mut state);
    let own = vec![
        structure(&world, 0, CENTRE),
        structure(&world, 1, PLANT),
        structure(&world, 2, REFINERY),
    ];
    let mut obs = observation(&world, 2000, 0, own.clone());
    full_store(&mut obs);
    turn(&world, &mut state, &obs);
    assert_eq!((state.full_turns, state.emergency), (1, None));
    turn(&world, &mut state, &obs);
    assert_eq!((state.full_turns, state.emergency), (2, Some(SILO)));
    // A store one ore short of 90 percent is not full: the count starts again, the wish goes.
    obs.ore_stored = 1799;
    turn(&world, &mut state, &obs);
    assert_eq!((state.full_turns, state.emergency), (0, None));
    // A goal that is open (a barracks missing) keeps the silo back.
    let mut state = rig().1;
    state.profile.build_order = vec![(PLANT, 1), (REFINERY, 1), (BARRACKS, 1)];
    full_store(&mut obs);
    for _ in 0..4 {
        turn(&world, &mut state, &obs);
    }
    assert_eq!((state.full_turns, state.emergency), (4, None));
    // No storage at all (a world without storage rules): never full.
    let mut state = rig().1;
    short_list(&mut state);
    obs.storage_capacity = 0;
    for _ in 0..3 {
        turn(&world, &mut state, &obs);
    }
    assert_eq!((state.full_turns, state.emergency), (0, None));
}

#[test]
fn at_most_one_silo_per_refinery_and_one_going_up_counts() {
    let (world, mut state) = rig();
    short_list(&mut state);
    let one_refinery = vec![
        structure(&world, 0, CENTRE),
        structure(&world, 1, PLANT),
        structure(&world, 2, REFINERY),
        structure(&world, 4, SILO),
    ];
    let mut obs = observation(&world, 2000, 0, one_refinery);
    full_store(&mut obs);
    for _ in 0..3 {
        turn(&world, &mut state, &obs);
    }
    assert_eq!((state.full_turns, state.emergency), (3, None));
    // A silo that is still going up counts as well.
    let mut state = rig().1;
    short_list(&mut state);
    let mut own = obs.own.clone();
    own[3] = going_up(own[3].clone());
    let mut going = observation(&world, 2000, 0, own);
    full_store(&mut going);
    for _ in 0..3 {
        turn(&world, &mut state, &going);
    }
    assert_eq!(state.emergency, None);
    // Two refineries and one silo: a second one is wanted.
    let mut state = rig().1;
    short_list(&mut state);
    let mut own = obs.own.clone();
    own.push(structure(&world, 3, REFINERY));
    own.sort_by_key(|o| o.id);
    let mut two = observation(&world, 2000, 0, own);
    full_store(&mut two);
    for _ in 0..2 {
        turn(&world, &mut state, &two);
    }
    assert_eq!(state.emergency, Some(SILO));
}

/// A base for the repair rules: the centre, a power plant, a barracks, a refinery and a tower, all at full.
fn repair_base(world: &World) -> Vec<Own> {
    vec![
        structure(world, 0, CENTRE),
        structure(world, 1, PLANT),
        structure(world, 2, BARRACKS),
        structure(world, 3, REFINERY),
        structure(world, 4, TOWER),
    ]
}

fn repairs(out: &[Command]) -> Vec<u32> {
    out.iter()
        .filter_map(|c| match c {
            Command::Repair {
                structure,
                on: true,
            } => Some(structure.index()),
            _ => None,
        })
        .collect()
}

#[test]
fn repair_goes_by_rank_two_at_a_time_and_only_below_the_threshold() {
    let (world, mut state) = rig();
    assert_eq!(state.profile.economy.repair_below_percent, 70);
    assert_eq!(REPAIRS_PER_TURN, 2);
    // Exactly 70 percent is not below it; 699 of 1000 is.
    let mut own = repair_base(&world);
    own[1] = damaged(own[1].clone(), 700);
    own[2] = damaged(own[2].clone(), 699);
    let out = turn(&world, &mut state, &observation(&world, 100, 500, own));
    assert_eq!(repairs(&out), [2]);
    // The anchor first, then a power plant or a factory, then the dock, then the rest; two a turn.
    let (_, mut state) = rig();
    let mut own = repair_base(&world);
    own = own.into_iter().map(|o| damaged(o, 100)).collect();
    let out = turn(
        &world,
        &mut state,
        &observation(&world, 100, 500, own.clone()),
    );
    assert_eq!(repairs(&out), [0, 1]);
    // Next turn: those two are in the backoff; plant-class 2 (the barracks) and the dock follow.
    let out = turn(
        &world,
        &mut state,
        &observation(&world, 140, 500, own.clone()),
    );
    assert_eq!(repairs(&out), [2, 3]);
    let out = turn(&world, &mut state, &observation(&world, 180, 500, own));
    assert_eq!(repairs(&out), [4]);
    // A power plant before the dock although the dock has the lower id, and the dock before the tower
    // that has a lower id still.
    let (_, mut state) = rig();
    let own = vec![
        structure(&world, 0, CENTRE),
        damaged(structure(&world, 1, TOWER), 100),
        damaged(structure(&world, 2, REFINERY), 100),
        damaged(structure(&world, 3, PLANT), 100),
    ];
    let out = turn(&world, &mut state, &observation(&world, 100, 500, own));
    assert_eq!(repairs(&out), [3, 2]);
    // The dock before the rest, whatever the ids: tower 1, dock 2.
    let (_, mut state) = rig();
    let own = vec![
        structure(&world, 0, CENTRE),
        damaged(structure(&world, 1, TOWER), 100),
        damaged(structure(&world, 2, REFINERY), 100),
        damaged(structure(&world, 3, TOWER), 100),
    ];
    let out = turn(&world, &mut state, &observation(&world, 100, 500, own));
    assert_eq!(repairs(&out), [2, 1]);
}

#[test]
fn a_repair_is_not_repeated_while_it_runs_and_not_before_the_backoff_when_it_never_shows() {
    let (world, mut state) = rig();
    let blocked = u64::from(state.profile.economy.blocked_ticks);
    let mut own = repair_base(&world);
    own[1] = damaged(own[1].clone(), 100);
    let first = turn(
        &world,
        &mut state,
        &observation(&world, 100, 500, own.clone()),
    );
    assert_eq!(repairs(&first), [1]);
    assert_eq!(
        state.repair_backoff.get(&EntityId::new(1, 0)),
        Some(&Tick(100 + blocked))
    );
    // Not repairing, still damaged (the command was ignored): silent until the backoff ends.
    for t in [140, 180, 100 + blocked - 1] {
        let out = turn(
            &world,
            &mut state,
            &observation(&world, t, 500, own.clone()),
        );
        assert_eq!(repairs(&out), [], "tick {t}");
    }
    let out = turn(
        &world,
        &mut state,
        &observation(&world, 100 + blocked, 500, own.clone()),
    );
    assert_eq!(repairs(&out), [1]);
    // Seen repairing: the entry goes, and nothing is sent while it runs.
    let (_, mut state) = rig();
    turn(
        &world,
        &mut state,
        &observation(&world, 100, 500, own.clone()),
    );
    own[1].repairing = true;
    let out = turn(
        &world,
        &mut state,
        &observation(&world, 140, 500, own.clone()),
    );
    assert_eq!(repairs(&out), []);
    assert!(state.repair_backoff.is_empty());
    // Hurt again after it was repaired (the flag is clear): asked at once, no backoff left to wait for.
    own[1].repairing = false;
    let out = turn(&world, &mut state, &observation(&world, 180, 500, own));
    assert_eq!(repairs(&out), [1]);
    // A structure under construction is not repaired (the simulation refuses it).
    let (_, mut state) = rig();
    let own = vec![
        structure(&world, 0, CENTRE),
        going_up(damaged(structure(&world, 1, PLANT), 100)),
    ];
    let out = turn(&world, &mut state, &observation(&world, 100, 500, own));
    assert_eq!(repairs(&out), []);
}

#[test]
fn while_an_emergency_build_waits_for_money_only_the_anchor_is_repaired() {
    let (world, mut state) = rig();
    // No refinery stands and one is going up, with no credits to pay it: the money is the refinery's.
    let own = vec![
        damaged(structure(&world, 0, CENTRE), 100),
        damaged(structure(&world, 1, PLANT), 100),
        going_up(structure(&world, 2, REFINERY)),
    ];
    let obs = observation(&world, 100, 0, own.clone());
    assert!(economy::emergency_stalled(&obs, &Lens::new(&world, P0)));
    assert_eq!(repairs(&turn(&world, &mut state, &obs)), [0]);
    // With credits (free money) the plant follows.
    let (_, mut state) = rig();
    let obs = observation(&world, 100, 5000, own);
    assert_eq!(repairs(&turn(&world, &mut state, &obs)), [0, 1]);
}

// ---- the sweep's gaps: each condition of the economy turn where it decides -----------------------------

#[test]
fn an_emergency_is_stalled_only_with_no_free_credit_and_the_first_dock_or_a_short_power_plant_going_up()
 {
    let (world, _) = rig();
    let lens = Lens::new(&world, P0);
    let short = Some(PowerTotals { made: 0, used: 50 });
    // The first dock going up, nothing free: stalled. One credit free: not.
    let own = vec![
        structure(&world, 0, CENTRE),
        going_up(structure(&world, 2, REFINERY)),
    ];
    let mut obs = observation(&world, 100, 0, own);
    assert!(economy::emergency_stalled(&obs, &lens));
    // Free credits are the credits less what the going-up refinery still costs: exactly that is not free.
    let owed = sf_ai::budget::committed(&obs, &lens);
    assert!(owed > 0);
    obs.credits = owed;
    assert!(economy::emergency_stalled(&obs, &lens));
    obs.credits = owed + 1;
    assert!(!economy::emergency_stalled(&obs, &lens));
    // A second dock going up while one stands is no emergency (a mutation counting every dock fails here).
    let mut own = vec![
        structure(&world, 0, CENTRE),
        structure(&world, 2, REFINERY),
        going_up(structure(&world, 3, REFINERY)),
    ];
    let obs = observation(&world, 100, 0, own.clone());
    assert!(!economy::emergency_stalled(&obs, &lens));
    // A plant going up is an emergency only while less is made than used: equal is not short.
    own.pop();
    own.push(going_up(structure(&world, 3, PLANT)));
    let mut obs = observation(&world, 100, 0, own.clone());
    obs.power = short;
    assert!(economy::emergency_stalled(&obs, &lens));
    obs.power = Some(PowerTotals {
        made: 100,
        used: 100,
    });
    assert!(!economy::emergency_stalled(&obs, &lens));
    obs.power = Some(PowerTotals {
        made: 99,
        used: 100,
    });
    assert!(economy::emergency_stalled(&obs, &lens));
    // Anything else going up (a barracks) is none.
    own.pop();
    own.push(going_up(structure(&world, 3, BARRACKS)));
    let mut obs = observation(&world, 100, 0, own);
    obs.power = short;
    assert!(!economy::emergency_stalled(&obs, &lens));
}

#[test]
fn a_refinery_that_is_still_going_up_wants_no_harvester() {
    let (world, mut state) = rig();
    let own = vec![
        structure(&world, 0, CENTRE),
        going_up(structure(&world, 2, REFINERY)),
        structure(&world, 5, FACTORY),
    ];
    assert_eq!(
        turn(&world, &mut state, &observation(&world, 2000, 5000, own)),
        []
    );
}

#[test]
fn the_silo_is_picked_by_storage_function_price_and_faction() {
    let both = 3u8;
    let none = BuildingFunction::None;
    let dock = BuildingFunction::HarvesterDock;
    let table = |bit: Option<u8>| {
        economy::pick_silo(
            [
                (Kind(2), 2000, dock, Some(1400), both), // stores, but a dock
                (Kind(40), 0, none, Some(100), both),    // stores nothing
                (Kind(41), 900, none, None, both),       // cannot be bought
                (Kind(42), 900, none, Some(200), 2),     // the other faction's only
                (Kind(43), 1500, none, Some(250), both),
                (Kind(44), 1500, none, Some(250), both),
            ]
            .into_iter(),
            bit,
        )
    };
    assert_eq!(table(Some(1)), Some(Kind(43)));
    // The other faction takes the first of the table that fits it.
    assert_eq!(table(Some(2)), Some(Kind(42)));
    // No faction in the game: any, still not the dock, the free or the unbuyable one.
    assert_eq!(table(None), Some(Kind(42)));
    assert_eq!(economy::pick_silo(std::iter::empty(), Some(1)), None);
    // The shipped table: the ore silo, kind 43, for either faction.
    let (world, _) = rig();
    for player in [0, 1] {
        let lens = Lens::new(&world, PlayerId(player));
        let silo = economy::pick_silo(
            lens.buildings().map(|b| {
                (
                    b.kind,
                    lens.storage_of(b.kind),
                    b.function,
                    b.cost,
                    b.factions,
                )
            }),
            lens.faction_bit(),
        );
        assert_eq!(silo, Some(SILO), "player {player}");
    }
}

#[test]
fn a_unit_of_the_other_faction_has_no_factory_for_the_lens() {
    // Kind 16 (sniper) is Meridian's, 17 (medic) Veyran's; player 0 is Meridian, player 1 Veyran.
    let (world, _) = rig();
    let (meridian, veyran) = (Lens::new(&world, P0), Lens::new(&world, PlayerId(1)));
    assert_eq!(
        meridian.unit_factory(Kind(16)),
        Some(BuildingFunction::InfantryFactory)
    );
    assert_eq!(meridian.unit_factory(Kind(17)), None);
    assert_eq!(veyran.unit_factory(Kind(16)), None);
    assert_eq!(
        veyran.unit_factory(Kind(17)),
        Some(BuildingFunction::InfantryFactory)
    );
    // A kind no factory makes (a structure).
    assert_eq!(meridian.unit_factory(PLANT), None);
}

#[test]
fn no_harvester_is_ordered_on_a_map_without_ore_ground() {
    let noore = load("ai_noore");
    let f = go(&noore, 6000);
    assert!(!Lens::new(&f.world, P0).has_ore_ground());
    // The vehicle factory stands (tick 1998) and a harvester is wanted, but none is ordered.
    assert!(
        f.world
            .snapshot()
            .iter()
            .any(|v| v.owner == P0 && v.kind == FACTORY && f.world.stands(v.id))
    );
    assert_eq!(sent(&f, is_harvester_order), Vec::<u64>::new());
    // Control: the map of `ai_economy` has ore ground, and there the order is sent.
    let (world, _) = rig();
    assert!(Lens::new(&world, P0).has_ore_ground());
    let f = go(&load("ai_economy"), 2100);
    assert_eq!(sent(&f, is_harvester_order), [2000]);
}

/// One construction turn: the commands it sends.
fn build_turn(world: &World, state: &mut sf_ai::AiState, obs: &Observation) -> Vec<Command> {
    let mut lens = Lens::new(world, P0);
    let mut out = Vec::new();
    sf_ai::base::turn(state, obs, &mut lens, &mut out);
    out
}

fn kinds_built(out: &[Command]) -> Vec<Kind> {
    out.iter()
        .filter_map(|c| match c {
            Command::BuildStructure { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect()
}

#[test]
fn the_construction_turn_builds_the_silo_the_economy_asked_for_before_the_goal_list() {
    // The real base of `ai_economy` at tick 2100 (its four goals stand, so the simulation's own placement
    // rule accepts a silo), the AI's state with the latches cleared and 5,000 credits.
    let f = go(&load("ai_economy"), 2100);
    let world = &*f.world;
    let mut state = f.ais[0].state().clone();
    state.pending_build = None;
    state.blocked_until.clear();
    let mut obs = Observation::build(world, P0, &state.known_structures);
    obs.credits = 5000;
    let now = obs.tick;
    let closed = |s: &mut sf_ai::AiState| s.profile.build_order = vec![(PLANT, 1), (REFINERY, 1)];
    let open = |s: &mut sf_ai::AiState| {
        s.profile.build_order = vec![(PLANT, 1), (REFINERY, 1), (TOWER, 1)]
    };
    // The wish and a closed list: the silo. No wish: nothing to build.
    let mut s = state.clone();
    closed(&mut s);
    s.emergency = Some(SILO);
    assert_eq!(kinds_built(&build_turn(world, &mut s, &obs)), [SILO]);
    let mut s = state.clone();
    closed(&mut s);
    assert_eq!(build_turn(world, &mut s, &obs), []);
    // The wish with a goal open (a tower): the silo still comes first; without the wish the tower does.
    let mut s = state.clone();
    open(&mut s);
    s.emergency = Some(SILO);
    assert_eq!(kinds_built(&build_turn(world, &mut s, &obs)), [SILO]);
    let mut s = state.clone();
    open(&mut s);
    assert_eq!(kinds_built(&build_turn(world, &mut s, &obs)), [TOWER]);
    // A blocked silo is not tried (the open goal goes on instead), and it is tried again when the block ends.
    let mut s = state.clone();
    open(&mut s);
    s.emergency = Some(SILO);
    s.blocked_until.insert(SILO, Tick(now.0 + 1));
    assert_eq!(kinds_built(&build_turn(world, &mut s, &obs)), [TOWER]);
    let mut s = state.clone();
    open(&mut s);
    s.emergency = Some(SILO);
    s.blocked_until.insert(SILO, now);
    assert_eq!(kinds_built(&build_turn(world, &mut s, &obs)), [SILO]);
    // Its prerequisite does not stand (the observation shows no refinery): not built.
    let mut s = state.clone();
    s.profile.build_order = vec![(PLANT, 1)];
    s.emergency = Some(SILO);
    let mut no_refinery = obs.clone();
    no_refinery.own.retain(|o| o.kind != REFINERY);
    assert_eq!(build_turn(world, &mut s, &no_refinery), []);
}

#[test]
fn the_harvester_rule_adds_ore_the_scenario_would_not_deliver_without_it() {
    // The same scenario with `harvesters_per_refinery` 1: the free harvester is all that is wanted, so none is
    // bought until the one removed at tick 8001 has to be replaced (at 8040).
    let s = load("ai_economy");
    let with = go(&s, s.ticks).world.ore_delivered(P0);
    let mut one = s.clone();
    let mut profile = one.ais[0].state().profile.clone();
    profile.economy.harvesters_per_refinery = 1;
    one.ais[0] = sf_ai::Ai::new(
        P0,
        profile,
        0xA1_0001,
        one.ais[0].state().start_sites.clone(),
    );
    let f = go(&one, one.ticks);
    // Only the replacement of the one removed at tick 8001 (one is wanted, none is left): nothing bought before.
    assert_eq!(sent(&f, is_harvester_order), [8040]);
    let without = f.world.ore_delivered(P0);
    println!("ore delivered in 12,000 ticks: with the harvester rule {with}, without it {without}");
    assert!(with >= without + 1500, "{with} against {without}");
    assert!(with >= 5000);
}

// ---- the rescue: selling structures out of a money deadlock, once per match --------------------------

/// The world of `ai_rebuild` (it has repair and sale rules) after one tick, and the AI's state.
fn rig_rebuild() -> (World, sf_ai::AiState) {
    let f = go(&load("ai_rebuild"), 1);
    (*f.world, f.ais[0].state().clone())
}

const RADAR: Kind = Kind(5);

/// A deadlocked base: the centre (id 0), a power plant (1), a barracks (2), a vehicle factory (3), a radar (4)
/// and a refinery going up (5) with nothing to pay it with and no harvester.
fn deadlock(world: &World) -> Vec<Own> {
    vec![
        structure(world, 0, CENTRE),
        structure(world, 1, PLANT),
        structure(world, 2, BARRACKS),
        structure(world, 3, FACTORY),
        structure(world, 4, RADAR),
        going_up(structure(world, 5, REFINERY)),
    ]
}

fn sells(out: &[Command]) -> Vec<u32> {
    out.iter()
        .filter_map(|c| match c {
            Command::Sell { structure } => Some(structure.index()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_sale_refunds_are_the_simulations_at_the_current_hit_points() {
    let (world, _) = rig_rebuild();
    let lens = Lens::new(&world, P0);
    // The shipped prices at 500 thousandths: the vehicle factory 750, the radar 450, the barracks 200.
    assert_eq!(lens.sale_refund(FACTORY, 1000, 1000), Some(750));
    assert_eq!(lens.sale_refund(RADAR, 700, 700), Some(450));
    assert_eq!(lens.sale_refund(BARRACKS, 700, 700), Some(200));
    // The current hit points count: a factory at a tenth of its hit points pays a tenth.
    assert_eq!(lens.sale_refund(FACTORY, 100, 1000), Some(75));
    // A world without sale rules (`ai_economy`) answers nothing.
    let (plain, _) = rig();
    assert_eq!(Lens::new(&plain, P0).sale_refund(FACTORY, 1000, 1000), None);
}

#[test]
fn the_cheapest_sufficient_sale_wins_and_a_sum_is_taken_only_when_none_suffices() {
    let id = |i| EntityId::new(i, 0);
    let list = vec![(750, 3, id(3)), (450, 4, id(4)), (200, 2, id(2))];
    // One suffices: the smallest refund that covers it.
    assert_eq!(economy::pick_sales(list.clone(), 150), [id(2)]);
    assert_eq!(economy::pick_sales(list.clone(), 200), [id(2)]);
    assert_eq!(economy::pick_sales(list.clone(), 201), [id(4)]);
    assert_eq!(economy::pick_sales(list.clone(), 451), [id(3)]);
    assert_eq!(economy::pick_sales(list.clone(), 750), [id(3)]);
    // None suffices alone: the smallest first until the sum covers it.
    assert_eq!(
        economy::pick_sales(list.clone(), 751),
        [id(2), id(4), id(3)]
    );
    assert_eq!(economy::pick_sales(list.clone(), 650), [id(3)]);
    assert_eq!(
        economy::pick_sales(list.clone(), 1400),
        [id(2), id(4), id(3)]
    );
    // All together fall short: nothing.
    assert_eq!(economy::pick_sales(list.clone(), 1401), []);
    assert_eq!(economy::pick_sales(Vec::new(), 1), []);
    // Ties: the later goal entry first, then the lower id.
    let tie = vec![(450, 3, id(8)), (450, 5, id(9)), (450, 5, id(7))];
    assert_eq!(economy::pick_sales(tie, 400), [id(7)]);
}

#[test]
fn nothing_is_sold_on_the_first_stalled_turn_and_the_sales_go_out_together_on_the_second() {
    let (world, mut state) = rig_rebuild();
    let obs = observation(&world, 5000, 0, deadlock(&world));
    assert_eq!(sells(&turn(&world, &mut state, &obs)), []);
    assert_eq!(state.stalled_turns, 1);
    let obs = observation(&world, 5040, 0, deadlock(&world));
    // The refinery owes half of its price, 600: more than the radar or the barracks pay, less than the factory:
    // the one smallest sufficient sale. (The numbers: owed 600 of 1200, refunds 750, 450, 200.)
    let out = turn(&world, &mut state, &obs);
    let owed = sf_ai::budget::committed(&obs, &Lens::new(&world, P0));
    println!("owed {owed}, sells {:?}", sells(&out));
    assert_eq!(state.stalled_turns, 2);
    assert_eq!(sells(&out), [3]);
    assert_eq!(state.sales, [(EntityId::new(3, 0), Tick(5040))]);
    assert!(!state.rescue_used);
}

#[test]
fn when_no_one_structure_pays_enough_the_smallest_are_added_until_the_sum_does() {
    let (world, mut state) = rig_rebuild();
    // The refinery has barely started: it still owes nearly its whole price, 1,188 of 1,200.
    let mut own = deadlock(&world);
    own[5].building = Some(Construction {
        left: 990,
        total: 1000,
    });
    for t in [5000, 5040] {
        let out = turn(&world, &mut state, &observation(&world, t, 0, own.clone()));
        if t == 5040 {
            assert_eq!(sells(&out), [2, 4, 3]);
        } else {
            assert_eq!(sells(&out), []);
        }
    }
}

#[test]
fn nothing_is_sold_when_all_the_eligible_structures_together_fall_short() {
    let (world, mut state) = rig_rebuild();
    // No factory and no radar: only the barracks (200) can be sold, and the refinery owes far more.
    let own = vec![
        structure(&world, 0, CENTRE),
        structure(&world, 1, PLANT),
        structure(&world, 2, BARRACKS),
        going_up(structure(&world, 5, REFINERY)),
    ];
    for t in [5000, 5040, 5080, 5120] {
        let out = turn(&world, &mut state, &observation(&world, t, 0, own.clone()));
        assert_eq!(sells(&out), [], "tick {t}");
    }
    assert!(!state.rescue_used);
    assert!(state.stalled_turns >= 4);
}

#[test]
fn the_anchor_a_dock_a_power_plant_and_the_stalled_structure_are_never_sold() {
    // By function, for every function there is: only the anchor, the dock and the plants are kept.
    for (function, sold) in [
        (BuildingFunction::None, true),
        (BuildingFunction::Anchor, false),
        (BuildingFunction::PowerPlant, false),
        (BuildingFunction::HarvesterDock, false),
        (BuildingFunction::InfantryFactory, true),
        (BuildingFunction::VehicleFactory, true),
        (BuildingFunction::VesselFactory, true),
        (BuildingFunction::Radar, true),
        (BuildingFunction::Defence, true),
        (BuildingFunction::Decoy, true),
    ] {
        assert_eq!(economy::sellable(function), sold, "{function:?}");
    }
    // In a deadlock where every single sale falls short, the smallest are added: the barracks, the radar and
    // the factory, and never the two power plants (150 each, which would come first) or the centre.
    let (world, mut state) = rig_rebuild();
    let mut own = deadlock(&world);
    own[5].building = Some(Construction {
        left: 990,
        total: 1000,
    });
    own.push(structure(&world, 6, PLANT));
    own.sort_by_key(|o| o.id);
    let mut sold = Vec::new();
    for t in [5000, 5040] {
        sold = sells(&turn(
            &world,
            &mut state,
            &observation(&world, t, 0, own.clone()),
        ));
    }
    assert_eq!(sold, [2, 4, 3]);
    // A refinery that stands (a dock) is not sold either, although its 600 would cover a small debt that the
    // barracks cannot: a plant going up for lack of power, no harvester left, so no income comes.
    let (_, mut state) = rig_rebuild();
    let mut own = vec![
        structure(&world, 0, CENTRE),
        structure(&world, 2, BARRACKS),
        structure(&world, 7, REFINERY),
        going_up(structure(&world, 5, PLANT)),
    ];
    own[3].building = Some(Construction {
        left: 990,
        total: 1000,
    });
    own.sort_by_key(|o| o.id);
    for t in [5000, 5040, 5080] {
        let mut obs = observation(&world, t, 0, own.clone());
        obs.power = Some(PowerTotals { made: 0, used: 50 });
        assert_eq!(sells(&turn(&world, &mut state, &obs)), [], "tick {t}");
    }
    assert!(!state.rescue_used);
}

#[test]
fn a_standing_dock_with_a_harvester_means_income_and_no_sale() {
    let (world, mut state) = rig_rebuild();
    // A power plant going up while power is short, nothing free: stalled. But a refinery stands and a
    // harvester mines: income can arrive, so nothing is sold, however long it lasts.
    let mut own = vec![
        structure(&world, 0, CENTRE),
        structure(&world, 2, BARRACKS),
        structure(&world, 3, FACTORY),
        structure(&world, 4, RADAR),
        structure(&world, 6, REFINERY),
        going_up(structure(&world, 5, PLANT)),
        unit(10, HARVESTER),
    ];
    own.sort_by_key(|o| o.id);
    for t in [5000, 5040, 5080, 5120] {
        let mut obs = observation(&world, t, 0, own.clone());
        obs.power = Some(PowerTotals { made: 0, used: 50 });
        assert_eq!(sells(&turn(&world, &mut state, &obs)), [], "tick {t}");
    }
    assert!(state.stalled_turns >= 4);
    // Without the harvester there is none coming: the rescue goes.
    let mut state = rig_rebuild().1;
    own.retain(|o| o.footprint.is_some());
    let mut last = Vec::new();
    for t in [5000, 5040] {
        let mut obs = observation(&world, t, 0, own.clone());
        obs.power = Some(PowerTotals { made: 0, used: 50 });
        last = sells(&turn(&world, &mut state, &obs));
    }
    assert!(!last.is_empty());
}

#[test]
fn a_refused_sale_does_not_spend_the_rescue_and_a_started_one_does() {
    let (world, mut state) = rig_rebuild();
    let blocked = u64::from(state.profile.economy.blocked_ticks);
    let own = deadlock(&world);
    turn(
        &world,
        &mut state,
        &observation(&world, 5000, 0, own.clone()),
    );
    assert_eq!(
        sells(&turn(
            &world,
            &mut state,
            &observation(&world, 5040, 0, own.clone())
        )),
        [3]
    );
    // The next turn: the factory is neither selling nor gone, so the simulation refused: not spent, and
    // quiet until `blocked_ticks` have passed.
    let out = turn(
        &world,
        &mut state,
        &observation(&world, 5080, 0, own.clone()),
    );
    assert_eq!(sells(&out), []);
    assert!(!state.rescue_used);
    assert_eq!(state.rescue_after, Tick(5080 + blocked));
    assert!(state.sales.is_empty());
    let out = turn(
        &world,
        &mut state,
        &observation(&world, 5080 + blocked - 40, 0, own.clone()),
    );
    assert_eq!(sells(&out), []);
    // After it, it tries again, and this time the factory shows `selling`: spent.
    let out = turn(
        &world,
        &mut state,
        &observation(&world, 5080 + blocked, 0, own.clone()),
    );
    assert_eq!(sells(&out), [3]);
    let mut selling = own.clone();
    selling[3].selling = true;
    let out = turn(
        &world,
        &mut state,
        &observation(&world, 5080 + blocked + 40, 0, selling),
    );
    assert_eq!(sells(&out), []);
    assert!(state.rescue_used);
    assert!(state.sales.is_empty());
    // Spent: a later deadlock sells nothing, however long.
    for k in 2..8 {
        let t = 5080 + blocked + 40 * k;
        assert_eq!(
            sells(&turn(
                &world,
                &mut state,
                &observation(&world, t, 0, own.clone())
            )),
            []
        );
    }
    // A structure that is gone counts as started too.
    let mut state = rig_rebuild().1;
    turn(
        &world,
        &mut state,
        &observation(&world, 5000, 0, own.clone()),
    );
    turn(
        &world,
        &mut state,
        &observation(&world, 5040, 0, own.clone()),
    );
    let mut gone = own;
    gone.remove(3);
    turn(&world, &mut state, &observation(&world, 5080, 0, gone));
    assert!(state.rescue_used);
}

#[test]
fn a_structure_being_sold_is_not_counted_as_standing_by_the_construction_turn() {
    // The base of `ai_economy` at tick 2100 with its goal list closed: nothing to build. With the barracks
    // shown as being sold the goal "a barracks" is open again (and, as it needs a standing power plant, it
    // is built).
    let f = go(&load("ai_economy"), 2100);
    let world = &*f.world;
    let mut state = f.ais[0].state().clone();
    state.pending_build = None;
    state.blocked_until.clear();
    let mut obs = Observation::build(world, P0, &state.known_structures);
    obs.credits = 5000;
    assert_eq!(build_turn(world, &mut state.clone(), &obs), []);
    for o in obs.own.iter_mut().filter(|o| o.kind == BARRACKS) {
        o.selling = true;
    }
    assert_eq!(
        kinds_built(&build_turn(world, &mut state, &obs)),
        [BARRACKS]
    );
}

// ---- the scenario `ai_deadlock` --------------------------------------------------------------------

const DEADLOCK_EXPECTED: &str = include_str!("../../../assets/scenarios/ai_deadlock.expected");
/// `Ai::digest` of player 0 at the end of `ai_deadlock`.
const DEADLOCK_DIGEST: u64 = 0x2613_96ec_364c_3dc4;

#[test]
fn the_deadlock_scenario_is_pinned() {
    let s = load("ai_deadlock");
    let outcome = run(&s, &RunOptions::for_scenario(&s)).unwrap();
    assert_eq!(
        outcome.final_checksum,
        parse_checksum(DEADLOCK_EXPECTED).unwrap()
    );
    assert_eq!(outcome.ai_digests, vec![(0, DEADLOCK_DIGEST)]);
    assert_eq!(outcome, run(&s, &RunOptions::for_scenario(&s)).unwrap());
}

/// T for the deadlock, measured: the refinery lost at tick 3601 is ordered at 3610, the sales go out at the economy
/// turn of tick 3680 and it stands again at tick 4314 (T = 713); 20 percent more.
const T_DEADLOCK: u64 = 860;

#[test]
fn a_money_deadlock_is_rescued_once_by_selling_what_covers_the_refinery() {
    let s = load("ai_deadlock");
    let mut stood: BTreeMap<EntityId, (Kind, u64)> = BTreeMap::new();
    let mut delivered = BTreeMap::new();
    let f = run_watched(&s, &RunOptions::for_scenario(&s), |w| {
        for v in w.snapshot().iter().filter(|v| v.owner == P0) {
            if w.footprint_of(v.id).is_some() && w.stands(v.id) {
                stood.entry(v.id).or_insert((v.kind, w.tick().0));
            }
        }
        delivered.insert(w.tick().0, w.ore_delivered(P0));
    })
    .unwrap();
    let f_cmds = go(&s, s.ticks);
    // The credits are at (about) zero when the refinery and the harvesters go.
    let credits_at = |t: u64| {
        let mut c = 0;
        run_watched(
            &s,
            &RunOptions {
                ticks: t,
                every: 100,
            },
            |w| c = w.credits(P0),
        )
        .unwrap();
        c
    };
    assert!(credits_at(3601) <= 10);
    // All the sales go out in one economy turn: the barracks, the radar and the vehicle factory.
    let sales: Vec<(u64, EntityId)> = f_cmds
        .ai_commands
        .iter()
        .filter_map(|(t, _, c)| match c {
            Command::Sell { structure } => Some((*t, *structure)),
            _ => None,
        })
        .collect();
    println!("the sales: {sales:?}");
    assert_eq!(
        sales,
        [
            (3680, EntityId::new(5, 0)),
            (3680, EntityId::new(8, 0)),
            (3680, EntityId::new(7, 0)),
        ]
    );
    // The refinery stands again after T; ore is delivered afterwards.
    let again = stood
        .values()
        .find(|&&(k, t)| k == REFINERY && t > 3601)
        .map(|&(_, t)| t)
        .unwrap();
    println!(
        "the refinery lost at 3601 is ordered at 3610, the sales are sent at 3680, it stands again at {again} (T = {})",
        again - 3601
    );
    assert!(again <= 3601 + T_DEADLOCK, "again at {again}");
    assert!(
        delivered[&8600] > delivered[&again],
        "{:?}",
        (delivered[&8600], delivered[&again])
    );
    // The second deadlock (the loss at 8601) sells nothing and the refinery is not rebuilt: it was ordered
    // again at 8610, paid what the credits held and stands still.
    let late = f_cmds
        .ai_commands
        .iter()
        .filter(|(t, ..)| *t > 8601)
        .map(|(t, _, c)| {
            (
                *t,
                matches!(c, Command::BuildStructure { kind, .. } if *kind == REFINERY),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(late, [(8610, true)]);
    assert!(!stood.values().any(|&(k, t)| k == REFINERY && t > 8601));
    let state = f.ais[0].state();
    assert!(state.rescue_used);
    assert!(state.sales.is_empty());
    assert_eq!(f.outcome.commands_ignored, 0);
}
