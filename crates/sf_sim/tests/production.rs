//! Production (WP6.5, `docs/rules/production.md`): factory queues, the price paid while a unit is
//! built, the exit and the free spot, the rally point, what happens to a queue when its factory
//! goes, and the checksum. Hand-made tables with distinct numbers, through the public API, so the
//! rules are tested apart from the shipped data (the bridge and headless tests hold the shipped
//! numbers against them).
//!
//! Map 64 x 48 of open grass. Player 0 (faction bit 1) has its anchor on (4, 4) (2 x 2, radius 24),
//! player 1 (bit 2) its anchor on (50, 40). The barracks (2 x 3, infantry) is placed on (10, 10):
//! its exit cell is (12, 11). The workshop (3 x 3, vehicles) is placed on (10, 20): its exit cell
//! is (13, 21).

use sf_math::{Fx, Vec2};
use sf_sim::{
    BuildingFunction, BuildingStats, BuildingTable, Cell, Command, ConfigError, EntityId,
    FULL_SPEED_PERMILLE, FactoryStatus, Footprint, HarvestRules, Kind, MAX_EXIT_RADIUS,
    MAX_QUEUE_LIMIT, MoneyRules, PlayerId, ProductionError, ProductionRules, ProductionTable,
    ProductionTableError, Side, StepReport, UnitBuild, UnitStats, UnitTable, World, WorldConfig,
    paid_at, per_tick, tick_cost,
};

const ANCHOR: Kind = Kind(20);
const BARRACKS: Kind = Kind(21); // infantry, 2 x 3, 80 ticks, 400 credits
const WORKSHOP: Kind = Kind(22); // vehicles, 3 x 3, 120 ticks, 900 credits
const WALL: Kind = Kind(23); // 2 x 3 power plant, to block an exit
const RADAR: Kind = Kind(24); // 2 x 2, 20 ticks, 50 credits
const REFINERY: Kind = Kind(25); // dock, 3 x 4, comes with a harvester
const RIFLE: Kind = Kind(30); // infantry, 70 credits over 14 ticks, both factions
const TANK: Kind = Kind(31); // vehicle, 301 credits over 21 ticks, both factions
const SNIPER: Kind = Kind(32); // infantry, 90 credits over 10 ticks, faction 1 only, needs RADAR
const HARVESTER: Kind = Kind(33); // vehicle, 311 credits over 13 ticks
const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SPEED: i32 = 4;

const HARVEST: HarvestRules = HarvestRules {
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
};

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
        speed_per_tick: per_tick(Fx::from_int(SPEED)),
        turn_per_tick: 32,
        size: Fx::from_ratio(1, 2),
        footprint: None,
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    }
}

fn units() -> UnitTable {
    UnitTable::new(vec![
        structure(ANCHOR, 2, 2),
        structure(BARRACKS, 2, 3),
        structure(WORKSHOP, 3, 3),
        structure(WALL, 2, 3),
        structure(RADAR, 2, 2),
        structure(REFINERY, 3, 4),
        mobile(RIFLE),
        mobile(TANK),
        mobile(SNIPER),
        mobile(HARVESTER),
    ])
    .unwrap()
}

fn entry(
    kind: Kind,
    (w, h): (u8, u8),
    function: BuildingFunction,
    ticks: u32,
    cost: Option<u32>,
) -> BuildingStats {
    BuildingStats {
        kind,
        footprint: Footprint {
            width: w,
            height: h,
        },
        function,
        exit_side: function.needs_exit().then_some(Side::PosX),
        build_ticks: ticks,
        cost,
        power: 0,
        factions: 3,
        requires: vec![],
        build_radius: if function == BuildingFunction::Anchor {
            24
        } else {
            0
        },
    }
}

fn buildings() -> BuildingTable {
    BuildingTable::new(vec![
        entry(ANCHOR, (2, 2), BuildingFunction::Anchor, 0, None),
        entry(
            BARRACKS,
            (2, 3),
            BuildingFunction::InfantryFactory,
            80,
            Some(400),
        ),
        entry(
            WORKSHOP,
            (3, 3),
            BuildingFunction::VehicleFactory,
            120,
            Some(900),
        ),
        entry(WALL, (2, 3), BuildingFunction::PowerPlant, 30, Some(60)),
        entry(RADAR, (2, 2), BuildingFunction::Radar, 20, Some(50)),
        entry(
            REFINERY,
            (3, 4),
            BuildingFunction::HarvesterDock,
            60,
            Some(700),
        ),
    ])
    .unwrap()
}

fn build_of(
    kind: Kind,
    factory: BuildingFunction,
    cost: u32,
    ticks: u32,
    factions: u8,
    requires: Vec<Kind>,
) -> UnitBuild {
    UnitBuild {
        kind,
        factory,
        cost,
        build_ticks: ticks,
        factions,
        requires,
    }
}

fn table() -> ProductionTable {
    ProductionTable::new(
        vec![
            build_of(RIFLE, BuildingFunction::InfantryFactory, 70, 14, 3, vec![]),
            build_of(TANK, BuildingFunction::VehicleFactory, 301, 21, 3, vec![]),
            build_of(
                SNIPER,
                BuildingFunction::InfantryFactory,
                90,
                10,
                1,
                vec![RADAR],
            ),
            build_of(
                HARVESTER,
                BuildingFunction::VehicleFactory,
                311,
                13,
                3,
                vec![],
            ),
        ],
        vec![(REFINERY, HARVESTER)],
    )
    .unwrap()
}

fn rules(limit: u8, radius: u8) -> ProductionRules {
    ProductionRules {
        queue_limit: limit,
        exit_radius: radius,
    }
}

fn base() -> World {
    World::new(WorldConfig::new(5).with_map_size(64, 48))
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_buildings(buildings())
        .unwrap()
}

fn start(mut w: World) -> World {
    place(&mut w, P0, ANCHOR, (4, 4));
    place(&mut w, P1, ANCHOR, (50, 40));
    place(&mut w, P0, BARRACKS, (10, 10));
    place(&mut w, P0, WORKSHOP, (10, 20));
    w.step();
    w
}

/// No money rules: units are built for free.
fn free_world(limit: u8, radius: u8) -> World {
    start(
        base()
            .with_production(rules(limit, radius), table())
            .unwrap()
            .with_player_factions(&[(P0, 1), (P1, 2)])
            .unwrap(),
    )
}

/// Money rules (the store is the pool), `credits` for player 0.
fn paid_world(credits: u64, limit: u8, radius: u8) -> World {
    start(
        base()
            .with_production(rules(limit, radius), table())
            .unwrap()
            .with_player_factions(&[(P0, 1), (P1, 2)])
            .unwrap()
            .with_harvest(HARVEST, &[HARVESTER])
            .unwrap()
            .with_storage(&[(REFINERY, 50)])
            .unwrap()
            .with_money(
                MoneyRules { credits_per_ore: 1 },
                &[(P0, credits), (P1, 99)],
            )
            .unwrap(),
    )
}

fn place(w: &mut World, owner: PlayerId, kind: Kind, cell: (u32, u32)) {
    let tick = w.tick();
    w.schedule(
        tick,
        owner,
        Command::PlaceStructure {
            kind,
            cell: Cell::new(cell.0, cell.1),
            owner,
        },
    )
    .unwrap();
}

fn send(w: &mut World, player: PlayerId, command: Command) -> StepReport {
    let tick = w.tick();
    w.schedule(tick, player, command).unwrap();
    w.step()
}

fn barracks(w: &World) -> EntityId {
    w.occupant_at(Cell::new(10, 10)).unwrap()
}

fn workshop(w: &World) -> EntityId {
    w.occupant_at(Cell::new(10, 20)).unwrap()
}

fn queue(w: &mut World, factory: EntityId, kind: Kind) -> StepReport {
    send(w, P0, Command::QueueUnit { factory, kind })
}

fn run(w: &mut World, ticks: u64) {
    for _ in 0..ticks {
        w.step();
    }
}

fn run_until(w: &mut World, limit: u64, mut stop: impl FnMut(&World) -> bool) -> u64 {
    for n in 0..limit {
        if stop(w) {
            return n;
        }
        w.step();
    }
    panic!("not reached within {limit} ticks");
}

fn units_of(w: &World, kind: Kind, owner: PlayerId) -> Vec<EntityId> {
    w.snapshot()
        .iter()
        .filter(|v| v.kind == kind && v.owner == owner)
        .map(|v| v.id)
        .collect()
}

fn cell_of(w: &World, id: EntityId) -> Cell {
    w.cell_at(w.get(id).unwrap().pos).unwrap()
}

fn last_reason(w: &World) -> Option<ProductionError> {
    w.production_rejections().last().map(|r| r.reason)
}

// ---- configuration --------------------------------------------------------------------------

#[test]
fn production_is_given_once_after_the_tables_and_before_the_start() {
    // Before the unit table: nothing to check the kinds against.
    let early = World::new(WorldConfig::new(5)).unwrap();
    assert_eq!(
        early.with_production(rules(5, 4), table()).unwrap_err(),
        ConfigError::Production
    );
    // Before the building table.
    let units_only = World::new(WorldConfig::new(5))
        .unwrap()
        .with_units(units())
        .unwrap();
    assert_eq!(
        units_only
            .with_production(rules(5, 4), table())
            .unwrap_err(),
        ConfigError::Production
    );
    // Twice.
    let once = base().with_production(rules(5, 4), table()).unwrap();
    assert_eq!(
        once.with_production(rules(5, 4), table()).unwrap_err(),
        ConfigError::Production
    );
    // After the start (a command is pending).
    let mut started = base();
    started
        .schedule(
            started.tick(),
            P0,
            Command::Despawn {
                id: EntityId::new(0, 0),
            },
        )
        .unwrap();
    assert_eq!(
        started.with_production(rules(5, 4), table()).unwrap_err(),
        ConfigError::Production
    );
    // A table given later froze kinds from the earlier ones.
    let w = base().with_production(rules(5, 4), table()).unwrap();
    assert_eq!(
        w.clone().with_units(units()).unwrap_err(),
        ConfigError::Production
    );
    assert_eq!(
        w.with_buildings(buildings()).unwrap_err(),
        ConfigError::Production
    );
}

#[test]
fn the_rules_have_bounds() {
    for (limit, radius, ok) in [
        (1, 1, true),
        (MAX_QUEUE_LIMIT, MAX_EXIT_RADIUS, true),
        (0, 4, false),
        (MAX_QUEUE_LIMIT + 1, 4, false),
        (5, 0, false),
        (5, MAX_EXIT_RADIUS + 1, false),
    ] {
        assert_eq!(rules(limit, radius).is_valid(), ok, "{limit} {radius}");
        assert_eq!(
            base()
                .with_production(rules(limit, radius), table())
                .is_ok(),
            ok
        );
    }
}

#[test]
fn a_table_that_does_not_fit_the_world_is_refused() {
    let with = |builds: Vec<UnitBuild>, comes: Vec<(Kind, Kind)>| {
        base().with_production(rules(5, 4), ProductionTable::new(builds, comes).unwrap())
    };
    let rifle = || build_of(RIFLE, BuildingFunction::InfantryFactory, 70, 14, 3, vec![]);
    assert!(with(vec![rifle()], vec![]).is_ok());
    // A unit that is not in the unit table, and a structure where a unit should be.
    assert!(
        with(
            vec![build_of(
                Kind(99),
                BuildingFunction::InfantryFactory,
                1,
                1,
                3,
                vec![]
            )],
            vec![]
        )
        .is_err()
    );
    assert!(
        with(
            vec![build_of(
                BARRACKS,
                BuildingFunction::InfantryFactory,
                1,
                1,
                3,
                vec![]
            )],
            vec![]
        )
        .is_err()
    );
    // A factory function no structure of the building table has.
    assert!(
        with(
            vec![build_of(
                RIFLE,
                BuildingFunction::VesselFactory,
                70,
                14,
                3,
                vec![]
            )],
            vec![]
        )
        .is_err()
    );
    // A prerequisite that is not a structure someone can build (the anchor has no price).
    assert!(
        with(
            vec![build_of(
                RIFLE,
                BuildingFunction::InfantryFactory,
                70,
                14,
                3,
                vec![ANCHOR]
            )],
            vec![]
        )
        .is_err()
    );
    assert!(
        with(
            vec![build_of(
                RIFLE,
                BuildingFunction::InfantryFactory,
                70,
                14,
                3,
                vec![Kind(98)]
            )],
            vec![]
        )
        .is_err()
    );
    // A free unit that is not mobile, and a structure without an exit side that gives one.
    assert!(with(vec![rifle()], vec![(REFINERY, BARRACKS)]).is_err());
    assert!(with(vec![rifle()], vec![(RADAR, RIFLE)]).is_err());
    assert!(with(vec![rifle()], vec![(REFINERY, RIFLE)]).is_ok());
}

#[test]
fn the_table_refuses_what_cannot_be_built() {
    let rifle = || build_of(RIFLE, BuildingFunction::InfantryFactory, 70, 14, 3, vec![]);
    let err = |builds: Vec<UnitBuild>, comes: Vec<(Kind, Kind)>| {
        ProductionTable::new(builds, comes).unwrap_err()
    };
    assert_eq!(
        err(vec![rifle(), rifle()], vec![]),
        ProductionTableError::DuplicateKind(RIFLE)
    );
    let mut free = rifle();
    free.cost = 0;
    assert_eq!(
        err(vec![free], vec![]),
        ProductionTableError::NoCostOrTime(RIFLE)
    );
    let mut instant = rifle();
    instant.build_ticks = 0;
    assert_eq!(
        err(vec![instant], vec![]),
        ProductionTableError::NoCostOrTime(RIFLE)
    );
    let mut nowhere = rifle();
    nowhere.factory = BuildingFunction::Radar;
    assert_eq!(
        err(vec![nowhere], vec![]),
        ProductionTableError::NotAFactory(RIFLE)
    );
    let mut nobody = rifle();
    nobody.factions = 0;
    assert_eq!(
        err(vec![nobody], vec![]),
        ProductionTableError::NoFaction(RIFLE)
    );
    assert_eq!(
        err(vec![rifle()], vec![(REFINERY, RIFLE), (REFINERY, TANK)]),
        ProductionTableError::DuplicateComesWith(REFINERY)
    );
    // The order of the input is irrelevant: same table, same digest.
    let a = ProductionTable::new(
        vec![
            rifle(),
            build_of(TANK, BuildingFunction::VehicleFactory, 301, 21, 3, vec![]),
        ],
        vec![],
    )
    .unwrap();
    let b = ProductionTable::new(
        vec![
            build_of(TANK, BuildingFunction::VehicleFactory, 301, 21, 3, vec![]),
            rifle(),
        ],
        vec![],
    )
    .unwrap();
    assert_eq!(a.digest(), b.digest());
    assert_eq!(a, b);
}

#[test]
fn each_number_of_the_table_is_its_own_in_the_digest() {
    let one = |f: &dyn Fn(&mut UnitBuild)| {
        let mut b = build_of(
            RIFLE,
            BuildingFunction::InfantryFactory,
            70,
            14,
            3,
            vec![RADAR],
        );
        f(&mut b);
        ProductionTable::new(vec![b], vec![]).unwrap().digest()
    };
    let digests = [
        one(&|_| {}),
        one(&|b| b.cost = 71),
        one(&|b| b.build_ticks = 15),
        one(&|b| b.factions = 1),
        one(&|b| b.requires = vec![]),
        one(&|b| b.factory = BuildingFunction::VehicleFactory),
        one(&|b| b.kind = TANK),
    ];
    for i in 0..digests.len() {
        for j in 0..i {
            assert_ne!(digests[i], digests[j], "{i} and {j}");
        }
    }
}

// ---- the queue and the price ----------------------------------------------------------------

#[test]
fn a_queued_unit_appears_after_its_build_time_and_the_price_is_paid_tick_by_tick() {
    let mut w = paid_world(1000, 5, 4);
    let b = barracks(&w);
    let before = w.credits(P0);
    let mut spent = 0;
    // The tick the command runs in is the first tick of progress.
    let r = queue(&mut w, b, RIFLE);
    assert_eq!(r.commands_executed, 1);
    let mut taken = vec![before - w.credits(P0)];
    for _ in 1..14 {
        let c = w.credits(P0);
        w.step();
        taken.push(c - w.credits(P0));
    }
    let expected: Vec<u64> = (0..14).map(|k| tick_cost(70, 14, k)).collect();
    assert_eq!(taken, expected, "each tick costs what a structure's would");
    spent += taken.iter().sum::<u64>();
    assert_eq!(spent, 70);
    assert_eq!(w.credits_spent(P0), 70);
    // It came out in the tick that paid the last of it.
    assert_eq!(w.produced().len(), 1);
    assert_eq!(units_of(&w, RIFLE, P0).len(), 1);
    assert!(w.production_of(b).unwrap().queue.is_empty());
    // And nothing more is taken.
    run(&mut w, 10);
    assert_eq!(w.credits(P0), before - 70);
}

#[test]
fn production_is_free_in_a_world_without_money_rules() {
    let mut w = free_world(5, 4);
    let b = barracks(&w);
    queue(&mut w, b, RIFLE);
    run(&mut w, 14);
    assert_eq!(units_of(&w, RIFLE, P0).len(), 1);
    assert_eq!(w.credits_spent(P0), 0);
}

#[test]
fn the_front_item_is_built_and_the_others_wait_their_turn() {
    let mut w = paid_world(1000, 5, 4);
    let b = barracks(&w);
    queue(&mut w, b, RIFLE); // 14 ticks, 70
    queue(&mut w, b, SNIPER); // needs the radar: refused
    assert_eq!(
        last_reason(&w),
        Some(ProductionError::MissingPrerequisite(RADAR))
    );
    queue(&mut w, b, RIFLE);
    queue(&mut w, b, RIFLE);
    let v = w.production_of(b).unwrap();
    assert_eq!(v.queue.len(), 3);
    // Only the front one pays: after 9 ticks (four commands, five more) 9 of its 14 are paid,
    // not 9 of each of the three.
    run(&mut w, 5);
    assert_eq!(w.credits_spent(P0), tick_sum(70, 14, 9));
    // They come out one after the other, each after its own 14 ticks.
    run_until(&mut w, 100, |w| w.produced().len() == 1);
    assert_eq!(units_of(&w, RIFLE, P0).len(), 1);
    assert_eq!(w.production_of(b).unwrap().queue.len(), 2);
    assert_eq!(w.production_of(b).unwrap().front_permille, 0);
    // The next one starts in the next tick, not in the same.
    w.step();
    assert_eq!(w.production_of(b).unwrap().front_permille, 71);
    run(&mut w, 100);
    assert_eq!(units_of(&w, RIFLE, P0).len(), 3);
    assert_eq!(w.credits_spent(P0), 210);
    assert_eq!(w.production_of(b).unwrap().status, FactoryStatus::Idle);
}

fn tick_sum(cost: u32, total: u32, ticks: u32) -> u64 {
    (0..ticks).map(|k| tick_cost(cost, total, k)).sum()
}

#[test]
fn every_factory_has_a_queue_of_its_own() {
    let mut w = paid_world(5000, 5, 4);
    let (b, ws) = (barracks(&w), workshop(&w));
    queue(&mut w, b, RIFLE);
    queue(&mut w, ws, TANK);
    // Both pay in the same tick.
    let c = w.credits(P0);
    w.step();
    assert_eq!(
        c - w.credits(P0),
        tick_cost(70, 14, 1) + tick_cost(301, 21, 1)
    );
    run_until(&mut w, 100, |w| !w.produced().is_empty());
    // The rifleman (14 ticks) is out before the tank (21).
    assert_eq!(units_of(&w, RIFLE, P0).len(), 1);
    assert!(units_of(&w, TANK, P0).is_empty());
    run(&mut w, 30);
    assert_eq!(units_of(&w, TANK, P0).len(), 1);
    // A rifleman cannot go into the workshop, a tank not into the barracks.
    queue(&mut w, ws, RIFLE);
    assert_eq!(last_reason(&w), Some(ProductionError::WrongFactory));
    queue(&mut w, b, TANK);
    assert_eq!(last_reason(&w), Some(ProductionError::WrongFactory));
}

#[test]
fn the_queue_has_a_limit() {
    let mut w = free_world(3, 4);
    let b = barracks(&w);
    for _ in 0..3 {
        assert_eq!(queue(&mut w, b, RIFLE).commands_executed, 1);
    }
    let r = queue(&mut w, b, RIFLE);
    assert_eq!((r.commands_executed, r.commands_ignored), (0, 1));
    assert_eq!(r.production_rejected, 1);
    assert_eq!(last_reason(&w), Some(ProductionError::QueueFull));
    assert_eq!(w.production_of(b).unwrap().queue.len(), 3);
    // Room again once the first is out.
    run_until(&mut w, 100, |w| {
        w.production_of(barracks(w)).unwrap().queue.len() == 2
    });
    assert_eq!(queue(&mut w, b, RIFLE).commands_executed, 1);
}

#[test]
fn who_may_queue_what() {
    let mut w = free_world(5, 4);
    let b = barracks(&w);
    // Not another player's factory.
    send(
        &mut w,
        P1,
        Command::QueueUnit {
            factory: b,
            kind: RIFLE,
        },
    );
    assert_eq!(last_reason(&w), Some(ProductionError::NotYours));
    // Not something that is no unit, not a structure that is no factory.
    queue(&mut w, b, Kind(99));
    assert_eq!(last_reason(&w), Some(ProductionError::UnknownUnit));
    queue(&mut w, b, BARRACKS);
    assert_eq!(last_reason(&w), Some(ProductionError::UnknownUnit));
    let anchor = w.occupant_at(Cell::new(4, 4)).unwrap();
    queue(&mut w, anchor, RIFLE);
    assert_eq!(last_reason(&w), Some(ProductionError::NotAFactory));
    // A stale id.
    queue(&mut w, EntityId::new(900, 0), RIFLE);
    assert_eq!(last_reason(&w), Some(ProductionError::NoSuchFactory));
    // Nothing was queued by any of that.
    assert!(w.production_of(b).is_none());
    // A player without a faction queues nothing.
    let mut nofaction = start(base().with_production(rules(5, 4), table()).unwrap());
    let nb = barracks(&nofaction);
    queue(&mut nofaction, nb, RIFLE);
    assert_eq!(last_reason(&nofaction), Some(ProductionError::NoFaction));
}

#[test]
fn a_unit_not_open_to_the_faction_is_refused() {
    // The sniper is for faction bit 1; player 1 has bit 2 and a barracks of its own.
    let mut w = free_world(5, 4);
    place(&mut w, P1, BARRACKS, (40, 30));
    place(&mut w, P1, RADAR, (44, 30));
    place(&mut w, P0, RADAR, (20, 10));
    w.step();
    let theirs = w.occupant_at(Cell::new(40, 30)).unwrap();
    send(
        &mut w,
        P1,
        Command::QueueUnit {
            factory: theirs,
            kind: SNIPER,
        },
    );
    assert_eq!(last_reason(&w), Some(ProductionError::WrongFaction));
    let ours = barracks(&w);
    assert_eq!(queue(&mut w, ours, SNIPER).commands_executed, 1);
}

#[test]
fn a_prerequisite_must_stand_to_queue_and_to_build() {
    let mut w = free_world(5, 4);
    let b = barracks(&w);
    queue(&mut w, b, SNIPER);
    assert_eq!(
        last_reason(&w),
        Some(ProductionError::MissingPrerequisite(RADAR))
    );
    // A radar under construction does not stand.
    send(
        &mut w,
        P0,
        Command::BuildStructure {
            kind: RADAR,
            cell: Cell::new(20, 10),
        },
    );
    queue(&mut w, b, SNIPER);
    assert_eq!(
        last_reason(&w),
        Some(ProductionError::MissingPrerequisite(RADAR))
    );
    run(&mut w, 25);
    assert_eq!(queue(&mut w, b, SNIPER).commands_executed, 1);
    // Lose the radar while it is queued: the sniper stops, nothing is lost.
    let radar = w.occupant_at(Cell::new(20, 10)).unwrap();
    send(&mut w, P0, Command::Despawn { id: radar });
    run(&mut w, 30);
    let v = w.production_of(b).unwrap();
    assert_eq!(v.status, FactoryStatus::MissingPrerequisite(RADAR));
    assert_eq!(v.queue.len(), 1);
    // One tick of it was done while the radar stood (the queue command's own).
    assert_eq!(v.front_permille, 100);
    run(&mut w, 30);
    assert_eq!(w.production_of(b).unwrap().front_permille, 100);
    assert!(units_of(&w, SNIPER, P0).is_empty());
    // Build it again and the sniper goes on.
    send(
        &mut w,
        P0,
        Command::BuildStructure {
            kind: RADAR,
            cell: Cell::new(20, 10),
        },
    );
    run(&mut w, 60);
    assert_eq!(units_of(&w, SNIPER, P0).len(), 1);
}

#[test]
fn a_factory_under_construction_builds_nothing() {
    let mut w = free_world(5, 4);
    send(
        &mut w,
        P0,
        Command::BuildStructure {
            kind: BARRACKS,
            cell: Cell::new(20, 20),
        },
    );
    let new = w.occupant_at(Cell::new(20, 20)).unwrap();
    assert!(w.construction_of(new).is_some());
    queue(&mut w, new, RIFLE);
    assert_eq!(last_reason(&w), Some(ProductionError::NotStanding));
    // Nor a rally point.
    send(
        &mut w,
        P0,
        Command::SetRally {
            factory: new,
            cell: Some(Cell::new(30, 30)),
        },
    );
    assert_eq!(last_reason(&w), Some(ProductionError::NotStanding));
    assert!(w.production_of(new).is_none());
    // Once complete it takes the order.
    run(&mut w, 100);
    assert!(w.construction_of(new).is_none());
    assert_eq!(queue(&mut w, new, RIFLE).commands_executed, 1);
}

// ---- cancel, destroyed -----------------------------------------------------------------------

#[test]
fn cancelling_gives_back_exactly_what_was_paid() {
    let mut w = paid_world(1000, 5, 4);
    let b = barracks(&w);
    queue(&mut w, b, RIFLE);
    queue(&mut w, b, RIFLE);
    run(&mut w, 5); // 7 ticks of the front one paid (two commands, five more)
    let paid = tick_sum(70, 14, 7);
    assert_eq!(w.credits(P0), 1000 - paid);
    assert_eq!(w.production_of(b).unwrap().front_paid, paid);
    // Cancelling the second (nothing paid for it) changes no money.
    let r = send(
        &mut w,
        P0,
        Command::CancelUnit {
            factory: b,
            index: 1,
        },
    );
    assert_eq!(r.commands_executed, 1);
    assert_eq!(w.production_of(b).unwrap().queue.len(), 1);
    let ticking = tick_cost(70, 14, 7);
    assert_eq!(
        w.credits(P0),
        1000 - paid - ticking,
        "only the tick that ran"
    );
    // Cancelling the front one gives back all of it, and the statistic goes with it.
    let r = send(
        &mut w,
        P0,
        Command::CancelUnit {
            factory: b,
            index: 0,
        },
    );
    assert_eq!(r.commands_executed, 1);
    assert_eq!(w.credits(P0), 1000);
    assert_eq!(w.credits_spent(P0), 0);
    let v = w.production_of(b).unwrap();
    assert!(v.queue.is_empty());
    assert_eq!(v.front_permille, 0);
    // Nothing is left to cancel.
    send(
        &mut w,
        P0,
        Command::CancelUnit {
            factory: b,
            index: 0,
        },
    );
    assert_eq!(last_reason(&w), Some(ProductionError::NothingToCancel));
    // Not another player's.
    queue(&mut w, b, RIFLE);
    send(
        &mut w,
        P1,
        Command::CancelUnit {
            factory: b,
            index: 0,
        },
    );
    assert_eq!(last_reason(&w), Some(ProductionError::NotYours));
}

#[test]
fn a_destroyed_factory_loses_its_queue_and_nothing_is_refunded() {
    let mut w = paid_world(1000, 5, 4);
    let b = barracks(&w);
    queue(&mut w, b, RIFLE);
    queue(&mut w, b, RIFLE);
    run(&mut w, 5);
    let spent = w.credits_spent(P0);
    assert!(spent > 0);
    send(&mut w, P0, Command::Despawn { id: b });
    run(&mut w, 40);
    assert!(w.production_of(b).is_none());
    assert_eq!(w.factories().count(), 0);
    assert_eq!(w.credits_spent(P0), spent);
    assert_eq!(w.credits(P0), 1000 - spent);
    assert!(units_of(&w, RIFLE, P0).is_empty());
    // A new factory on the same slot starts empty; the old queue does not come back.
    place(&mut w, P0, BARRACKS, (10, 10));
    w.step();
    let again = barracks(&w);
    assert!(w.production_of(again).is_none());
    // Not even when the very same tick despawns and places (the slot is reused at once).
    queue(&mut w, again, RIFLE);
    let tick = w.tick();
    w.schedule(tick, P0, Command::Despawn { id: again })
        .unwrap();
    w.schedule(
        tick,
        P0,
        Command::PlaceStructure {
            kind: BARRACKS,
            cell: Cell::new(10, 10),
            owner: P0,
        },
    )
    .unwrap();
    w.step();
    let third = barracks(&w);
    assert_ne!(third, again);
    assert!(w.production_of(again).is_none());
    assert!(w.production_of(third).is_none_or(|v| v.queue.is_empty()));
}

#[test]
fn without_money_the_item_waits_and_nothing_is_paid() {
    // 40 credits for a unit of 70: it is paid for as far as the money goes and then it waits.
    let mut w = paid_world(40, 5, 4);
    let b = barracks(&w);
    queue(&mut w, b, RIFLE);
    run(&mut w, 30);
    let v = w.production_of(b).unwrap();
    assert_eq!(v.status, FactoryStatus::WaitingForMoney);
    assert!(units_of(&w, RIFLE, P0).is_empty());
    let held = v.front_permille;
    assert!(held > 0 && held < 1000);
    // Nothing leaves the store while it waits, and the progress does not move.
    let credits = w.credits(P0);
    run(&mut w, 30);
    assert_eq!(w.credits(P0), credits);
    assert_eq!(w.production_of(b).unwrap().front_permille, held);
    // The money it did take is what the progress stands for, and cancelling gives it back.
    assert_eq!(40 - credits, w.production_of(b).unwrap().front_paid);
    send(
        &mut w,
        P0,
        Command::CancelUnit {
            factory: b,
            index: 0,
        },
    );
    assert_eq!(w.credits(P0), 40);
}

#[test]
fn the_lower_slot_is_paid_first_when_money_is_short() {
    // 24 credits. Both queue in the same tick. Tick 1: the barracks (lower slot) takes 5 and the
    // workshop 14, 5 are left. Tick 2: the barracks takes its 5, the workshop cannot pay its 14.
    let mut w = paid_world(24, 5, 4);
    let (b, ws) = (barracks(&w), workshop(&w));
    let tick = w.tick();
    w.schedule(
        tick,
        P0,
        Command::QueueUnit {
            factory: ws,
            kind: TANK,
        },
    )
    .unwrap();
    w.schedule(
        tick,
        P0,
        Command::QueueUnit {
            factory: b,
            kind: RIFLE,
        },
    )
    .unwrap();
    w.step();
    assert_eq!(w.credits(P0), 5);
    w.step();
    assert_eq!(w.credits(P0), 0);
    let (vb, vw) = (w.production_of(b).unwrap(), w.production_of(ws).unwrap());
    assert_eq!(vb.front_permille, 142, "two ticks of 14");
    assert_eq!(vw.front_permille, 47, "one tick of 21");
    assert_eq!(vw.status, FactoryStatus::WaitingForMoney);
    assert_eq!(vb.status, FactoryStatus::Building, "it paid in this tick");
    w.step();
    assert_eq!(
        w.production_of(b).unwrap().status,
        FactoryStatus::WaitingForMoney
    );
}

// ---- the exit -------------------------------------------------------------------------------

#[test]
fn a_unit_appears_on_the_exit_cell_and_the_next_on_the_first_free_cell_beyond() {
    let mut w = free_world(5, 4);
    let b = barracks(&w);
    let exit = Cell::new(12, 11);
    for _ in 0..4 {
        queue(&mut w, b, RIFLE);
    }
    let mut cells = Vec::new();
    for _ in 0..4 {
        run_until(&mut w, 100, |w| !w.produced().is_empty());
        let p = w.produced()[0];
        assert_eq!(p.factory, b);
        assert_eq!(p.owner, P0);
        assert_eq!(cell_of(&w, p.unit), p.cell);
        cells.push((p.cell.x, p.cell.y));
        w.step();
    }
    // Ring 0 is the exit cell. Ring 1 goes row by row from the north-west: (11, 10) is the
    // factory, then (12, 10) and (13, 10) are free; the units stand where they were put.
    assert_eq!(cells[0], (exit.x, exit.y));
    assert_eq!(cells[1], (12, 10));
    assert_eq!(cells[2], (13, 10));
    // Row y = 11 of the ring: (11, 11) is the factory and (12, 11) holds the first unit.
    assert_eq!(cells[3], (13, 11));
}

#[test]
fn a_blocked_exit_holds_the_queue_and_loses_nothing() {
    // Radius 1: the window is (11..=13, 10..=12). The factory covers x = 11, so the 2 x 3 block
    // on (12, 10) closes every cell of it.
    let mut w = paid_world(1000, 5, 1);
    let b = barracks(&w);
    place(&mut w, P0, WALL, (12, 10));
    w.step();
    let wall = w.occupant_at(Cell::new(12, 10)).unwrap();
    queue(&mut w, b, RIFLE);
    queue(&mut w, b, RIFLE);
    run(&mut w, 60);
    let v = w.production_of(b).unwrap();
    assert_eq!(v.status, FactoryStatus::WaitingForExit);
    assert_eq!(v.queue.len(), 2, "the queue holds");
    assert_eq!(v.front_permille, 1000);
    assert!(units_of(&w, RIFLE, P0).is_empty());
    // It paid for the unit that is waiting and for nothing more.
    assert_eq!(w.credits_spent(P0), 70);
    // The block goes: the waiting unit comes out in the next tick, the next one starts after it.
    send(&mut w, P0, Command::Despawn { id: wall });
    assert_eq!(units_of(&w, RIFLE, P0).len(), 1);
    assert_eq!(w.production_of(b).unwrap().queue.len(), 1);
    run(&mut w, 40);
    assert_eq!(units_of(&w, RIFLE, P0).len(), 2);
    assert_eq!(w.credits_spent(P0), 140);
}

#[test]
fn a_unit_standing_on_the_spot_is_not_built_on_top_of() {
    let mut w = free_world(5, 4);
    let b = barracks(&w);
    // Someone stands on the exit cell and someone is walking to the cell after it.
    let tick = w.tick();
    let centre = |x: i32, y: i32| Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF);
    w.schedule(
        tick,
        P0,
        Command::SpawnUnit {
            kind: TANK,
            pos: centre(12, 11),
            owner: P0,
        },
    )
    .unwrap();
    w.step();
    queue(&mut w, b, RIFLE);
    run_until(&mut w, 100, |w| !w.produced().is_empty());
    assert_eq!(
        (w.produced()[0].cell.x, w.produced()[0].cell.y),
        (12, 10),
        "the tank holds the exit cell"
    );
}

// ---- the rally point -------------------------------------------------------------------------

#[test]
fn a_new_unit_walks_to_the_rally_point_and_without_one_it_stays() {
    let mut w = free_world(5, 4);
    let b = barracks(&w);
    queue(&mut w, b, RIFLE);
    run_until(&mut w, 100, |w| !w.produced().is_empty());
    let stayed = w.produced()[0].unit;
    let spot = cell_of(&w, stayed);
    run(&mut w, 60);
    assert_eq!(
        cell_of(&w, stayed),
        spot,
        "no rally point: it stays on its spot"
    );
    // Now a rally point: the next one gets a move order from the tick it appears, and arrives.
    let rally = Cell::new(24, 14);
    assert_eq!(
        send(
            &mut w,
            P0,
            Command::SetRally {
                factory: b,
                cell: Some(rally)
            }
        )
        .commands_executed,
        1
    );
    assert_eq!(w.production_of(b).unwrap().rally, Some(rally));
    queue(&mut w, b, RIFLE);
    run_until(&mut w, 100, |w| !w.produced().is_empty());
    let walker = w.produced()[0].unit;
    assert!(matches!(w.order_of(walker), Some(sf_sim::Order::Move(_))));
    run(&mut w, 200);
    assert_eq!(cell_of(&w, walker), rally);
    // The one that was already out is not sent anywhere.
    assert_eq!(cell_of(&w, stayed), spot);
    // Clearing the rally point: the next stays.
    send(
        &mut w,
        P0,
        Command::SetRally {
            factory: b,
            cell: None,
        },
    );
    assert_eq!(w.production_of(b).unwrap().rally, None);
}

#[test]
fn a_rally_point_is_the_owners_and_on_the_map() {
    let mut w = free_world(5, 4);
    let b = barracks(&w);
    send(
        &mut w,
        P1,
        Command::SetRally {
            factory: b,
            cell: Some(Cell::new(1, 1)),
        },
    );
    assert_eq!(last_reason(&w), Some(ProductionError::NotYours));
    send(
        &mut w,
        P0,
        Command::SetRally {
            factory: b,
            cell: Some(Cell::new(64, 1)),
        },
    );
    assert_eq!(last_reason(&w), Some(ProductionError::OffMap));
    send(
        &mut w,
        P0,
        Command::SetRally {
            factory: b,
            cell: Some(Cell::new(63, 47)),
        },
    );
    assert_eq!(w.production_of(b).unwrap().rally, Some(Cell::new(63, 47)));
}

// ---- the free unit of a structure ---------------------------------------------------------------

#[test]
fn a_refinery_that_is_built_brings_a_harvester_out_and_one_that_is_placed_does_not() {
    let mut w = free_world(5, 4);
    // Placed (a mission object or a start state): no harvester.
    place(&mut w, P0, REFINERY, (30, 10));
    run(&mut w, 5);
    assert!(units_of(&w, HARVESTER, P0).is_empty());
    // Built: complete after 60 ticks, then its harvester comes out on the refinery's exit cell
    // (30 + 3, 10 + 4 / 2) = (33, 12).
    send(
        &mut w,
        P0,
        Command::BuildStructure {
            kind: REFINERY,
            cell: Cell::new(30, 20),
        },
    );
    let refinery = w.occupant_at(Cell::new(30, 20)).unwrap();
    run_until(&mut w, 100, |w| w.construction_of(refinery).is_none());
    run(&mut w, 2);
    let harvesters = units_of(&w, HARVESTER, P0);
    assert_eq!(harvesters.len(), 1);
    assert_eq!(cell_of(&w, harvesters[0]), Cell::new(33, 22));
    // It cost nothing and the queue it used is empty again.
    assert!(w.production_of(refinery).unwrap().queue.is_empty());
    // And it does not come again.
    run(&mut w, 100);
    assert_eq!(units_of(&w, HARVESTER, P0).len(), 1);
}

#[test]
fn a_free_harvester_waits_inside_when_the_exit_is_blocked() {
    let mut w = free_world(5, 1);
    // The refinery's window is (33..=35, 21..=23) for exit (33, 22) and radius 1 (its own column
    // x = 32 is the footprint): a 2 x 3 block closes 33..34 and a second the rest of it.
    send(
        &mut w,
        P0,
        Command::BuildStructure {
            kind: REFINERY,
            cell: Cell::new(30, 20),
        },
    );
    place(&mut w, P0, WALL, (33, 21));
    place(&mut w, P0, WALL, (35, 21));
    w.step();
    let blocker = w.occupant_at(Cell::new(33, 21)).unwrap();
    run(&mut w, 100);
    assert!(units_of(&w, HARVESTER, P0).is_empty());
    let refinery = w.occupant_at(Cell::new(30, 20)).unwrap();
    assert_eq!(
        w.production_of(refinery).unwrap().status,
        FactoryStatus::WaitingForExit
    );
    send(&mut w, P0, Command::Despawn { id: blocker });
    assert_eq!(units_of(&w, HARVESTER, P0).len(), 1);
}

// ---- the speed hook --------------------------------------------------------------------------

#[test]
fn the_speed_factor_is_a_hook_fixed_at_full_speed() {
    let w = free_world(5, 4);
    assert_eq!(FULL_SPEED_PERMILLE, 1000);
    assert_eq!(w.production_speed_permille(P0), 1000);
    assert_eq!(w.production_speed_permille(P1), 1000);
    // At full speed the price of a tick is the structure rule's.
    for k in 0..14u32 {
        let by_progress = paid_at(70, 14, (k + 1) * 1000) - paid_at(70, 14, k * 1000);
        assert_eq!(by_progress, tick_cost(70, 14, k));
    }
    // And at any speed the costs add up to the price.
    for speed in [1, 7, 333, 999, 1000] {
        let total = 14u32 * 1000;
        let mut done = 0u32;
        let mut sum = 0u64;
        while done < total {
            let next = (done + speed).min(total);
            sum += paid_at(70, 14, next) - paid_at(70, 14, done);
            done = next;
        }
        assert_eq!(sum, 70, "speed {speed}");
    }
    assert_eq!(paid_at(70, 0, 5), 0);
}

// ---- the checksum -------------------------------------------------------------------------------

#[test]
fn a_world_without_production_has_no_production_section() {
    // Same tables and the same placements, with and without the production rules: they differ,
    // and the one without them equals one built by an older caller (no section at all).
    let plain = start(base().with_player_factions(&[(P0, 1), (P1, 2)]).unwrap());
    let with = free_world(5, 4);
    assert_ne!(plain.checksum(), with.checksum());
    assert!(plain.production_rules().is_none());
    assert!(plain.production_table().is_none());
    // Commands for production are ignored there, not an error.
    let mut w = plain;
    let b = barracks(&w);
    let r = queue(&mut w, b, RIFLE);
    assert_eq!((r.commands_executed, r.commands_ignored), (0, 1));
    assert_eq!(r.production_rejected, 0);
}

#[test]
fn queues_progress_rally_and_the_tuning_numbers_are_in_the_checksum() {
    let sum = |f: &dyn Fn(&mut World)| {
        let mut w = free_world(5, 4);
        f(&mut w);
        w.checksum()
    };
    let plain = sum(&|_| {});
    let queued = sum(&|w| {
        let f = barracks(w);
        queue(w, f, RIFLE);
    });
    let other = sum(&|w| {
        let f = workshop(w);
        queue(w, f, TANK);
    });
    let twice = sum(&|w| {
        let f = barracks(w);
        queue(w, f, RIFLE);
        queue(w, f, RIFLE);
    });
    let rallied = sum(&|w| {
        let f = barracks(w);
        send(
            w,
            P0,
            Command::SetRally {
                factory: f,
                cell: Some(Cell::new(30, 30)),
            },
        );
    });
    let rallied_elsewhere = sum(&|w| {
        let f = barracks(w);
        send(
            w,
            P0,
            Command::SetRally {
                factory: f,
                cell: Some(Cell::new(30, 31)),
            },
        );
    });
    let rallied_east = sum(&|w| {
        let f = barracks(w);
        send(
            w,
            P0,
            Command::SetRally {
                factory: f,
                cell: Some(Cell::new(31, 30)),
            },
        );
    });
    let all = [
        plain,
        queued,
        other,
        twice,
        rallied,
        rallied_elsewhere,
        rallied_east,
    ];
    for i in 0..all.len() {
        for j in 0..i {
            assert_ne!(all[i], all[j], "{i} and {j}");
        }
    }
    // Progress is in it: the same queue one tick later differs in more than the tick, and two
    // worlds that ran the same way agree.
    let run_it = |ticks: u64| {
        let mut w = free_world(5, 4);
        let f = barracks(&w);
        queue(&mut w, f, RIFLE);
        run(&mut w, ticks);
        w.checksum_history().to_vec()
    };
    assert_eq!(run_it(6), run_it(6));
    // The tuning numbers and the table are in it.
    assert_ne!(free_world(5, 4).checksum(), free_world(6, 4).checksum());
    assert_ne!(free_world(5, 4).checksum(), free_world(5, 3).checksum());
    let dearer = {
        let mut builds: Vec<UnitBuild> = table().iter().cloned().collect();
        builds[0].cost += 1;
        ProductionTable::new(builds, vec![(REFINERY, HARVESTER)]).unwrap()
    };
    let w = start(
        base()
            .with_production(rules(5, 4), dearer)
            .unwrap()
            .with_player_factions(&[(P0, 1), (P1, 2)])
            .unwrap(),
    );
    assert_ne!(free_world(5, 4).checksum(), w.checksum());
}

#[test]
fn the_commands_have_permanent_numbers_in_the_pending_queue_hash() {
    // A pending command is part of the checksum; each of the three differs from the others and
    // from nothing at all.
    let sum = |c: Option<Command>| {
        let mut w = free_world(5, 4);
        let f = barracks(&w);
        if let Some(c) = c {
            let tick = sf_sim::Tick(w.tick().0 + 5);
            let c = match c {
                Command::QueueUnit { kind, .. } => Command::QueueUnit { factory: f, kind },
                Command::CancelUnit { index, .. } => Command::CancelUnit { factory: f, index },
                Command::SetRally { cell, .. } => Command::SetRally { factory: f, cell },
                other => other,
            };
            w.schedule(tick, P0, c).unwrap();
        }
        w.checksum()
    };
    let f = EntityId::new(0, 0);
    let all = [
        sum(None),
        sum(Some(Command::QueueUnit {
            factory: f,
            kind: RIFLE,
        })),
        sum(Some(Command::QueueUnit {
            factory: f,
            kind: TANK,
        })),
        sum(Some(Command::CancelUnit {
            factory: f,
            index: 1,
        })),
        sum(Some(Command::CancelUnit {
            factory: f,
            index: 2,
        })),
        sum(Some(Command::SetRally {
            factory: f,
            cell: Some(Cell::new(1, 2)),
        })),
        sum(Some(Command::SetRally {
            factory: f,
            cell: Some(Cell::new(2, 1)),
        })),
        sum(Some(Command::SetRally {
            factory: f,
            cell: None,
        })),
    ];
    for i in 0..all.len() {
        for j in 0..i {
            assert_ne!(all[i], all[j], "{i} and {j}");
        }
    }
}

#[test]
fn two_runs_of_the_same_commands_agree_tick_by_tick() {
    let play = || {
        let mut w = paid_world(900, 5, 4);
        let (b, ws) = (barracks(&w), workshop(&w));
        send(
            &mut w,
            P0,
            Command::SetRally {
                factory: b,
                cell: Some(Cell::new(20, 12)),
            },
        );
        for k in [RIFLE, RIFLE, RIFLE] {
            queue(&mut w, b, k);
        }
        queue(&mut w, ws, TANK);
        queue(&mut w, ws, HARVESTER);
        run(&mut w, 150);
        send(
            &mut w,
            P0,
            Command::CancelUnit {
                factory: ws,
                index: 0,
            },
        );
        run(&mut w, 150);
        w.checksum_history().to_vec()
    };
    assert_eq!(play(), play());
}
