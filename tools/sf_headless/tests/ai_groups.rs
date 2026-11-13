//! WP9.2a, the army in groups (`docs/rules/ai.md`, "Groups"): the home guard, staging, reinforcement, partial
//! defence, the chunking of a march command and the timing jitter, one test per rule on an observation the test
//! builds (the profile of `ai_vs_passive` with the numbers of WP9.2a switched on one by one), the invariant that a
//! unit is in at most one group over a long run, and a real game where the army is out and the guard answers a
//! raid (`ai_raid_guard`).

mod common;

use std::collections::BTreeMap;

use common::scenario_path;
use sf_ai::army;
use sf_ai::state::{GROUPS_MAX, GroupStage};
use sf_ai::{Ai, AiState, Lens, Observation, Own, OwnFactory, Seen};
use sf_headless::run::{Finished, RunOptions, run_full, run_observed};
use sf_headless::scenario::Scenario;
use sf_math::{Fx, Vec2};
use sf_sim::{Cell, Command, EntityId, FactoryStatus, Kind, Order, PlayerId, Tick, World};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const CENTRE: Kind = Kind(0);
const PLANT: Kind = Kind(1);
const BARRACKS: Kind = Kind(3);
const HARVESTER: Kind = Kind(11);
const RIFLEMAN: Kind = Kind(12);

fn load(name: &str) -> Scenario {
    Scenario::load(&scenario_path(&format!("{name}.ron"))).unwrap()
}

fn go(s: &Scenario, ticks: u64) -> Finished {
    run_full(s, &RunOptions { ticks, every: 100 }).unwrap()
}

fn middle(c: Cell) -> Vec2 {
    let half = Fx::from_ratio(1, 2);
    Vec2::new(
        Fx::from_i64(i64::from(c.x)) + half,
        Fx::from_i64(i64::from(c.y)) + half,
    )
}

fn gather() -> Cell {
    Cell::new(24, 64)
}

/// The world of `ai_vs_passive` after one tick and the AI's state, with the gather cell set and no scout, and
/// every number of WP9.2a at 0 (the frozen profile): a test switches on the one it tests.
fn army_state() -> (World, AiState) {
    let f = go(&load("ai_vs_passive"), 1);
    let mut s = f.ais[0].state().clone();
    s.gather = Some(gather());
    s.profile.scout = None;
    let a = &s.profile.army;
    assert!(
        !a.groups_on(),
        "the frozen profile has the numbers of WP9.2a off"
    );
    ((*f.world), s)
}

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

fn soldier(id: u32, x: u32, y: u32) -> Own {
    Own {
        id: EntityId::new(id, 0),
        kind: RIFLEMAN,
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
        armed: true,
        mobile: true,
    }
}

/// A base whose barracks rallies to the gather cell: centre 0, plants 1 and 5, refinery 2, barracks 3, two
/// harvesters 10 and 11.
fn base(world: &World) -> Vec<Own> {
    let mut barracks = structure_at(world, 3, BARRACKS, 12, 60);
    barracks.factory = Some(OwnFactory {
        queue: Vec::new(),
        front_paid: 0,
        status: FactoryStatus::Building,
        rally: Some(gather()),
    });
    let harvester = |id, x| {
        let mut o = soldier(id, x, 80);
        o.kind = HARVESTER;
        o.armed = false;
        o
    };
    vec![
        structure_at(world, 0, CENTRE, 12, 68),
        structure_at(world, 1, PLANT, 18, 60),
        structure_at(world, 2, Kind(2), 18, 74),
        barracks,
        structure_at(world, 5, PLANT, 8, 60),
        harvester(10, 15),
        harvester(11, 16),
    ]
}

fn observation(world: &World, tick: u64, own: Vec<Own>) -> Observation {
    let mut obs = Observation::build(world, P0, &BTreeMap::new());
    obs.tick = Tick(tick);
    obs.credits = 0;
    obs.own = own;
    obs.own.sort_by_key(|o| o.id);
    obs
}

fn army_turn(world: &World, state: &mut AiState, obs: &Observation) -> Vec<Command> {
    let mut lens = Lens::new(world, P0);
    let mut out = Vec::new();
    army::turn(state, obs, &mut lens, &mut out);
    out
}

fn watch(world: &World, state: &mut AiState, obs: &Observation) {
    army::watch(state, obs, &Lens::new(world, P0));
}

/// `n` riflemen idle at the gather cell, ids from 20.
fn gathered_soldiers(n: u32) -> Vec<Own> {
    (0..n)
        .map(|i| soldier(20 + i, 24 + i % 6, 64 + i / 6))
        .collect()
}

fn rifleman_cost(world: &World) -> u64 {
    u64::from(Lens::new(world, P0).unit_cost(RIFLEMAN).unwrap())
}

/// The `AttackMove` commands: ids (indexes) and the point.
fn attack_moves(out: &[Command]) -> Vec<(Vec<u32>, Vec2)> {
    out.iter()
        .filter_map(|c| match c {
            Command::Order {
                ids,
                order: Order::AttackMove(p),
                ..
            } => Some((ids.iter().map(|i| i.index()).collect(), *p)),
            _ => None,
        })
        .collect()
}

fn ids(list: &[EntityId]) -> Vec<u32> {
    list.iter().map(|i| i.index()).collect()
}

/// An armed rifleman of player 1 that the AI sees at `(x, y)`.
fn seen_raider(id: u32, x: u32, y: u32, armed: bool) -> Seen {
    Seen {
        id: EntityId::new(id, 0),
        kind: RIFLEMAN,
        owner: P1,
        cell: Cell::new(x, y),
        footprint: None,
        hp: 100,
        armed,
    }
}

/// A state with `n` riflemen gathered (and the base), at tick 3000, with an attack value that the strike meets
/// exactly and the numbers `change` sets; returns the world, the state after the launch turn, the commands of it
/// and the own units.
fn launched(
    n: u32,
    attack_value_units: u64,
    change: impl FnOnce(&mut sf_data::ai_profile::ArmyRules),
) -> (World, AiState, Vec<Command>, Vec<Own>) {
    let (world, mut s) = army_state();
    let c = rifleman_cost(&world);
    s.profile.army.attack_value = (attack_value_units * c) as u32;
    change(&mut s.profile.army);
    let mut own = base(&world);
    own.extend(gathered_soldiers(n));
    let out = army_turn(&world, &mut s, &observation(&world, 3000, own.clone()));
    (world, s, out, own)
}

// ---- G1: the home guard --------------------------------------------------------------------------------------

#[test]
fn the_guard_holds_back_the_highest_ids_until_a_quarter_of_the_value_and_not_one_unit_less() {
    // Eight riflemen, ids 20 to 27: 250 permille of 8 c is 2 c, so the two highest ids (26, 27) stay home and the
    // strike is the other six.
    let (_, s, out, _) = launched(8, 6, |a| a.home_guard_permille = 250);
    assert_eq!(s.attacks_launched, 1);
    let g = &s.groups[0];
    assert_eq!(ids(&g.members), [20, 21, 22, 23, 24, 25]);
    assert_eq!(attack_moves(&out)[0].0, [20, 21, 22, 23, 24, 25]);
    // One permille more than two units' worth of eight: three are held (2 c * 1000 < 251 * 8 c = 2,008 c), so
    // the strike is five and a trigger of six units does not fire.
    let (_, s, out, _) = launched(8, 6, |a| a.home_guard_permille = 251);
    assert_eq!(s.attacks_launched, 0, "five units are worth less than six");
    assert!(s.groups.is_empty() && out.is_empty(), "{out:?}");
    let (_, s, _, _) = launched(8, 5, |a| a.home_guard_permille = 251);
    assert_eq!(ids(&s.groups[0].members), [20, 21, 22, 23, 24]);
    // The launch value is the strike's, not the gathered units'.
    let (world, s, _, _) = launched(8, 6, |a| a.home_guard_permille = 250);
    assert_eq!(s.groups[0].launch_value, 6 * rifleman_cost(&world));
}

#[test]
fn without_a_guard_the_whole_wave_goes_and_the_trigger_counts_the_whole_wave() {
    let (_, s, out, _) = launched(8, 8, |_| {});
    assert_eq!(ids(&s.groups[0].members), [20, 21, 22, 23, 24, 25, 26, 27]);
    assert_eq!(attack_moves(&out)[0].0.len(), 8);
    // The guard makes the same value trigger fail: eight units gathered, a strike of six.
    let (_, s, _, _) = launched(8, 8, |a| a.home_guard_permille = 250);
    assert_eq!(s.attacks_launched, 0);
}

#[test]
fn the_strike_after_the_guard_must_still_have_min_units() {
    // `max_wait` after the last attack with `min_units` 4: five gathered, a guard of 400 permille holds two
    // back, the strike of three is under `min_units`: no launch. With a guard of 200 permille one is held: four go.
    let run = |guard: u16| {
        let (world, mut s) = army_state();
        s.profile.army.attack_value = 1_000_000;
        s.profile.army.max_wait = 100;
        s.profile.army.home_guard_permille = guard;
        let mut own = base(&world);
        own.extend(gathered_soldiers(5));
        army_turn(&world, &mut s, &observation(&world, 3000, own));
        s
    };
    assert_eq!(run(400).attacks_launched, 0);
    let s = run(200);
    assert_eq!(s.attacks_launched, 1);
    assert_eq!(s.groups[0].members.len(), 4);
}

// ---- G2: staging ---------------------------------------------------------------------------------------------

#[test]
fn the_staging_cell_is_on_the_line_to_the_target_short_of_it_and_on_open_ground() {
    let (world, mut s) = army_state();
    let lens = Lens::new(&world, P0);
    let target = Cell::new(72, 17);
    // Off: no cell.
    assert_eq!(army::staging_cell(&s, &lens, gather(), target), None);
    s.profile.army.stage_distance = 14;
    let cell = army::staging_cell(&s, &lens, gather(), target).unwrap();
    // The line runs 48 cells east and 47 north (the larger axis is x): 14 short is the point 34 cells east and
    // 33 north; the cell is that point or ground within three cells of it.
    let (px, py) = (24 + 48 * 34 / 48, 64 - 47 * 34 / 48);
    assert!(
        cell.x.abs_diff(px) <= 3 && cell.y.abs_diff(py) <= 3,
        "{cell:?} vs ({px}, {py})"
    );
    assert!(lens.ground(cell));
    // A target no further than the distance: no staging.
    assert_eq!(
        army::staging_cell(&s, &lens, gather(), Cell::new(24 + 14, 64)),
        None
    );
    assert!(army::staging_cell(&s, &lens, gather(), Cell::new(24 + 15, 64)).is_some());
    // The distance is the profile's: 20 puts the cell further back.
    s.profile.army.stage_distance = 20;
    let back = army::staging_cell(&s, &lens, gather(), target).unwrap();
    assert!(back.x < cell.x && back.y > cell.y, "{back:?} {cell:?}");
}

/// The launch of `n` riflemen with staging on (distance 14).
fn staged(n: u32) -> (World, AiState, Vec<Own>, Cell) {
    let (world, s, _, own) = launched(n, 1, |a| {
        a.stage_distance = 14;
        a.stage_permille = 700;
        a.stage_patience = 600;
    });
    let stage = s.groups[0].staging.expect("a staging cell");
    (world, s, own, stage)
}

#[test]
fn a_strike_with_staging_first_goes_to_the_staging_cell_and_not_to_the_target() {
    let (world, mut s, _, own) = launched(6, 1, |a| a.stage_distance = 14);
    let g = s.groups[0].clone();
    let stage = g.staging.unwrap();
    assert_eq!(g.stage, GroupStage::Staging);
    // The first command was the launch turn's: re-run it on a fresh state to read it.
    let (world2, mut s2) = army_state();
    s2.profile.army.attack_value = rifleman_cost(&world2) as u32;
    s2.profile.army.stage_distance = 14;
    let mut own2 = base(&world2);
    own2.extend(gathered_soldiers(6));
    let out = army_turn(&world2, &mut s2, &observation(&world2, 3000, own2));
    let moves = attack_moves(&out);
    assert_eq!(moves.len(), 1, "{out:?}");
    assert_eq!(moves[0].1, middle(stage));
    assert_ne!(moves[0].1, middle(g.goal));
    // Marching on the way (not idle), nothing more is ordered and the stage stays.
    let walking: Vec<Own> = own
        .iter()
        .cloned()
        .map(|mut o| {
            if o.armed {
                o.order = Order::AttackMove(middle(stage));
                o.cell = Cell::new(40, 50);
            }
            o
        })
        .collect();
    let out = army_turn(&world, &mut s, &observation(&world, 3040, walking));
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(s.groups[0].stage, GroupStage::Staging);
}

/// `own` with the first `idle` of the strike's members idle at the stage cell and the others on their way.
fn at_stage(own: &[Own], members: &[EntityId], stage: Cell, idle: usize) -> Vec<Own> {
    own.iter()
        .cloned()
        .map(|mut o| {
            if let Some(i) = members.iter().position(|&m| m == o.id) {
                if i < idle {
                    o.cell = Cell::new(stage.x + (i as u32 % 3), stage.y + (i as u32 / 3));
                    o.order = Order::Idle;
                } else {
                    o.cell = Cell::new(40, 50);
                    o.order = Order::AttackMove(middle(stage));
                }
            }
            o
        })
        .collect()
}

#[test]
fn the_strike_leaves_the_staging_cell_at_seven_tenths_idle_and_not_one_member_less() {
    let (world, s0, own, stage) = staged(10);
    let members = s0.groups[0].members.clone();
    assert_eq!(members.len(), 10);
    // Six of ten idle (600 permille): it waits. Seven (700): it advances, and the idle ones are sent on.
    let mut s = s0.clone();
    let out = army_turn(
        &world,
        &mut s,
        &observation(&world, 3200, at_stage(&own, &members, stage, 6)),
    );
    assert_eq!(s.groups[0].stage, GroupStage::Staging);
    assert!(attack_moves(&out).iter().all(|m| m.1 == middle(stage)));
    let mut s = s0.clone();
    let out = army_turn(
        &world,
        &mut s,
        &observation(&world, 3200, at_stage(&own, &members, stage, 7)),
    );
    assert_eq!(s.groups[0].stage, GroupStage::Advancing);
    assert_eq!(s.groups[0].since, Tick(3200));
    let goal = middle(s.groups[0].goal);
    let moves = attack_moves(&out);
    assert_eq!(moves.len(), 1, "{out:?}");
    assert_eq!(moves[0].1, goal);
    assert_eq!(moves[0].0.len(), 7);
}

#[test]
fn the_dead_members_leave_the_staging_denominator() {
    let (world, s0, own, stage) = staged(10);
    let members = s0.groups[0].members.clone();
    // Four of the ten are dead: five idle of six living is 833 permille, enough; five idle of ten was not.
    let living: Vec<Own> = at_stage(&own, &members, stage, 6)
        .into_iter()
        .filter(|o| !members[6..].contains(&o.id))
        .collect();
    let mut s = s0.clone();
    army_turn(&world, &mut s, &observation(&world, 3200, living.clone()));
    assert_eq!(s.groups[0].members.len(), 6);
    assert_eq!(s.groups[0].stage, GroupStage::Advancing);
    // Control: the same five idle with all ten living wait.
    let mut s = s0.clone();
    army_turn(
        &world,
        &mut s,
        &observation(&world, 3200, at_stage(&own, &members, stage, 5)),
    );
    assert_eq!(s.groups[0].stage, GroupStage::Staging);
}

#[test]
fn the_strike_leaves_the_staging_cell_after_the_patience_whatever_its_members_do() {
    let (world, s0, own, stage) = staged(10);
    let members = s0.groups[0].members.clone();
    let none_there = at_stage(&own, &members, stage, 0);
    // 599 ticks after the launch (3,000): waiting; at 600: gone on.
    let mut s = s0.clone();
    army_turn(
        &world,
        &mut s,
        &observation(&world, 3599, none_there.clone()),
    );
    assert_eq!(s.groups[0].stage, GroupStage::Staging);
    let mut s = s0.clone();
    army_turn(&world, &mut s, &observation(&world, 3600, none_there));
    assert_eq!(s.groups[0].stage, GroupStage::Advancing);
}

// ---- G3: reinforcement -----------------------------------------------------------------------------------------

/// A strike of six staging (value 6 c, a guard of 0) and `more` riflemen gathered at home, ids from 40;
/// reinforcement from `reinforce` units' worth. Returns the world, the state after the launch turn and the own
/// units of the next turn (the strike's members still at the gather cell, on their way).
fn with_a_strike(reinforce: u64) -> (World, AiState, Vec<Own>) {
    let (world, mut s, _, own) = launched(6, 1, |a| a.stage_distance = 14);
    s.profile.army.reinforce_value = (reinforce * rifleman_cost(&world)) as u32;
    let members = s.groups[0].members.clone();
    let stage = s.groups[0].staging.unwrap();
    let mut own = own;
    for o in own.iter_mut().filter(|o| members.contains(&o.id)) {
        o.cell = Cell::new(40, 50);
        o.order = Order::AttackMove(middle(stage));
    }
    (world, s, own)
}

fn more_at_home(own: &[Own], n: u32) -> Vec<Own> {
    let mut v = own.to_vec();
    v.extend((0..n).map(|i| soldier(40 + i, 24 + i % 6, 66 + i / 6)));
    v
}

#[test]
fn units_at_home_worth_the_reinforce_value_follow_the_strike_as_a_second_group_not_one_unit_less() {
    let (world, s0, own) = with_a_strike(3);
    // Two at home: worth less than three: nothing.
    let mut s = s0.clone();
    let out = army_turn(
        &world,
        &mut s,
        &observation(&world, 3040, more_at_home(&own, 2)),
    );
    assert_eq!(s.groups.len(), 1, "{:?}", s.groups);
    assert!(attack_moves(&out).is_empty(), "{out:?}");
    // Three: a second group with the three, merging into the strike, going to the staging cell.
    let mut s = s0.clone();
    let out = army_turn(
        &world,
        &mut s,
        &observation(&world, 3040, more_at_home(&own, 3)),
    );
    assert_eq!(s.groups.len(), 2);
    let r = &s.groups[1];
    assert_eq!(ids(&r.members), [40, 41, 42]);
    assert_eq!(r.merge_into, Some(s.groups[0].id));
    assert_eq!(r.goal, s.groups[0].staging.unwrap());
    assert_eq!(r.launch_value, 3 * rifleman_cost(&world));
    let moves = attack_moves(&out);
    assert_eq!(moves.len(), 1, "{out:?}");
    assert_eq!(
        (moves[0].0.clone(), moves[0].1),
        (vec![40, 41, 42], middle(r.goal))
    );
    // The reinforcement is no attack: the counters stand.
    assert_eq!(s.attacks_launched, 1);
    assert_eq!(s.last_attack_tick, Tick(3000));
}

#[test]
fn at_most_one_reinforcement_is_out_and_a_third_group_is_never_made() {
    let (world, s0, own) = with_a_strike(3);
    let mut s = s0.clone();
    army_turn(
        &world,
        &mut s,
        &observation(&world, 3040, more_at_home(&own, 3)),
    );
    assert_eq!(s.groups.len(), 2);
    // Ten more arrive at home while the reinforcement is on its way (walking, not at the strike yet).
    let mut now = more_at_home(&own, 3);
    for o in now.iter_mut().filter(|o| (40..43).contains(&o.id.index())) {
        o.cell = Cell::new(30, 60);
        o.order = Order::AttackMove(middle(s.groups[1].goal));
    }
    now.extend((0..10).map(|i| soldier(50 + i, 24 + i % 6, 67 + i / 6)));
    army_turn(&world, &mut s, &observation(&world, 3080, now));
    assert_eq!(s.groups.len(), 2, "no third group: {:?}", s.groups);
    const { assert!(GROUPS_MAX >= 2) };
}

#[test]
fn the_reinforcement_merges_into_the_strike_within_arrive_radius_of_its_staging_cell() {
    let (world, s0, own) = with_a_strike(3);
    let mut s = s0.clone();
    army_turn(
        &world,
        &mut s,
        &observation(&world, 3040, more_at_home(&own, 3)),
    );
    assert_eq!(s.groups.len(), 2);
    let stage = s.groups[0].staging.unwrap();
    let place = |near: bool| {
        let mut now = more_at_home(&own, 3);
        for o in now.iter_mut().filter(|o| (40..43).contains(&o.id.index())) {
            o.order = Order::Idle;
            // `arrive_radius` is 6: the middle cell 6 cells from the staging cell is in, 7 is out.
            o.cell = Cell::new(stage.x + if near { 6 } else { 7 }, stage.y);
        }
        now
    };
    // 7 cells away: still two groups.
    let mut far = s.clone();
    army_turn(&world, &mut far, &observation(&world, 3120, place(false)));
    assert_eq!(far.groups.len(), 2, "{:?}", far.groups);
    // 6 cells away: merged, the strike has the nine, one group is left, and the launch value adds up.
    army_turn(&world, &mut s, &observation(&world, 3120, place(true)));
    assert_eq!(s.groups.len(), 1, "{:?}", s.groups);
    assert_eq!(s.groups[0].members.len(), 9);
    assert_eq!(s.groups[0].launch_value, 9 * rifleman_cost(&world));
    assert_eq!(ids(&s.groups[0].members)[6..], [40, 41, 42]);
}

#[test]
fn a_strike_that_has_advanced_is_met_at_its_middle_cell() {
    let (world, s0, own) = with_a_strike(3);
    let mut s = s0.clone();
    army_turn(
        &world,
        &mut s,
        &observation(&world, 3040, more_at_home(&own, 3)),
    );
    // The strike left its cell and stands, idle, around (50, 40); the reinforcement walks to that middle cell.
    s.groups[0].stage = GroupStage::Advancing;
    let members = s.groups[0].members.clone();
    let mut now = more_at_home(&own, 3);
    for (i, o) in now.iter_mut().enumerate() {
        if members.contains(&o.id) {
            o.cell = Cell::new(50, 40);
            o.order = Order::AttackMove(middle(Cell::new(70, 20)));
        } else if (40..43).contains(&o.id.index()) {
            o.cell = Cell::new(50 + (i as u32 % 3), 44);
            o.order = Order::Idle;
        }
    }
    army_turn(&world, &mut s, &observation(&world, 3200, now));
    assert_eq!(s.groups.len(), 1, "merged at the strike's middle cell");
    assert_eq!(s.groups[0].members.len(), 9);
}

#[test]
fn no_reinforcement_while_the_strike_is_engaged_or_called_back_and_none_with_the_number_off() {
    let (world, s0, own) = with_a_strike(3);
    let now = more_at_home(&own, 6);
    // Engaged (a member within arrive_radius of the target): no second group.
    let mut s = s0.clone();
    s.groups[0].stage = GroupStage::Engaged;
    let goal = s.groups[0].goal;
    let mut engaged = now.clone();
    let first = s.groups[0].members[0];
    engaged.iter_mut().find(|o| o.id == first).unwrap().cell = goal;
    army_turn(&world, &mut s, &observation(&world, 3040, engaged));
    assert_eq!(s.groups.len(), 1);
    // Called back by a hit on the construction centre: the group is `Returning`, and nothing launches.
    let mut s = s0.clone();
    watch(&world, &mut s, &observation(&world, 3030, now.clone()));
    let mut hit = now.clone();
    hit[0].hp = 10;
    watch(&world, &mut s, &observation(&world, 3040, hit.clone()));
    assert_eq!(s.groups[0].stage, GroupStage::Returning);
    army_turn(&world, &mut s, &observation(&world, 3080, hit));
    assert_eq!(s.groups.len(), 1);
    assert_eq!(s.groups[0].stage, GroupStage::Returning);
    // The number 0: never.
    let mut s = s0.clone();
    s.profile.army.reinforce_value = 0;
    army_turn(&world, &mut s, &observation(&world, 3040, now));
    assert_eq!(s.groups.len(), 1);
}

#[test]
fn the_guard_is_not_part_of_a_reinforcement_either() {
    // Eight at home, a guard of 250 permille: the highest two stay, six go; reinforcement from six.
    let (world, mut s, _, own) = launched(6, 1, |a| a.stage_distance = 14);
    s.profile.army.reinforce_value = (6 * rifleman_cost(&world)) as u32;
    s.profile.army.home_guard_permille = 250;
    let members = s.groups[0].members.clone();
    let mut now = own;
    for o in now.iter_mut().filter(|o| members.contains(&o.id)) {
        o.cell = Cell::new(40, 50);
        o.order = Order::AttackMove(middle(s.groups[0].staging.unwrap()));
    }
    now.extend((0..8).map(|i| soldier(40 + i, 24 + i % 6, 66 + i / 6)));
    army_turn(&world, &mut s, &observation(&world, 3040, now));
    assert_eq!(s.groups.len(), 2);
    assert_eq!(ids(&s.groups[1].members), [40, 41, 42, 43, 44, 45]);
}

// ---- G4: partial defence ----------------------------------------------------------------------------------------

/// Ten riflemen at home at distances from the plant (18, 60) that rise with the id, one plant hit and the seen
/// raiders `visible`; returns the commands of the army turn after the hit.
fn defence(
    ratio: u32,
    visible: Vec<Seen>,
    change: impl FnOnce(&mut AiState),
) -> (AiState, Vec<Command>) {
    let (world, mut s) = army_state();
    s.profile.army.defend_ratio_permille = ratio;
    s.profile.army.attack_value = 1_000_000;
    change(&mut s);
    let mut own = base(&world);
    // Id 29 is the nearest to the threat, id 20 the farthest (x from 47 down to 29, y 62: all more than
    // `arrive_radius` from the cells the tests use).
    own.extend((0..10).map(|i| soldier(20 + i, 47 - 2 * i, 62)));
    watch(&world, &mut s, &observation(&world, 100, own.clone()));
    let mut hit = own.clone();
    hit[1].hp = 900;
    let mut o = observation(&world, 110, hit.clone());
    o.visible = visible.clone();
    watch(&world, &mut s, &o);
    assert!(s.threat.is_some());
    let mut o = observation(&world, 120, hit);
    o.visible = visible;
    let out = army_turn(&world, &mut s, &o);
    (s, out)
}

#[test]
fn the_defence_sends_units_worth_one_and_a_half_times_the_armed_enemies_seen_nearest_first() {
    // Two armed raiders seen near the plant: threat 2 c, ratio 1500: 3 c, the three nearest to the threat cell.
    let raiders = vec![seen_raider(40, 22, 61, true), seen_raider(41, 23, 61, true)];
    let (s, out) = defence(1500, raiders.clone(), |_| {});
    let cell = s.threat.unwrap().0;
    let moves = attack_moves(&out);
    assert_eq!(moves.len(), 1, "{out:?}");
    assert_eq!(moves[0].1, middle(cell));
    assert_eq!(moves[0].0.len(), 3, "{moves:?}");
    // The threat cell is the nearest raider's cell (22, 61), and the riflemen nearest to it are the highest ids
    // here (x 29, 31, 33): the order is the distance, not the id.
    assert_eq!(cell, Cell::new(22, 61));
    assert_eq!(moves[0].0, [27, 28, 29]);
    // Ratio 3000 (two times as much): six units.
    let (_, out) = defence(3000, raiders.clone(), |_| {});
    assert_eq!(attack_moves(&out)[0].0.len(), 6);
    // Ratio 0 is today's rule: all ten.
    let (_, out) = defence(0, raiders.clone(), |_| {});
    assert_eq!(attack_moves(&out)[0].0.len(), 10);
    // One permille above one raider's worth times the ratio: 1 c * 1500 / 1000 = 1.5 c: two units.
    let (_, out) = defence(1500, vec![seen_raider(40, 22, 61, true)], |_| {});
    assert_eq!(attack_moves(&out)[0].0.len(), 2);
}

#[test]
fn among_units_at_the_same_distance_the_lowest_id_goes_first() {
    // One raider worth one unit, ratio 1000: one unit. Ids 20 and 21 stand 7 cells east of the threat cell, one
    // row north and one south of it (the same squared distance); id 22 is farther. The lowest id goes.
    let (world, mut s) = army_state();
    s.profile.army.defend_ratio_permille = 1000;
    s.profile.army.attack_value = 1_000_000;
    let mut own = base(&world);
    own.push(soldier(21, 29, 60));
    own.push(soldier(20, 29, 62));
    own.push(soldier(22, 40, 62));
    watch(&world, &mut s, &observation(&world, 100, own.clone()));
    let mut hit = own.clone();
    hit[1].hp = 900;
    let raider = vec![seen_raider(40, 22, 61, true)];
    let mut o = observation(&world, 110, hit.clone());
    o.visible = raider.clone();
    watch(&world, &mut s, &o);
    assert_eq!(s.threat.unwrap().0, Cell::new(22, 61));
    let mut o = observation(&world, 120, hit);
    o.visible = raider;
    let out = army_turn(&world, &mut s, &o);
    assert_eq!(attack_moves(&out)[0].0, [20]);
}

#[test]
fn nothing_seen_sends_all_of_home_and_unarmed_or_far_enemies_do_not_count() {
    // No enemy in sight: the whole home goes.
    let (_, out) = defence(1500, vec![], |_| {});
    assert_eq!(attack_moves(&out)[0].0.len(), 10);
    // An unarmed one, and an armed one 20 cells off (gather_radius is 8): nothing counts, all go.
    let (_, out) = defence(
        1500,
        vec![
            seen_raider(40, 22, 61, false),
            seen_raider(41, 50, 61, true),
        ],
        |_| {},
    );
    assert_eq!(attack_moves(&out)[0].0.len(), 10);
}

#[test]
fn the_threat_value_reads_only_what_is_visible() {
    let (world, s) = army_state();
    let lens = Lens::new(&world, P0);
    let c = rifleman_cost(&world);
    let mut obs = observation(&world, 100, base(&world));
    let at = Cell::new(22, 61);
    assert_eq!(army::threat_value(&s, &obs, &lens, at), 0);
    obs.visible = vec![
        seen_raider(40, 22, 61, true),
        seen_raider(41, 30, 61, true),
        seen_raider(42, 31, 61, true),
    ];
    // Within 8 cells of (22, 61): the first two (30 is 8 away), not the third (9).
    assert_eq!(army::threat_value(&s, &obs, &lens, at), 2 * c);
}

#[test]
fn units_already_idle_at_the_threat_count_for_the_amount_but_are_not_ordered_again() {
    // The three nearest are already at the threat cell: they count as sent and nothing else goes.
    let raiders = vec![seen_raider(40, 22, 61, true), seen_raider(41, 23, 61, true)];
    let (world, mut s) = army_state();
    s.profile.army.defend_ratio_permille = 1500;
    s.profile.army.attack_value = 1_000_000;
    let mut own = base(&world);
    own.extend((0..10).map(|i| soldier(20 + i, 47 - 2 * i, 62)));
    watch(&world, &mut s, &observation(&world, 100, own.clone()));
    let mut hit = own.clone();
    hit[1].hp = 900;
    let mut o = observation(&world, 110, hit.clone());
    o.visible = raiders.clone();
    watch(&world, &mut s, &o);
    let cell = s.threat.unwrap().0;
    for u in hit.iter_mut().filter(|u| (21..24).contains(&u.id.index())) {
        u.cell = cell;
    }
    let mut o = observation(&world, 120, hit);
    o.visible = raiders;
    let out = army_turn(&world, &mut s, &o);
    assert!(attack_moves(&out).is_empty(), "{out:?}");
}

#[test]
fn only_a_hit_on_the_construction_centre_recalls_a_group_and_the_defence_stays_with_home() {
    // The plant is hit while a strike is out: the strike stays out, only home defends.
    let (world, mut s, _, own) = launched(6, 1, |a| {
        a.defend_ratio_permille = 1500;
    });
    assert_eq!(s.groups.len(), 1);
    let mut now = own.clone();
    now.extend((0..3).map(|i| soldier(40 + i, 24 + i, 66)));
    watch(&world, &mut s, &observation(&world, 3010, now.clone()));
    let mut hit = now.clone();
    hit[1].hp = 900;
    let mut o = observation(&world, 3020, hit.clone());
    o.visible = vec![seen_raider(60, 20, 62, true)];
    watch(&world, &mut s, &o);
    assert_eq!(s.groups[0].stage, GroupStage::Advancing);
    let out = army_turn(&world, &mut s, &o);
    let moves = attack_moves(&out);
    let sent: Vec<u32> = moves.iter().flat_map(|m| m.0.clone()).collect();
    assert!(sent.iter().all(|i| (40..43).contains(i)), "{moves:?}");
    // The centre (slot 0) hit: the group is called back.
    let mut centre = now;
    centre[0].hp = 10;
    watch(&world, &mut s, &observation(&world, 3030, centre));
    assert_eq!(s.groups[0].stage, GroupStage::Returning);
}

// ---- chunking --------------------------------------------------------------------------------------------------

#[test]
fn a_march_command_carries_at_most_gather_group_ids() {
    // 45 gathered units and an attack at 1 unit: the 45 go in commands of at most 19 (the shipped gather_group),
    // 19 + 19 + 7, one turn.
    let (_, s, out, _) = launched(45, 1, |a| {
        assert_eq!(a.gather_group, 19);
    });
    assert_eq!(s.groups[0].members.len(), 45);
    let moves = attack_moves(&out);
    let sizes: Vec<usize> = moves.iter().map(|m| m.0.len()).collect();
    assert_eq!(sizes, [19, 19, 7], "{out:?}");
    // Every id once, ascending across the commands.
    let all: Vec<u32> = moves.iter().flat_map(|m| m.0.clone()).collect();
    assert_eq!(all, (20..65).collect::<Vec<_>>());
    // The size is the profile's: 10 gives 10, 10, 10, 10 and the budget (4 commands) leaves 5 for the next turn.
    let (world, mut s, out, own) = launched(45, 1, |a| a.gather_group = 10);
    let sizes: Vec<usize> = attack_moves(&out).iter().map(|m| m.0.len()).collect();
    assert_eq!(sizes, [10, 10, 10, 10]);
    // The 5 not sent are still idle at the gather cell: the next turn orders them.
    let out = army_turn(&world, &mut s, &observation(&world, 3040, own));
    let sizes: Vec<usize> = attack_moves(&out).iter().map(|m| m.0.len()).collect();
    assert_eq!(sizes, [5], "{out:?}");
}

// ---- the timing jitter -----------------------------------------------------------------------------------------

fn seeded(base: &AiState, seed: u64, spread: u16) -> AiState {
    let mut profile = base.profile.clone();
    profile.army.timing_jitter_permille = spread;
    Ai::new(P0, profile, seed, base.start_sites.clone())
        .state()
        .clone()
}

#[test]
fn the_jitter_is_drawn_from_the_ais_own_stream_within_the_range_and_the_same_seed_gives_the_same_draw()
 {
    let (_, base) = army_state();
    let mut seen = std::collections::BTreeSet::new();
    for seed in 0..200u64 {
        let mut a = seeded(&base, seed, 100);
        let mut b = seeded(&base, seed, 100);
        army::draw_jitter(&mut a);
        army::draw_jitter(&mut b);
        let j = a.jitter.expect("drawn");
        assert!((-100..=100).contains(&j), "seed {seed}: {j}");
        assert_eq!(a.jitter, b.jitter, "seed {seed}");
        assert_eq!(a.digest(), b.digest());
        seen.insert(j);
    }
    // Both ends are reached over 200 seeds, and the draws are not one number.
    assert!(seen.len() > 50, "{} distinct draws", seen.len());
    assert!(seen.iter().any(|&j| j < -80) && seen.iter().any(|&j| j > 80));
    // Another stream (another player) draws another sequence: the player is in the stream.
    let mut p0 = seeded(&base, 7, 100);
    let mut p1 = seeded(&base, 7, 100);
    p1.player = PlayerId(1);
    p1.rng = sf_math::Rng::with_stream(7, sf_ai::state::AI_STREAM + 1);
    let draws = |s: &mut AiState| {
        (0..8)
            .map(|_| {
                s.jitter = None;
                army::draw_jitter(s);
                s.jitter.unwrap()
            })
            .collect::<Vec<_>>()
    };
    assert_ne!(draws(&mut p0), draws(&mut p1));
}

#[test]
fn the_jitter_is_drawn_once_per_launch_and_the_number_zero_never_touches_the_stream() {
    let (_, base) = army_state();
    let mut s = seeded(&base, 3, 100);
    let before = s.rng.clone();
    army::draw_jitter(&mut s);
    let first = s.jitter;
    let after = s.rng.clone();
    assert_ne!(before, after);
    // Drawn: asking again changes neither the value nor the stream.
    army::draw_jitter(&mut s);
    assert_eq!((s.jitter, s.rng.clone()), (first, after));
    // Off: no draw, the stream is untouched.
    let mut z = seeded(&base, 3, 0);
    let rng = z.rng.clone();
    army::draw_jitter(&mut z);
    assert_eq!((z.jitter, z.rng.clone()), (None, rng));
}

#[test]
fn the_draw_scales_attack_value_and_max_wait_of_that_launch_and_is_cleared_by_it() {
    let (world, mut s) = army_state();
    let c = rifleman_cost(&world);
    s.profile.army.attack_value = (10 * c) as u32;
    let mut own = base(&world);
    own.extend(gathered_soldiers(9));
    // Nine units gathered, a trigger of ten: no launch with no jitter; with a draw of -100 (a tenth less: nine c),
    // the launch goes, and with +100 it does not.
    s.profile.army.timing_jitter_permille = 100;
    for (draw, launches) in [(-100, true), (-99, false), (100, false), (0, false)] {
        let mut t = s.clone();
        t.jitter = Some(draw);
        // The draw is made already: the turn keeps it.
        army_turn(&world, &mut t, &observation(&world, 3000, own.clone()));
        assert_eq!(t.attacks_launched, u32::from(launches), "draw {draw}");
        if launches {
            // The launch cleared the draw for the next one.
            assert_eq!(t.jitter, None);
        }
    }
    // No draw yet: the turn draws one (the stream moves) when it checks the trigger.
    let mut t = s.clone();
    let rng = t.rng.clone();
    army_turn(&world, &mut t, &observation(&world, 3000, own.clone()));
    assert_ne!(t.rng, rng);
    // The same state again gives the same result: deterministic.
    let mut u = s.clone();
    army_turn(&world, &mut u, &observation(&world, 3000, own));
    assert_eq!(
        (t.jitter, t.attacks_launched),
        (u.jitter, u.attacks_launched)
    );
    assert_eq!(t.digest(), u.digest());
}

#[test]
fn the_draw_scales_max_wait_too() {
    let (world, mut s) = army_state();
    s.profile.army.attack_value = 1_000_000;
    s.profile.army.max_wait = 1000;
    s.profile.army.first_attack_tick = 0;
    s.profile.army.timing_jitter_permille = 200;
    let mut own = base(&world);
    own.extend(gathered_soldiers(5));
    // Last attack at 0: max_wait 1000 scaled by 800 permille is 800: a launch at 800, not at 799.
    for (tick, launches) in [(799, false), (800, true)] {
        let mut t = s.clone();
        t.jitter = Some(-200);
        army_turn(&world, &mut t, &observation(&world, tick, own.clone()));
        assert_eq!(t.attacks_launched, u32::from(launches), "tick {tick}");
    }
    let mut t = s.clone();
    t.jitter = Some(200);
    army_turn(&world, &mut t, &observation(&world, 1199, own.clone()));
    assert_eq!(t.attacks_launched, 0);
    army_turn(&world, &mut t, &observation(&world, 1200, own));
    assert_eq!(t.attacks_launched, 1);
}

// ---- the invariant ---------------------------------------------------------------------------------------------

/// No unit is in two groups, and the groups ascend by id and are at most `GROUPS_MAX`.
fn check_groups(s: &AiState, at: u64) {
    assert!(s.groups.len() <= GROUPS_MAX, "tick {at}");
    let mut seen = std::collections::BTreeSet::new();
    let mut last = None;
    for g in &s.groups {
        assert!(last.is_none_or(|l| l < g.id), "ids ascend at tick {at}");
        last = Some(g.id);
        assert!(g.members.windows(2).all(|w| w[0] < w[1]), "members ascend");
        for m in &g.members {
            assert!(seen.insert(*m), "unit {m:?} is in two groups at tick {at}");
        }
    }
}

#[test]
fn a_unit_is_in_at_most_one_group_in_every_army_turn_of_a_long_run() {
    // The shipped numbers on the scenario of `ai_vs_passive` (the AI wins at about 14,000 ticks), observed after
    // every tick.
    let s = {
        let mut s = load("ai_vs_passive");
        let old = s.ais[0].state().clone();
        let mut profile = old.profile.clone();
        profile.army = sf_data::ai_profile::AiProfile::embedded().army;
        s.ais[0] = Ai::new(old.player, profile, 0xA1_0001, old.start_sites);
        s
    };
    let mut out_at = None;
    let mut turns = 0u32;
    run_observed(
        &s,
        &RunOptions {
            ticks: 16_000,
            every: 100,
        },
        |world: &World, ais: &[Ai]| {
            let st = ais[0].state();
            let tick = world.tick().0;
            check_groups(st, tick);
            if !st.groups.is_empty() && out_at.is_none() {
                out_at = Some(tick);
            }
            turns += 1;
        },
    )
    .unwrap();
    assert!(out_at.is_some(), "a group went out");
    assert!(turns > 1000);
}

#[test]
fn the_invariant_holds_through_launch_merge_death_and_recall_on_built_observations() {
    let (world, mut s, _, own) = launched(8, 1, |a| {
        a.stage_distance = 14;
        a.reinforce_value = 100;
        a.home_guard_permille = 250;
    });
    check_groups(&s, 3000);
    let mut now = own;
    let mut tick = 3040;
    let mut next_id = 40;
    for round in 0..40u32 {
        // New units at home every other turn, one dies every third, everything idle half the time.
        if round % 2 == 0 {
            now.push(soldier(next_id, 24 + next_id % 6, 66));
            next_id += 1;
        }
        if round % 3 == 0
            && let Some(i) = now.iter().position(|o| o.armed && o.id.index() >= 20)
        {
            now.remove(i);
        }
        for o in now.iter_mut().filter(|o| o.armed) {
            o.order = if (round + o.id.index()) % 2 == 0 {
                Order::Idle
            } else {
                Order::AttackMove(middle(Cell::new(60, 30)))
            };
            if round % 5 == 0 {
                o.cell = Cell::new(40 + o.id.index() % 4, 50);
            }
        }
        if round == 25 {
            // The centre is hit.
            let mut hit = now.clone();
            hit[0].hp = 5;
            watch(&world, &mut s, &observation(&world, tick, now.clone()));
            watch(&world, &mut s, &observation(&world, tick + 10, hit));
        }
        army_turn(&world, &mut s, &observation(&world, tick, now.clone()));
        check_groups(&s, tick);
        tick += 40;
    }
}

// ---- the real game: the army is out and the guard kills the raiders -------------------------------------------

#[test]
fn ai_raid_guard_the_army_is_out_and_the_guard_kills_the_raiders() {
    let s = load("ai_raid_guard");
    let mut out_when_raided = false;
    let f = run_observed(
        &s,
        &RunOptions {
            ticks: s.ticks,
            every: 20,
        },
        |world: &World, ais: &[Ai]| {
            let st = ais[0].state();
            let tick = world.tick().0;
            check_groups(st, tick);
            if tick == 1700 {
                out_when_raided = !st.attackers().is_empty();
            }
        },
    )
    .unwrap();
    assert!(out_when_raided, "the strike is out when the raid comes");
    let st = f.ais[0].state();
    assert_eq!(st.attacks_launched, 1);
    // The raiders (player 1's riflemen) are dead at the end; the power plant (id 1) stands.
    let snap = f.world.snapshot();
    let raiders: Vec<_> = snap
        .iter()
        .filter(|v| v.owner == P1 && v.kind == RIFLEMAN)
        .collect();
    assert!(
        raiders.is_empty(),
        "the guard killed the raiders: {raiders:?}"
    );
    assert!(
        snap.iter().any(|v| v.id == EntityId::new(1, 0)),
        "the power plant stands"
    );
    // The strike was never recalled: only a hit on the centre does that.
    assert!(st.groups.iter().all(|g| g.stage != GroupStage::Returning));
}

/// The scenario `ai_raid_guard` with the AI's army numbers changed by `change`.
fn raid_guard_with(change: impl Fn(&mut sf_data::ai_profile::ArmyRules)) -> Finished {
    let mut s = load("ai_raid_guard");
    let old = s.ais[0].state().clone();
    let mut profile = old.profile.clone();
    change(&mut profile.army);
    s.ais[0] = Ai::new(old.player, profile, 0xA1_0001, old.start_sites);
    go(&s, s.ticks)
}

/// What is left of the raid at the end of `f`: (raiders alive, whether the power plant stands).
fn raid_outcome(f: &Finished) -> (usize, bool) {
    let snap = f.world.snapshot();
    (
        snap.iter()
            .filter(|v| v.owner == P1 && v.kind == RIFLEMAN)
            .count(),
        snap.iter().any(|v| v.id == EntityId::new(1, 0)),
    )
}

#[test]
fn ai_raid_guard_without_the_guard_the_raiders_win() {
    // The control of the scenario: no guard, no partial defence (the rules of before WP9.2a): the whole army is
    // out, nobody is at home, and the raiders are alive at the end, or the power plant is gone.
    let f = raid_guard_with(|a| {
        a.home_guard_permille = 0;
        a.defend_ratio_permille = 0;
    });
    let (raiders, plant) = raid_outcome(&f);
    assert!(raiders > 0 || !plant, "raiders {raiders}, plant {plant}");
    // And with the guard the same scenario ends with no raider and the plant standing.
    let f = raid_guard_with(|_| {});
    assert_eq!(raid_outcome(&f), (0, true));
}

// ---- the fence: what the AI has not seen changes nothing ---------------------------------------------------------

/// `ai_raid_guard` without its raid, with three Veyran riflemen standing at `cells` from tick 0.
fn guard_scenario_with_riflemen_at(cells: [(u32, u32); 3]) -> Scenario {
    let mut s = load("ai_raid_guard");
    s.commands.clear();
    for cell in cells {
        s.commands.push(sf_headless::scenario::TimedCommand {
            tick: 0,
            player: 1,
            command: sf_headless::scenario::Action::SpawnUnit {
                kind: 12,
                owner: 1,
                cell,
            },
        });
    }
    s
}

#[test]
fn an_unseen_army_beside_the_route_changes_nothing_the_groups_do_until_it_is_seen() {
    // The strike of `ai_raid_guard` (guard, staging, and the march after it) passes the cells (57, 28) and
    // (58, 28), beside its staging cell. Three enemy riflemen there, against the same three in a far corner: the
    // commands and the AI digest are equal until the cell comes into view of the AI.
    let probe = Cell::new(57, 28);
    let near = guard_scenario_with_riflemen_at([(57, 28), (58, 28), (57, 29)]);
    let mut seen_at = None;
    run_observed(
        &near,
        &RunOptions {
            ticks: near.ticks,
            every: 100,
        },
        |w: &World, _| {
            if seen_at.is_none() && w.visibility(P0, probe) == sf_sim::Visibility::Visible {
                seen_at = Some(w.tick().0);
            }
        },
    )
    .unwrap();
    let until = seen_at.expect("the riflemen come into view") - 1;
    // Before they are seen the strike has gone out, staged and set off: the rules ran.
    assert!(until > 900, "seen at {until}");
    let a = go(&near, until);
    let b = go(
        &guard_scenario_with_riflemen_at([(90, 90), (91, 90), (90, 91)]),
        until,
    );
    assert_eq!(a.world.len(), b.world.len());
    assert_ne!(a.world.checksum(), b.world.checksum());
    assert!(a.ais[0].state().attacks_launched >= 1);
    assert_eq!(a.ai_commands, b.ai_commands);
    assert_eq!(a.ais[0].digest(), b.ais[0].digest());
}

#[test]
fn the_threat_value_and_the_defence_ignore_an_armed_enemy_that_is_not_visible() {
    // Three enemy riflemen 8 cells north of the power plant (18, 60), out of sight of everything the AI has: the
    // threat value there is 0 and `defenders` sends all of home, as with no enemy at all. The same three beside the
    // plant are seen, and then they count.
    let at = Cell::new(18, 60);
    let value = |cells: [(u32, u32); 3]| {
        let s = guard_scenario_with_riflemen_at(cells);
        let f = go(&s, 20);
        let obs = Observation::build(&f.world, P0, &BTreeMap::new());
        let lens = Lens::new(&f.world, P0);
        let state = f.ais[0].state().clone();
        let seen = army::threat_value(&state, &obs, &lens, at);
        let home: Vec<&Own> = obs.own.iter().filter(|o| o.armed && o.mobile).collect();
        let mut with_ratio = state.clone();
        with_ratio.profile.army.defend_ratio_permille = 1500;
        let sent = army::defenders(&with_ratio, &obs, &lens, at, &home);
        (seen, sent.len(), home.len(), obs.visible.len())
    };
    let c = {
        let (w, _) = army_state();
        rifleman_cost(&w)
    };
    let (seen, sent, home, visible) = value([(18, 52), (19, 52), (20, 52)]);
    assert_eq!((seen, visible), (0, 0), "nothing of them is visible");
    assert_eq!(sent, home, "nothing seen: all of home goes");
    let (seen, sent, home, visible) = value([(20, 62), (21, 62), (22, 62)]);
    assert_eq!(visible, 3);
    assert_eq!(seen, 3 * c);
    assert!(
        sent < home,
        "seen: only 1.5 times their value goes ({sent} of {home})"
    );
    assert_eq!(sent, 5, "4.5 units' worth rounds up to five riflemen");
}

// ---- ai_vs_passive with the shipped numbers --------------------------------------------------------------------

/// `ai_vs_passive` with the AI's army numbers those of the shipped profile.
fn vs_passive_with_the_shipped_army() -> Scenario {
    let mut s = load("ai_vs_passive");
    let old = s.ais[0].state().clone();
    let mut profile = old.profile.clone();
    profile.army = sf_data::ai_profile::AiProfile::embedded().army;
    s.ais[0] = Ai::new(old.player, profile, 0xA1_0001, old.start_sites);
    s
}

/// The tick the match names the AI the winner in `ai_vs_passive` with the numbers off (13,839, pinned in
/// `ai_army.rs`) and with the shipped numbers.
const VS_PASSIVE_WON_OFF: u64 = 13_839;
const VS_PASSIVE_WON_SHIPPED: u64 = 15_908;

#[test]
fn ai_vs_passive_still_wins_with_the_shipped_numbers_and_the_tick_is_pinned() {
    let s = vs_passive_with_the_shipped_army();
    let mut won = None;
    let mut waves = Vec::new();
    let mut launched = 0;
    run_observed(
        &s,
        &RunOptions {
            ticks: s.ticks,
            every: 100,
        },
        |w: &World, ais: &[Ai]| {
            let st = ais[0].state();
            if st.attacks_launched > launched {
                launched = st.attacks_launched;
                let g = &st.groups[0];
                waves.push((w.tick().0 - 1, g.members.len(), g.launch_value));
            }
            if won.is_none()
                && let Some(r) = w.match_result()
                && r.verdict == sf_sim::Verdict::Won(P0)
            {
                won = Some(r.tick.0);
            }
        },
    )
    .unwrap();
    println!(
        "ai_vs_passive, shipped numbers: won at {won:?}, waves {waves:?} (numbers off: {VS_PASSIVE_WON_OFF})"
    );
    let won = won.expect("the AI wins");
    assert!(won < s.ticks, "{won}");
    assert_eq!(won, VS_PASSIVE_WON_SHIPPED);
}
