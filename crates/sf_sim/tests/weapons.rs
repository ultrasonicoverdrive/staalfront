//! Weapons (WP5.1), through the public API: the table and what it refuses, the reload and the
//! burst that count down in the unit, the placeholder shot of an attack order (reach, minimum
//! range, aim, a victim with no hit points left), and what the checksum covers.
//!
//! The numbers are small and invented so a test can follow them tick by tick; the real set is
//! `assets/config/weapon_defs.ron`, which `tools/sf_headless/tests/weapons_reload.rs` runs.

use sf_math::{Facing, Fx, Vec2};
use sf_sim::{
    Command, ConfigError, EntityId, GuardTarget, Kind, Loadout, Mount, Order, PlayerId,
    ProjectileKind, Shot, UnitStats, UnitTable, WeaponStats, WeaponTable, WeaponsError, World,
    WorldConfig, per_tick,
};

const MAP: u32 = 64;

// A rifleman with a hull-mounted gun, a tank with a twin gun on a turret, a mortar with a
// minimum range, three targets that cannot shoot (a sturdy one, a weak one, one that dies of a
// single shell) and an object that cannot be damaged.
const GUNNER: Kind = Kind(1);
const TANK: Kind = Kind(2);
const MORTAR: Kind = Kind(3);
const DUMMY: Kind = Kind(4);
const THIN: Kind = Kind(5);
const ROCK: Kind = Kind(6);
const WEAK: Kind = Kind(7);
const FRAIL: Kind = Kind(8); // seven hit points, and a gun that does seven, all round

const GUN: u16 = 10; // reload 5, one shot of 7, range 4
const TWIN: u16 = 11; // reload 8, two shots of 10 three ticks apart, range 5, on the turret
const ZAP: u16 = 13; // reload 5, one shot of 7, range 4, all round
const SHELL: u16 = 12; // reload 12, one shot of 20, range 10, not nearer than 3, on the turret

fn unit(kind: Kind, hp: u32, range: i32, turret: u8) -> UnitStats {
    UnitStats {
        kind,
        hp,
        movement_class: Some(0),
        speed_per_tick: per_tick(Fx::from_int(2)),
        turn_per_tick: 16,
        size: Fx::ONE,
        footprint: None,
        weapon_range: Fx::from_int(range),
        turret_per_tick: turret,
    }
}

fn units() -> UnitTable {
    let rock = UnitStats {
        movement_class: None,
        speed_per_tick: Fx::ZERO,
        turn_per_tick: 0,
        ..unit(ROCK, 0, 0, 0)
    };
    UnitTable::new(vec![
        unit(GUNNER, 50, 4, 0),
        unit(TANK, 400, 5, 12),
        unit(MORTAR, 100, 10, 6),
        unit(DUMMY, 1000, 0, 0),
        unit(THIN, 15, 0, 0),
        unit(WEAK, 10, 0, 0),
        unit(FRAIL, 7, 4, 0),
        rock,
    ])
    .unwrap()
}

fn weapon(kind: u16, range: i32, reload: u32, damage: u32) -> WeaponStats {
    WeaponStats {
        kind,
        range: Fx::from_int(range),
        min_range: Fx::ZERO,
        reload_ticks: reload,
        burst: 1,
        burst_delay_ticks: 0,
        damage,
        damage_type: 0,
        projectile: ProjectileKind::Instant,
        projectile_speed_per_tick: Fx::ZERO,
        flight_height: Fx::ZERO,
        guide_turn_per_tick: 0,
        splash_radius: Fx::ZERO,
        splash_inner: Fx::ZERO,
        friendly_fire: true,
        mount: Mount::Hull,
        fire_arc: 8,
    }
}

fn weapons() -> Vec<WeaponStats> {
    vec![
        weapon(GUN, 4, 5, 7),
        WeaponStats {
            burst: 2,
            burst_delay_ticks: 3,
            mount: Mount::Turret,
            ..weapon(TWIN, 5, 8, 10)
        },
        WeaponStats {
            fire_arc: 128,
            ..weapon(ZAP, 4, 5, 7)
        },
        WeaponStats {
            min_range: Fx::from_int(3),
            mount: Mount::Turret,
            projectile: ProjectileKind::Arc,
            projectile_speed_per_tick: Fx::HALF,
            flight_height: Fx::from_int(2),
            ..weapon(SHELL, 10, 12, 20)
        },
    ]
}

fn loadout(unit: Kind, weapons: &[u16]) -> Loadout {
    Loadout {
        unit,
        weapons: weapons.to_vec(),
    }
}

fn loadouts() -> Vec<Loadout> {
    vec![
        loadout(GUNNER, &[GUN]),
        loadout(TANK, &[TWIN]),
        loadout(MORTAR, &[SHELL]),
        loadout(FRAIL, &[ZAP]),
    ]
}

fn table() -> WeaponTable {
    WeaponTable::new(weapons(), loadouts()).unwrap()
}

fn base() -> World {
    World::new(WorldConfig::new(3).with_map_size(MAP, MAP))
        .unwrap()
        .with_units(units())
        .unwrap()
}

fn armed() -> World {
    base().with_weapons(table()).unwrap()
}

fn at(x: i32, y: i32) -> Vec2 {
    Vec2::from_ints(x, y)
}

/// Steps one tick with `commands` (all by player 0) and returns the shots of that tick.
fn tick_with(w: &mut World, commands: Vec<Command>) -> Vec<Shot> {
    let tick = w.tick();
    for c in commands {
        w.schedule(tick, PlayerId(0), c).unwrap();
    }
    let report = w.step();
    assert_eq!(report.shots as usize, w.shots().len());
    w.shots().to_vec()
}

fn spawn(w: &mut World, kind: Kind, pos: Vec2) -> EntityId {
    let id = EntityId::new(w.slot_count() as u32, 0);
    tick_with(
        w,
        vec![Command::SpawnUnit {
            kind,
            pos,
            owner: PlayerId(0),
        }],
    );
    id
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

/// Steps `n` ticks and collects every shot.
fn run(w: &mut World, n: u32) -> Vec<Shot> {
    (0..n).flat_map(|_| tick_with(w, vec![])).collect()
}

/// Gives the commands on one tick, then runs `n - 1` more, and collects every shot.
fn go(w: &mut World, commands: Vec<Command>, n: u32) -> Vec<Shot> {
    let mut shots = tick_with(w, commands);
    shots.extend(run(w, n - 1));
    shots
}

fn ticks(shots: &[Shot]) -> Vec<u64> {
    shots.iter().map(|s| s.tick.0).collect()
}

fn gaps(ticks: &[u64]) -> Vec<u64> {
    ticks.windows(2).map(|p| p[1] - p[0]).collect()
}

// ---- the table ------------------------------------------------------------------------------

#[test]
fn a_good_table_is_accepted_in_any_order() {
    let a = table();
    let mut w = weapons();
    w.reverse();
    let mut l = loadouts();
    l.reverse();
    let b = WeaponTable::new(w, l).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.digest(), b.digest());
    assert_ne!(a.digest(), 0);
    assert_eq!(a.loadout(TANK), [TWIN]);
    assert_eq!(a.loadout(DUMMY), [] as [u16; 0]);
    assert_eq!(a.weapon(TWIN).unwrap().burst, 2);
    assert!(a.weapon(99).is_none());
    // Any number changes the digest.
    let mut c = weapons();
    c[0].damage += 1;
    assert_ne!(
        WeaponTable::new(c, loadouts()).unwrap().digest(),
        a.digest()
    );
    let empty = WeaponTable::empty();
    assert!(empty.is_empty());
    assert_eq!(empty.digest(), 0);
}

fn refused(w: Vec<WeaponStats>, l: Vec<Loadout>) -> WeaponsError {
    WeaponTable::new(w, l).unwrap_err()
}

/// The error for one weapon changed by `f`.
fn bad_weapon(f: impl Fn(&mut WeaponStats)) -> WeaponsError {
    let mut w = weapon(GUN, 4, 5, 7);
    f(&mut w);
    refused(vec![w], vec![])
}

#[test]
fn a_bad_table_is_refused_with_the_reason() {
    assert_eq!(
        refused(vec![weapon(GUN, 4, 5, 7), weapon(GUN, 3, 5, 7)], vec![]),
        WeaponsError::DuplicateWeapon(GUN)
    );
    assert_eq!(
        bad_weapon(|w| w.range = Fx::ZERO),
        WeaponsError::BadRange(GUN)
    );
    assert_eq!(
        bad_weapon(|w| w.min_range = Fx::from_int(-1)),
        WeaponsError::BadRange(GUN)
    );
    assert_eq!(
        bad_weapon(|w| w.min_range = Fx::from_int(4)),
        WeaponsError::BadRange(GUN)
    );
    assert_eq!(
        bad_weapon(|w| w.reload_ticks = 0),
        WeaponsError::NoReload(GUN)
    );
    // A burst needs shots, a delay exactly when it has more than one, and room in the reload.
    assert_eq!(bad_weapon(|w| w.burst = 0), WeaponsError::BadBurst(GUN));
    assert_eq!(bad_weapon(|w| w.burst = 2), WeaponsError::BadBurst(GUN));
    assert_eq!(
        bad_weapon(|w| w.burst_delay_ticks = 2),
        WeaponsError::BadBurst(GUN)
    );
    assert_eq!(
        bad_weapon(|w| {
            w.burst = 3;
            w.burst_delay_ticks = 3; // six ticks of burst in a reload of five
        }),
        WeaponsError::BadBurst(GUN)
    );
    assert_eq!(
        bad_weapon(|w| {
            w.burst = 2;
            w.burst_delay_ticks = 5; // exactly the reload: no rest before the next burst
        }),
        WeaponsError::BadBurst(GUN)
    );
    assert_eq!(bad_weapon(|w| w.fire_arc = 129), WeaponsError::BadArc(GUN));
    // The loadouts.
    let w = vec![weapon(GUN, 4, 5, 7)];
    assert_eq!(
        refused(
            w.clone(),
            vec![loadout(GUNNER, &[GUN]), loadout(GUNNER, &[GUN])]
        ),
        WeaponsError::DuplicateLoadout(GUNNER)
    );
    assert_eq!(
        refused(w.clone(), vec![loadout(GUNNER, &[])]),
        WeaponsError::BadLoadout(GUNNER)
    );
    assert_eq!(
        refused(w.clone(), vec![loadout(GUNNER, &[GUN; 5])]),
        WeaponsError::BadLoadout(GUNNER)
    );
    assert_eq!(
        refused(w, vec![loadout(GUNNER, &[GUN, 77])]),
        WeaponsError::UnknownWeapon {
            unit: GUNNER,
            weapon: 77
        }
    );
    // Every refusal has words.
    assert!(!WeaponsError::BadArc(1).to_string().is_empty());
}

#[test]
fn the_table_has_to_fit_the_unit_table() {
    let fit = |w: Vec<WeaponStats>, l: Vec<Loadout>| {
        base()
            .with_weapons(WeaponTable::new(w, l).unwrap())
            .map(|_| ())
    };
    // A loadout for a kind the unit table does not have.
    assert_eq!(
        fit(weapons(), vec![loadout(Kind(99), &[GUN])]),
        Err(ConfigError::Weapons(WeaponsError::UnknownUnit(Kind(99))))
    );
    // The first weapon of a kind has the range its stats call the reach of an attack order.
    assert_eq!(
        fit(weapons(), vec![loadout(GUNNER, &[TWIN])]),
        Err(ConfigError::Weapons(WeaponsError::ReachMismatch(GUNNER)))
    );
    // A kind with a reach and no weapon at all is as much a mismatch.
    assert_eq!(
        fit(weapons(), vec![loadout(GUNNER, &[GUN])]),
        Err(ConfigError::Weapons(WeaponsError::ReachMismatch(TANK)))
    );
    // A turret mount needs a turret: the gunner has none.
    let mut turret_gun = weapons();
    turret_gun[0].mount = Mount::Turret;
    assert_eq!(
        fit(turret_gun, loadouts()),
        Err(ConfigError::Weapons(WeaponsError::TurretWithoutTurret {
            unit: GUNNER,
            weapon: GUN
        }))
    );
    // The good table fits, and only before the world has started.
    assert!(armed().weapon_table().weapon(SHELL).is_some());
    let mut started = base();
    started.step();
    assert_eq!(
        started.with_weapons(table()).err(),
        Some(ConfigError::WeaponsAfterStart)
    );
    assert!(!ConfigError::WeaponsAfterStart.to_string().is_empty());
    assert!(
        !ConfigError::Weapons(WeaponsError::UnknownUnit(Kind(1)))
            .to_string()
            .is_empty()
    );
}

// ---- the reload ------------------------------------------------------------------------------

/// A gunner three cells from a dummy, east of it: in reach, and already facing the dummy.
fn duel() -> (World, EntityId, EntityId) {
    let mut w = armed();
    let gunner = spawn(&mut w, GUNNER, at(10, 10));
    let dummy = spawn(&mut w, DUMMY, at(13, 10));
    (w, gunner, dummy)
}

#[test]
fn a_weapon_fires_once_per_reload() {
    let (mut w, gunner, dummy) = duel();
    let shots = go(&mut w, vec![attack(gunner, dummy)], 100);
    let t = ticks(&shots);
    assert!(t.len() >= 19, "{t:?}");
    assert!(gaps(&t).iter().all(|&g| g == 5), "{t:?}");
    for s in &shots {
        assert_eq!(
            (s.attacker, s.target, s.weapon, s.damage),
            (gunner, dummy, GUN, 7)
        );
        assert_eq!(s.slot, 0);
    }
    // The hit points are the start less every hit, and each shot says what was left.
    for (k, s) in shots.iter().enumerate() {
        assert_eq!(s.hp_after, 1000 - 7 * (k as u32 + 1));
    }
    assert_eq!(shots.last().unwrap().hp_after, w.hp_of(dummy).unwrap());
    // The gunner is ready again exactly one reload after its last shot.
    let last = shots.last().unwrap().tick.0;
    let wait = u64::from(w.weapon_wait(gunner, 0).unwrap());
    assert_eq!(w.tick().0 - 1 + wait, last + 5);
}

#[test]
fn it_walks_into_reach_and_fires_from_there() {
    let mut w = armed();
    let gunner = spawn(&mut w, GUNNER, at(2, 10));
    let dummy = spawn(&mut w, DUMMY, at(30, 10));
    tick_with(&mut w, vec![attack(gunner, dummy)]);
    let mut first = None;
    for _ in 0..400 {
        if let Some(shot) = tick_with(&mut w, vec![]).first() {
            first = Some(*shot);
            break;
        }
    }
    let shot = first.expect("it fired");
    // At the tick of the first shot the gunner is within its range of the dummy's edge (four
    // cells and the dummy's radius of half a cell), and has not gone further than it had to.
    let gap = (w.get(dummy).unwrap().pos - w.get(gunner).unwrap().pos).length();
    assert!(gap <= Fx::from_ratio(9, 2), "{gap:?}");
    assert!(gap > Fx::from_int(4), "{gap:?}");
    assert!(shot.tick.0 > 200, "{shot:?}");
}

#[test]
fn the_reload_is_not_reset_by_a_new_order() {
    // Two identical duels. In one the gunner is stopped right after a shot and told to attack
    // again at once; its shots must stay where they were.
    let play = |interrupt: bool| {
        let (mut w, gunner, dummy) = duel();
        tick_with(&mut w, vec![attack(gunner, dummy)]);
        while tick_with(&mut w, vec![]).is_empty() {}
        let mut shots = Vec::new();
        if interrupt {
            shots.extend(tick_with(&mut w, vec![Command::Stop { ids: vec![gunner] }]));
            shots.extend(tick_with(&mut w, vec![attack(gunner, dummy)]));
            shots.extend(run(&mut w, 38));
        } else {
            shots.extend(run(&mut w, 40));
        }
        ticks(&shots)
    };
    let (control, interrupted) = (play(false), play(true));
    assert_eq!(control, interrupted);
    assert_eq!(control.len(), 8);
}

#[test]
fn a_unit_fires_only_on_an_attack_order() {
    let (mut w, gunner, _dummy) = duel();
    // Idle, on guard and on attack-move next to a target in reach: nothing is fired (choosing
    // targets is WP5.4).
    assert!(run(&mut w, 30).is_empty());
    let on_guard = order(gunner, Order::Guard(GuardTarget::Point(at(10, 10))));
    assert!(go(&mut w, vec![on_guard], 30).is_empty());
    let on_the_move = order(gunner, Order::AttackMove(at(10, 11)));
    assert!(go(&mut w, vec![on_the_move], 30).is_empty());
}

#[test]
fn a_victim_that_cannot_be_hurt_is_not_shot_at() {
    let mut w = armed();
    let gunner = spawn(&mut w, GUNNER, at(10, 10));
    let rock = spawn(&mut w, ROCK, at(12, 10));
    assert_eq!(w.hp_of(rock), Some(0));
    assert!(go(&mut w, vec![attack(gunner, rock)], 60).is_empty());
    assert_eq!(w.hp_of(rock), Some(0));
}

#[test]
fn a_victim_at_zero_is_left_alone_and_the_order_stands() {
    let mut w = armed();
    let gunner = spawn(&mut w, GUNNER, at(10, 10));
    let thin = spawn(&mut w, THIN, at(12, 10)); // 15 hit points: 7, 7, and a third that takes 1
    let shots = go(&mut w, vec![attack(gunner, thin)], 200);
    assert_eq!(
        shots.iter().map(|s| s.hp_after).collect::<Vec<_>>(),
        [8, 1, 0]
    );
    assert_eq!(w.hp_of(thin), Some(0));
    assert_eq!(w.order_of(gunner), Some(Order::Attack(thin)));
    assert!(w.is_alive(thin), "nothing is removed before WP5.5");
}

#[test]
fn a_unit_that_has_been_shot_down_fires_no_more() {
    let mut w = armed();
    let doomed = spawn(&mut w, GUNNER, at(10, 10));
    let dummy = spawn(&mut w, DUMMY, at(12, 10));
    let killer = spawn(&mut w, GUNNER, at(10, 12));
    let shots = go(
        &mut w,
        vec![attack(doomed, dummy), attack(killer, doomed)],
        120,
    );
    let down = shots
        .iter()
        .find(|s| s.target == doomed && s.hp_after == 0)
        .expect("fifty hit points are eight shots of seven")
        .tick;
    assert_eq!(w.hp_of(doomed), Some(0));
    let late = shots
        .iter()
        .filter(|s| s.attacker == doomed && s.tick > down);
    assert_eq!(late.count(), 0);
    // It did fire while it could.
    assert!(shots.iter().any(|s| s.attacker == doomed && s.tick <= down));
    assert_eq!(w.order_of(doomed), Some(Order::Attack(dummy)));
}

// ---- the burst -------------------------------------------------------------------------------

/// A tank four cells west of a target of the given kind; both face east, so the turret is on it.
fn tank_duel(target: Kind) -> (World, EntityId, EntityId) {
    let mut w = armed();
    let tank = spawn(&mut w, TANK, at(10, 10));
    let victim = spawn(&mut w, target, at(14, 10));
    (w, tank, victim)
}

#[test]
fn a_burst_fires_its_shots_a_delay_apart_and_the_reload_counts_from_the_first() {
    let (mut w, tank, dummy) = tank_duel(DUMMY);
    let shots = go(&mut w, vec![attack(tank, dummy)], 80);
    let t = ticks(&shots);
    assert!(t.len() >= 18, "{t:?}");
    // Three ticks inside a burst, then the rest of the eight of the reload.
    let want: Vec<u64> = (0..t.len() - 1)
        .map(|k| if k % 2 == 0 { 3 } else { 5 })
        .collect();
    assert_eq!(gaps(&t), want, "{t:?}");
    assert!(shots.iter().all(|s| s.damage == 10 && s.weapon == TWIN));
}

#[test]
fn a_burst_counts_down_in_the_unit() {
    let (mut w, tank, dummy) = tank_duel(DUMMY);
    tick_with(&mut w, vec![attack(tank, dummy)]);
    let mut seen = Vec::new();
    for _ in 0..40 {
        if !tick_with(&mut w, vec![]).is_empty() {
            seen.push((
                w.weapon_burst_left(tank, 0).unwrap(),
                w.weapon_wait(tank, 0).unwrap(),
            ));
        }
    }
    // After the first shell of a burst: one to come in three ticks. After the second: none,
    // and the rest of the reload (eight less the three of the burst).
    assert!(seen.contains(&(1, 3)), "{seen:?}");
    assert!(seen.contains(&(0, 5)), "{seen:?}");
    assert!(
        seen.iter().all(|s| *s == (1, 3) || *s == (0, 5)),
        "{seen:?}"
    );
}

/// Steps until something fires, at most `limit` ticks, and returns that tick's first shot.
fn until_a_shot(w: &mut World, limit: u32) -> Shot {
    for _ in 0..limit {
        if let Some(shot) = tick_with(w, vec![]).first() {
            return *shot;
        }
    }
    panic!("nothing fired in {limit} ticks");
}

#[test]
fn a_burst_that_has_no_victim_for_its_second_shell_is_dropped_but_the_reload_is_kept() {
    // A target of 10 hit points dies of the first shell; the second has nobody to go to.
    let mut w = armed();
    let tank = spawn(&mut w, TANK, at(10, 10));
    let weak = spawn(&mut w, WEAK, at(14, 10));
    let dummy = spawn(&mut w, DUMMY, at(10, 14));
    tick_with(&mut w, vec![attack(tank, weak)]);
    let first = until_a_shot(&mut w, 30);
    assert_eq!((first.target, first.hp_after), (weak, 0));
    assert_eq!(w.weapon_burst_left(tank, 0), Some(1));
    assert_eq!(w.weapon_wait(tank, 0), Some(3));
    // Three ticks on, the second shell is due: it is dropped, and the weapon waits out the rest
    // of the reload, as after a whole burst.
    assert!(run(&mut w, 3).is_empty());
    assert_eq!(w.weapon_burst_left(tank, 0), Some(0));
    assert_eq!(w.weapon_wait(tank, 0), Some(5));
    // A new target at once does not get the weapon any sooner: one reload after the first shell
    // at the earliest (the dummy lies to the south, so the turret has to swing first as well).
    let more = go(&mut w, vec![attack(tank, dummy)], 60);
    let next = more.first().expect("it fires again");
    assert_eq!(next.target, dummy);
    assert!(next.tick.0 >= first.tick.0 + 8, "{:?}", ticks(&more));
}

#[test]
fn a_burst_cut_by_a_stop_is_not_owed_afterwards() {
    let (mut w, tank, dummy) = tank_duel(DUMMY);
    tick_with(&mut w, vec![attack(tank, dummy)]);
    let first = until_a_shot(&mut w, 30);
    // Stop it between the shells: the second never comes, and the weapon is ready one reload
    // after the first shell, no sooner and no later.
    assert!(tick_with(&mut w, vec![Command::Stop { ids: vec![tank] }]).is_empty());
    let mut ready_at = None;
    for _ in 0..30 {
        assert!(tick_with(&mut w, vec![]).is_empty());
        if ready_at.is_none() && w.weapon_wait(tank, 0) == Some(0) {
            ready_at = Some(w.tick().0 - 1);
        }
    }
    assert_eq!(ready_at, Some(first.tick.0 + 8));
    assert_eq!(w.weapon_burst_left(tank, 0), Some(0));
    assert_eq!(w.hp_of(dummy), Some(1000 - 10));
    // Ordered again after the reload: a fresh burst of two.
    let again = ticks(&go(&mut w, vec![attack(tank, dummy)], 40));
    assert_eq!(gaps(&again)[..3], [3, 5, 3], "{again:?}");
}

// ---- reach, minimum range, aim ---------------------------------------------------------------

#[test]
fn the_minimum_range_is_measured_to_the_edge_like_the_range() {
    // The mortar (reach 10, not nearer than 3) and a dummy whose radius is half a cell, east of it.
    let fires_at = |gap: Fx| {
        let mut w = armed();
        let mortar = spawn(&mut w, MORTAR, at(20, 20));
        let dummy = spawn(
            &mut w,
            DUMMY,
            Vec2::new(Fx::from_int(20) + gap, Fx::from_int(20)),
        );
        !go(&mut w, vec![attack(mortar, dummy)], 60).is_empty()
    };
    // Three cells from the edge is three and a half from the middle: allowed.
    assert!(fires_at(Fx::from_ratio(7, 2)));
    // A hair nearer is not, and neither is anything closer.
    assert!(!fires_at(Fx::from_ratio(7, 2) - Fx::EPSILON));
    assert!(!fires_at(Fx::from_int(2)));
    assert!(!fires_at(Fx::ZERO));
    // Farther is fine, up to the reach of ten from the edge.
    assert!(fires_at(Fx::from_int(8)));
    assert!(fires_at(Fx::from_ratio(21, 2)));
}

/// An armed world whose seed gives the unit spawned first a heading far from `toward` (a unit
/// takes a random heading when it is spawned), so that a test about aiming has something to wait
/// for. Returns the world with the unit and its victim spawned, one `gap` cells north of it.
fn aiming_duel(kind: Kind, gap: i32) -> (World, EntityId, EntityId) {
    for seed in 0..64 {
        let mut w = World::new(WorldConfig::new(seed).with_map_size(MAP, MAP))
            .unwrap()
            .with_units(units())
            .unwrap()
            .with_weapons(table())
            .unwrap();
        let shooter = spawn(&mut w, kind, at(20, 20));
        let victim = spawn(&mut w, DUMMY, at(20, 20 - gap));
        let view = w.get(shooter).unwrap();
        let heading = view.turret.map_or(view.facing, |t| t.facing);
        let wanted = Facing::from_vec2(w.get(victim).unwrap().pos - view.pos).unwrap();
        if heading.abs_diff(wanted) >= 64 {
            return (w, shooter, victim);
        }
    }
    panic!("no seed in 64 turns a unit away from its target");
}

#[test]
fn a_turret_weapon_waits_for_the_turret() {
    // The tank's turret turns 12 steps a tick; the gun fires within 8 steps of the target.
    let (mut w, tank, north) = aiming_duel(TANK, 4);
    let before = w.get(tank).unwrap().turret.unwrap().facing;
    let start = w.tick().0;
    tick_with(&mut w, vec![attack(tank, north)]);
    let mut first = None;
    let mut turned = Vec::new();
    for _ in 0..60 {
        let shots = tick_with(&mut w, vec![]);
        turned.push(w.get(tank).unwrap().turret.unwrap().facing);
        if let Some(shot) = shots.first() {
            first = Some(*shot);
            break;
        }
    }
    let shot = first.expect("it fired once the turret was on target");
    let wanted = Facing::from_vec2(w.get(north).unwrap().pos - w.get(tank).unwrap().pos).unwrap();
    let turret = w.get(tank).unwrap().turret.unwrap().facing;
    assert!(turret.abs_diff(wanted) <= 8, "{turret:?} {wanted:?}");
    // The swing took the time it takes: the steps outside the arc over the rate, counting the
    // tick of the order, which already turns the turret once.
    let needed = u64::from(before.abs_diff(wanted).saturating_sub(8)).div_ceil(12);
    assert!(needed >= 5, "{needed}");
    assert!(
        shot.tick.0 - start + 1 >= needed,
        "{} updates of the turret, {needed} needed",
        shot.tick.0 - start + 1
    );
    // Until then it was not in the arc (it did not fire in a hurry and then turn).
    for facing in &turned[..turned.len() - 1] {
        assert!(facing.abs_diff(wanted) > 8, "{facing:?} {wanted:?}");
    }
}

#[test]
fn a_hull_weapon_waits_for_the_hull() {
    // The gunner has no turret: its hull has to turn to the target, 3 cells north of it.
    let (mut w, gunner, north) = aiming_duel(GUNNER, 3);
    let start = w.tick().0;
    tick_with(&mut w, vec![attack(gunner, north)]);
    let mut first = None;
    for _ in 0..60 {
        if let Some(shot) = tick_with(&mut w, vec![]).first() {
            first = Some(*shot);
            break;
        }
    }
    let shot = first.expect("it fired once the hull was on target");
    let view = w.get(gunner).unwrap();
    let wanted = Facing::from_vec2(w.get(north).unwrap().pos - view.pos).unwrap();
    assert!(view.facing.abs_diff(wanted) <= 8);
    // At 16 steps a tick, a turn of at least a quarter takes at least three ticks.
    assert!(shot.tick.0 - start >= 3, "{}", shot.tick.0 - start);
}

// ---- the checksum ----------------------------------------------------------------------------

#[test]
fn a_world_with_a_weapon_table_differs_from_one_without_from_the_first_tick() {
    // The table is in the checksum for as long as the world has one, so peers with different
    // tables (or one with and one without) disagree at once, not at the first shot.
    let build = |table: Option<WeaponTable>| match table {
        Some(table) => base().with_weapons(table).unwrap(),
        None => base(),
    };
    let mut tweaked = weapons();
    tweaked[0].damage += 1;
    let tweaked = WeaponTable::new(tweaked, loadouts()).unwrap();
    let mut worlds = [build(None), build(Some(table())), build(Some(tweaked))];
    let first: Vec<u64> = worlds.iter_mut().map(|w| w.step().checksum).collect();
    assert_ne!(first[0], first[1]);
    assert_ne!(first[1], first[2]);
    assert_ne!(first[0], first[2]);
    // And a world without a table is what it always was: the section does not exist for it
    // (the pinned scenarios of the earlier phases fix that).
    assert!(worlds[0].weapon_table().is_empty());
}

#[test]
fn a_shot_changes_the_checksum_of_a_world_that_has_a_table() {
    // Two armed worlds that differ only in whether the tank has been given its order.
    let build = |order: bool| {
        let mut w = armed();
        let tank = spawn(&mut w, TANK, at(10, 10));
        let dummy = spawn(&mut w, DUMMY, at(14, 10));
        if order {
            tick_with(&mut w, vec![attack(tank, dummy)]);
        } else {
            tick_with(&mut w, vec![]);
        }
        w
    };
    let (mut a, mut b) = (build(true), build(false));
    let mut fired = false;
    for _ in 0..100 {
        a.step();
        b.step();
        fired |= !a.shots().is_empty();
    }
    assert!(fired);
    assert_ne!(a.checksum(), b.checksum());
    assert!(b.shots().is_empty());
}

#[test]
fn a_run_repeats_and_a_clone_goes_on_the_same() {
    let play = || {
        let (mut w, gunner, dummy) = duel();
        let shots = go(&mut w, vec![attack(gunner, dummy)], 120);
        (w, shots)
    };
    let (a, sa) = play();
    let (b, sb) = play();
    assert_eq!(a.checksum_history(), b.checksum_history());
    assert_eq!(sa, sb);
    // A clone taken in the middle of a reload carries it along.
    let (mut c, gunner, dummy) = duel();
    tick_with(&mut c, vec![attack(gunner, dummy)]);
    run(&mut c, 7);
    let mut d = c.clone();
    assert_eq!(c.checksum(), d.checksum());
    assert_eq!(run(&mut c, 50), run(&mut d, 50));
    assert_eq!(c.checksum_history(), d.checksum_history());
}

#[test]
fn different_weapon_tables_give_different_checksums() {
    let play = |damage: u32| {
        let mut ws = weapons();
        ws[0].damage = damage;
        let mut w = base()
            .with_weapons(WeaponTable::new(ws, loadouts()).unwrap())
            .unwrap();
        let gunner = spawn(&mut w, GUNNER, at(10, 10));
        let dummy = spawn(&mut w, DUMMY, at(13, 10));
        go(&mut w, vec![attack(gunner, dummy)], 30);
        w.checksum()
    };
    assert_ne!(play(7), play(8));
    assert_eq!(play(7), play(7));
}

#[test]
fn a_despawned_victim_ends_the_shooting() {
    let (mut w, gunner, dummy) = duel();
    assert!(!go(&mut w, vec![attack(gunner, dummy)], 20).is_empty());
    tick_with(&mut w, vec![Command::Despawn { id: dummy }]);
    assert!(run(&mut w, 40).is_empty());
    assert_eq!(w.order_of(gunner), Some(Order::Idle));
    // A new unit in the old victim's slot is not the old victim.
    let fresh = spawn(&mut w, DUMMY, at(13, 10));
    assert_ne!(fresh, dummy);
    assert!(run(&mut w, 20).is_empty());
}

// ---- the order of the slots does not decide a fight ---------------------------------------------

/// What a skirmish came to, by the position of each unit (not by its slot).
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    /// Hit points at the end.
    hp: Vec<u32>,
    /// Who shot whom on the tick the orders were given, as (shooter, victim) positions, sorted.
    first_tick: Vec<(usize, usize)>,
    /// Shots over the whole run.
    total: usize,
    /// Ticks until each unit may fire again, after the first tick.
    waits_after_first: Vec<u32>,
}

const SPOTS: [(i32, i32); 3] = [(10, 10), (13, 10), (11, 13)];

/// Seven-hit-point units at the first `n` spots, spawned in `order` (so the slot of each differs
/// between runs), with `links` as attack orders given on one tick. Returns the outcome and the
/// checksum of the end.
fn skirmish(n: usize, links: &[(usize, usize)], order: &[usize]) -> (Outcome, u64) {
    let mut w = armed();
    let mut ids = vec![EntityId::new(0, 0); n];
    for &p in order {
        ids[p] = spawn(&mut w, FRAIL, at(SPOTS[p].0, SPOTS[p].1));
    }
    let pos_of = |id: EntityId| ids.iter().position(|&i| i == id).unwrap();
    let commands = links.iter().map(|&(a, b)| attack(ids[a], ids[b])).collect();
    let first = tick_with(&mut w, commands);
    let mut first_tick: Vec<(usize, usize)> = first
        .iter()
        .map(|s| (pos_of(s.attacker), pos_of(s.target)))
        .collect();
    first_tick.sort_unstable();
    let waits_after_first = ids.iter().map(|&i| w.weapon_wait(i, 0).unwrap()).collect();
    let total = first.len() + run(&mut w, 30).len();
    let hp = ids.iter().map(|&i| w.hp_of(i).unwrap()).collect();
    (
        Outcome {
            hp,
            first_tick,
            total,
            waits_after_first,
        },
        w.checksum(),
    )
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

/// Runs the skirmish in every spawn order and expects one outcome, and the same checksum every
/// time the same order is played again.
fn in_every_order(n: usize, links: &[(usize, usize)]) -> Outcome {
    let mut seen: Option<Outcome> = None;
    for order in permutations(n) {
        let (outcome, checksum) = skirmish(n, links, &order);
        assert_eq!(skirmish(n, links, &order).1, checksum, "{order:?} repeats");
        match &seen {
            Some(first) => assert_eq!(&outcome, first, "spawn order {order:?}"),
            None => seen = Some(outcome),
        }
    }
    seen.unwrap()
}

#[test]
fn two_units_that_kill_each_other_both_die() {
    let o = in_every_order(2, &[(0, 1), (1, 0)]);
    assert_eq!(o.hp, [0, 0]);
    assert_eq!(o.first_tick, [(0, 1), (1, 0)]);
    assert_eq!(o.total, 2);
    // Both spent their reload.
    assert_eq!(o.waits_after_first, [5, 5]);
}

#[test]
fn two_shooters_that_overkill_one_victim_both_fire_and_both_reload() {
    let o = in_every_order(3, &[(0, 2), (1, 2)]);
    assert_eq!(o.hp, [7, 7, 0]);
    assert_eq!(o.first_tick, [(0, 2), (1, 2)]);
    assert_eq!(o.total, 2);
    assert_eq!(o.waits_after_first, [5, 5, 0]);
}

#[test]
fn a_ring_of_three_kills_all_three() {
    let o = in_every_order(3, &[(0, 1), (1, 2), (2, 0)]);
    assert_eq!(o.hp, [0, 0, 0]);
    assert_eq!(o.first_tick, [(0, 1), (1, 2), (2, 0)]);
    assert_eq!(o.total, 3);
    assert_eq!(o.waits_after_first, [5, 5, 5]);
}

#[test]
fn a_unit_that_is_shot_down_on_a_tick_still_fires_on_that_tick() {
    // 0 shoots 1 and 1 shoots 2 on the same tick: 1 dies, and its shot at 2 still lands.
    let o = in_every_order(3, &[(0, 1), (1, 2)]);
    assert_eq!(o.hp, [7, 0, 0]);
    assert_eq!(o.first_tick, [(0, 1), (1, 2)]);
    assert_eq!(o.total, 2);
}

#[test]
fn damage_is_the_sum_of_the_hits_of_the_tick_saturating_at_zero() {
    // Both shooters hit a victim that cannot take two: the hit points are the sum of the hits
    // whoever has the lower slot, and both shots report what is left at the end of the tick (the
    // same number, not a running total that would follow the slot order).
    let mut w = armed();
    let a = spawn(&mut w, FRAIL, at(10, 10));
    let b = spawn(&mut w, FRAIL, at(13, 10));
    let v = spawn(&mut w, DUMMY, at(11, 12));
    let shots = tick_with(&mut w, vec![attack(a, v), attack(b, v)]);
    assert_eq!(shots.len(), 2);
    assert_eq!(shots[0].hp_after, 1000 - 14);
    assert_eq!(shots[1].hp_after, 1000 - 14);
    assert_eq!(w.hp_of(v), Some(1000 - 14));
}
