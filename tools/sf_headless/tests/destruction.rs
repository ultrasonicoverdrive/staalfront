//! The `destruction` scenario (WP5.5, `docs/rules/destruction.md`): two stories on the map of the `pathing`
//! scenario, with explicit attack orders only.
//!
//! * **The gap.** Structures close the two gaps of a rock wall. Three tanks shoot the two structures of the first
//!   gap down (ticks 372 and 772): their cells are free and the routes over them are rebuilt, a rifleman for
//!   whom the east bank was a long way round the lake walks through them, and the rubble lies where the
//!   structures stood and goes after 1200 ticks. The second gap stays shut.
//! * **The wreck.** Two tanks and two riflemen against four rocket troopers: the first tank is destroyed on tick
//!   71 and leaves a wreck that goes on tick 671, a trooper that falls leaves nothing, and the credit goes by
//!   the damage of the killing tick.
//!
//! The cast and the commands are written in `assets/scenarios/destruction.ron` (which also lists the entity
//! ids used below), on a frozen copy of the shipped definitions. The checksum is pinned in
//! `assets/scenarios/destruction.expected` (see `pinned.rs`).

mod common;

use common::scenario_path;
use sf_headless::cli::parse_checksum;
use sf_headless::scenario::Scenario;
use sf_math::{Fx, Vec2};
use sf_sim::{Cell, Death, EntityId, Order, PlayerId, Shot, Tick, World, WreckKind};

const EXPECTED: &str = include_str!("../../../assets/scenarios/destruction.expected");

const ENTITIES: u32 = 16;
/// The structures of the first gap, and of the second.
const FIRST_GAP: [u32; 2] = [0, 1];
const RIFLEMAN: u32 = 7;
const LIGHT_TANK: u16 = 19;
const ROCKET_TROOPER: u16 = 13;
const RELAY: u16 = 32;
/// The simulation's own settings: a vehicle wreck lies 600 ticks, rubble 1200.
const WRECK_TICKS: u64 = 600;
const RUBBLE_TICKS: u64 = 1200;

fn id(slot: u32) -> EntityId {
    EntityId::new(slot, 0)
}

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("destruction.ron")).unwrap()
}

/// What a run reported, tick by tick.
struct Run {
    world: World,
    shots: Vec<Shot>,
    deaths: Vec<Death>,
    /// The occupied cells at the end of each tick.
    occupied: Vec<usize>,
    /// `(kind, anchor)` of the wrecks and rubble lying at the end of each tick.
    wrecks: Vec<Vec<(WreckKind, u16, Option<Cell>)>>,
    /// Where the rifleman was at the end of each tick (where it was last, once it is gone).
    rifleman: Vec<Vec2>,
    /// The cells of the rifleman.
    rifleman_cells: Vec<Option<Cell>>,
    /// Slots in the world at the end of each tick.
    gone: Vec<Vec<bool>>,
    checksums: Vec<u64>,
}

fn run_scenario(scenario: &Scenario) -> Run {
    let mut world = scenario.build_world().unwrap();
    let mut r = Run {
        world: World::new(sf_sim::WorldConfig::new(1)).unwrap(),
        shots: Vec::new(),
        deaths: Vec::new(),
        occupied: Vec::new(),
        wrecks: Vec::new(),
        rifleman: Vec::new(),
        rifleman_cells: Vec::new(),
        gone: Vec::new(),
        checksums: Vec::new(),
    };
    for _ in 0..scenario.ticks {
        let report = world.step();
        r.checksums.push(report.checksum);
        r.shots.extend_from_slice(world.shots());
        r.deaths.extend_from_slice(world.deaths());
        r.occupied.push(world.occupied_cells());
        r.wrecks.push(
            world
                .wrecks()
                .iter()
                .map(|w| (w.kind, w.unit.0, w.anchor))
                .collect(),
        );
        let v = world.get(id(RIFLEMAN)).unwrap();
        r.rifleman.push(v.pos);
        r.rifleman_cells.push(world.cell_at(v.pos));
        r.gone
            .push((0..ENTITIES).map(|s| !world.is_alive(id(s))).collect());
    }
    r.world = world;
    r
}

fn run() -> Run {
    run_scenario(&scenario())
}

fn death_of(r: &Run, slot: u32) -> &Death {
    r.deaths
        .iter()
        .find(|d| d.victim == id(slot))
        .unwrap_or_else(|| panic!("unit {slot} did not fall"))
}

// ---- the same outcome every run ---------------------------------------------------------------------------

#[test]
fn the_same_outcome_and_checksum_come_out_every_run() {
    let (a, b) = (run(), run());
    assert_eq!(a.checksums, b.checksums);
    assert_eq!(a.shots, b.shots);
    assert_eq!(a.deaths, b.deaths);
    assert_eq!(a.occupied, b.occupied);
    assert_eq!(a.wrecks, b.wrecks);
    assert_eq!(a.rifleman, b.rifleman);
    assert_eq!(a.world.checksum(), b.world.checksum());
    assert_eq!(
        a.world.checksum(),
        parse_checksum(EXPECTED).expect("destruction.expected is a checksum")
    );
}

#[test]
fn the_scenario_is_valid_and_needs_combat() {
    let mut s = scenario();
    assert!(s.problems().is_empty(), "{:?}", s.problems());
    assert!(s.combat && s.destruction && !s.targeting);
    s.combat = false;
    assert!(
        s.problems()
            .iter()
            .any(|p| p.contains("destruction needs `combat`"))
    );
}

// ---- the gap ----------------------------------------------------------------------------------------------

#[test]
fn the_structures_of_the_first_gap_fall_on_ticks_372_and_772_and_free_their_cells() {
    let r = run();
    // Four structures of four cells at the start, eight cells after the first falls, and so on.
    assert_eq!(r.occupied[0], 16);
    assert_eq!(r.occupied[371], 16);
    assert_eq!(r.occupied[372], 12);
    assert_eq!(r.occupied[771], 12);
    assert_eq!(r.occupied[772], 8);
    assert_eq!(*r.occupied.last().unwrap(), 8, "the second gap still shut");
    let (a, b) = (death_of(&r, 0), death_of(&r, 1));
    assert_eq!((a.tick, b.tick), (Tick(372), Tick(772)));
    assert_eq!((a.kind.0, a.wreck), (RELAY, Some(WreckKind::Rubble)));
    // The cells the first one stood on are free, the second one's and the second gap's are not.
    let w = &r.world;
    for (x, y) in [(70, 44), (71, 44), (70, 45), (71, 45), (70, 46), (71, 47)] {
        assert!(!w.is_occupied(Cell::new(x, y)), "({x}, {y}) is free");
    }
    for (x, y) in [(70, 56), (71, 57), (70, 58), (71, 59)] {
        assert!(w.is_occupied(Cell::new(x, y)), "({x}, {y}) is shut");
    }
    assert!(!w.is_alive(id(0)) && !w.is_alive(id(1)));
    assert!(w.is_alive(id(2)) && w.is_alive(id(3)));
}

#[test]
fn the_routes_see_the_cells_as_free() {
    let scenario = scenario();
    let mut world = scenario.build_world().unwrap();
    world.step();
    let class = world
        .stats_of(id(RIFLEMAN))
        .unwrap()
        .movement_class
        .unwrap();
    let from = Vec2::new(Fx::from_int(62) + Fx::HALF, Fx::from_int(50) + Fx::HALF);
    let to = Vec2::new(Fx::from_int(90) + Fx::HALF, Fx::from_int(46) + Fx::HALF);
    // The wall runs from the lake to the south edge of the map, so with both gaps shut the east bank is a
    // long way round the lake in the north.
    let length = |route: &sf_sim::PlannedRoute| -> Fx {
        let mut at = from;
        let mut total = Fx::ZERO;
        for p in &route.waypoints {
            total += (*p - at).length();
            at = *p;
        }
        total
    };
    let shut = world.plan_route(class, from, to).unwrap();
    assert!(!shut.unreachable);
    assert!(
        shut.waypoints.iter().any(|p| p.y < Fx::from_int(8)),
        "the way round goes by the north edge"
    );
    assert!(length(&shut) > Fx::from_int(100), "{}", length(&shut));
    let sectors = world.path_stats().sectors_rebuilt;
    while world.tick().0 <= 372 {
        world.step();
    }
    assert!(!world.is_alive(id(0)));
    assert!(
        world.path_stats().sectors_rebuilt > sectors,
        "the sectors over the freed cells were rebuilt"
    );
    let half = world.plan_route(class, from, to).unwrap();
    assert!(!half.unreachable, "two rows of the first gap are open");
    assert!(
        length(&half) < Fx::from_int(45),
        "the way through is short: {}",
        length(&half)
    );
    for p in &half.waypoints {
        let c = world.cell_at(*p).unwrap();
        assert!(
            !(70..=71).contains(&c.x) || (44..=45).contains(&c.y),
            "the route goes through the freed cells, not the standing structure: {c:?}"
        );
    }
    while world.tick().0 <= 772 {
        world.step();
    }
    let open = world.plan_route(class, from, to).unwrap();
    assert!(!open.unreachable);
    assert!(length(&open) < Fx::from_int(45), "{}", length(&open));
}

#[test]
fn the_rifleman_walks_through_the_gap_that_was_shut() {
    let r = run();
    // He is sent at tick 800, after both structures have fallen, and arrives.
    assert_eq!(r.world.order_of(id(RIFLEMAN)), Some(Order::Idle));
    let end = r.world.get(id(RIFLEMAN)).unwrap().pos;
    assert!(
        (end - Vec2::from_ints(90, 46)).length() < Fx::from_int(2),
        "{end:?}"
    );
    let through: Vec<usize> = (0..r.rifleman_cells.len())
        .filter(|&t| {
            r.rifleman_cells[t]
                .is_some_and(|c| (70..=71).contains(&c.x) && (44..=47).contains(&c.y))
        })
        .collect();
    assert!(!through.is_empty(), "he crossed the wall in the first gap");
    assert!(through[0] > 800, "not before he was sent: {}", through[0]);
    // Never in the second gap, and never anywhere else in the wall.
    for (t, c) in r.rifleman_cells.iter().enumerate() {
        let c = c.unwrap();
        if (70..=71).contains(&c.x) {
            assert!((44..=47).contains(&c.y), "tick {t}: in the wall at {c:?}");
        }
    }
    // He stood west of the wall until he was sent.
    assert!(r.rifleman[..800].iter().all(|p| p.x < Fx::from_int(70)));
}

#[test]
fn without_destruction_the_gap_stays_shut() {
    let mut s = scenario();
    s.destruction = false;
    let r = run_scenario(&s);
    assert!(r.deaths.is_empty() && r.wrecks.iter().all(Vec::is_empty));
    // Four structures stand to the end, one of them at zero hit points; nobody gets through.
    assert!(r.occupied.iter().all(|&n| n == 16));
    assert_eq!(r.world.hp_of(id(0)), Some(0), "shot down and still there");
    assert_eq!(r.world.hp_of(id(1)), Some(0));
    assert!(r.rifleman.iter().all(|p| p.x < Fx::from_int(70)));
    assert_ne!(r.world.checksum(), parse_checksum(EXPECTED).unwrap());
    // And the tank of the other story stays where it fell, at zero.
    assert!(r.world.is_alive(id(8)));
    assert_eq!(r.world.hp_of(id(8)), Some(0));
}

// ---- the wreck --------------------------------------------------------------------------------------------

#[test]
fn the_tank_is_destroyed_on_tick_71_and_its_wreck_goes_on_tick_671() {
    let r = run();
    let d = death_of(&r, 8);
    assert_eq!((d.tick, d.kind.0), (Tick(71), LIGHT_TANK));
    assert_eq!(d.wreck, Some(WreckKind::Vehicle));
    assert_eq!(d.size, Fx::from_ratio(12, 10));
    // Present after ticks 71 to 670 (600 states), gone after tick 671.
    let tank_wreck = |t: usize| {
        r.wrecks[t]
            .iter()
            .filter(|w| w.0 == WreckKind::Vehicle && w.1 == LIGHT_TANK)
            .count()
    };
    assert_eq!(tank_wreck(70), 0);
    assert_eq!(tank_wreck(71), 1);
    assert_eq!(tank_wreck(670), 1);
    assert_eq!(tank_wreck(671), 0);
    assert_eq!(WRECK_TICKS, 671 - 71);
    // A trooper that fell leaves nothing.
    let t = death_of(&r, 12);
    assert_eq!(
        (t.tick, t.kind.0, t.wreck),
        (Tick(109), ROCKET_TROOPER, None)
    );
    assert!(
        r.wrecks[109]
            .iter()
            .all(|w| w.0 != WreckKind::Vehicle || w.1 == LIGHT_TANK)
    );
    assert_eq!(r.wrecks[109].len(), 1, "only the tank's wreck lies there");
}

#[test]
fn the_rubble_lies_where_the_structures_stood_and_goes_after_1200_ticks() {
    let r = run();
    let rubble = |t: usize| -> Vec<Option<Cell>> {
        r.wrecks[t]
            .iter()
            .filter(|w| w.0 == WreckKind::Rubble)
            .map(|w| w.2)
            .collect()
    };
    assert!(rubble(371).is_empty());
    assert_eq!(rubble(372), [Some(Cell::new(70, 44))]);
    assert_eq!(
        rubble(772),
        [Some(Cell::new(70, 44)), Some(Cell::new(70, 46))]
    );
    assert_eq!(RUBBLE_TICKS, 1572 - 372);
    assert_eq!(rubble(1571).len(), 2);
    assert_eq!(
        rubble(1572),
        [Some(Cell::new(70, 46))],
        "the first pile is gone"
    );
    assert_eq!(rubble(1971).len(), 1);
    assert!(rubble(1972).is_empty());
    assert!(r.world.wrecks().is_empty());
    // The rubble is as big as the footprint: two by two, in the middle of it.
    let mut scenario = scenario();
    scenario.ticks = 373;
    scenario.commands.retain(|c| c.tick < 373);
    let world = run_scenario(&scenario).world;
    let w = world
        .wrecks()
        .iter()
        .find(|w| w.kind == WreckKind::Rubble)
        .unwrap();
    assert_eq!(
        (w.footprint, w.pos),
        (
            Some(sf_sim::Footprint {
                width: 2,
                height: 2
            }),
            Vec2::from_ints(71, 45)
        )
    );
    // The rubble blocks nothing: the cells are free and the rifleman walked over them (above).
    assert!(!world.is_occupied(Cell::new(70, 44)));
}

// ---- who gets the credit ------------------------------------------------------------------------------------

#[test]
fn the_credit_is_settled_from_the_damage_of_the_killing_tick() {
    let r = run();
    let credit = |slot: u32| {
        let d = death_of(&r, slot);
        (
            d.killer.map(|k| (k.shooter.index(), k.owner.0, k.damage)),
            d.assists.len(),
        )
    };
    // The tank: the trooper whose rocket fell on that tick (59 = 130 percent of 45), nobody else on that tick.
    assert_eq!(credit(8), (Some((13, 1, 59)), 0));
    // The trooper: a tank shell of 14 (40 percent of 35) finished it.
    assert_eq!(credit(12), (Some((9, 0, 14)), 0));
    // The structures: a shell of 21 (60 percent of 35) each from the tank that was shooting them.
    assert_eq!(credit(0), (Some((5, 0, 21)), 0));
    assert_eq!(credit(1), (Some((6, 0, 21)), 0));
    let t = |p: u8| r.world.tally_of(PlayerId(p));
    assert_eq!((t(0).kills, t(0).assists, t(0).losses), (3, 0, 1));
    assert_eq!((t(1).kills, t(1).assists, t(1).losses), (1, 0, 3));
    assert_eq!(r.world.kills_of(id(5)), Some(1));
    assert_eq!(r.world.kills_of(id(6)), Some(1));
    assert_eq!(r.world.kills_of(id(9)), Some(1));
    assert_eq!(r.world.kills_of(id(13)), Some(1));
    assert_eq!(r.world.kills_of(id(4)), Some(0));
}

// ---- what the rules say, along the run --------------------------------------------------------------------

#[test]
fn nothing_gone_fires_or_is_shot_at_and_nobody_stands_at_zero() {
    let r = run();
    for s in &r.shots {
        let t = s.tick.0 as usize;
        if t > 0 {
            assert!(!r.gone[t - 1][s.attacker.index() as usize], "{s:?}");
            assert!(!r.gone[t - 1][s.target.index() as usize], "{s:?}");
        }
    }
    // 16 placed, 4 removed.
    assert_eq!(r.deaths.len(), 4);
    assert_eq!(r.world.snapshot().len(), 12);
    for v in r.world.snapshot().iter() {
        assert!(v.hp > 0, "{v:?}");
    }
    // The ones the first gap's structures are in the report in the order of their ticks.
    assert_eq!(
        r.deaths
            .iter()
            .map(|d| (d.tick.0, d.victim.index()))
            .collect::<Vec<_>>(),
        [(71, 8), (109, 12), (372, 0), (772, 1)]
    );
    assert!(
        FIRST_GAP
            .iter()
            .all(|&s| r.gone.last().unwrap()[s as usize])
    );
}
