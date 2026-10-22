//! Target selection and automatic return fire (WP5.4, `docs/rules/targeting.md`), through the public API:
//! what a unit picks and why (the armor table, the distance, the position), the scan cooldown, when a
//! target is let go, what each stance does (idle, hold, guard, attack-move, move), return fire and the
//! pursuit that bounds it, an explicit order that overrides it all, a unit at zero hit points, and that no
//! outcome depends on the order the units were spawned in.
//!
//! The numbers are small and invented so a test can follow them tick by tick; the shipped set is
//! `assets/config`, which `tools/sf_headless/tests/tanks_vs_infantry.rs` runs.
//!
//! Every shooter here that is not about aiming has an all-round arc, so that its random birth facing (it
//! comes from the order the unit was spawned in) cannot decide when it fires.

use sf_math::{Fx, Vec2};
use sf_sim::{
    ARRIVE_TOLERANCE, ArmorTable, Command, ConfigError, EntityId, Footprint, GuardTarget, Kind,
    Loadout, Mount, Order, PlayerId, ProjectileKind, Shot, TargetingConfig, UnitStats, UnitTable,
    WeaponStats, WeaponTable, WeaponsError, World, WorldConfig, per_tick,
};

const MAP: u32 = 64;

// Damage types.
const BULLET: u16 = 0;
const ROCKET: u16 = 1;
const PRECISION: u16 = 2;
// Armor types.
const NONE: u16 = 0;
const SOFT: u16 = 1;
const HARD: u16 = 3;
const STRUCT: u16 = 4;

// Armed units.
const RIFLEMAN: Kind = Kind(1); // range 4, a rifle: 6 bullet damage every 10 ticks, all round
const ROCKETEER: Kind = Kind(2); // range 5, a rocket: 40 every 20 ticks
const SNIPER: Kind = Kind(3); // range 8, 10 precision damage every 30 ticks (nothing against vehicles)
const GRUNT: Kind = Kind(4); // a rifleman whose gun points only 8 steps either side of the hull
const TANK: Kind = Kind(5); // hard, 400 hit points, a cannon on a turret, range 5
const MORTAR: Kind = Kind(6); // range 10 and not nearer than 3
const HYBRID: Kind = Kind(7); // a rifle (range 4) in slot 0 and a long gun (range 8, precision) in slot 1
const ARTY: Kind = Kind(8); // range 20, 5 damage every 30 ticks, 40 hit points
const GLASS: Kind = Kind(9); // seven hit points and a rifle
const LONG: Kind = Kind(10); // the shooter of the long gun alone, to compare with the hybrid
// Things that are shot at.
const DUMMY: Kind = Kind(20); // soft, 1000 hit points, cannot shoot
const IRON: Kind = Kind(21); // hard, 1000
const WALL: Kind = Kind(22); // a structure of 2 by 2 cells, 1000
const ROCK: Kind = Kind(24); // an object that cannot be hurt (zero hit points)
const THIN: Kind = Kind(25); // soft, 12 hit points: two rifle shots

const RIFLE: u16 = 1;
const ROCKET_GUN: u16 = 2;
const SNIPE: u16 = 3;
const GRUNT_GUN: u16 = 4;
const CANNON: u16 = 5;
const MORTAR_GUN: u16 = 6;
const LONG_GUN: u16 = 7;
const LOBBER: u16 = 8;

fn at(x: i32, y: i32) -> Vec2 {
    Vec2::from_ints(x, y)
}

fn unit(kind: Kind, hp: u32, range: i32, turret: u8) -> UnitStats {
    UnitStats {
        kind,
        hp,
        movement_class: Some(0),
        speed_per_tick: per_tick(Fx::from_int(2)),
        // A unit turns to any heading in a tick, so that the facing it was born with (from the order
        // it was spawned in) cannot decide when it sets off. The aiming test uses the slow GRUNT.
        turn_per_tick: 255,
        size: Fx::ONE,
        footprint: None,
        weapon_range: Fx::from_int(range),
        turret_per_tick: turret,
    }
}

fn units() -> UnitTable {
    UnitTable::new(vec![
        unit(RIFLEMAN, 50, 4, 0),
        unit(ROCKETEER, 50, 5, 0),
        unit(SNIPER, 40, 8, 0),
        UnitStats {
            turn_per_tick: 16,
            ..unit(GRUNT, 50, 4, 0)
        },
        unit(TANK, 400, 5, 12),
        unit(MORTAR, 100, 10, 0),
        unit(HYBRID, 100, 4, 0),
        unit(ARTY, 40, 20, 0),
        unit(GLASS, 7, 4, 0),
        unit(LONG, 40, 8, 0),
        unit(DUMMY, 1000, 0, 0),
        unit(IRON, 1000, 0, 0),
        UnitStats {
            kind: WALL,
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
        },
        UnitStats {
            movement_class: None,
            speed_per_tick: Fx::ZERO,
            turn_per_tick: 0,
            ..unit(ROCK, 0, 0, 0)
        },
        unit(THIN, 12, 0, 0),
    ])
    .unwrap()
}

fn weapon(kind: u16, damage_type: u16, damage: u32, range: i32, reload: u32) -> WeaponStats {
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
        mount: Mount::Hull,
        fire_arc: 128,
    }
}

fn weapons() -> Vec<WeaponStats> {
    vec![
        weapon(RIFLE, BULLET, 6, 4, 10),
        weapon(ROCKET_GUN, ROCKET, 40, 5, 20),
        weapon(SNIPE, PRECISION, 10, 8, 30),
        WeaponStats {
            fire_arc: 8,
            ..weapon(GRUNT_GUN, BULLET, 6, 4, 10)
        },
        WeaponStats {
            mount: Mount::Turret,
            fire_arc: 8,
            ..weapon(CANNON, ROCKET, 30, 5, 20)
        },
        WeaponStats {
            min_range: Fx::from_int(3),
            ..weapon(MORTAR_GUN, BULLET, 10, 10, 20)
        },
        weapon(LONG_GUN, PRECISION, 40, 8, 30),
        weapon(LOBBER, BULLET, 5, 20, 30),
    ]
}

fn loadouts() -> Vec<Loadout> {
    let l = |unit: Kind, w: &[u16]| Loadout {
        unit,
        weapons: w.to_vec(),
    };
    vec![
        l(RIFLEMAN, &[RIFLE]),
        l(ROCKETEER, &[ROCKET_GUN]),
        l(SNIPER, &[SNIPE]),
        l(GRUNT, &[GRUNT_GUN]),
        l(TANK, &[CANNON]),
        l(MORTAR, &[MORTAR_GUN]),
        l(HYBRID, &[RIFLE, LONG_GUN]),
        l(ARTY, &[LOBBER]),
        l(GLASS, &[RIFLE]),
        l(LONG, &[LONG_GUN]),
    ]
}

fn armor_of(kind: Kind) -> u16 {
    match kind {
        TANK | IRON => HARD,
        WALL => STRUCT,
        ROCK => NONE,
        _ => SOFT,
    }
}

/// Bullets: soft 100, hard 10, structure 20. Rockets: soft 40, hard 100, structure 70. Precision: soft 100
/// and nothing against the rest.
fn armor_table() -> ArmorTable {
    let cells = vec![
        (BULLET, NONE, 0),
        (BULLET, SOFT, 100),
        (BULLET, HARD, 10),
        (BULLET, STRUCT, 20),
        (ROCKET, NONE, 0),
        (ROCKET, SOFT, 40),
        (ROCKET, HARD, 100),
        (ROCKET, STRUCT, 70),
        (PRECISION, NONE, 0),
        (PRECISION, SOFT, 100),
        (PRECISION, HARD, 0),
        (PRECISION, STRUCT, 0),
    ];
    ArmorTable::new(
        vec![BULLET, ROCKET, PRECISION],
        vec![NONE, SOFT, HARD, STRUCT],
        cells,
        units().iter().map(|u| (u.kind, armor_of(u.kind))).collect(),
    )
    .unwrap()
}

fn table() -> WeaponTable {
    WeaponTable::new(weapons(), loadouts())
        .unwrap()
        .with_armor(armor_table())
        .unwrap()
}

/// A world with weapons and no targeting.
fn armed() -> World {
    World::new(WorldConfig::new(7).with_map_size(MAP, MAP))
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_weapons(table())
        .unwrap()
}

/// A world whose units choose their own targets, with the shipped settings (a scan every 10 ticks, a
/// pursuit of 6 cells).
fn fought() -> World {
    fought_with(TargetingConfig::DEFAULT)
}

fn fought_with(config: TargetingConfig) -> World {
    World::new(WorldConfig::new(7).with_map_size(MAP, MAP))
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_weapons(table())
        .unwrap()
        .with_targeting(config)
        .unwrap()
}

/// One thing to put in the world: a kind, an owner and a place.
type Spawn = (Kind, u8, Vec2);

/// Puts the list in the world on one tick, in that order (the `i`-th is slot `i`), and runs that tick.
fn spawn(w: &mut World, list: &[Spawn]) -> Vec<EntityId> {
    let tick = w.tick();
    for &(kind, owner, pos) in list {
        let command = if kind == WALL {
            Command::PlaceStructure {
                kind,
                cell: sf_sim::Cell {
                    x: pos.x.floor_int() as u32,
                    y: pos.y.floor_int() as u32,
                },
                owner: PlayerId(owner),
            }
        } else {
            Command::SpawnUnit {
                kind,
                pos,
                owner: PlayerId(owner),
            }
        };
        w.schedule(tick, PlayerId(0), command).unwrap();
    }
    w.step();
    let ids: Vec<EntityId> = (0..list.len() as u32)
        .map(|i| EntityId::new(i, 0))
        .collect();
    for (id, s) in ids.iter().zip(list) {
        assert!(w.get(*id).is_some(), "{s:?} was not placed");
    }
    ids
}

fn order(who: EntityId, order: Order) -> Command {
    Command::Order {
        ids: vec![who],
        order,
        queued: false,
    }
}

fn attack(who: EntityId, victim: EntityId) -> Command {
    order(who, Order::Attack(victim))
}

fn mv(who: EntityId, to: Vec2) -> Command {
    order(who, Order::Move(to))
}

/// Gives the commands on the next tick and runs `n` ticks in all, returning every shot.
fn go(w: &mut World, commands: Vec<Command>, n: u32) -> Vec<Shot> {
    let tick = w.tick();
    for c in commands {
        w.schedule(tick, PlayerId(0), c).unwrap();
    }
    let mut shots = Vec::new();
    for _ in 0..n {
        w.step();
        shots.extend_from_slice(w.shots());
    }
    shots
}

fn run(w: &mut World, n: u32) -> Vec<Shot> {
    go(w, vec![], n)
}

fn shots_by(shots: &[Shot], who: EntityId) -> Vec<Shot> {
    shots
        .iter()
        .copied()
        .filter(|s| s.attacker == who)
        .collect()
}

fn pos(w: &World, id: EntityId) -> Vec2 {
    w.get(id).unwrap().pos
}

fn hp(w: &World, id: EntityId) -> u32 {
    w.hp_of(id).unwrap()
}

/// Steps until `done` is true or `limit` ticks have passed; returns the number of ticks run, and every
/// shot.
fn until(w: &mut World, limit: u32, mut done: impl FnMut(&World) -> bool) -> (u32, Vec<Shot>) {
    let mut shots = Vec::new();
    for n in 0..limit {
        w.step();
        shots.extend_from_slice(w.shots());
        if done(w) {
            return (n + 1, shots);
        }
    }
    panic!("not done in {limit} ticks");
}

// ---- the settings ------------------------------------------------------------------------------------

#[test]
fn the_settings_are_checked_and_only_given_before_the_start() {
    let plain = || World::new(WorldConfig::new(1)).unwrap();
    assert_eq!(plain().targeting_config(), None);
    let ok = TargetingConfig::from_cells(10, 6);
    assert_eq!(ok, TargetingConfig::DEFAULT);
    assert_eq!(ok, TargetingConfig::default());
    assert_eq!(
        plain().with_targeting(ok).unwrap().targeting_config(),
        Some(ok)
    );
    for bad in [
        TargetingConfig::from_cells(0, 6),
        TargetingConfig::from_cells(1001, 6),
        TargetingConfig::from_cells(10, 33),
        TargetingConfig {
            pursuit: Fx::from_int(-1),
            ..ok
        },
    ] {
        assert!(!bad.is_valid(), "{bad:?}");
        assert_eq!(
            plain().with_targeting(bad).err(),
            Some(ConfigError::Targeting),
            "{bad:?}"
        );
    }
    assert!(TargetingConfig::from_cells(1, 0).is_valid());
    assert!(TargetingConfig::from_cells(1000, 32).is_valid());
    // After the first tick it is too late.
    let mut started = plain();
    started.step();
    assert_eq!(
        started.with_targeting(ok).err(),
        Some(ConfigError::Targeting)
    );
    let words = ConfigError::Targeting.to_string();
    assert!(words.contains("targeting"), "{words}");
}

#[test]
fn a_hull_weapon_on_a_unit_with_a_turret_is_refused() {
    // WP5.4 decides that the turret aims and the hull does not follow it for a weapon, so a hull
    // weapon on such a unit would never be aimed: the world refuses the table.
    let mut ws = weapons();
    ws[4].mount = Mount::Hull; // the cannon of the tank
    let t = WeaponTable::new(ws, loadouts()).unwrap();
    let e = World::new(WorldConfig::new(1))
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_weapons(t)
        .err();
    assert_eq!(
        e,
        Some(ConfigError::Weapons(WeaponsError::HullWeaponOnTurret {
            unit: TANK,
            weapon: CANNON
        }))
    );
    let words = WeaponsError::HullWeaponOnTurret {
        unit: TANK,
        weapon: CANNON,
    }
    .to_string();
    assert!(words.contains("hull-mounted"), "{words}");
}

#[test]
fn a_world_without_settings_chooses_nothing() {
    // Weapons but no targeting: units fire only at the victim of an attack order, as before WP5.4.
    let mut w = armed();
    let ids = spawn(&mut w, &[(RIFLEMAN, 0, at(10, 10)), (DUMMY, 1, at(13, 10))]);
    assert!(run(&mut w, 60).is_empty());
    assert_eq!(w.order_of(ids[0]), Some(Order::Idle));
    assert_eq!(w.engagement_of(ids[0]), None);
    assert_eq!(hp(&w, ids[1]), 1000);
    // And an attack order still works.
    let shots = go(&mut w, vec![attack(ids[0], ids[1])], 11);
    assert_eq!(shots.len(), 2);
}

// ---- choosing ----------------------------------------------------------------------------------------

#[test]
fn an_idle_unit_picks_an_enemy_in_reach_and_fires_at_it_every_reload() {
    let mut w = fought();
    let ids = spawn(&mut w, &[(RIFLEMAN, 0, at(10, 10)), (DUMMY, 1, at(13, 10))]);
    let (r, d) = (ids[0], ids[1]);
    // It chose on the tick it was spawned, and fires on that tick: the choice is made before the weapons.
    assert_eq!(w.order_of(r), Some(Order::Pursue(d)));
    let e = w.engagement_of(r).unwrap();
    assert_eq!(e.target, d);
    assert!(!e.following);
    assert_eq!(e.resumes, Order::Idle);
    assert_eq!(w.shots().len(), 1);
    assert_eq!(hp(&w, d), 994);
    // Then once a reload, to the tick.
    let shots = run(&mut w, 40);
    let ticks: Vec<u64> = shots.iter().map(|s| s.tick.0).collect();
    assert_eq!(ticks, [10, 20, 30, 40]);
    assert!(shots.iter().all(|s| s.attacker == r && s.target == d));
    assert_eq!(hp(&w, d), 1000 - 6 * 5);
    // The dummy does not shoot back; the rifleman did not move.
    assert_eq!(pos(&w, r), at(10, 10));
}

#[test]
fn it_does_not_pick_its_own_side_nor_what_is_out_of_reach_nor_what_cannot_be_hurt() {
    let mut w = fought();
    let ids = spawn(
        &mut w,
        &[
            (RIFLEMAN, 0, at(10, 10)),
            (DUMMY, 0, at(12, 10)),  // a friend, in reach
            (DUMMY, 1, at(16, 10)),  // an enemy at 6: out of reach (4 and half a cell of body)
            (ROCK, 1, at(10, 12)),   // an enemy that cannot be hurt
            (SNIPER, 0, at(30, 30)), // a sniper...
            (IRON, 1, at(32, 30)),   // ...next to a tank-armored enemy it does nothing to
        ],
    );
    assert!(run(&mut w, 60).is_empty());
    for id in [ids[0], ids[4]] {
        assert_eq!(w.order_of(id), Some(Order::Idle));
        assert_eq!(w.engagement_of(id), None);
    }
    assert_eq!(hp(&w, ids[1]), 1000);
    assert_eq!(hp(&w, ids[2]), 1000);
    assert_eq!(hp(&w, ids[5]), 1000);
}

#[test]
fn the_armor_table_decides_what_a_weapon_prefers() {
    // A rifle (bullets: soft 100, hard 10, structure 20) takes the soft target over the nearer hard one.
    let mut w = fought();
    let ids = spawn(
        &mut w,
        &[
            (RIFLEMAN, 0, at(10, 10)),
            (IRON, 1, at(12, 10)),
            (DUMMY, 1, at(14, 10)),
        ],
    );
    assert_eq!(w.order_of(ids[0]), Some(Order::Pursue(ids[2])));
    // A rocket (soft 40, hard 100, structure 70) takes the hard one over the nearer soft one, and the
    // wall over the soft one.
    let mut w = fought();
    let ids = spawn(
        &mut w,
        &[
            (ROCKETEER, 0, at(10, 10)),
            (DUMMY, 1, at(12, 10)),
            (IRON, 1, at(14, 10)),
            (WALL, 1, at(10, 13)),
        ],
    );
    assert_eq!(w.order_of(ids[0]), Some(Order::Pursue(ids[2])));
    // Without the hard one it takes the wall (70) over the soft one (40), though the soft one is nearer.
    let mut w = fought();
    let ids = spawn(
        &mut w,
        &[
            (ROCKETEER, 0, at(10, 10)),
            (DUMMY, 1, at(12, 10)),
            (WALL, 1, at(10, 13)),
        ],
    );
    assert_eq!(w.order_of(ids[0]), Some(Order::Pursue(ids[2])));
    // And the rifle takes a wall (20 percent) when it is all there is.
    let mut w = fought();
    let ids = spawn(&mut w, &[(RIFLEMAN, 0, at(10, 10)), (WALL, 1, at(10, 13))]);
    assert_eq!(w.order_of(ids[0]), Some(Order::Pursue(ids[1])));
}

#[test]
fn a_weapon_that_does_nothing_to_a_target_does_not_pick_it() {
    // The sniper's precision is zero against a hard armor: it picks the soft one though it is farther,
    // and with only the hard one in reach it picks nothing.
    let mut w = fought();
    let ids = spawn(
        &mut w,
        &[
            (SNIPER, 0, at(10, 10)),
            (IRON, 1, at(12, 10)),
            (DUMMY, 1, at(17, 10)),
        ],
    );
    assert_eq!(w.order_of(ids[0]), Some(Order::Pursue(ids[2])));
    let mut w = fought();
    let ids = spawn(&mut w, &[(SNIPER, 0, at(10, 10)), (IRON, 1, at(12, 10))]);
    assert_eq!(w.order_of(ids[0]), Some(Order::Idle));
    assert!(run(&mut w, 60).is_empty());
}

#[test]
fn among_equally_good_targets_the_nearer_wins_and_then_the_position_not_the_slot() {
    // Nearer first.
    let mut w = fought();
    let ids = spawn(
        &mut w,
        &[
            (RIFLEMAN, 0, at(10, 10)),
            (DUMMY, 1, at(13, 10)),
            (DUMMY, 1, at(12, 10)),
        ],
    );
    assert_eq!(w.order_of(ids[0]), Some(Order::Pursue(ids[2])));
    // Exactly as near: the one with the smaller y, then the smaller x, whichever was spawned first.
    for flip in [false, true] {
        let mut list = vec![(DUMMY, 1, at(10, 7)), (DUMMY, 1, at(10, 13))];
        if flip {
            list.reverse();
        }
        list.insert(0, (RIFLEMAN, 0, at(10, 10)));
        let mut w = fought();
        let ids = spawn(&mut w, &list);
        let Some(Order::Pursue(chosen)) = w.order_of(ids[0]) else {
            panic!("it chose nothing");
        };
        assert_eq!(pos(&w, chosen), at(10, 7), "flip {flip}");
    }
    for flip in [false, true] {
        let mut list = vec![(DUMMY, 1, at(7, 10)), (DUMMY, 1, at(13, 10))];
        if flip {
            list.reverse();
        }
        list.insert(0, (RIFLEMAN, 0, at(10, 10)));
        let mut w = fought();
        let ids = spawn(&mut w, &list);
        let Some(Order::Pursue(chosen)) = w.order_of(ids[0]) else {
            panic!("it chose nothing");
        };
        assert_eq!(pos(&w, chosen), at(7, 10), "flip {flip}");
    }
}

#[test]
fn the_minimum_range_is_kept_a_target_too_near_is_not_picked() {
    // The mortar cannot hit what is within 3 cells (and a half of body) of it: the farther target is
    // picked though the near one is nearer; with only the near one it picks nothing and does not back off.
    let mut w = fought();
    let ids = spawn(
        &mut w,
        &[
            (MORTAR, 0, at(10, 10)),
            (DUMMY, 1, at(11, 10)),
            (DUMMY, 1, at(16, 10)),
        ],
    );
    assert_eq!(w.order_of(ids[0]), Some(Order::Pursue(ids[2])));
    let mut w = fought();
    let ids = spawn(&mut w, &[(MORTAR, 0, at(10, 10)), (DUMMY, 1, at(11, 10))]);
    assert_eq!(w.order_of(ids[0]), Some(Order::Idle));
    assert!(run(&mut w, 60).is_empty());
    assert_eq!(pos(&w, ids[0]), at(10, 10));
}

#[test]
fn of_two_weapons_the_better_one_chooses_and_a_weapon_that_does_nothing_stays_quiet() {
    // Against a soft target the long gun (40 precision, 30 ticks) beats the rifle (6 bullets, 10), and
    // both are in reach at 3 cells: both fire, one pull each on the first tick.
    let mut w = fought();
    let ids = spawn(&mut w, &[(HYBRID, 0, at(10, 10)), (DUMMY, 1, at(13, 10))]);
    let first = w.shots().to_vec();
    assert_eq!(first.len(), 2, "{first:?}");
    assert_eq!(hp(&w, ids[1]), 1000 - 6 - 40);
    // Against a hard target the rifle is all that hurts (10 percent), so it picks, and the long gun
    // (zero percent) never fires, though it is in reach.
    let mut w = fought();
    let ids = spawn(&mut w, &[(HYBRID, 0, at(10, 10)), (IRON, 1, at(13, 10))]);
    assert_eq!(w.order_of(ids[0]), Some(Order::Pursue(ids[1])));
    let shots = go(&mut w, vec![], 60);
    assert!(!shots.is_empty());
    assert!(shots.iter().all(|s| s.slot == 0 && s.weapon == RIFLE));
    // Only the long gun reaches a soft target at 7: that is the weapon it picks with, and it fires.
    let mut w = fought();
    let ids = spawn(&mut w, &[(HYBRID, 0, at(10, 10)), (DUMMY, 1, at(17, 10))]);
    assert_eq!(w.order_of(ids[0]), Some(Order::Pursue(ids[1])));
    assert!(w.shots().iter().all(|s| s.weapon == LONG_GUN));
    assert_eq!(w.shots().len(), 1);
}

// ---- the cooldown and letting go ---------------------------------------------------------------------

#[test]
fn a_unit_looks_round_only_once_a_cooldown_and_picks_up_a_target_that_comes_in_reach() {
    let mut w = fought();
    let ids = spawn(&mut w, &[(RIFLEMAN, 0, at(10, 10)), (DUMMY, 1, at(20, 10))]);
    let (r, d) = (ids[0], ids[1]);
    // It looked on tick 0 and found nothing; the cooldown runs.
    assert_eq!(w.order_of(r), Some(Order::Idle));
    assert_eq!(w.scan_wait_of(r), Some(10));
    // The dummy walks up (2 cells a second: 0.1 a tick) to the edge of its reach; the unit notices at
    // its next look, which is at a multiple of ten ticks and not at the tick it came in reach.
    let came = {
        // Tick on which the dummy first stands within 4 cells and a half (its own half cell) of it.
        let mut probe = fought();
        let p = spawn(
            &mut probe,
            &[(RIFLEMAN, 0, at(10, 10)), (DUMMY, 1, at(20, 10))],
        );
        let tick = probe.tick();
        probe
            .schedule(tick, PlayerId(0), mv(p[1], at(8, 10)))
            .unwrap();
        until(&mut probe, 200, |w| {
            (pos(w, p[1]) - at(10, 10)).length() <= Fx::from_ratio(9, 2)
        })
        .0 + tick.0 as u32
    };
    let tick = w.tick();
    w.schedule(tick, PlayerId(0), mv(d, at(8, 10))).unwrap();
    let (n, _) = until(&mut w, 200, |w| w.order_of(r) == Some(Order::Pursue(d)));
    let chose = n + tick.0 as u32;
    assert!(chose >= came, "it cannot pick what is not in reach yet");
    assert!(
        chose < came + 10,
        "it looks round every ten ticks: {came} {chose}"
    );
    assert_eq!((chose - 1) % 10, 0, "at the tick of a look: {chose}");
}

#[test]
fn the_cooldown_is_data() {
    let mut w = fought_with(TargetingConfig::from_cells(25, 6));
    let ids = spawn(&mut w, &[(RIFLEMAN, 0, at(10, 10))]);
    assert_eq!(w.scan_wait_of(ids[0]), Some(25));
    run(&mut w, 3);
    assert_eq!(w.scan_wait_of(ids[0]), Some(22));
    // An enemy appears (tick 4): nothing happens until the 25 ticks are over.
    spawn(&mut w, &[(DUMMY, 1, at(12, 10))]);
    let d = EntityId::new(1, 0);
    assert_eq!(w.order_of(ids[0]), Some(Order::Idle));
    let (n, _) = until(&mut w, 100, |w| {
        w.order_of(ids[0]) == Some(Order::Pursue(d))
    });
    assert_eq!(n, 21, "the look comes 25 ticks after the last: tick 25");
    assert_eq!(w.tick().0, 26);
}

#[test]
fn a_unit_lets_go_of_a_target_that_leaves_its_reach() {
    let mut w = fought();
    let ids = spawn(&mut w, &[(RIFLEMAN, 0, at(10, 10)), (DUMMY, 1, at(13, 10))]);
    let (r, d) = (ids[0], ids[1]);
    assert_eq!(w.order_of(r), Some(Order::Pursue(d)));
    // The dummy walks off; the rifleman does not follow (nobody shot at it).
    run_command(&mut w, mv(d, at(30, 10)));
    let (_, shots) = until(&mut w, 100, |w| w.order_of(r) == Some(Order::Idle));
    assert!(shots.iter().all(|s| s.attacker == r));
    assert_eq!(pos(&w, r), at(10, 10), "a chosen target is not followed");
    // Out of reach it is never picked again.
    let more = run(&mut w, 100);
    assert!(more.is_empty(), "{more:?}");
}

fn run_command(w: &mut World, c: Command) {
    let tick = w.tick();
    w.schedule(tick, PlayerId(0), c).unwrap();
}

#[test]
fn a_unit_lets_go_of_a_target_at_zero_hit_points_and_does_not_pick_it_again() {
    let mut w = fought();
    let ids = spawn(
        &mut w,
        &[
            (RIFLEMAN, 0, at(10, 10)),
            (THIN, 1, at(12, 10)),  // 12 hit points: two rifle shots
            (DUMMY, 1, at(10, 13)), // farther
        ],
    );
    let (r, thin, dummy) = (ids[0], ids[1], ids[2]);
    assert_eq!(w.order_of(r), Some(Order::Pursue(thin)));
    // Shots at tick 0 and 10: the thin one is at zero after the second. The unit looks round again
    // on the tick it lets go, because its cooldown (ten ticks from tick 0) has run out, and takes the
    // other one; its weapon is reloading until tick 20.
    let (_, shots) = until(&mut w, 30, |w| hp(w, thin) == 0);
    assert!(w.is_alive(thin), "nothing is removed before WP5.5");
    assert_eq!(shots.iter().map(|s| s.tick.0).collect::<Vec<_>>(), [10]);
    assert!(shots.iter().all(|s| s.target == thin));
    assert_eq!(
        w.order_of(r),
        Some(Order::Pursue(thin)),
        "it lets go on the next tick"
    );
    w.step();
    assert_eq!(w.order_of(r), Some(Order::Pursue(dummy)));
    let shots = run(&mut w, 40);
    let ticks: Vec<u64> = shots.iter().map(|s| s.tick.0).collect();
    assert_eq!(ticks, [20, 30, 40, 50]);
    assert!(shots.iter().all(|s| s.target == dummy), "{shots:?}");
}

#[test]
fn a_despawned_target_ends_the_engagement_and_the_next_one_is_picked_at_the_next_look() {
    let mut w = fought();
    let ids = spawn(
        &mut w,
        &[
            (RIFLEMAN, 0, at(10, 10)),
            (DUMMY, 1, at(12, 10)),
            (DUMMY, 1, at(10, 13)),
        ],
    );
    let (r, a, b) = (ids[0], ids[1], ids[2]);
    assert_eq!(w.order_of(r), Some(Order::Pursue(a)));
    // The first one is gone at tick 3: the rifleman is idle at once, with seven ticks of its
    // cooldown left, and takes the other at tick 10, not before.
    run(&mut w, 2);
    run_command(&mut w, Command::Despawn { id: a });
    let mut idle_since = None;
    let mut shots = Vec::new();
    for _ in 0..30 {
        w.step();
        shots.extend_from_slice(w.shots());
        let tick = w.tick().0 - 1;
        match w.order_of(r).unwrap() {
            Order::Idle => idle_since = idle_since.or(Some(tick)),
            Order::Pursue(t) => {
                assert_eq!(t, b);
                assert_eq!(tick, 10, "the next look");
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(idle_since, Some(3));
    assert!(shots.iter().all(|s| s.tick.0 >= 10), "{shots:?}");
    // A new unit in the old target's slot is not the old target.
    assert_ne!(w.order_of(r), Some(Order::Pursue(a)));
}

// ---- the stances -------------------------------------------------------------------------------------

#[test]
fn a_unit_on_hold_fires_from_where_it_stands_and_never_walks() {
    let mut w = fought();
    let r = spawn(&mut w, &[(RIFLEMAN, 0, at(10, 10))])[0];
    run_command(&mut w, order(r, Order::HoldPosition));
    run(&mut w, 2);
    assert_eq!(w.order_of(r), Some(Order::HoldPosition));
    // Two enemies appear on tick 3: one in reach, one out of it. The hold looks round at tick 10.
    spawn(&mut w, &[(DUMMY, 1, at(13, 10)), (DUMMY, 1, at(18, 10))]);
    let (near, far) = (EntityId::new(1, 0), EntityId::new(2, 0));
    let mut shots = Vec::new();
    for _ in 0..40 {
        w.step();
        shots.extend_from_slice(w.shots());
        if w.order_of(r) == Some(Order::Pursue(near)) {
            break;
        }
    }
    assert_eq!(w.tick().0, 11, "it picked at the look of tick 10");
    assert_eq!(shots.iter().map(|s| s.tick.0).collect::<Vec<_>>(), [10]);
    let e = w.engagement_of(r).unwrap();
    assert!(!e.following);
    assert_eq!(e.resumes, Order::HoldPosition);
    let shots = run(&mut w, 30);
    assert!(!shots.is_empty() && shots.iter().all(|s| s.target == near));
    assert_eq!(pos(&w, r), at(10, 10));
    // The near target walks away: the hold takes its old order back and stays, and the far one is
    // never walked to.
    run_command(&mut w, mv(near, at(40, 10)));
    until(&mut w, 100, |w| w.order_of(r) == Some(Order::HoldPosition));
    run(&mut w, 100);
    assert_eq!(pos(&w, r), at(10, 10));
    assert_eq!(hp(&w, far), 1000);
}

#[test]
fn a_guard_picks_a_target_in_reach_and_goes_back_to_its_post() {
    let mut w = fought();
    let post = at(10, 10);
    let ids = spawn(&mut w, &[(RIFLEMAN, 0, post), (THIN, 1, at(12, 10))]);
    let (r, thin) = (ids[0], ids[1]);
    run_command(&mut w, order(r, Order::Guard(GuardTarget::Point(post))));
    w.step();
    // The guard has its first look at tick 10 (it was spawned and looked at tick 0 as an idle unit).
    let (_, _) = until(&mut w, 40, |w| w.order_of(r) == Some(Order::Pursue(thin)));
    assert_eq!(
        w.engagement_of(r).unwrap().resumes,
        Order::Guard(GuardTarget::Point(post))
    );
    // Two shots and it is dead; the guard has its order back.
    until(&mut w, 40, |w| hp(w, thin) == 0);
    w.step();
    assert_eq!(w.order_of(r), Some(Order::Guard(GuardTarget::Point(post))));
}

#[test]
fn a_unit_on_attack_move_stops_to_fight_what_it_meets_and_goes_on() {
    let mut w = fought();
    let goal = at(40, 10);
    let ids = spawn(&mut w, &[(RIFLEMAN, 0, at(10, 10)), (THIN, 1, at(20, 10))]);
    let (r, thin) = (ids[0], ids[1]);
    run_command(&mut w, order(r, Order::AttackMove(goal)));
    // It marches (2 cells a second), notices the thin one when it comes in reach, and stops.
    let (_, _) = until(&mut w, 400, |w| w.order_of(r) == Some(Order::Pursue(thin)));
    let stopped_at = pos(&w, r);
    assert!(stopped_at.x > at(10, 10).x && stopped_at.x < at(20, 10).x);
    assert_eq!(w.engagement_of(r).unwrap().resumes, Order::AttackMove(goal));
    // It stands while it shoots (two shots), and then it marches on to the goal.
    let (_, shots) = until(&mut w, 100, |w| hp(w, thin) == 0);
    assert_eq!(pos(&w, r), stopped_at);
    assert!(!shots.is_empty());
    w.step();
    assert_eq!(w.order_of(r), Some(Order::AttackMove(goal)));
    until(&mut w, 600, |w| w.order_of(r) == Some(Order::Idle));
    assert!(
        (pos(&w, r) - goal).length() <= ARRIVE_TOLERANCE,
        "{:?}",
        pos(&w, r)
    );
}

#[test]
fn a_unit_on_a_move_order_does_not_stop_and_does_not_fire_back() {
    let mut w = fought();
    let goal = at(40, 10);
    let ids = spawn(
        &mut w,
        &[(RIFLEMAN, 0, at(10, 10)), (ROCKETEER, 1, at(20, 11))],
    );
    let (r, shooter) = (ids[0], ids[1]);
    // The rifleman walks past the rocket thrower, which is at attack-move range of its whole path and
    // ordered to shoot it.
    let shots = go(
        &mut w,
        vec![mv(r, goal), order(shooter, Order::Attack(r))],
        400,
    );
    assert!(shots_by(&shots, r).is_empty(), "a move does not fire");
    assert!(
        !shots_by(&shots, shooter).is_empty(),
        "it was shot at on the way"
    );
    assert!(shots_by(&shots, shooter).iter().all(|s| s.target == r));
    // It never engaged, and got where it was going (alive or not, see the zero-hit-point test).
    assert_eq!(w.engagement_of(r), None);
}

#[test]
fn an_explicit_order_overrides_the_choice_and_a_stop_ends_it() {
    let mut w = fought();
    let ids = spawn(
        &mut w,
        &[
            (RIFLEMAN, 0, at(10, 10)),
            (DUMMY, 1, at(12, 10)),
            (DUMMY, 1, at(10, 13)),
        ],
    );
    let (r, a, b) = (ids[0], ids[1], ids[2]);
    assert_eq!(w.order_of(r), Some(Order::Pursue(a)));
    // An attack order on the other one replaces the choice; the unit fires at that one only.
    let shots = go(&mut w, vec![attack(r, b)], 40);
    assert_eq!(w.order_of(r), Some(Order::Attack(b)));
    assert_eq!(w.engagement_of(r), None);
    assert!(shots.iter().all(|s| s.target == b), "{shots:?}");
    // A unit with an attack order is not distracted by the enemy next to it: the dummy at 12 is in reach
    // all the time and is never picked, though it is the nearer.
    let mut w = fought();
    let ids = spawn(
        &mut w,
        &[
            (RIFLEMAN, 0, at(10, 10)),
            (DUMMY, 1, at(12, 10)),
            (DUMMY, 1, at(10, 13)),
        ],
    );
    let shots = go(&mut w, vec![attack(ids[0], ids[2])], 100);
    assert!(shots.iter().all(|s| s.target == ids[2]));
    assert_eq!(
        hp(&w, ids[1]),
        1000 - 6,
        "only the first tick, before the order"
    );
    // Stop: idle at once; it looks round again at its next look.
    let mut w = fought();
    let ids = spawn(&mut w, &[(RIFLEMAN, 0, at(10, 10)), (DUMMY, 1, at(12, 10))]);
    assert_eq!(w.order_of(ids[0]), Some(Order::Pursue(ids[1])));
    run_command(&mut w, Command::Stop { ids: vec![ids[0]] });
    w.step();
    assert_eq!(w.order_of(ids[0]), Some(Order::Idle));
    assert_eq!(w.engagement_of(ids[0]), None);
    until(&mut w, 20, |w| {
        w.order_of(ids[0]) == Some(Order::Pursue(ids[1]))
    });
}

#[test]
fn a_player_cannot_give_the_order_a_unit_gives_itself() {
    let mut w = fought();
    let ids = spawn(&mut w, &[(RIFLEMAN, 0, at(10, 10)), (DUMMY, 1, at(30, 30))]);
    run_command(&mut w, order(ids[0], Order::Pursue(ids[1])));
    let report = w.step();
    assert_eq!(report.commands_ignored, 1);
    assert_eq!(w.order_of(ids[0]), Some(Order::Idle));
}

#[test]
fn a_queued_order_starts_when_an_idle_unit_has_done_fighting() {
    let mut w = fought();
    let ids = spawn(&mut w, &[(RIFLEMAN, 0, at(10, 10)), (THIN, 1, at(12, 10))]);
    let (r, thin) = (ids[0], ids[1]);
    assert_eq!(w.order_of(r), Some(Order::Pursue(thin)));
    // The player queues a move while the unit fights: it starts when the fight is over.
    run_command(
        &mut w,
        Command::Order {
            ids: vec![r],
            order: Order::Move(at(20, 20)),
            queued: true,
        },
    );
    until(&mut w, 40, |w| hp(w, thin) == 0);
    w.step();
    assert_eq!(w.order_of(r), Some(Order::Move(at(20, 20))));
}

// ---- return fire -------------------------------------------------------------------------------------

/// A rifleman at (10, 10) of player 0 and a long gun at `x` of player 1 that shoots at it.
fn shot_at_from(x: i32) -> (World, EntityId, EntityId) {
    let mut w = fought();
    let ids = spawn(&mut w, &[(RIFLEMAN, 0, at(10, 10)), (ARTY, 1, at(x, 10))]);
    run_command(&mut w, attack(ids[1], ids[0]));
    (w, ids[0], ids[1])
}

#[test]
fn return_fire_does_not_wait_for_the_cooldown() {
    // A rifleman looks round at tick 0 (nothing there) and not again until tick 10. On tick 3 a rocket
    // thrower is spawned four cells away; it picks the rifleman and fires on the same tick. The rifleman,
    // hit, turns on it that very tick, and fires back at once.
    let mut w = fought();
    let r = spawn(&mut w, &[(RIFLEMAN, 0, at(10, 10))])[0];
    run(&mut w, 2);
    assert_eq!(w.order_of(r), Some(Order::Idle));
    assert!(w.scan_wait_of(r).unwrap() > 0);
    spawn(&mut w, &[(ROCKETEER, 1, at(14, 10))]);
    let s = EntityId::new(1, 0);
    assert_eq!(w.tick().0, 3 + 1);
    assert_eq!(
        hp(&w, r),
        50 - 16,
        "a rocket does 40 percent of 40 to soft armor"
    );
    assert_eq!(w.order_of(r), Some(Order::Pursue(s)));
    let e = w.engagement_of(r).unwrap();
    assert!(!e.following, "it is in reach");
    // It fires on the next tick (the tick of its first look at the attacker would have been 10).
    w.step();
    let mine = shots_by(w.shots(), r);
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].target, s);
    assert_eq!(pos(&w, r), at(10, 10));
}

#[test]
fn an_attacker_beyond_reach_but_within_the_pursuit_is_followed_and_fired_at() {
    // Eight cells: beyond the rifle's four and a half, inside four and a half plus six.
    let (mut w, r, s) = shot_at_from(18);
    w.step();
    let hit_at = w.tick().0;
    assert!(hp(&w, r) < 50, "the first shot hit at tick {hit_at}");
    // It turned on its attacker on the tick it was hit, and follows (the engagement says so).
    assert_eq!(w.order_of(r), Some(Order::Pursue(s)));
    let e = w.engagement_of(r).unwrap();
    assert!(e.following);
    assert_eq!(e.resumes, Order::Idle);
    // It walks until the attacker is in reach (a cell and a half closer: its body is half a cell) and
    // then fires, and it does not walk on.
    let (_, shots) = until(&mut w, 300, |w| !shots_by(w.shots(), r).is_empty());
    let first = shots_by(&shots, r)[0];
    assert_eq!(first.target, s);
    let stood = pos(&w, r);
    assert!(stood.x > at(10, 10).x && stood.x <= at(10, 10).x + Fx::from_int(6));
    assert!((pos(&w, s) - stood).length() <= Fx::from_ratio(9, 2));
    let (_, shots) = until(&mut w, 300, |w| w.order_of(r) == Some(Order::Idle));
    assert_eq!(pos(&w, r), stood, "in reach it stands");
    assert_eq!(hp(&w, s), 0, "the rifleman killed its attacker: {shots:?}");
    // It is where the chase ended, idle: nothing sends it back.
    run(&mut w, 60);
    assert_eq!(pos(&w, r), stood);
}

#[test]
fn an_attacker_beyond_the_pursuit_is_not_followed() {
    // Fifteen cells: beyond four and a half plus six.
    let (mut w, r, s) = shot_at_from(25);
    let shots = run(&mut w, 200);
    assert!(hp(&w, r) < 50);
    assert!(shots_by(&shots, r).is_empty());
    assert_eq!(w.order_of(r), Some(Order::Idle));
    assert_eq!(pos(&w, r), at(10, 10));
    assert_eq!(hp(&w, s), 40);
}

#[test]
fn a_pursuit_of_zero_never_follows() {
    let mut w = fought_with(TargetingConfig::from_cells(10, 0));
    let ids = spawn(&mut w, &[(RIFLEMAN, 0, at(10, 10)), (ARTY, 1, at(18, 10))]);
    run_command(&mut w, attack(ids[1], ids[0]));
    let shots = run(&mut w, 100);
    assert!(hp(&w, ids[0]) < 50);
    assert!(shots_by(&shots, ids[0]).is_empty());
    assert_eq!(pos(&w, ids[0]), at(10, 10));
    // Within reach it still answers.
    let mut w = fought_with(TargetingConfig::from_cells(10, 0));
    let ids = spawn(&mut w, &[(RIFLEMAN, 0, at(10, 10)), (ARTY, 1, at(13, 10))]);
    // (The look at tick 0 has already picked the nearby shooter, which the test is not about.)
    assert_eq!(w.order_of(ids[0]), Some(Order::Pursue(ids[1])));
}

#[test]
fn the_pursuit_is_bounded_a_following_unit_stops_where_the_attacker_gets_away() {
    // The attacker hits once and then runs off at the speed of the follower: the follower cannot close
    // the gap and gives up at six cells from where it began.
    let (mut w, r, s) = shot_at_from(18);
    w.step();
    assert_eq!(w.order_of(r), Some(Order::Pursue(s)));
    run_command(&mut w, mv(s, at(60, 10)));
    let mut farthest = Fx::ZERO;
    let (_, _) = until(&mut w, 400, |w| {
        farthest = farthest.max((pos(w, r) - at(10, 10)).length());
        w.order_of(r) == Some(Order::Idle)
    });
    assert!(
        farthest <= Fx::from_int(6) + Fx::from_ratio(1, 4),
        "it strayed {farthest}"
    );
    assert!(farthest >= Fx::from_int(5), "it did follow: {farthest}");
}

#[test]
fn a_unit_on_hold_fires_back_only_from_where_it_stands() {
    // Out of its reach: it is hit and does nothing.
    let (mut w, r, _) = shot_at_from(18);
    let hold = order(r, Order::HoldPosition);
    run_command(&mut w, hold);
    let shots = run(&mut w, 100);
    assert!(hp(&w, r) < 50);
    assert!(shots_by(&shots, r).is_empty());
    assert_eq!(pos(&w, r), at(10, 10));
    assert_eq!(w.order_of(r), Some(Order::HoldPosition));
    // In reach: it answers at once, without waiting for a look, and does not move.
    let mut w = fought();
    let ids = spawn(
        &mut w,
        &[(RIFLEMAN, 0, at(10, 10)), (ROCKETEER, 1, at(14, 10))],
    );
    let (r, s) = (ids[0], ids[1]);
    // (They both looked at tick 0 and picked each other; stop the rifleman and hold it so that it
    // has to be shot at to answer.)
    run_command(&mut w, order(r, Order::HoldPosition));
    w.step();
    assert_eq!(w.order_of(r), Some(Order::HoldPosition));
    assert_eq!(w.scan_wait_of(r).map(|c| c > 0), Some(true));
    let hits_before = hp(&w, r);
    let (_, shots) = until(&mut w, 40, |w| hp(w, r) < hits_before);
    assert!(shots_by(&shots, s).len() == 1);
    // The tick it was hit it turned on the attacker, from where it stands.
    assert_eq!(w.order_of(r), Some(Order::Pursue(s)));
    assert_eq!(w.engagement_of(r).unwrap().resumes, Order::HoldPosition);
    assert!(!w.engagement_of(r).unwrap().following);
    assert_eq!(pos(&w, r), at(10, 10));
}

#[test]
fn a_guard_follows_an_attacker_and_goes_back_to_its_post() {
    // The long gun hit the rifleman on tick 0 and every 30 ticks; the rifleman is given a guard order
    // on tick 1, so it is the second hit (tick 30) that it answers, as a guard.
    let (mut w, r, s) = shot_at_from(18);
    let post = at(10, 10);
    run_command(&mut w, order(r, Order::Guard(GuardTarget::Point(post))));
    until(&mut w, 100, |w| w.order_of(r) == Some(Order::Pursue(s)));
    assert_eq!(w.tick().0, 31);
    let e = w.engagement_of(r).unwrap();
    assert!(e.following);
    assert_eq!(e.resumes, Order::Guard(GuardTarget::Point(post)));
    // It follows (the attacker is a cell and a half too far), shoots it dead (40 hit points, 6 a
    // shot), and walks back to its post, where a guard stands.
    until(&mut w, 400, |w| hp(w, s) == 0);
    assert!(pos(&w, r).x > post.x + Fx::ONE);
    until(&mut w, 400, |w| {
        w.order_of(r) == Some(Order::Guard(GuardTarget::Point(post)))
            && (pos(w, r) - post).length() <= ARRIVE_TOLERANCE
    });
}

#[test]
fn a_unit_on_attack_move_answers_only_from_where_it_is_and_goes_on() {
    // Hit from beyond its reach it marches on; the attacker is a long gun and keeps firing.
    let mut w = fought();
    let ids = spawn(&mut w, &[(RIFLEMAN, 0, at(10, 10)), (ARTY, 1, at(18, 12))]);
    let (r, s) = (ids[0], ids[1]);
    let goal = at(30, 10);
    run_command(&mut w, order(r, Order::AttackMove(goal)));
    run_command(&mut w, attack(s, r));
    // On the first tick it is hit and (beyond reach, an attack-move does not follow) goes on; it will
    // pass the attacker, and engage it by the ordinary look when it is in reach.
    w.step();
    assert_eq!(w.order_of(r), Some(Order::AttackMove(goal)));
    assert!(w.engagement_of(r).is_none());
    until(&mut w, 400, |w| w.order_of(r) == Some(Order::Pursue(s)));
    assert!(pos(&w, r).x > at(10, 10).x);
}

#[test]
fn when_several_enemies_hurt_a_unit_in_one_tick_it_answers_the_best_by_the_key_in_every_spawn_order()
 {
    // Two long guns, five cells from the rifleman (out of its reach of four and a half, inside the
    // pursuit), pick it on the tick they are spawned and hit it together. It answers the one with the
    // smaller y, whichever was spawned first.
    for flip in [false, true] {
        let mut w = fought();
        let mut list = vec![(ARTY, 1, at(10, 5)), (ARTY, 1, at(15, 10))];
        if flip {
            list.reverse();
        }
        list.insert(0, (RIFLEMAN, 0, at(10, 10)));
        let ids = spawn(&mut w, &list);
        assert_eq!(hp(&w, ids[0]), 50 - 5 - 5, "flip {flip}");
        let Some(Order::Pursue(chosen)) = w.order_of(ids[0]) else {
            panic!("it did not answer (flip {flip})");
        };
        assert_eq!(pos(&w, chosen), at(10, 5), "flip {flip}");
        assert!(w.engagement_of(ids[0]).unwrap().following);
    }
}

// ---- zero hit points ---------------------------------------------------------------------------------

#[test]
fn a_unit_at_zero_hit_points_is_nobodys_target_picks_nothing_and_stays_where_it_fell() {
    let mut w = fought();
    let ids = spawn(
        &mut w,
        &[
            (GLASS, 0, at(10, 10)),
            (ROCKETEER, 1, at(13, 10)),
            (RIFLEMAN, 1, at(30, 30)),
        ],
    );
    let (glass, rocketeer, far) = (ids[0], ids[1], ids[2]);
    // Both picked each other on tick 0: the rocket (16 on soft armor) takes the glass unit's seven hit
    // points on that same tick, and the glass unit still fired on it.
    assert_eq!(hp(&w, glass), 0);
    assert_eq!(w.shots().len(), 2, "{:?}", w.shots());
    // It stands where it fell, on hold, and takes no orders any more: the move is not carried out.
    run_command(&mut w, mv(glass, at(40, 40)));
    assert_eq!(w.step().commands_ignored, 1);
    let shots = run(&mut w, 60);
    assert_eq!(w.order_of(glass), Some(Order::HoldPosition));
    assert_eq!(pos(&w, glass), at(10, 10));
    assert!(
        shots_by(&shots, glass).is_empty(),
        "a unit at zero fires no more"
    );
    // The rocket thrower does not pick it again, and the far rifleman does not either.
    assert_eq!(
        hp(&w, rocketeer),
        50 - 6,
        "hit once, by the shot it fired on that tick"
    );
    assert_eq!(w.engagement_of(rocketeer), None);
    assert_eq!(w.engagement_of(far), None);
    assert!(w.is_alive(glass), "nothing is removed before WP5.5");
    // A unit that is shot to zero while it walks stops where it fell.
    let mut w = fought();
    let ids = spawn(&mut w, &[(GLASS, 0, at(10, 10)), (ARTY, 1, at(30, 10))]);
    run_command(&mut w, mv(ids[0], at(50, 10)));
    run_command(&mut w, attack(ids[1], ids[0]));
    let (_, _) = until(&mut w, 400, |w| hp(w, ids[0]) == 0);
    let fell = pos(&w, ids[0]);
    assert!(fell.x > at(10, 10).x, "it walked: {fell:?}");
    run(&mut w, 100);
    assert_eq!(w.order_of(ids[0]), Some(Order::HoldPosition));
    assert_eq!(pos(&w, ids[0]), fell);
}

// ---- aiming ------------------------------------------------------------------------------------------

#[test]
fn a_unit_with_a_narrow_arc_turns_to_its_target_before_it_fires() {
    let mut w = fought();
    let ids = spawn(&mut w, &[(GRUNT, 0, at(10, 10)), (DUMMY, 1, at(10, 13))]);
    let (g, d) = (ids[0], ids[1]);
    assert_eq!(w.order_of(g), Some(Order::Pursue(d)));
    let (_, shots) = until(&mut w, 100, |w| !w.shots().is_empty());
    assert_eq!(shots.len(), 1);
    // North of it: the bearing is 64 steps (a quarter turn) from east; at 16 steps a tick it was on
    // target by then, within 8 steps either way, and every shot after is as well.
    let facing = w.get(g).unwrap().facing;
    let wanted = (pos(&w, d) - pos(&w, g)).facing().unwrap();
    assert!(facing.abs_diff(wanted) <= 8, "{facing:?} {wanted:?}");
    assert_eq!(pos(&w, g), at(10, 10));
}

#[test]
fn a_tank_aims_its_turret_at_what_it_picked() {
    let mut w = fought();
    let ids = spawn(&mut w, &[(TANK, 0, at(10, 10)), (DUMMY, 1, at(13, 10))]);
    let (t, d) = (ids[0], ids[1]);
    assert_eq!(w.order_of(t), Some(Order::Pursue(d)));
    let (_, shots) = until(&mut w, 100, |w| !w.shots().is_empty());
    let turret = w.turret_of(t).unwrap();
    let wanted = (pos(&w, d) - pos(&w, t)).facing().unwrap();
    assert!(turret.abs_diff(wanted) <= 8, "{turret:?} {wanted:?}");
    assert_eq!(shots[0].attacker, t);
    // Tank armor, rockets: the tank cannon (30 rocket) does 12 to a soft target.
    assert_eq!(hp(&w, d), 1000 - 12);
}

// ---- the end state ----------------------------------------------------------------------------------

#[test]
fn different_settings_give_different_checksums_from_the_first_tick() {
    let play = |config: TargetingConfig| {
        let mut w = fought_with(config);
        spawn(&mut w, &[(RIFLEMAN, 0, at(10, 10))]);
        w.checksum()
    };
    assert_ne!(
        play(TargetingConfig::from_cells(10, 6)),
        play(TargetingConfig::from_cells(11, 6))
    );
    assert_ne!(
        play(TargetingConfig::from_cells(10, 6)),
        play(TargetingConfig::from_cells(10, 7))
    );
    assert_eq!(
        play(TargetingConfig::from_cells(10, 6)),
        play(TargetingConfig::from_cells(10, 6))
    );
    // And a world with settings differs from one without, once it has a unit that looks round.
    let mut w = armed();
    spawn(&mut w, &[(RIFLEMAN, 0, at(10, 10))]);
    assert_ne!(w.checksum(), play(TargetingConfig::DEFAULT));
}

// ---- the order of the slots does not decide a fight -------------------------------------------------

/// What a skirmish came to, by the place each unit started at (not by its slot).
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    /// Hit points at the end, per starting place.
    hp: Vec<u32>,
    /// Where each unit ended, per starting place.
    end: Vec<(i64, i64)>,
    /// The order each ended on, as text, per starting place.
    order: Vec<String>,
    /// Shots over the run: (tick, shooter's place, target's place), sorted.
    shots: Vec<(u64, usize, usize)>,
}

/// A fight between kinds at fixed places, spawned in `order` (so the slot of each differs between runs),
/// with `commands(ids)` given on the tick after: its outcome and the checksum of the end.
fn skirmish(
    cast: &[Spawn],
    order: &[usize],
    commands: &dyn Fn(&[EntityId]) -> Vec<Command>,
    ticks: u32,
) -> (Outcome, u64) {
    let mut w = fought();
    let list: Vec<Spawn> = order.iter().map(|&p| cast[p]).collect();
    let slots = spawn(&mut w, &list);
    // ids[p] is the entity that started at place p.
    let mut ids = vec![EntityId::new(0, 0); cast.len()];
    for (k, &p) in order.iter().enumerate() {
        ids[p] = slots[k];
    }
    let place_of = |id: EntityId| ids.iter().position(|&i| i == id).unwrap();
    let mut shots: Vec<(u64, usize, usize)> = w
        .shots()
        .iter()
        .map(|s| (s.tick.0, place_of(s.attacker), place_of(s.target)))
        .collect();
    for c in commands(&ids) {
        run_command(&mut w, c);
    }
    for _ in 0..ticks {
        w.step();
        shots.extend(
            w.shots()
                .iter()
                .map(|s| (s.tick.0, place_of(s.attacker), place_of(s.target))),
        );
    }
    shots.sort_unstable();
    let outcome = Outcome {
        hp: ids.iter().map(|&i| hp(&w, i)).collect(),
        end: ids
            .iter()
            .map(|&i| (pos(&w, i).x.raw(), pos(&w, i).y.raw()))
            .collect(),
        order: ids
            .iter()
            .map(|&i| match w.order_of(i).unwrap() {
                Order::Pursue(t) => format!("pursue {}", place_of(t)),
                other => format!("{other:?}"),
            })
            .collect(),
        shots,
    };
    (outcome, w.checksum())
}

fn permutations(n: usize) -> Vec<Vec<usize>> {
    if n == 1 {
        return vec![vec![0]];
    }
    let mut out = Vec::new();
    for rest in permutations(n - 1) {
        for at in 0..=rest.len() {
            let mut p = rest.clone();
            p.insert(at, n - 1);
            out.push(p);
        }
    }
    out
}

/// Plays the skirmish in every spawn order and expects one outcome.
fn in_every_order(
    cast: &[Spawn],
    commands: &dyn Fn(&[EntityId]) -> Vec<Command>,
    ticks: u32,
) -> Outcome {
    let mut seen: Option<Outcome> = None;
    for order in permutations(cast.len()) {
        let (outcome, checksum) = skirmish(cast, &order, commands, ticks);
        assert_eq!(
            skirmish(cast, &order, commands, ticks).1,
            checksum,
            "{order:?} repeats"
        );
        match &seen {
            Some(first) => assert_eq!(&outcome, first, "spawn order {order:?}"),
            None => seen = Some(outcome),
        }
    }
    seen.unwrap()
}

#[test]
fn idle_units_that_pick_each_other_fight_the_same_in_every_spawn_order() {
    // Two riflemen and a rocket thrower, all within each other's reach, all with distinct distances. The
    // rocket thrower shoots first (40 damage against soft armor at 40 percent: 16).
    let cast = [
        (RIFLEMAN, 0, at(10, 10)),
        (RIFLEMAN, 1, at(13, 10)),
        (ROCKETEER, 1, at(11, 14)),
        (RIFLEMAN, 0, at(8, 13)),
    ];
    let o = in_every_order(&cast, &|_| vec![], 60);
    // Everybody picked and fired from the first tick.
    assert!(o.shots.iter().any(|s| s.0 == 0));
    assert!(o.hp.iter().any(|&h| h < 50));
}

#[test]
fn units_on_attack_move_that_meet_fight_the_same_in_every_spawn_order() {
    // Two on each side march at each other along different rows; they meet, stop, fight and go on.
    let cast = [
        (RIFLEMAN, 0, at(8, 20)),
        (ROCKETEER, 0, at(8, 24)),
        (RIFLEMAN, 1, at(30, 21)),
        (ROCKETEER, 1, at(30, 25)),
    ];
    let go_on = |ids: &[EntityId]| {
        vec![
            order(ids[0], Order::AttackMove(at(30, 21))),
            order(ids[1], Order::AttackMove(at(30, 25))),
            order(ids[2], Order::AttackMove(at(8, 20))),
            order(ids[3], Order::AttackMove(at(8, 24))),
        ]
    };
    let o = in_every_order(&cast, &go_on, 400);
    assert!(!o.shots.is_empty());
    assert!(o.hp.iter().any(|&h| h < 50), "{o:?}");
}

#[test]
fn units_that_return_fire_do_the_same_in_every_spawn_order() {
    // A rifleman and a guard shot at by two long guns from different places; both follow, fight and end.
    let cast = [
        (RIFLEMAN, 0, at(10, 10)),
        (RIFLEMAN, 0, at(10, 14)),
        (ARTY, 1, at(18, 9)),
        (ARTY, 1, at(19, 15)),
    ];
    let shoot = |ids: &[EntityId]| {
        vec![
            attack(ids[2], ids[0]),
            attack(ids[3], ids[1]),
            order(ids[1], Order::Guard(GuardTarget::Point(at(10, 14)))),
        ]
    };
    let o = in_every_order(&cast, &shoot, 300);
    assert!(o.hp[2] < 40 || o.hp[3] < 40, "{o:?}");
}

/// Where every unit stands after each tick, per starting place: a rifleman that is shot at by a long gun
/// beyond its reach follows it, and the long gun walks away while it is followed. Spawned in `order`.
fn chase_trace(order: &[usize]) -> Vec<Vec<(i64, i64)>> {
    let cast = [
        (RIFLEMAN, 0, at(10, 10)),
        (ARTY, 1, at(18, 10)),
        (RIFLEMAN, 0, at(10, 16)),
    ];
    let mut w = fought();
    let list: Vec<Spawn> = order.iter().map(|&p| cast[p]).collect();
    let slots = spawn(&mut w, &list);
    let mut ids = vec![EntityId::new(0, 0); cast.len()];
    for (k, &p) in order.iter().enumerate() {
        ids[p] = slots[k];
    }
    let mut trace = Vec::new();
    let mut record = |w: &World| {
        trace.push(
            ids.iter()
                .map(|&i| (pos(w, i).x.raw(), pos(w, i).y.raw()))
                .collect::<Vec<_>>(),
        );
    };
    run_command(&mut w, attack(ids[1], ids[0]));
    record(&w);
    w.step();
    record(&w);
    assert_eq!(w.order_of(ids[0]), Some(Order::Pursue(ids[1])), "{order:?}");
    // The long gun walks off along the row while the rifleman follows it.
    run_command(&mut w, mv(ids[1], at(40, 13)));
    record(&w);
    for _ in 0..120 {
        w.step();
        record(&w);
    }
    trace
}

#[test]
fn a_follower_walks_to_where_a_moving_target_stood_at_the_start_of_the_tick_in_every_spawn_order() {
    // The follower aims at the snapshot of the target's position; were it to read the live position, a
    // target in a lower slot would already have stepped this tick, and the paths would part.
    let first = chase_trace(&[0, 1, 2]);
    assert!(
        first.last().unwrap()[0].0 > at(12, 10).x.raw(),
        "the rifleman did follow"
    );
    for order in permutations(3) {
        let trace = chase_trace(&order);
        for (tick, (a, b)) in first.iter().zip(&trace).enumerate() {
            assert_eq!(a, b, "spawn order {order:?} parts at step {tick}");
        }
    }
}

/// A long gun hits `r` from eight cells, steps back seven cells while `r` follows, and as soon as `r`
/// has let go hits it again from where it now is; six times. Returns how far `r` got from `from`.
fn lure(w: &mut World, r: EntityId, s: EntityId, from: Vec2) -> Fx {
    let mut farthest = Fx::ZERO;
    for round in 0..6 {
        run_command(w, attack(s, r));
        let followed = (0..80).any(|_| {
            w.step();
            farthest = farthest.max((pos(w, r) - from).length());
            matches!(w.order_of(r), Some(Order::Pursue(t)) if t == s)
        });
        if !followed {
            break;
        }
        let back = pos(w, s) + Vec2::from_ints(7, 0);
        run_command(w, mv(s, back));
        for _ in 0..200 {
            w.step();
            farthest = farthest.max((pos(w, r) - from).length());
            if !matches!(w.order_of(r), Some(Order::Pursue(_))) && round < 5 {
                break;
            }
        }
    }
    farthest
}

#[test]
fn an_idle_unit_cannot_be_lured_away_chase_by_chase() {
    let mut w = fought();
    let ids = spawn(&mut w, &[(RIFLEMAN, 0, at(6, 10)), (ARTY, 1, at(14, 10))]);
    let farthest = lure(&mut w, ids[0], ids[1], at(6, 10));
    assert!(
        farthest <= Fx::from_int(6) + Fx::from_ratio(1, 4),
        "it strayed {farthest}"
    );
    assert!(farthest >= Fx::from_int(5), "it did follow: {farthest}");
}

#[test]
fn a_guard_measures_its_pursuit_from_its_post() {
    let mut w = fought();
    let post = at(6, 10);
    let ids = spawn(&mut w, &[(RIFLEMAN, 0, post), (ARTY, 1, at(14, 10))]);
    run_command(
        &mut w,
        order(ids[0], Order::Guard(GuardTarget::Point(post))),
    );
    let farthest = lure(&mut w, ids[0], ids[1], post);
    assert!(
        farthest <= Fx::from_int(6) + Fx::from_ratio(1, 4),
        "it strayed {farthest}"
    );
    assert!(farthest >= Fx::from_int(5), "it did follow: {farthest}");
}

#[test]
fn after_a_rest_an_idle_unit_measures_a_new_chase_from_where_it_stands() {
    // The same lure, but the long gun waits longer than pursuit_reset_ticks between rounds: each chase
    // is then a new one.
    let mut w = fought_with(TargetingConfig::DEFAULT.with_pursuit_reset(20));
    let ids = spawn(&mut w, &[(RIFLEMAN, 0, at(6, 10)), (ARTY, 1, at(14, 10))]);
    let (r, s) = (ids[0], ids[1]);
    let farthest = lure(&mut w, r, s, at(6, 10));
    assert!(farthest > Fx::from_int(7), "it went on: {farthest}");
}

#[test]
fn a_unit_shot_at_from_inside_its_minimum_range_does_not_follow_and_stays_at_rest() {
    // A mortar (reach 10, not nearer than 3) is hit by a long gun two cells away. It cannot fire at it,
    // and walking will not help: it does not take the attacker on (before, it took a `Pursue` it kept
    // for ever without a shot).
    let mut w = fought();
    let ids = spawn(&mut w, &[(MORTAR, 0, at(10, 10)), (ARTY, 1, at(12, 10))]);
    let (m, s) = (ids[0], ids[1]);
    let mut shots = Vec::new();
    for _ in 0..100 {
        w.step();
        shots.extend_from_slice(w.shots());
        assert_eq!(w.order_of(m), Some(Order::Idle), "tick {}", w.tick().0);
    }
    assert!(hp(&w, m) < 100, "it was hit");
    assert!(shots_by(&shots, s).len() >= 3);
    assert!(shots_by(&shots, m).is_empty());
}
