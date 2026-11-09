//! The outcome of a match (WP7.6, changed by WP7.7, `docs/rules/match.md`), through the public API: who
//! is defeated (a player with no standing construction centre), who wins, when, and that nothing changes
//! once it is decided.
//!
//! The numbers are small and invented so a test can follow them tick by tick. The shipped data runs in
//! `tools/sf_headless/tests/match_m1.rs`.

use sf_math::{Fx, Vec2};
use sf_sim::{
    BuildingFunction, BuildingStats, BuildingTable, Cell, Command, ConfigError, DestructionConfig,
    EntityId, Footprint, Kind, Loadout, MatchError, MatchResult, MatchRules, Mount, Order,
    PlayerId, ProjectileKind, RepairRules, Tick, UnitStats, UnitTable, Verdict, WeaponStats,
    WeaponTable, World, WorldConfig, per_tick,
};

const MAP: u32 = 40;

const RIFLEMAN: Kind = Kind(1); // armed mobile unit, 50 hit points
const HARVESTER: Kind = Kind(2); // unarmed mobile unit, 80 hit points
const BARN: Kind = Kind(3); // structure, 1 by 1, 40 hit points, no weapon, no function
const CRATE: Kind = Kind(4); // an object, zero hit points
const GUNNER: Kind = Kind(5); // armed mobile unit, 30 hit points, a rifle of 30: two hits
const HQ: Kind = Kind(6); // the construction centre: 1 by 1, 40 hit points, cannot be built
const OUTPOST: Kind = Kind(7); // a construction centre that can be built: 2 by 2, 20 ticks, 40 hit points

const RIFLE: u16 = 1;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);
const P3: PlayerId = PlayerId(3);

fn mobile(kind: Kind, hp: u32, range: i32) -> UnitStats {
    UnitStats {
        kind,
        hp,
        movement_class: Some(0),
        speed_per_tick: per_tick(Fx::from_int(2)),
        turn_per_tick: 255,
        size: Fx::from_ratio(4, 10),
        footprint: None,
        weapon_range: Fx::from_int(range),
        turret_per_tick: 0,
    }
}

fn structure(kind: Kind, side: u8) -> UnitStats {
    UnitStats {
        kind,
        hp: 40,
        movement_class: None,
        speed_per_tick: Fx::ZERO,
        turn_per_tick: 0,
        size: Fx::ZERO,
        footprint: Some(Footprint {
            width: side,
            height: side,
        }),
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    }
}

fn is_structure(kind: Kind) -> bool {
    [BARN, HQ, OUTPOST].contains(&kind)
}

fn units() -> UnitTable {
    UnitTable::new(vec![
        mobile(RIFLEMAN, 50, 6),
        mobile(HARVESTER, 80, 0),
        structure(BARN, 1),
        UnitStats {
            kind: CRATE,
            hp: 0,
            movement_class: None,
            speed_per_tick: Fx::ZERO,
            turn_per_tick: 0,
            size: Fx::from_ratio(4, 10),
            footprint: None,
            weapon_range: Fx::ZERO,
            turret_per_tick: 0,
        },
        mobile(GUNNER, 30, 6),
        structure(HQ, 1),
        structure(OUTPOST, 2),
    ])
    .unwrap()
}

fn building(kind: Kind, side: u8, function: BuildingFunction, ticks: u32) -> BuildingStats {
    BuildingStats {
        kind,
        footprint: Footprint {
            width: side,
            height: side,
        },
        function,
        exit_side: None,
        build_ticks: ticks,
        cost: (ticks > 0).then_some(10),
        power: 0,
        factions: 1,
        requires: Vec::new(),
        build_radius: if function == BuildingFunction::Anchor {
            6
        } else {
            0
        },
    }
}

fn buildings() -> BuildingTable {
    BuildingTable::new(vec![
        building(BARN, 1, BuildingFunction::None, 0),
        building(HQ, 1, BuildingFunction::Anchor, 0),
        building(OUTPOST, 2, BuildingFunction::Anchor, 20),
    ])
    .unwrap()
}

const SELL_TICKS: u32 = 10;

fn rifle() -> WeaponStats {
    WeaponStats {
        kind: RIFLE,
        range: Fx::from_int(6),
        min_range: Fx::ZERO,
        reload_ticks: 10,
        burst: 1,
        burst_delay_ticks: 0,
        damage: 20,
        damage_type: 0,
        projectile: ProjectileKind::Instant,
        projectile_speed_per_tick: Fx::ZERO,
        flight_height: Fx::ZERO,
        guide_turn_per_tick: 0,
        splash_radius: Fx::ZERO,
        splash_inner: Fx::ZERO,
        friendly_fire: true,
        mount: Mount::Hull,
        fire_arc: 128,
    }
}

fn weapons() -> WeaponTable {
    let l = |unit: Kind| Loadout {
        unit,
        weapons: vec![RIFLE],
    };
    WeaponTable::new(vec![rifle()], vec![l(RIFLEMAN), l(GUNNER)]).unwrap()
}

/// A world with units, weapons and destruction, without match rules and without a building table.
fn plain() -> World {
    World::new(WorldConfig::new(7).with_map_size(MAP, MAP))
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_weapons(weapons())
        .unwrap()
        .with_destruction(DestructionConfig::DEFAULT)
        .unwrap()
}

fn rules(players: &[PlayerId]) -> MatchRules {
    MatchRules::new(players).unwrap()
}

/// The same with the building table (a construction centre among it), repair and sale, and match rules
/// for `players`; every player is of faction 1.
fn world(players: &[PlayerId]) -> World {
    let factions: Vec<(PlayerId, u8)> = (0..4).map(|p| (PlayerId(p), 1)).collect();
    plain()
        .with_buildings(buildings())
        .unwrap()
        .with_player_factions(&factions)
        .unwrap()
        .with_repair(RepairRules {
            hp_per_tick: 1,
            full_cost_permille: 0,
            sell_ticks: SELL_TICKS,
            refund_permille: 500,
        })
        .unwrap()
        .with_match(rules(players))
        .unwrap()
}

fn at(x: i32, y: i32) -> Vec2 {
    Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF)
}

/// A spawn, a structure placement or a despawn, scheduled for `tick`.
fn put(w: &mut World, tick: u64, kind: Kind, owner: PlayerId, x: i32, y: i32) {
    let command = if is_structure(kind) {
        Command::PlaceStructure {
            kind,
            cell: Cell::new(x as u32, y as u32),
            owner,
        }
    } else {
        Command::SpawnUnit {
            kind,
            pos: at(x, y),
            owner,
        }
    };
    w.schedule(Tick(tick), PlayerId(0), command).unwrap();
}

/// The id of the one entity of `kind` and `owner` (there is exactly one in these tests).
fn find(w: &World, kind: Kind, owner: PlayerId) -> EntityId {
    let mut found = w
        .snapshot()
        .iter()
        .filter(|v| v.kind == kind && v.owner == owner);
    let id = found.next().expect("the entity exists").id;
    assert!(found.next().is_none(), "exactly one");
    id
}

/// The id of the entity of `kind` and `owner`, if there is one.
fn find_opt(w: &World, kind: Kind, owner: PlayerId) -> Option<EntityId> {
    w.snapshot()
        .iter()
        .find(|v| v.kind == kind && v.owner == owner)
        .map(|v| v.id)
}

fn remove(w: &mut World, id: EntityId) {
    w.schedule(w.tick(), PlayerId(0), Command::Despawn { id })
        .unwrap();
}

/// Two players, each with a construction centre, a barn and a rifleman; the first tick has run and
/// the match has begun.
fn two_sides() -> World {
    let mut w = world(&[P0, P1]);
    for (owner, x) in [(P0, 2), (P1, 30)] {
        put(&mut w, 0, HQ, owner, x, 5);
        put(&mut w, 0, BARN, owner, x, 7);
        put(&mut w, 0, RIFLEMAN, owner, x, 10);
    }
    w.step();
    assert!(w.match_begun());
    w
}

#[test]
fn rules_need_two_to_eight_players_each_once() {
    assert_eq!(MatchRules::new(&[P0]), Err(MatchError::PlayerCount));
    assert_eq!(MatchRules::new(&[]), Err(MatchError::PlayerCount));
    let nine: Vec<PlayerId> = (0..9).map(PlayerId).collect();
    assert_eq!(MatchRules::new(&nine), Err(MatchError::PlayerCount));
    assert_eq!(MatchRules::new(&nine[..8]).unwrap().players().len(), 8);
    assert_eq!(
        MatchRules::new(&[P1, P0, P1]),
        Err(MatchError::Duplicate(P1))
    );
    // Kept ascending whatever the order given.
    assert_eq!(
        MatchRules::new(&[P3, P0, P2]).unwrap().players(),
        &[P0, P2, P3]
    );
}

#[test]
fn a_world_takes_match_rules_once_and_last() {
    let r = || rules(&[P0, P1]);
    // Without the units, the weapons or the destruction there is no match to decide.
    let bare = World::new(WorldConfig::new(7).with_map_size(MAP, MAP)).unwrap();
    assert_eq!(bare.with_match(r()).err(), Some(ConfigError::Match));
    let no_weapons = World::new(WorldConfig::new(7).with_map_size(MAP, MAP))
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_buildings(buildings())
        .unwrap();
    assert_eq!(no_weapons.with_match(r()).err(), Some(ConfigError::Match));
    let no_destruction = World::new(WorldConfig::new(7).with_map_size(MAP, MAP))
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_buildings(buildings())
        .unwrap()
        .with_weapons(weapons())
        .unwrap();
    assert_eq!(
        no_destruction.with_match(r()).err(),
        Some(ConfigError::Match)
    );
    // Twice.
    assert_eq!(
        world(&[P0, P1]).with_match(r()).err(),
        Some(ConfigError::Match)
    );
    // After the start.
    let mut started = plain();
    started.step();
    assert_eq!(started.with_match(r()).err(), Some(ConfigError::Match));
    // Last: no unit table, building table, weapon table or destruction settings after the match rules.
    assert_eq!(
        world(&[P0, P1]).with_units(units()).err(),
        Some(ConfigError::Match)
    );
    assert_eq!(
        world(&[P0, P1]).with_buildings(buildings()).err(),
        Some(ConfigError::Match)
    );
    assert_eq!(
        world(&[P0, P1]).with_weapons(weapons()).err(),
        Some(ConfigError::Match)
    );
    assert_eq!(
        world(&[P0, P1])
            .with_destruction(DestructionConfig::DEFAULT)
            .err(),
        Some(ConfigError::Match)
    );
    // And the accepted one reads back.
    assert_eq!(world(&[P1, P0]).match_players(), &[P0, P1]);
}

#[test]
fn match_rules_need_a_construction_centre_kind_in_the_building_table() {
    // WP7.7: the defeat rule asks for a standing construction centre, so a world whose building table
    // has no kind of that function (or no building table at all) could never keep a player in.
    let r = rules(&[P0, P1]);
    let without_table = plain();
    assert_eq!(
        without_table.with_match(r.clone()).err(),
        Some(ConfigError::Match)
    );
    let only_barns =
        BuildingTable::new(vec![building(BARN, 1, BuildingFunction::None, 0)]).unwrap();
    let no_anchor = plain().with_buildings(only_barns).unwrap();
    assert_eq!(
        no_anchor.with_match(r.clone()).err(),
        Some(ConfigError::Match)
    );
    let with_anchor = plain().with_buildings(buildings()).unwrap();
    assert!(with_anchor.with_match(r).is_ok());
}

#[test]
fn a_world_without_match_rules_has_no_outcome_and_no_section() {
    let build = |players: Option<&[PlayerId]>| {
        let mut w = match players {
            Some(p) => world(p),
            // The world of WP7.6, with its five unit kinds: the literal below was measured on it
            // before this WP, so it also shows that the new code hashes such a world the same.
            None => World::new(WorldConfig::new(7).with_map_size(MAP, MAP))
                .unwrap()
                .with_units(UnitTable::new(units().iter().take(5).cloned().collect()).unwrap())
                .unwrap()
                .with_weapons(weapons())
                .unwrap()
                .with_destruction(DestructionConfig::DEFAULT)
                .unwrap(),
        };
        for (owner, x) in [(P0, 2), (P1, 30)] {
            put(&mut w, 0, BARN, owner, x, 5);
            put(&mut w, 0, RIFLEMAN, owner, x, 8);
        }
        for _ in 0..5 {
            w.step();
        }
        w
    };
    let (without, with) = (build(None), build(Some(&[P0, P1])));
    assert_eq!(without.match_result(), None);
    assert_eq!(without.match_players(), &[] as &[PlayerId]);
    assert!(!without.match_begun());
    assert!(without.defeats().is_empty());
    assert_eq!(without.defeat_tick(P0), None);
    // The rules are state: the same world with them hashes differently, and the one without them
    // has no section `Z` at all (the literal is what this world hashes to; the pinned scenarios and
    // `golden.rs` show that a world without match rules hashes as it did before WP7.6).
    assert_ne!(without.checksum(), with.checksum());
    assert_eq!(
        without.checksum(),
        0xba76_70f3_58cc_6d46,
        "a world without match rules hashes as it did before WP7.6"
    );
}

#[test]
fn only_a_standing_construction_centre_keeps_a_player_in_the_match() {
    let mut w = two_sides();
    // P1 loses its barn and its rifleman: it still has its construction centre, so it is still in.
    let (barn, man) = (find(&w, BARN, P1), find(&w, RIFLEMAN, P1));
    remove(&mut w, barn);
    remove(&mut w, man);
    w.step();
    assert_eq!((w.match_result(), w.defeat_tick(P1)), (None, None));
    // It gets a barn and two armed units, and loses the construction centre: they do not keep it in.
    let now = w.tick().0;
    put(&mut w, now, BARN, P1, 33, 7);
    put(&mut w, now, GUNNER, P1, 31, 8);
    put(&mut w, now, RIFLEMAN, P1, 31, 9);
    w.step();
    assert_eq!((w.match_result(), w.defeat_tick(P1)), (None, None));
    let hq = find(&w, HQ, P1);
    remove(&mut w, hq);
    let tick = w.tick();
    w.step();
    assert_eq!(w.defeat_tick(P1), Some(tick));
    assert_eq!(
        w.match_result(),
        Some(MatchResult {
            tick,
            verdict: Verdict::Won(P0)
        })
    );
    // The things that did not count are still there.
    let count = |kind: Kind| {
        w.snapshot()
            .iter()
            .filter(|v| v.kind == kind && v.owner == P1)
            .count()
    };
    assert_eq!((count(BARN), count(GUNNER), count(RIFLEMAN)), (1, 1, 1));
}

#[test]
fn an_unarmed_unit_and_an_object_do_not_keep_a_player_in() {
    let mut w = two_sides();
    // P1 owns a harvester and a crate besides.
    put(&mut w, 1, HARVESTER, P1, 31, 11);
    put(&mut w, 1, CRATE, P1, 32, 11);
    w.step();
    assert_eq!(w.match_result(), None);
    let hq = find(&w, HQ, P1);
    remove(&mut w, hq);
    let tick = w.tick();
    w.step();
    assert_eq!(w.defeat_tick(P1), Some(tick));
    assert_eq!(
        w.match_result(),
        Some(MatchResult {
            tick,
            verdict: Verdict::Won(P0)
        })
    );
    // The harvester is still there: it is no part of the rule.
    assert_eq!(
        w.snapshot().iter().filter(|v| v.kind == HARVESTER).count(),
        1
    );
}

#[test]
fn the_last_player_standing_wins_in_the_tick_of_the_last_defeat() {
    let mut w = two_sides();
    for _ in 0..4 {
        w.step();
    }
    assert_eq!(w.match_result(), None);
    let hq = find(&w, HQ, P0);
    remove(&mut w, hq);
    let tick = w.tick();
    assert_eq!(tick, Tick(5));
    w.step();
    assert_eq!(w.defeats(), &[(P0, Tick(5))]);
    assert_eq!(
        w.match_result(),
        Some(MatchResult {
            tick: Tick(5),
            verdict: Verdict::Won(P1)
        })
    );
}

#[test]
fn the_last_two_falling_together_is_a_draw() {
    let mut w = two_sides();
    for owner in [P0, P1] {
        let id = find(&w, HQ, owner);
        remove(&mut w, id);
    }
    w.step();
    assert_eq!(w.defeats(), &[(P0, Tick(1)), (P1, Tick(1))]);
    assert_eq!(
        w.match_result(),
        Some(MatchResult {
            tick: Tick(1),
            verdict: Verdict::Draw
        })
    );
}

#[test]
fn three_players_need_two_defeats_and_each_is_recorded_in_its_tick() {
    let mut w = world(&[P0, P1, P3]);
    for (owner, x) in [(P0, 2), (P1, 20), (P3, 35)] {
        put(&mut w, 0, HQ, owner, x, 5);
    }
    // Player 2 is not in the match: whatever it owns or loses is nobody's business.
    put(&mut w, 0, HQ, P2, 10, 20);
    w.step();
    let hq = |w: &World, owner| find(w, HQ, owner);
    let id = hq(&w, P2);
    remove(&mut w, id);
    w.step();
    assert_eq!((w.match_result(), w.defeats().len()), (None, 0));
    for _ in 0..2 {
        w.step();
    }
    let id = hq(&w, P1);
    remove(&mut w, id);
    w.step();
    assert_eq!(w.defeats(), &[(P1, Tick(4))]);
    assert_eq!(w.match_result(), None, "two are still in");
    for _ in 0..3 {
        w.step();
    }
    let id = hq(&w, P3);
    remove(&mut w, id);
    w.step();
    assert_eq!(w.defeats(), &[(P1, Tick(4)), (P3, Tick(8))]);
    assert_eq!(
        w.match_result(),
        Some(MatchResult {
            tick: Tick(8),
            verdict: Verdict::Won(P0)
        })
    );
    assert_eq!(w.defeat_tick(P0), None);
    assert_eq!(w.defeat_tick(P2), None);
}

#[test]
fn nobody_is_defeated_before_every_player_has_a_construction_centre() {
    let mut w = world(&[P0, P1]);
    put(&mut w, 0, HQ, P0, 2, 5);
    // P1 has a barn and an army from the start, but its construction centre comes at tick 5.
    put(&mut w, 0, BARN, P1, 30, 7);
    put(&mut w, 0, RIFLEMAN, P1, 30, 9);
    put(&mut w, 5, HQ, P1, 30, 5);
    for _ in 0..5 {
        w.step();
    }
    assert!(!w.match_begun());
    assert_eq!((w.match_result(), w.defeats().len()), (None, 0));
    w.step();
    assert!(w.match_begun());
    // And then P1 can lose.
    let id = find(&w, HQ, P1);
    remove(&mut w, id);
    w.step();
    assert_eq!(w.defeat_tick(P1), Some(Tick(6)));
    assert!(matches!(
        w.match_result(),
        Some(MatchResult {
            verdict: Verdict::Won(p),
            ..
        }) if p == P0
    ));
}

#[test]
fn a_world_in_which_a_player_never_owns_a_construction_centre_has_no_result() {
    let mut w = world(&[P0, P1]);
    put(&mut w, 0, HQ, P0, 2, 5);
    put(&mut w, 0, BARN, P1, 30, 5);
    put(&mut w, 0, RIFLEMAN, P1, 30, 8);
    for _ in 0..30 {
        w.step();
    }
    assert_eq!((w.match_begun(), w.match_result()), (false, None));
}

#[test]
fn once_decided_the_result_never_changes() {
    let mut w = two_sides();
    let hq = find(&w, HQ, P1);
    remove(&mut w, hq);
    w.step();
    let result = w.match_result().unwrap();
    let defeats = w.defeats().to_vec();
    assert_eq!(result.verdict, Verdict::Won(P0));
    // The loser comes back (a new construction centre) and the winner loses everything: nothing is
    // decided again.
    let now = w.tick().0;
    put(&mut w, now, HQ, P1, 33, 5);
    w.step();
    let hq = find(&w, HQ, P0);
    remove(&mut w, hq);
    for _ in 0..40 {
        w.step();
    }
    assert_eq!(w.match_result(), Some(result));
    assert_eq!(w.defeats(), &defeats[..]);
    assert_eq!(w.defeat_tick(P0), None);
}

#[test]
fn a_construction_centre_shot_down_defeats_its_owner_in_the_tick_it_died() {
    let mut w = world(&[P0, P1]);
    put(&mut w, 0, HQ, P0, 2, 5);
    put(&mut w, 0, HQ, P1, 12, 5);
    put(&mut w, 0, GUNNER, P0, 8, 5);
    w.step();
    let (gunner, hq) = (find(&w, GUNNER, P0), find(&w, HQ, P1));
    w.schedule(
        w.tick(),
        P0,
        Command::Order {
            ids: vec![gunner],
            order: Order::Attack(hq),
            queued: false,
        },
    )
    .unwrap();
    let mut died = None;
    for _ in 0..400 {
        let report = w.step();
        if report.deaths > 0 {
            died = Some(report.tick);
            break;
        }
    }
    let died = died.expect("two hits of 20 kill a construction centre of 40");
    assert_eq!(w.defeat_tick(P1), Some(died));
    assert_eq!(
        w.match_result(),
        Some(MatchResult {
            tick: died,
            verdict: Verdict::Won(P0)
        })
    );
}

#[test]
fn selling_the_construction_centre_defeats_the_player_at_once() {
    let mut w = two_sides();
    for _ in 0..3 {
        w.step();
    }
    let hq = find(&w, HQ, P1);
    let sold_at = w.tick();
    w.schedule(sold_at, P1, Command::Sell { structure: hq })
        .unwrap();
    let report = w.step();
    assert_eq!(report.commands_executed, 1, "the sale was accepted");
    // The structure is still in the world (the sale takes SELL_TICKS), the player is already out.
    assert!(find_opt(&w, HQ, P1).is_some());
    assert!(!w.stands(hq));
    assert_eq!(w.defeat_tick(P1), Some(sold_at));
    assert_eq!(
        w.match_result(),
        Some(MatchResult {
            tick: sold_at,
            verdict: Verdict::Won(P0)
        })
    );
    // And nothing changes when the sale ends.
    for _ in 0..=SELL_TICKS {
        w.step();
    }
    assert!(find_opt(&w, HQ, P1).is_none());
    assert_eq!(w.defeats(), &[(P1, sold_at)]);
}

#[test]
fn selling_another_structure_does_not_defeat_the_player() {
    let mut w = two_sides();
    let barn = find(&w, BARN, P1);
    w.schedule(w.tick(), P1, Command::Sell { structure: barn })
        .unwrap();
    for _ in 0..=SELL_TICKS + 1 {
        w.step();
    }
    assert!(find_opt(&w, BARN, P1).is_none(), "the barn was sold");
    assert_eq!((w.match_result(), w.defeats().len()), (None, 0));
}

#[test]
fn a_construction_centre_under_construction_does_not_count_until_it_is_complete() {
    let mut w = two_sides();
    // P1 builds a second construction centre next to its first (build radius 6).
    let started = w.tick();
    w.schedule(
        started,
        P1,
        Command::BuildStructure {
            kind: OUTPOST,
            cell: Cell::new(32, 5),
        },
    )
    .unwrap();
    w.step();
    assert_eq!(w.under_construction(), 1);
    // The first one falls while the second is not complete: P1 is out.
    let hq = find(&w, HQ, P1);
    remove(&mut w, hq);
    let tick = w.tick();
    w.step();
    assert_eq!(
        w.under_construction(),
        1,
        "the outpost is still not complete"
    );
    assert_eq!(w.defeat_tick(P1), Some(tick));
    assert!(matches!(
        w.match_result(),
        Some(MatchResult {
            verdict: Verdict::Won(p),
            ..
        }) if p == P0
    ));

    // The same again, but the second one is given the time to complete: now it keeps P1 in.
    let mut w = two_sides();
    w.schedule(
        w.tick(),
        P1,
        Command::BuildStructure {
            kind: OUTPOST,
            cell: Cell::new(32, 5),
        },
    )
    .unwrap();
    for _ in 0..25 {
        w.step();
    }
    assert_eq!(w.under_construction(), 0);
    let hq = find(&w, HQ, P1);
    remove(&mut w, hq);
    w.step();
    assert_eq!((w.match_result(), w.defeats().len()), (None, 0));
    // And when that one falls too, P1 is out.
    let outpost = find(&w, OUTPOST, P1);
    remove(&mut w, outpost);
    let tick = w.tick();
    w.step();
    assert_eq!(w.defeat_tick(P1), Some(tick));
}

#[test]
fn the_result_does_not_depend_on_the_order_things_were_made_in() {
    let run = |order: &[(Kind, PlayerId, i32)]| {
        let mut w = world(&[P0, P1]);
        for &(kind, owner, x) in order {
            let y = match kind {
                HQ => 5,
                BARN => 7,
                _ => 9,
            };
            put(&mut w, 0, kind, owner, x, y);
        }
        w.step();
        let id = find(&w, HQ, P1);
        remove(&mut w, id);
        w.step();
        (w.match_result(), w.defeats().to_vec())
    };
    let forward = [
        (HQ, P0, 2),
        (RIFLEMAN, P0, 3),
        (HQ, P1, 30),
        (BARN, P1, 31),
        (RIFLEMAN, P1, 31),
        (HARVESTER, P1, 32),
    ];
    let mut backward = forward;
    backward.reverse();
    assert_eq!(run(&forward), run(&backward));
    assert!(run(&forward).0.is_some());
}

#[test]
fn the_defeats_and_the_result_are_in_the_checksum() {
    let (mut a, mut b) = (two_sides(), two_sides());
    assert_eq!(a.checksum(), b.checksum());
    // The same removal, a tick apart: the defeat tick differs, and so does the hash afterwards.
    let hq = find(&a, HQ, P1);
    remove(&mut a, hq);
    b.step();
    let hq = find(&b, HQ, P1);
    remove(&mut b, hq);
    a.step();
    a.step();
    b.step();
    assert_eq!(a.tick(), b.tick());
    assert_eq!(a.match_result().unwrap().tick, Tick(1));
    assert_eq!(b.match_result().unwrap().tick, Tick(2));
    assert_ne!(a.checksum(), b.checksum());
}

/// The worst case of the check (WP7.6), a measurement and not an assertion on time: 20,000 unarmed units
/// of player 0 and 20,000 of player 1 fill the low slots, and the one thing each player owns that
/// counts (a construction centre) has the highest slots, so the scan of a tick that removed something has to walk
/// every slot before it finds them; one unarmed unit is removed in every tick, so every tick scans.
/// Compared with the same world without match rules, alternating. Run with `cargo test --release -p
/// sf_sim --test outcome -- --ignored --nocapture worst_case_cost`.
#[test]
#[ignore]
#[allow(clippy::cast_precision_loss)]
fn worst_case_cost() {
    const N: u32 = 20_000;
    let build = |with_rules: bool| {
        let base = World::new(WorldConfig::new(7).with_map_size(MAP, MAP))
            .unwrap()
            .with_units(units())
            .unwrap()
            .with_buildings(buildings())
            .unwrap()
            .with_weapons(weapons())
            .unwrap()
            .with_destruction(DestructionConfig::DEFAULT)
            .unwrap();
        let mut w = if with_rules {
            base.with_match(rules(&[P0, P1])).unwrap()
        } else {
            base
        };
        for i in 0..N {
            for owner in [P0, P1] {
                let (x, y) = ((i % 38) as i32, (i / 38 % 38) as i32);
                put(&mut w, 0, HARVESTER, owner, x, y);
            }
        }
        put(&mut w, 0, HQ, P0, 2, 5);
        put(&mut w, 0, HQ, P1, 30, 5);
        w.step();
        w
    };
    let run = |with_rules: bool| {
        let mut w = build(with_rules);
        let ids: Vec<EntityId> = w
            .snapshot()
            .iter()
            .filter(|v| v.kind == HARVESTER)
            .map(|v| v.id)
            .collect();
        let t = std::time::Instant::now();
        let ticks = 2000;
        for id in ids.into_iter().take(ticks) {
            w.schedule(w.tick(), PlayerId(0), Command::Despawn { id })
                .unwrap();
            w.step();
        }
        (
            t.elapsed().as_secs_f64() * 1e6 / ticks as f64,
            w.match_result(),
        )
    };
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for _ in 0..5 {
        a.push(run(true).0);
        b.push(run(false).0);
    }
    let median = |v: &mut Vec<f64>| {
        v.sort_by(|x, y| x.partial_cmp(y).unwrap());
        v[v.len() / 2]
    };
    println!("with the match rules:    {a:.1?}");
    println!("without the match rules: {b:.1?}");
    println!(
        "median us per tick (40,000 units, one removed every tick, full scan every tick): with {:.1}, without {:.1}",
        median(&mut a),
        median(&mut b)
    );
}
