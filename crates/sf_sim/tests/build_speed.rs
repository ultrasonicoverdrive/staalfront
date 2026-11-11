//! The build speed (WP6.11, `docs/rules/production.md`, "Build speed"): everything is built
//! `num / den` times as fast as the data's build times, units and structures alike, rounded up in
//! ticks, and still costs exactly its price. Hand-made tables with distinct numbers through the
//! public API; the shipped numbers are held against these rules in `sf_bridge/tests/build_speed.rs`.
//!
//! Map 64 x 48 of open grass, player 0 with its anchor on (4, 4) and a barracks on (10, 10).

use sf_math::Fx;
use sf_sim::{
    BuildSpeed, BuildingFunction, BuildingStats, BuildingTable, Cell, Command, Footprint, Kind,
    MAX_BUILD_SPEED_RATIO, MAX_BUILD_SPEED_TERM, MoneyRules, PlayerId, ProductionRules,
    ProductionTable, Side, UnitBuild, UnitStats, UnitTable, World, WorldConfig, paid_at, per_tick,
    tick_cost,
};

const ANCHOR: Kind = Kind(20);
const BARRACKS: Kind = Kind(21); // infantry, 2 x 3, 80 ticks, 400 credits
const PLANT: Kind = Kind(22); // 2 x 2, 45 ticks, 301 credits
const REFINERY: Kind = Kind(25); // dock, 3 x 4: the store money needs
const HARVESTER: Kind = Kind(33);
const RIFLE: Kind = Kind(30); // 70 credits over 14 ticks
const GRUNT: Kind = Kind(31); // 333 credits over 100 ticks
const P0: PlayerId = PlayerId(0);

const SPEED_3_2: BuildSpeed = BuildSpeed { num: 3, den: 2 };

fn structure(kind: Kind, w: u8, h: u8) -> UnitStats {
    UnitStats {
        kind,
        hp: 500,
        movement_class: None,
        speed_per_tick: Fx::ZERO,
        turn_per_tick: 0,
        size: Fx::ZERO,
        footprint: Some(Footprint {
            width: w,
            height: h,
        }),
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    }
}

fn mobile(kind: Kind) -> UnitStats {
    UnitStats {
        kind,
        hp: 100,
        movement_class: Some(1),
        speed_per_tick: per_tick(Fx::from_int(4)),
        turn_per_tick: 32,
        size: Fx::from_ratio(1, 2),
        footprint: None,
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    }
}

#[allow(clippy::too_many_arguments)]
fn entry(
    kind: Kind,
    size: (u8, u8),
    function: BuildingFunction,
    ticks: u32,
    cost: Option<u32>,
    exit: Option<Side>,
    radius: u16,
) -> BuildingStats {
    BuildingStats {
        kind,
        footprint: Footprint {
            width: size.0,
            height: size.1,
        },
        function,
        exit_side: exit,
        build_ticks: ticks,
        cost,
        power: 0,
        factions: 1,
        requires: vec![],
        build_radius: radius,
    }
}

fn buildings() -> BuildingTable {
    BuildingTable::new(vec![
        entry(ANCHOR, (2, 2), BuildingFunction::Anchor, 0, None, None, 24),
        entry(
            BARRACKS,
            (2, 3),
            BuildingFunction::InfantryFactory,
            80,
            Some(400),
            Some(Side::PosX),
            0,
        ),
        entry(
            REFINERY,
            (3, 4),
            BuildingFunction::HarvesterDock,
            60,
            Some(700),
            Some(Side::PosX),
            0,
        ),
        entry(
            PLANT,
            (2, 2),
            BuildingFunction::PowerPlant,
            45,
            Some(301),
            None,
            0,
        ),
    ])
    .unwrap()
}

fn units() -> UnitTable {
    UnitTable::new(vec![
        structure(ANCHOR, 2, 2),
        structure(BARRACKS, 2, 3),
        structure(PLANT, 2, 2),
        structure(REFINERY, 3, 4),
        mobile(HARVESTER),
        mobile(RIFLE),
        mobile(GRUNT),
    ])
    .unwrap()
}

fn table() -> ProductionTable {
    let build = |kind, cost, build_ticks| UnitBuild {
        kind,
        factory: BuildingFunction::InfantryFactory,
        cost,
        build_ticks,
        factions: 1,
        requires: vec![],
    };
    ProductionTable::new(vec![build(RIFLE, 70, 14), build(GRUNT, 333, 100)], vec![]).unwrap()
}

fn rules(speed: BuildSpeed) -> ProductionRules {
    ProductionRules {
        queue_limit: 5,
        exit_radius: 4,
        build_speed: speed,
    }
}

/// A world with money (5000 credits for player 0) and production at `speed`; the anchor and the
/// barracks stand from the start (`PlaceStructure`), the plant is built with `BuildStructure`.
fn world(speed: BuildSpeed) -> World {
    let mut w = World::new(WorldConfig::new(5).with_map_size(64, 48))
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_buildings(buildings())
        .unwrap()
        .with_production(rules(speed), table())
        .unwrap()
        .with_player_factions(&[(P0, 1)])
        .unwrap()
        .with_harvest(
            sf_sim::HarvestRules {
                capacity: 100,
                mine_amount: 5,
                mine_period: 4,
                unload_amount: 20,
                unload_period: 3,
                search_radius: 40,
                retry_period: 20,
                call_range: 6,
                grant_timeout: 200,
                call_timeout: 10,
                max_tries: 3,
                park_distance: 4,
                park_spread: 5,
                park_near_tenths: 20,
                mine_start_tenths: 10,
                mine_reach_tenths: 15,
                mine_lose_tenths: 25,
                dock_reach_tenths: 20,
                travel_cost: 16,
                queue_cost: 100,
                switch_margin: 100,
                recheck_period: 20,
                queue_gap: 3,
                queue_start: 1,
                queue_step: 2,
                queue_rows: 8,
            },
            &[HARVESTER],
        )
        .unwrap()
        .with_storage(&[(REFINERY, 50)])
        .unwrap()
        .with_money(MoneyRules { credits_per_ore: 1 }, &[(P0, 5000)])
        .unwrap();
    for (kind, cell) in [(ANCHOR, (4, 4)), (BARRACKS, (10, 10))] {
        let tick = w.tick();
        w.schedule(
            tick,
            P0,
            Command::PlaceStructure {
                kind,
                cell: Cell::new(cell.0, cell.1),
                owner: P0,
            },
        )
        .unwrap();
    }
    w.step();
    w
}

fn send(w: &mut World, command: Command) {
    let tick = w.tick();
    w.schedule(tick, P0, command).unwrap();
    w.step();
}

fn count(w: &World, kind: Kind) -> usize {
    w.snapshot().iter().filter(|v| v.kind == kind).count()
}

/// Steps from the one that queues `kind` (included) until a unit of it stands on the map, and the
/// credits that took.
fn make(w: &mut World, kind: Kind) -> (u32, u64) {
    let barracks = w.occupant_at(Cell::new(10, 10)).unwrap();
    let before = count(w, kind);
    let credits = w.credits(P0);
    send(
        w,
        Command::QueueUnit {
            factory: barracks,
            kind,
        },
    );
    let mut n = 1;
    while count(w, kind) == before {
        assert!(n < 500, "not made within 500 ticks");
        w.step();
        n += 1;
    }
    (n, credits - w.credits(P0))
}

/// Its scaled build ticks, checked against the steps from the one that places `kind` until it stands, and the credits that took.
fn build(w: &mut World, kind: Kind, cell: (u32, u32)) -> (u32, u64) {
    let credits = w.credits(P0);
    send(
        w,
        Command::BuildStructure {
            kind,
            cell: Cell::new(cell.0, cell.1),
        },
    );
    let id = w.occupant_at(Cell::new(cell.0, cell.1)).unwrap();
    let total = w.construction_of(id).unwrap().total;
    let mut n = 1;
    while !w.stands(id) {
        assert!(n < 500, "not built within 500 ticks");
        w.step();
        n += 1;
    }
    // The step that places it does not count: it stands `total` steps later.
    assert_eq!(n - 1, total, "it stands after exactly its scaled ticks");
    (total, credits - w.credits(P0))
}

#[test]
fn ticks_are_the_base_times_den_over_num_rounded_up() {
    // Distinct numbers, a few of them exact (45 * 2 / 3 = 30), most not.
    let s = SPEED_3_2;
    assert_eq!(s.ticks(45), 30, "exact");
    assert_eq!(s.ticks(80), 54, "53.33 up");
    assert_eq!(s.ticks(14), 10, "9.33 up");
    assert_eq!(s.ticks(100), 67, "66.67 up");
    assert_eq!(s.ticks(1), 1, "a build of one tick still takes one");
    assert_eq!(s.ticks(0), 0, "what cannot be built stays so");
    // Slower, and a different fraction.
    assert_eq!(BuildSpeed { num: 2, den: 3 }.ticks(14), 21);
    assert_eq!(BuildSpeed { num: 5, den: 4 }.ticks(14), 12, "11.2 up");
    assert_eq!(BuildSpeed { num: 5, den: 4 }.ticks(100), 80);
    // Not a floor, and not nearest: 10 * 2 / 3 = 6.67 is 7, and 4 * 2 / 3 = 2.67 is 3.
    assert_eq!((s.ticks(10), s.ticks(4)), (7, 3));
}

#[test]
fn one_over_one_gives_the_data_s_own_tick_counts() {
    let full = BuildSpeed::FULL;
    for base in [0, 1, 2, 14, 45, 80, 100, 1000, u32::MAX] {
        assert_eq!(full.ticks(base), base);
        assert_eq!(BuildSpeed { num: 7, den: 7 }.ticks(base), base, "7 / 7 too");
    }
    // A world at 1 / 1 builds in the table's ticks.
    let mut w = world(full);
    assert_eq!(build(&mut w, PLANT, (20, 30)).0, 45);
    assert_eq!(make(&mut w, RIFLE).0, 14);
    assert_eq!(make(&mut w, GRUNT).0, 100);
}

#[test]
fn the_bounds_of_the_factor() {
    let ok = |num, den| BuildSpeed { num, den }.is_valid();
    assert!(ok(1, 1) && ok(3, 2) && ok(2, 3));
    assert!(ok(MAX_BUILD_SPEED_RATIO, 1) && ok(1, MAX_BUILD_SPEED_RATIO));
    assert!(ok(MAX_BUILD_SPEED_TERM, MAX_BUILD_SPEED_TERM));
    assert!(!ok(0, 1) && !ok(1, 0), "a zero term");
    assert!(
        !ok(MAX_BUILD_SPEED_TERM + 1, MAX_BUILD_SPEED_TERM),
        "term too big"
    );
    assert!(!ok(1, MAX_BUILD_SPEED_TERM + 1));
    assert!(!ok(9, 2) && !ok(2, 9), "more than 4 times either way");
    // The world refuses invalid rules.
    let bad = ProductionRules {
        queue_limit: 5,
        exit_radius: 4,
        build_speed: BuildSpeed { num: 0, den: 1 },
    };
    assert!(
        World::new(WorldConfig::new(5).with_map_size(64, 48))
            .unwrap()
            .with_units(units())
            .unwrap()
            .with_buildings(buildings())
            .unwrap()
            .with_production(bad, table())
            .is_err()
    );
}

#[test]
fn units_are_built_in_the_scaled_ticks_and_cost_exactly_their_price() {
    let mut w = world(SPEED_3_2);
    // 14 -> 10 and 100 -> 67; the price is the table's, not scaled.
    assert_eq!(make(&mut w, RIFLE), (10, 70));
    assert_eq!(make(&mut w, GRUNT), (67, 333));
    assert_eq!(w.credits_spent(P0), 70 + 333);
    // The table the world holds has the scaled ticks (the sidebar reads them).
    let t = w.production_table().unwrap();
    assert_eq!(t.get(RIFLE).unwrap().build_ticks, 10);
    assert_eq!(t.get(GRUNT).unwrap().build_ticks, 67);
    assert_eq!(t.get(GRUNT).unwrap().cost, 333);
}

#[test]
fn structures_are_built_in_the_scaled_ticks_and_cost_exactly_their_price() {
    let mut w = world(SPEED_3_2);
    assert_eq!(build(&mut w, PLANT, (20, 30)), (30, 301));
    // 80 -> 54 (53.33 up).
    let mut w = world(SPEED_3_2);
    let (n, paid) = build(&mut w, BARRACKS, (20, 30));
    assert_eq!((n, paid), (54, 400));
    assert_eq!(w.building_table().get(PLANT).unwrap().build_ticks, 30);
    assert_eq!(w.building_table().get(BARRACKS).unwrap().build_ticks, 54);
}

/// Whatever the factor, the per-tick payments of a build add up to its price: no credit is lost
/// or made by the rounding of the ticks (structures: `tick_cost`; units: `paid_at` in thousandths).
#[test]
fn the_money_adds_up_for_every_price_and_every_factor() {
    for (num, den) in [
        (1, 1),
        (3, 2),
        (2, 3),
        (5, 4),
        (7, 3),
        (4, 1),
        (1, 4),
        (99, 100),
    ] {
        let speed = BuildSpeed { num, den };
        for cost in [1u32, 50, 70, 301, 333, 400, 1500, 9999] {
            for base in [1u32, 10, 14, 45, 80, 100, 211, 1000] {
                let ticks = speed.ticks(base);
                let by_tick: u64 = (0..ticks).map(|k| tick_cost(cost, ticks, k)).sum();
                assert_eq!(
                    by_tick,
                    u64::from(cost),
                    "{num}/{den} cost {cost} base {base}"
                );
                let by_progress: u64 = (0..ticks)
                    .map(|k| paid_at(cost, ticks, (k + 1) * 1000) - paid_at(cost, ticks, k * 1000))
                    .sum();
                assert_eq!(
                    by_progress,
                    u64::from(cost),
                    "{num}/{den} cost {cost} base {base}"
                );
            }
        }
    }
}

/// Two factors that give the same ticks for every build of the tables are still two different
/// rules: the factor is in the checksum (only when it is not 1 / 1, so a world at the data's own
/// times hashes as before).
#[test]
fn the_factor_is_in_the_checksum() {
    // Every build here is 1 tick or 0 (the structures too), so 3 / 2 and 2 / 1 give the same scaled
    // tables and digests, and only the factor itself can part the two checksums.
    let one_tick_buildings = || {
        let stats = buildings()
            .iter()
            .cloned()
            .map(|mut b| {
                b.build_ticks = b.build_ticks.min(1);
                b
            })
            .collect();
        BuildingTable::new(stats).unwrap()
    };
    let one_tick_world = |speed: BuildSpeed| {
        let build = |kind| UnitBuild {
            kind,
            factory: BuildingFunction::InfantryFactory,
            cost: 5,
            build_ticks: 1,
            factions: 1,
            requires: vec![],
        };
        World::new(WorldConfig::new(5).with_map_size(64, 48))
            .unwrap()
            .with_units(units())
            .unwrap()
            .with_buildings(one_tick_buildings())
            .unwrap()
            .with_production(
                rules(speed),
                ProductionTable::new(vec![build(RIFLE)], vec![]).unwrap(),
            )
            .unwrap()
    };
    let a = one_tick_world(SPEED_3_2);
    let b = one_tick_world(BuildSpeed { num: 2, den: 1 });
    assert_eq!(
        a.production_table()
            .unwrap()
            .get(RIFLE)
            .unwrap()
            .build_ticks,
        b.production_table()
            .unwrap()
            .get(RIFLE)
            .unwrap()
            .build_ticks,
    );
    assert_eq!(a.building_table().digest(), b.building_table().digest());
    assert_eq!(
        a.production_table().unwrap().digest(),
        b.production_table().unwrap().digest()
    );
    assert_ne!(a.checksum(), b.checksum(), "3/2 against 2/1");
    // The same fraction written differently is the same rule.
    let c = one_tick_world(BuildSpeed { num: 6, den: 4 });
    assert_eq!(a.checksum(), c.checksum(), "6/4 is 3/2");
    // 1 / 1 and 7 / 7 hash the same (and as a world without the factor did).
    let d = one_tick_world(BuildSpeed::FULL);
    let e = one_tick_world(BuildSpeed { num: 7, den: 7 });
    assert_eq!(d.checksum(), e.checksum());
    assert_ne!(a.checksum(), d.checksum());
}

#[test]
fn the_factory_view_reports_the_scaled_ticks() {
    // The factory view reports the scaled ticks and progress in permille of them.
    let mut w = world(SPEED_3_2);
    let barracks = w.occupant_at(Cell::new(10, 10)).unwrap();
    send(
        &mut w,
        Command::QueueUnit {
            factory: barracks,
            kind: GRUNT,
        },
    );
    for _ in 0..32 {
        w.step();
    }
    let v = w.production_of(barracks).unwrap();
    assert_eq!(v.front_ticks, 67);
    // 33 ticks of 67: 492 permille.
    assert_eq!(v.front_permille, 492);
}
