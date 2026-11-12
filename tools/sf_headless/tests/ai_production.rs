//! The AI's production manager, scout and gathering (WP9.1c-1, `docs/rules/ai.md`, "Production" and "The
//! army in WP9.1c-1"). The scenario `ai_scout` with its checksum and AI digest pinned and the rules
//! seen from outside (the first unit is the scout, the scout visits the other player's start site once and
//! finds its construction centre, units are bought by the mix, every factory that makes no harvester rallies to the gather cell, the
//! command set and rate), then each rule where it decides: the production or army turn called on an
//! observation that is the world's own with the entities the test needs put in (the way `ai_economy.rs`
//! does), so every test runs the production code (`sf_ai::production::turn`, `army::turn`, `Lens`, the
//! runner's own driver).
//!
//! Player 0 is the AI (Meridian, a construction centre at (12, 68)) on the first match's map, player 1 a
//! static construction centre with a power plant; `ai_scout` runs on its own frozen copy of the shipped
//! definitions and profile (`ai_scout_defs`).

mod common;

use std::collections::BTreeMap;

use common::scenario_path;
use sf_ai::army::{self, ARMY_COMMANDS_PER_TURN, GATHER_COMMANDS_PER_TURN};
use sf_ai::production;
use sf_ai::{Ai, AiState, Lens, Observation, Own, OwnFactory};
use sf_headless::cli::parse_checksum;
use sf_headless::run::{Finished, RunOptions, run, run_full, run_observed};
use sf_headless::scenario::{Action, IdSpec, Scenario, TimedCommand};
use sf_math::{Fx, Vec2};
use sf_sim::{
    Cell, Command, EntityId, FactoryStatus, Kind, Order, PlayerId, Tick, Visibility, World,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const SCOUT_EXPECTED: &str = include_str!("../../../assets/scenarios/ai_scout.expected");
/// `Ai::digest` of player 0 at the end of `ai_scout`. Re-pinned by WP9.1c-2 with the checksum (was
/// `0xf2be53002209e2f6`, digest `0x584e_81c7_938e_7859`): the army now attacks; measured by
/// `the_ai_scout_repin_of_wp9_1c_2_is_the_attack`.
const SCOUT_DIGEST: u64 = 0x3c88_41f5_b721_2ab8;

/// The kinds of the frozen definitions (`ai_scout_defs/unit_defs.ron`).
const CENTRE: Kind = Kind(0);
const PLANT: Kind = Kind(1);
const REFINERY: Kind = Kind(2);
const BARRACKS: Kind = Kind(3);
const FACTORY: Kind = Kind(4);
const HARVESTER: Kind = Kind(11);
const RIFLEMAN: Kind = Kind(12);
const ROCKET: Kind = Kind(13);
const SCOUT: Kind = Kind(15);
const TANK: Kind = Kind(19);

fn load(name: &str) -> Scenario {
    Scenario::load(&scenario_path(&format!("{name}.ron"))).unwrap()
}

fn go(s: &Scenario, ticks: u64) -> Finished {
    run_full(s, &RunOptions { ticks, every: 100 }).unwrap()
}

/// The ticks at which each command of the AI that matches `pick` was sent.
fn sent(f: &Finished, pick: impl Fn(&Command) -> bool) -> Vec<u64> {
    f.ai_commands
        .iter()
        .filter(|(_, _, c)| pick(c))
        .map(|(t, _, _)| *t)
        .collect()
}

fn is_queue_of(c: &Command, kind: Kind) -> bool {
    matches!(c, Command::QueueUnit { kind: k, .. } if *k == kind)
}

/// The middle of a cell, as the AI writes it into an order.
fn middle(x: u32, y: u32) -> Vec2 {
    let half = Fx::from_ratio(1, 2);
    Vec2::new(
        Fx::from_i64(i64::from(x)) + half,
        Fx::from_i64(i64::from(y)) + half,
    )
}

fn move_order(ids: &[u32], x: u32, y: u32) -> Command {
    Command::Order {
        ids: ids.iter().map(|&i| EntityId::new(i, 0)).collect(),
        order: Order::Move(middle(x, y)),
        queued: false,
    }
}

/// The scout of the first run: what the AI knew and when, tick by tick.
struct Story {
    finished: Finished,
    /// The first tick the AI's memory holds player 1's construction centre.
    anchor_known: Option<u64>,
    /// The first tick the scout stood within 3 cells of the start site of player 1, (72, 14).
    arrived: Option<u64>,
}

fn story(s: &Scenario, ticks: u64) -> Story {
    let (mut anchor_known, mut arrived) = (None, None);
    let finished = run_observed(s, &RunOptions { ticks, every: 100 }, |w, ais| {
        let t = w.tick().0;
        let state = ais[0].state();
        if anchor_known.is_none()
            && state
                .known_structures
                .values()
                .any(|k| k.owner == P1 && k.kind == CENTRE)
        {
            anchor_known = Some(t);
        }
        if arrived.is_none()
            && let Some(id) = state.scout
            && let Some(v) = w.snapshot().iter().find(|v| v.id == id)
            && let Some(c) = w.cell_at(v.pos)
            && c.x.abs_diff(72).max(c.y.abs_diff(14)) <= 3
        {
            arrived = Some(t);
        }
    })
    .unwrap();
    Story {
        finished,
        anchor_known,
        arrived,
    }
}

/// `s` with the AI's profile changed by `change`.
fn with_profile(mut s: Scenario, change: impl Fn(&mut sf_ai::Profile)) -> Scenario {
    let old = s.ais[0].state().clone();
    let mut profile = old.profile.clone();
    change(&mut profile);
    s.ais[0] = Ai::new(old.player, profile, 0xA1_0001, old.start_sites);
    s
}

// ---- the scenario ----------------------------------------------------------------------------------

#[test]
fn the_checksum_and_the_ai_digest_of_ai_scout_are_the_pinned_values() {
    let s = load("ai_scout");
    let outcome = run(&s, &RunOptions::for_scenario(&s)).unwrap();
    assert_eq!(
        outcome.final_checksum,
        parse_checksum(SCOUT_EXPECTED).unwrap()
    );
    assert_eq!(outcome.ai_digests, vec![(0, SCOUT_DIGEST)]);
    assert_eq!(outcome, run(&s, &RunOptions::for_scenario(&s)).unwrap());
}

#[test]
fn the_first_unit_is_the_scout_it_visits_each_leg_once_and_finds_the_centre() {
    let s = load("ai_scout");
    let st = story(&s, s.ticks);
    let f = &st.finished;
    // The first unit queued is the scout, on the barracks (id 6), at the first production turn after the
    // barracks stands (the profile's `scout_tick` is 600; the barracks is not up before).
    let first = f
        .ai_commands
        .iter()
        .find(|(_, _, c)| matches!(c, Command::QueueUnit { kind, .. } if *kind != HARVESTER))
        .unwrap();
    assert_eq!(
        (first.0, &first.2),
        (
            FIRST_UNIT_TICK,
            &Command::QueueUnit {
                factory: EntityId::new(6, 0),
                kind: SCOUT
            }
        )
    );
    assert!(first.0 >= 600);
    assert_eq!(sent(f, |c| is_queue_of(c, SCOUT)), [FIRST_UNIT_TICK]);
    // The scout (id 8) is ordered once to each leg, in this order, and never again after the last.
    let legs: Vec<(u64, Command)> = f
        .ai_commands
        .iter()
        .filter(
            |(_, _, c)| matches!(c, Command::Order { ids, .. } if ids == &[EntityId::new(8, 0)]),
        )
        .map(|(t, _, c)| (*t, c.clone()))
        .collect();
    let want = [
        (72, 14, SCOUT_LEG_TICKS[0]),
        (24, 24, SCOUT_LEG_TICKS[1]),
        (72, 24, SCOUT_LEG_TICKS[2]),
        (24, 72, SCOUT_LEG_TICKS[3]),
        (72, 72, SCOUT_LEG_TICKS[4]),
    ];
    let want: Vec<(u64, Command)> = want
        .iter()
        .map(|&(x, y, t)| (t, move_order(&[8], x, y)))
        .collect();
    assert_eq!(legs, want);
    // It sees the centre and the AI remembers it; the tick it stood next to the site is a little later.
    println!(
        "scout ordered at {FIRST_UNIT_TICK} (queued), first leg at {}, next to the site at {:?}, centre known at {:?}",
        SCOUT_LEG_TICKS[0], st.arrived, st.anchor_known
    );
    assert_eq!(
        (st.anchor_known, st.arrived),
        (Some(ANCHOR_KNOWN), Some(ARRIVED))
    );
    assert!(st.anchor_known <= st.arrived);
    let known = f.ais[0]
        .state()
        .known_structures
        .values()
        .find(|k| k.owner == P1 && k.kind == CENTRE);
    let k = known.expect("the centre of player 1 is in the memory");
    assert_eq!((k.anchor, k.width, k.height), (Cell::new(72, 14), 4, 4));
    assert!(k.seen_tick.0 >= ANCHOR_KNOWN);
}

/// Measured (release): the scout is queued at tick 1100, comes out and is sent to the first leg at 1270; the legs follow when it is within 3 cells of the one before (the
/// first at tick 2439) and when the army turn after that finds it.
const FIRST_UNIT_TICK: u64 = 1100;
const SCOUT_LEG_TICKS: [u64; 5] = [1270, 2470, 3190, 3870, 4870];
/// The AI's memory holds the centre from this tick on (the scout's sight is 8 cells).
const ANCHOR_KNOWN: u64 = 2351;
const ARRIVED: u64 = 2439;
/// The replacement of a scout lost at tick 3000.
const REPLACEMENT_TICK: u64 = 7980;

#[test]
fn without_a_scout_the_ai_never_learns_where_the_centre_stands_before_the_first_attack() {
    // `scouts_max` 0: no scout is bought. Nothing leaves the base before the first attack (the army gathers),
    // so the memory is empty until the attack arrives (WP9.1c-2: the attack marches on the start site and sees
    // the centre there, measured at tick 15,781); with the scout it holds the centre from tick 2351.
    let s = with_profile(load("ai_scout"), |p| p.army.scouts_max = 0);
    let st = story(&s, s.ticks);
    let attack = sent(&st.finished, |c| {
        matches!(
            c,
            Command::Order {
                order: Order::AttackMove(_),
                ..
            }
        )
    });
    let first_attack = *attack.first().expect("an attack goes out");
    println!(
        "without a scout: first attack at {first_attack}, the centre known from {:?}",
        st.anchor_known
    );
    let known = st.anchor_known.expect("the attack finds the centre");
    assert!(known > first_attack, "{known} {first_attack}");
    assert_eq!(known, 15_781);
    assert_eq!(sent(&st.finished, |c| is_queue_of(c, SCOUT)), []);
    // Before the attack the only orders are gather orders (to the gather cell), none to a leg.
    assert!(
        st.finished
            .ai_commands
            .iter()
            .filter(|(t, _, _)| *t < first_attack)
            .all(|(_, _, c)| !matches!(c, Command::Order { order: Order::Move(v), .. } if *v != middle(24, 64)))
    );
    // The first unit is then an army unit; and the scenario with the scout does know (control).
    assert!(sent(&st.finished, |c| is_queue_of(c, RIFLEMAN)).len() > 1);
}

#[test]
fn a_lost_scout_is_replaced_after_scout_retry_and_at_most_scouts_max_times() {
    // The first scout (id 8) is lost at tick 3000; the AI sees it at its next army turn (3010 is one: thinks
    // 3, 7, ... of the rotation; the test only needs the order of events), and buys the replacement `scout_retry`
    // (3000) ticks after that. The replacement is lost too; with `scouts_max` 2 there is no third.
    let mut s = load("ai_scout");
    let lost = |tick, (index, generation)| TimedCommand {
        tick,
        player: 1,
        command: Action::Despawn {
            ids: vec![IdSpec::Id(index, generation)],
        },
    };
    s.commands.push(lost(3000, (8, 0)));
    let f = go(&s, 12000);
    let bought = sent(&f, |c| is_queue_of(c, SCOUT));
    println!("scouts queued at {bought:?}");
    assert_eq!(bought.len(), 2, "{bought:?}");
    assert_eq!(bought[0], FIRST_UNIT_TICK);
    // Not before `scout_retry` (3,000) ticks after the loss at 3,000; the money (700 free: the goal list is
    // still spending everything it earns) decides the rest: measured 7980.
    assert_eq!(bought[1], REPLACEMENT_TICK, "{bought:?}");
    assert!(bought[1] >= 3000 + 3000);
    assert_eq!(f.ais[0].state().scouts_sent, 2);
    // The replacement is sent along the legs from the start (leg 0 again): its first order goes to the site.
    let orders: Vec<&Command> = f
        .ai_commands
        .iter()
        .filter(|(t, _, c)| *t > bought[1] && matches!(c, Command::Order { .. }))
        .map(|(_, _, c)| c)
        .collect();
    let new_id = f_scout_id(&f);
    assert_eq!(
        orders[0].clone(),
        Command::Order {
            ids: vec![EntityId::new(new_id.0, new_id.1)],
            order: Order::Move(middle(72, 14)),
            queued: false
        }
    );
    // The second is lost as well, once it has been seen alive: nothing is bought again.
    let mut s2 = load("ai_scout");
    s2.commands.push(lost(3000, (8, 0)));
    s2.commands.push(lost(bought[1] + 500, new_id));
    let f2 = go(&s2, 20000);
    assert_eq!(sent(&f2, |c| is_queue_of(c, SCOUT)).len(), 2);
    assert_eq!(f2.ais[0].state().scouts_sent, 2);
    assert_eq!(f2.ais[0].state().scout, None);
}

/// The `(index, generation)` of the scout the AI holds at the end of `f`.
fn f_scout_id(f: &Finished) -> (u32, u32) {
    let id = f.ais[0].state().scout.unwrap();
    (id.index(), id.generation())
}

// ---- the rig: one production or army turn on an observation the test builds ---------------------------------

/// The world of `ai_scout` after one tick and the AI's state.
fn rig() -> (World, AiState) {
    let f = go(&load("ai_scout"), 1);
    (*f.world, f.ais[0].state().clone())
}

/// A structure of player 0 for the observation: complete and undamaged.
fn structure_at(world: &World, id: u32, kind: Kind, x: u32, y: u32) -> Own {
    Own {
        id: EntityId::new(id, 0),
        kind,
        cell: Cell::new(x, y),
        footprint: world.building_table().get(kind).map(|b| b.footprint),
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

/// A unit of player 0, idle: `armed` and mobile as the shipped definitions have them.
fn unit_at(id: u32, kind: Kind, armed: bool, x: u32, y: u32) -> Own {
    Own {
        id: EntityId::new(id, 0),
        kind,
        cell: Cell::new(x, y),
        footprint: None,
        hp: 100,
        order: Order::Idle,
        standing: true,
        max_hp: 100,
        building: None,
        repairing: false,
        factory: None,
        selling: false,
        armed,
        mobile: true,
    }
}

/// The gather cell of the base at (12, 68): the factories of the rigs have their rally point there already, so
/// that a test sees only the orders it is about.
fn gather() -> Cell {
    Cell::new(24, 64)
}

/// A factory of player 0 that stands, with an empty queue and its rally point on the gather cell.
fn factory_at(world: &World, id: u32, kind: Kind, x: u32, y: u32) -> Own {
    with_queue(structure_at(world, id, kind, x, y), &[], Some(gather()))
}

fn soldier(id: u32, x: u32, y: u32) -> Own {
    unit_at(id, RIFLEMAN, true, x, y)
}

fn with_queue(mut o: Own, items: &[Kind], rally: Option<Cell>) -> Own {
    o.factory = Some(OwnFactory {
        queue: items.iter().map(|&k| (k, false)).collect(),
        front_paid: 0,
        status: FactoryStatus::Building,
        rally,
    });
    o
}

/// The world's own observation at tick `tick` with `own` as the player's entities and `credits`; plenty of
/// power.
fn observation(world: &World, tick: u64, credits: u64, own: Vec<Own>) -> Observation {
    let mut obs = Observation::build(world, P0, &BTreeMap::new());
    obs.tick = Tick(tick);
    obs.credits = credits;
    obs.storage_capacity = 0;
    obs.power = Some(sf_sim::PowerTotals {
        made: 500,
        used: 100,
    });
    obs.own = own;
    obs
}

fn production_turn(world: &World, state: &mut AiState, obs: &Observation) -> Vec<Command> {
    let mut lens = Lens::new(world, P0);
    let mut out = Vec::new();
    production::turn(state, obs, &mut lens, &mut out);
    out
}

fn army_turn(world: &World, state: &mut AiState, obs: &Observation) -> Vec<Command> {
    let mut lens = Lens::new(world, P0);
    let mut out = Vec::new();
    army::turn(state, obs, &mut lens, &mut out);
    out
}

/// A base with every goal entry of `short_list` standing: the centre (0), two power plants (1, 5), a
/// refinery (2), a barracks (3) and two harvesters (10, 11), which are all the economy wants of one refinery.
/// Positions keep the structures out of the gather cell's way.
fn base(world: &World) -> Vec<Own> {
    vec![
        structure_at(world, 0, CENTRE, 12, 68),
        structure_at(world, 1, PLANT, 18, 60),
        structure_at(world, 2, REFINERY, 18, 74),
        factory_at(world, 3, BARRACKS, 12, 60),
        structure_at(world, 5, PLANT, 8, 60),
        unit_at(10, HARVESTER, false, 15, 80),
        unit_at(11, HARVESTER, false, 16, 80),
    ]
}

/// The goal list of the tests: power plant 2, refinery, barracks. The `base` satisfies it.
fn short_list(state: &mut AiState) {
    state.profile.build_order = vec![(PLANT, 2), (REFINERY, 1), (BARRACKS, 1)];
}

/// The same list with a vehicle factory (1,500 credits) still to build.
fn open_list(state: &mut AiState) {
    state.profile.build_order = vec![(PLANT, 2), (REFINERY, 1), (BARRACKS, 1), (FACTORY, 1)];
}

fn queues(out: &[Command]) -> Vec<(u32, u16)> {
    out.iter()
        .filter_map(|c| match c {
            Command::QueueUnit { factory, kind } => Some((factory.index(), kind.0)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_prices_the_tests_use_are_the_frozen_definitions() {
    let (world, state) = rig();
    let lens = Lens::new(&world, P0);
    for (kind, price) in [
        (RIFLEMAN, 100),
        (ROCKET, 300),
        (SCOUT, 400),
        (TANK, 700),
        (HARVESTER, 1100),
    ] {
        assert_eq!(lens.unit_cost(kind), Some(price), "{kind:?}");
    }
    for (kind, price) in [
        (PLANT, 300),
        (REFINERY, 1200),
        (BARRACKS, 400),
        (FACTORY, 1500),
    ] {
        assert_eq!(lens.building(kind).unwrap().cost, Some(price), "{kind:?}");
    }
    assert_eq!(state.profile.economy.reserve, 300);
    assert_eq!(state.profile.economy.max_queue, 2);
    assert_eq!(state.profile.economy.blocked_ticks, 600);
    let mix: Vec<(u16, u16)> = state
        .profile
        .unit_mix
        .iter()
        .map(|&(k, w)| (k.0, w))
        .collect();
    assert_eq!(mix, [(12, 4), (13, 2), (19, 3)]);
    assert_eq!(state.profile.scout, Some(SCOUT));
    // The gather cell of M1 for the base at (12, 68) and `gather_distance` 10.
    assert_eq!(state.profile.army.gather_distance, 10);
}

// ---- which unit: the mix, by share ---------------------------------------------------------------------

#[test]
fn the_kind_furthest_below_its_share_is_made_next_by_cross_multiplication() {
    let mix = [(RIFLEMAN, 4), (ROCKET, 2), (TANK, 3)];
    let all = |_: Kind| true;
    // Nothing made: a tie, the first of the list (not the last: a list iterated in reverse picks the tank).
    assert_eq!(production::pick_kind(&mix, &[0, 0, 0], all), Some(0));
    // The rocket trooper has the smallest share made, then the tank, then the rifleman again.
    assert_eq!(production::pick_kind(&mix, &[1, 0, 0], all), Some(1));
    assert_eq!(production::pick_kind(&mix, &[1, 1, 0], all), Some(2));
    assert_eq!(production::pick_kind(&mix, &[1, 1, 1], all), Some(0));
    // 3/4 = 0.75, 1/2 = 0.5, 2/3 = 0.67: the rocket trooper. With integer division the three shares are all
    // 0 and the first would win.
    assert_eq!(production::pick_kind(&mix, &[3, 1, 2], all), Some(1));
    // The mix is kept over a long run: 4 : 2 : 3 within one unit.
    let mut have = [0u32; 3];
    for _ in 0..90 {
        let i = production::pick_kind(&mix, &have, all).unwrap();
        have[i] += 1;
    }
    assert_eq!(have, [40, 20, 30]);
    // Only the kinds a factory can take now count.
    assert_eq!(
        production::pick_kind(&mix, &[0, 0, 0], |k| k == TANK),
        Some(2)
    );
    assert_eq!(production::pick_kind(&mix, &[0, 0, 0], |_| false), None);
    assert_eq!(production::pick_kind(&[], &[], all), None);
}

// ---- how much money: the reserve and the next goal entry ------------------------------------------------

#[test]
fn with_no_goal_open_the_price_alone_decides() {
    let (world, mut state) = rig();
    short_list(&mut state);
    state.profile.army.scouts_max = 0;
    let own = base(&world);
    for (credits, want) in [(99, vec![]), (100, vec![(3, 12)])] {
        let mut state = state.clone();
        let obs = observation(&world, 5000, credits, own.clone());
        assert_eq!(queues(&production_turn(&world, &mut state, &obs)), want);
    }
}

#[test]
fn with_a_goal_open_production_keeps_the_reserve_and_the_price_of_the_next_goal_back() {
    let (world, mut state) = rig();
    open_list(&mut state);
    state.profile.army.scouts_max = 0;
    let own = base(&world);
    // Next goal: the vehicle factory, 1,500. Held back: the reserve 300 and 1,500; a rifleman is 100 more.
    for (credits, want) in [(1899, vec![]), (1900, vec![(3, 12)]), (2500, vec![(3, 12)])] {
        let mut state = state.clone();
        let obs = observation(&world, 5000, credits, own.clone());
        assert_eq!(
            queues(&production_turn(&world, &mut state, &obs)),
            want,
            "{credits}"
        );
    }
    // The goal's price is capped by what the stores can hold, as the construction turn saves it: stores of
    // 1,000 and `silo_percent` 90 make it 900, so 300 + 900 + 100.
    for (credits, want) in [(1299, vec![]), (1300, vec![(3, 12)])] {
        let mut state = state.clone();
        let mut obs = observation(&world, 5000, credits, own.clone());
        obs.storage_capacity = 1000;
        assert_eq!(
            queues(&production_turn(&world, &mut state, &obs)),
            want,
            "{credits}"
        );
    }
    // The goal entry the construction turn would take is the power plant when power is short.
    let mut state2 = state.clone();
    let mut obs = observation(&world, 5000, 1900, own.clone());
    obs.power = Some(sf_sim::PowerTotals {
        made: 100,
        used: 90,
    });
    // 300 + 300 + 100 = 700: the plant comes first, so a rifleman at 1,900 is queued and at 699 is not.
    assert_eq!(queues(&production_turn(&world, &mut state2, &obs)).len(), 1);
    let mut state3 = state;
    let mut obs = observation(&world, 5000, 699, own);
    obs.power = Some(sf_sim::PowerTotals {
        made: 100,
        used: 90,
    });
    assert_eq!(queues(&production_turn(&world, &mut state3, &obs)), []);
}

#[test]
fn a_queued_unit_is_booked_once_for_the_base_and_for_production() {
    let (world, mut state) = rig();
    short_list(&mut state);
    state.profile.army.scouts_max = 0;
    // A rifleman already queued (100 unpaid) takes 100 of the free credits: by share the rocket trooper (300) is
    // next, and at 399 credits nothing is ordered although the price alone would be paid; at 400, one.
    let mut own = base(&world);
    own[3] = with_queue(own[3].clone(), &[RIFLEMAN], Some(gather()));
    for (credits, want) in [(399, vec![]), (400, vec![(3, 13)])] {
        let mut state = state.clone();
        let obs = observation(&world, 5000, credits, own.clone());
        assert_eq!(queues(&production_turn(&world, &mut state, &obs)), want);
    }
}

#[test]
fn production_leaves_the_money_to_a_harvester_the_economy_wants() {
    let (world, mut state) = rig();
    short_list(&mut state);
    state.profile.army.scouts_max = 0;
    // Two refineries want four harvesters, there are two, and a vehicle factory (4) can make one: the economy
    // orders it (the economy turn, not this one) and production orders nothing, however rich the AI is.
    let mut own = base(&world);
    own.push(factory_at(&world, 4, FACTORY, 12, 52));
    own.push(structure_at(&world, 6, REFINERY, 24, 74));
    own.sort_by_key(|o| o.id);
    let obs = observation(&world, 5000, 10_000, own.clone());
    assert_eq!(production_turn(&world, &mut state.clone(), &obs), []);
    // With four harvesters it makes units (the first of the mix, a rifleman, on the barracks, and a tank on the
    // vehicle factory: one order for each factory).
    own.push(unit_at(12, HARVESTER, false, 15, 82));
    own.push(unit_at(13, HARVESTER, false, 16, 82));
    own.sort_by_key(|o| o.id);
    let obs = observation(&world, 5000, 10_000, own);
    assert_eq!(
        queues(&production_turn(&world, &mut state, &obs)),
        [(3, 12), (4, 19)]
    );
}

// ---- which factory: queue room, the shortest queue, one order per factory per turn ----------------------

#[test]
fn a_factory_keeps_at_most_max_queue_items_and_gets_one_order_per_turn() {
    let (world, mut state) = rig();
    short_list(&mut state);
    state.profile.army.scouts_max = 0;
    let own = base(&world);
    let at = |queue: &[Kind]| {
        let mut own = own.clone();
        own[3] = with_queue(own[3].clone(), queue, Some(Cell::new(24, 64)));
        observation(&world, 5000, 10_000, own)
    };
    // Room for two: with one item queued, one more; with two, none.
    assert_eq!(
        queues(&production_turn(
            &world,
            &mut state.clone(),
            &at(&[RIFLEMAN])
        )),
        [(3, 13)]
    );
    assert_eq!(
        production_turn(&world, &mut state.clone(), &at(&[RIFLEMAN, ROCKET])),
        []
    );
    // The number is the profile's: three items fit with `max_queue` 3.
    let mut roomy = state.clone();
    roomy.profile.economy.max_queue = 3;
    assert_eq!(
        queues(&production_turn(
            &world,
            &mut roomy,
            &at(&[RIFLEMAN, ROCKET])
        )),
        [(3, 12)]
    );
    // However rich the AI is, one factory gets one order in a turn: a single barracks, one command.
    let out = production_turn(&world, &mut state.clone(), &at(&[]));
    assert_eq!(queues(&out), [(3, 12)]);
    assert_eq!(out.len(), 1);
    // Two factories of the same function (barracks 3 and 7), a vehicle factory (4): three orders, one each.
    let mut two = own.clone();
    two.push(factory_at(&world, 7, BARRACKS, 12, 44));
    two.push(factory_at(&world, 4, FACTORY, 12, 52));
    two.sort_by_key(|o| o.id);
    let obs = observation(&world, 5000, 10_000, two);
    let out = production_turn(&world, &mut state, &obs);
    // Rifleman (a tie, the first of the list) on barracks 3, then the rocket trooper (next furthest below its
    // share) on barracks 7, then the tank on the vehicle factory 4.
    assert_eq!(queues(&out), [(3, 12), (7, 13), (4, 19)]);
}

#[test]
fn the_factory_with_the_shortest_queue_gets_the_unit_and_the_lowest_id_among_equals() {
    let (world, mut state) = rig();
    short_list(&mut state);
    state.profile.army.scouts_max = 0;
    let mut own = base(&world);
    own.push(factory_at(&world, 7, BARRACKS, 12, 44));
    own.sort_by_key(|o| o.id);
    let pick = |q3: &[Kind], q7: &[Kind]| {
        let mut own = own.clone();
        for o in &mut own {
            if o.id.index() == 3 {
                *o = with_queue(o.clone(), q3, Some(gather()));
            }
            if o.id.index() == 7 {
                *o = with_queue(o.clone(), q7, Some(gather()));
            }
        }
        let obs = observation(&world, 5000, 10_000, own);
        // One of the two barracks is used by the first order; the second is the rocket trooper's.
        queues(&production_turn(&world, &mut state.clone(), &obs))
    };
    assert_eq!(pick(&[], &[]), [(3, 12), (7, 13)]);
    // A queued rifleman counts for the share, so the rocket trooper is next: on the barracks with the shorter
    // queue (7), then the rifleman on 3.
    assert_eq!(pick(&[RIFLEMAN], &[]), [(7, 13), (3, 12)]);
    assert_eq!(pick(&[], &[RIFLEMAN]), [(3, 13), (7, 12)]);
    // Equal queues: the lowest id.
    assert_eq!(pick(&[RIFLEMAN], &[RIFLEMAN]), [(3, 13), (7, 12)]);
}

// ---- an order that had no effect: the latch and the block ------------------------------------------------

#[test]
fn an_order_the_simulation_refused_blocks_its_kind_for_blocked_ticks() {
    let (world, mut state) = rig();
    short_list(&mut state);
    state.profile.army.scouts_max = 0;
    let own = base(&world);
    // Turn 1: a rifleman is ordered; the latch holds the factory, the kind, the tick and how many there were.
    let obs = observation(&world, 5000, 10_000, own.clone());
    assert_eq!(
        queues(&production_turn(&world, &mut state, &obs)),
        [(3, 12)]
    );
    assert_eq!(
        state.units_sent,
        [(EntityId::new(3, 0), RIFLEMAN, Tick(5000), 0, 0)]
    );
    // Turn 2: it is in the queue. Nothing is blocked, and by share the rocket trooper is next.
    let mut seen = own.clone();
    seen[3] = with_queue(seen[3].clone(), &[RIFLEMAN], Some(gather()));
    let obs = observation(&world, 5040, 10_000, seen);
    assert_eq!(
        queues(&production_turn(&world, &mut state, &obs)),
        [(3, 13)]
    );
    assert!(state.blocked_until.is_empty());
    // The same orders but the simulation refused the rifleman (the queue is empty and there is no new unit):
    // the kind is blocked for 600 ticks, and the rocket trooper is made instead.
    let mut state = rig().1;
    short_list(&mut state);
    state.profile.army.scouts_max = 0;
    let obs = observation(&world, 5000, 10_000, own.clone());
    production_turn(&world, &mut state, &obs);
    let obs = observation(&world, 5040, 10_000, own.clone());
    assert_eq!(
        queues(&production_turn(&world, &mut state, &obs)),
        [(3, 13)]
    );
    assert_eq!(
        state
            .blocked_until
            .iter()
            .map(|(k, t)| (k.0, t.0))
            .collect::<Vec<_>>(),
        [(12, 5640)]
    );
    // At 5600 the rocket trooper ordered at 5040 had no effect either: both kinds are blocked (the rifleman until
    // 5640, the rocket trooper until 6200) and nothing is ordered; at 5640 the rifleman is tried again.
    let obs = observation(&world, 5600, 10_000, own.clone());
    assert_eq!(production_turn(&world, &mut state, &obs), []);
    assert_eq!(
        state
            .blocked_until
            .iter()
            .map(|(k, t)| (k.0, t.0))
            .collect::<Vec<_>>(),
        [(12, 5640), (13, 6200)]
    );
    let obs = observation(&world, 5640, 10_000, own);
    assert_eq!(
        queues(&production_turn(&world, &mut state, &obs)),
        [(3, 12)]
    );
}

// ---- the scout in production ---------------------------------------------------------------------------

#[test]
fn the_scout_is_the_first_unit_from_scout_tick_on_and_nothing_goes_before_it() {
    let (world, mut state) = rig();
    short_list(&mut state);
    assert_eq!(
        (state.profile.army.scout_tick, state.profile.army.scouts_max),
        (600, 2)
    );
    let own = base(&world);
    // Before `scout_tick` nothing is ordered, although a rifleman could be paid and the barracks stands.
    let obs = observation(&world, 599, 10_000, own.clone());
    assert_eq!(production_turn(&world, &mut state.clone(), &obs), []);
    // At 600 the scout (400) goes on the barracks; `scouts_sent` counts it.
    let obs = observation(&world, 600, 10_000, own.clone());
    assert_eq!(
        queues(&production_turn(&world, &mut state, &obs)),
        [(3, 15)]
    );
    assert_eq!(state.scouts_sent, 1);
    // From then on the mix: the scout in the queue is one unit more on the barracks, which has room for a
    // second item.
    let mut queued = own.clone();
    queued[3] = with_queue(queued[3].clone(), &[SCOUT], Some(gather()));
    let obs = observation(&world, 640, 10_000, queued);
    assert_eq!(
        queues(&production_turn(&world, &mut state, &obs)),
        [(3, 12)]
    );
    // With `scouts_max` 0 there is no scout and no wait: a rifleman at tick 0.
    let mut none = rig().1;
    short_list(&mut none);
    none.profile.army.scouts_max = 0;
    let obs = observation(&world, 0, 10_000, own.clone());
    assert_eq!(queues(&production_turn(&world, &mut none, &obs)), [(3, 12)]);
    // A profile without a scout makes units at once as well.
    let mut none = rig().1;
    short_list(&mut none);
    none.profile.scout = None;
    assert_eq!(queues(&production_turn(&world, &mut none, &obs)), [(3, 12)]);
}

#[test]
fn the_scout_is_held_back_by_the_reserve_only_not_by_the_next_goal() {
    let (world, mut state) = rig();
    open_list(&mut state);
    let own = base(&world);
    // The vehicle factory (1,500) is the next goal; the army units wait for 300 + 1,500 + their price, the
    // scout for 300 + 400.
    for (credits, want) in [(699, vec![]), (700, vec![(3, 15)])] {
        let mut state = state.clone();
        let obs = observation(&world, 700, credits, own.clone());
        assert_eq!(
            queues(&production_turn(&world, &mut state, &obs)),
            want,
            "{credits}"
        );
    }
    // Without the next goal the reserve is not kept back either once the list is complete.
    short_list(&mut state);
    let obs = observation(&world, 700, 400, own);
    assert_eq!(
        queues(&production_turn(&world, &mut state, &obs)),
        [(3, 15)]
    );
}

#[test]
fn a_scout_lost_is_bought_again_after_scout_retry_and_never_more_than_scouts_max() {
    let (world, state) = rig();
    let mut state = state;
    short_list(&mut state);
    let own = base(&world);
    // One scout ordered, it was seen lost at tick 1000: nothing before 4000, then the scout first.
    state.scouts_sent = 1;
    state.scout_gone_at = Some(Tick(1000));
    let obs = observation(&world, 3999, 10_000, own.clone());
    let out = production_turn(&world, &mut state.clone(), &obs);
    assert!(!out.iter().any(|c| is_queue_of(c, SCOUT)), "{out:?}");
    let obs = observation(&world, 4000, 10_000, own.clone());
    let mut again = state.clone();
    assert_eq!(
        queues(&production_turn(&world, &mut again, &obs)),
        [(3, 15)]
    );
    assert_eq!(again.scouts_sent, 2);
    // A scout that is alive (or queued) is not replaced; two ordered is the most.
    let mut alive = own.clone();
    alive.push(unit_at(20, SCOUT, true, 30, 60));
    alive.sort_by_key(|o| o.id);
    let obs = observation(&world, 9000, 10_000, alive);
    let out = production_turn(&world, &mut state.clone(), &obs);
    assert!(!out.iter().any(|c| is_queue_of(c, SCOUT)), "{out:?}");
    let mut spent = state;
    spent.scouts_sent = 2;
    let obs = observation(&world, 9000, 10_000, own);
    let out = production_turn(&world, &mut spent, &obs);
    assert!(!out.iter().any(|c| is_queue_of(c, SCOUT)), "{out:?}");
}

#[test]
fn a_scout_order_that_had_no_effect_is_taken_back_and_blocked() {
    let (world, mut state) = rig();
    short_list(&mut state);
    let own = base(&world);
    let obs = observation(&world, 600, 10_000, own.clone());
    production_turn(&world, &mut state, &obs);
    assert_eq!(state.scouts_sent, 1);
    // Forty ticks later nothing is queued and no scout stands: the order was refused. It no longer counts,
    // the kind is blocked for 600 ticks (and, as the scout is blocked, a rifleman goes meanwhile).
    let obs = observation(&world, 640, 10_000, own.clone());
    assert_eq!(
        queues(&production_turn(&world, &mut state, &obs)),
        [(3, 12)]
    );
    assert_eq!(state.scouts_sent, 0);
    assert_eq!(state.blocked_until.get(&SCOUT), Some(&Tick(1240)));
    // When the block is over the scout is tried again.
    let obs = observation(&world, 1240, 10_000, own);
    let mut later = state;
    assert_eq!(
        queues(&production_turn(&world, &mut later, &obs)),
        [(3, 15)]
    );
}

// ---- the rally point -----------------------------------------------------------------------------------

#[test]
fn every_complete_factory_gets_one_rally_point_to_the_gather_cell_and_only_when_it_differs() {
    let (world, mut state) = rig();
    short_list(&mut state);
    state.profile.army.scouts_max = 0;
    state.profile.unit_mix.clear();
    state.profile.scout = Some(SCOUT);
    let g = Cell::new(24, 64);
    let mut own = base(&world);
    own.push(factory_at(&world, 4, FACTORY, 12, 52));
    own.sort_by_key(|o| o.id);
    own[3] = with_queue(own[3].clone(), &[], None);
    own[4] = with_queue(own[4].clone(), &[], None);
    // `makes_units` holds through the scout in the profile; the scout is not bought (max 0).
    let rally = |out: &[Command]| -> Vec<(u32, Option<Cell>)> {
        out.iter()
            .filter_map(|c| match c {
                Command::SetRally { factory, cell } => Some((factory.index(), *cell)),
                _ => None,
            })
            .collect()
    };
    // No rally point: the barracks is sent one; the vehicle factory (it makes harvesters, which would mine
    // later for the detour: see `makes_harvesters`), the refinery and the centre are not.
    let obs = observation(&world, 5000, 0, own.clone());
    assert_eq!(
        rally(&production_turn(&world, &mut state, &obs)),
        [(3, Some(g))]
    );
    assert_eq!(state.gather, Some(g));
    // Both show the point at the next turn: nothing, and the backoff is gone.
    let mut set = own.clone();
    set[3] = with_queue(set[3].clone(), &[], Some(g));
    set[4] = with_queue(set[4].clone(), &[], Some(g));
    let obs = observation(&world, 5040, 0, set.clone());
    assert_eq!(rally(&production_turn(&world, &mut state, &obs)), []);
    assert!(state.rally_backoff.is_empty());
    // Another point (the human's, or an old one): sent again, once; asked, not shown: not repeated before
    // `blocked_ticks` (600) have passed.
    set[3] = with_queue(set[3].clone(), &[], Some(Cell::new(30, 30)));
    let obs = observation(&world, 5080, 0, set.clone());
    assert_eq!(
        rally(&production_turn(&world, &mut state, &obs)),
        [(3, Some(g))]
    );
    let obs = observation(&world, 5120, 0, set.clone());
    assert_eq!(rally(&production_turn(&world, &mut state, &obs)), []);
    let obs = observation(&world, 5679, 0, set.clone());
    assert_eq!(rally(&production_turn(&world, &mut state, &obs)), []);
    let obs = observation(&world, 5680, 0, set.clone());
    assert_eq!(
        rally(&production_turn(&world, &mut state, &obs)),
        [(3, Some(g))]
    );
    // A complete factory with no production record at all has no rally point either.
    let mut bare = own.clone();
    bare[3].factory = None;
    bare[4] = with_queue(bare[4].clone(), &[], Some(g));
    let mut fresh = rig().1;
    fresh.profile.unit_mix.clear();
    let obs = observation(&world, 9000, 0, bare);
    assert_eq!(
        rally(&production_turn(&world, &mut fresh, &obs)),
        [(3, Some(g))]
    );
    // A factory still going up is not asked.
    let mut up = set;
    up[3] = {
        let mut o = up[3].clone();
        o.standing = false;
        o.building = Some(sf_sim::Construction {
            left: 100,
            total: 200,
        });
        o
    };
    let mut fresh = rig().1;
    fresh.profile.unit_mix.clear();
    let obs = observation(&world, 9000, 0, up);
    assert_eq!(rally(&production_turn(&world, &mut fresh, &obs)), []);
}

// ---- the gather cell -----------------------------------------------------------------------------------

#[test]
fn the_gather_cell_is_ten_cells_from_the_base_towards_the_middle_of_the_map() {
    let (world, mut state) = rig();
    let obs = observation(&world, 100, 0, base(&world));
    let lens = Lens::new(&world, P0);
    // The centre's middle is (14, 70), the map's (48, 48): 34 across and 22 up, so ten along x and
    // 22 * 10 / 34 = 6 up, truncated.
    assert_eq!(
        army::ensure_gather(&mut state, &obs, &lens),
        Some(Cell::new(24, 64))
    );
    assert_eq!(state.gather, Some(Cell::new(24, 64)));
    // The same from the other corner of the map (a centre at (80, 12), middle (82, 14)): 34 left and 34 down,
    // the larger axis is x (a tie goes to x), ten left and 34 * 10 / 34 = 10 down.
    let mut other = rig().1;
    let mut own = base(&world);
    own[0] = structure_at(&world, 0, CENTRE, 80, 12);
    let obs = observation(&world, 100, 0, own);
    let g = army::ensure_gather(&mut other, &obs, &lens).unwrap();
    assert_eq!(g, Cell::new(72, 24));
}

#[test]
fn the_gather_cell_keeps_off_structures_aprons_and_a_lane_of_three_cells() {
    let (world, state) = rig();
    let lens = Lens::new(&world, P0);
    let first = Cell::new(24, 64);
    let cell_for = |own: Vec<Own>| {
        let mut state = state.clone();
        let obs = observation(&world, 100, 0, own);
        army::ensure_gather(&mut state, &obs, &lens)
    };
    // An own structure three cells (the lane of one plus two) or less from the first choice pushes it out,
    // to the first free cell of the rings: here the plant at (27, 64), 2 x 3 cells, keeps the cells of rings 1
    // and 2 within three cells of it, and the base's own structures the rest, down to (21, 67) in ring 3. (The
    // expected cells were worked out again by a short script of the rule, apart from the code.)
    let mut own = base(&world);
    own.push(structure_at(&world, 9, PLANT, 27, 64));
    own.sort_by_key(|o| o.id);
    let g = cell_for(own).unwrap();
    assert_eq!(g, Cell::new(21, 67));
    // Four cells away is far enough.
    let mut own = base(&world);
    own.push(structure_at(&world, 9, PLANT, 29, 64));
    own.sort_by_key(|o| o.id);
    assert_eq!(cell_for(own), Some(first));
    // The apron of a refinery (3 wide, 5 deep from its exit cell) reaches further than the lane: the
    // refinery at (17, 63) has its exit at (20, 64) and its apron over x 20..=24, y 63..=65, the first choice
    // included, and it is more than three cells from it. The first free cell of ring 1 is (24, 63).
    let mut own = base(&world);
    own.push(structure_at(&world, 9, REFINERY, 17, 63));
    own.sort_by_key(|o| o.id);
    let b = world.building_table().get(REFINERY).unwrap();
    assert_eq!(
        (b.footprint.width, b.footprint.height, b.exit_side),
        (3, 4, Some(sf_sim::Side::PosX))
    );
    assert_eq!(cell_for(own), Some(Cell::new(24, 63)));
    // A structure of another player on the cell (seen) keeps it away too.
    let mut s = state.clone();
    let mut obs = observation(&world, 100, 0, base(&world));
    obs.visible.push(sf_ai::Seen {
        id: EntityId::new(50, 0),
        kind: PLANT,
        owner: P1,
        cell: Cell::new(23, 63),
        footprint: world.building_table().get(PLANT).map(|b| b.footprint),
        hp: 600,
        armed: false,
    });
    let g = army::ensure_gather(&mut s, &obs, &lens).unwrap();
    assert!(
        !(23..25).contains(&g.x) || !(63..66).contains(&g.y),
        "{g:?} is under the enemy plant"
    );
}

#[test]
fn the_gather_cell_is_kept_and_worked_out_again_only_when_it_stops_being_open_ground() {
    let (world, mut state) = rig();
    let lens = Lens::new(&world, P0);
    let obs = observation(&world, 100, 0, base(&world));
    // A cell of open ground that the search would not choose stays, turn after turn.
    state.gather = Some(Cell::new(30, 60));
    assert!(lens.ground(Cell::new(30, 60)));
    for _ in 0..3 {
        assert_eq!(
            army::ensure_gather(&mut state, &obs, &lens),
            Some(Cell::new(30, 60))
        );
    }
    // A cell that is not open ground is replaced by the one the search gives.
    let (w, h) = world.map_size();
    let rock = (0..h)
        .flat_map(|y| (0..w).map(move |x| Cell::new(x, y)))
        .find(|&c| !lens.ground(c))
        .expect("the map has ground that is not open");
    state.gather = Some(rock);
    assert_eq!(
        army::ensure_gather(&mut state, &obs, &lens),
        Some(Cell::new(24, 64))
    );
    // Without an anchor there is no cell, and it is asked again at the next turn.
    let mut lost = rig().1;
    let none = observation(&world, 100, 0, vec![]);
    assert_eq!(army::ensure_gather(&mut lost, &none, &lens), None);
    assert_eq!(
        army::ensure_gather(&mut lost, &obs, &lens),
        Some(Cell::new(24, 64))
    );
}

// ---- the army turn: gathering --------------------------------------------------------------------------

/// The state with the gather cell of the base at (12, 68), as the army turn computes it.
fn army_state() -> (World, AiState) {
    let (world, mut state) = rig();
    short_list(&mut state);
    (world, state)
}

fn ids_of(c: &Command) -> Vec<u32> {
    match c {
        Command::Order { ids, .. } => ids.iter().map(|i| i.index()).collect(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn idle_army_units_far_from_the_gather_cell_are_sent_there_in_groups_of_nineteen() {
    let (world, mut state) = army_state();
    // 41 riflemen idle at (60, 60); two commands of 19 in the first turn (ids ascending), the last three at
    // the next; `GATHER_COMMANDS_PER_TURN` is 2.
    assert_eq!(GATHER_COMMANDS_PER_TURN, 2);
    let mut own = base(&world);
    own.extend((0..41).map(|i| soldier(20 + i, 60, 60)));
    let obs = observation(&world, 1000, 0, own.clone());
    let out = army_turn(&world, &mut state, &obs);
    assert_eq!(out.len(), 2);
    assert_eq!(ids_of(&out[0]), (20..39).collect::<Vec<_>>());
    assert_eq!(ids_of(&out[1]), (39..58).collect::<Vec<_>>());
    for c in &out {
        let Command::Order { order, queued, .. } = c else {
            panic!()
        };
        assert_eq!((*order, *queued), (Order::Move(middle(24, 64)), false));
    }
    // The next turn: the 38 were ordered 40 ticks ago, the other three go.
    let obs = observation(&world, 1040, 0, own.clone());
    let out = army_turn(&world, &mut state, &obs);
    assert_eq!(out.len(), 1);
    assert_eq!(ids_of(&out[0]), [58, 59, 60]);
    // At 1120 the first 38 may be sent again (120 ticks), not the three (80 ticks): two commands.
    let obs = observation(&world, 1120, 0, own.clone());
    let out = army_turn(&world, &mut state, &obs);
    assert_eq!(out.len(), 2);
    assert_eq!(ids_of(&out[0]).len() + ids_of(&out[1]).len(), 38);
    // At 1240 all 41 may be sent again, the cap of two commands takes the first 38 (third order of each).
    let obs = observation(&world, 1240, 0, own.clone());
    let out = army_turn(&world, &mut state, &obs);
    assert_eq!(out.len(), 2);
    assert_eq!(ids_of(&out[0]), (20..39).collect::<Vec<_>>());
    // Three orders in all without arriving (`gather_tries`), and then a unit is left where it is: at 1360 only
    // the three get their second, at 1480 their third, and at 1600 nobody is ordered.
    let obs = observation(&world, 1360, 0, own.clone());
    let out = army_turn(&world, &mut state, &obs);
    assert_eq!(out.len(), 1);
    assert_eq!(ids_of(&out[0]), [58, 59, 60]);
    let obs = observation(&world, 1480, 0, own.clone());
    let out = army_turn(&world, &mut state, &obs);
    assert_eq!(out.len(), 1);
    assert_eq!(ids_of(&out[0]), [58, 59, 60]);
    let obs = observation(&world, 1600, 0, own);
    assert_eq!(army_turn(&world, &mut state, &obs), []);
    assert_eq!(state.sent.len(), 41);
}

#[test]
fn a_unit_is_ordered_only_when_it_is_idle_and_never_the_scout_a_harvester_or_an_unarmed_unit() {
    let (world, mut state) = army_state();
    let mut own = base(&world);
    let moving = |mut o: Own| {
        o.order = Order::Move(middle(30, 30));
        o
    };
    own.extend([
        soldier(20, 60, 60),                   // idle: sent
        moving(soldier(21, 60, 60)),           // walking: left alone
        unit_at(22, SCOUT, true, 60, 60),      // the scout
        unit_at(23, HARVESTER, false, 60, 60), // harvests
        unit_at(24, Kind(14), false, 60, 60),  // unarmed (an engineer)
        {
            let mut o = soldier(25, 60, 60);
            o.mobile = false; // does not move
            o
        },
        soldier(26, 60, 60), // idle: sent
    ]);
    state.scout = Some(EntityId::new(22, 0));
    state.scout_leg_sent = Some(Tick(900));
    let obs = observation(&world, 1000, 0, own);
    let out = army_turn(&world, &mut state, &obs);
    assert_eq!(out.len(), 1);
    assert_eq!(ids_of(&out[0]), [20, 26]);
}

#[test]
fn a_unit_within_the_gather_radius_has_arrived_and_its_tries_start_again() {
    let (world, mut state) = army_state();
    // Chebyshev distance 8 is inside, 9 is not: ids 20 (32, 64) and 21 (24, 56) arrived, 22 (33, 64) and 23
    // (24, 55) did not; 24 at (16, 72) is on the corner, distance 8.
    let own_with = |cells: &[(u32, u32)]| {
        let mut own = base(&world);
        own.extend(
            cells
                .iter()
                .enumerate()
                .map(|(i, &(x, y))| soldier(20 + i as u32, x, y)),
        );
        own
    };
    let cells = [(32, 64), (24, 56), (33, 64), (24, 55), (16, 72)];
    let obs = observation(&world, 1000, 0, own_with(&cells));
    let out = army_turn(&world, &mut state, &obs);
    assert_eq!(out.len(), 1);
    assert_eq!(ids_of(&out[0]), [22, 23]);
    assert_eq!(state.profile.army.gather_radius, 8);
    // 22 walks in and arrives: its entry goes. When it is outside again and idle it is ordered at once (a
    // new first order, not a wait of `resend_ticks`).
    let mut arrived = cells;
    arrived[2] = (30, 64);
    let obs = observation(&world, 1040, 0, own_with(&arrived));
    assert_eq!(army_turn(&world, &mut state, &obs), []);
    assert!(!state.sent.contains_key(&EntityId::new(22, 0)));
    assert!(state.sent.contains_key(&EntityId::new(23, 0)));
    let obs = observation(&world, 1060, 0, own_with(&cells));
    let out = army_turn(&world, &mut state, &obs);
    assert_eq!(out.len(), 1);
    assert_eq!(ids_of(&out[0]), [22]);
}

#[test]
fn the_group_size_is_the_profiles_and_dead_units_leave_the_bookkeeping() {
    let (world, mut state) = army_state();
    state.profile.army.gather_group = 5;
    let mut own = base(&world);
    own.extend((0..12).map(|i| soldier(20 + i, 60, 60)));
    let obs = observation(&world, 1000, 0, own.clone());
    let out = army_turn(&world, &mut state, &obs);
    assert_eq!(
        out.iter().map(|c| ids_of(c).len()).collect::<Vec<_>>(),
        [5, 5]
    );
    assert_eq!(state.sent.len(), 10);
    // Ten are gone: only the live ones stay in `sent`.
    own.truncate(own.len() - 10);
    let obs = observation(&world, 1040, 0, own);
    army_turn(&world, &mut state, &obs);
    assert_eq!(
        state.sent.keys().map(|i| i.index()).collect::<Vec<_>>(),
        [20, 21]
    );
}

#[test]
fn soldiers_far_from_the_gather_cell_are_gathered_and_a_profile_without_units_leaves_both_managers_off()
 {
    let (world, mut state) = army_state();
    let mut own = base(&world);
    own.extend((0..6).map(|i| soldier(20 + i, 60, 60)));
    let obs = observation(&world, 1000, 0, own.clone());
    let out = army_turn(&world, &mut state, &obs);
    assert!(out.iter().all(|c| matches!(
        c,
        Command::Order {
            order: Order::Move(_),
            ..
        }
    )));
    // Six soldiers far away are not gathered: no attack (WP9.1c-2 counts only gathered units).
    assert_eq!(state.directive, sf_ai::state::Directive::default());
    assert_eq!(state.directive.posture, sf_ai::state::Posture::Gather);
    assert_eq!(state.attacks_launched, 0);
    // No unit mix and no scout: neither manager does anything, whatever stands in the observation.
    let mut off = state.clone();
    off.profile.unit_mix.clear();
    off.profile.scout = None;
    let before = off.clone();
    let obs = observation(&world, 1040, 10_000, own);
    assert_eq!(army_turn(&world, &mut off, &obs), []);
    assert_eq!(production_turn(&world, &mut off, &obs), []);
    assert_eq!(off, before);
}

// ---- the scout in the army turn ------------------------------------------------------------------------

#[test]
fn the_scout_legs_are_the_other_start_sites_then_the_four_quarters() {
    let (world, state) = rig();
    let obs = observation(&world, 0, 0, vec![]);
    let legs: Vec<(u32, u32)> = army::waypoints(&state, &obs)
        .iter()
        .map(|c| (c.x, c.y))
        .collect();
    assert_eq!(
        state.start_sites,
        [(P0, Cell::new(12, 68)), (P1, Cell::new(72, 14))]
    );
    assert_eq!(legs, [(72, 14), (24, 24), (72, 24), (24, 72), (72, 72)]);
}

#[test]
fn a_new_scout_is_sent_once_to_each_leg_and_the_legs_end() {
    let (world, mut state) = army_state();
    let at = |cell: (u32, u32), order: Order| {
        let mut own = base(&world);
        let mut s = unit_at(20, SCOUT, true, cell.0, cell.1);
        s.order = order;
        own.push(s);
        own
    };
    // It stands idle in the base: it becomes the scout and is sent to the start site of player 1.
    let obs = observation(&world, 1200, 0, at((26, 66), Order::Idle));
    let out = army_turn(&world, &mut state, &obs);
    assert_eq!(out, [move_order(&[20], 72, 14)]);
    assert_eq!(state.scout, Some(EntityId::new(20, 0)));
    assert_eq!(
        (state.scout_leg, state.scout_leg_sent),
        (0, Some(Tick(1200)))
    );
    // Walking, or idle again after giving up: not ordered again to the same leg, until the patience (1,200
    // ticks) has run out; then the next leg.
    let walking = Order::Move(middle(72, 14));
    for t in [1240, 1600, 2000, 2399] {
        let obs = observation(&world, t, 0, at((40, 40), walking));
        assert_eq!(army_turn(&world, &mut state, &obs), [], "tick {t}");
        let obs = observation(&world, t, 0, at((40, 40), Order::Idle));
        assert_eq!(army_turn(&world, &mut state, &obs), [], "idle, tick {t}");
    }
    let obs = observation(&world, 2400, 0, at((40, 40), Order::Idle));
    assert_eq!(
        army_turn(&world, &mut state, &obs),
        [move_order(&[20], 24, 24)]
    );
    assert_eq!(state.scout_leg, 1);
    // Within 3 cells of the next waypoint it is there: the leg advances and the next one is sent in the same
    // turn, even when the order has not ended (a waypoint on a structure never ends).
    let obs = observation(&world, 2440, 0, at((26, 26), walking));
    assert_eq!(
        army_turn(&world, &mut state, &obs),
        [move_order(&[20], 72, 24)]
    );
    assert_eq!(state.scout_leg, 2);
    let obs = observation(&world, 2480, 0, at((70, 25), walking));
    assert_eq!(
        army_turn(&world, &mut state, &obs),
        [move_order(&[20], 24, 72)]
    );
    let obs = observation(&world, 2520, 0, at((24, 70), walking));
    assert_eq!(
        army_turn(&world, &mut state, &obs),
        [move_order(&[20], 72, 72)]
    );
    // The last leg reached: nothing more, ever.
    for t in [2560, 5000, 20_000] {
        let obs = observation(&world, t, 0, at((72, 70), Order::Idle));
        assert_eq!(army_turn(&world, &mut state, &obs), [], "tick {t}");
    }
    assert_eq!(state.scout_leg, 5);
    // The scout is not an army unit: it was not sent to the gather cell although it stood far from it.
}

#[test]
fn a_scout_that_is_gone_is_noted_and_a_scout_lost_in_a_queue_too() {
    let (world, mut state) = army_state();
    state.scout = Some(EntityId::new(20, 0));
    state.scouts_sent = 1;
    let obs = observation(&world, 3000, 0, base(&world));
    army_turn(&world, &mut state, &obs);
    assert_eq!((state.scout, state.scout_gone_at), (None, Some(Tick(3000))));
    // Ordered (scouts_sent 1), never seen as a unit and in no queue: lost before it came out.
    let mut state = army_state().1;
    state.scouts_sent = 1;
    let obs = observation(&world, 3000, 0, base(&world));
    army_turn(&world, &mut state, &obs);
    assert_eq!(state.scout_gone_at, Some(Tick(3000)));
    // In the queue it is not lost.
    let mut state = army_state().1;
    state.scouts_sent = 1;
    let mut own = base(&world);
    own[3] = with_queue(own[3].clone(), &[SCOUT], Some(gather()));
    let obs = observation(&world, 3000, 0, own);
    army_turn(&world, &mut state, &obs);
    assert_eq!(state.scout_gone_at, None);
}

// ---- what the observation says about armed and mobile ----------------------------------------------------

#[test]
fn armed_and_mobile_come_from_the_public_tables_and_a_hidden_unit_is_not_in_the_observation() {
    let mut s = load("ai_scout");
    let spawn = |owner, kind, cell| TimedCommand {
        tick: 0,
        player: owner,
        command: Action::SpawnUnit { kind, owner, cell },
    };
    s.commands.extend([
        spawn(0, 12, (16, 66)), // own rifleman
        spawn(0, 11, (17, 66)), // own harvester
        spawn(0, 15, (18, 66)), // own scout
        spawn(1, 12, (14, 64)), // enemy rifleman next to the base: seen
        spawn(1, 11, (15, 64)), // enemy harvester next to it: seen, unarmed
        spawn(1, 12, (90, 90)), // enemy rifleman in the far corner: not seen
    ]);
    let f = go(&s, 2);
    let obs = Observation::build(&f.world, P0, &BTreeMap::new());
    let kinds = |list: &[Own]| -> Vec<(u16, bool, bool)> {
        list.iter().map(|o| (o.kind.0, o.armed, o.mobile)).collect()
    };
    let own = kinds(&obs.own);
    // The centre is neither armed nor mobile; the units: rifleman and scout armed, harvester not, all mobile.
    assert!(own.contains(&(0, false, false)), "{own:?}");
    assert!(own.contains(&(12, true, true)), "{own:?}");
    assert!(own.contains(&(11, false, true)), "{own:?}");
    assert!(own.contains(&(15, true, true)), "{own:?}");
    let seen: Vec<(u16, bool)> = obs.visible.iter().map(|e| (e.kind.0, e.armed)).collect();
    assert!(
        seen.contains(&(12, true)) && seen.contains(&(11, false)),
        "{seen:?}"
    );
    // Exactly the two near enemy units are visible: the one in the corner is not there, and the enemy's own
    // structures are far away.
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert!(
        f.world.visibility(P0, Cell::new(90, 90)) != Visibility::Visible,
        "the corner is in the fog"
    );
}

// ---- the commands the AI sends --------------------------------------------------------------------------

/// Whether `c` is one of the commands a player may send that the AI of WP9.1c-1 uses: a build, a unit, a
/// rally point, a repair, the rescue sale, or a plain `Move` of units that is not queued.
fn on_the_list(c: &Command) -> bool {
    match c {
        Command::BuildStructure { .. }
        | Command::QueueUnit { .. }
        | Command::SetRally { .. }
        | Command::Repair { .. }
        | Command::Sell { .. } => true,
        // WP9.1c-2: the army attack-moves and attacks too.
        Command::Order { ids, order, queued } => {
            !ids.is_empty()
                && !queued
                && matches!(
                    order,
                    Order::Move(_) | Order::AttackMove(_) | Order::Attack(_)
                )
        }
        _ => false,
    }
}

#[test]
fn the_ai_of_ai_scout_sends_only_what_the_list_allows_and_at_a_bounded_rate() {
    let s = load("ai_scout");
    let f = go(&s, 20_000);
    assert!(!f.ai_commands.is_empty());
    for (t, p, c) in &f.ai_commands {
        assert_eq!(*p, 0);
        assert!(on_the_list(c), "tick {t}: {c:?}");
    }
    // Every command kind of the list shows up at least once except the sale (nothing is sold here).
    let has = |pick: fn(&Command) -> bool| f.ai_commands.iter().any(|(_, _, c)| pick(c));
    assert!(has(|c| matches!(c, Command::BuildStructure { .. })));
    assert!(has(|c| matches!(c, Command::QueueUnit { .. })));
    assert!(has(|c| matches!(c, Command::SetRally { .. })));
    assert!(has(|c| matches!(c, Command::Order { .. })));
    assert!(!has(|c| matches!(c, Command::Sell { .. })));
    // The most commands in any 1,000 ticks.
    let worst = worst_window(&f);
    println!("most commands in 1,000 ticks over 20,000: {worst}");
    assert!(worst <= COMMANDS_PER_1000, "{worst}");
    // And at most `ARMY_COMMANDS_PER_TURN` orders per army turn (WP9.1c-2; the scout's included), of which at
    // most `GATHER_COMMANDS_PER_TURN` to the gather cell.
    let mut per_turn: BTreeMap<u64, usize> = BTreeMap::new();
    for (t, _, c) in &f.ai_commands {
        if matches!(c, Command::Order { .. }) {
            *per_turn.entry(*t).or_default() += 1;
        }
    }
    assert!(per_turn.values().all(|&n| n <= ARMY_COMMANDS_PER_TURN));
    assert_eq!((GATHER_COMMANDS_PER_TURN, ARMY_COMMANDS_PER_TURN), (2, 4));
}

/// Measured: the most in any window of 1,000 ticks over `ai_scout` is 6, and 6 over its 40,000-tick run; the bound
/// leaves room for a busier start of the same AI (a turn sends at most one order per factory, a rally point per
/// factory and two gather orders).
const COMMANDS_PER_1000: usize = 10;

/// The most commands the AI sent in any run of 1,000 ticks that starts on a multiple of 100.
fn worst_window(f: &Finished) -> usize {
    let end = f.outcome.ticks;
    (0..=end.saturating_sub(1000))
        .step_by(100)
        .map(|from| {
            f.ai_commands
                .iter()
                .filter(|(t, _, _)| *t >= from && *t < from + 1000)
                .count()
        })
        .max()
        .unwrap_or(0)
}

#[test]
fn over_forty_thousand_ticks_the_ai_stays_on_the_list_and_under_the_rate_bound() {
    // The AI's whole economy and production for 40,000 ticks. Since WP9.1c-2 the army attacks the static centre
    // and wins (the run goes on after the match is decided, the AI then sends nothing); the "no enemy reachable"
    // run of the memo is `with_no_enemy_known_...` in ai_army.rs.
    let s = load("ai_scout");
    let f = go(&s, 40_000);
    for (t, _, c) in &f.ai_commands {
        assert!(on_the_list(c), "tick {t}: {c:?}");
    }
    let worst = worst_window(&f);
    let army = f
        .world
        .snapshot()
        .iter()
        .filter(|v| {
            v.owner == P0 && f.world.footprint_of(v.id).is_none() && ![11, 15].contains(&v.kind.0)
        })
        .count();
    println!(
        "40,000 ticks: {} commands, most in 1,000 ticks {worst}, {army} army units",
        f.ai_commands.len()
    );
    assert!(worst <= COMMANDS_PER_1000, "{worst}");
    assert_eq!(
        f.world.match_result().map(|r| r.verdict),
        Some(sf_sim::Verdict::Won(P0))
    );
}

// ---- the older scenarios: the managers are off for their frozen profiles ---------------------------------

/// The scenarios of WP9.1a and WP9.1b, each on a frozen profile with an empty `unit_mix` and no scout, with the
/// checksum pinned in its own test file.
const FROZEN: [&str; 7] = [
    "ai_fog",
    "ai_base",
    "ai_noore",
    "ai_boxed",
    "ai_economy",
    "ai_rebuild",
    "ai_deadlock",
];

#[test]
fn the_managers_stay_off_for_the_frozen_profiles_and_the_old_mix_would_move_every_checksum() {
    for name in FROZEN {
        let s = load(name);
        let expected = parse_checksum(
            &std::fs::read_to_string(scenario_path(&format!("{name}.expected"))).unwrap(),
        )
        .unwrap();
        assert!(!s.ais[0].state().profile.makes_units(), "{name}");
        let f = go(&s, s.ticks);
        // The pinned checksum is the one it always had, and the AI sent nothing of WP9.1c: no rally point, no
        // order of units, no unit but a harvester; the state of the new parts is at its start values.
        assert_eq!(f.outcome.final_checksum, expected, "{name}");
        for (t, _, c) in &f.ai_commands {
            let ok = match c {
                Command::SetRally { .. } | Command::Order { .. } => false,
                Command::QueueUnit { kind, .. } => *kind == HARVESTER,
                _ => true,
            };
            assert!(ok, "{name}, tick {t}: {c:?}");
        }
        let fresh = AiState::new(
            P0,
            f.ais[0].state().profile.clone(),
            0,
            f.ais[0].state().start_sites.clone(),
        );
        let s0 = f.ais[0].state();
        assert_eq!(
            (
                s0.gather,
                &s0.rally_backoff,
                &s0.units_sent,
                s0.scout,
                s0.scouts_sent,
                &s0.sent,
                s0.directive
            ),
            (
                fresh.gather,
                &fresh.rally_backoff,
                &fresh.units_sent,
                fresh.scout,
                fresh.scouts_sent,
                &fresh.sent,
                fresh.directive
            ),
            "{name}"
        );
        // WP9.1c-2: the threat and attack state too (the threat rule is off with the managers).
        assert_eq!(
            (
                &s0.hp_seen,
                s0.threat,
                &s0.attackers,
                s0.attacks_launched,
                &s0.sites_cleared
            ),
            (
                &fresh.hp_seen,
                fresh.threat,
                &fresh.attackers,
                fresh.attacks_launched,
                &fresh.sites_cleared
            ),
            "{name}"
        );
        // The cause of the re-pin, measured: the same scenario with the unit mix the profile had before WP9.1c-1
        // (the managers on) ends on another checksum. (Cause: units are bought, rally points and gather orders
        // are sent.)
        let on = with_profile(s.clone(), |p| {
            p.unit_mix = vec![(RIFLEMAN, 4), (ROCKET, 2), (TANK, 3)];
        });
        let moved = go(&on, s.ticks).outcome.final_checksum;
        assert_ne!(moved, expected, "{name}");
    }
}

// ---- how big the army is at the gather cell --------------------------------------------------------------

/// The units of player 0 that are army (not a harvester, not the scout) within the gather radius of its gather
/// cell: what `army::is_army` and `gather_radius` call arrived.
fn gathered(w: &World, ai: &Ai) -> usize {
    let g = ai.state().gather.expect("a gather cell");
    let r = u32::from(ai.state().profile.army.gather_radius);
    w.snapshot()
        .iter()
        .filter(|v| v.owner == P0 && w.footprint_of(v.id).is_none())
        .filter(|v| v.kind != HARVESTER)
        .filter(|v| ai.state().scout != Some(v.id))
        .filter_map(|v| w.cell_at(v.pos))
        .filter(|c| c.x.abs_diff(g.x).max(c.y.abs_diff(g.y)) <= r)
        .count()
}

#[test]
fn the_army_at_the_gather_cell_by_tick_on_the_first_matchs_map() {
    let s = load("ai_scout");
    let mut at = BTreeMap::new();
    let f = run_observed(
        &s,
        &RunOptions {
            ticks: 20_000,
            every: 100,
        },
        |w, ais| {
            let t = w.tick().0;
            if [4000, 8000, 12000, 16000, 20000].contains(&t) {
                at.insert(t, gathered(w, &ais[0]));
            }
        },
    )
    .unwrap();
    let first_army = sent(
        &f,
        |c| matches!(c, Command::QueueUnit { kind, .. } if [RIFLEMAN, ROCKET, TANK].contains(kind)),
    );
    let builds = sent(&f, |c| matches!(c, Command::BuildStructure { .. }));
    println!(
        "army at the gather cell: {at:?}; first army unit queued at {:?}; the last structure of the list ordered at {:?}",
        first_army.first(),
        builds.last()
    );
    assert_eq!(builds.last(), Some(&LIST_DONE_TICK));
    // Measured. The goal list (ten entries, 7,700 credits of structures at list prices and a third power plant,
    // two more harvesters) takes the 4,000 credits and the ore the harvesters bring until its last structure is
    // ordered at tick 14,610, and production holds back the next goal's price, so the first army unit is queued
    // at tick 15,020 and there is no army at 4,000, 8,000 or 12,000.
    assert_eq!(
        at.values().copied().collect::<Vec<_>>(),
        ARMY_AT_GATHER,
        "{at:?}"
    );
    assert_eq!(first_army.first(), Some(&FIRST_ARMY_TICK));
}

/// The army at the gather cell at ticks 4,000, 8,000, 12,000, 16,000 and 20,000 of `ai_scout`.
/// WP9.1c-2: 10 at 20,000 became 4, because an attack went out (the frozen profile of `ai_scout` has no army
/// floor, so the list and the timing of 9.1c-1 hold until then).
const ARMY_AT_GATHER: [usize; 5] = [0, 0, 0, 2, 4];
const FIRST_ARMY_TICK: u64 = 15_020;
/// The tick of the last `BuildStructure` of the goal list (the eleventh: a third power plant comes by the
/// power rule): the list is done and the holdback ends.
const LIST_DONE_TICK: u64 = 14_610;

// ---- the fence --------------------------------------------------------------------------------------------

/// `ai_scout` with three Veyran riflemen standing at `cells`.
fn with_riflemen_at(cells: [(u32, u32); 3]) -> Scenario {
    let mut s = load("ai_scout");
    for cell in cells {
        s.commands.push(TimedCommand {
            tick: 0,
            player: 1,
            command: Action::SpawnUnit {
                kind: 12,
                owner: 1,
                cell,
            },
        });
    }
    s
}

#[test]
fn an_army_the_ai_has_not_seen_changes_nothing_it_does() {
    // The same three riflemen in two far corners of the map, which neither the scout's legs (sight 8) nor the
    // base reach: the worlds differ, and the AI's commands, memory and digest do not. (The units are in both
    // worlds, so that the slots of everything the AI builds are the same: the harvest loop times itself by slot, and
    // a world with three more entities delivers its first ore later, which the AI would rightly answer.)
    let a = go(&with_riflemen_at([(90, 90), (91, 90), (90, 91)]), 12_000);
    let b = go(&with_riflemen_at([(90, 6), (91, 6), (90, 7)]), 12_000);
    assert_eq!(a.world.len(), b.world.len());
    assert_ne!(a.world.checksum(), b.world.checksum());
    assert!(!a.ai_commands.is_empty());
    assert_eq!(a.ai_commands, b.ai_commands);
    assert_eq!(
        a.ais[0].state().known_structures,
        b.ais[0].state().known_structures
    );
    assert_eq!(a.ais[0].digest(), b.ais[0].digest());
    // The control: the AI does see units that come into the scout's sight. A rifleman of player 1 beside the
    // last leg's waypoint (72, 72) is in the observation (armed flag included) once the scout stands there.
    let mut seen_at = None;
    let s = with_riflemen_at([(90, 90), (91, 90), (74, 70)]);
    let c = run_observed(
        &s,
        &RunOptions {
            ticks: 12_000,
            every: 100,
        },
        |w, ais| {
            if seen_at.is_none() && w.tick().0.is_multiple_of(10) {
                let obs = Observation::build(w, P0, &ais[0].state().known_structures);
                if obs.visible.iter().any(|e| e.kind == RIFLEMAN && e.armed) {
                    seen_at = Some(w.tick().0);
                }
            }
        },
    )
    .unwrap();
    println!("a rifleman beside the last waypoint is in the observation from tick {seen_at:?}");
    let t = seen_at.expect("the scout comes within sight of a unit beside its last waypoint");
    assert!((4870..12_000).contains(&t), "{t}");
    assert_ne!(c.world.checksum(), a.world.checksum());
}

/// Ore delivered by player 0 at ticks 2,000, 4,000, ... 20,000 of `s`, as the difference per window.
fn ore_windows(s: &Scenario) -> Vec<u64> {
    let mut delivered = vec![0u64];
    run_observed(s, &RunOptions::for_scenario(s), |w, _| {
        if w.tick().0 % 2000 == 0 {
            delivered.push(w.ore_delivered(P0));
        }
    })
    .unwrap();
    delivered.windows(2).map(|p| p[1] - p[0]).collect()
}

#[test]
fn a_rally_point_on_the_vehicle_factory_costs_ore_so_the_ai_gives_it_none() {
    // The vehicle factory (id 9) makes the harvesters the economy buys; a harvester with a rally point walks
    // to the gather cell first. The AI therefore sets no rally point there (`makes_harvesters`). Baseline,
    // measured: the same scenario with the point set by hand at tick 2,100 (the AI sees it is right and leaves
    // it) delivers 8,330 ore in 20,000 ticks, the AI's own 9,800: the point costs 15 percent. (The rule: more
    // than 10 percent, no rally on a factory that can make a harvester.)
    let s = load("ai_scout");
    let mut with = s.clone();
    with.commands.push(TimedCommand {
        tick: 2100,
        player: 0,
        command: Action::SetRally {
            factory: (9, 0),
            cell: Some((24, 64)),
        },
    });
    let (own, forced) = (ore_windows(&s), ore_windows(&with));
    println!(
        "delivered per 2,000 ticks: the AI's rule {own:?}, rally forced on the vehicle factory {forced:?}"
    );
    let (a, b): (u64, u64) = (own.iter().sum(), forced.iter().sum());
    assert_eq!((a, b), (9800, 8330));
    assert!(b * 100 < a * 90, "the point costs more than 10 percent");
    // The AI's own windows: every one is at least 150, and the pinned values.
    assert!(own.iter().all(|&n| n >= 150), "{own:?}");
    assert_eq!(own, [200, 600, 400, 1200, 800, 1160, 1840, 1400, 1210, 990]);
    // And the AI sent no rally point to the vehicle factory.
    let f = go(&s, 5000);
    assert!(
        !f.ai_commands.iter().any(|(_, _, c)| matches!(c,
            Command::SetRally { factory, .. } if factory.index() == 9)),
        "no rally on the vehicle factory"
    );
}

// ---- fix round 1: the gather cell and the AI's own placement ------------------------------------------------

#[test]
fn the_gather_cell_and_its_eight_neighbours_are_kept_free_of_the_ais_own_structures() {
    // The best place for a power plant on the world of `ai_scout` after one tick is some cell c. With the
    // gather cell on the middle of that plant, the search must put it elsewhere, off the 3 x 3 cells round the
    // gather cell.
    let (world, state) = rig();
    let obs = Observation::build(&world, P0, &BTreeMap::new());
    let mut lens = Lens::new(&world, P0);
    let best = sf_ai::placement::search(&mut state.clone(), &obs, &mut lens, PLANT).unwrap();
    let mut with_gather = state.clone();
    let g = Cell::new(best.x + 1, best.y + 1);
    with_gather.gather = Some(g);
    let got = sf_ai::placement::search(&mut with_gather, &obs, &mut lens, PLANT).unwrap();
    assert_ne!(got, best);
    // The plant is 2 x 3 cells at `got`; the keep-out is x g-1..=g+1, y g-1..=g+1.
    let (w, h) = (2, 3);
    let apart = got.x + w < g.x || got.x > g.x + 1 || got.y + h < g.y || got.y > g.y + 1;
    assert!(
        apart,
        "the plant at {got:?} covers part of the 3 x 3 round {g:?}"
    );
    // A gather cell elsewhere changes nothing (the same best place).
    let mut far = state;
    far.gather = Some(Cell::new(60, 60));
    assert_eq!(
        sf_ai::placement::search(&mut far, &obs, &mut lens, PLANT),
        Some(best)
    );
}

#[test]
fn a_gather_cell_under_a_structure_is_worked_out_again() {
    let (world, mut state) = rig();
    let lens = Lens::new(&world, P0);
    let g = Cell::new(24, 64);
    assert!(lens.ground(g));
    // Own structure on the kept cell (a plant of 2 x 3 at (23, 63) covers (24, 64)).
    let mut own = base(&world);
    own.push(structure_at(&world, 9, PLANT, 23, 63));
    own.sort_by_key(|o| o.id);
    let obs = observation(&world, 100, 0, own);
    state.gather = Some(g);
    // (Every cell of the rings is within the lane of that plant, so there is no new cell: `None`, again at
    // the next turn. What matters is that the kept cell is not kept.)
    let new = army::ensure_gather(&mut state, &obs, &lens);
    assert_ne!(new, Some(g));
    assert_eq!(state.gather, new);
    // The same for a structure the AI has seen of another player, and one it remembers.
    let mut seen = observation(&world, 100, 0, base(&world));
    seen.visible.push(sf_ai::Seen {
        id: EntityId::new(50, 0),
        kind: PLANT,
        owner: P1,
        cell: Cell::new(23, 63),
        footprint: world.building_table().get(PLANT).map(|b| b.footprint),
        hp: 600,
        armed: false,
    });
    let mut s2 = rig().1;
    s2.gather = Some(g);
    assert_ne!(army::ensure_gather(&mut s2, &seen, &lens), Some(g));
    let mut s3 = rig().1;
    s3.gather = Some(g);
    s3.known_structures.insert(
        EntityId::new(51, 0),
        sf_ai::Known {
            kind: PLANT,
            owner: P1,
            anchor: Cell::new(23, 63),
            width: 2,
            height: 3,
            seen_tick: Tick(50),
        },
    );
    let plain = observation(&world, 100, 0, base(&world));
    assert_ne!(army::ensure_gather(&mut s3, &plain, &lens), Some(g));
    // Kept when nothing stands on it.
    let mut s4 = rig().1;
    s4.gather = Some(g);
    assert_eq!(army::ensure_gather(&mut s4, &plain, &lens), Some(g));
}

// ---- fix round 1: a scout the AI starts with, and a unit that dies in the verify window ---------------------

#[test]
fn an_unordered_scout_the_match_starts_with_is_the_first_scout() {
    let (world, mut state) = rig();
    short_list(&mut state);
    let mut own = base(&world);
    own.push(unit_at(20, SCOUT, true, 26, 66));
    own.sort_by_key(|o| o.id);
    let obs = observation(&world, 700, 10_000, own.clone());
    // Production, which runs first: with a scout alive the AI does not wait for one, it makes units.
    assert_eq!(
        queues(&production_turn(&world, &mut state, &obs)),
        [(3, 12)]
    );
    assert_eq!(state.scouts_sent, 0);
    // The army turn makes it the scout and counts it: it is the first of `scouts_max`.
    let out = army_turn(&world, &mut state, &obs);
    assert_eq!(state.scout, Some(EntityId::new(20, 0)));
    assert_eq!(state.scouts_sent, 1);
    assert_eq!(out, [move_order(&[20], 72, 14)]);
    // Lost, it is replaced once after the retry and then never again (max 2).
    let mut gone = base(&world);
    gone.sort_by_key(|o| o.id);
    let obs = observation(&world, 1000, 10_000, gone.clone());
    army_turn(&world, &mut state, &obs);
    assert_eq!(state.scout_gone_at, Some(Tick(1000)));
    let obs = observation(&world, 4000, 10_000, gone);
    assert_eq!(
        queues(&production_turn(&world, &mut state, &obs))
            .iter()
            .filter(|q| q.1 == 15)
            .count(),
        1
    );
    assert_eq!(state.scouts_sent, 2);
}

#[test]
fn an_order_whose_kind_lost_a_unit_meanwhile_is_not_read_as_refused() {
    let (world, _) = rig();
    let mut own = base(&world);
    own.push(soldier(20, 24, 64));
    own.sort_by_key(|o| o.id);
    let mut state = rig().1;
    short_list(&mut state);
    state.profile.army.scouts_max = 0;
    state.profile.unit_mix = vec![(RIFLEMAN, 1)];
    // A rifleman stands (id 20); a second is ordered.
    let obs = observation(&world, 5000, 10_000, own.clone());
    assert_eq!(
        queues(&production_turn(&world, &mut state, &obs)),
        [(3, 12)]
    );
    assert_eq!(
        state.units_sent[0].3, 1,
        "one rifleman stood before the order"
    );
    // 40 ticks later the order is in the barracks' queue, and the old rifleman is dead: the live count of
    // the kind (units plus queue) is 1, as before, but the order was accepted.
    let mut after = base(&world);
    after[3] = with_queue(after[3].clone(), &[RIFLEMAN], Some(gather()));
    let obs = observation(&world, 5040, 10_000, after);
    production_turn(&world, &mut state, &obs);
    assert!(state.blocked_until.is_empty(), "{:?}", state.blocked_until);
}

#[test]
fn a_scout_that_entered_a_queue_is_never_taken_back() {
    let (world, mut state) = rig();
    short_list(&mut state);
    let own = base(&world);
    let obs = observation(&world, 700, 10_000, own.clone());
    assert_eq!(
        queues(&production_turn(&world, &mut state, &obs)),
        [(3, 15)]
    );
    // It is in the queue at the next turn: accepted, `scouts_sent` stays 1 ...
    let mut queued = own.clone();
    queued[3] = with_queue(queued[3].clone(), &[SCOUT], Some(gather()));
    let obs = observation(&world, 740, 10_000, queued);
    production_turn(&world, &mut state, &obs);
    assert_eq!(state.scouts_sent, 1);
    // ... and when it is lost at once (no unit, no item at the turn after), it stays counted: the latch of
    // the order is gone, nothing takes it back, and the replacement waits for `scout_retry`.
    let obs = observation(&world, 780, 10_000, own.clone());
    production_turn(&world, &mut state, &obs);
    assert_eq!(state.scouts_sent, 1);
    assert!(!state.blocked_until.contains_key(&SCOUT));
    // The scout came out and died within the 40 ticks: no item in the queue, no unit, but the order had
    // been seen in the queue at the turn before. (A scout that came out and is alive: a unit more.)
    let mut s2 = rig().1;
    short_list(&mut s2);
    let obs = observation(&world, 700, 10_000, own.clone());
    production_turn(&world, &mut s2, &obs);
    let mut out = own;
    out.push(unit_at(20, SCOUT, true, 26, 66));
    out.sort_by_key(|o| o.id);
    let obs = observation(&world, 740, 10_000, out);
    production_turn(&world, &mut s2, &obs);
    assert_eq!((s2.scouts_sent, s2.blocked_until.len()), (1, 0));
}

#[test]
fn what_the_scouts_400_credits_cost_the_goal_list() {
    // Measured, `ai_scout`: the tick of the last structure of the goal list with the scout and without one.
    let last_build = |s: &Scenario| {
        let f = go(s, s.ticks);
        *sent(&f, |c| matches!(c, Command::BuildStructure { .. }))
            .last()
            .unwrap()
    };
    let with = last_build(&load("ai_scout"));
    let without = last_build(&with_profile(load("ai_scout"), |p| p.army.scouts_max = 0));
    println!("last goal structure ordered at {with} with the scout, at {without} without");
    assert_eq!(with, LIST_DONE_TICK);
    assert_eq!((with, without), (LIST_WITH_SCOUT, LIST_WITHOUT_SCOUT));
}

const LIST_WITH_SCOUT: u64 = 14_610;
const LIST_WITHOUT_SCOUT: u64 = 12_650;

#[test]
fn the_ai_scout_repin_of_wp9_1c_2_is_the_attack() {
    // The cause of the re-pin, measured: `ai_scout` with the attack trigger made unreachable (no value reaches
    // it, `max_wait` beyond the run) ends on the checksum it had in WP9.1c-1. So the move is the attack and what
    // follows it; the threat rule and the new state send nothing else here.
    let s = with_profile(load("ai_scout"), |p| {
        p.army.attack_value = 1_000_000;
        p.army.max_wait = 72_000;
    });
    let f = go(&s, s.ticks);
    assert_eq!(f.outcome.final_checksum, 0xf2be_5300_2209_e2f6);
    assert!(f.ai_commands.iter().all(|c| !matches!(
        c.2,
        Command::Order {
            order: Order::AttackMove(_) | Order::Attack(_),
            ..
        }
    )));
    assert_ne!(
        f.outcome.final_checksum,
        parse_checksum(SCOUT_EXPECTED).unwrap()
    );
}
