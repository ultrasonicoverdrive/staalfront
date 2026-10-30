//! Money (WP7.4, `docs/rules/economy.md`, "Money"): the store is the money pool, and a structure a
//! player builds is paid for tick by tick while it goes up. Hand-made tables, through the public
//! API, so the rules are tested apart from the shipped data (the bridge and headless tests hold
//! the shipped numbers against them).
//!
//! Map 64 x 48 of grass with one ore field (cells x 24..28, y 18..22, 100 each, no regrowth). The
//! refinery stands on (6, 20) (3 x 4, dock point (9.5, 22.5)). Player 0 has its anchor on (30, 6)
//! (2 x 2, radius 12) and player 1 its anchor on (30, 40).

use sf_math::{Fx, Vec2};
use sf_sim::{
    BuildingFunction, BuildingStats, BuildingTable, Cell, Command, ConfigError, EntityId,
    Footprint, HarvestRules, HarvestStage, Kind, MAX_CREDITS_PER_ORE, MAX_STARTING_CREDITS,
    MoneyRules, OreRules, PlayerId, Side, Terrain, TerrainDef, TerrainFlags, TerrainId, UnitStats,
    UnitTable, World, WorldConfig, per_tick, tick_cost,
};

const GRASS: u8 = 0;
const ORE: u8 = 4;
const REFINERY: Kind = Kind(2);
const HARVESTER: Kind = Kind(11);
const ANCHOR: Kind = Kind(20); // 2 x 2, radius 12, not built
const PLANT: Kind = Kind(21); // 2 x 2, 40 ticks, 100 credits
const TOWER: Kind = Kind(22); // 1 x 1, 30 ticks, 250 credits
const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const PRICE: MoneyRules = MoneyRules { credits_per_ore: 1 };

const RULES: HarvestRules = HarvestRules {
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

const ORE_RULES: OreRules = OreRules {
    start: 100,
    max: 120,
    regrow_amount: 0,
    regrow_period: 10,
};

fn unit(kind: Kind, w: u8, h: u8) -> UnitStats {
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

fn units() -> UnitTable {
    UnitTable::new(vec![
        unit(REFINERY, 3, 4),
        UnitStats {
            kind: HARVESTER,
            hp: 800,
            movement_class: Some(1),
            speed_per_tick: per_tick(Fx::from_ratio(12, 10)),
            turn_per_tick: 6,
            size: Fx::from_ratio(3, 2),
            footprint: None,
            weapon_range: Fx::ZERO,
            turret_per_tick: 0,
        },
        unit(ANCHOR, 2, 2),
        unit(PLANT, 2, 2),
        unit(TOWER, 1, 1),
    ])
    .unwrap()
}

fn entry(
    kind: Kind,
    (w, h): (u8, u8),
    function: BuildingFunction,
    ticks: u32,
    cost: Option<u32>,
    radius: u16,
) -> BuildingStats {
    BuildingStats {
        kind,
        footprint: Footprint {
            width: w,
            height: h,
        },
        function,
        exit_side: (function == BuildingFunction::HarvesterDock).then_some(Side::PosX),
        build_ticks: ticks,
        cost,
        power: 0,
        factions: 3,
        requires: vec![],
        build_radius: radius,
    }
}

fn buildings() -> BuildingTable {
    BuildingTable::new(vec![
        entry(
            REFINERY,
            (3, 4),
            BuildingFunction::HarvesterDock,
            800,
            Some(1200),
            0,
        ),
        entry(ANCHOR, (2, 2), BuildingFunction::Anchor, 0, None, 12),
        entry(
            PLANT,
            (2, 2),
            BuildingFunction::PowerPlant,
            40,
            Some(100),
            0,
        ),
        entry(TOWER, (1, 1), BuildingFunction::Defence, 30, Some(250), 0),
    ])
    .unwrap()
}

fn terrain() -> Terrain {
    let ids: Vec<u8> = (0..48u32)
        .flat_map(|y| (0..64u32).map(move |x| (x, y)))
        .map(|(x, y)| {
            if (24..28).contains(&x) && (18..22).contains(&y) {
                ORE
            } else {
                GRASS
            }
        })
        .collect();
    let defs = vec![
        TerrainDef {
            id: TerrainId(GRASS),
            flags: TerrainFlags::OPEN,
        },
        TerrainDef {
            id: TerrainId(ORE),
            flags: TerrainFlags {
                buildable: false,
                harvestable: true,
                ground: true,
            },
        },
    ];
    Terrain::from_layer(64, 48, &ids, &defs).unwrap()
}

/// Harvest rules, storage `cap` per refinery, the given starting credits: no money rules yet.
fn stored(cap: u32) -> World {
    World::with_terrain(WorldConfig::new(7).with_map_size(64, 48), terrain())
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_buildings(buildings())
        .unwrap()
        .with_player_factions(&[(P0, 1), (P1, 2)])
        .unwrap()
        .with_ore(ORE_RULES)
        .unwrap()
        .with_harvest(RULES, &[HARVESTER])
        .unwrap()
        .with_storage(&[(REFINERY, cap)])
        .unwrap()
}

/// A world with money: `start` credits for player 0 and none for player 1, each unit of ore worth
/// `price`, room for `cap` in a refinery, the two anchors placed.
fn world_with(start: u64, price: u32, cap: u32) -> World {
    let mut w = stored(cap)
        .with_money(
            MoneyRules {
                credits_per_ore: price,
            },
            &[(P0, start)],
        )
        .unwrap();
    place(&mut w, P0, ANCHOR, (30, 6));
    place(&mut w, P1, ANCHOR, (30, 40));
    w.step();
    w
}

fn world(start: u64) -> World {
    world_with(start, 1, 2000)
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

fn build(w: &mut World, player: PlayerId, kind: Kind, cell: (u32, u32)) -> EntityId {
    let tick = w.tick();
    w.schedule(
        tick,
        player,
        Command::BuildStructure {
            kind,
            cell: Cell::new(cell.0, cell.1),
        },
    )
    .unwrap();
    w.step();
    w.occupant_at(Cell::new(cell.0, cell.1))
        .expect("the structure was placed")
}

fn spawn_harvester(w: &mut World, pos: (i32, i32)) {
    let tick = w.tick();
    w.schedule(
        tick,
        P0,
        Command::SpawnUnit {
            kind: HARVESTER,
            pos: Vec2::new(
                Fx::from_int(pos.0) + Fx::HALF,
                Fx::from_int(pos.1) + Fx::HALF,
            ),
            owner: P0,
        },
    )
    .unwrap();
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

fn left(w: &World, id: EntityId) -> u32 {
    w.construction_of(id).map_or(0, |c| c.left)
}

// ---- the arithmetic -------------------------------------------------------------------------

#[test]
fn the_ticks_of_a_build_add_up_to_exactly_the_price() {
    for (cost, total) in [
        (100, 40),
        (250, 30),
        (1, 1),
        (1, 50),
        (7, 3),
        (1200, 800),
        (999_999, 997),
        (u32::MAX, 5000),
        (0, 10),
    ] {
        let sum: u64 = (0..total).map(|k| tick_cost(cost, total, k)).sum();
        assert_eq!(sum, u64::from(cost), "{cost} over {total}");
        // No tick costs more than the price spread evenly, rounded up.
        let most = u64::from(cost).div_ceil(u64::from(total));
        assert!(
            (0..total).all(|k| tick_cost(cost, total, k) <= most),
            "{cost} over {total}"
        );
    }
    // The remainder falls on the later ticks: 7 over 3 is 2, 2, 3 with the floor of the running
    // total (0, 2, 4, 7).
    assert_eq!(
        (0..3).map(|k| tick_cost(7, 3, k)).collect::<Vec<_>>(),
        [2, 2, 3]
    );
    assert_eq!(tick_cost(7, 0, 0), 0);
}

// ---- configuration --------------------------------------------------------------------------

#[test]
fn money_is_given_once_after_the_storage_rules_and_before_the_start() {
    let start = [(P0, 500)];
    // Without storage rules (the store is the pool).
    let no_storage = World::with_terrain(WorldConfig::new(7).with_map_size(64, 48), terrain())
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_buildings(buildings())
        .unwrap()
        .with_harvest(RULES, &[HARVESTER])
        .unwrap();
    assert_eq!(
        no_storage.with_money(PRICE, &start).unwrap_err(),
        ConfigError::Money
    );
    // Twice.
    let once = stored(100).with_money(PRICE, &start).unwrap();
    assert!(once.has_money());
    assert_eq!(
        once.with_money(PRICE, &start).unwrap_err(),
        ConfigError::Money
    );
    // After the start.
    let mut started = stored(100);
    started.step();
    assert_eq!(
        started.with_money(PRICE, &start).unwrap_err(),
        ConfigError::Money
    );
    // A price of zero or above the maximum, too many starting credits, a player twice.
    let bad_price = |p| MoneyRules { credits_per_ore: p };
    assert_eq!(
        stored(100).with_money(bad_price(0), &start).unwrap_err(),
        ConfigError::Money
    );
    assert_eq!(
        stored(100)
            .with_money(bad_price(MAX_CREDITS_PER_ORE + 1), &start)
            .unwrap_err(),
        ConfigError::Money
    );
    assert!(
        stored(100)
            .with_money(bad_price(MAX_CREDITS_PER_ORE), &start)
            .is_ok()
    );
    assert_eq!(
        stored(100)
            .with_money(PRICE, &[(P0, MAX_STARTING_CREDITS + 1)])
            .unwrap_err(),
        ConfigError::Money
    );
    assert!(
        stored(100)
            .with_money(PRICE, &[(P0, MAX_STARTING_CREDITS)])
            .is_ok()
    );
    assert_eq!(
        stored(100)
            .with_money(PRICE, &[(P0, 5), (P0, 6)])
            .unwrap_err(),
        ConfigError::Money
    );
    // Not given: no money.
    assert!(!stored(100).has_money());
    assert_eq!(stored(100).money_rules(), None);
}

#[test]
fn each_player_starts_with_its_own_credits() {
    let w = stored(100)
        .with_money(PRICE, &[(P1, 70), (P0, 500)])
        .unwrap();
    assert_eq!((w.credits(P0), w.credits(P1)), (500, 70));
    assert_eq!(w.credits(PlayerId(2)), 0);
    assert_eq!(w.money_rules(), Some(PRICE));
    // The store is the pool: it is the stored ore too, and nothing has been spent.
    assert_eq!(w.ore_stored(P0), 500);
    assert_eq!((w.credits_spent(P0), w.credits_spent(P1)), (0, 0));
}

// ---- paying while building ------------------------------------------------------------------

#[test]
fn a_structure_drains_its_price_over_its_build_time() {
    let mut w = world(1000);
    let plant = build(&mut w, P0, PLANT, (34, 6));
    // Placed in this tick's commands: nothing is paid yet; the first payment is the next tick.
    assert_eq!((w.credits(P0), left(&w, plant)), (1000, 40));
    let mut seen = vec![];
    for _ in 0..40 {
        w.step();
        seen.push(w.credits(P0));
    }
    // 100 credits over 40 ticks: 2, 3, 2, 3, ... (the floor of 2.5 per tick, running).
    let expected: Vec<u64> = (1..=40u64).map(|k| 1000 - 100 * k / 40).collect();
    assert_eq!(seen, expected);
    assert!(w.stands(plant));
    assert_eq!(w.credits(P0), 900);
    assert_eq!(w.credits_spent(P0), 100);
    // Completed: nothing more is paid.
    run(&mut w, 30);
    assert_eq!(w.credits(P0), 900);
    assert!(w.stalled().is_empty());
}

#[test]
fn it_is_the_owner_who_pays() {
    let mut w = world(1000);
    // Player 1 has no credits at all: its structure waits, player 0's is not touched.
    let theirs = build(&mut w, P1, PLANT, (34, 40));
    let ours = build(&mut w, P0, PLANT, (34, 6));
    run(&mut w, 60);
    assert_eq!(left(&w, theirs), 40);
    assert!(w.is_stalled(theirs));
    assert!(w.stands(ours));
    assert_eq!((w.credits(P0), w.credits(P1)), (900, 0));
    assert_eq!((w.credits_spent(P0), w.credits_spent(P1)), (100, 0));
}

#[test]
fn construction_pauses_without_money_and_goes_on_when_money_arrives() {
    // 30 credits for a plant of 100: it runs while it can pay (the running total reaches 30 after
    // 12 ticks: 100 * 12 / 40 = 30) and then waits.
    let mut w = world(30);
    let plant = build(&mut w, P0, PLANT, (34, 6));
    run(&mut w, 12);
    assert_eq!((left(&w, plant), w.credits(P0)), (28, 0));
    assert!(w.stalled().is_empty());
    // Out of money: nothing moves, not the progress, not the credits, however long it waits.
    for _ in 0..100 {
        w.step();
        assert_eq!(left(&w, plant), 28);
        assert!(w.is_stalled(plant));
        assert_eq!(w.stalled(), [plant]);
    }
    assert_eq!(w.credits(P0), 0);
    assert!(!w.stands(plant));
    // Ore arrives: a refinery, a harvester and the first unload put credits in the store, and the
    // plant goes on from where it stopped.
    place(&mut w, P0, REFINERY, (6, 20));
    spawn_harvester(&mut w, (14, 22));
    let waited = run_until(&mut w, 3000, |w| left(w, plant) < 28);
    assert!(waited > 0);
    assert!(w.credits_spent(P0) > 30);
    run_until(&mut w, 3000, |w| w.stands(plant));
    // The whole price was paid, once: 100 spent, and the rest of the ore is still in the store.
    assert_eq!(w.credits_spent(P0), 100);
    assert_eq!(w.credits(P0), w.ore_delivered(P0) - 70);
}

#[test]
fn a_tick_that_cannot_be_paid_in_full_is_not_paid_in_part() {
    // Price 7 over 3 ticks is 2, 2, 3. With 6 credits the third tick (3) cannot be paid: 4 are
    // spent, 2 stay, and the structure waits one tick short of complete.
    let mut w = world(6);
    // A tower is 250 over 30 ticks; use the plant with the price in mind instead: 100 over 40
    // is 2 or 3 per tick. With 6 credits: 2 + 3 + 2 = 7 > 6, so after 2 ticks (5 spent) the third
    // tick (2) fits (7)? No: 6 - 5 = 1 < 2.
    let plant = build(&mut w, P0, PLANT, (34, 6));
    run(&mut w, 10);
    assert_eq!(w.credits_spent(P0), 5);
    assert_eq!(w.credits(P0), 1);
    assert_eq!(left(&w, plant), 38);
    assert!(w.is_stalled(plant));
}

#[test]
fn with_too_little_money_for_two_the_older_slot_is_paid_first() {
    // A tower is 250 over 30 ticks: the running total after 1, 2, 3 ticks is 8, 16, 25, so the
    // ticks cost 8, 8, 9. With 25 credits: in the first tick both pay 8 (9 left), in the second
    // the older slot pays 8 (1 left) and the younger cannot, and from then on neither can.
    let mut w = world(25);
    let first = build(&mut w, P0, TOWER, (34, 6));
    let second = build(&mut w, P0, TOWER, (36, 6));
    assert!(first.index() < second.index());
    run(&mut w, 20);
    assert_eq!((left(&w, first), left(&w, second)), (28, 29));
    assert_eq!((w.credits(P0), w.credits_spent(P0)), (1, 24));
    assert_eq!(w.stalled(), [first, second]);
}

#[test]
fn mission_objects_cost_nothing() {
    let mut w = world(10);
    place(&mut w, P0, TOWER, (34, 6));
    run(&mut w, 3);
    let tower = w.occupant_at(Cell::new(34, 6)).unwrap();
    assert!(w.stands(tower));
    assert_eq!((w.credits(P0), w.credits_spent(P0)), (10, 0));
}

#[test]
fn what_was_paid_for_a_structure_destroyed_half_built_is_not_refunded() {
    let mut w = world(1000);
    let plant = build(&mut w, P0, PLANT, (34, 6));
    run(&mut w, 20);
    assert_eq!(w.credits(P0), 950);
    // The removal runs in the commands of the next step, after that step's payment: 21 ticks paid.
    let tick = w.tick();
    w.schedule(tick, P0, Command::Despawn { id: plant })
        .unwrap();
    run(&mut w, 40);
    assert!(w.construction_of(plant).is_none());
    assert_eq!((w.credits(P0), w.credits_spent(P0)), (948, 52));
    assert!(w.stalled().is_empty());
}

#[test]
fn a_world_without_money_rules_builds_for_free_and_never_stalls() {
    let mut w = stored(2000);
    place(&mut w, P0, ANCHOR, (30, 6));
    w.step();
    let plant = build(&mut w, P0, PLANT, (34, 6));
    run(&mut w, 40);
    assert!(w.stands(plant));
    assert_eq!((w.credits(P0), w.credits_spent(P0)), (0, 0));
    assert!(w.stalled().is_empty());
    assert!(!w.has_money());
}

// ---- the store is the pool ------------------------------------------------------------------

#[test]
fn a_unit_of_ore_is_worth_its_price_and_the_store_counts_credits() {
    // 3 credits a unit, room for 300 credits: 100 units fit and the bite is cut to the whole
    // units that fit. After the first load of 100 units the store holds 300.
    let mut w = world_with(0, 3, 400);
    place(&mut w, P0, REFINERY, (6, 20));
    spawn_harvester(&mut w, (14, 22));
    run_until(&mut w, 3000, |w| w.ore_delivered(P0) > 0);
    run_until(&mut w, 3000, |w| w.ore_delivered(P0) >= 100);
    assert_eq!(w.credits(P0), 3 * w.ore_delivered(P0));
    // 400 of room is 133 whole units (399 credits), never 400.
    run(&mut w, 3000);
    assert_eq!(w.storage_capacity(P0), 400);
    assert_eq!(w.ore_delivered(P0), 133);
    assert_eq!(w.credits(P0), 399);
}

#[test]
fn spending_makes_room_and_a_waiting_harvester_unloads() {
    // A store of 100, full of starting credits: the harvester comes back loaded and waits in the
    // bay. A structure that costs money opens room as it is built, and the load goes in.
    let mut w = world_with(100, 1, 100);
    place(&mut w, P0, REFINERY, (6, 20));
    spawn_harvester(&mut w, (14, 22));
    run_until(&mut w, 3000, |w| {
        w.harvester_of(EntityId::new(3, 0))
            .is_some_and(|h| h.stage == HarvestStage::Unloading && h.load == RULES.capacity)
    });
    run(&mut w, 200);
    assert_eq!((w.credits(P0), w.ore_delivered(P0)), (100, 0));
    // Now spend.
    let tower = build(&mut w, P0, TOWER, (34, 6));
    // The first ore goes in while the tower is still going up: spending made the room.
    run_until(&mut w, 200, |w| w.ore_delivered(P0) > 0);
    assert!(left(&w, tower) > 0, "the room came from the first payments");
    assert!(w.credits_spent(P0) > 0);
    // The tower is paid in full (it waits when the room has not been refilled yet), and the books
    // balance: 100 at the start, plus the ore delivered, minus the 250 spent.
    run_until(&mut w, 3000, |w| w.stands(tower));
    assert_eq!(w.credits_spent(P0), 250);
    assert_eq!(w.credits(P0), 100 + w.ore_delivered(P0) - 250);
}

#[test]
fn starting_credits_above_the_capacity_stay_and_fill_nothing_more() {
    // 500 starting credits against a capacity of 100: the store holds more than its capacity, the
    // harvester waits with its load, and nothing is clamped.
    let mut w = world_with(500, 1, 100);
    place(&mut w, P0, REFINERY, (6, 20));
    spawn_harvester(&mut w, (14, 22));
    run(&mut w, 1500);
    assert_eq!((w.credits(P0), w.storage_capacity(P0)), (500, 100));
    assert_eq!(w.ore_delivered(P0), 0);
}

// ---- determinism ----------------------------------------------------------------------------

#[test]
fn the_same_run_gives_the_same_checksums_and_credits_change_them() {
    let run_one = |start: u64| {
        let mut w = world(start);
        build(&mut w, P0, PLANT, (34, 6));
        run(&mut w, 50);
        (w.checksum(), w.credits(P0))
    };
    assert_eq!(run_one(1000), run_one(1000));
    assert_ne!(run_one(1000).0, run_one(1001).0);
}
