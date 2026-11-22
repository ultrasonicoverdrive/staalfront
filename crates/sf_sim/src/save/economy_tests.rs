//! WP13.1c-2: the ore, the harvesters and their docks, the stores and the money, construction,
//! production and power through save and restore. One world builds all of it: two players, an ore
//! field, three harvesters of player 0 sharing one refinery (a dock queue), a factory with a full
//! queue, a rally point and repeat, a structure going up, player 0 short of power and player 1
//! with a plant. Inside the crate to build worlds the public API cannot (counters at their bounds)
//! and to compare by `PartialEq`.

use sf_math::{Fx, Vec2};

use super::tests::settled;
use super::*;
use crate::buildings::{BuildingFunction, BuildingStats, BuildingTable, Side};
use crate::command::Command;
use crate::ids::per_tick;
use crate::money::MoneyRules;
use crate::ore::OreRules;
use crate::power::PowerRules;
use crate::production::{BuildSpeed, ProductionRules, ProductionTable, UnitBuild};
use crate::terrain::{Terrain, TerrainDef, TerrainFlags, TerrainId};
use crate::units::{Footprint, UnitStats, UnitTable};
use crate::world::WorldConfig;

const ANCHOR: Kind = Kind(1);
const REFINERY: Kind = Kind(2);
const FACTORY: Kind = Kind(3);
const PLANT: Kind = Kind(4);
const SILO: Kind = Kind(5);
const HUT: Kind = Kind(6);
const HARVESTER: Kind = Kind(11);
const MAN: Kind = Kind(12);
const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn fp(width: u8, height: u8) -> Footprint {
    Footprint { width, height }
}

fn structure(kind: Kind, hp: u32, footprint: Footprint) -> UnitStats {
    UnitStats {
        kind,
        hp,
        movement_class: None,
        speed_per_tick: Fx::ZERO,
        turn_per_tick: 0,
        size: Fx::ZERO,
        footprint: Some(footprint),
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    }
}

fn mobile(kind: Kind, hp: u32) -> UnitStats {
    UnitStats {
        kind,
        hp,
        movement_class: Some(1),
        speed_per_tick: per_tick(Fx::from_ratio(12, 10)),
        turn_per_tick: 6,
        size: Fx::from_ratio(3, 2),
        footprint: None,
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    }
}

fn building(
    kind: Kind,
    footprint: Footprint,
    function: BuildingFunction,
    ticks: u32,
    power: i32,
) -> BuildingStats {
    BuildingStats {
        kind,
        footprint,
        function,
        exit_side: function.needs_exit().then_some(Side::PosX),
        build_ticks: ticks,
        cost: (ticks > 0).then_some(ticks * 3),
        power,
        factions: 3,
        requires: Vec::new(),
        build_radius: if function == BuildingFunction::Anchor {
            30
        } else {
            0
        },
    }
}

pub(super) const HARVEST: crate::harvest::HarvestRules = crate::harvest::tests::RULES;

/// The fresh world: tables and rules, nothing placed. `queue_limit` and `floor` vary the rules for
/// the digest cases.
fn fresh_with(queue_limit: u8, floor: u32) -> World {
    let defs = vec![
        TerrainDef {
            id: TerrainId(0),
            flags: TerrainFlags::OPEN,
        },
        TerrainDef {
            id: TerrainId(4),
            flags: TerrainFlags {
                buildable: false,
                harvestable: true,
                ground: true,
            },
        },
    ];
    let ids: Vec<u8> = (0..48u32)
        .flat_map(|y| (0..64u32).map(move |x| (x, y)))
        .map(|(x, y)| u8::from((24..28).contains(&x) && (18..22).contains(&y)) * 4)
        .collect();
    let terrain = Terrain::from_layer(64, 48, &ids, &defs).unwrap();
    let units = UnitTable::new(vec![
        structure(ANCHOR, 500, fp(2, 2)),
        structure(REFINERY, 900, fp(3, 4)),
        structure(FACTORY, 700, fp(2, 2)),
        structure(PLANT, 600, fp(2, 2)),
        structure(SILO, 300, fp(1, 1)),
        structure(HUT, 300, fp(2, 2)),
        mobile(HARVESTER, 800),
        mobile(MAN, 50),
    ])
    .unwrap();
    let buildings = BuildingTable::new(vec![
        building(ANCHOR, fp(2, 2), BuildingFunction::Anchor, 0, 0),
        building(REFINERY, fp(3, 4), BuildingFunction::HarvesterDock, 60, -30),
        building(
            FACTORY,
            fp(2, 2),
            BuildingFunction::InfantryFactory,
            50,
            -20,
        ),
        building(PLANT, fp(2, 2), BuildingFunction::PowerPlant, 40, 100),
        building(SILO, fp(1, 1), BuildingFunction::None, 0, 0),
        building(HUT, fp(2, 2), BuildingFunction::None, 40, -30),
    ])
    .unwrap();
    let table = ProductionTable::new(
        vec![UnitBuild {
            kind: MAN,
            factory: BuildingFunction::InfantryFactory,
            cost: 40,
            build_ticks: 30,
            factions: 3,
            requires: Vec::new(),
        }],
        vec![(REFINERY, HARVESTER)],
    )
    .unwrap();
    World::with_terrain(WorldConfig::new(11).with_map_size(64, 48), terrain)
        .unwrap()
        .with_units(units)
        .unwrap()
        .with_buildings(buildings)
        .unwrap()
        .with_ore(OreRules {
            start: 60,
            max: 80,
            regrow_amount: 1,
            regrow_period: 7,
        })
        .unwrap()
        .with_harvest(HARVEST, &[HARVESTER])
        .unwrap()
        .with_storage(&[(REFINERY, 2000), (SILO, 3000)])
        .unwrap()
        .with_money(MoneyRules { credits_per_ore: 2 }, &[(P0, 400), (P1, 900)])
        .unwrap()
        .with_production(
            ProductionRules {
                queue_limit,
                exit_radius: 3,
                build_speed: BuildSpeed::FULL,
            },
            table,
        )
        .unwrap()
        .with_power(PowerRules {
            shortage_floor_permille: floor,
            defence_cutoff_permille: 1000,
        })
        .unwrap()
        .with_player_factions(&[(P0, 1), (P1, 2)])
        .unwrap()
}

pub(super) fn fresh() -> World {
    fresh_with(3, 250)
}

fn cell(x: u32, y: u32) -> crate::terrain::Cell {
    crate::terrain::Cell::new(x, y)
}

fn send(w: &mut World, tick: u64, player: PlayerId, c: Command) {
    w.schedule(Tick(tick), player, c).unwrap();
}

/// Slots: 0 anchor P0, 1 refinery P0, 2 factory P0, 3 silo P0, 4 anchor P1, 5 plant P1, 6 refinery
/// P1, 7..=9 harvesters P0, 10 harvester P1; then the hut P0 builds (11) and what comes out.
pub(super) fn economy() -> World {
    let mut w = fresh();
    let place = |kind, x, y, owner| Command::PlaceStructure {
        kind,
        cell: cell(x, y),
        owner,
    };
    for c in [
        place(ANCHOR, 4, 4, P0),
        place(REFINERY, 12, 18, P0),
        place(FACTORY, 6, 30, P0),
        place(SILO, 4, 10, P0),
        place(ANCHOR, 50, 40, P1),
        place(PLANT, 54, 40, P1),
        place(REFINERY, 40, 20, P1),
    ] {
        send(&mut w, 0, P0, c);
    }
    for (x, owner) in [(20, P0), (21, P0), (22, P0), (32, P1)] {
        send(
            &mut w,
            0,
            P0,
            Command::SpawnUnit {
                kind: HARVESTER,
                pos: Vec2::new(Fx::from_int(x), Fx::from_int(24)),
                owner,
            },
        );
    }
    let factory = EntityId::new(2, 0);
    for _ in 0..3 {
        send(&mut w, 1, P0, Command::QueueUnit { factory, kind: MAN });
    }
    send(
        &mut w,
        1,
        P0,
        Command::SetRally {
            factory,
            cell: Some(cell(14, 34)),
        },
    );
    send(&mut w, 1, P0, Command::SetRepeat { factory, on: true });
    send(
        &mut w,
        2,
        P0,
        Command::BuildStructure {
            kind: HUT,
            cell: cell(8, 4),
        },
    );
    w
}

fn round_trip(w: &World) -> World {
    let bytes = w.save().unwrap();
    let back = World::restore(fresh(), &bytes).unwrap();
    assert_eq!(back.save().unwrap(), bytes, "a resave is the save");
    back
}

/// The economy world after `n` ticks.
fn after(n: u64) -> World {
    let mut w = economy();
    for _ in 0..n {
        w.step();
    }
    w
}

#[test]
fn the_economy_world_holds_what_it_claims() {
    let mut w = economy();
    let mut seen = SaveContents::default();
    let mut max = |a: &mut u32, b: u32| *a = (*a).max(b);
    for _ in 0..600 {
        w.step();
        let c = w.save_contents();
        max(&mut seen.harvesting, c.harvesting);
        max(&mut seen.returning, c.returning);
        max(&mut seen.unloading, c.unloading);
        max(&mut seen.dock_queued, c.dock_queued);
        max(&mut seen.dock_inbox, c.dock_inbox);
        max(&mut seen.bay_reserved, c.bay_reserved);
        max(&mut seen.constructing, c.constructing);
        max(&mut seen.producing, c.producing);
        max(&mut seen.queue_full, c.queue_full);
        max(&mut seen.power_low, c.power_low);
        max(&mut seen.free_items, c.free_items);
        seen.ore_changed |= c.ore_changed;
        seen.spent |= c.spent;
        seen.delivered |= c.delivered;
    }
    assert!(
        w.placement_rejections().is_empty(),
        "{:?}",
        w.placement_rejections()
    );
    let s = seen;
    assert!(
        s.harvesting > 0
            && s.returning > 0
            && s.unloading > 0
            && s.dock_queued > 0
            && s.dock_inbox > 0
            && s.bay_reserved > 0
            && s.constructing > 0
            && s.producing > 0
            && s.queue_full > 0
            && s.power_low > 0
            && s.ore_changed
            && s.spent
            && s.delivered,
        "{s:?}"
    );
}

#[test]
fn the_economy_world_round_trips() {
    for n in [0, 1, 40, 150, 333, 600] {
        let w = after(n);
        let back = round_trip(&w);
        assert_eq!(back.checksum(), w.checksum(), "tick {n}");
        assert_eq!(settled(back), settled(w), "tick {n}");
    }
}

/// The every-tick probe in every build: save, restore and resave after every one of 700 ticks, go
/// on with the restored world, and compare every checksum with the uninterrupted run. Also asserts
/// that the states the WP claims are present at some save tick: a harvester mid-trip and docked,
/// a dock queue and inbox, a structure half built, a full queue mid-item, low power.
#[test]
fn the_economy_world_restores_after_every_tick() {
    let ticks = 700;
    let mut straight = economy();
    let mut want = Vec::new();
    for _ in 0..ticks {
        want.push(straight.step().checksum);
    }
    let mut w = economy();
    let mut seen = [false; 9];
    for (t, &sum) in want.iter().enumerate() {
        assert_eq!(w.step().checksum, sum, "tick {t}");
        let c = w.save_contents();
        let now = [
            c.harvesting > 0,
            c.returning > 0,
            c.unloading > 0,
            c.dock_queued > 0,
            c.dock_inbox > 0,
            c.constructing > 0,
            c.producing > 0 && c.queue_full > 0,
            c.power_low > 0,
            c.repeating > 0,
        ];
        for (s, n) in seen.iter_mut().zip(now) {
            *s |= n;
        }
        let back = round_trip(&w);
        assert_eq!(settled(back.clone()), settled(w.clone()), "tick {t}");
        w = back;
    }
    assert_eq!(seen, [true; 9], "{seen:?}");
}

type Case = (&'static str, &'static str, fn(&mut World, bool));

fn hv(w: &mut World) -> &mut crate::harvest::Harvest {
    w.harvest.as_deref_mut().unwrap()
}

/// One case per restore check of WP13.1c-2: the value at its bound restores (and the world steps
/// on), one past it is refused with the message.
fn cases() -> Vec<Case> {
    vec![
        ("a harvester carries more than it holds", "load", |w, at| {
            hv(w).edit_for_test("load", at)
        }),
        (
            "a harvester timer the harvest stage never sets",
            "wait",
            |w, at| hv(w).edit_for_test("wait", at),
        ),
        (
            "a harvester tried more often than it may",
            "tries",
            |w, at| hv(w).edit_for_test("tries", at),
        ),
        ("a queue spot past the last row", "spot", |w, at| {
            hv(w).edit_for_test("spot", at)
        }),
        ("a harvester cell that is no ore field", "cell", |w, at| {
            hv(w).edit_for_test("cell", at)
        }),
        ("a harvester walk off the map", "own", |w, at| {
            hv(w).edit_for_test("own", at)
        }),
        (
            "a harvester's mail the harvest stage never leaves",
            "mail",
            |w, at| hv(w).edit_for_test("mail", at),
        ),
        ("two harvesters claim one ore cell", "claims", |w, at| {
            hv(w).edit_for_test("claims", at)
        }),
        (
            "more harvester records than entity slots",
            "records",
            |w, at| hv(w).edit_for_test("records", at),
        ),
        (
            "a dock bay the harvest stage never leaves",
            "reserved",
            |w, at| hv(w).edit_for_test("reserved", at),
        ),
        (
            "a dock bay the harvest stage never leaves",
            "unloading",
            |w, at| hv(w).edit_for_test("unloading", at),
        ),
        (
            "a dock queue the harvest stage never leaves",
            "queue",
            |w, at| hv(w).edit_for_test("queue", at),
        ),
        (
            "a dock inbox the harvest stage never leaves",
            "inbox",
            |w, at| hv(w).edit_for_test("inbox", at),
        ),
        ("the docks are out of order", "dock_order", |w, at| {
            hv(w).edit_for_test("dock_order", at)
        }),
        (
            "a dock point off the map or off its cell",
            "dock_point",
            |w, at| hv(w).edit_for_test("dock_point", at),
        ),
        (
            "a dock that is not its refinery's",
            "dock_owner",
            |w, at| hv(w).edit_for_test("dock_owner", at),
        ),
        ("player totals out of order", "delivered", |w, at| {
            hv(w).edit_for_test("delivered", at)
        }),
        ("player totals out of order", "spent", |w, at| {
            w.money.as_mut().unwrap().spent = if at {
                vec![(P0, 1), (P1, 2)]
            } else {
                vec![(P1, 2), (P0, 1)]
            };
        }),
        ("more ore in a cell than the rules allow", "ore", |w, at| {
            // The bound is the maximum of the rules, 80.
            w.ore
                .as_mut()
                .unwrap()
                .set_amount_for_test(cell(25, 19), 80 + u16::from(!at));
        }),
        ("the ore version cannot advance", "ore_version", |w, at| {
            w.ore
                .as_mut()
                .unwrap()
                .set_version_for_test(u64::MAX - u64::from(at));
        }),
        (
            "a construction the construction stage never leaves",
            "build_left",
            |w, at| {
                // The hut (slot 11) builds in 40 ticks.
                w.entities.build_left[11] = 40 + u32::from(!at);
            },
        ),
        (
            "the factories are out of order",
            "factory_order",
            |w, at| {
                let p = w.production.as_deref_mut().unwrap();
                let mut f = p.factories[0].clone();
                f.id = EntityId::new(if at { 30 } else { 0 }, 0);
                f.items.clear();
                f.done = 0;
                p.factories.push(f);
            },
        ),
        ("a rally point off the map", "rally", |w, at| {
            let x = if at { 63 } else { 64 };
            w.production.as_deref_mut().unwrap().factories[0].rally = Some(cell(x, 47));
        }),
        ("a queue longer than its limit", "queue_limit", |w, at| {
            let f = &mut w.production.as_deref_mut().unwrap().factories[0];
            f.items.truncate(1);
            let item = f.items[0];
            let n = if at { 3 } else { 4 };
            f.items.resize(n, item);
        }),
        (
            "a queue item the production stage never holds",
            "free_first",
            |w, at| {
                let f = &mut w.production.as_deref_mut().unwrap().factories[0];
                f.items.truncate(1);
                f.done = 0;
                let free = crate::production::QueueItem {
                    kind: HARVESTER,
                    free: true,
                };
                if at {
                    f.items[0] = free;
                } else {
                    f.items.push(free);
                }
            },
        ),
        (
            "a queue item the production stage never holds",
            "unknown_unit",
            |w, at| {
                let f = &mut w.production.as_deref_mut().unwrap().factories[0];
                f.items.truncate(1);
                f.done = 0;
                f.items[0].kind = if at { MAN } else { HARVESTER };
            },
        ),
        (
            "progress the production stage never leaves",
            "done",
            |w, at| {
                // 30 build ticks, a thousand per tick.
                w.production.as_deref_mut().unwrap().factories[0].done = 30_000 + u32::from(!at);
            },
        ),
        ("power totals out of order", "power", |w, at| {
            let p = w.power.as_mut().unwrap();
            let t = crate::power::PowerTotals { made: 1, used: 2 };
            p.totals = if at {
                vec![(P0, t), (P1, t)]
            } else {
                vec![(P1, t), (P0, t)]
            };
        }),
    ]
}

#[test]
fn every_economy_bound_restores_and_one_past_it_is_refused() {
    let base = after(150);
    let c = base.save_contents();
    assert!(
        c.factories > 0 && c.harvesters >= 3 && c.constructing > 0,
        "{c:?}"
    );
    for (what, name, change) in cases() {
        let mut ok = base.clone();
        change(&mut ok, true);
        let mut back = World::restore(fresh(), &ok.save().unwrap())
            .unwrap_or_else(|e| panic!("{name} at its bound: {e}"));
        for _ in 0..30 {
            back.step();
        }
        let mut bad = base.clone();
        change(&mut bad, false);
        assert_eq!(
            World::restore(fresh(), &bad.save().unwrap()),
            Err(SaveError::Corrupt(what)),
            "{name}"
        );
    }
}

/// A count of ore cells that is not the fresh world's is refused (a byte edit: the world cannot
/// write one).
#[test]
fn another_number_of_ore_cells_is_refused() {
    let bytes = after(20).save().unwrap();
    let (_, start, _) = *super::tests::sections(&bytes)
        .iter()
        .find(|s| s.0 == b'O')
        .unwrap();
    let mut bad = bytes.clone();
    let n = u32::from_le_bytes(bad[start..start + 4].try_into().unwrap());
    bad[start..start + 4].copy_from_slice(&(n - 1).to_le_bytes());
    assert_eq!(
        World::restore(fresh(), &bad),
        Err(SaveError::Corrupt("another number of ore cells"))
    );
}

/// Lesson 3: the saved values no CI scenario holds non-default at a save tick
/// (`the_coverage_record_is_pinned` in `save_economy.rs`), each set here at a value the stages can
/// leave, round trip by `PartialEq` and resave byte-equal.
#[test]
fn fields_no_scenario_reaches_round_trip() {
    use crate::production::FactoryStatus as S;
    let mut w = after(150);
    hv(&mut w).set_unreached_for_test(18 * 64 + 25);
    let p = w.production.as_deref_mut().unwrap();
    let mut extra = Vec::new();
    for (k, status) in [
        S::MissingPrerequisite(PLANT),
        S::Stopped,
        S::NotStanding,
        S::WaitingForExit,
    ]
    .into_iter()
    .enumerate()
    {
        let mut f = p.factories[0].clone();
        f.id = EntityId::new(40 + k as u32, 1);
        f.status = status;
        f.done = 0;
        f.items = if k == 3 {
            vec![crate::production::QueueItem {
                kind: HARVESTER,
                free: true,
            }]
        } else {
            Vec::new()
        };
        extra.push(f);
    }
    p.factories.extend(extra);
    let cov = w.save_coverage();
    for name in [
        "harvester.generation",
        "harvester.focus",
        "harvester.bad",
        "harvester.tries",
        "harvester.phase.search",
        "harvester.phase.park",
        "harvester.phase.called",
        "harvester.phase.manual",
        "dock.owner",
        "dock.side",
        "dock.inbox.request",
        "dock.inbox.cancel",
        "factory.item.free",
        "factory.status.missing_prerequisite",
        "factory.status.stopped",
        "factory.status.not_standing",
    ] {
        assert!(cov.contains(&name), "{name} not set: {cov:?}");
    }
    let back = round_trip(&w);
    assert_eq!(settled(back), settled(w));
}

/// Every economy number is in the rules digest: another value of any of them is other rules.
#[test]
fn other_economy_numbers_are_other_rules() {
    let bytes = after(60).save().unwrap();
    assert!(World::restore(fresh(), &bytes).is_ok());
    assert_eq!(
        World::restore(fresh_with(4, 250), &bytes),
        Err(SaveError::Rules)
    );
    assert_eq!(
        World::restore(fresh_with(3, 300), &bytes),
        Err(SaveError::Rules)
    );
    let changes: [fn(&mut World); 9] = [
        |w| w.ore.as_mut().unwrap().set_regrow_for_test(2),
        |w| hv(w).rules_edit_for_test(0),
        |w| hv(w).rules_edit_for_test(1),
        |w| hv(w).rules_edit_for_test(2),
        |w| hv(w).rules_edit_for_test(3),
        |w| w.money.as_mut().unwrap().rules.credits_per_ore = 3,
        |w| w.power.as_mut().unwrap().rules.defence_cutoff_permille = 999,
        |w| w.production.as_deref_mut().unwrap().rules.build_speed = BuildSpeed { num: 2, den: 2 },
        |w| w.production.as_deref_mut().unwrap().rules.exit_radius = 4,
    ];
    for (k, change) in changes.into_iter().enumerate() {
        let mut other = fresh();
        change(&mut other);
        assert_eq!(
            World::restore(other, &bytes),
            Err(SaveError::Rules),
            "change {k}"
        );
    }
}
