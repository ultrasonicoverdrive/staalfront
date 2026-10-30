//! Tests that need to reach into the world: a structure at zero hit points does not stand, so it
//! is no refinery (`World::is_standing`, the rule WP6.3 extends with "complete"). The public API
//! cannot put a structure at zero hit points without a weapon, so these set the column.

use super::*;
use crate::buildings::{BuildingStats, BuildingTable};
use crate::ids::per_tick;
use crate::terrain::{Terrain, TerrainDef, TerrainFlags, TerrainId};
use crate::units::{Footprint, UnitStats, UnitTable};
use crate::world::WorldConfig;
use crate::{Command, Tick};

const REFINERY: Kind = Kind(2);
const HARVESTER: Kind = Kind(11);

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

fn world() -> World {
    world_with(RULES)
}

fn world_with(rules: HarvestRules) -> World {
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
        UnitStats {
            kind: REFINERY,
            hp: 900,
            movement_class: None,
            speed_per_tick: Fx::ZERO,
            turn_per_tick: 0,
            size: Fx::ZERO,
            footprint: Some(Footprint {
                width: 3,
                height: 4,
            }),
            weapon_range: Fx::ZERO,
            turret_per_tick: 0,
        },
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
    ])
    .unwrap();
    let buildings = BuildingTable::new(vec![BuildingStats {
        kind: REFINERY,
        footprint: Footprint {
            width: 3,
            height: 4,
        },
        function: BuildingFunction::HarvesterDock,
        exit_side: Some(Side::PosX),
        build_ticks: 800,
        cost: Some(1200),
        power: -30,
        factions: 3,
        requires: vec![],
        build_radius: 0,
    }])
    .unwrap();
    World::with_terrain(WorldConfig::new(7).with_map_size(64, 48), terrain)
        .unwrap()
        .with_units(units)
        .unwrap()
        .with_buildings(buildings)
        .unwrap()
        .with_ore(crate::OreRules {
            start: 100,
            max: 100,
            regrow_amount: 0,
            regrow_period: 10,
        })
        .unwrap()
        .with_harvest(rules, &[HARVESTER])
        .unwrap()
}

fn id(slot: u32) -> EntityId {
    EntityId::new(slot, 0)
}

fn place(w: &mut World, cell: (u32, u32)) {
    let command = Command::PlaceStructure {
        kind: REFINERY,
        cell: Cell::new(cell.0, cell.1),
        owner: PlayerId(0),
    };
    w.schedule(Tick(0), PlayerId(0), command).unwrap();
}

/// Two refineries, slot 0 the nearer to the field and slot 1 the farther, and a harvester (slot 2)
/// that has mined a full load and is driving back.
fn returning() -> World {
    let mut w = world();
    place(&mut w, (6, 20)); // slot 0, dock point in the middle of cell (9, 22)
    place(&mut w, (6, 36)); // slot 1, dock point in the middle of cell (9, 38)
    let spawn = Command::SpawnUnit {
        kind: HARVESTER,
        pos: Vec2::new(Fx::from_ratio(29, 2), Fx::from_ratio(45, 2)),
        owner: PlayerId(0),
    };
    w.schedule(Tick(0), PlayerId(0), spawn).unwrap();
    w
}

fn until_returning(w: &mut World) {
    for _ in 0..1500 {
        w.step();
        if w.harvester_of(id(2))
            .is_some_and(|v| v.stage == HarvestStage::ToRefinery)
        {
            return;
        }
    }
    panic!("never started back");
}

#[test]
fn the_nearer_refinery_wins_while_it_stands() {
    let mut w = returning();
    until_returning(&mut w);
    assert_eq!(w.harvester_of(id(2)).unwrap().dock, Some(id(0)));
}

#[test]
fn a_refinery_at_zero_hit_points_is_not_a_refinery() {
    let mut w = returning();
    // Run to just before the harvester is full, then take the nearer refinery down.
    for _ in 0..40 {
        w.step();
    }
    w.entities.hp[0] = 0;
    until_returning(&mut w);
    assert_eq!(w.harvester_of(id(2)).unwrap().dock, Some(id(1)));
    // The dead one has no dock state at all.
    assert!(w.dock_of(id(0)).is_none());
    assert!(w.dock_of(id(1)).is_some());
}

#[test]
fn a_refinery_that_stops_standing_under_a_harvester_on_its_way_sends_it_to_the_other() {
    let mut w = returning();
    until_returning(&mut w);
    // It is on its way to refinery 0; that one stops standing.
    w.entities.hp[0] = 0;
    w.step();
    assert_eq!(w.harvester_of(id(2)).unwrap().dock, Some(id(1)));
}

#[test]
fn a_structure_stands_while_it_has_hit_points() {
    let mut w = world();
    place(&mut w, (6, 20));
    w.step();
    assert!(w.is_standing(0));
    w.entities.hp[0] = 0;
    assert!(!w.is_standing(0));
}

#[test]
fn the_ore_delivered_so_far_is_in_the_checksum() {
    let mut a = world();
    place(&mut a, (6, 20));
    a.step();
    let mut b = a.clone();
    assert_eq!(a.checksum(), b.checksum());
    b.harvest.as_mut().unwrap().credit(PlayerId(0), 5);
    assert_ne!(a.checksum(), b.checksum());
    assert_eq!(b.ore_delivered(PlayerId(0)), 5);
    // The amount counts, not only that there is an entry.
    let mut c = a.clone();
    c.harvest.as_mut().unwrap().credit(PlayerId(0), 6);
    assert_ne!(b.checksum(), c.checksum());
}

// ---- storage (WP7.3) ------------------------------------------------------------------------

fn stored_world() -> World {
    let mut w = world().with_storage(&[(REFINERY, 300)]).unwrap();
    place(&mut w, (6, 20));
    place(&mut w, (6, 30));
    w.step();
    w
}

#[test]
fn a_refinery_at_zero_hit_points_stores_nothing() {
    let mut w = stored_world();
    assert_eq!(w.storage_capacity(PlayerId(0)), 600);
    w.entities.hp[1] = 0;
    w.step();
    assert_eq!(w.storage_capacity(PlayerId(0)), 300);
}

#[test]
fn the_storage_state_is_in_the_checksum() {
    let a = stored_world();
    let base = a.checksum();
    // The stored amount alone (the delivered counter is left as it is), and how much.
    let mut b = a.clone();
    {
        let s = b.harvest.as_mut().unwrap().storage.as_mut().unwrap();
        add_to(&mut s.stored, PlayerId(0), 5);
    }
    assert_ne!(base, b.checksum());
    let mut c = a.clone();
    {
        let s = c.harvest.as_mut().unwrap().storage.as_mut().unwrap();
        add_to(&mut s.stored, PlayerId(0), 6);
    }
    assert_ne!(b.checksum(), c.checksum());
    // The capacity of the last sync.
    let mut d = a.clone();
    d.harvest
        .as_mut()
        .unwrap()
        .storage
        .as_mut()
        .unwrap()
        .capacity[0]
        .1 += 1;
    assert_ne!(base, d.checksum());
    // The table.
    let mut e = a.clone();
    e.harvest.as_mut().unwrap().storage.as_mut().unwrap().table[0].1 += 1;
    assert_ne!(base, e.checksum());
}

#[test]
fn a_world_without_storage_rules_has_no_storage_section() {
    let mut a = world();
    place(&mut a, (6, 20));
    let mut b = a.clone();
    a.step();
    b.step();
    // Turning storage on afterwards (a test-only shortcut) moves the hash; turning it off again
    // brings the old one back: the section is written only with storage rules.
    let before = a.checksum();
    a.harvest
        .as_mut()
        .unwrap()
        .set_storage(vec![(REFINERY, 300)], &b.units);
    assert_ne!(before, a.checksum());
    a.harvest.as_mut().unwrap().storage = None;
    assert_eq!(before, a.checksum());
    assert_eq!(b.checksum(), before);
}

#[test]
fn the_money_state_is_in_the_checksum() {
    // Money rules on top of storage (WP7.4): the price and what each player has spent are hashed,
    // the credits themselves are the stored amount of `V`.
    let mut a = world().with_storage(&[(REFINERY, 300)]).unwrap();
    a = a
        .with_money(
            crate::MoneyRules { credits_per_ore: 2 },
            &[(PlayerId(0), 50)],
        )
        .unwrap();
    place(&mut a, (6, 20));
    a.step();
    let base = a.checksum();
    // The price.
    let mut b = a.clone();
    b.money.as_mut().unwrap().rules.credits_per_ore = 3;
    assert_ne!(base, b.checksum());
    // What was spent, and how much of it, and by whom.
    let mut c = a.clone();
    assert!(c.pay(PlayerId(0), 7));
    assert_ne!(base, c.checksum());
    let mut d = a.clone();
    assert!(d.pay(PlayerId(0), 8));
    assert_ne!(c.checksum(), d.checksum());
    let mut e = a.clone();
    e.money.as_mut().unwrap().spent.push((PlayerId(1), 7));
    assert_ne!(c.checksum(), e.checksum());
    // Spending changes the stored amount as well, so spent alone is checked on its own.
    let mut f = a.clone();
    f.money.as_mut().unwrap().spent.push((PlayerId(0), 7));
    assert_ne!(base, f.checksum());
}

#[test]
fn a_world_without_money_rules_has_no_money_section() {
    let mut a = stored_world();
    let before = a.checksum();
    // A test-only shortcut: money switched on afterwards moves the hash, switched off again it
    // brings the old one back.
    a.money = Some(crate::money::Money {
        rules: crate::MoneyRules { credits_per_ore: 1 },
        spent: Vec::new(),
    });
    assert_ne!(before, a.checksum());
    a.money = None;
    assert_eq!(before, a.checksum());
}

#[test]
fn a_payment_larger_than_the_store_pays_nothing() {
    let mut w = world().with_storage(&[(REFINERY, 300)]).unwrap();
    w = w
        .with_money(
            crate::MoneyRules { credits_per_ore: 1 },
            &[(PlayerId(0), 10)],
        )
        .unwrap();
    assert!(!w.pay(PlayerId(0), 11));
    assert_eq!(
        (w.credits(PlayerId(0)), w.credits_spent(PlayerId(0))),
        (10, 0)
    );
    assert!(w.pay(PlayerId(0), 10));
    assert_eq!(
        (w.credits(PlayerId(0)), w.credits_spent(PlayerId(0))),
        (0, 10)
    );
    // A world without money rules never pays.
    let mut plain = stored_world();
    assert!(!plain.pay(PlayerId(0), 0));
}

// ---- the choice of refinery and the queue spots (WP7.5) ------------------------------------------
//
// Refinery A (slot 0) has its dock point on (9.5, 22.5), refinery B (slot 1) on (9.5, 38.5). The
// harvesters stand east of them. Every weight below is a different number, so a field read from the
// wrong place gives another cost.

/// The two refineries and one harvester per `at` (slots 2, 3, ...), stepped once so that the
/// docks and the harvesters are registered.
fn stepped(rules: HarvestRules, at: &[(i32, i32)]) -> World {
    let mut w = world_with(rules);
    place(&mut w, (6, 20));
    place(&mut w, (6, 36));
    for &(x, y) in at {
        let spawn = Command::SpawnUnit {
            kind: HARVESTER,
            pos: Vec2::new(
                Fx::from_int(x) + Fx::HALF,
                Fx::from_int(y) + Fx::HALF,
            ),
            owner: PlayerId(0),
        };
        w.schedule(Tick(0), PlayerId(0), spawn).unwrap();
    }
    w.step();
    w
}

/// Runs `f` on the harvest state of `w` (taken out of the world for the call, as the tick does).
fn with_state<R>(w: &mut World, f: impl FnOnce(&mut Harvest, &World) -> R) -> R {
    let mut h = w.harvest.take().unwrap();
    let r = f(&mut h, w);
    w.harvest = Some(h);
    r
}

fn pos_of(w: &World, slot: usize) -> Vec2 {
    w.entities.pos[slot]
}

#[test]
fn a_cost_is_the_cells_times_travel_cost_plus_the_committed_times_queue_cost() {
    let rules = HarvestRules {
        travel_cost: 7,
        queue_cost: 300,
        ..RULES
    };
    let mut w = stepped(rules, &[(20, 22)]);
    let pos = pos_of(&w, 2);
    with_state(&mut w, |h, _| {
        // 11 cells to A and sqrt(121 + 256) = 19.42 cells to B, at 7 ticks a cell.
        assert_eq!(h.dock_cost(0, pos, None, &[0, 0]), 77);
        assert_eq!(h.dock_cost(1, pos, None, &[0, 0]), 136);
        // Two committed to A: 2 x 300 more. None to B.
        assert_eq!(h.dock_cost(0, pos, None, &[2, 0]), 677);
        assert_eq!(h.dock_cost(1, pos, None, &[2, 0]), 136);
        // The harvester itself, already bound for A, is not counted against it.
        assert_eq!(h.dock_cost(0, pos, Some(id(0)), &[2, 0]), 377);
        assert_eq!(h.dock_cost(1, pos, Some(id(0)), &[2, 0]), 136);
    });
}

#[test]
fn the_cheapest_refinery_is_chosen_and_a_tie_goes_to_the_nearer_then_the_lower_slot() {
    let at = |queue_cost: u16| HarvestRules {
        travel_cost: 7,
        queue_cost,
        ..RULES
    };
    let owner = PlayerId(0);
    // One committed to A. A costs 77 + q, B 136: q = 59 is a tie on the cost, which goes to the
    // nearer dock point (A); q = 60 makes A dearer and B wins; q = 58 keeps A.
    for (q, want) in [(58, 0), (59, 0), (60, 1)] {
        let mut w = stepped(at(q), &[(20, 22)]);
        let pos = pos_of(&w, 2);
        let got = with_state(&mut w, |h, _| h.choose_dock(owner, pos, None, &[1, 0]));
        assert_eq!(got.map(|(k, _)| k), Some(want), "queue_cost {q}");
    }
    // Equal cost and equal distance (the harvester halfway between): the lower slot.
    let mut w = stepped(at(59), &[(20, 30)]);
    let pos = pos_of(&w, 2);
    let (k, cost) = with_state(&mut w, |h, _| h.choose_dock(owner, pos, None, &[0, 0]).unwrap());
    assert_eq!((k, cost), (0, 95));
    // A different owner has no refinery here.
    let none = with_state(&mut w, |h, _| h.choose_dock(PlayerId(1), pos, None, &[0, 0]));
    assert_eq!(none, None);
}

#[test]
fn queue_spots_lie_in_rows_beside_the_approach() {
    let rules = HarvestRules {
        queue_gap: 3,
        queue_start: 1,
        queue_step: 2,
        queue_rows: 3,
        ..RULES
    };
    let mut w = stepped(rules, &[]);
    // The exit cell of A is (9, 22), the dock point (9.5, 22.5), the exit side +x.
    let spot = |h: &Harvest, w: &World, row: usize, minus: bool| {
        let p = h.queue_point(w, &h.docks[0], row, minus);
        (p.x.raw() as f64 / 65536.0, p.y.raw() as f64 / 65536.0)
    };
    with_state(&mut w, |h, w| {
        // (row, minus side) and where it is: the positive side is +y for this refinery.
        let want = [
            (0, false, (10.5, 25.5)),
            (0, true, (10.5, 19.5)),
            (1, false, (12.5, 25.5)),
            (1, true, (12.5, 19.5)),
            (2, false, (14.5, 25.5)),
            (2, true, (14.5, 19.5)),
            // Past the last row (queue_rows 3) they share it.
            (7, false, (14.5, 25.5)),
            (7, true, (14.5, 19.5)),
        ];
        for (row, minus, want) in want {
            assert_eq!(spot(h, w, row, minus), want, "row {row} minus {minus}");
        }
        // None is in the approach, on either side.
        for row in 0..40 {
            for minus in [false, true] {
                let (_, y) = spot(h, w, row, minus);
                assert!((y - 22.5).abs() >= 3.0, "row {row} at y {y}");
            }
        }
        // The side a position is on.
        let dock = &h.docks[0];
        assert!(!Harvest::on_minus_side(dock, Vec2::new(Fx::from_int(20), Fx::from_int(23))));
        assert!(Harvest::on_minus_side(dock, Vec2::new(Fx::from_int(20), Fx::from_int(22))));
    });
}

#[test]
fn a_driving_harvester_switches_only_when_the_other_refinery_is_cheaper_by_more_than_the_margin() {
    // Bound for A with one other committed there (load 2 with itself): A costs 77 + 300, B 136.
    // B is cheaper by 241.
    for (margin, switches) in [(240, true), (241, false), (0, true), (1000, false)] {
        let rules = HarvestRules {
            travel_cost: 7,
            queue_cost: 300,
            switch_margin: margin,
            ..RULES
        };
        let mut w = stepped(rules, &[(20, 22)]);
        let mut load = [2u16, 0];
        let plan = with_state(&mut w, |h, w| {
            let mut plan = Plan {
                core: Core {
                    bound: true,
                    generation: 0,
                    phase: Phase::ToDock(Call::Driving),
                    dock: Some(id(0)),
                    own: Order::Move(h.docks[0].point),
                    ..Core::NONE
                },
                act: Act::Keep,
                take: None,
                send: None,
                spaced: false,
                focus: NO_CELL,
            };
            let own = h.docks[0].clone();
            h.recheck(w, 2, &own, &mut plan, &mut load);
            plan
        });
        if switches {
            assert_eq!(plan.core.dock, Some(id(1)), "margin {margin}");
            assert_eq!(plan.act, Act::Go(Vec2::new(Fx::from_ratio(19, 2), Fx::from_ratio(77, 2))));
            assert_eq!(load, [1, 1], "margin {margin}");
        } else {
            assert_eq!(plan.core.dock, Some(id(0)), "margin {margin}");
            assert_eq!(plan.act, Act::Keep);
            assert_eq!(load, [2, 0], "margin {margin}");
        }
    }
}

#[test]
fn harvesters_that_set_out_in_one_tick_see_each_others_choice() {
    let rules = HarvestRules {
        travel_cost: 7,
        queue_cost: 300,
        ..RULES
    };
    // Two full harvesters a cell apart: both are nearer to A, but the second finds the first
    // already bound for it in the same pass and takes B.
    let mut w = stepped(rules, &[(20, 22), (20, 23)]);
    let plans = with_state(&mut w, |h, w| {
        for slot in [2, 3] {
            let c = &mut h.units[slot].core;
            c.phase = Phase::Mine;
            c.load = rules.capacity;
        }
        h.decide_all(w)
    });
    let docks: Vec<_> = plans.iter().map(|(_, p)| p.core.dock).collect();
    assert_eq!(docks, [Some(id(0)), Some(id(1))]);
}

#[test]
fn harvesters_bound_for_a_dock_count_against_it_whatever_their_step() {
    let mut w = stepped(RULES, &[(20, 22), (20, 23), (20, 24), (20, 25)]);
    let load = with_state(&mut w, |h, _| {
        for (slot, call) in [Call::Driving, Call::Queued, Call::Granted].into_iter().enumerate() {
            let c = &mut h.units[2 + slot].core;
            c.phase = Phase::ToDock(call);
            c.dock = Some(id(0));
        }
        let c = &mut h.units[5].core;
        c.phase = Phase::Unload;
        c.dock = Some(id(1));
        h.committed()
    });
    assert_eq!(load, [3, 1]);
}

#[test]
fn the_new_numbers_are_in_the_checksum_each_on_its_own() {
    let base = world().checksum();
    let moved = |rules: HarvestRules| world_with(rules).checksum() != base;
    assert!(moved(HarvestRules { travel_cost: 17, ..RULES }));
    assert!(moved(HarvestRules { queue_cost: 101, ..RULES }));
    assert!(moved(HarvestRules { switch_margin: 101, ..RULES }));
    assert!(moved(HarvestRules { recheck_period: 21, ..RULES }));
    assert!(moved(HarvestRules { queue_gap: 4, ..RULES }));
    assert!(moved(HarvestRules { queue_start: 2, ..RULES }));
    assert!(moved(HarvestRules { queue_step: 3, ..RULES }));
    assert!(moved(HarvestRules { queue_rows: 9, ..RULES }));
}
