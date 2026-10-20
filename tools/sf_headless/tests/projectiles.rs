//! The `projectiles` scenario (WP5.2, `docs/rules/projectiles.md`) and the shipped weapons: a weapon
//! whose projectile is not `Instant` launches a projectile that flies and goes off, and the hit points
//! come off then. Asserted along the scenario's groups: a straight shell's flight is the distance over
//! the speed, an arcing shell lands where the target stood (a miss on a walking target, a hit on a
//! standing one), a guided rocket hits a zigzagging transport, a falling bomb takes the drop's time and
//! can be walked out from under, a shell in the air lands when its shooter has been shot down, and two
//! rocket throwers shoot each other down on the same tick. The second half of the file runs the
//! **shipped** definitions (`assets/config`): every weapon that flies hits a standing target at its full
//! range from every launch heading the firing arc allows, and the rocket hits a target that walks
//! across its line of fire. The checksum of the scenario is pinned in
//! `assets/scenarios/projectiles.expected` (see `pinned.rs`).
//!
//! The cast and the commands are written in `assets/scenarios/projectiles.ron`, which also lists the
//! entity ids used below.

mod common;

use common::scenario_path;
use sf_bridge::{unit_table, weapon_table};
use sf_data::units::{Category, UnitDefs};
use sf_headless::scenario::Scenario;
use sf_math::{Fx, Vec2};
use sf_sim::{
    Command, EntityId, HIT_SLACK, Impact, ImpactCause, Order, PlayerId, ProjectileKind, Shot, Tick,
    World, WorldConfig,
};

const TA: EntityId = EntityId::new(0, 0);
const LA: EntityId = EntityId::new(1, 0);
const SB: EntityId = EntityId::new(2, 0);
const AB: EntityId = EntityId::new(3, 0);
const TC: EntityId = EntityId::new(4, 0);
const RC: EntityId = EntityId::new(5, 0);
const TD1: EntityId = EntityId::new(6, 0);
const BD1: EntityId = EntityId::new(7, 0);
const TD2: EntityId = EntityId::new(8, 0);
const BD2: EntityId = EntityId::new(9, 0);
const TE: EntityId = EntityId::new(10, 0);
const BE: EntityId = EntityId::new(11, 0);
const SE: EntityId = EntityId::new(12, 0);
const RF1: EntityId = EntityId::new(13, 0);
const RF2: EntityId = EntityId::new(14, 0);

/// The kind numbers of the weapons in the frozen definitions.
const ROCKET: u16 = 1;
const CANNON: u16 = 6;
const HOWITZER: u16 = 8;
const BOMB: u16 = 15;

/// A shot with where the shooter and the target stood at the end of its tick (what the weapon saw).
#[derive(Copy, Clone, Debug)]
struct Fired {
    shot: Shot,
    shooter: Vec2,
    target: Vec2,
    /// Whether the target had moved during the tick of the shot.
    target_moving: bool,
}

struct Run {
    world: World,
    fired: Vec<Fired>,
    impacts: Vec<Impact>,
    most_in_the_air: usize,
}

fn run_scenario(scenario: &Scenario) -> Run {
    let mut world = scenario.build_world().unwrap();
    let mut fired = Vec::new();
    let mut impacts = Vec::new();
    let mut most_in_the_air = 0;
    for _ in 0..scenario.ticks {
        world.step();
        for shot in world.shots() {
            let shooter = world.get(shot.attacker).unwrap().pos;
            let target = world.get(shot.target).unwrap();
            fired.push(Fired {
                shot: *shot,
                shooter,
                target: target.pos,
                target_moving: target.pos != target.prev_pos,
            });
        }
        impacts.extend_from_slice(world.impacts());
        most_in_the_air = most_in_the_air.max(world.projectile_count());
    }
    Run {
        world,
        fired,
        impacts,
        most_in_the_air,
    }
}

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("projectiles.ron")).unwrap()
}

fn run() -> Run {
    run_scenario(&scenario())
}

impl Run {
    fn shots_by(&self, attacker: EntityId) -> Vec<Fired> {
        self.fired
            .iter()
            .copied()
            .filter(|f| f.shot.attacker == attacker)
            .collect()
    }

    fn impacts_of(&self, owner: EntityId) -> Vec<Impact> {
        self.impacts
            .iter()
            .copied()
            .filter(|i| i.owner == owner)
            .collect()
    }

    fn hp(&self, id: EntityId) -> u32 {
        self.world.get(id).expect("alive").hp
    }

    fn speed_of(&self, weapon: u16) -> Fx {
        self.world
            .weapon_table()
            .weapon(weapon)
            .unwrap()
            .projectile_speed_per_tick
    }

    fn radius_of(&self, id: EntityId) -> Fx {
        self.world.stats_of(id).unwrap().size.div_int(2)
    }
}

fn ticks_to_cover(distance: Fx, speed: Fx) -> u64 {
    (distance.raw().max(0) as u64).div_ceil(speed.raw() as u64)
}

/// Shots and the impacts that came of them, pairwise in order (one shell is in the air at a time for
/// these weapons: the reload is longer than the flight).
fn pairs(r: &Run, owner: EntityId) -> Vec<(Fired, Impact)> {
    let (shots, impacts) = (r.shots_by(owner), r.impacts_of(owner));
    assert!(shots.len() >= impacts.len(), "{owner:?}");
    for (f, i) in shots.iter().zip(&impacts) {
        assert!(i.tick > f.shot.tick, "{f:?} {i:?}");
    }
    shots.into_iter().zip(impacts).collect()
}

#[test]
fn a_straight_shell_takes_the_distance_over_the_speed_to_arrive() {
    let r = run();
    let shells = pairs(&r, LA);
    // Every shot of the light tank that was fired early enough has landed, on the tank, at 35.
    assert_eq!(shells.len(), 11);
    assert_eq!(r.shots_by(LA).len(), 11);
    let speed = r.speed_of(CANNON);
    assert_eq!(speed, Fx::from_ratio(3, 2));
    let reach = r.radius_of(TA) + HIT_SLACK;
    for (f, i) in &shells {
        assert_eq!(
            (i.victim, i.cause, i.damage),
            (Some(TA), ImpactCause::Proximity, 35)
        );
        let d = (f.target - f.shooter).length();
        let flight = i.tick.0 - f.shot.tick.0;
        assert_eq!(flight, ticks_to_cover(d - reach, speed), "{f:?}");
        // Within a tick or two of the plain quotient of the distance and the speed, as the data says.
        let plain = ticks_to_cover(d, speed);
        assert!(
            flight <= plain && plain - flight <= 2,
            "{flight} against {plain}"
        );
    }
    // The tank lost 35 for every shell that landed, and nothing for the one in the air.
    assert_eq!(r.hp(TA), 900 - 35 * 11);
    // The launch reports what the target had then, not what the shell will do.
    let first = r.shots_by(LA)[0].shot;
    assert_eq!((first.tick.0, first.hp_after), (43, 900));
}

#[test]
fn an_arcing_shell_lands_where_the_target_stood_and_misses_one_that_is_walking() {
    let r = run();
    let shells = pairs(&r, AB);
    assert_eq!(shells.len(), 3);
    let speed = r.speed_of(HOWITZER);
    for (f, i) in &shells {
        // It lands at the aim point, after the ground distance over the speed.
        assert_eq!(i.at, f.target, "{f:?}");
        let d = (f.target - f.shooter).length();
        assert_eq!(i.tick.0 - f.shot.tick.0, ticks_to_cover(d, speed), "{f:?}");
        assert_eq!(i.cause, ImpactCause::Arrived);
    }
    // The first two were fired at a scout on the move and land where it was: nothing there.
    for (f, i) in &shells[..2] {
        assert!(f.target_moving, "{f:?}");
        assert_eq!((i.victim, i.damage), (None, 0), "{i:?}");
        let scout_then = r.world.get(SB).unwrap().pos;
        assert_ne!(i.at, scout_then);
    }
    // The third is fired when the scout has stopped, and kills it (90 against 25).
    let (f, i) = &shells[2];
    assert!(!f.target_moving);
    assert_eq!((i.victim, i.hp_after), (Some(SB), 0));
    assert_eq!(r.hp(SB), 0);
    // The scout is down, so the howitzer fires no more.
    assert_eq!(r.shots_by(AB).len(), 3);
}

#[test]
fn a_guided_rocket_hits_a_transport_that_zigzags() {
    let r = run();
    let rockets = pairs(&r, RC);
    // The transport (250) takes six rockets of 45 and is down; the thrower stops there.
    assert_eq!(rockets.len(), 6);
    assert_eq!(r.shots_by(RC).len(), 6);
    let mut left = 250;
    for (f, i) in &rockets {
        left -= 45.min(left);
        assert_eq!(
            (i.victim, i.cause, i.hp_after),
            (Some(TC), ImpactCause::Proximity, left),
            "{f:?}"
        );
    }
    assert_eq!(r.hp(TC), 0);
    // It was not standing still: it was on the move when most of the rockets were launched.
    let moving = rockets.iter().filter(|(f, _)| f.target_moving).count();
    assert!(moving >= 4, "{moving} of 6");
    // A rocket takes a few ticks, not the instant of a rifle.
    assert!(rockets.iter().all(|(f, i)| i.tick.0 > f.shot.tick.0));
}

#[test]
fn a_falling_bomb_takes_the_drop_to_land_and_can_be_walked_out_from_under() {
    let r = run();
    let weapon = r.world.weapon_table().weapon(BOMB).unwrap();
    let drop = ticks_to_cover(weapon.flight_height, weapon.projectile_speed_per_tick);
    assert_eq!(
        drop, 16,
        "three cells at a fifth of a cell a tick, rounded up"
    );
    // The bomb team shelling the tank that stands still: every bomb lands on it, at the place it stood.
    let standing = pairs(&r, BD1);
    assert_eq!(standing.len(), 8);
    for (f, i) in &standing {
        assert_eq!(i.tick.0 - f.shot.tick.0, drop);
        assert_eq!(
            (i.victim, i.at, i.cause),
            (Some(TD1), f.target, ImpactCause::Arrived)
        );
    }
    assert_eq!(r.hp(TD1), 900 - 60 * 8);
    // The other team fires its first bomb at a transport that is driving off. The bomb falls where the
    // transport was.
    let shelling = pairs(&r, BD2);
    let (f, i) = shelling[0];
    assert!(f.target_moving);
    assert_eq!((i.victim, i.at), (None, f.target));
    assert_ne!(r.world.get(TD2).unwrap().pos, i.at);
    // Once the transport has stopped every bomb hits it, until it is down (250: five of 60).
    let rest = &shelling[1..];
    assert!(
        rest.iter()
            .all(|(f, i)| !f.target_moving && i.victim == Some(TD2))
    );
    assert_eq!(rest.len(), 5);
    assert_eq!(r.hp(TD2), 0);
}

#[test]
fn a_shell_in_the_air_lands_when_its_shooter_has_been_shot_down() {
    let r = run();
    // The bomb team BE drops its bomb at tick 1; the sniper's 40 points take its 30 at tick 6.
    let bomb = r.shots_by(BE);
    assert_eq!(bomb.len(), 1);
    assert_eq!(bomb[0].shot.tick, Tick(1));
    let sniper = r.shots_by(SE);
    assert_eq!(
        (
            sniper[0].shot.tick,
            sniper[0].shot.target,
            sniper[0].shot.hp_after
        ),
        (Tick(6), BE, 0)
    );
    assert_eq!(r.hp(BE), 0);
    // The bomb is in the air from tick 1 to tick 17, and it lands on the tank all the same.
    let landed = r.impacts_of(BE);
    assert_eq!(landed.len(), 1);
    assert_eq!(landed[0].tick, Tick(17));
    assert!(landed[0].tick > sniper[0].shot.tick);
    assert_eq!((landed[0].victim, landed[0].hp_after), (Some(TE), 900 - 60));
    // The dead team does not fire again, though its order stands.
    assert_eq!(r.world.order_of(BE), Some(Order::Attack(TE)));
    // Only the one bomb: the sniper shot once at a unit that stayed down.
    assert_eq!(r.shots_by(BE).len(), 1);
    assert_eq!(r.hp(TE), 900 - 60);
}

#[test]
fn two_rocket_throwers_shoot_each_other_down_on_the_same_tick() {
    let r = run();
    let (a, b) = (r.impacts_of(RF1), r.impacts_of(RF2));
    assert_eq!((a.len(), b.len()), (1, 1));
    // Each fired once, on the first tick, and each rocket killed its target (45 against 45) on the
    // same tick as the other: neither shooter got to stop the other's rocket.
    assert_eq!(r.shots_by(RF1).len(), 1);
    assert_eq!(r.shots_by(RF2).len(), 1);
    assert_eq!(a[0].tick, b[0].tick);
    assert_eq!((a[0].victim, b[0].victim), (Some(RF2), Some(RF1)));
    assert_eq!((r.hp(RF1), r.hp(RF2)), (0, 0));
    assert!(a[0].tick.0 > r.shots_by(RF1)[0].shot.tick.0);
}

#[test]
fn the_scenario_has_several_projectiles_in_the_air_together_and_leaves_none_behind_for_long() {
    let r = run();
    assert!(r.most_in_the_air >= 4, "{}", r.most_in_the_air);
    // Every kind took part.
    let kinds: Vec<ProjectileKind> = [CANNON, HOWITZER, ROCKET, BOMB]
        .iter()
        .map(|&w| r.world.weapon_table().weapon(w).unwrap().projectile)
        .collect();
    assert_eq!(
        kinds,
        [
            ProjectileKind::Straight,
            ProjectileKind::Arc,
            ProjectileKind::Guided,
            ProjectileKind::Falling
        ]
    );
    for weapon in [CANNON, HOWITZER, ROCKET, BOMB] {
        assert!(
            r.impacts.iter().any(|i| i.weapon == weapon),
            "weapon {weapon}"
        );
    }
}

#[test]
fn the_same_scenario_without_combat_fires_nothing() {
    let mut quiet = scenario();
    quiet.combat = false;
    quiet.weapon_table = None;
    let r = run_scenario(&quiet);
    assert!(r.fired.is_empty() && r.impacts.is_empty());
    assert_eq!(r.most_in_the_air, 0);
    assert_eq!(r.hp(TA), 900);
}

#[test]
fn the_run_is_the_same_every_time() {
    let (a, b) = (run(), run());
    assert_eq!(a.world.checksum_history(), b.world.checksum_history());
    assert_eq!(a.impacts, b.impacts);
    assert_eq!(a.world.tick(), Tick(480));
}

// ---- the shipped definitions -----------------------------------------------------------------

/// A world with the shipped units and weapons, on an open 64 by 64 map.
fn shipped_world(seed: u64) -> (World, UnitDefs) {
    let defs = UnitDefs::embedded();
    let world = World::new(WorldConfig::new(seed).with_map_size(64, 64))
        .unwrap()
        .with_units(unit_table(&defs).unwrap())
        .unwrap()
        .with_weapons(weapon_table(&defs).unwrap())
        .unwrap();
    (world, defs)
}

fn name(defs: &UnitDefs, kind: u16) -> String {
    defs.units()
        .iter()
        .find(|u| u.kind == kind)
        .map_or_else(|| format!("kind {kind}"), |u| u.id.clone())
}

fn send(world: &mut World, command: Command) {
    let tick = world.tick();
    world.schedule(tick, PlayerId(0), command).unwrap();
}

/// Steps until the first impact, at most `limit` ticks.
fn first_impact(world: &mut World, limit: u32) -> Option<Impact> {
    for _ in 0..limit {
        world.step();
        if let Some(i) = world.impacts().first() {
            return Some(*i);
        }
    }
    None
}

#[test]
fn every_shipped_weapon_that_flies_hits_a_standing_target_at_its_full_range() {
    let defs = UnitDefs::embedded();
    let table = weapon_table(&defs).unwrap();
    let tank = defs.kind_of("heavy_tank").unwrap();
    let mut tried = Vec::new();
    for role in defs.sim_units() {
        let Some(&first) = role.weapons.first() else {
            continue;
        };
        let weapon = table.weapon(first).unwrap();
        // Fire from a unit of every role that carries a flying weapon first, except a vessel (it
        // needs water; the weapon is the same straight shell as the light tank's) and a structure (it
        // is immobile and takes no order, so it fires only when targets are chosen for it: WP5.4 and
        // WP6.7; its weapon is the same straight shell as well).
        if weapon.projectile == ProjectileKind::Instant
            || role.category == Category::Vessel
            || role.footprint.is_some()
        {
            continue;
        }
        // Every heading the unit may start with: the world's seed decides it.
        for seed in 0..24 {
            let (mut world, _) = shipped_world(seed);
            let reach = weapon.range + Fx::from_ratio(3, 4); // the heavy tank's radius
            let shooter_at = Vec2::new(Fx::from_int(20), Fx::from_int(20));
            let target_at = shooter_at + Vec2::new(reach, Fx::ZERO);
            let shooter = EntityId::new(0, 0);
            let target = EntityId::new(1, 0);
            send(
                &mut world,
                Command::SpawnUnit {
                    kind: sf_sim::Kind(role.kind),
                    pos: shooter_at,
                    owner: PlayerId(0),
                },
            );
            send(
                &mut world,
                Command::SpawnUnit {
                    kind: sf_sim::Kind(tank),
                    pos: target_at,
                    owner: PlayerId(1),
                },
            );
            world.step();
            send(
                &mut world,
                Command::Order {
                    ids: vec![shooter],
                    order: Order::Attack(target),
                    queued: false,
                },
            );
            let impact = first_impact(&mut world, 400).unwrap_or_else(|| {
                panic!("{} (seed {seed}): nothing went off", name(&defs, role.kind))
            });
            assert_eq!(
                impact.victim,
                Some(target),
                "{} (seed {seed}): the first shell missed a target that stands still: {impact:?}",
                name(&defs, role.kind)
            );
        }
        tried.push(first);
    }
    // The light tank, the heavy tank, the artillery, the rocket thrower and the gun team: every
    // shipped weapon that flies on a unit, but the gunboat's.
    tried.sort_unstable();
    tried.dedup();
    assert_eq!(tried.len(), 5, "{tried:?}");
}

#[test]
fn the_shipped_rocket_hits_a_transport_that_walks_across_its_line_of_fire() {
    // The fastest unit of the shipped set, a light transport at 2.5 cells a second, walks off across the
    // line from the moment the rocket thrower fires, at every distance up to the rocket's full range.
    let defs = UnitDefs::embedded();
    let trooper = defs.kind_of("rocket_trooper").unwrap();
    let transport = defs.kind_of("light_transport").unwrap();
    let mut hits = 0;
    for gap in [1, 2, 3, 4, 5] {
        for seed in 0..24 {
            let (mut world, _) = shipped_world(seed);
            let at = |x: i32, y: i32| Vec2::from_ints(x, y);
            send(
                &mut world,
                Command::SpawnUnit {
                    kind: sf_sim::Kind(trooper),
                    pos: at(20, 20),
                    owner: PlayerId(0),
                },
            );
            send(
                &mut world,
                Command::SpawnUnit {
                    kind: sf_sim::Kind(transport),
                    pos: at(20 + gap, 20),
                    owner: PlayerId(1),
                },
            );
            world.step();
            let (shooter, runner) = (EntityId::new(0, 0), EntityId::new(1, 0));
            send(
                &mut world,
                Command::Order {
                    ids: vec![shooter],
                    order: Order::Attack(runner),
                    queued: false,
                },
            );
            // Wait for the launch, then the transport breaks away across the line.
            let mut launched = false;
            for _ in 0..30 {
                world.step();
                if !world.shots().is_empty() {
                    launched = true;
                    break;
                }
            }
            assert!(launched, "gap {gap} seed {seed}");
            send(
                &mut world,
                Command::Order {
                    ids: vec![runner],
                    order: Order::Move(at(20 + gap, 50)),
                    queued: false,
                },
            );
            let impact = first_impact(&mut world, 60).expect("it goes off");
            assert_eq!(
                impact.victim,
                Some(runner),
                "gap {gap} seed {seed}: {impact:?}"
            );
            hits += 1;
        }
    }
    assert_eq!(hits, 5 * 24);
}
