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
    Command, Construction, EntityId, FactoryStatus, Footprint, Kind, Order, PlayerId, PowerTotals,
    Tick, World,
};

const P0: PlayerId = PlayerId(0);

const ECONOMY_EXPECTED: &str = include_str!("../../../assets/scenarios/ai_economy.expected");
const REBUILD_EXPECTED: &str = include_str!("../../../assets/scenarios/ai_rebuild.expected");
/// `Ai::digest` of player 0 at the end of each scenario.
const ECONOMY_DIGEST: u64 = 0xb244_e3ea_8dd7_26ad;
const REBUILD_DIGEST: u64 = 0xc541_cd51_b2e4_5292;

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
    let f = run_watched(&s, &RunOptions::for_scenario(&s), |w| {
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
    let _ = f;
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
    // Two harvesters and a harvester queued and a second wanted queued too: still one in flight at most.
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
