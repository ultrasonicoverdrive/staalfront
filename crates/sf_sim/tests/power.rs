//! Power (WP6.4, `docs/rules/power.md`) through the public API: a shortage slows a factory and a
//! structure under construction, the price still adds up to exactly the cost at any speed, at speed
//! zero everything stops and starts again, and the world replays the same. Hand-made tables with
//! distinct numbers, apart from the shipped data. (Damage is tested inside the crate, where the hit
//! points can be set; the headless scenario `power` does it with real combat.)
//!
//! Map 64 x 64 of open grass. Player 0 (faction bit 1) has its anchor on (4, 4) (radius 40), player 1
//! (bit 2) its anchor on (50, 50). The barracks (2 x 3, infantry) is placed on (10, 10): its exit
//! cell is (12, 11).

use sf_math::Fx;
use sf_sim::{
    BuildingFunction, BuildingStats, BuildingTable, Cell, Command, EntityId, FactoryStatus,
    Footprint, HarvestRules, Kind, MoneyRules, PlayerId, PowerRules, ProductionRules,
    ProductionTable, Side, UnitBuild, UnitStats, UnitTable, World, WorldConfig, per_tick,
    tick_cost,
};

const ANCHOR: Kind = Kind(20);
const BARRACKS: Kind = Kind(21); // infantry factory, 2 x 3, -20
const WORKSHOP: Kind = Kind(22); // vehicle factory, 3 x 3, -40
const PLANT: Kind = Kind(23); // +100
const DRAIN: Kind = Kind(24); // -100, placed to make a shortage
const HUT: Kind = Kind(25); // -30, 97 credits over 20 ticks
const REFINERY: Kind = Kind(26); // dock, only so that there is money
const RIFLE: Kind = Kind(30); // 70 credits over 14 ticks
const HARVESTER: Kind = Kind(33);
const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

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
        speed_per_tick: per_tick(Fx::from_int(4)),
        turn_per_tick: 32,
        size: Fx::from_ratio(1, 2),
        footprint: None,
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    }
}

fn entry(
    kind: Kind,
    (w, h): (u8, u8),
    function: BuildingFunction,
    ticks: u32,
    cost: Option<u32>,
    power: i32,
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
        power,
        factions: 3,
        requires: vec![],
        build_radius: if function == BuildingFunction::Anchor {
            40
        } else {
            0
        },
    }
}

/// The world: power with the given floor, production, money (`credits` each).
fn world(floor: u32, credits: u64) -> World {
    let units = UnitTable::new(vec![
        structure(ANCHOR, 2, 2),
        structure(BARRACKS, 2, 3),
        structure(WORKSHOP, 3, 3),
        structure(PLANT, 2, 2),
        structure(DRAIN, 2, 2),
        structure(HUT, 2, 2),
        structure(REFINERY, 3, 4),
        mobile(RIFLE),
        mobile(HARVESTER),
    ])
    .unwrap();
    let buildings = BuildingTable::new(vec![
        entry(ANCHOR, (2, 2), BuildingFunction::Anchor, 0, None, 0),
        entry(
            BARRACKS,
            (2, 3),
            BuildingFunction::InfantryFactory,
            80,
            Some(400),
            -20,
        ),
        entry(
            WORKSHOP,
            (3, 3),
            BuildingFunction::VehicleFactory,
            120,
            Some(900),
            -40,
        ),
        entry(
            PLANT,
            (2, 2),
            BuildingFunction::PowerPlant,
            30,
            Some(300),
            100,
        ),
        entry(DRAIN, (2, 2), BuildingFunction::None, 10, Some(50), -100),
        entry(HUT, (2, 2), BuildingFunction::None, 20, Some(97), -30),
        entry(
            REFINERY,
            (3, 4),
            BuildingFunction::HarvesterDock,
            60,
            Some(700),
            0,
        ),
    ])
    .unwrap();
    let table = ProductionTable::new(
        vec![UnitBuild {
            kind: RIFLE,
            factory: BuildingFunction::InfantryFactory,
            cost: 70,
            build_ticks: 14,
            factions: 3,
            requires: vec![],
        }],
        vec![],
    )
    .unwrap();
    World::new(WorldConfig::new(5).with_map_size(64, 64))
        .unwrap()
        .with_units(units)
        .unwrap()
        .with_buildings(buildings)
        .unwrap()
        .with_production(
            ProductionRules {
                queue_limit: 5,
                exit_radius: 4,
                build_speed: sf_sim::BuildSpeed::FULL,
            },
            table,
        )
        .unwrap()
        .with_player_factions(&[(P0, 1), (P1, 2)])
        .unwrap()
        .with_harvest(HARVEST, &[HARVESTER])
        .unwrap()
        .with_storage(&[(REFINERY, 50)])
        .unwrap()
        .with_money(
            MoneyRules { credits_per_ore: 1 },
            &[(P0, credits), (P1, credits)],
        )
        .unwrap()
        .with_power(PowerRules {
            shortage_floor_permille: floor,
            defence_cutoff_permille: 1000,
        })
        .unwrap()
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

fn send(w: &mut World, player: PlayerId, command: Command) {
    let tick = w.tick();
    w.schedule(tick, player, command).unwrap();
    w.step();
}

fn at(w: &World, cell: (u32, u32)) -> EntityId {
    w.occupant_at(Cell::new(cell.0, cell.1)).unwrap()
}

/// Player 0: anchor, barracks (-20), workshop (-40), `plants` plants (+100 each) and `drains`
/// drains (-100 each); player 1: anchor, a plant and a barracks (enough power). Two steps later the
/// totals are counted.
fn base(floor: u32, plants: u32, drains: u32) -> World {
    let mut w = world(floor, 5000);
    place(&mut w, P0, ANCHOR, (4, 4));
    place(&mut w, P1, ANCHOR, (50, 50));
    place(&mut w, P0, BARRACKS, (10, 10));
    place(&mut w, P0, WORKSHOP, (10, 20));
    for k in 0..plants {
        place(&mut w, P0, PLANT, (20 + 4 * k, 30));
    }
    for k in 0..drains {
        place(&mut w, P0, DRAIN, (20 + 4 * k, 36));
    }
    place(&mut w, P1, PLANT, (40, 50));
    place(&mut w, P1, BARRACKS, (40, 40));
    w.step();
    w.step();
    w
}

fn status(w: &World, factory: EntityId) -> FactoryStatus {
    w.production_of(factory).unwrap().status
}

fn riflemen(w: &World, owner: PlayerId) -> usize {
    w.snapshot()
        .iter()
        .filter(|v| v.kind == RIFLE && v.owner == owner)
        .count()
}

/// Steps from the step that queues one rifleman (included) until it stands on the map.
fn ticks_to_make_a_rifleman(w: &mut World, owner: PlayerId, factory: EntityId, limit: u32) -> u32 {
    let before = riflemen(w, owner);
    send(
        w,
        owner,
        Command::QueueUnit {
            factory,
            kind: RIFLE,
        },
    );
    let mut n = 1;
    while riflemen(w, owner) == before {
        assert!(n < limit, "not made within {limit} ticks");
        w.step();
        n += 1;
    }
    n
}

#[test]
fn a_shortage_slows_a_factory_and_the_price_is_still_paid_exactly() {
    // 100 made; used 20 + 40 + 2 x 100 = 260: speed 100 * 1000 / 260 = 384.
    let mut w = base(250, 1, 2);
    assert_eq!(w.power_of(P0).unwrap().made, 100);
    assert_eq!(w.power_of(P0).unwrap().used, 260);
    assert_eq!(w.production_speed_permille(P0), 384);
    assert_eq!(w.production_speed_permille(P1), 1000, "the other player");
    let barracks = at(&w, (10, 10));
    let spent = w.credits_spent(P0);
    let credits = w.credits(P0);
    let made = ticks_to_make_a_rifleman(&mut w, P0, barracks, 100);
    // 14 000 thousandths of a tick at 384 per tick: 37 ticks (36 x 384 = 13 824 is not enough).
    assert_eq!(made, 37);
    assert_eq!(w.credits_spent(P0) - spent, 70);
    assert_eq!(credits - w.credits(P0), 70);
    // Player 1 has enough power and builds at the normal 14 ticks.
    let theirs = at(&w, (40, 40));
    assert_eq!(ticks_to_make_a_rifleman(&mut w, P1, theirs, 100), 14);
}

#[test]
fn the_progress_and_the_status_follow_the_speed_and_a_rifleman_costs_nothing_more_to_wait() {
    let mut w = base(250, 1, 2);
    let barracks = at(&w, (10, 10));
    send(
        &mut w,
        P0,
        Command::QueueUnit {
            factory: barracks,
            kind: RIFLE,
        },
    );
    for _ in 0..9 {
        w.step();
    }
    // Ten steps of 384 thousandths: 3840 of 14000 = 274 permille.
    let view = w.production_of(barracks).unwrap();
    assert_eq!(view.status, FactoryStatus::Building);
    assert_eq!(view.front_permille, 274);
    assert_eq!(view.front_paid, 70 * 3840 / 14000);
}

#[test]
fn at_speed_zero_everything_stops_and_nothing_is_paid_then_it_goes_on() {
    // Floor 0 and no plant: the consumers have no power at all.
    let mut w = base(0, 0, 0);
    assert_eq!(w.power_of(P0).unwrap().made, 0);
    assert_eq!(w.production_speed_permille(P0), 0);
    let barracks = at(&w, (10, 10));
    let credits = w.credits(P0);
    send(
        &mut w,
        P0,
        Command::QueueUnit {
            factory: barracks,
            kind: RIFLE,
        },
    );
    for _ in 0..50 {
        w.step();
    }
    let view = w.production_of(barracks).unwrap();
    assert_eq!(view.status, FactoryStatus::Stopped);
    assert_eq!(view.front_permille, 0);
    assert_eq!(view.front_paid, 0);
    assert_eq!(w.credits(P0), credits, "nothing paid");
    assert_eq!(riflemen(&w, P0), 0);
    // A structure waits too: a hut under construction does not move.
    send(
        &mut w,
        P0,
        Command::BuildStructure {
            kind: HUT,
            cell: Cell::new(30, 10),
        },
    );
    let hut = at(&w, (30, 10));
    for _ in 0..50 {
        w.step();
    }
    assert_eq!(w.construction_of(hut).unwrap().left, 20);
    // A plant placed in the world ends the shortage: counted from the step after the next one.
    place(&mut w, P0, PLANT, (20, 30));
    w.step();
    assert_eq!(w.production_speed_permille(P0), 0, "not counted yet");
    w.step();
    assert_eq!(w.production_speed_permille(P0), 1000);
    for _ in 0..20 {
        w.step();
    }
    assert_eq!(riflemen(&w, P0), 1);
    assert_eq!(w.construction_of(hut), None, "complete");
    assert_eq!(status(&w, barracks), FactoryStatus::Idle);
}

#[test]
fn a_structure_under_construction_slows_by_the_speed_and_the_price_adds_up_exactly() {
    // The speeds worked out by hand: made * 1000 / used, not below the floor.
    let cases: [(u32, u32, u32, u32); 7] = [
        (1, 0, 250, 1000), // 100 made, 60 used
        (1, 2, 250, 384),  // 100 / 260
        (1, 3, 250, 277),  // 100 / 360 = 277.7
        (1, 1, 250, 625),  // 100 / 160
        (2, 3, 250, 555),  // 200 / 360 = 555.5
        (0, 0, 1, 1),      // nothing made, 60 used: the floor
        (0, 0, 333, 333),  // the floor, higher
    ];
    for (plants, drains, floor, speed) in cases {
        let mut w = base(floor, plants, drains);
        assert_eq!(w.production_speed_permille(P0), speed, "{plants} {drains}");
        let spent0 = w.credits_spent(P0);
        send(
            &mut w,
            P0,
            Command::BuildStructure {
                kind: HUT,
                cell: Cell::new(30, 10),
            },
        );
        let hut = at(&w, (30, 10));
        assert_eq!(w.construction_of(hut).unwrap().left, 20);
        // 20 advances are needed. Over n ticks at `speed` thousandths there are at least
        // n * speed / 1000 - 1 and at most n * speed / 1000 + 1 of them.
        let mut ticks = 0u64;
        let mut advances = 0u32;
        let mut paid = w.credits_spent(P0) - spent0;
        let mut left = 20;
        while w.construction_of(hut).is_some() {
            assert!(ticks < 60_000, "never completes at speed {speed}");
            w.step();
            ticks += 1;
            let now = w.construction_of(hut).map_or(0, |c| c.left);
            let spent = w.credits_spent(P0) - spent0;
            if now == left {
                assert_eq!(spent, paid, "paid on a tick it waited, speed {speed}");
            } else {
                assert_eq!(now + 1, left, "more than one tick of progress");
                assert_eq!(
                    spent - paid,
                    tick_cost(97, 20, advances),
                    "the charge of advance {advances}, speed {speed}"
                );
                advances += 1;
                left = now;
            }
            paid = spent;
        }
        assert_eq!(advances, 20);
        assert_eq!(paid, 97, "the whole price, speed {speed}");
        // Over n ticks the pattern gives between n * speed / 1000 - 1 and n * speed / 1000 + 1
        // advances, depending on where it starts; 20 of them took n ticks.
        let n = ticks * u64::from(speed);
        assert!(
            (19_000..=21_000).contains(&n),
            "speed {speed}: {ticks} ticks is not about 20 000 / speed"
        );
        if speed == 1000 {
            assert_eq!(ticks, 20);
        }
    }
}

#[test]
fn a_world_replays_the_same_with_a_shortage() {
    let run = || {
        let mut w = base(250, 1, 2);
        let barracks = at(&w, (10, 10));
        send(
            &mut w,
            P0,
            Command::QueueUnit {
                factory: barracks,
                kind: RIFLE,
            },
        );
        send(
            &mut w,
            P0,
            Command::BuildStructure {
                kind: HUT,
                cell: Cell::new(30, 10),
            },
        );
        for _ in 0..200 {
            w.step();
        }
        w.checksum_history().to_vec()
    };
    assert_eq!(run(), run());
}
