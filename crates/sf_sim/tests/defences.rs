//! Defensive buildings (WP6.7, `docs/rules/defences.md`) through the public API: a standing structure
//! with weapons chooses its own targets, turns its turret to them and fires only within its arc; it
//! takes no order; a defence that uses power is silent in a shortage and one that does not use any is
//! not; a structure under construction or at zero hit points does nothing; a weaponless bunker does
//! nothing; and the world replays the same whatever order things were put in.
//!
//! Hand-made tables with distinct numbers (turn rates 12, 5 and 9, powers -20, -40, 0 and -10), apart
//! from the shipped data (`assets/config`, which `tools/sf_headless/tests/defences.rs` runs).
//!
//! Map 64 x 64. Player 0 (faction bit 1) owns the defences and has an anchor on (4, 4); player 1 owns
//! the targets (dummies that cannot shoot back, 1000 hit points).

use sf_math::{Facing, Fx, Vec2};
use sf_sim::{
    ArmorTable, BuildingFunction, BuildingStats, BuildingTable, Cell, Command, EntityId, Footprint,
    Kind, Loadout, Mount, Order, PlayerId, PowerRules, ProjectileKind, Shot, TargetingConfig,
    UnitStats, UnitTable, WeaponStats, WeaponTable, World, WorldConfig, per_tick,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const ANCHOR: Kind = Kind(20);
const PLANT: Kind = Kind(21); // +100
const TOWER: Kind = Kind(22); // 1 x 1, turret 12, -20
const EMPL: Kind = Kind(23); // 2 x 2, turret 5, -40
const BUNKER: Kind = Kind(24); // 2 x 2, no weapon, power 0
const STATION: Kind = Kind(25); // 1 x 1, turret 9, an armed structure that uses no power
const HUT: Kind = Kind(26); // 1 x 1, hull gun (no turret), -10
const DUMMY: Kind = Kind(30); // a unit that cannot shoot, 1000 hit points

const TOWER_GUN: u16 = 1;
const EMPL_GUN: u16 = 2;
const STATION_GUN: u16 = 3;
const HUT_GUN: u16 = 4;

const BULLET: u16 = 0;
const ROCKET: u16 = 1;
const NONE: u16 = 0;
const SOFT: u16 = 1;
const STRUCT: u16 = 4;

fn at(x: i32, y: i32) -> Vec2 {
    Vec2::from_ints(x, y)
}

fn structure(kind: Kind, w: u8, h: u8, hp: u32, turret: u8) -> UnitStats {
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
        turret_per_tick: turret,
    }
}

fn units() -> UnitTable {
    let armed = |s: UnitStats, range: i32| UnitStats {
        weapon_range: Fx::from_int(range),
        ..s
    };
    UnitTable::new(vec![
        structure(ANCHOR, 2, 2, 500, 0),
        structure(PLANT, 2, 2, 600, 0),
        armed(structure(TOWER, 1, 1, 600, 12), 6),
        armed(structure(EMPL, 2, 2, 800, 5), 7),
        structure(BUNKER, 2, 2, 1200, 0),
        armed(structure(STATION, 1, 1, 400, 9), 6),
        armed(structure(HUT, 1, 1, 300, 0), 6),
        UnitStats {
            kind: DUMMY,
            hp: 1000,
            movement_class: Some(0),
            speed_per_tick: per_tick(Fx::from_int(2)),
            turn_per_tick: 255,
            size: Fx::ONE,
            footprint: None,
            weapon_range: Fx::ZERO,
            turret_per_tick: 0,
        },
    ])
    .unwrap()
}

fn gun(kind: u16, damage_type: u16, damage: u32, range: i32, reload: u32) -> WeaponStats {
    WeaponStats {
        kind,
        range: Fx::from_int(range),
        min_range: Fx::ZERO,
        reload_ticks: reload,
        burst: 1,
        burst_delay_ticks: 0,
        damage,
        damage_type,
        projectile: ProjectileKind::Instant,
        projectile_speed_per_tick: Fx::ZERO,
        flight_height: Fx::ZERO,
        guide_turn_per_tick: 0,
        splash_radius: Fx::ZERO,
        splash_inner: Fx::ZERO,
        friendly_fire: true,
        mount: Mount::Turret,
        fire_arc: 8,
    }
}

fn weapons() -> WeaponTable {
    let list = vec![
        gun(TOWER_GUN, BULLET, 6, 6, 10),
        gun(EMPL_GUN, ROCKET, 30, 7, 20),
        gun(STATION_GUN, BULLET, 4, 6, 10),
        WeaponStats {
            mount: Mount::Hull,
            fire_arc: 128,
            ..gun(HUT_GUN, BULLET, 5, 6, 10)
        },
    ];
    let l = |unit: Kind, w: u16| Loadout {
        unit,
        weapons: vec![w],
    };
    let loadouts = vec![
        l(TOWER, TOWER_GUN),
        l(EMPL, EMPL_GUN),
        l(STATION, STATION_GUN),
        l(HUT, HUT_GUN),
    ];
    let armor = |kind: Kind| if kind == DUMMY { SOFT } else { STRUCT };
    let cells = vec![
        (BULLET, NONE, 0),
        (BULLET, SOFT, 100),
        (BULLET, STRUCT, 20),
        (ROCKET, NONE, 0),
        (ROCKET, SOFT, 40),
        (ROCKET, STRUCT, 70),
    ];
    let table = ArmorTable::new(
        vec![BULLET, ROCKET],
        vec![NONE, SOFT, STRUCT],
        cells,
        units().iter().map(|u| (u.kind, armor(u.kind))).collect(),
    )
    .unwrap();
    WeaponTable::new(list, loadouts)
        .unwrap()
        .with_armor(table)
        .unwrap()
}

fn entry(
    kind: Kind,
    size: u8,
    function: BuildingFunction,
    ticks: u32,
    power: i32,
) -> BuildingStats {
    BuildingStats {
        kind,
        footprint: Footprint {
            width: size,
            height: size,
        },
        function,
        exit_side: None,
        build_ticks: ticks,
        cost: (ticks > 0).then_some(100),
        power,
        factions: 1,
        requires: vec![],
        build_radius: if function == BuildingFunction::Anchor {
            40
        } else {
            0
        },
    }
}

fn buildings() -> BuildingTable {
    BuildingTable::new(vec![
        entry(ANCHOR, 2, BuildingFunction::Anchor, 0, 0),
        entry(PLANT, 2, BuildingFunction::PowerPlant, 30, 100),
        entry(TOWER, 1, BuildingFunction::Defence, 40, -20),
        entry(EMPL, 2, BuildingFunction::Defence, 40, -40),
        entry(BUNKER, 2, BuildingFunction::Defence, 40, 0),
        entry(STATION, 1, BuildingFunction::Defence, 40, 0),
        entry(HUT, 1, BuildingFunction::Defence, 40, -10),
    ])
    .unwrap()
}

/// A world with defences and targeting; with power rules (floor 250, cutoff 1000) or without.
fn world(power: bool) -> World {
    let w = World::new(WorldConfig::new(9).with_map_size(64, 64))
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_buildings(buildings())
        .unwrap()
        .with_weapons(weapons())
        .unwrap()
        .with_targeting(TargetingConfig::DEFAULT)
        .unwrap()
        .with_player_factions(&[(P0, 1)])
        .unwrap();
    if power {
        w.with_power(PowerRules {
            shortage_floor_permille: 250,
            defence_cutoff_permille: 1000,
        })
        .unwrap()
    } else {
        w
    }
}

fn send(w: &mut World, player: PlayerId, command: Command) {
    let tick = w.tick();
    w.schedule(tick, player, command).unwrap();
}

fn place(w: &mut World, kind: Kind, cell: (u32, u32)) {
    send(
        w,
        P0,
        Command::PlaceStructure {
            kind,
            cell: Cell::new(cell.0, cell.1),
            owner: P0,
        },
    );
}

fn dummy(w: &mut World, pos: Vec2) {
    send(
        w,
        P1,
        Command::SpawnUnit {
            kind: DUMMY,
            pos,
            owner: P1,
        },
    );
}

fn id_at(w: &World, cell: (u32, u32)) -> EntityId {
    w.occupant_at(Cell::new(cell.0, cell.1)).unwrap()
}

/// Runs `n` ticks and returns every shot, with the tick it was fired on.
fn run(w: &mut World, n: u32) -> Vec<Shot> {
    let mut all = Vec::new();
    for _ in 0..n {
        w.step();
        all.extend_from_slice(w.shots());
    }
    all
}

fn shots_by(shots: &[Shot], id: EntityId) -> usize {
    shots.iter().filter(|s| s.attacker == id).count()
}

fn bearing(w: &World, from: EntityId, to: EntityId) -> Facing {
    let a = w.get(from).unwrap().pos;
    let b = w.get(to).unwrap().pos;
    (b - a).facing().unwrap()
}

#[test]
fn a_tower_picks_an_enemy_in_reach_fires_and_takes_no_order() {
    let mut w = world(false);
    place(&mut w, TOWER, (30, 30));
    dummy(&mut w, at(30, 35));
    w.step();
    let tower = id_at(&w, (30, 30));
    let target = EntityId::new(1, 0);
    let shots = run(&mut w, 100);
    assert!(shots_by(&shots, tower) > 0, "the tower never fired");
    assert!(shots.iter().all(|s| s.target == target));
    assert!(w.hp_of(target).unwrap() < 1000);
    // It has no order: it chose a target, which is not an order.
    assert_eq!(w.order_of(tower), Some(Order::Idle));
    assert_eq!(w.turret_target_of(tower), Some(target));
}

#[test]
fn a_target_out_of_reach_is_not_picked_and_one_that_leaves_is_dropped() {
    let mut w = world(false);
    place(&mut w, TOWER, (30, 30));
    // 12 cells away: far out of reach 6.
    dummy(&mut w, at(30, 42));
    w.step();
    let tower = id_at(&w, (30, 30));
    assert_eq!(shots_by(&run(&mut w, 60), tower), 0);
    assert_eq!(w.turret_target_of(tower), None);
    // Walk it in, then out again.
    let dummy_id = EntityId::new(1, 0);
    let go = |w: &mut World, to: Vec2| {
        send(
            w,
            P1,
            Command::Order {
                ids: vec![dummy_id],
                order: Order::Move(to),
                queued: false,
            },
        );
    };
    go(&mut w, at(30, 35));
    run(&mut w, 80);
    assert_eq!(w.turret_target_of(tower), Some(dummy_id));
    go(&mut w, at(30, 50));
    run(&mut w, 120);
    assert_eq!(w.turret_target_of(tower), None);
}

#[test]
fn the_turret_turns_at_its_own_rate_and_fires_only_within_its_arc() {
    // The same target behind each structure's initial facing; the two turn at 12 and at 5.
    let mut w = world(false);
    place(&mut w, TOWER, (20, 20));
    place(&mut w, EMPL, (40, 40));
    dummy(&mut w, at(20, 25)); // south of the tower
    dummy(&mut w, at(40, 46)); // south of the emplacement (2 x 2: middle (41, 41))
    w.step();
    let tower = id_at(&w, (20, 20));
    let empl = id_at(&w, (40, 40));
    let (mut max_tower, mut max_empl) = (0u8, 0u8);
    let (mut prev_tower, mut prev_empl) = (w.turret_of(tower).unwrap(), w.turret_of(empl).unwrap());
    let mut first = [None, None];
    for _ in 0..200 {
        w.step();
        let (tt, te) = (w.turret_of(tower).unwrap(), w.turret_of(empl).unwrap());
        max_tower = max_tower.max(tt.abs_diff(prev_tower));
        max_empl = max_empl.max(te.abs_diff(prev_empl));
        (prev_tower, prev_empl) = (tt, te);
        for s in w.shots() {
            let (who, k) = if s.attacker == tower {
                (tower, 0)
            } else {
                (empl, 1)
            };
            first[k].get_or_insert(w.tick().0);
            // Every shot is fired with the turret within its arc of the bearing to the victim.
            let turret = w.turret_of(who).unwrap();
            let want = bearing(&w, who, s.target);
            assert!(
                turret.abs_diff(want) <= 8,
                "fired off the arc: {turret:?} {want:?}"
            );
        }
    }
    assert_eq!(max_tower, 12, "the tower's turret turns at 12 a tick");
    assert_eq!(max_empl, 5, "the emplacement's turret turns at 5 a tick");
    // The slower turret needs the longer to come round, so it fires later.
    let (ft, fe) = (first[0].unwrap(), first[1].unwrap());
    assert!(fe > ft, "tower {ft}, emplacement {fe}");
}

#[test]
fn a_target_outside_the_arc_waits_for_the_turret() {
    // The turret is born pointing along the hull (facing 0); the victim is some 70 steps away. The
    // structure picks it on the tick it stands and the turret turns 12 a tick from then on: the first
    // shot is on the first tick on which the turret is within 8 steps of the bearing, not before.
    let mut w = world(false);
    place(&mut w, TOWER, (30, 30));
    dummy(&mut w, at(30, 34));
    let tower = EntityId::new(0, 0);
    let mut fired_at = None;
    let (mut turret, mut expected) = (Facing::new(0), None);
    let mut want = None;
    for n in 1..=40u64 {
        w.step();
        want.get_or_insert_with(|| bearing(&w, tower, EntityId::new(1, 0)));
        let want = want.unwrap();
        turret = turret.turn_toward(want, 12);
        if expected.is_none() && turret.abs_diff(want) <= 8 {
            expected = Some(n);
        }
        if fired_at.is_none() && !w.shots().is_empty() {
            fired_at = Some(w.tick().0);
        }
        assert_eq!(w.turret_of(tower), Some(turret), "tick {n}");
    }
    let expected = expected.unwrap();
    assert!(
        expected >= 5,
        "the test needs a turret that has to turn: {expected}"
    );
    assert_eq!(fired_at, Some(expected));
}

#[test]
fn a_hull_weapon_without_a_turret_fires_all_round() {
    let mut w = world(false);
    place(&mut w, HUT, (30, 30));
    dummy(&mut w, at(30, 34));
    w.step();
    let hut = id_at(&w, (30, 30));
    let shots = run(&mut w, 40);
    assert!(shots_by(&shots, hut) >= 2);
    assert!(w.turret_of(hut).is_none(), "no turret");
}

#[test]
fn orders_and_aim_commands_to_a_structure_are_refused() {
    let mut w = world(false);
    place(&mut w, TOWER, (30, 30));
    // A dummy of the tower's own side, which the tower would never choose: an AimTurret that worked
    // would make it shoot at it.
    send(
        &mut w,
        P0,
        Command::SpawnUnit {
            kind: DUMMY,
            pos: at(30, 34),
            owner: P0,
        },
    );
    w.step();
    let tower = id_at(&w, (30, 30));
    let friend = EntityId::new(1, 0);
    let before = w.get(tower).unwrap().pos;
    for order in [
        Order::Move(at(10, 10)),
        Order::AttackMove(at(10, 10)),
        Order::Attack(friend),
        Order::Pursue(friend),
        Order::HoldPosition,
        Order::Guard(sf_sim::GuardTarget::Point(at(5, 5))),
    ] {
        send(
            &mut w,
            P0,
            Command::Order {
                ids: vec![tower],
                order,
                queued: false,
            },
        );
        let report = w.step();
        assert_eq!(report.commands_executed, 0, "{order:?} was accepted");
    }
    send(
        &mut w,
        P0,
        Command::AimTurret {
            ids: vec![tower],
            target: Some(friend),
        },
    );
    let report = w.step();
    assert_eq!(report.commands_executed, 0, "AimTurret was accepted");
    assert_eq!(w.order_of(tower), Some(Order::Idle));
    assert_eq!(w.turret_target_of(tower), None);
    assert_eq!(shots_by(&run(&mut w, 100), tower), 0);
    assert_eq!(w.hp_of(friend), Some(1000));
    assert_eq!(w.get(tower).unwrap().pos, before);
}

#[test]
fn a_defence_that_uses_power_is_silent_in_a_shortage_and_one_that_does_not_is_not() {
    let mut w = world(true);
    // No plant: the tower (-20), the emplacement (-40) and the hut (-10) have nothing to run on.
    place(&mut w, TOWER, (30, 30));
    place(&mut w, EMPL, (40, 30));
    place(&mut w, HUT, (30, 40));
    place(&mut w, STATION, (40, 40)); // armed, uses nothing
    place(&mut w, BUNKER, (20, 30)); // weaponless, uses nothing
    dummy(&mut w, at(30, 34));
    dummy(&mut w, at(41, 35));
    dummy(&mut w, at(30, 44));
    dummy(&mut w, at(40, 44));
    w.step();
    // The totals are counted at the start of a step, from the state the last one left.
    w.step();
    let (tower, empl, hut, station) = (
        id_at(&w, (30, 30)),
        id_at(&w, (40, 30)),
        id_at(&w, (30, 40)),
        id_at(&w, (40, 40)),
    );
    assert!(!w.defences_powered(P0));
    let shots = run(&mut w, 150);
    assert_eq!(shots_by(&shots, tower), 0, "the tower fired without power");
    assert_eq!(
        shots_by(&shots, empl),
        0,
        "the emplacement fired without power"
    );
    assert_eq!(shots_by(&shots, hut), 0, "the hut fired without power");
    assert!(
        shots_by(&shots, station) > 0,
        "a defence that uses no power is not switched off"
    );
    // A plant restores them.
    place(&mut w, PLANT, (10, 10));
    w.step();
    w.step();
    assert!(w.defences_powered(P0));
    let shots = run(&mut w, 150);
    assert!(
        shots_by(&shots, tower) > 0,
        "the tower stayed silent with power"
    );
    assert!(shots_by(&shots, empl) > 0);
    assert!(shots_by(&shots, hut) > 0);
}

#[test]
fn a_world_without_power_rules_always_fires() {
    let mut w = world(false);
    place(&mut w, TOWER, (30, 30));
    dummy(&mut w, at(30, 34));
    w.step();
    assert_eq!(shots_by(&run(&mut w, 100), id_at(&w, (30, 30))) > 0, true);
}

#[test]
fn a_structure_under_construction_does_nothing_until_it_stands() {
    let mut w = world(false);
    place(&mut w, ANCHOR, (4, 4));
    dummy(&mut w, at(14, 10));
    w.step();
    send(
        &mut w,
        P0,
        Command::BuildStructure {
            kind: TOWER,
            cell: Cell::new(14, 6),
        },
    );
    // 40 build ticks. The dummy stands 4 cells from the tower's middle.
    let (mut first, mut done) = (None, None);
    for _ in 0..120 {
        w.step();
        if done.is_none() && !w.completions().is_empty() {
            done = Some(w.tick().0);
        }
        if first.is_none() && !w.shots().is_empty() {
            first = Some(w.tick().0);
        }
    }
    let (first, done) = (
        first.expect("it fired once it stood"),
        done.expect("it was built"),
    );
    assert!(
        first > done - 1,
        "fired on tick {first}, before it was complete on {done}"
    );
    assert!(done > 30, "the tower was built too fast to test: {done}");
}

#[test]
fn a_weaponless_bunker_does_nothing() {
    let mut w = world(false);
    place(&mut w, BUNKER, (30, 30));
    dummy(&mut w, at(30, 33));
    w.step();
    let bunker = id_at(&w, (30, 30));
    let shots = run(&mut w, 200);
    assert!(shots.is_empty());
    assert_eq!(w.turret_target_of(bunker), None);
    assert!(w.turret_of(bunker).is_none());
    assert_eq!(w.hp_of(EntityId::new(1, 0)), Some(1000));
}

#[test]
fn the_same_world_replays_to_the_same_checksums() {
    let make = || {
        let mut w = world(true);
        place(&mut w, PLANT, (10, 10));
        place(&mut w, TOWER, (30, 30));
        place(&mut w, EMPL, (36, 30));
        dummy(&mut w, at(33, 35));
        dummy(&mut w, at(33, 26));
        for _ in 0..300 {
            w.step();
        }
        w.checksum_history().to_vec()
    };
    let (a, b) = (make(), make());
    assert_eq!(a, b);
    assert!(a.windows(2).any(|p| p[0] != p[1]));
}
