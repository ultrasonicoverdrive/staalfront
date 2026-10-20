//! Projectiles (WP5.2), through the public API: the four kinds of flight and how long each takes, what
//! happens when the target moves or dies, the fuse, a shooter that dies with a shell in the air, the
//! two passes of a tick (every hit decided before any hit point changes, the damage summed per victim),
//! that no outcome depends on the order of the slots, the snapshot and the checksum.
//!
//! The numbers are small and invented so a test can follow them tick by tick; the shipped set is
//! `assets/config/weapon_defs.ron`, which `tools/sf_headless/tests/projectiles.rs` runs.

use sf_math::{Facing, Fx, Vec2};
use sf_sim::{
    Command, ConfigError, EntityId, GUIDED_FLIGHT_FACTOR, HIT_SLACK, Impact, ImpactCause, Kind,
    Loadout, Mount, Order, PlayerId, ProjectileKind, Shot, UnitStats, UnitTable, WeaponStats,
    WeaponTable, WeaponsError, World, WorldConfig, per_tick,
};

const MAP: u32 = 64;

// Shooters, one weapon each.
const CANNONEER: Kind = Kind(1); // Straight, 8 cells, a shell of 30
const SLOWER: Kind = Kind(2); // Straight but slow, 12 cells
const LOBBER: Kind = Kind(3); // Arc, 12 cells
const ROCKETEER: Kind = Kind(4); // Guided, 6 cells, fires within 8 steps of the target
const STRAIGHT_ROCKETEER: Kind = Kind(5); // the same rocket without steering
const BOMBER: Kind = Kind(6); // Falling, 8 cells
const GLASS: Kind = Kind(7); // seven hit points and a lobber's weapon
const ZAPPER: Kind = Kind(8); // an instant gun of seven
const TWINNER: Kind = Kind(9); // two straight shells three ticks apart
const SNAPPER: Kind = Kind(10); // seven hit points, a straight shell of seven, 4 cells
const FASTER: Kind = Kind(11); // a rocket thrower with a rocket that is slower than a runner
// Things that are shot at.
const DUMMY: Kind = Kind(20); // 1000 hit points, walks at 2 cells a second
const FRAGILE: Kind = Kind(21); // 10 hit points
const ROCK: Kind = Kind(22); // cannot be hurt
const RUNNER: Kind = Kind(23); // 1000 hit points, walks at 15 cells a second (0.75 a tick)

const CANNON: u16 = 1; // reload 20, 30 damage, speed 1/2 a tick, range 8, all round
const SLOW: u16 = 2; // reload 20, 30 damage, speed 2/5 a tick, range 12, all round
const LOB: u16 = 3; // reload 40, 50 damage, speed 1/2, height 4, range 12, all round
const ROCKET: u16 = 4; // reload 30, 45 damage, speed 3/5, turns 8, range 6, arc 8
const STRAIGHT_ROCKET: u16 = 5; // the same without steering
const BOMB: u16 = 6; // reload 40, 60 damage, falls 1/4 a tick from 3, range 8, all round
const ZAP: u16 = 7; // instant, reload 5, 7 damage, range 4, all round
const TWIN: u16 = 8; // two shells of 10, 3 ticks apart, reload 20, speed 1/2, range 8
const SNAP: u16 = 9; // reload 20, 7 damage, speed 1/2, range 4, all round
const SLOW_ROCKET: u16 = 10; // guided at 2/5 a tick, range 6

const TURN: u8 = 8;

fn half() -> Fx {
    Fx::HALF
}

fn unit(kind: Kind, hp: u32, range: i32, speed_cps: i32) -> UnitStats {
    UnitStats {
        kind,
        hp,
        movement_class: Some(0),
        speed_per_tick: per_tick(Fx::from_int(speed_cps)),
        turn_per_tick: 16,
        size: Fx::ONE,
        footprint: None,
        weapon_range: Fx::from_int(range),
        turret_per_tick: 0,
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
        unit(CANNONEER, 50, 8, 1),
        unit(SLOWER, 50, 12, 1),
        unit(LOBBER, 50, 12, 1),
        unit(ROCKETEER, 50, 6, 1),
        unit(STRAIGHT_ROCKETEER, 50, 6, 1),
        unit(BOMBER, 50, 8, 1),
        unit(GLASS, 7, 12, 1),
        unit(ZAPPER, 50, 4, 1),
        unit(TWINNER, 50, 8, 1),
        unit(SNAPPER, 7, 4, 1),
        unit(FASTER, 50, 6, 1),
        unit(DUMMY, 1000, 0, 2),
        unit(FRAGILE, 10, 0, 2),
        unit(RUNNER, 1000, 0, 15),
        rock,
    ])
    .unwrap()
}

#[allow(clippy::too_many_arguments)]
fn weapon(
    kind: u16,
    projectile: ProjectileKind,
    range: i32,
    reload: u32,
    damage: u32,
    speed: Fx,
    height: Fx,
    turn: u8,
    arc: u8,
) -> WeaponStats {
    WeaponStats {
        kind,
        range: Fx::from_int(range),
        min_range: Fx::ZERO,
        reload_ticks: reload,
        burst: 1,
        burst_delay_ticks: 0,
        damage,
        damage_type: 0,
        projectile,
        projectile_speed_per_tick: speed,
        flight_height: height,
        guide_turn_per_tick: turn,
        splash_radius: Fx::ZERO,
        splash_inner: Fx::ZERO,
        friendly_fire: true,
        mount: Mount::Hull,
        fire_arc: arc,
    }
}

fn weapons() -> Vec<WeaponStats> {
    use ProjectileKind::{Arc, Falling, Guided, Instant, Straight};
    vec![
        weapon(CANNON, Straight, 8, 20, 30, half(), Fx::ZERO, 0, 128),
        weapon(
            SLOW,
            Straight,
            12,
            20,
            30,
            Fx::from_ratio(2, 5),
            Fx::ZERO,
            0,
            128,
        ),
        weapon(LOB, Arc, 12, 40, 50, half(), Fx::from_int(4), 0, 128),
        weapon(
            ROCKET,
            Guided,
            6,
            30,
            45,
            Fx::from_ratio(3, 5),
            Fx::ZERO,
            TURN,
            8,
        ),
        weapon(
            STRAIGHT_ROCKET,
            Straight,
            6,
            30,
            45,
            Fx::from_ratio(3, 5),
            Fx::ZERO,
            0,
            8,
        ),
        weapon(
            BOMB,
            Falling,
            8,
            40,
            60,
            Fx::from_ratio(1, 4),
            Fx::from_int(3),
            0,
            128,
        ),
        weapon(ZAP, Instant, 4, 5, 7, Fx::ZERO, Fx::ZERO, 0, 128),
        WeaponStats {
            burst: 2,
            burst_delay_ticks: 3,
            ..weapon(TWIN, Straight, 8, 20, 10, half(), Fx::ZERO, 0, 128)
        },
        weapon(SNAP, Straight, 4, 20, 7, half(), Fx::ZERO, 0, 128),
        weapon(
            SLOW_ROCKET,
            Guided,
            6,
            30,
            45,
            Fx::from_ratio(2, 5),
            Fx::ZERO,
            TURN,
            8,
        ),
    ]
}

fn loadouts() -> Vec<Loadout> {
    let l = |unit: Kind, w: u16| Loadout {
        unit,
        weapons: vec![w],
    };
    vec![
        l(CANNONEER, CANNON),
        l(SLOWER, SLOW),
        l(LOBBER, LOB),
        l(ROCKETEER, ROCKET),
        l(STRAIGHT_ROCKETEER, STRAIGHT_ROCKET),
        l(BOMBER, BOMB),
        l(GLASS, LOB),
        l(ZAPPER, ZAP),
        l(TWINNER, TWIN),
        l(SNAPPER, SNAP),
        l(FASTER, SLOW_ROCKET),
    ]
}

fn table() -> WeaponTable {
    WeaponTable::new(weapons(), loadouts()).unwrap()
}

fn armed() -> World {
    World::new(WorldConfig::new(3).with_map_size(MAP, MAP))
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_weapons(table())
        .unwrap()
}

fn at(x: i32, y: i32) -> Vec2 {
    Vec2::from_ints(x, y)
}

/// Everything a test wants to know about the ticks it ran.
#[derive(Default)]
struct Log {
    shots: Vec<Shot>,
    impacts: Vec<Impact>,
}

impl Log {
    fn only_impact(&self) -> Impact {
        assert_eq!(self.impacts.len(), 1, "{:?}", self.impacts);
        self.impacts[0]
    }

    fn only_shot(&self) -> Shot {
        assert_eq!(self.shots.len(), 1, "{:?}", self.shots);
        self.shots[0]
    }
}

/// Steps one tick with `commands` (all by player 0) and records the shots and impacts of the tick.
fn tick_with(w: &mut World, log: &mut Log, commands: Vec<Command>) {
    let tick = w.tick();
    for c in commands {
        w.schedule(tick, PlayerId(0), c).unwrap();
    }
    let report = w.step();
    assert_eq!(report.shots as usize, w.shots().len());
    assert_eq!(report.impacts as usize, w.impacts().len());
    log.shots.extend_from_slice(w.shots());
    log.impacts.extend_from_slice(w.impacts());
}

fn spawn(w: &mut World, kind: Kind, pos: Vec2) -> EntityId {
    let id = EntityId::new(w.slot_count() as u32, 0);
    let mut log = Log::default();
    tick_with(
        w,
        &mut log,
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

fn walk(who: EntityId, to: Vec2) -> Command {
    order(who, Order::Move(to))
}

/// Gives the commands on one tick, then runs `n - 1` more.
fn go(w: &mut World, commands: Vec<Command>, n: u32) -> Log {
    let mut log = Log::default();
    tick_with(w, &mut log, commands);
    for _ in 1..n {
        tick_with(w, &mut log, vec![]);
    }
    log
}

fn run(w: &mut World, n: u32) -> Log {
    let mut log = Log::default();
    for _ in 0..n {
        tick_with(w, &mut log, vec![]);
    }
    log
}

/// Steps until a projectile has gone off, at most `limit` ticks.
fn until_impact(w: &mut World, log: &mut Log, limit: u32) {
    for _ in 0..limit {
        tick_with(w, log, vec![]);
        if !log.impacts.is_empty() {
            return;
        }
    }
    panic!("nothing went off in {limit} ticks: {:?}", log.shots);
}

/// Steps until something has been fired, at most `limit` ticks (a unit with a narrow arc turns to its
/// target first).
fn until_shot(w: &mut World, log: &mut Log, limit: u32) {
    for _ in 0..limit {
        if !log.shots.is_empty() {
            return;
        }
        tick_with(w, log, vec![]);
    }
    assert!(!log.shots.is_empty(), "nothing fired in {limit} ticks");
}

/// The ticks a projectile needs to cover `distance` at `speed` per tick, rounded up.
fn ticks_to_cover(distance: Fx, speed: Fx) -> u64 {
    (distance.raw() as u64).div_ceil(speed.raw() as u64)
}

fn dist(w: &World, a: EntityId, b: EntityId) -> Fx {
    (w.get(a).unwrap().pos - w.get(b).unwrap().pos).length()
}

/// The reach by which a projectile counts as touching a unit of size one: its radius and the slack.
fn touch_radius() -> Fx {
    Fx::HALF + HIT_SLACK
}

// ---- the table ------------------------------------------------------------------------------

#[test]
fn a_projectile_that_flies_needs_a_speed_and_a_falling_one_a_height() {
    let bad = |f: &dyn Fn(&mut WeaponStats)| {
        let mut w = weapons();
        f(&mut w[0]);
        WeaponTable::new(w, vec![]).unwrap_err()
    };
    assert_eq!(
        bad(&|w| w.projectile_speed_per_tick = Fx::ZERO),
        WeaponsError::NoSpeed(CANNON)
    );
    for kind in [
        ProjectileKind::Arc,
        ProjectileKind::Guided,
        ProjectileKind::Falling,
    ] {
        assert_eq!(
            bad(&|w| {
                w.projectile = kind;
                w.flight_height = Fx::ONE;
                w.projectile_speed_per_tick = Fx::ZERO;
            }),
            WeaponsError::NoSpeed(CANNON),
            "{kind:?}"
        );
    }
    assert_eq!(
        bad(&|w| {
            w.projectile = ProjectileKind::Falling;
            w.flight_height = Fx::ZERO;
        }),
        WeaponsError::NoHeight(CANNON)
    );
    // An instant weapon needs neither.
    let mut w = weapons();
    w[0].projectile = ProjectileKind::Instant;
    w[0].projectile_speed_per_tick = Fx::ZERO;
    assert!(WeaponTable::new(w, vec![]).is_ok());
    assert!(!WeaponsError::NoSpeed(1).to_string().is_empty());
    assert!(!WeaponsError::NoHeight(1).to_string().is_empty());
    // A world refuses the table the same way (through the bridge it would be this error).
    let mut w = weapons();
    w[0].projectile_speed_per_tick = Fx::ZERO;
    assert_eq!(
        WeaponTable::new(w, loadouts()).err(),
        Some(WeaponsError::NoSpeed(CANNON))
    );
    let _ = ConfigError::WeaponsAfterStart;
}

// ---- straight -------------------------------------------------------------------------------

/// A cannoneer at (10, 10) and a dummy `gap` cells east, already facing it (all round anyway).
fn straight_duel(gap: i32) -> (World, EntityId, EntityId) {
    let mut w = armed();
    let shooter = spawn(&mut w, CANNONEER, at(10, 10));
    let dummy = spawn(&mut w, DUMMY, at(10 + gap, 10));
    (w, shooter, dummy)
}

#[test]
fn a_shot_does_nothing_until_its_shell_arrives() {
    let (mut w, shooter, dummy) = straight_duel(5);
    let launch = go(&mut w, vec![attack(shooter, dummy)], 1);
    let shot = launch.only_shot();
    assert_eq!((shot.weapon, shot.damage), (CANNON, 30));
    assert!(launch.impacts.is_empty());
    assert_eq!(w.projectile_count(), 1);
    // The hit points are untouched, and the shot reports what the target has at the end of the tick.
    assert_eq!(w.hp_of(dummy), Some(1000));
    assert_eq!(shot.hp_after, 1000);
    let report_launch = w.checksum();
    // Each tick on the way the shell is somewhere new and nothing has been taken.
    let mut log = Log::default();
    until_impact(&mut w, &mut log, 40);
    let impact = log.only_impact();
    assert_eq!(impact.victim, Some(dummy));
    assert_eq!(impact.cause, ImpactCause::Proximity);
    assert_eq!((impact.damage, impact.hp_after), (30, 970));
    assert_eq!(w.hp_of(dummy), Some(970));
    assert_eq!(w.projectile_count(), 0);
    assert_ne!(w.checksum(), report_launch);
}

#[test]
fn a_straight_shell_takes_the_distance_over_the_speed_to_arrive() {
    // It goes off when its path comes within reach of the target's middle, so it covers the distance
    // less the reach: no more, no less than a tick off the plain quotient.
    for gap in [2, 3, 4, 5, 6, 7, 8] {
        let (mut w, shooter, dummy) = straight_duel(gap);
        let mut log = go(&mut w, vec![attack(shooter, dummy)], 1);
        let fired = log.only_shot().tick;
        until_impact(&mut w, &mut log, 60);
        let flight = log.only_impact().tick.0 - fired.0;
        let d = dist(&w, shooter, dummy);
        let want = ticks_to_cover(d - touch_radius(), half());
        assert_eq!(flight, want, "gap {gap}");
        // And within a tick of the plain distance over the speed, as the data suggests.
        let plain = ticks_to_cover(d, half());
        assert!(
            flight <= plain && plain - flight <= 2,
            "gap {gap}: {flight} {plain}"
        );
    }
}

#[test]
fn a_straight_shell_misses_a_target_that_walks_out_of_its_line() {
    // 8 cells off at half a cell a tick is 16 ticks; the dummy walks a tenth of a cell a tick across the
    // line and is 1.5 cells away when the shell arrives where it stood.
    let (mut w, shooter, dummy) = straight_duel(8);
    let mut log = go(
        &mut w,
        vec![attack(shooter, dummy), walk(dummy, at(18, 30))],
        1,
    );
    let aim = w.get(dummy).unwrap().pos;
    until_impact(&mut w, &mut log, 60);
    let impact = log.only_impact();
    assert_eq!(impact.cause, ImpactCause::Arrived);
    assert_eq!(impact.victim, None);
    assert_eq!(impact.damage, 0);
    // It went off at the aim point: where the dummy stood when the shell was fired.
    assert_eq!(impact.at, aim);
    assert_eq!(w.hp_of(dummy), Some(1000));
    assert!(dist_to(&w, dummy, aim) > touch_radius());
}

fn dist_to(w: &World, id: EntityId, p: Vec2) -> Fx {
    (w.get(id).unwrap().pos - p).length()
}

#[test]
fn a_straight_shell_hits_a_target_that_walks_into_its_path() {
    // The dummy walks toward the cannoneer along the line of fire: the path touches it sooner than it
    // would have touched a dummy that stood still.
    let (mut w, shooter, dummy) = straight_duel(8);
    let mut log = go(
        &mut w,
        vec![attack(shooter, dummy), walk(dummy, at(14, 10))],
        1,
    );
    let fired = log.only_shot().tick;
    until_impact(&mut w, &mut log, 60);
    let impact = log.only_impact();
    assert_eq!(impact.victim, Some(dummy));
    let flight = impact.tick.0 - fired.0;
    let still = ticks_to_cover(Fx::from_int(8) - touch_radius(), half());
    assert!(flight < still, "{flight} against {still}");
    assert_eq!(w.hp_of(dummy), Some(970));
}

#[test]
fn only_the_target_is_hit_what_stands_in_the_path_is_not() {
    let (mut w, shooter, dummy) = straight_duel(8);
    let bystander = spawn(&mut w, DUMMY, at(14, 10));
    let mut log = go(&mut w, vec![attack(shooter, dummy)], 1);
    until_impact(&mut w, &mut log, 60);
    assert_eq!(log.only_impact().victim, Some(dummy));
    assert_eq!(w.hp_of(bystander), Some(1000));
    assert_eq!(w.hp_of(dummy), Some(970));
}

#[test]
fn a_straight_shell_goes_off_at_its_aim_point_when_its_target_has_gone() {
    let (mut w, shooter, dummy) = straight_duel(8);
    let mut log = go(&mut w, vec![attack(shooter, dummy)], 1);
    let aim = w.get(dummy).unwrap().pos;
    // Three ticks in the target disappears, and a new unit takes its slot.
    for _ in 0..3 {
        tick_with(&mut w, &mut log, vec![]);
    }
    tick_with(&mut w, &mut log, vec![Command::Despawn { id: dummy }]);
    spawn(&mut w, DUMMY, at(10, 20));
    let stand_in = EntityId::new(dummy.index(), dummy.generation() + 1);
    assert_eq!(w.hp_of(stand_in), Some(1000));
    until_impact(&mut w, &mut log, 60);
    let impact = log.only_impact();
    assert_eq!(
        (impact.cause, impact.victim, impact.at),
        (ImpactCause::Arrived, None, aim)
    );
    assert_eq!(w.hp_of(stand_in), Some(1000));
}

#[test]
fn a_target_that_cannot_be_hurt_is_not_a_victim() {
    let mut w = armed();
    let shooter = spawn(&mut w, CANNONEER, at(10, 10));
    let rock = spawn(&mut w, ROCK, at(14, 10));
    // Nothing is fired at it at all (as in WP5.1).
    let log = go(&mut w, vec![attack(shooter, rock)], 60);
    assert!(log.shots.is_empty() && log.impacts.is_empty());
    assert_eq!(w.projectile_count(), 0);
}

// ---- arcing ---------------------------------------------------------------------------------

/// A lobber at (10, 10) and a dummy `gap` cells east.
fn lob_duel(gap: i32) -> (World, EntityId, EntityId) {
    let mut w = armed();
    let shooter = spawn(&mut w, LOBBER, at(10, 10));
    let dummy = spawn(&mut w, DUMMY, at(10 + gap, 10));
    (w, shooter, dummy)
}

#[test]
fn an_arc_lands_at_its_aim_point_after_distance_over_speed_ticks() {
    for gap in [3, 5, 8, 11] {
        let (mut w, shooter, dummy) = lob_duel(gap);
        let mut log = go(&mut w, vec![attack(shooter, dummy)], 1);
        let fired = log.only_shot().tick;
        let aim = w.get(dummy).unwrap().pos;
        until_impact(&mut w, &mut log, 80);
        let impact = log.only_impact();
        // It lands, with no way of touching anything on its way, at the aim point.
        assert_eq!(
            (impact.cause, impact.at),
            (ImpactCause::Arrived, aim),
            "gap {gap}"
        );
        assert_eq!(impact.victim, Some(dummy));
        assert_eq!(w.hp_of(dummy), Some(950));
        let d = dist(&w, shooter, dummy);
        assert_eq!(
            impact.tick.0 - fired.0,
            ticks_to_cover(d, half()),
            "gap {gap}"
        );
    }
}

#[test]
fn an_arc_rises_to_its_flight_height_and_comes_down() {
    let (mut w, shooter, dummy) = lob_duel(10);
    let mut log = go(&mut w, vec![attack(shooter, dummy)], 1);
    let mut heights = Vec::new();
    for _ in 0..60 {
        for p in w.snapshot().projectiles() {
            heights.push(p.height);
        }
        tick_with(&mut w, &mut log, vec![]);
        if !log.impacts.is_empty() {
            break;
        }
    }
    let peak = Fx::from_int(4);
    let top = *heights.iter().max().unwrap();
    // The ticks are discrete, so the highest sample is a hair under the top of the parabola: at most
    // (speed / span) squared of it.
    let slack = peak.mul_div(Fx::HALF.square(), Fx::from_int(10).square());
    assert!(top <= peak && top >= peak - slack - Fx::EPSILON, "{top:?}");
    // Rises, then falls, and is on the ground when it is launched.
    assert_eq!(heights[0], Fx::ZERO);
    let at_top = heights.iter().position(|&h| h == top).unwrap();
    assert!(heights[..=at_top].windows(2).all(|p| p[0] <= p[1]));
    assert!(heights[at_top..].windows(2).all(|p| p[0] >= p[1]));
    assert!(*heights.last().unwrap() < Fx::ONE, "{heights:?}");
    // Symmetric: the middle of the flight is the top of it.
    let n = heights.len();
    assert!(
        (at_top as i64 - (n as i64 - 1) / 2).abs() <= 1,
        "{at_top} of {n}"
    );
}

#[test]
fn an_arc_flies_over_what_is_in_its_way() {
    let (mut w, shooter, dummy) = lob_duel(10);
    let bystander = spawn(&mut w, DUMMY, at(15, 10));
    let mut log = go(&mut w, vec![attack(shooter, dummy)], 1);
    until_impact(&mut w, &mut log, 80);
    assert_eq!(log.only_impact().victim, Some(dummy));
    assert_eq!(w.hp_of(bystander), Some(1000));
}

#[test]
fn an_arc_misses_a_target_that_has_moved_away_from_where_it_lands() {
    // 10 cells at half a cell a tick is 20 ticks, during which the dummy walks two cells.
    let (mut w, shooter, dummy) = lob_duel(10);
    let mut log = go(
        &mut w,
        vec![attack(shooter, dummy), walk(dummy, at(20, 30))],
        1,
    );
    let aim = w.get(dummy).unwrap().pos;
    until_impact(&mut w, &mut log, 80);
    let impact = log.only_impact();
    assert_eq!(
        (impact.cause, impact.victim, impact.at),
        (ImpactCause::Arrived, None, aim)
    );
    assert_eq!(w.hp_of(dummy), Some(1000));
    assert!(dist_to(&w, dummy, aim) > touch_radius());
}

#[test]
fn an_arc_hits_a_target_that_has_only_moved_a_little() {
    // The landing point counts within the reach of the target (its radius and the slack), so a unit
    // that has walked less than that from where it stood is still hit. A walk of 5 cells over 20 ticks
    // at a tenth of a cell a tick is two cells; a short walk is half a cell.
    let (mut w, shooter, dummy) = lob_duel(10);
    let mut log = go(
        &mut w,
        vec![attack(shooter, dummy), walk(dummy, at(20, 11))],
        1,
    );
    until_impact(&mut w, &mut log, 80);
    // It walked a cell in ten ticks and then stopped one cell off: out of reach.
    assert_eq!(log.only_impact().victim, None);
    let (mut w, shooter, dummy) = lob_duel(10);
    let mut log = go(
        &mut w,
        vec![
            attack(shooter, dummy),
            walk(dummy, at(20, 10) + Vec2::new(Fx::ZERO, half())),
        ],
        1,
    );
    until_impact(&mut w, &mut log, 80);
    assert_eq!(log.only_impact().victim, Some(dummy));
}

// ---- falling --------------------------------------------------------------------------------

#[test]
fn a_falling_shell_starts_above_its_aim_point_and_drops_height_over_speed_ticks() {
    let mut w = armed();
    let bomber = spawn(&mut w, BOMBER, at(10, 10));
    let dummy = spawn(&mut w, DUMMY, at(15, 10));
    let mut log = go(&mut w, vec![attack(bomber, dummy)], 1);
    let fired = log.only_shot().tick;
    let aim = w.get(dummy).unwrap().pos;
    let view = w.snapshot().projectiles().next().unwrap();
    assert_eq!(
        (view.pos, view.height, view.kind),
        (aim, Fx::from_int(3), ProjectileKind::Falling)
    );
    let mut heights = vec![view.height];
    while log.impacts.is_empty() {
        tick_with(&mut w, &mut log, vec![]);
        heights.extend(w.snapshot().projectiles().map(|p| p.height));
    }
    let impact = log.only_impact();
    // Three cells at a quarter cell a tick is twelve ticks, and the height goes down by a quarter each.
    assert_eq!(impact.tick.0 - fired.0, 12);
    assert_eq!(heights.len(), 12);
    assert!(
        heights
            .windows(2)
            .all(|p| p[0] - p[1] == Fx::from_ratio(1, 4))
    );
    assert_eq!(
        (impact.cause, impact.at, impact.victim),
        (ImpactCause::Arrived, aim, Some(dummy))
    );
    assert_eq!(w.hp_of(dummy), Some(940));
}

#[test]
fn a_falling_shell_can_be_walked_out_from_under() {
    let mut w = armed();
    let bomber = spawn(&mut w, BOMBER, at(10, 10));
    let dummy = spawn(&mut w, DUMMY, at(15, 10));
    let mut log = go(
        &mut w,
        vec![attack(bomber, dummy), walk(dummy, at(15, 30))],
        1,
    );
    until_impact(&mut w, &mut log, 40);
    let impact = log.only_impact();
    assert_eq!(impact.victim, None);
    assert_eq!(w.hp_of(dummy), Some(1000));
}

// ---- guided ---------------------------------------------------------------------------------

/// A rocket thrower of `kind` at (20, 20) and a dummy-sized target `gap` cells east.
fn rocket_duel(kind: Kind, target: Kind, gap: i32) -> (World, EntityId, EntityId) {
    let mut w = armed();
    let shooter = spawn(&mut w, kind, at(20, 20));
    let victim = spawn(&mut w, target, at(20 + gap, 20));
    (w, shooter, victim)
}

#[test]
fn a_guided_projectile_hits_a_dodging_target_that_a_straight_one_misses() {
    // The same rocket, with and without steering, at a runner that breaks away from the line of fire
    // the moment the rocket is launched, and breaks the other way four ticks later.
    let dodge = |kind: Kind| {
        let (mut w, shooter, runner) = rocket_duel(kind, RUNNER, 5);
        let mut log = go(&mut w, vec![attack(shooter, runner)], 1);
        until_shot(&mut w, &mut log, 20);
        tick_with(&mut w, &mut log, vec![walk(runner, at(25, 40))]);
        for _ in 0..3 {
            tick_with(&mut w, &mut log, vec![]);
        }
        tick_with(&mut w, &mut log, vec![walk(runner, at(25, 0))]);
        for _ in 0..40 {
            tick_with(&mut w, &mut log, vec![]);
        }
        (log, w.hp_of(runner).unwrap())
    };
    let (guided, hp) = dodge(ROCKETEER);
    assert_eq!(guided.impacts.len(), 1, "{:?}", guided.impacts);
    assert!(guided.impacts[0].victim.is_some());
    assert_eq!(guided.impacts[0].cause, ImpactCause::Proximity);
    assert_eq!(hp, 1000 - 45);
    let (straight, hp) = dodge(STRAIGHT_ROCKETEER);
    assert_eq!(straight.impacts.len(), 1, "{:?}", straight.impacts);
    assert_eq!(straight.impacts[0].victim, None);
    assert_eq!(hp, 1000);
}

#[test]
fn a_guided_projectile_turns_at_most_guide_turn_a_tick() {
    // A runner crossing in front of the launcher makes the rocket keep turning.
    let (mut w, shooter, runner) = rocket_duel(ROCKETEER, RUNNER, 5);
    let mut log = go(&mut w, vec![attack(shooter, runner)], 1);
    until_shot(&mut w, &mut log, 20);
    tick_with(&mut w, &mut log, vec![walk(runner, at(25, 40))]);
    let mut headings = vec![w.snapshot().projectiles().next().unwrap().heading];
    while log.impacts.is_empty() && headings.len() < 60 {
        tick_with(&mut w, &mut log, vec![]);
        headings.extend(w.snapshot().projectiles().map(|p| p.heading));
    }
    assert!(headings.len() > 4);
    let turns: Vec<u8> = headings.windows(2).map(|p| p[0].abs_diff(p[1])).collect();
    assert!(turns.iter().all(|&t| t <= TURN), "{turns:?}");
    assert!(turns.contains(&TURN), "the cap never bound: {turns:?}");
}

#[test]
fn a_guided_projectile_leaves_the_way_its_launcher_points_and_steers_from_there() {
    // The hull may point up to 8 steps off the target when it fires. The first heading of the rocket
    // is the hull's, not the bearing to the target.
    let mut seen_off_by_some = false;
    for seed in 0..64 {
        let mut w = World::new(WorldConfig::new(seed).with_map_size(MAP, MAP))
            .unwrap()
            .with_units(units())
            .unwrap()
            .with_weapons(table())
            .unwrap();
        let shooter = spawn(&mut w, ROCKETEER, at(20, 20));
        let dummy = spawn(&mut w, DUMMY, at(20, 25));
        let mut log = go(&mut w, vec![attack(shooter, dummy)], 1);
        while log.shots.is_empty() {
            tick_with(&mut w, &mut log, vec![]);
        }
        let view = w.snapshot().projectiles().next().unwrap();
        let hull = w.get(shooter).unwrap().facing;
        assert_eq!(view.heading, hull, "seed {seed}");
        let bearing =
            Facing::from_vec2(w.get(dummy).unwrap().pos - w.get(shooter).unwrap().pos).unwrap();
        assert!(hull.abs_diff(bearing) <= 8);
        seen_off_by_some |= hull.abs_diff(bearing) != 0;
        // And it hits.
        until_impact(&mut w, &mut log, 40);
        assert_eq!(log.only_impact().victim, Some(dummy), "seed {seed}");
    }
    assert!(
        seen_off_by_some,
        "no seed launched with an error to steer out"
    );
}

#[test]
fn a_guided_projectile_flies_on_to_the_last_known_point_when_its_target_dies() {
    // The target walks away for a few ticks, and then dies: despawned, or shot down to zero hit points
    // by a second shooter. Either way the rocket gives it up and flies for the place it saw it last.
    for despawn in [true, false] {
        let (mut w, shooter, victim) = rocket_duel(ROCKETEER, FRAGILE, 5);
        let zapper = spawn(&mut w, ZAPPER, at(23, 21));
        let mut log = go(&mut w, vec![attack(shooter, victim)], 1);
        until_shot(&mut w, &mut log, 20);
        let first = w.snapshot().projectiles().next().unwrap();
        assert_eq!(first.target, Some(victim));
        tick_with(&mut w, &mut log, vec![walk(victim, at(26, 30))]);
        // The last place the rocket sees the target is where it stands at the end of the tick before
        // the one it dies in.
        let mut last_seen = w.get(victim).unwrap().pos;
        if despawn {
            for _ in 0..3 {
                tick_with(&mut w, &mut log, vec![]);
                last_seen = w.get(victim).unwrap().pos;
            }
            tick_with(&mut w, &mut log, vec![Command::Despawn { id: victim }]);
        } else {
            // Two shots of seven take ten hit points down to zero, five ticks apart.
            tick_with(&mut w, &mut log, vec![attack(zapper, victim)]);
            while w.hp_of(victim) != Some(0) {
                tick_with(&mut w, &mut log, vec![]);
            }
            // It walks on after it is down, but the rocket last saw it where it was brought to zero.
            last_seen = w.get(victim).unwrap().pos;
        }
        assert_eq!(
            w.projectile_count(),
            1,
            "despawn {despawn}: it is still flying"
        );
        assert!(
            log.impacts.is_empty(),
            "despawn {despawn}: {:?}",
            log.impacts
        );
        tick_with(&mut w, &mut log, vec![]);
        let flying = w.snapshot().projectiles().next().expect("still in the air");
        assert_eq!(flying.target, None, "despawn {despawn}");
        until_impact(&mut w, &mut log, 60);
        let impact = log.only_impact();
        assert_eq!(impact.victim, None, "despawn {despawn}");
        assert_eq!(impact.cause, ImpactCause::Arrived, "despawn {despawn}");
        assert!(
            (impact.at - last_seen).length() <= HIT_SLACK,
            "despawn {despawn}: went off at {:?}, the last place seen was {last_seen:?}",
            impact.at
        );
    }
}

#[test]
fn a_guided_projectile_that_cannot_catch_its_target_runs_out_of_fuse() {
    // A runner (three quarters of a cell a tick) outpaces a rocket of two fifths. The rocket is fired
    // the moment the runner is in reach, and chases until its fuse is out: twice the time to fly its
    // weapon's range.
    let mut w = armed();
    let shooter = spawn(&mut w, FASTER, at(20, 20));
    let runner = spawn(&mut w, RUNNER, at(25, 20));
    let mut log = go(&mut w, vec![attack(shooter, runner)], 1);
    until_shot(&mut w, &mut log, 20);
    let fired = log.only_shot().tick;
    tick_with(&mut w, &mut log, vec![walk(runner, at(60, 20))]);
    until_impact(&mut w, &mut log, 100);
    let impact = log.only_impact();
    assert_eq!((impact.cause, impact.victim), (ImpactCause::Fuse, None));
    let fuse =
        u64::from(GUIDED_FLIGHT_FACTOR) * ticks_to_cover(Fx::from_int(6), Fx::from_ratio(2, 5));
    assert_eq!(impact.tick.0 - fired.0, fuse);
    assert_eq!(w.hp_of(runner), Some(1000));
}

// ---- the shooter ----------------------------------------------------------------------------

#[test]
fn a_shell_in_flight_lands_when_its_shooter_is_shot_down() {
    let mut w = armed();
    let glass = spawn(&mut w, GLASS, at(10, 10));
    let dummy = spawn(&mut w, DUMMY, at(20, 10));
    let zapper = spawn(&mut w, ZAPPER, at(10, 13));
    let mut log = go(&mut w, vec![attack(glass, dummy)], 1);
    let fired = log.only_shot().tick;
    assert_eq!(w.projectile_count(), 1);
    // Three ticks later the zapper shoots the glass shooter down with one shot of seven.
    for _ in 0..3 {
        tick_with(&mut w, &mut log, vec![]);
    }
    tick_with(&mut w, &mut log, vec![attack(zapper, glass)]);
    assert_eq!(w.hp_of(glass), Some(0));
    assert_eq!(w.projectile_count(), 1, "the shell flies on");
    until_impact(&mut w, &mut log, 60);
    let impact = log.only_impact();
    assert_eq!((impact.owner, impact.victim), (glass, Some(dummy)));
    assert!(impact.tick.0 > fired.0 + 4);
    assert_eq!(w.hp_of(dummy), Some(950));
    // The dead shooter fires no more, and the shell was the only one.
    let more = run(&mut w, 100);
    assert!(more.shots.iter().all(|s| s.attacker != glass));
}

#[test]
fn a_shell_in_flight_lands_when_its_shooter_is_gone() {
    let mut w = armed();
    let glass = spawn(&mut w, GLASS, at(10, 10));
    let dummy = spawn(&mut w, DUMMY, at(20, 10));
    let mut log = go(&mut w, vec![attack(glass, dummy)], 1);
    tick_with(&mut w, &mut log, vec![Command::Despawn { id: glass }]);
    assert!(!w.is_alive(glass));
    // A new unit takes the slot of the shooter; the shell is not its.
    spawn(&mut w, DUMMY, at(30, 30));
    let newcomer = EntityId::new(glass.index(), glass.generation() + 1);
    assert_eq!(w.hp_of(newcomer), Some(1000));
    until_impact(&mut w, &mut log, 60);
    let impact = log.only_impact();
    assert_eq!((impact.owner, impact.victim), (glass, Some(dummy)));
    assert_eq!(w.hp_of(dummy), Some(950));
    assert_eq!(w.hp_of(newcomer), Some(1000));
}

// ---- two passes -----------------------------------------------------------------------------

/// Two snappers (7 damage each) fire at one fragile target (10 hit points) from either side, both
/// at the same distance.
fn pincer() -> (World, EntityId, EntityId, EntityId) {
    let mut w = armed();
    let a = spawn(&mut w, SNAPPER, at(10, 10));
    let b = spawn(&mut w, SNAPPER, at(14, 10));
    let victim = spawn(&mut w, FRAGILE, at(12, 10));
    (w, a, b, victim)
}

#[test]
fn two_shells_that_land_on_one_tick_are_summed_and_saturate_at_zero() {
    let (mut w, a, b, victim) = pincer();
    let mut log = go(&mut w, vec![attack(a, victim), attack(b, victim)], 1);
    assert_eq!(log.shots.len(), 2);
    until_impact(&mut w, &mut log, 30);
    // Both went off on the same tick, both counted, and the hit points are the sum (14 of 10).
    assert_eq!(log.impacts.len(), 2, "{:?}", log.impacts);
    assert_eq!(log.impacts[0].tick, log.impacts[1].tick);
    assert!(
        log.impacts
            .iter()
            .all(|i| i.victim == Some(victim) && i.damage == 7)
    );
    assert_eq!(w.hp_of(victim), Some(0));
    // Neither impact reports the 3 that a running total would give the first: both report the end of
    // the tick.
    assert!(
        log.impacts.iter().all(|i| i.hp_after == 0),
        "{:?}",
        log.impacts
    );
}

#[test]
fn a_shell_and_an_instant_shot_that_land_on_one_tick_are_summed_too() {
    // A snapper's shell (7) and a zapper's instant shot (7) reach a victim of ten on the same tick.
    let build = || {
        let mut w = armed();
        let snapper = spawn(&mut w, SNAPPER, at(8, 10));
        let victim = spawn(&mut w, FRAGILE, at(12, 10));
        let zapper = spawn(&mut w, ZAPPER, at(12, 13));
        (w, snapper, victim, zapper)
    };
    // First find out on which tick the shell lands.
    let (mut probe, snapper, victim, _) = build();
    let mut log = go(&mut probe, vec![attack(snapper, victim)], 1);
    let fired = log.only_shot().tick;
    until_impact(&mut probe, &mut log, 30);
    let lands = log.only_impact().tick;
    // Then play it again with the zapper's first shot on that tick.
    let (mut w, snapper, victim, zapper) = build();
    let mut log = go(&mut w, vec![attack(snapper, victim)], 1);
    assert_eq!(log.only_shot().tick, fired);
    while w.tick().0 < lands.0 {
        tick_with(&mut w, &mut log, vec![]);
    }
    log.shots.clear();
    log.impacts.clear();
    tick_with(&mut w, &mut log, vec![attack(zapper, victim)]);
    let impact = log.only_impact();
    let zap = log.only_shot();
    assert_eq!((impact.tick, zap.tick), (lands, lands));
    assert_eq!(impact.victim, Some(victim));
    assert_eq!(w.hp_of(victim), Some(0));
    // Both report the end of the tick: nothing left, whichever was counted first.
    assert_eq!((impact.hp_after, zap.hp_after), (0, 0));
}

#[test]
fn a_shell_that_lands_on_a_victim_already_at_zero_is_a_miss() {
    // The snapper's shell takes seven ticks over four cells; the zapper takes the victim's ten hit
    // points down to zero in two shots, five ticks apart, and is done before the shell is there.
    let mut w = armed();
    let snapper = spawn(&mut w, SNAPPER, at(8, 10));
    let victim = spawn(&mut w, FRAGILE, at(12, 10));
    let zapper = spawn(&mut w, ZAPPER, at(12, 13));
    let mut log = go(
        &mut w,
        vec![attack(snapper, victim), attack(zapper, victim)],
        1,
    );
    let fired = log.shots[0].tick;
    for _ in 0..5 {
        tick_with(&mut w, &mut log, vec![]);
    }
    assert_eq!(w.hp_of(victim), Some(0));
    assert!(log.impacts.is_empty(), "the shell is still in the air");
    until_impact(&mut w, &mut log, 30);
    let impact = log.only_impact();
    assert!(impact.tick.0 > fired.0 + 5);
    assert_eq!(
        (impact.victim, impact.damage, impact.hp_after),
        (None, 0, 0)
    );
    assert_eq!(impact.cause, ImpactCause::Arrived);
}

#[test]
fn a_burst_launches_one_projectile_per_shot() {
    let mut w = armed();
    let twin = spawn(&mut w, TWINNER, at(10, 10));
    let dummy = spawn(&mut w, DUMMY, at(15, 10));
    let mut log = go(&mut w, vec![attack(twin, dummy)], 4);
    assert_eq!(log.shots.len(), 2);
    assert_eq!(log.shots[1].tick.0 - log.shots[0].tick.0, 3);
    assert_eq!(w.projectile_count(), 2);
    while w.projectile_count() > 0 {
        tick_with(&mut w, &mut log, vec![]);
    }
    assert_eq!(log.impacts.len(), 2);
    assert_eq!(log.impacts[1].tick.0 - log.impacts[0].tick.0, 3);
    assert_eq!(w.hp_of(dummy), Some(980));
}

#[test]
fn the_report_of_a_shot_does_not_depend_on_the_order_of_the_attackers() {
    let mut w = armed();
    let a = spawn(&mut w, ZAPPER, at(10, 10));
    let b = spawn(&mut w, ZAPPER, at(13, 10));
    let v = spawn(&mut w, DUMMY, at(11, 12));
    let mut log = Log::default();
    tick_with(&mut w, &mut log, vec![attack(a, v), attack(b, v)]);
    assert_eq!(log.shots.len(), 2);
    assert!(log.shots.iter().all(|s| s.hp_after == 1000 - 14));
}

// ---- the order of the slots does not decide anything ------------------------------------------------

/// What a skirmish came to, by the position of each unit (not by its slot).
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    hp: Vec<u32>,
    /// (tick, shooter, victim) of every shot, sorted.
    shots: Vec<(u64, usize, usize)>,
    /// (tick, owner, victim or none, cause) of every impact, sorted.
    impacts: Vec<(u64, usize, Option<usize>, ImpactCause)>,
    /// Where the projectiles are three ticks in, as (owner, x, y) raw, sorted.
    in_the_air: Vec<(usize, i64, i64)>,
}

const SPOTS: [(i32, i32); 4] = [(10, 10), (13, 10), (11, 13), (14, 13)];

/// Seven-hit-point snappers at the first `n` spots, spawned in `order`, with `links` as attack orders
/// given on one tick, then run for 40 ticks.
/// [`skirmish`] with the units at `spots`.
fn skirmish_at(
    spots: &[(i32, i32)],
    n: usize,
    links: &[(usize, usize)],
    order: &[usize],
) -> (Outcome, u64) {
    let mut w = armed();
    let mut ids = vec![EntityId::new(0, 0); n];
    for &p in order {
        ids[p] = spawn(&mut w, SNAPPER, at(spots[p].0, spots[p].1));
    }
    let pos_of = |id: EntityId| ids.iter().position(|&i| i == id).unwrap();
    let mut log = Log::default();
    let commands = links.iter().map(|&(a, b)| attack(ids[a], ids[b])).collect();
    tick_with(&mut w, &mut log, commands);
    for _ in 0..2 {
        tick_with(&mut w, &mut log, vec![]);
    }
    let mut in_the_air: Vec<_> = w
        .snapshot()
        .projectiles()
        .map(|p| (pos_of(p.owner), p.pos.x.raw(), p.pos.y.raw()))
        .collect();
    in_the_air.sort_unstable();
    for _ in 0..37 {
        tick_with(&mut w, &mut log, vec![]);
    }
    let mut shots: Vec<_> = log
        .shots
        .iter()
        .map(|s| (s.tick.0, pos_of(s.attacker), pos_of(s.target)))
        .collect();
    shots.sort_unstable();
    let mut impacts: Vec<_> = log
        .impacts
        .iter()
        .map(|i| (i.tick.0, pos_of(i.owner), i.victim.map(pos_of), i.cause))
        .collect();
    impacts.sort_unstable_by_key(|i| (i.0, i.1, i.2, i.3 as u8));
    let hp = ids.iter().map(|&i| w.hp_of(i).unwrap()).collect();
    (
        Outcome {
            hp,
            shots,
            impacts,
            in_the_air,
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

/// Runs the skirmish in every spawn order and expects one outcome, and the same checksum every time
/// the same order is played again.
fn in_every_order(n: usize, links: &[(usize, usize)]) -> Outcome {
    in_every_order_at(&SPOTS, n, links)
}

fn in_every_order_at(spots: &[(i32, i32)], n: usize, links: &[(usize, usize)]) -> Outcome {
    let mut seen: Option<Outcome> = None;
    for order in permutations(n) {
        let (outcome, checksum) = skirmish_at(spots, n, links, &order);
        assert_eq!(
            skirmish_at(spots, n, links, &order).1,
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
fn two_units_that_kill_each_other_with_shells_both_die() {
    let o = in_every_order(2, &[(0, 1), (1, 0)]);
    assert_eq!(o.hp, [0, 0]);
    assert_eq!(o.impacts.len(), 2);
    // Both shells flew at once and both landed on the same tick.
    assert_eq!(o.impacts[0].0, o.impacts[1].0);
    assert!(o.impacts.iter().all(|i| i.3 == ImpactCause::Proximity));
    assert_eq!(o.in_the_air.len(), 2);
    // Each fired once in the 40 ticks and then nobody was left to fire.
    assert_eq!(o.shots.len(), 2);
}

#[test]
fn two_shooters_that_overkill_one_victim_with_shells_both_fire_and_the_late_shell_finds_it_down() {
    // Both shells fly at once. The victim is 3.2 cells from one and 3.6 from the other, so one lands
    // a tick before the other: it takes the seven hit points down, and the late one lands on a victim
    // that was at zero at the start of its tick, a miss. Whichever was spawned first.
    let o = in_every_order(3, &[(0, 2), (1, 2)]);
    assert_eq!(o.hp, [7, 7, 0]);
    assert_eq!(o.shots.len(), 2);
    assert_eq!(o.impacts.len(), 2);
    assert_eq!(
        o.impacts.iter().map(|i| i.2).collect::<Vec<_>>(),
        [Some(2), None]
    );
    assert!(o.impacts[0].0 < o.impacts[1].0);
}

#[test]
fn two_shells_that_land_together_on_a_victim_one_of_them_kills_both_count() {
    // Both shooters are two cells from a victim with seven hit points, so the shells land on the same
    // tick, and either alone would kill it. Neither is a miss, whichever was launched first, and both
    // report that the victim is at zero.
    let spots = [(10, 10), (14, 10), (12, 10)];
    let o = in_every_order_at(&spots, 3, &[(0, 2), (1, 2)]);
    assert_eq!(o.hp, [7, 7, 0]);
    assert_eq!(o.impacts.len(), 2, "{o:?}");
    assert_eq!(o.impacts[0].0, o.impacts[1].0);
    assert!(o.impacts.iter().all(|i| i.2 == Some(2)), "{o:?}");
}

#[test]
fn a_ring_of_three_kills_all_three_with_shells() {
    let o = in_every_order(3, &[(0, 1), (1, 2), (2, 0)]);
    assert_eq!(o.hp, [0, 0, 0]);
    assert_eq!(o.impacts.len(), 3);
    assert!(o.impacts.iter().all(|i| i.2.is_some()));
}

#[test]
fn a_unit_that_is_shot_down_while_its_own_shell_flies_still_lands_it() {
    // 0 shoots 1 and 1 shoots 2 on the same tick: 1 is dead before its shell arrives, 2 dies of it.
    let o = in_every_order(3, &[(0, 1), (1, 2)]);
    assert_eq!(o.hp, [7, 0, 0]);
    assert_eq!(o.impacts.len(), 2);
}

#[test]
fn four_units_in_a_tangle_come_out_the_same_in_every_spawn_order() {
    // 0 and 1 shoot 2 (0's second order replaces its first), 2 shoots 3, and 3 is out of reach of 0 and
    // walks toward it. 2 and 3 are dead before 3 is close enough; 1's shell is the late one.
    let o = in_every_order(4, &[(0, 1), (1, 2), (2, 3), (3, 0), (0, 2)]);
    assert_eq!(o.hp, [7, 7, 0, 0]);
    assert_eq!(o.shots.len(), 3);
    assert_eq!(o.impacts.len(), 3, "{o:?}");
    assert_eq!(o.impacts.iter().filter(|i| i.2.is_none()).count(), 1);
}

/// Volleys that overlap in the air, so that slots are freed and reused while others still fly: a twin
/// cannon (two shells three ticks apart, a reload of twenty, ten ticks in the air) fires at two dummies,
/// and a cannoneer joins in halfway. Played in every spawn order.
/// (tick, owner, victim) of every impact, sorted; the hit points at the end; the most projectiles in the
/// air at once.
type VolleyOutcome = (Vec<(u64, usize, Option<usize>)>, Vec<u32>, usize);

fn volleys(order: &[usize]) -> VolleyOutcome {
    const KINDS: [Kind; 4] = [TWINNER, CANNONEER, DUMMY, DUMMY];
    const WHERE: [(i32, i32); 4] = [(10, 10), (10, 14), (16, 10), (10, 20)];
    let mut w = armed();
    let mut ids = [EntityId::new(0, 0); 4];
    for &p in order {
        ids[p] = spawn(&mut w, KINDS[p], at(WHERE[p].0, WHERE[p].1));
    }
    let pos_of = |id: EntityId| ids.iter().position(|&i| i == id).unwrap();
    let mut log = Log::default();
    tick_with(&mut w, &mut log, vec![attack(ids[0], ids[2])]);
    let mut most = 0;
    for t in 1..150 {
        let cmds = if t == 15 {
            vec![attack(ids[1], ids[3])]
        } else if t == 70 {
            vec![attack(ids[0], ids[3])]
        } else {
            vec![]
        };
        tick_with(&mut w, &mut log, cmds);
        most = most.max(w.projectile_count());
    }
    let mut impacts: Vec<_> = log
        .impacts
        .iter()
        .map(|i| (i.tick.0, pos_of(i.owner), i.victim.map(pos_of)))
        .collect();
    impacts.sort_unstable();
    let hp = ids.iter().map(|&i| w.hp_of(i).unwrap()).collect();
    (impacts, hp, most)
}

#[test]
fn overlapping_volleys_with_reused_slots_come_out_the_same_in_every_spawn_order() {
    let mut seen = None;
    let mut most = 0;
    for order in permutations(4) {
        let (impacts, hp, m) = volleys(&order);
        most = most.max(m);
        match &seen {
            Some(first) => assert_eq!(&(impacts, hp), first, "spawn order {order:?}"),
            None => seen = Some((impacts, hp)),
        }
    }
    let (impacts, hp) = seen.unwrap();
    // Several shells were in the air together, and every one hit.
    assert!(most >= 3, "{most}");
    assert!(impacts.len() >= 8, "{impacts:?}");
    assert!(impacts.iter().all(|i| i.2.is_some()), "{impacts:?}");
    assert!(hp[2] < 1000 && hp[3] < 1000);
}

// ---- the snapshot ---------------------------------------------------------------------------

#[test]
fn the_snapshot_lists_the_projectiles_in_flight() {
    let (mut w, shooter, dummy) = straight_duel(8);
    assert_eq!(w.snapshot().projectile_count(), 0);
    assert_eq!(w.snapshot().projectiles().count(), 0);
    let mut log = go(&mut w, vec![attack(shooter, dummy)], 1);
    let first = w.snapshot().projectiles().next().unwrap();
    assert_eq!(first.owner, shooter);
    assert_eq!(first.target, Some(dummy));
    assert_eq!(
        (first.weapon, first.kind, first.player),
        (CANNON, ProjectileKind::Straight, PlayerId(0))
    );
    assert_eq!(first.pos, first.prev_pos);
    assert_eq!(first.height, Fx::ZERO);
    assert_eq!(first.aim, w.get(dummy).unwrap().pos);
    assert_eq!(w.projectile(first.id), Some(first));
    // It moves half a cell a tick toward the target, and the previous place is the one before.
    tick_with(&mut w, &mut log, vec![]);
    let next = w.snapshot().projectiles().next().unwrap();
    assert_eq!(next.id, first.id);
    assert_eq!(next.prev_pos, first.pos);
    assert_eq!((next.pos - next.prev_pos).length(), half());
    assert_eq!(next.heading, Facing::PLUS_X);
    let mut out = Vec::new();
    w.snapshot().collect_projectiles_into(&mut out);
    assert_eq!(out, vec![next]);
    // When it goes off it leaves the list.
    until_impact(&mut w, &mut log, 40);
    assert_eq!(w.snapshot().projectiles().count(), 0);
    assert_eq!(w.projectile(first.id), None);
    // The cannoneer reloads and fires again: the same slot, another generation, so a cache keyed by
    // the id does not mistake the new shell for the old one.
    let second = loop {
        tick_with(&mut w, &mut log, vec![]);
        if let Some(p) = w.snapshot().projectiles().next() {
            break p;
        }
    };
    assert_eq!(second.id.index(), first.id.index());
    assert_eq!(second.id.generation(), first.id.generation() + 1);
    assert_eq!(w.projectile(first.id), None);
}

// ---- the checksum ---------------------------------------------------------------------------

#[test]
fn a_shell_in_the_air_is_part_of_the_checksum_and_a_clone_goes_on_the_same() {
    let build = |fire: bool| {
        let (mut w, shooter, dummy) = straight_duel(8);
        let cmds = if fire {
            vec![attack(shooter, dummy)]
        } else {
            vec![]
        };
        let mut log = Log::default();
        tick_with(&mut w, &mut log, cmds);
        for _ in 0..4 {
            tick_with(&mut w, &mut log, vec![]);
        }
        w
    };
    let (mut a, mut b) = (build(true), build(false));
    assert_eq!(a.projectile_count(), 1);
    assert_ne!(a.checksum(), b.checksum());
    // A clone taken with a shell in the air carries it along: same checksums, same impacts.
    let mut c = a.clone();
    assert_eq!(a.checksum(), c.checksum());
    let la = run(&mut a, 40);
    let lc = run(&mut c, 40);
    assert!(!la.impacts.is_empty());
    assert_eq!(la.impacts, lc.impacts);
    assert_eq!(a.checksum_history(), c.checksum_history());
    run(&mut b, 40);
    assert_ne!(a.checksum(), b.checksum());
}

#[test]
fn a_run_with_projectiles_repeats_bit_for_bit() {
    let play = || {
        let mut w = armed();
        let a = spawn(&mut w, LOBBER, at(10, 10));
        let b = spawn(&mut w, TWINNER, at(10, 16));
        let c = spawn(&mut w, DUMMY, at(18, 12));
        let d = spawn(&mut w, DUMMY, at(18, 18));
        let mut log = go(
            &mut w,
            vec![attack(a, c), attack(b, d), walk(d, at(25, 30))],
            1,
        );
        for _ in 0..200 {
            tick_with(&mut w, &mut log, vec![]);
        }
        (w.checksum_history().to_vec(), log.impacts)
    };
    let (ha, ia) = play();
    let (hb, ib) = play();
    assert_eq!(ha, hb);
    assert_eq!(ia, ib);
    assert!(ia.len() >= 6);
}

#[test]
fn a_world_without_a_weapon_table_has_no_projectiles_and_hashes_as_before() {
    let mut plain = World::new(WorldConfig::new(3).with_map_size(MAP, MAP))
        .unwrap()
        .with_units(units())
        .unwrap();
    let mut log = Log::default();
    tick_with(&mut plain, &mut log, vec![]);
    assert_eq!(plain.projectile_count(), 0);
    assert!(plain.impacts().is_empty());
    assert_eq!(plain.snapshot().projectiles().count(), 0);
    // The projectile section is there for a world with a table, even with nothing in the air.
    let mut with = armed();
    tick_with(&mut with, &mut log, vec![]);
    assert_ne!(plain.checksum(), with.checksum());
}
