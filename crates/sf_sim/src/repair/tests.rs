//! Repair and sale (WP6.6): the pay rule, the stops, the refusals, power, the sale and its refund,
//! the checksum. Inside the crate because damage is set on the hit-point column directly (the
//! combat that does it in a game is in the headless scenario `repair_sell`). Every number is
//! distinct (hp per tick 7, full cost 300, sale 40 ticks, refund 600, prices 300, 430, 1200, 97, 40,
//! hit points 600, 500, 900, 250, 0) so that a swapped or dropped term shows.

use sf_math::Fx;

use super::*;
use crate::buildings::{BuildingFunction, BuildingStats, BuildingTable, Side};
use crate::harvest::HarvestRules;
use crate::money::MoneyRules;
use crate::power::{PowerRules, advances_on};
use crate::production::{ProductionRules, ProductionTable, UnitBuild};
use crate::terrain::Cell;
use crate::units::{Footprint, UnitStats, UnitTable};
use crate::world::WorldConfig;

const ANCHOR: Kind = Kind(20);
const PLANT: Kind = Kind(21); // +100, 600 hit points, 300 credits
const LAB: Kind = Kind(22); // -45, 500 hit points, 430 credits
const REFINERY: Kind = Kind(23); // dock, 900 hit points, 1200 credits, stores 2000
const HUT: Kind = Kind(24); // -30, 250 hit points, 97 credits
const IRON: Kind = Kind(25); // +10, cannot be damaged, 40 credits
const BARRACKS: Kind = Kind(26); // infantry factory, 700 hit points, 400 credits
const RIFLE: Kind = Kind(30);
const HARVESTER: Kind = Kind(33);
const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const RULES: RepairRules = RepairRules {
    hp_per_tick: 7,
    full_cost_permille: 300,
    sell_ticks: 40,
    refund_permille: 600,
};

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

fn structure(kind: Kind, hp: u32, (w, h): (u8, u8)) -> UnitStats {
    UnitStats {
        kind,
        hp,
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
        speed_per_tick: crate::ids::per_tick(Fx::from_int(4)),
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

/// A world with money (`credits` each), production and power, and `rules` for repair and sale (none
/// when `rules` is `None`).
fn world_with(rules: Option<RepairRules>, credits: u64) -> World {
    let units = UnitTable::new(vec![
        structure(ANCHOR, 50, (2, 2)),
        structure(PLANT, 600, (2, 2)),
        structure(LAB, 500, (2, 2)),
        structure(REFINERY, 900, (3, 4)),
        structure(HUT, 250, (2, 2)),
        structure(IRON, 0, (2, 2)),
        structure(BARRACKS, 700, (2, 3)),
        mobile(RIFLE),
        mobile(HARVESTER),
    ])
    .unwrap();
    let buildings = BuildingTable::new(vec![
        entry(ANCHOR, (2, 2), BuildingFunction::Anchor, 0, None, 0),
        entry(
            PLANT,
            (2, 2),
            BuildingFunction::PowerPlant,
            30,
            Some(300),
            100,
        ),
        entry(LAB, (2, 2), BuildingFunction::None, 25, Some(430), -45),
        entry(
            REFINERY,
            (3, 4),
            BuildingFunction::HarvesterDock,
            60,
            Some(1200),
            0,
        ),
        entry(HUT, (2, 2), BuildingFunction::None, 20, Some(97), -30),
        entry(IRON, (2, 2), BuildingFunction::PowerPlant, 20, Some(40), 10),
        entry(
            BARRACKS,
            (2, 3),
            BuildingFunction::InfantryFactory,
            80,
            Some(400),
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
    let w = World::new(WorldConfig::new(5).with_map_size(64, 64))
        .unwrap()
        .with_units(units)
        .unwrap()
        .with_buildings(buildings)
        .unwrap()
        .with_production(
            ProductionRules {
                queue_limit: 5,
                exit_radius: 4,
            },
            table,
        )
        .unwrap()
        .with_player_factions(&[(P0, 1), (P1, 2)])
        .unwrap()
        .with_harvest(HARVEST, &[HARVESTER])
        .unwrap()
        .with_storage(&[(REFINERY, 2000)])
        .unwrap()
        .with_money(
            MoneyRules { credits_per_ore: 1 },
            &[(P0, credits), (P1, credits)],
        )
        .unwrap()
        .with_power(PowerRules {
            shortage_floor_permille: 250,
            defence_cutoff_permille: 1000,
        })
        .unwrap();
    match rules {
        Some(r) => w.with_repair(r).unwrap(),
        None => w,
    }
}

fn world() -> World {
    world_with(Some(RULES), 5000)
}

fn send(w: &mut World, player: PlayerId, command: Command) {
    let tick = w.tick();
    w.schedule(tick, player, command).unwrap();
    w.step();
}

fn place(w: &mut World, owner: PlayerId, kind: Kind, cell: (u32, u32)) -> EntityId {
    send(
        w,
        owner,
        Command::PlaceStructure {
            kind,
            cell: Cell::new(cell.0, cell.1),
            owner,
        },
    );
    w.occupant_at(Cell::new(cell.0, cell.1)).unwrap()
}

fn set_hp(w: &mut World, id: EntityId, hp: u32) {
    w.entities.hp[id.index() as usize] = hp;
}

fn hp(w: &World, id: EntityId) -> u32 {
    w.entities.hp[id.index() as usize]
}

fn repair(w: &mut World, player: PlayerId, id: EntityId, on: bool) {
    send(w, player, Command::Repair { structure: id, on });
}

fn last_refusal(w: &World) -> Option<RepairError> {
    w.repair_rejections().last().map(|r| r.reason)
}

/// The cumulative cost of the plant (300 credits, 600 hit points, 300 thousandths) written out
/// by hand: `300 * 300 * h / (600 * 1000)`, which is `3 * h / 20`.
fn plant_cum(h: u32) -> u64 {
    u64::from(3 * h / 20)
}

#[test]
fn a_repair_gains_hit_points_and_pays_by_the_cumulative_rule_every_tick() {
    let mut w = world();
    place(&mut w, P0, ANCHOR, (4, 4));
    let plant = place(&mut w, P0, PLANT, (10, 10));
    set_hp(&mut w, plant, 100);
    let credits = w.credits(P0);
    repair(&mut w, P0, plant, true);
    // The command ran after the repair step of its tick: nothing gained yet.
    assert!(w.is_repairing(plant));
    assert_eq!(hp(&w, plant), 100);
    assert_eq!(w.credits(P0), credits);
    let mut expected_hp = 100;
    let mut steps = 0;
    while w.is_repairing(plant) {
        w.step();
        steps += 1;
        assert!(steps < 200);
        expected_hp = (expected_hp + 7).min(600);
        assert_eq!(hp(&w, plant), expected_hp, "step {steps}");
        assert_eq!(
            credits - w.credits(P0),
            plant_cum(expected_hp) - plant_cum(100),
            "step {steps}"
        );
    }
    // 500 hit points at 7 per tick: 71 full ticks and a last one of 3.
    assert_eq!(steps, 72);
    assert_eq!(hp(&w, plant), 600);
    assert_eq!(
        credits - w.credits(P0),
        75,
        "90 for a full repair, 15 of it already there"
    );
    // Nothing is paid or gained afterwards.
    w.step();
    assert_eq!(credits - w.credits(P0), 75);
    assert!(!w.is_repairing(plant));
}

#[test]
fn the_pay_rule_adds_up_to_the_computed_cost_for_every_gap() {
    for price in [97, 300, 431, 1200] {
        for permille in [0, 1, 300, 999, 1000] {
            for max in [250, 600, 901] {
                for step in [1, 7, 13, 1000] {
                    for from in [0, 1, max / 3, max - 1] {
                        let mut hp = from;
                        let mut paid = 0;
                        while hp < max {
                            let to = (hp + step).min(max);
                            paid += repair_cost(price, permille, max, hp, to);
                            hp = to;
                        }
                        assert_eq!(
                            paid,
                            repair_cost(price, permille, max, from, max),
                            "price {price} permille {permille} max {max} step {step} from {from}"
                        );
                    }
                }
            }
            // A full repair from nothing costs exactly the fraction of the price.
            for max in [250, 600, 901] {
                assert_eq!(
                    repair_cost(price, permille, max, 0, max),
                    u64::from(price * permille / 1000)
                );
            }
        }
    }
    assert_eq!(repair_cost(300, 300, 600, 100, 600), 75);
    assert_eq!(repair_cost(300, 300, 0, 0, 5), 0, "cannot be damaged");
    assert_eq!(repair_cost(300, 300, 600, 500, 400), 0);
}

#[test]
fn a_repair_never_goes_past_full_hit_points_and_a_full_structure_refuses_it() {
    let mut w = world();
    let plant = place(&mut w, P0, PLANT, (10, 10));
    set_hp(&mut w, plant, 597);
    repair(&mut w, P0, plant, true);
    w.step();
    assert_eq!(
        hp(&w, plant),
        600,
        "3 missing, 7 per tick: stops at the maximum"
    );
    assert!(!w.is_repairing(plant));
    repair(&mut w, P0, plant, true);
    assert_eq!(last_refusal(&w), Some(RepairError::FullHitPoints));
    // One that cannot be damaged has nothing to repair either.
    let iron = place(&mut w, P0, IRON, (20, 20));
    repair(&mut w, P0, iron, true);
    assert_eq!(last_refusal(&w), Some(RepairError::FullHitPoints));
}

#[test]
fn a_repair_waits_without_money_and_goes_on_when_there_is_some() {
    let mut w = world_with(Some(RULES), 5000);
    let plant = place(&mut w, P0, PLANT, (10, 10));
    set_hp(&mut w, plant, 100);
    // Leave the owner with 6 credits.
    let burn = w.credits(P0) - 6;
    assert!(w.pay(P0, burn));
    repair(&mut w, P0, plant, true);
    let mut waited = None;
    for n in 0..30 {
        w.step();
        if w.repair_status(plant) == Some(RepairStatus::WaitingForMoney) {
            waited = Some(n);
            break;
        }
    }
    assert!(waited.is_some(), "6 credits do not repair 500 hit points");
    let frozen = hp(&w, plant);
    let credits = w.credits(P0);
    assert!(frozen > 100 && frozen < 600);
    for _ in 0..25 {
        w.step();
        assert_eq!(hp(&w, plant), frozen, "no hit points without the money");
        assert_eq!(w.credits(P0), credits, "nothing is paid while it waits");
        assert!(w.is_repairing(plant), "the flag stays");
    }
    // Money comes: it carries on and the whole run still costs exactly 75.
    w.harvest.as_deref_mut().unwrap().refund(P0, 100);
    let before = w.credits(P0);
    let mut n = 0;
    while w.is_repairing(plant) {
        w.step();
        n += 1;
        assert!(n < 200);
    }
    assert_eq!(hp(&w, plant), 600);
    // 6 credits paid for the first part, the rest now: 75 in all.
    assert_eq!(6 + 100 - w.credits(P0), 75);
    assert!(before > w.credits(P0));
}

#[test]
fn only_the_owner_may_repair_or_sell_and_every_refusal_is_listed() {
    let mut w = world();
    let plant = place(&mut w, P0, PLANT, (10, 10));
    set_hp(&mut w, plant, 100);
    // An enemy's structure.
    repair(&mut w, P1, plant, true);
    assert_eq!(last_refusal(&w), Some(RepairError::NotYours));
    send(&mut w, P1, Command::Sell { structure: plant });
    assert_eq!(last_refusal(&w), Some(RepairError::NotYours));
    assert!(!w.is_repairing(plant));
    assert!(w.sale_of(plant).is_none());
    // A neutral structure (owner 2) is nobody's to repair.
    let neutral = place(&mut w, PlayerId(2), HUT, (30, 30));
    set_hp(&mut w, neutral, 10);
    repair(&mut w, P0, neutral, true);
    assert_eq!(last_refusal(&w), Some(RepairError::NotYours));
    send(&mut w, P0, Command::Sell { structure: neutral });
    assert_eq!(last_refusal(&w), Some(RepairError::NotYours));
    // A stale id and a unit.
    let stale = EntityId::new(900, 0);
    repair(&mut w, P0, stale, true);
    assert_eq!(last_refusal(&w), Some(RepairError::NoSuchStructure));
    send(
        &mut w,
        P0,
        Command::SpawnUnit {
            kind: RIFLE,
            pos: sf_math::Vec2::new(Fx::from_int(40), Fx::from_int(40)),
            owner: P0,
        },
    );
    let unit = w
        .snapshot()
        .iter()
        .find(|v| v.kind == RIFLE)
        .map(|v| v.id)
        .unwrap();
    repair(&mut w, P0, unit, true);
    assert_eq!(last_refusal(&w), Some(RepairError::NoSuchStructure));
    send(&mut w, P0, Command::Sell { structure: unit });
    assert_eq!(last_refusal(&w), Some(RepairError::NoSuchStructure));
    // The owner is accepted; a second start and a stop without a start are refused.
    repair(&mut w, P0, plant, true);
    assert!(w.is_repairing(plant));
    repair(&mut w, P0, plant, true);
    assert_eq!(last_refusal(&w), Some(RepairError::AlreadyRepairing));
    repair(&mut w, P0, plant, false);
    assert!(!w.is_repairing(plant));
    repair(&mut w, P0, plant, false);
    assert_eq!(last_refusal(&w), Some(RepairError::NotRepairing));
    // A wreck (zero hit points, kept in a world without destruction settings) does not stand.
    let hut = place(&mut w, P0, HUT, (20, 10));
    set_hp(&mut w, hut, 0);
    repair(&mut w, P0, hut, true);
    assert_eq!(last_refusal(&w), Some(RepairError::NotStanding));
    send(&mut w, P0, Command::Sell { structure: hut });
    assert_eq!(last_refusal(&w), Some(RepairError::NotStanding));
}

#[test]
fn a_structure_under_construction_or_being_sold_cannot_be_repaired_or_sold() {
    let mut w = world();
    place(&mut w, P0, ANCHOR, (4, 4));
    send(
        &mut w,
        P0,
        Command::BuildStructure {
            kind: HUT,
            cell: Cell::new(12, 12),
        },
    );
    let hut = w.occupant_at(Cell::new(12, 12)).unwrap();
    assert!(w.construction_of(hut).is_some());
    repair(&mut w, P0, hut, true);
    assert_eq!(last_refusal(&w), Some(RepairError::UnderConstruction));
    send(&mut w, P0, Command::Sell { structure: hut });
    assert_eq!(last_refusal(&w), Some(RepairError::UnderConstruction));
    // Being sold: neither again nor repaired.
    let plant = place(&mut w, P0, PLANT, (10, 20));
    set_hp(&mut w, plant, 100);
    send(&mut w, P0, Command::Sell { structure: plant });
    assert!(w.sale_of(plant).is_some());
    send(&mut w, P0, Command::Sell { structure: plant });
    assert_eq!(last_refusal(&w), Some(RepairError::BeingSold));
    repair(&mut w, P0, plant, true);
    assert_eq!(last_refusal(&w), Some(RepairError::BeingSold));
    assert!(!w.is_repairing(plant));
}

#[test]
fn a_world_without_the_rules_ignores_both_commands() {
    let mut w = world_with(None, 5000);
    let plant = place(&mut w, P0, PLANT, (10, 10));
    set_hp(&mut w, plant, 100);
    repair(&mut w, P0, plant, true);
    send(&mut w, P0, Command::Sell { structure: plant });
    assert!(!w.is_repairing(plant));
    assert!(w.sale_of(plant).is_none());
    assert!(w.repair_rejections().is_empty());
    assert_eq!(hp(&w, plant), 100);
}

#[test]
fn a_shortage_slows_a_repair_and_the_price_is_still_exact() {
    let mut w = world();
    // 100 made, 3 x 45 used: speed 740.
    let lab = place(&mut w, P0, LAB, (10, 10));
    place(&mut w, P0, LAB, (14, 10));
    place(&mut w, P0, LAB, (18, 10));
    place(&mut w, P0, PLANT, (22, 10));
    w.step();
    assert_eq!(w.power_speed_permille(P0), 740);
    set_hp(&mut w, lab, 100);
    let credits = w.credits(P0);
    repair(&mut w, P0, lab, true);
    let mut gains = 0;
    let mut ticks = 0;
    let mut last = hp(&w, lab);
    // The ticks on which it may advance are those `advances_on` names, tick by tick.
    while w.is_repairing(lab) {
        let tick = w.tick();
        w.step();
        ticks += 1;
        let advanced = hp(&w, lab) > last;
        assert_eq!(advanced, advances_on(tick, 740), "tick {}", tick.0);
        if advanced {
            gains += 1;
        }
        last = hp(&w, lab);
        assert!(ticks < 500);
    }
    // 400 hit points at 7: 57 full ticks and one of 1; 58 gains in all, spread over more ticks.
    assert_eq!(gains, 58);
    assert!(ticks > 58 && ticks <= 80, "{ticks} ticks at 740 of 1000");
    // The cumulative cost at 500 is 430 * 300 * 500 / 500000 = 129, at 100 it is 25 (25.8): the
    // price of the gap is 104, exactly, however slowly it was paid.
    assert_eq!(credits - w.credits(P0), 104);
}

#[test]
fn a_repaired_producer_makes_more_power_from_the_next_tick() {
    let mut w = world();
    let plant = place(&mut w, P0, PLANT, (10, 10));
    set_hp(&mut w, plant, 300);
    w.step();
    assert_eq!(w.power_of(P0).unwrap().made, 50, "100 * 300 / 600");
    repair(&mut w, P0, plant, true);
    for _ in 0..20 {
        w.step();
    }
    let h = hp(&w, plant);
    assert_eq!(h, 300 + 7 * 20);
    // The totals are counted at the start of a step, from the state the last one left.
    w.step();
    assert_eq!(w.power_of(P0).unwrap().made, 100 * u64::from(h + 0) / 600);
    while w.is_repairing(plant) {
        w.step();
    }
    w.step();
    assert_eq!(w.power_of(P0).unwrap().made, 100);
}

#[test]
fn a_sale_takes_its_ticks_pays_the_refund_by_hit_points_and_removes_the_structure() {
    let mut w = world();
    let plant = place(&mut w, P0, PLANT, (10, 10));
    set_hp(&mut w, plant, 333);
    let credits = w.credits(P0);
    let start = w.tick();
    send(&mut w, P0, Command::Sell { structure: plant });
    assert_eq!(w.sale_of(plant).unwrap().left, 40);
    assert_eq!(w.sale_of(plant).unwrap().permille(), 0);
    // Gone from the first tick: it does not stand.
    assert!(!w.stands(plant));
    // A step in: 39 ticks still to go; the structure is still there, the credits not yet paid.
    w.step();
    assert_eq!(w.sale_of(plant).unwrap().left, 39);
    assert_eq!(w.credits(P0), credits);
    while w.live_index(plant).is_some() {
        w.step();
        assert!(w.tick().0 < start.0 + 100);
    }
    // Sold in the step of tick `start + 40`.
    assert_eq!(
        w.tick().0,
        start.0 + 41,
        "the step of tick start + 40 has run"
    );
    let sale = w.sales()[0];
    assert_eq!(sale.tick.0, start.0 + 40);
    assert_eq!((sale.kind, sale.owner), (PLANT, P0));
    // 300 * 600 * 333 / (600 * 1000) = 99.9, rounded down; the full refund would be 180.
    assert_eq!(sale.refund, 99);
    assert_eq!(w.credits(P0), credits + 99);
    assert!(
        w.occupant_at(Cell::new(10, 10)).is_none(),
        "the cells are free"
    );
    assert!(!w.repair_active());
    // No wreck and no death for the removal.
    assert!(w.sales().len() == 1);
}

#[test]
fn the_refund_is_a_part_of_the_price_scaled_by_the_hit_points() {
    let rows: [(Kind, (u32, u32), Option<u32>, u64); 5] = [
        (PLANT, (10, 10), None, 180),         // 300 * 600 / 1000
        (PLANT, (14, 10), Some(150), 45),     // 300 * 600 * 150 / (600 * 1000)
        (REFINERY, (20, 10), Some(450), 360), // 1200 * 600 * 450 / (900 * 1000)
        (HUT, (30, 10), Some(1), 0),          // 97 * 600 / 250000 rounds down to 0
        (IRON, (40, 10), None, 24),           // cannot be damaged: the full 40 * 600 / 1000
    ];
    for (kind, cell, damaged, refund) in rows {
        let mut w = world();
        let id = place(&mut w, P0, kind, cell);
        if let Some(h) = damaged {
            set_hp(&mut w, id, h);
        }
        let credits = w.credits(P0);
        send(&mut w, P0, Command::Sell { structure: id });
        for _ in 0..45 {
            w.step();
        }
        assert_eq!(
            w.credits(P0),
            credits + refund,
            "{kind:?} damaged {damaged:?}"
        );
    }
    assert_eq!(sale_refund(300, 600, 333, 600), 99);
    assert_eq!(sale_refund(300, 600, 600, 600), 180);
    assert_eq!(sale_refund(300, 600, 0, 0), 180);
}

#[test]
fn a_structure_being_sold_gives_no_power_no_prerequisite_and_builds_nothing() {
    let mut w = world();
    place(&mut w, P0, ANCHOR, (4, 4));
    let plant = place(&mut w, P0, PLANT, (10, 10));
    let barracks = place(&mut w, P0, BARRACKS, (20, 20));
    w.step();
    assert_eq!(w.power_of(P0).unwrap().made, 100);
    send(&mut w, P0, Command::Sell { structure: plant });
    send(
        &mut w,
        P0,
        Command::Sell {
            structure: barracks,
        },
    );
    w.step();
    assert_eq!(
        w.power_of(P0).unwrap().made,
        0,
        "counted again at the start of the step"
    );
    assert!(!w.stands(plant));
    // A factory being sold accepts nothing.
    send(
        &mut w,
        P0,
        Command::QueueUnit {
            factory: barracks,
            kind: RIFLE,
        },
    );
    assert_eq!(w.production_rejections().len(), 1);
}

#[test]
fn selling_a_refinery_lowers_the_capacity_and_the_store_keeps_what_it_holds() {
    let mut w = world_with(Some(RULES), 5000);
    let refinery = place(&mut w, P0, REFINERY, (10, 10));
    w.step();
    assert_eq!(w.storage_capacity(P0), 2000);
    assert_eq!(w.credits(P0), 5000, "more than the capacity already");
    send(
        &mut w,
        P0,
        Command::Sell {
            structure: refinery,
        },
    );
    w.step();
    assert_eq!(
        w.storage_capacity(P0),
        0,
        "a structure being sold stores nothing"
    );
    for _ in 0..45 {
        w.step();
    }
    // The refund 1200 * 600 / 1000 = 720 goes into the store above the capacity; nothing is lost.
    assert_eq!(w.credits(P0), 5000 + 720);
    assert_eq!(w.storage_capacity(P0), 0);
    assert!(w.live_index(refinery).is_none());
}

#[test]
fn a_structure_removed_during_its_sale_pays_nothing_and_leaves_no_state() {
    let mut w = world();
    let plant = place(&mut w, P0, PLANT, (10, 10));
    let credits = w.credits(P0);
    send(&mut w, P0, Command::Sell { structure: plant });
    for _ in 0..10 {
        w.step();
    }
    send(&mut w, P0, Command::Despawn { id: plant });
    assert!(!w.repair_active());
    for _ in 0..60 {
        w.step();
    }
    assert_eq!(w.credits(P0), credits);
    assert!(w.sales().is_empty());
    // The slot is reused by a new structure with nothing left over.
    let hut = place(&mut w, P0, HUT, (10, 10));
    assert_eq!(hut.index(), plant.index());
    assert!(w.sale_of(hut).is_none());
    assert!(!w.is_repairing(hut));
    assert!(w.stands(hut));
}

#[test]
fn a_sale_cancels_a_repair_in_progress() {
    let mut w = world();
    let plant = place(&mut w, P0, PLANT, (10, 10));
    set_hp(&mut w, plant, 100);
    repair(&mut w, P0, plant, true);
    w.step();
    assert!(hp(&w, plant) > 100);
    send(&mut w, P0, Command::Sell { structure: plant });
    assert!(!w.is_repairing(plant));
    let h = hp(&w, plant);
    w.step();
    assert_eq!(hp(&w, plant), h, "no hit points gained while it is sold");
}

#[test]
fn the_checksum_is_untouched_until_somebody_repairs_or_sells() {
    // The same steps in a world with the rules and in one without: identical checksums.
    let run = |rules: Option<RepairRules>, act: bool| {
        let mut w = world_with(rules, 5000);
        let plant = place(&mut w, P0, PLANT, (10, 10));
        set_hp(&mut w, plant, 100);
        let mut sums = Vec::new();
        for n in 0..20 {
            if act && n == 5 {
                let tick = w.tick();
                w.schedule(
                    tick,
                    P0,
                    Command::Repair {
                        structure: plant,
                        on: true,
                    },
                )
                .unwrap();
            }
            sums.push(w.step().checksum);
        }
        sums
    };
    let without = run(None, false);
    let idle = run(Some(RULES), false);
    assert_eq!(without, idle, "rules but no repair: hashes as before");
    let active = run(Some(RULES), true);
    assert_eq!(active[..5], without[..5]);
    assert_ne!(active[5..], without[5..]);
}

#[test]
fn every_number_of_the_state_and_the_rules_is_in_the_checksum() {
    let sum = |rules: RepairRules, sell: bool, off: bool| {
        let mut w = world_with(Some(rules), 5000);
        let plant = place(&mut w, P0, PLANT, (10, 10));
        set_hp(&mut w, plant, 100);
        if sell {
            send(&mut w, P0, Command::Sell { structure: plant });
        } else {
            repair(&mut w, P0, plant, true);
        }
        if off {
            // Same slot, flag set directly to a different value.
            w.entities.sell_left[plant.index() as usize] += 1;
        }
        w.checksum()
    };
    let base = sum(RULES, false, false);
    assert_eq!(base, sum(RULES, false, false), "deterministic");
    assert_ne!(base, sum(RULES, true, false), "a sale is not a repair");
    assert_ne!(base, sum(RULES, false, true), "the ticks of a sale");
    for other in [
        RepairRules {
            hp_per_tick: 8,
            ..RULES
        },
        RepairRules {
            full_cost_permille: 301,
            ..RULES
        },
        RepairRules {
            sell_ticks: 41,
            ..RULES
        },
        RepairRules {
            refund_permille: 601,
            ..RULES
        },
    ] {
        // The rules are in the hash only while something is active; here it is.
        assert_ne!(base, sum(other, false, false), "{other:?}");
    }
    let sold = sum(RULES, true, false);
    assert_ne!(
        sold,
        sum(
            RepairRules {
                sell_ticks: 41,
                ..RULES
            },
            true,
            false
        )
    );
}

#[test]
fn the_two_commands_have_their_own_numbers_in_the_pending_queue_hash() {
    let sum = |c: Option<Command>| {
        let mut w = world();
        if let Some(c) = c {
            let tick = crate::ids::Tick(w.tick().0 + 5);
            w.schedule(tick, P0, c).unwrap();
        }
        w.checksum()
    };
    let id = EntityId::new(3, 0);
    let all = [
        sum(None),
        sum(Some(Command::Repair {
            structure: id,
            on: true,
        })),
        sum(Some(Command::Repair {
            structure: id,
            on: false,
        })),
        sum(Some(Command::Sell { structure: id })),
        sum(Some(Command::Sell {
            structure: EntityId::new(4, 0),
        })),
    ];
    for (a, x) in all.iter().enumerate() {
        for (b, y) in all.iter().enumerate() {
            assert_eq!(a == b, x == y, "{a} {b}");
        }
    }
}

#[test]
fn the_rules_are_checked_and_given_once_before_the_start() {
    let bad = |r: RepairRules| !r.is_valid();
    assert!(bad(RepairRules {
        hp_per_tick: 0,
        ..RULES
    }));
    assert!(bad(RepairRules {
        hp_per_tick: 1001,
        ..RULES
    }));
    assert!(bad(RepairRules {
        full_cost_permille: 1001,
        ..RULES
    }));
    assert!(bad(RepairRules {
        sell_ticks: 0,
        ..RULES
    }));
    assert!(bad(RepairRules {
        sell_ticks: 6001,
        ..RULES
    }));
    assert!(bad(RepairRules {
        refund_permille: 1001,
        ..RULES
    }));
    assert!(
        RepairRules {
            full_cost_permille: 0,
            refund_permille: 0,
            ..RULES
        }
        .is_valid()
    );
    assert!(
        RepairRules {
            hp_per_tick: 1000,
            sell_ticks: 6000,
            full_cost_permille: 1000,
            refund_permille: 1000
        }
        .is_valid()
    );
    assert_eq!(
        world().with_repair(RULES).err(),
        Some(ConfigError::Repair),
        "twice"
    );
    let mut started = world_with(None, 100);
    started.step();
    assert_eq!(started.with_repair(RULES).err(), Some(ConfigError::Repair));
    let bare = World::new(WorldConfig::new(1)).unwrap();
    assert_eq!(bare.with_repair(RULES).err(), Some(ConfigError::Repair));
    assert_eq!(
        world_with(None, 1)
            .with_repair(RepairRules {
                sell_ticks: 0,
                ..RULES
            })
            .err(),
        Some(ConfigError::Repair)
    );
}
