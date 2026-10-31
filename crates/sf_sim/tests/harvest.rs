//! Harvesters and the refinery dock (WP7.2), through the public API and a hand-made unit and
//! building table, so the rules are tested apart from the shipped data (the bridge and headless
//! tests hold the shipped data against the same rules). The rules are in `docs/rules/economy.md`.
//!
//! Map 64 x 48 of grass. The refinery stands on (6, 20) (3 x 4 cells, exit cell (9, 22), so the
//! dock point is (9.5, 22.5)). Field A is the 4 x 4 cells x 24..28, y 18..22; field B the 4 x 4
//! cells x 24..28, y 34..38. Each ore cell holds 100 and nothing grows back unless a test says so.

use sf_math::{Fx, Vec2};
use sf_sim::{
    BuildingFunction, BuildingStats, BuildingTable, Cell, Command, ConfigError, EntityId,
    Footprint, HarvestRules, HarvestStage, Kind, Order, OreRules, PlayerId, Side, Terrain,
    TerrainDef, TerrainFlags, TerrainId, Tick, UnitStats, UnitTable, World, WorldConfig, per_tick,
};

const GRASS: u8 = 0;
const ORE: u8 = 4;
const REFINERY: Kind = Kind(2);
const HARVESTER: Kind = Kind(11);
const RIFLEMAN: Kind = Kind(12);
const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

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
};

const ORE_RULES: OreRules = OreRules {
    start: 100,
    max: 120,
    regrow_amount: 0,
    regrow_period: 10,
};

fn terrain_defs() -> Vec<TerrainDef> {
    vec![
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
    ]
}

fn units() -> UnitTable {
    UnitTable::new(vec![
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
        UnitStats {
            kind: RIFLEMAN,
            hp: 50,
            movement_class: Some(2),
            speed_per_tick: per_tick(Fx::from_ratio(7, 10)),
            turn_per_tick: 16,
            size: Fx::from_ratio(4, 10),
            footprint: None,
            weapon_range: Fx::ZERO,
            turret_per_tick: 0,
        },
    ])
    .unwrap()
}

fn buildings() -> BuildingTable {
    BuildingTable::new(vec![BuildingStats {
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
    .unwrap()
}

fn is_ore(x: u32, y: u32) -> bool {
    ((24..28).contains(&x) && (18..22).contains(&y))
        || ((24..28).contains(&x) && (34..38).contains(&y))
}

/// The test world with the given harvest rules and ore, with no ore at all when `ore` is false.
fn world_with(rules: HarvestRules, ore: Option<OreRules>, field: bool) -> World {
    let ids: Vec<u8> = (0..48u32)
        .flat_map(|y| (0..64u32).map(move |x| (x, y)))
        .map(|(x, y)| if field && is_ore(x, y) { ORE } else { GRASS })
        .collect();
    let terrain = Terrain::from_layer(64, 48, &ids, &terrain_defs()).unwrap();
    let mut w = World::with_terrain(WorldConfig::new(7).with_map_size(64, 48), terrain)
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_buildings(buildings())
        .unwrap();
    if let Some(o) = ore {
        w = w.with_ore(o).unwrap();
    }
    w.with_harvest(rules, &[HARVESTER]).unwrap()
}

fn world() -> World {
    world_with(RULES, Some(ORE_RULES), true)
}

fn at(w: &mut World, tick: u64, command: Command) {
    w.schedule(Tick(tick), P0, command).unwrap();
}

fn place_refinery(w: &mut World, tick: u64, cell: (u32, u32), owner: PlayerId) {
    at(
        w,
        tick,
        Command::PlaceStructure {
            kind: REFINERY,
            cell: Cell::new(cell.0, cell.1),
            owner,
        },
    );
}

fn spawn_harvester(w: &mut World, tick: u64, pos: (i32, i32), owner: PlayerId) {
    at(
        w,
        tick,
        Command::SpawnUnit {
            kind: HARVESTER,
            pos: Vec2::new(
                Fx::from_int(pos.0) + Fx::HALF,
                Fx::from_int(pos.1) + Fx::HALF,
            ),
            owner,
        },
    );
}

fn run(w: &mut World, ticks: u64) {
    for _ in 0..ticks {
        w.step();
    }
}

/// Runs until `stop` holds (at most `limit` ticks); the tick count it took.
fn run_until(w: &mut World, limit: u64, mut stop: impl FnMut(&World) -> bool) -> u64 {
    for n in 0..limit {
        if stop(w) {
            return n;
        }
        w.step();
    }
    panic!("not reached within {limit} ticks");
}

fn id(slot: u32) -> EntityId {
    EntityId::new(slot, 0)
}

fn stage(w: &World, slot: u32) -> Option<HarvestStage> {
    w.harvester_of(id(slot)).map(|v| v.stage)
}

fn load(w: &World, slot: u32) -> u16 {
    w.harvester_of(id(slot)).map_or(0, |v| v.load)
}

fn field_total(w: &World) -> u64 {
    w.ore().unwrap().total()
}

/// One refinery (slot 0) and one harvester (slot 1) next to it.
fn one_harvester() -> World {
    let mut w = world();
    place_refinery(&mut w, 0, (6, 20), P0);
    spawn_harvester(&mut w, 0, (14, 22), P0);
    w
}

// ---- the loop -----------------------------------------------------------------------------

#[test]
fn a_harvester_runs_the_loop_and_every_unit_is_accounted_for() {
    let mut w = one_harvester();
    let start = field_total(&w);
    assert_eq!(start, 32 * 100);
    // Over time the harvester mines, drives back, unloads and goes again.
    run(&mut w, 3000);
    let delivered = w.ore_delivered(P0);
    // Several full loads were delivered.
    assert!(delivered >= 3 * 100, "delivered {delivered}");
    // Nothing is lost or made: what left the field is what was delivered plus what is carried.
    let taken = start - field_total(&w);
    assert_eq!(taken, delivered + u64::from(load(&w, 1)));
    // Another player got nothing.
    assert_eq!(w.ore_delivered(P1), 0);
}

#[test]
fn the_stages_come_in_the_order_of_the_loop() {
    let mut w = one_harvester();
    let mut seen: Vec<HarvestStage> = Vec::new();
    for _ in 0..1500 {
        w.step();
        if let Some(s) = stage(&w, 1)
            && seen.last() != Some(&s)
        {
            seen.push(s);
        }
    }
    let first_cycle: Vec<HarvestStage> = seen.iter().take(7).copied().collect();
    assert_eq!(
        first_cycle,
        [
            HarvestStage::ToOre,
            HarvestStage::Mining,
            HarvestStage::ToRefinery,
            HarvestStage::Granted,
            HarvestStage::Unloading,
            HarvestStage::ToOre,
            HarvestStage::Mining,
        ],
        "{seen:?}"
    );
}

#[test]
fn a_bite_is_the_data_amount_and_the_load_never_passes_the_capacity() {
    let mut w = one_harvester();
    let mut last = 0u16;
    let mut max = 0u16;
    let mut biggest_bite = 0u16;
    for _ in 0..600 {
        w.step();
        let l = load(&w, 1);
        if l > last {
            biggest_bite = biggest_bite.max(l - last);
        }
        max = max.max(l);
        last = l;
    }
    assert_eq!(biggest_bite, RULES.mine_amount);
    assert_eq!(max, RULES.capacity);
}

#[test]
fn mining_takes_a_bite_every_period_ticks() {
    let mut w = one_harvester();
    run_until(&mut w, 800, |w| stage(w, 1) == Some(HarvestStage::Mining));
    let mut times: Vec<u64> = Vec::new();
    let mut last = load(&w, 1);
    for _ in 0..60 {
        w.step();
        let l = load(&w, 1);
        if l > last {
            times.push(w.tick().0);
        }
        last = l;
    }
    assert!(times.len() >= 5, "{times:?}");
    for pair in times.windows(2) {
        assert_eq!(pair[1] - pair[0], u64::from(RULES.mine_period), "{times:?}");
    }
}

#[test]
fn a_full_load_is_unloaded_in_bites_over_ticks() {
    let mut w = one_harvester();
    run_until(&mut w, 1500, |w| {
        stage(w, 1) == Some(HarvestStage::Unloading)
    });
    let entered = w.tick().0;
    assert_eq!(load(&w, 1), 100);
    let mut loads = vec![(w.tick().0, load(&w, 1))];
    run_until(&mut w, 100, |w| {
        let l = load(w, 1);
        if loads.last().map(|x| x.1) != Some(l) {
            loads.push((w.tick().0, l));
        }
        l == 0
    });
    // 100 units at 20 per bite: five bites, one every unload_period ticks.
    let values: Vec<u16> = loads.iter().map(|x| x.1).collect();
    assert_eq!(values, [100, 80, 60, 40, 20, 0], "{loads:?}");
    for pair in loads.windows(2).skip(1) {
        assert_eq!(pair[1].0 - pair[0].0, u64::from(RULES.unload_period));
    }
    // The first bite is unload_period ticks after the harvester is in the bay (a message tick
    // for the Arrived, then the wait).
    assert!(loads[1].0 > entered);
    assert_eq!(w.ore_delivered(P0), 100);
}

// ---- the messages -------------------------------------------------------------------------

#[test]
fn a_request_is_answered_in_the_tick_after_it_was_sent() {
    let mut w = one_harvester();
    let dock = id(0);
    // Until it is full the dock sees nothing.
    run_until(&mut w, 1500, |w| {
        stage(w, 1) == Some(HarvestStage::ToRefinery)
    });
    assert!(w.dock_of(dock).unwrap().holder.is_none());
    // Within call range the harvester asks. The request is read by the dock in the next tick,
    // and the answer by the harvester in that same tick: one tick for the request, none for the
    // answer.
    let dock_point = Vec2::new(Fx::from_ratio(19, 2), Fx::from_ratio(45, 2));
    run_until(&mut w, 400, |w| {
        (w.get(id(1)).unwrap().pos - dock_point).length_squared() <= Fx::from_int(6).square()
    });
    // The next tick is the one in which the harvester decides to ask.
    w.step();
    assert_eq!(stage(&w, 1), Some(HarvestStage::ToRefinery));
    assert!(w.dock_of(dock).unwrap().holder.is_none());
    // In the one after that the dock has read it and granted the bay, and the harvester has read
    // the answer.
    w.step();
    assert_eq!(stage(&w, 1), Some(HarvestStage::Granted));
    assert_eq!(w.dock_of(dock).unwrap().holder, Some(id(1)));
}

#[test]
fn two_harvesters_on_one_refinery_take_turns_in_the_order_they_asked() {
    let mut w = world();
    place_refinery(&mut w, 0, (6, 20), P0);
    // Both full-load distance from the dock; the second is a little farther, so it asks later.
    spawn_harvester(&mut w, 0, (14, 22), P0);
    spawn_harvester(&mut w, 0, (15, 24), P0);
    let mut saw_queue = false;
    let mut holders: Vec<EntityId> = Vec::new();
    let mut both_in_bay = false;
    for _ in 0..4000 {
        w.step();
        let d = w.dock_of(id(0)).unwrap();
        if !d.queue.is_empty() {
            saw_queue = true;
            // The one in the queue stands.
            let q = d.queue[0];
            assert_eq!(stage(&w, q.index()), Some(HarvestStage::Queued));
        }
        if let Some(h) = d.holder
            && holders.last() != Some(&h)
        {
            holders.push(h);
        }
        let unloading = [1, 2]
            .into_iter()
            .filter(|&s| stage(&w, s) == Some(HarvestStage::Unloading))
            .count();
        both_in_bay |= unloading > 1;
    }
    assert!(saw_queue, "the second harvester never waited");
    assert!(!both_in_bay, "two harvesters in one bay");
    // Both delivered, alternating at least once.
    assert!(
        holders.contains(&id(1)) && holders.contains(&id(2)),
        "{holders:?}"
    );
    assert!(w.ore_delivered(P0) >= 200);
}

#[test]
fn a_grant_that_is_not_used_in_time_is_revoked_and_the_next_in_line_gets_the_bay() {
    // A short timeout: a harvester is granted at the call range, 6 cells out, and needs about 100
    // ticks to arrive, so its first grant runs out before it is ever in the bay.
    let rules = HarvestRules {
        grant_timeout: 10,
        ..RULES
    };
    let mut w = world_with(rules, Some(ORE_RULES), true);
    place_refinery(&mut w, 0, (6, 20), P0);
    spawn_harvester(&mut w, 0, (14, 22), P0);
    spawn_harvester(&mut w, 0, (15, 24), P0);
    spawn_harvester(&mut w, 0, (16, 26), P0);
    let mut prev = w.dock_of(id(0));
    let mut first_revoke: Option<(EntityId, u64)> = None;
    let mut handovers = 0;
    let mut unloaded_before_revoke = false;
    for _ in 0..4000 {
        w.step();
        let cur = w.dock_of(id(0));
        if let (Some(p), Some(c)) = (&prev, &cur) {
            if c.unloading && first_revoke.is_none() {
                unloaded_before_revoke = true;
            }
            // The holder lost a grant it never used: not unloading, still alive, not ordered.
            if let Some(h) = p.holder
                && !p.unloading
                && c.holder != Some(h)
            {
                if first_revoke.is_none() {
                    first_revoke = Some((h, w.ore_delivered(P0)));
                    // It drives on: it was not taken away, only made to ask again.
                    assert!(w.get(h).is_some());
                }
                // The first of the queue gets the bay in the same tick.
                if let Some(&next) = p.queue.first() {
                    assert_eq!(c.holder, Some(next), "{p:?} -> {c:?}");
                    handovers += 1;
                }
            }
        }
        prev = cur;
    }
    let (_, delivered) = first_revoke.expect("a grant ran out");
    assert!(
        !unloaded_before_revoke,
        "someone unloaded before any grant ran out"
    );
    assert_eq!(delivered, 0, "ore delivered before the first grant ran out");
    assert!(
        handovers >= 1,
        "a queued harvester never took over a lost grant"
    );
    // Revoked harvesters ask again and get in when they are near enough.
    assert!(w.ore_delivered(P0) > 0);
}

/// Orders the dock's holder away as soon as `ready` holds with someone queued behind it, and
/// checks that the bay goes to the first of the queue and that the holder gets nothing more.
fn order_the_holder_away(ready: impl Fn(&sf_sim::DockView) -> bool) {
    let mut w = world();
    place_refinery(&mut w, 0, (6, 20), P0);
    spawn_harvester(&mut w, 0, (14, 22), P0);
    spawn_harvester(&mut w, 0, (15, 24), P0);
    spawn_harvester(&mut w, 0, (16, 26), P0);
    run(&mut w, 2);
    run_until(&mut w, 4000, |w| {
        w.dock_of(id(0))
            .is_some_and(|d| d.holder.is_some() && !d.queue.is_empty() && ready(&d))
    });
    let d = w.dock_of(id(0)).unwrap();
    assert!(ready(&d) && !d.queue.is_empty(), "never ready: {d:?}");
    let (holder, next) = (d.holder.unwrap(), d.queue[0]);
    let tick = w.tick().0;
    at(
        &mut w,
        tick,
        Command::Order {
            ids: vec![holder],
            order: Order::Move(Vec2::from_ints(40, 5)),
            queued: false,
        },
    );
    w.step();
    w.step();
    let d = w.dock_of(id(0)).unwrap();
    assert_eq!(d.holder, Some(next), "{d:?}");
    assert!(!d.queue.contains(&holder));
    assert_eq!(stage(&w, holder.index()), Some(HarvestStage::Manual));
    // Nothing more is taken off the departed harvester.
    let kept = load(&w, holder.index());
    for _ in 0..200 {
        w.step();
        assert_eq!(load(&w, holder.index()), kept);
    }
}

#[test]
fn a_granted_holder_ordered_away_frees_the_bay_for_the_next_in_line() {
    order_the_holder_away(|d| !d.unloading);
}

#[test]
fn an_unloading_holder_ordered_away_frees_the_bay_for_the_next_in_line() {
    order_the_holder_away(|d| d.unloading);
}

#[test]
fn two_harvesters_ordered_to_one_ore_cell_in_one_tick_both_keep_the_field() {
    // Both stand by the northern field; the player sends both to the southern one in one tick.
    // The second loses the cell to the first in that tick and must still search the field the
    // player chose, not the one it stands next to.
    let mut w = world();
    place_refinery(&mut w, 0, (6, 20), P0);
    spawn_harvester(&mut w, 0, (22, 20), P0);
    spawn_harvester(&mut w, 0, (22, 22), P0);
    run(&mut w, 3);
    let target = Vec2::from_ints(25, 35) + Vec2::new(Fx::HALF, Fx::HALF);
    let tick = w.tick().0;
    at(
        &mut w,
        tick,
        Command::Order {
            ids: vec![id(1), id(2)],
            order: Order::Move(target),
            queued: false,
        },
    );
    let south = |c: Option<Cell>| c.is_some_and(|c| (34..38).contains(&c.y));
    for _ in 0..5 {
        w.step();
    }
    for slot in [1, 2] {
        let cell = w.harvester_of(id(slot)).and_then(|v| v.cell);
        assert!(south(cell), "slot {slot} went to {cell:?}");
    }
}

#[test]
fn a_player_order_cancels_the_check_in() {
    let mut w = world();
    place_refinery(&mut w, 0, (6, 20), P0);
    spawn_harvester(&mut w, 0, (14, 22), P0);
    spawn_harvester(&mut w, 0, (15, 24), P0);
    // Wait until one of them stands in the queue.
    run(&mut w, 2);
    run_until(&mut w, 2500, |w| {
        !w.dock_of(id(0)).unwrap().queue.is_empty()
    });
    let queued = w.dock_of(id(0)).unwrap().queue[0];
    let tick = w.tick().0;
    at(
        &mut w,
        tick,
        Command::Order {
            ids: vec![queued],
            order: Order::Move(Vec2::from_ints(40, 5)),
            queued: false,
        },
    );
    w.step();
    w.step();
    assert!(w.dock_of(id(0)).unwrap().queue.is_empty());
    assert_eq!(stage(&w, queued.index()), Some(HarvestStage::Manual));
}

// ---- no ore, no refinery ------------------------------------------------------------------

#[test]
fn without_ore_a_harvester_idles_near_its_refinery_and_looks_again_later() {
    // Every field cell gains 100 on its turn, once in 600 ticks (the turns are in ticks 0 to 31
    // of each period). The field is taken empty at tick 40, so from then on there is no ore until
    // tick 600.
    let rules = OreRules {
        start: 100,
        max: 100,
        regrow_amount: 100,
        regrow_period: 600,
    };
    let mut w = world_with(RULES, Some(rules), true);
    place_refinery(&mut w, 0, (6, 20), P0);
    spawn_harvester(&mut w, 100, (30, 30), P0);
    for (x, y) in (0..64u32)
        .flat_map(|x| (0..48u32).map(move |y| (x, y)))
        .filter(|&(x, y)| is_ore(x, y))
    {
        at(
            &mut w,
            40,
            Command::TakeOre {
                cell: Cell::new(x, y),
                amount: 100,
            },
        );
    }
    run(&mut w, 130);
    assert_eq!(stage(&w, 1), Some(HarvestStage::Parked));
    // It drives to a spot a few cells out from the refinery's exit and stays there.
    run(&mut w, 400);
    assert_eq!(stage(&w, 1), Some(HarvestStage::Parked));
    assert_eq!(w.tick().0, 530);
    let pos = w.get(id(1)).unwrap().pos;
    let dock_point = Vec2::new(Fx::from_ratio(19, 2), Fx::from_ratio(45, 2));
    assert!(
        (pos - dock_point).length_squared() <= Fx::from_int(8).square(),
        "parked at {pos:?}"
    );
    assert!(load(&w, 1) == 0);
    // The field is back at tick 600; at a later look (every 20 ticks) the harvester finds it.
    run_until(&mut w, 400, |w| load(w, 1) > 0);
    assert!(w.tick().0 >= 600, "found ore at {}", w.tick().0);
    assert!(w.tick().0 <= 600 + 32 + 20 + 400);
}

#[test]
fn without_a_refinery_a_full_harvester_holds_its_load_and_waits() {
    let mut w = world();
    spawn_harvester(&mut w, 0, (14, 22), P0);
    run(&mut w, 1500);
    assert_eq!(stage(&w, 0), Some(HarvestStage::NoRefinery));
    assert_eq!(load(&w, 0), 100);
    assert_eq!(w.ore_delivered(P0), 0);
    // A refinery appears (placed by a command at a later tick): it drives there and unloads.
    let t = w.tick().0;
    place_refinery(&mut w, t, (6, 20), P0);
    run(&mut w, 1200);
    assert!(w.ore_delivered(P0) >= 100);
}

#[test]
fn a_refinery_of_another_player_is_not_a_refinery() {
    let mut w = world();
    place_refinery(&mut w, 0, (6, 20), P1);
    spawn_harvester(&mut w, 0, (14, 22), P0);
    run(&mut w, 1500);
    assert_eq!(stage(&w, 1), Some(HarvestStage::NoRefinery));
    assert_eq!(w.ore_delivered(P0), 0);
    assert_eq!(w.ore_delivered(P1), 0);
}

// ---- losses -------------------------------------------------------------------------------

#[test]
fn a_destroyed_harvester_loses_its_load() {
    let mut w = one_harvester();
    let start = field_total(&w);
    run_until(&mut w, 1500, |w| load(w, 1) >= 60);
    let carried = u64::from(load(&w, 1));
    let tick = w.tick().0;
    at(&mut w, tick, Command::Despawn { id: id(1) });
    run(&mut w, 2);
    assert!(w.harvester_of(id(1)).is_none());
    run(&mut w, 600);
    // Nothing was delivered, and the ore it carried is gone from the world.
    assert_eq!(w.ore_delivered(P0), 0);
    assert_eq!(start - field_total(&w), carried);
}

#[test]
fn a_destroyed_harvester_frees_the_bay_and_its_ore_cell() {
    let mut w = world();
    place_refinery(&mut w, 0, (6, 20), P0);
    spawn_harvester(&mut w, 0, (14, 22), P0);
    spawn_harvester(&mut w, 0, (15, 24), P0);
    // Kill the one that holds the bay while the other waits.
    run(&mut w, 2);
    run_until(&mut w, 2500, |w| {
        !w.dock_of(id(0)).unwrap().queue.is_empty()
    });
    let holder = w.dock_of(id(0)).unwrap().holder.unwrap();
    let other = w.dock_of(id(0)).unwrap().queue[0];
    let tick = w.tick().0;
    at(&mut w, tick, Command::Despawn { id: holder });
    run(&mut w, 5);
    let d = w.dock_of(id(0)).unwrap();
    assert_eq!(d.holder, Some(other));
    assert!(d.queue.is_empty());
}

#[test]
fn a_refinery_destroyed_under_a_docked_harvester_leaves_it_the_rest_of_its_load() {
    let mut w = one_harvester();
    run_until(&mut w, 1500, |w| {
        stage(w, 1) == Some(HarvestStage::Unloading)
    });
    // Two bites, then the refinery goes.
    run_until(&mut w, 100, |w| load(w, 1) <= 60);
    let tick = w.tick().0;
    at(&mut w, tick, Command::Despawn { id: id(0) });
    run(&mut w, 40);
    let kept = load(&w, 1);
    assert_eq!(kept, 60);
    assert_eq!(w.ore_delivered(P0), 40);
    assert_eq!(stage(&w, 1), Some(HarvestStage::NoRefinery));
    // A new refinery later: it delivers the rest.
    let t = w.tick().0;
    place_refinery(&mut w, t, (6, 20), P0);
    run(&mut w, 800);
    assert!(w.ore_delivered(P0) >= 100);
}

#[test]
fn a_refinery_destroyed_under_a_queued_harvester_sends_it_to_the_next_one() {
    let mut w = world();
    place_refinery(&mut w, 0, (6, 20), P0);
    place_refinery(&mut w, 0, (6, 30), P0);
    spawn_harvester(&mut w, 0, (14, 22), P0);
    spawn_harvester(&mut w, 0, (15, 24), P0);
    // Both go to the first refinery (nearer); the second stands in its queue.
    run(&mut w, 2);
    run_until(&mut w, 2500, |w| {
        !w.dock_of(id(0)).unwrap().queue.is_empty()
    });
    let queued = w.dock_of(id(0)).unwrap().queue[0];
    let tick = w.tick().0;
    at(&mut w, tick, Command::Despawn { id: id(0) });
    run(&mut w, 3);
    let v = w.harvester_of(queued).unwrap();
    assert_eq!(v.dock, Some(id(1)), "{v:?}");
    run(&mut w, 1500);
    assert!(w.ore_delivered(P0) >= 200);
}

#[test]
fn the_nearest_standing_refinery_wins() {
    let mut w = world();
    place_refinery(&mut w, 0, (6, 20), P0); // slot 0, near the field
    place_refinery(&mut w, 0, (6, 36), P0); // slot 1
    spawn_harvester(&mut w, 0, (40, 38), P0); // slot 2, nearer to the second
    run_until(&mut w, 1500, |w| {
        matches!(
            stage(w, 2),
            Some(HarvestStage::ToRefinery | HarvestStage::Granted)
        )
    });
    // From (40, 38) the refinery at (6, 36) is nearer than the one at (6, 20).
    assert_eq!(w.harvester_of(id(2)).unwrap().dock, Some(id(1)));
}

// ---- a player order -----------------------------------------------------------------------

#[test]
fn a_player_order_to_a_field_overrides_the_search() {
    let mut w = world();
    place_refinery(&mut w, 0, (6, 20), P0);
    // From (14, 22) the nearest ore is field A.
    spawn_harvester(&mut w, 0, (14, 22), P0);
    run(&mut w, 5);
    let a = w.harvester_of(id(1)).unwrap().cell.unwrap();
    assert!((24..28).contains(&a.x) && (18..22).contains(&a.y), "{a:?}");
    // The player orders it to a cell of field B.
    let tick = w.tick().0;
    at(
        &mut w,
        tick,
        Command::Order {
            ids: vec![id(1)],
            order: Order::Move(Vec2::new(
                Fx::from_int(26) + Fx::HALF,
                Fx::from_int(36) + Fx::HALF,
            )),
            queued: false,
        },
    );
    w.step();
    let b = w.harvester_of(id(1)).unwrap().cell.unwrap();
    assert!((24..28).contains(&b.x) && (34..38).contains(&b.y), "{b:?}");
    assert_eq!(b, Cell::new(26, 36));
    // It mines there.
    run_until(&mut w, 1500, |w| load(w, 1) > 0);
    assert_eq!(stage(&w, 1), Some(HarvestStage::Mining));
    assert!(w.ore_at(Cell::new(26, 36)) < 100);
    // Field A was not touched.
    for x in 24..28 {
        for y in 18..22 {
            assert_eq!(w.ore_at(Cell::new(x, y)), 100);
        }
    }
}

#[test]
fn a_player_order_that_is_not_a_field_is_obeyed_and_the_loop_resumes_after_it() {
    let mut w = one_harvester();
    run(&mut w, 5);
    let tick = w.tick().0;
    at(
        &mut w,
        tick,
        Command::Order {
            ids: vec![id(1)],
            order: Order::Move(Vec2::from_ints(50, 5)),
            queued: false,
        },
    );
    run(&mut w, 3);
    assert_eq!(stage(&w, 1), Some(HarvestStage::Manual));
    assert_eq!(w.harvester_of(id(1)).unwrap().cell, None);
    // It goes there and stays Manual on the way.
    run(&mut w, 100);
    assert_eq!(stage(&w, 1), Some(HarvestStage::Manual));
    // Once there it goes back to work.
    run_until(&mut w, 3000, |w| load(w, 1) > 0);
    assert!(w.get(id(1)).unwrap().pos.x < Fx::from_int(50));
}

// ---- claims -------------------------------------------------------------------------------

#[test]
fn harvesters_claim_different_cells() {
    let mut w = world();
    place_refinery(&mut w, 0, (6, 20), P0);
    for k in 0..4 {
        spawn_harvester(&mut w, 0, (14, 20 + k), P0);
    }
    run(&mut w, 12);
    let cells: Vec<Cell> = (1..=4)
        .map(|s| w.harvester_of(id(s)).unwrap().cell.unwrap())
        .collect();
    for (a, ca) in cells.iter().enumerate() {
        for cb in &cells[a + 1..] {
            assert_ne!(ca, cb, "{cells:?}");
        }
    }
}

#[test]
fn a_harvester_is_not_sent_to_a_cell_another_one_is_driving_to() {
    // The same four, but they all start on the same spot: the same tick, the same nearest cell.
    // The lowest slot keeps it; the rest look again the next tick.
    let mut w = world();
    place_refinery(&mut w, 0, (6, 20), P0);
    for _ in 0..3 {
        spawn_harvester(&mut w, 0, (14, 22), P0);
    }
    run(&mut w, 1);
    let first = w.harvester_of(id(1)).unwrap().cell;
    assert!(first.is_some());
    run(&mut w, 2);
    let cells: Vec<Cell> = (1..=3)
        .map(|s| w.harvester_of(id(s)).unwrap().cell.unwrap())
        .collect();
    assert_eq!(Some(cells[0]), first, "the lowest slot keeps its cell");
    assert!(cells[0] != cells[1] && cells[0] != cells[2] && cells[1] != cells[2]);
}

#[test]
fn harvesters_do_not_claim_cells_next_to_each_other_while_there_is_room() {
    let mut w = world();
    place_refinery(&mut w, 0, (6, 20), P0);
    for k in 0..4 {
        spawn_harvester(&mut w, 0, (14, 20 + k), P0);
    }
    run(&mut w, 12);
    let cells: Vec<Cell> = (1..=4)
        .map(|s| w.harvester_of(id(s)).unwrap().cell.unwrap())
        .collect();
    // Field A is 4 x 4 cells: four harvesters fit with a cell between every two of them.
    for (a, ca) in cells.iter().enumerate() {
        for cb in &cells[a + 1..] {
            let apart = ca.x.abs_diff(cb.x).max(ca.y.abs_diff(cb.y));
            assert!(apart >= 2, "{ca:?} and {cb:?} are next to each other");
        }
    }
}

#[test]
fn when_there_is_no_room_a_harvester_takes_any_cell_nobody_has() {
    // Field A is 16 cells; eight harvesters cannot all keep a cell between them, but every one of
    // them still gets a cell of its own.
    let mut w = world();
    place_refinery(&mut w, 0, (6, 20), P0);
    for k in 0..8 {
        spawn_harvester(&mut w, 0, (14, 18 + k), P0);
    }
    run(&mut w, 40);
    let mut cells: Vec<Cell> = (1..=8)
        .filter_map(|s| w.harvester_of(id(s)).and_then(|v| v.cell))
        .collect();
    assert_eq!(cells.len(), 8, "a harvester was left without a cell");
    cells.sort_by_key(|c| (c.y, c.x));
    cells.dedup();
    assert_eq!(cells.len(), 8, "two harvesters share a cell");
}

#[test]
fn the_queue_is_first_come_first_served() {
    let mut w = world();
    place_refinery(&mut w, 0, (6, 20), P0);
    // Three harvesters, each a little farther from the field than the one before: they come back
    // full in that order, a few ticks apart.
    for k in 0..3 {
        spawn_harvester(&mut w, 0, (14 + 3 * k, 19 + 2 * k), P0);
    }
    run(&mut w, 2);
    let mut asked: Vec<EntityId> = Vec::new();
    let mut holders: Vec<EntityId> = Vec::new();
    for _ in 0..4000 {
        w.step();
        let d = w.dock_of(id(0)).unwrap();
        for h in d.holder.into_iter().chain(d.queue.iter().copied()) {
            if !asked.contains(&h) {
                asked.push(h);
            }
        }
        if let Some(h) = d.holder
            && holders.last() != Some(&h)
        {
            holders.push(h);
        }
        if holders.len() >= 3 {
            break;
        }
    }
    assert_eq!(holders.len(), 3, "{holders:?}");
    // The order they were first seen at the dock is the order they got the bay.
    assert_eq!(holders, asked[..3], "asked {asked:?}, served {holders:?}");
}

#[test]
fn a_destroyed_harvester_releases_its_claim_on_an_ore_cell() {
    let mut w = world();
    place_refinery(&mut w, 0, (6, 20), P0);
    spawn_harvester(&mut w, 0, (14, 22), P0);
    run(&mut w, 3);
    let claimed = w.harvester_of(id(1)).unwrap().cell.unwrap();
    // It dies. In the same tick a second refinery takes the slot it leaves, so that the harvester
    // that appears next is in a slot of its own and nothing but the released claim stands between
    // it and the cell.
    at(&mut w, 3, Command::Despawn { id: id(1) });
    place_refinery(&mut w, 3, (6, 30), P0);
    spawn_harvester(&mut w, 5, (14, 22), P0);
    run(&mut w, 12);
    let again = w
        .harvester_of(id(2))
        .and_then(|v| v.cell)
        .expect("the new harvester has a cell");
    assert_eq!(again, claimed);
}

#[test]
fn a_load_is_never_above_the_capacity_even_when_a_bite_does_not_divide_it() {
    let rules = HarvestRules {
        capacity: 102,
        ..RULES
    };
    let mut w = world_with(rules, Some(ORE_RULES), true);
    place_refinery(&mut w, 0, (6, 20), P0);
    spawn_harvester(&mut w, 0, (14, 22), P0);
    let mut most = 0;
    for _ in 0..1200 {
        w.step();
        most = most.max(load(&w, 1));
    }
    // 20 bites of 5 and one of 2.
    assert_eq!(most, 102);
}

#[test]
fn a_harvester_destroyed_in_the_bay_frees_it_for_the_next_in_line() {
    let mut w = world();
    place_refinery(&mut w, 0, (6, 20), P0);
    spawn_harvester(&mut w, 0, (14, 22), P0);
    spawn_harvester(&mut w, 0, (15, 24), P0);
    run(&mut w, 2);
    run_until(&mut w, 3000, |w| {
        w.dock_of(id(0))
            .is_some_and(|d| d.unloading && !d.queue.is_empty())
    });
    let d = w.dock_of(id(0)).unwrap();
    let (holder, next) = (d.holder.unwrap(), d.queue[0]);
    let tick = w.tick().0;
    at(&mut w, tick, Command::Despawn { id: holder });
    run(&mut w, 4);
    let d = w.dock_of(id(0)).unwrap();
    assert_eq!(d.holder, Some(next));
}

// ---- configuration ------------------------------------------------------------------------

#[test]
fn the_rules_must_be_valid_early_and_name_mobile_kinds() {
    let base = || {
        World::new(WorldConfig::new(1).with_map_size(16, 16))
            .unwrap()
            .with_units(units())
            .unwrap()
            .with_buildings(buildings())
            .unwrap()
    };
    assert!(base().with_harvest(RULES, &[HARVESTER]).is_ok());
    let bad = |rules: HarvestRules| {
        assert_eq!(
            base().with_harvest(rules, &[HARVESTER]).err(),
            Some(ConfigError::Harvest),
            "{rules:?}"
        );
    };
    bad(HarvestRules {
        capacity: 0,
        ..RULES
    });
    bad(HarvestRules {
        mine_amount: 0,
        ..RULES
    });
    bad(HarvestRules {
        mine_amount: 101,
        ..RULES
    });
    bad(HarvestRules {
        mine_period: 0,
        ..RULES
    });
    bad(HarvestRules {
        unload_amount: 0,
        ..RULES
    });
    bad(HarvestRules {
        unload_period: 0,
        ..RULES
    });
    bad(HarvestRules {
        search_radius: 0,
        ..RULES
    });
    bad(HarvestRules {
        retry_period: 0,
        ..RULES
    });
    bad(HarvestRules {
        call_range: 0,
        ..RULES
    });
    bad(HarvestRules {
        grant_timeout: 0,
        ..RULES
    });
    // The tuning numbers (harvest.ron since the fix round).
    for rules in [
        HarvestRules {
            call_timeout: 0,
            ..RULES
        },
        HarvestRules {
            call_timeout: 1001,
            ..RULES
        },
        HarvestRules {
            max_tries: 0,
            ..RULES
        },
        HarvestRules {
            max_tries: 21,
            ..RULES
        },
        HarvestRules {
            park_distance: 0,
            ..RULES
        },
        HarvestRules {
            park_distance: 33,
            ..RULES
        },
        HarvestRules {
            park_spread: 0,
            ..RULES
        },
        HarvestRules {
            park_spread: 33,
            ..RULES
        },
        HarvestRules {
            park_near_tenths: 0,
            ..RULES
        },
        HarvestRules {
            park_near_tenths: 321,
            ..RULES
        },
        HarvestRules {
            mine_start_tenths: 0,
            ..RULES
        },
        HarvestRules {
            mine_start_tenths: 16,
            ..RULES
        },
        HarvestRules {
            mine_reach_tenths: 25,
            ..RULES
        },
        HarvestRules {
            mine_lose_tenths: 321,
            ..RULES
        },
        HarvestRules {
            dock_reach_tenths: 0,
            ..RULES
        },
        HarvestRules {
            dock_reach_tenths: 321,
            ..RULES
        },
    ] {
        bad(rules);
    }
    // No kinds, a structure, an unknown kind.
    assert_eq!(
        base().with_harvest(RULES, &[]).err(),
        Some(ConfigError::Harvest)
    );
    assert_eq!(
        base().with_harvest(RULES, &[REFINERY]).err(),
        Some(ConfigError::Harvest)
    );
    assert_eq!(
        base().with_harvest(RULES, &[Kind(99)]).err(),
        Some(ConfigError::Harvest)
    );
    // After the world has started.
    let mut started = base();
    started.step();
    assert_eq!(
        started.with_harvest(RULES, &[HARVESTER]).err(),
        Some(ConfigError::Harvest)
    );
    // Without a unit table.
    assert_eq!(
        World::new(WorldConfig::new(1).with_map_size(16, 16))
            .unwrap()
            .with_harvest(RULES, &[HARVESTER])
            .err(),
        Some(ConfigError::Harvest)
    );
    // Without a building table: there would be no refinery to find.
    assert_eq!(
        World::new(WorldConfig::new(1).with_map_size(16, 16))
            .unwrap()
            .with_units(units())
            .unwrap()
            .with_harvest(RULES, &[HARVESTER])
            .err(),
        Some(ConfigError::Harvest)
    );
    // A unit or building table after the rules: the harvester and dock kinds were taken from the
    // tables the rules were given with.
    let harvesting = || base().with_harvest(RULES, &[HARVESTER]).unwrap();
    assert_eq!(
        harvesting().with_units(units()).err(),
        Some(ConfigError::Harvest)
    );
    assert_eq!(
        harvesting().with_buildings(buildings()).err(),
        Some(ConfigError::Harvest)
    );
}

#[test]
fn a_world_without_harvest_rules_has_no_harvest_state_and_its_units_just_stand() {
    let mut w = World::new(WorldConfig::new(1).with_map_size(16, 16))
        .unwrap()
        .with_units(units())
        .unwrap();
    assert!(w.harvest_rules().is_none());
    assert!(w.harvester_kinds().is_empty());
    at(
        &mut w,
        0,
        Command::SpawnUnit {
            kind: HARVESTER,
            pos: Vec2::from_ints(5, 5),
            owner: P0,
        },
    );
    run(&mut w, 10);
    assert!(w.harvester_of(id(0)).is_none());
    assert_eq!(w.ore_delivered(P0), 0);
    assert_eq!(w.get(id(0)).unwrap().pos, Vec2::from_ints(5, 5));
}

#[test]
fn the_world_reports_its_rules() {
    let w = world();
    assert_eq!(w.harvest_rules(), Some(&RULES));
    assert_eq!(w.harvester_kinds(), [HARVESTER]);
}

// ---- the checksum -------------------------------------------------------------------------

#[test]
fn the_checksum_covers_the_harvest_state() {
    let mut a = one_harvester();
    let mut b = one_harvester();
    run(&mut a, 200);
    run(&mut b, 200);
    assert_eq!(a.checksum(), b.checksum());
    // A different load, a different world.
    let rules = HarvestRules {
        mine_amount: 4,
        ..RULES
    };
    let mut c = world_with(rules, Some(ORE_RULES), true);
    place_refinery(&mut c, 0, (6, 20), P0);
    spawn_harvester(&mut c, 0, (14, 22), P0);
    run(&mut c, 200);
    assert_ne!(a.checksum(), c.checksum());
}

#[test]
fn the_checksum_has_a_harvest_section_only_with_harvest_rules() {
    // The same world with and without the rules differs from the first tick.
    let plain = |harvest: bool| {
        let mut w = World::new(WorldConfig::new(1).with_map_size(16, 16))
            .unwrap()
            .with_units(units())
            .unwrap()
            .with_buildings(buildings())
            .unwrap();
        if harvest {
            w = w.with_harvest(RULES, &[HARVESTER]).unwrap();
        }
        w.step().checksum
    };
    assert_ne!(plain(true), plain(false));
}

#[test]
fn a_clone_runs_on_identically() {
    let mut a = one_harvester();
    run(&mut a, 700);
    let mut b = a.clone();
    run(&mut a, 700);
    run(&mut b, 700);
    assert_eq!(a.checksum_history(), b.checksum_history());
    assert!(a == b);
}

// ---- construction (WP6.3) -----------------------------------------------------------------

const YARD: Kind = Kind(1);

/// The test world with a base anchor (`YARD`, kind 1) in the tables and player 0's faction, so
/// that player 0 can build with `BuildStructure` (WP6.3).
fn yard_world() -> World {
    let mut stats: Vec<UnitStats> = units().iter().cloned().collect();
    stats.push(UnitStats {
        kind: YARD,
        hp: 1000,
        movement_class: None,
        speed_per_tick: Fx::ZERO,
        turn_per_tick: 0,
        size: Fx::ZERO,
        footprint: Some(Footprint {
            width: 2,
            height: 2,
        }),
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    });
    let mut entries: Vec<BuildingStats> = buildings().iter().cloned().collect();
    entries.push(BuildingStats {
        kind: YARD,
        footprint: Footprint {
            width: 2,
            height: 2,
        },
        function: BuildingFunction::Anchor,
        exit_side: None,
        build_ticks: 100,
        cost: Some(1000),
        power: 0,
        factions: 3,
        requires: vec![],
        build_radius: 8,
    });
    let ids: Vec<u8> = (0..48u32)
        .flat_map(|y| (0..64u32).map(move |x| (x, y)))
        .map(|(x, y)| if is_ore(x, y) { ORE } else { GRASS })
        .collect();
    let terrain = Terrain::from_layer(64, 48, &ids, &terrain_defs()).unwrap();
    World::with_terrain(WorldConfig::new(7).with_map_size(64, 48), terrain)
        .unwrap()
        .with_units(UnitTable::new(stats).unwrap())
        .unwrap()
        .with_buildings(BuildingTable::new(entries).unwrap())
        .unwrap()
        .with_player_factions(&[(P0, 1)])
        .unwrap()
        .with_ore(ORE_RULES)
        .unwrap()
        .with_harvest(RULES, &[HARVESTER])
        .unwrap()
}

#[test]
fn a_refinery_under_construction_is_not_a_dock_and_becomes_one_when_complete() {
    // The obligation of whichever of #76 and #78 landed second: a refinery that a player builds
    // with `BuildStructure` starts under construction (WP6.3); harvest asks `World::is_standing`,
    // which is "complete and with hit points", so it is no dock until it is done.
    let mut w = yard_world();
    at(
        &mut w,
        0,
        Command::PlaceStructure {
            kind: YARD,
            cell: Cell::new(3, 20),
            owner: P0,
        },
    );
    spawn_harvester(&mut w, 0, (14, 22), P0);
    at(
        &mut w,
        1,
        Command::BuildStructure {
            kind: REFINERY,
            cell: Cell::new(6, 20),
        },
    );
    run(&mut w, 2);
    let refinery = id(2);
    assert_eq!(w.get(refinery).map(|e| e.kind), Some(REFINERY));
    assert!(w.construction_of(refinery).is_some());
    assert!(!w.stands(refinery));
    // The refinery's build takes 800 ticks: the harvester fills up long before and finds no dock.
    run_until(&mut w, 700, |w| {
        stage(w, 1) == Some(HarvestStage::NoRefinery)
    });
    assert_eq!(stage(&w, 1), Some(HarvestStage::NoRefinery));
    assert!(w.construction_of(refinery).is_some(), "built too soon");
    assert!(w.dock_of(refinery).is_none());
    assert_eq!(w.ore_delivered(P0), 0);
    // Complete: a dock, and the waiting harvester delivers.
    run_until(&mut w, 1000, |w| w.stands(refinery));
    assert!(w.stands(refinery));
    w.step();
    assert!(w.dock_of(refinery).is_some());
    run_until(&mut w, 1500, |w| {
        w.ore_delivered(P0) >= u64::from(RULES.capacity)
    });
    assert_eq!(w.ore_delivered(P0), RULES.capacity.into());
}

// ---- storage (WP7.3) ----------------------------------------------------------------------

/// `one_harvester` with storage: each refinery stores `cap`.
fn stored(cap: u32) -> World {
    let mut w = world().with_storage(&[(REFINERY, cap)]).unwrap();
    place_refinery(&mut w, 0, (6, 20), P0);
    spawn_harvester(&mut w, 0, (14, 22), P0);
    w
}

#[test]
fn storage_is_given_once_after_the_harvest_rules_and_before_the_start() {
    let table = [(REFINERY, 100)];
    // Without harvest rules.
    let plain = World::new(WorldConfig::new(7))
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_buildings(buildings())
        .unwrap();
    assert_eq!(
        plain.with_storage(&table).unwrap_err(),
        ConfigError::Storage
    );
    // Twice.
    let once = world().with_storage(&table).unwrap();
    assert!(once.has_storage());
    assert_eq!(once.with_storage(&table).unwrap_err(), ConfigError::Storage);
    // After the start.
    let mut started = world();
    started.step();
    assert_eq!(
        started.with_storage(&table).unwrap_err(),
        ConfigError::Storage
    );
    // A kind that is not a structure of the building table, a kind twice, a zero.
    for bad in [
        vec![(HARVESTER, 100)],
        vec![(REFINERY, 100), (REFINERY, 50)],
        vec![(REFINERY, 0)],
    ] {
        assert_eq!(
            world().with_storage(&bad).unwrap_err(),
            ConfigError::Storage,
            "{bad:?}"
        );
    }
    // An empty table is storage with no capacity anywhere.
    let none = world().with_storage(&[]).unwrap();
    assert!(none.has_storage());
    assert!(!world().has_storage());
}

#[test]
fn a_full_store_takes_nothing_and_the_harvester_waits_in_the_bay_with_its_load() {
    // 50 of room, a load of 100 in bites of 20: 20, 20, then a bite cut to the 10 that fit.
    let mut w = stored(50);
    run_until(&mut w, 2000, |w| w.ore_stored(P0) == 50);
    assert_eq!(w.storage_capacity(P0), 50);
    let mut seen = Vec::new();
    for _ in 0..200 {
        w.step();
        seen.push((stage(&w, 1), load(&w, 1), w.ore_stored(P0)));
    }
    // It stays in the bay with the 50 that did not fit, and nothing more is taken.
    assert!(
        seen.iter()
            .all(|&s| s == (Some(HarvestStage::Unloading), 50, 50)),
        "{:?}",
        &seen[..3]
    );
    assert_eq!(w.ore_delivered(P0), 50);
    // The dock still holds it: the bay is not given to anyone else.
    let dock = w.dock_of(id(0)).unwrap();
    assert_eq!((dock.holder, dock.unloading), (Some(id(1)), true));
}

#[test]
fn the_last_bite_is_cut_to_the_room_left() {
    let mut w = stored(50);
    let mut stores = Vec::new();
    run_until(&mut w, 2000, |w| {
        let s = w.ore_stored(P0);
        if stores.last() != Some(&s) {
            stores.push(s);
        }
        s == 50
    });
    assert_eq!(stores, vec![0, 20, 40, 50]);
}

#[test]
fn a_second_refinery_raises_the_capacity_and_the_waiting_harvester_unloads() {
    let mut w = stored(50);
    run_until(&mut w, 2000, |w| w.ore_stored(P0) == 50);
    run(&mut w, 20);
    let now = w.tick().0;
    place_refinery(&mut w, now, (6, 30), P0);
    w.step();
    assert_eq!(w.storage_capacity(P0), 100);
    // The one in the bay unloads its 50 into the new room and goes back to work.
    run_until(&mut w, 100, |w| w.ore_stored(P0) == 100);
    run_until(&mut w, 10, |w| stage(w, 1) != Some(HarvestStage::Unloading));
    assert_eq!(load(&w, 1), 0);
}

#[test]
fn a_lost_refinery_lowers_the_capacity_and_the_store_keeps_its_ore() {
    let mut w = world().with_storage(&[(REFINERY, 60)]).unwrap();
    place_refinery(&mut w, 0, (6, 20), P0);
    place_refinery(&mut w, 0, (6, 30), P0);
    spawn_harvester(&mut w, 0, (14, 22), P0);
    run_until(&mut w, 4000, |w| w.ore_stored(P0) == 120);
    assert_eq!(w.storage_capacity(P0), 120);
    let now = w.tick().0;
    at(&mut w, now, Command::Despawn { id: id(1) });
    w.step();
    // 60 of capacity, 120 kept: nothing is lost and nothing more goes in.
    assert_eq!((w.storage_capacity(P0), w.ore_stored(P0)), (60, 120));
    run(&mut w, 1500);
    assert_eq!(w.ore_stored(P0), 120);
    assert_eq!(w.ore_delivered(P0), 120);
}

#[test]
fn storage_counts_only_the_standing_buildings_of_the_owner() {
    let mut w = world().with_storage(&[(REFINERY, 70)]).unwrap();
    place_refinery(&mut w, 0, (6, 20), P0);
    place_refinery(&mut w, 0, (6, 30), P1);
    w.step();
    assert_eq!(w.storage_capacity(P0), 70);
    assert_eq!(w.storage_capacity(P1), 70);
    assert_eq!(w.storage_capacity(PlayerId(2)), 0);
    // A world without storage rules reports none and stores nothing, with no limit.
    let mut free = one_harvester();
    run(&mut free, 1500);
    assert_eq!(free.storage_capacity(P0), 0);
    assert_eq!(free.ore_stored(P0), 0);
    assert!(free.ore_delivered(P0) >= 100);
}

#[test]
fn a_refinery_under_construction_stores_nothing_until_it_is_complete() {
    let mut w = yard_world().with_storage(&[(REFINERY, 90)]).unwrap();
    at(
        &mut w,
        0,
        Command::PlaceStructure {
            kind: YARD,
            cell: Cell::new(3, 20),
            owner: P0,
        },
    );
    at(
        &mut w,
        1,
        Command::BuildStructure {
            kind: REFINERY,
            cell: Cell::new(6, 20),
        },
    );
    run(&mut w, 3);
    let refinery = id(1);
    assert!(w.construction_of(refinery).is_some());
    assert_eq!(w.storage_capacity(P0), 0);
    run_until(&mut w, 1000, |w| w.stands(refinery));
    w.step();
    assert_eq!(w.storage_capacity(P0), 90);
}
