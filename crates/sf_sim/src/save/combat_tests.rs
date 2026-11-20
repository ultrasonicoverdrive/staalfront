//! Tests of the sections WP13.1c-1 saves: the weapon slots, the projectiles, the engagement memory,
//! the wrecks, the tally and the kills, and the path service's epoch rule for a structure destroyed
//! after the service (`docs/rules/save.md`). The world is a small battle on a walled map with the
//! smallest path budget: every projectile kind, a burst weapon, units that choose their targets, a
//! defence tower, structures shot down while routes are being planned, and units that walk long
//! detours the whole time.

use sf_math::{Facing, Fx, Vec2};

use super::tests::{sections, settled};
use super::*;
use crate::destruction::{DestructionConfig, Tally, WreckKind};
use crate::movement::{ClassCosts, Cost, MovementTable};
use crate::orders::{GuardTarget, Order};
use crate::projectiles::Target;
use crate::targeting::TargetingConfig;
use crate::terrain::{Terrain, TerrainDef, TerrainFlags, TerrainId};
use crate::units::{Footprint, UnitStats, UnitTable};
use crate::weapons::{Loadout, Mount, ProjectileKind, WeaponStats, WeaponTable};
use crate::world::{World, WorldConfig};

const RIFLE: Kind = Kind(10);
const TANK: Kind = Kind(11);
const ARTY: Kind = Kind(12);
const ROCKET: Kind = Kind(13);
const MORTAR: Kind = Kind(14);
const WALKER: Kind = Kind(15);
const TOWER: Kind = Kind(20);
const HQ: Kind = Kind(21);

const GUN: u16 = 1;
const CANNON: u16 = 2;
const HOWITZER: u16 = 3;
const MISSILE: u16 = 4;
const SHELL: u16 = 5;
const SIDEARM: u16 = 6;

const W: u32 = 64;
const H: u32 = 48;
const TICKS: u64 = 700;
/// The walkers that keep the path service busy.
const WALKERS: usize = 18;

fn weapon(kind: u16, projectile: ProjectileKind, range: i32) -> WeaponStats {
    WeaponStats {
        kind,
        range: Fx::from_int(range),
        min_range: Fx::ZERO,
        reload_ticks: 30,
        burst: 1,
        burst_delay_ticks: 0,
        damage: 12,
        damage_type: 0,
        projectile,
        projectile_speed_per_tick: Fx::from_ratio(1, 2),
        flight_height: match projectile {
            ProjectileKind::Arc => Fx::from_int(3),
            ProjectileKind::Falling => Fx::from_int(4),
            _ => Fx::ZERO,
        },
        guide_turn_per_tick: if projectile == ProjectileKind::Guided {
            12
        } else {
            0
        },
        splash_radius: Fx::ZERO,
        splash_inner: Fx::ZERO,
        friendly_fire: false,
        mount: Mount::Hull,
        fire_arc: 128,
    }
}

fn weapons() -> Vec<WeaponStats> {
    vec![
        // A burst of three, four ticks apart.
        WeaponStats {
            burst: 3,
            burst_delay_ticks: 4,
            damage: 6,
            ..weapon(GUN, ProjectileKind::Instant, 5)
        },
        WeaponStats {
            mount: Mount::Turret,
            splash_radius: Fx::ONE,
            ..weapon(CANNON, ProjectileKind::Straight, 7)
        },
        WeaponStats {
            reload_ticks: 50,
            damage: 30,
            ..weapon(HOWITZER, ProjectileKind::Arc, 12)
        },
        weapon(MISSILE, ProjectileKind::Guided, 8),
        WeaponStats {
            reload_ticks: 40,
            damage: 20,
            ..weapon(SHELL, ProjectileKind::Falling, 9)
        },
        weapon(SIDEARM, ProjectileKind::Instant, 3),
    ]
}

fn loadouts() -> Vec<Loadout> {
    let l = |unit, weapons: &[u16]| Loadout {
        unit,
        weapons: weapons.to_vec(),
    };
    vec![
        l(RIFLE, &[GUN]),
        l(TANK, &[CANNON]),
        l(ARTY, &[HOWITZER, SIDEARM]),
        l(ROCKET, &[MISSILE]),
        l(MORTAR, &[SHELL]),
        l(TOWER, &[GUN]),
    ]
}

fn mobile(kind: Kind, range: i32, size: Fx, turret: u8) -> UnitStats {
    UnitStats {
        kind,
        hp: 60,
        movement_class: Some(0),
        speed_per_tick: Fx::from_ratio(1, 5),
        turn_per_tick: 16,
        size,
        footprint: None,
        weapon_range: Fx::from_int(range),
        turret_per_tick: turret,
    }
}

fn structure(kind: Kind, hp: u32, side: u8, range: i32) -> UnitStats {
    UnitStats {
        kind,
        hp,
        movement_class: None,
        speed_per_tick: Fx::ZERO,
        turn_per_tick: 0,
        size: Fx::ONE,
        footprint: Some(Footprint {
            width: side,
            height: side,
        }),
        weapon_range: Fx::from_int(range),
        turret_per_tick: 0,
    }
}

fn table() -> UnitTable {
    let half = Fx::from_ratio(1, 2);
    UnitTable::new(vec![
        mobile(RIFLE, 5, half, 0),
        mobile(TANK, 7, Fx::from_ratio(3, 2), 8),
        mobile(ARTY, 12, Fx::from_ratio(3, 2), 0),
        mobile(ROCKET, 8, half, 0),
        mobile(MORTAR, 9, half, 0),
        mobile(WALKER, 0, half, 0),
        structure(TOWER, 200, 1, 5),
        structure(HQ, 40, 2, 0),
    ])
    .unwrap()
}

/// Grass (0) and rock (1): a wall across the west half with a gap at its west end, so that a
/// walker on the west side plans a long detour over several ticks.
fn terrain() -> Terrain {
    let mut ids = vec![0u8; (W * H) as usize];
    for x in 4..30 {
        ids[(24 * W + x) as usize] = 1;
    }
    let defs = [
        TerrainDef {
            id: TerrainId(0),
            flags: TerrainFlags::OPEN,
        },
        TerrainDef {
            id: TerrainId(1),
            flags: TerrainFlags::default(),
        },
    ];
    Terrain::from_layer(W, H, &ids, &defs).unwrap()
}

fn movement() -> MovementTable {
    MovementTable::new(vec![ClassCosts {
        class: 0,
        costs: vec![
            (TerrainId(0), Cost::Speed(1000)),
            (TerrainId(1), Cost::Impassable),
        ],
    }])
    .unwrap()
}

/// The tick-0 world the host builds: the same every time.
fn fresh() -> World {
    World::with_terrain(WorldConfig::new(13).with_map_size(W, H), terrain())
        .unwrap()
        .with_units(table())
        .unwrap()
        .with_movement(movement())
        .unwrap()
        .with_path_budget(crate::path::MIN_NODE_BUDGET)
        .unwrap()
        .with_weapons(WeaponTable::new(weapons(), loadouts()).unwrap())
        .unwrap()
        .with_targeting(TargetingConfig::DEFAULT)
        .unwrap()
        .with_destruction(DestructionConfig::DEFAULT)
        .unwrap()
}

fn p(x: i32, y: i32) -> Vec2 {
    Vec2::from_ints(x, y)
}

/// The battle: player 1 holds a line of four HQs, a defence tower and a few riflemen around x = 46;
/// player 0 comes from the west with riflemen, tanks, artillery, rocket troopers and mortars on
/// attack-moves, two of them told to attack an HQ. Six walkers of player 0 go back and forth across
/// the wall in the west, out of everyone's reach, each turned round every 30 ticks, so a route is
/// being planned in most ticks, also in the ticks the HQs fall.
fn battle_with(mut w: World, rush: &[u64]) -> World {
    let s = |w: &mut World, t: u64, c: Command| w.schedule(Tick(t), PlayerId(0), c).unwrap();
    for (n, y) in [14u32, 20, 26, 32].into_iter().enumerate() {
        s(
            &mut w,
            0,
            Command::PlaceStructure {
                kind: HQ,
                cell: Cell::new(48, y),
                owner: PlayerId(1),
            },
        );
        let _ = n;
    }
    s(
        &mut w,
        0,
        Command::PlaceStructure {
            kind: TOWER,
            cell: Cell::new(44, 23),
            owner: PlayerId(1),
        },
    );
    for (n, y) in [16, 22, 28, 34].into_iter().enumerate() {
        s(
            &mut w,
            0,
            Command::SpawnUnit {
                kind: if n % 2 == 0 { RIFLE } else { TANK },
                pos: p(45, y),
                owner: PlayerId(1),
            },
        );
    }
    let army = [
        (RIFLE, 30, 18),
        (RIFLE, 30, 22),
        (RIFLE, 30, 30),
        (TANK, 28, 20),
        (TANK, 28, 28),
        (ARTY, 31, 21),
        (ROCKET, 29, 26),
        (MORTAR, 27, 16),
        (MORTAR, 27, 32),
    ];
    for &(kind, x, y) in &army {
        s(
            &mut w,
            0,
            Command::SpawnUnit {
                kind,
                pos: p(x, y),
                owner: PlayerId(0),
            },
        );
    }
    for n in 0..WALKERS as i32 {
        s(
            &mut w,
            0,
            Command::SpawnUnit {
                kind: WALKER,
                pos: p(4 + 3 * (n % 9), if n < 9 { 6 } else { 42 }),
                owner: PlayerId(0),
            },
        );
    }
    w.step();
    let id = |w: &World, i: usize| EntityId::new(i as u32, w.entities.generation[i]);
    let slots = |w: &World, keep: &dyn Fn(Kind, PlayerId) -> bool| -> Vec<usize> {
        live(w)
            .filter(|&i| keep(w.entities.kind[i], w.entities.owner[i]))
            .collect()
    };
    let hqs = slots(&w, &|k, _| k == HQ);
    let army = slots(&w, &|k, o| o == PlayerId(0) && k != WALKER);
    let walkers = slots(&w, &|k, _| k == WALKER);
    assert_eq!((hqs.len(), army.len(), walkers.len()), (4, 9, WALKERS));
    for (k, &i) in army.iter().enumerate() {
        // The artillery and a mortar are told to attack an HQ; the others attack-move.
        let order = match w.entities.kind[i] {
            ARTY => Order::Attack(id(&w, hqs[1])),
            MORTAR if k == 7 => Order::Attack(id(&w, hqs[2])),
            _ => Order::AttackMove(p(50, 14 + 3 * k as i32)),
        };
        let ids = vec![id(&w, i)];
        s(
            &mut w,
            1 + k as u64,
            Command::Order {
                ids,
                order,
                queued: false,
            },
        );
    }
    let walk = |w: &mut World, t: u64, n: usize, south: bool| {
        let i = walkers[n];
        let ids = vec![id(w, i)];
        let x = 4 + 3 * (n % 9) as i32;
        s(
            w,
            t,
            Command::Order {
                ids,
                order: Order::Move(p(x, if south { 40 } else { 8 })),
                queued: false,
            },
        );
    };
    for t in (1..TICKS).step_by(2) {
        let n = ((t / 2) % WALKERS as u64) as usize;
        walk(&mut w, t, n, (t / 36) % 2 == 0);
    }
    // On the ticks given, every walker is sent across the wall at once: a route is then being
    // planned when the service ends (the budget is the smallest), on the tick a structure falls.
    for &t in rush {
        for n in 0..walkers.len() {
            walk(&mut w, t, n, (t / 30 + n as u64) % 2 == 1);
        }
    }
    w
}

/// The ticks to rush the walkers on: the rush changes how fast the army's routes are planned, so
/// the ticks the structures fall on move with it. Starting without rushes, the ticks a structure
/// fell on are added to the rushes until a battle has a tick on which a structure falls while a
/// route is being planned (a job behind the service's epoch at the save after it).
fn falls() -> &'static [u64] {
    static FALLS: std::sync::OnceLock<Vec<u64>> = std::sync::OnceLock::new();
    FALLS.get_or_init(|| {
        let mut rush: Vec<u64> = Vec::new();
        for _ in 0..8 {
            let mut w = battle_with(fresh(), &rush);
            let mut behind = false;
            let mut fell = Vec::new();
            for _ in 1..TICKS {
                let t = w.tick;
                w.step();
                if w.destruction.rubble_born(t) > 0 && !rush.contains(&t.0) {
                    fell.push(t.0);
                }
                behind |= w.save_contents().job_behind;
            }
            if behind {
                return rush;
            }
            rush.extend(fell);
            rush.sort_unstable();
        }
        panic!("no structure falls while a route is planned: rushes {rush:?}");
    })
}

/// The battle, with the walkers rushed on the ticks of [`falls`].
fn battle(w: World) -> World {
    battle_with(w, falls())
}

/// What the battle's saves are asserted to carry across, at least once each.
type Claim = (&'static str, fn(&SaveContents) -> bool);
const CLAIMS: [Claim; 13] = [
    ("a projectile in flight", |c| c.projectiles > 0),
    ("a weapon reloading", |c| c.reloading > 0),
    ("a weapon in a burst", |c| c.bursting > 0),
    ("a unit engaged", |c| c.engaged > 0),
    ("a defence with a target", |c| c.defending > 0),
    ("engagement memory", |c| c.engage_memory > 0),
    ("a wreck", |c| c.wrecks > 0),
    ("a tally", |c| c.tallies > 0),
    ("a unit with kills", |c| c.killers > 0),
    ("reports of the weapon stage", |c| c.reports),
    ("a route being planned", |c| c.path_job),
    ("a job behind the service's epoch", |c| c.job_behind),
    ("a route", |c| c.routes > 0),
];

/// The kinds of the projectiles in flight.
fn kinds_in_flight(w: &World) -> Vec<ProjectileKind> {
    let mut kinds: Vec<u8> = w
        .arsenal
        .projectiles
        .iter()
        .filter_map(|(_, p)| w.arsenal.table.weapon(p.weapon))
        .map(|s| s.projectile as u8)
        .collect();
    kinds.sort_unstable();
    kinds.dedup();
    kinds
        .into_iter()
        .map(|k| match k {
            1 => ProjectileKind::Straight,
            2 => ProjectileKind::Arc,
            3 => ProjectileKind::Guided,
            4 => ProjectileKind::Falling,
            _ => ProjectileKind::Instant,
        })
        .collect()
}

/// The battle with a save, a restore and a resave after every tick: each restored world equals the
/// saved one by `PartialEq` (reports of the last tick and the interpolation put equal), its resave
/// is the save, and the run goes on with the restored world. Every checksum equals an uninterrupted
/// run, and each claim holds at some save tick (every projectile kind too).
#[test]
fn the_battle_restores_after_every_tick() {
    let mut straight = battle(fresh());
    for _ in 1..TICKS {
        straight.step();
    }
    let mut w = battle(fresh());
    let template = fresh();
    let mut seen = [0u32; CLAIMS.len()];
    let mut kinds: Vec<ProjectileKind> = Vec::new();
    for _ in 1..TICKS {
        let c = w.save_contents();
        for (k, (_, holds)) in CLAIMS.iter().enumerate() {
            seen[k] += u32::from(holds(&c));
        }
        for kind in kinds_in_flight(&w) {
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
        let bytes = w
            .save()
            .unwrap_or_else(|e| panic!("tick {}: {e}", w.tick.0));
        let back = World::restore(template.clone(), &bytes)
            .unwrap_or_else(|e| panic!("restore at tick {}: {e}", w.tick.0));
        assert_eq!(back.save().unwrap(), bytes, "resave at tick {}", w.tick.0);
        assert_eq!(
            settled(back.clone()),
            settled(w.clone()),
            "tick {}",
            w.tick.0
        );
        w = back;
        w.step();
        assert_eq!(
            w.checksum(),
            straight.checksum_after(Tick(w.tick.0 - 1)).unwrap(),
            "the checksum after tick {}",
            w.tick.0 - 1
        );
    }
    for (k, (what, _)) in CLAIMS.iter().enumerate() {
        assert!(seen[k] > 0, "no save with {what}");
        eprintln!("{what}: {} save ticks", seen[k]);
    }
    for kind in [
        ProjectileKind::Straight,
        ProjectileKind::Arc,
        ProjectileKind::Guided,
        ProjectileKind::Falling,
    ] {
        assert!(kinds.contains(&kind), "no save with a {kind:?} projectile");
    }
}

/// The battle run to the first tick after `from` at which `holds`.
fn battle_until(from: u64, holds: impl Fn(&World) -> bool) -> World {
    let mut w = battle(fresh());
    while w.tick.0 < from || !holds(&w) {
        assert!(w.tick.0 < TICKS, "the battle never reaches the state");
        w.step();
    }
    w
}

/// What a corruption case needs in the world it changes.
#[derive(Copy, Clone, PartialEq, Debug)]
enum Need {
    /// A weapon in a burst and one reloading, a unit engaged and one that is not, a defence with a
    /// target, two players with a tally, kills, an HQ standing.
    Early,
    /// A vehicle wreck and rubble.
    Late,
    /// A vehicle wreck more than 600 ticks (its lifetime) after the start.
    Old,
    /// A projectile of this kind in flight.
    Shot(ProjectileKind),
}

impl Need {
    fn holds(self, w: &World) -> bool {
        let c = w.save_contents();
        let has = |kind| w.destruction.wrecks.iter().any(|r| r.kind == kind);
        match self {
            Need::Early => {
                c.bursting > 0
                    && c.reloading > 0
                    && c.engaged > 0
                    && c.defending > 0
                    && c.tallies >= 2
                    && c.killers > 0
                    && live(w).any(|i| w.entities.kind[i] == HQ)
                    && live(w).any(|i| is_calm(w, i))
            }
            Need::Late => has(WreckKind::Vehicle) && has(WreckKind::Rubble),
            Need::Old => has(WreckKind::Vehicle) && w.tick.0 > 650,
            Need::Shot(kind) => kinds_in_flight(w).contains(&kind),
        }
    }
}

/// The first world of the battle in which `need` holds (worked out once per need).
fn base(need: Need) -> World {
    static BASES: std::sync::OnceLock<Vec<(Need, World)>> = std::sync::OnceLock::new();
    let bases = BASES.get_or_init(|| {
        let needs = [
            Need::Early,
            Need::Late,
            Need::Old,
            Need::Shot(ProjectileKind::Straight),
            Need::Shot(ProjectileKind::Arc),
            Need::Shot(ProjectileKind::Guided),
            Need::Shot(ProjectileKind::Falling),
        ];
        let mut found: Vec<(Need, World)> = Vec::new();
        let mut w = battle(fresh());
        while found.len() < needs.len() {
            assert!(
                w.tick.0 < TICKS,
                "the battle has only {:?}",
                found.iter().map(|f| f.0).collect::<Vec<_>>()
            );
            for &n in &needs {
                if !found.iter().any(|f| f.0 == n) && n.holds(&w) {
                    found.push((n, w.clone()));
                }
            }
            w.step();
        }
        found
    });
    bases.iter().find(|b| b.0 == need).unwrap().1.clone()
}

fn live(w: &World) -> impl Iterator<Item = usize> + '_ {
    (0..w.entities.len()).filter(|&i| w.entities.alive[i])
}

fn of_kind(w: &World, kind: Kind) -> usize {
    live(w).find(|&i| w.entities.kind[i] == kind).unwrap()
}

fn bursting(w: &World) -> (usize, usize) {
    live(w)
        .find_map(|i| {
            (0..4)
                .find(|&s| w.entities.weapon[i][s].burst_left > 0)
                .map(|s| (i, s))
        })
        .unwrap()
}

fn reloading(w: &World) -> (usize, usize) {
    live(w)
        .find_map(|i| {
            (0..4)
                .find(|&s| {
                    let st = w.entities.weapon[i][s];
                    st.burst_left == 0 && st.wait > 0
                })
                .map(|s| (i, s))
        })
        .unwrap()
}

fn engaged(w: &World) -> usize {
    live(w)
        .find(|&i| matches!(w.entities.order[i], Order::Pursue(_)))
        .unwrap()
}

/// Whether slot `i` is a live armed mobile unit that is not engaged and not held.
fn is_calm(w: &World, i: usize) -> bool {
    w.entities.alive[i]
        && w.entities.kind[i] != WALKER
        && w.entities.def[i] != NO_DEF
        && w.units
            .at(w.entities.def[i] as usize)
            .movement_class
            .is_some()
        && w.entities.eng[i].resume.is_none()
        && !w.entities.eng[i].held
}

fn calm(w: &World) -> usize {
    live(w).find(|&i| is_calm(w, i)).unwrap()
}

fn tower(w: &World) -> usize {
    of_kind(w, TOWER)
}

/// The slot of the first projectile of `kind` in flight.
fn projectile(w: &World, kind: ProjectileKind) -> usize {
    w.arsenal
        .projectiles
        .iter()
        .find(|(_, p)| w.arsenal.table.weapon(p.weapon).unwrap().projectile == kind)
        .unwrap()
        .0
        .index() as usize
}

fn shell_mut(w: &mut World, kind: ProjectileKind) -> &mut crate::projectiles::Projectile {
    let i = projectile(w, kind);
    w.arsenal.projectiles.row_mut(i).unwrap()
}

fn wreck_of(w: &World, kind: WreckKind) -> usize {
    w.destruction
        .wrecks
        .iter()
        .position(|r| r.kind == kind)
        .unwrap()
}

type Case = (&'static str, Need, fn(&mut World));

/// One impossible world per restore check of this WP, each made by changing the rich battle world
/// before it is saved (`save` does not judge what it writes), with the exact error.
fn corruptions() -> Vec<Case> {
    const SLOT: &str = "a weapon slot the weapon stage never leaves";
    const ENG: &str = "an engagement the targeting stage never leaves";
    const SHOT: &str = "a projectile the weapon stage never leaves in flight";
    const WRECK: &str = "a wreck the death stage never leaves";
    const TALLY: &str = "a tally out of order, all zero or without destruction";
    vec![
        // Weapons: the burst, the reload, the slots without a weapon.
        (SLOT, Need::Early, |w| {
            let (i, s) = bursting(w);
            w.entities.weapon[i][s].burst_left = 3;
        }),
        (SLOT, Need::Early, |w| {
            let (i, s) = bursting(w);
            w.entities.weapon[i][s].wait = 5;
        }),
        (SLOT, Need::Early, |w| {
            let (i, s) = reloading(w);
            let def = w.entities.def[i];
            let tail = w.arsenal.weapons_of(def).nth(s).unwrap().tail_ticks();
            w.entities.weapon[i][s].wait = tail + 1;
        }),
        (SLOT, Need::Early, |w| {
            // The rifle has one weapon: its second slot never moves.
            let i = of_kind(w, RIFLE);
            w.entities.weapon[i][1].wait = 1;
        }),
        (SLOT, Need::Early, |w| {
            let i = of_kind(w, HQ);
            w.entities.weapon[i][0].burst_left = 1;
        }),
        // Engagements.
        (ENG, Need::Early, |w| {
            let i = calm(w);
            w.entities.eng[i].cool = TargetingConfig::DEFAULT.scan_cooldown + 1;
        }),
        (ENG, Need::Early, |w| {
            let i = engaged(w);
            w.entities.eng[i].rest = 1;
        }),
        (ENG, Need::Early, |w| {
            let i = engaged(w);
            w.entities.eng[i].held = false;
        }),
        (ENG, Need::Early, |w| {
            let i = calm(w);
            w.entities.eng[i].held = true;
            w.entities.eng[i].rest = TargetingConfig::DEFAULT.pursuit_reset;
        }),
        (ENG, Need::Early, |w| {
            let i = calm(w);
            w.entities.eng[i].rest = 1;
        }),
        (ENG, Need::Early, |w| {
            let i = calm(w);
            w.entities.eng[i].anchor = p(3, 3);
        }),
        (ENG, Need::Early, |w| {
            let i = calm(w);
            w.entities.eng[i].resume = Some(Order::Idle);
        }),
        (ENG, Need::Early, |w| {
            let i = engaged(w);
            w.entities.eng[i].resume = Some(Order::Move(p(3, 3)));
        }),
        (ENG, Need::Early, |w| {
            let i = engaged(w);
            w.entities.eng[i].resume = Some(Order::Guard(GuardTarget::Point(p(99, 3))));
        }),
        (ENG, Need::Early, |w| {
            let i = engaged(w);
            w.entities.eng[i].reach += Fx::ONE;
        }),
        (ENG, Need::Early, |w| {
            let i = engaged(w);
            w.entities.eng[i].aim = p(99, 3);
        }),
        (ENG, Need::Early, |w| {
            let i = engaged(w);
            w.entities.eng[i].held = true;
            w.entities.eng[i].anchor = p(-1, 3);
        }),
        (ENG, Need::Early, |w| {
            let i = calm(w);
            w.entities.eng[i].aim = p(3, 3);
        }),
        (ENG, Need::Early, |w| {
            let i = calm(w);
            w.entities.eng[i].chase = true;
        }),
        (ENG, Need::Early, |w| {
            let i = tower(w);
            w.entities.eng[i].held = true;
        }),
        (ENG, Need::Early, |w| {
            let i = tower(w);
            w.entities.eng[i].reach = Fx::from_int(4);
        }),
        (ENG, Need::Early, |w| {
            let i = of_kind(w, HQ);
            w.entities.eng[i].cool = 1;
        }),
        (ENG, Need::Early, |w| {
            // An unarmed unit never engages.
            let i = of_kind(w, WALKER);
            w.entities.eng[i].cool = 1;
        }),
        // Kills.
        ("kills without a weapon or destruction", Need::Early, |w| {
            let i = of_kind(w, HQ);
            w.entities.kills[i] = 1;
        }),
        ("more kills than the tally", Need::Early, |w| {
            let i = of_kind(w, RIFLE);
            w.entities.kills[i] = 900;
        }),
        // Projectiles.
        (SHOT, Need::Shot(ProjectileKind::Straight), |w| {
            shell_mut(w, ProjectileKind::Straight).weapon = 99
        }),
        (SHOT, Need::Shot(ProjectileKind::Straight), |w| {
            shell_mut(w, ProjectileKind::Straight).weapon = GUN
        }),
        (SHOT, Need::Shot(ProjectileKind::Straight), |w| {
            shell_mut(w, ProjectileKind::Straight).target = Target::Point;
        }),
        (SHOT, Need::Shot(ProjectileKind::Arc), |w| {
            shell_mut(w, ProjectileKind::Arc).aim = p(65, 3)
        }),
        (SHOT, Need::Shot(ProjectileKind::Arc), |w| {
            shell_mut(w, ProjectileKind::Arc).fuse = 0
        }),
        (SHOT, Need::Shot(ProjectileKind::Arc), |w| {
            shell_mut(w, ProjectileKind::Arc).fuse += 100
        }),
        (SHOT, Need::Shot(ProjectileKind::Falling), |w| {
            let p = shell_mut(w, ProjectileKind::Falling);
            p.pos.x += Fx::ONE;
        }),
        (SHOT, Need::Shot(ProjectileKind::Straight), |w| {
            let p = shell_mut(w, ProjectileKind::Straight);
            p.pos.x = Fx::from_int(-1);
        }),
        (SHOT, Need::Shot(ProjectileKind::Guided), |w| {
            let p = shell_mut(w, ProjectileKind::Guided);
            p.pos.x = Fx::from_int(-300);
        }),
        (SHOT, Need::Shot(ProjectileKind::Straight), |w| {
            shell_mut(w, ProjectileKind::Straight).height = Fx::ONE
        }),
        (SHOT, Need::Shot(ProjectileKind::Arc), |w| {
            shell_mut(w, ProjectileKind::Arc).height = Fx::from_int(4);
        }),
        (SHOT, Need::Shot(ProjectileKind::Falling), |w| {
            shell_mut(w, ProjectileKind::Falling).height = Fx::ZERO
        }),
        (SHOT, Need::Shot(ProjectileKind::Falling), |w| {
            shell_mut(w, ProjectileKind::Falling).prev_height = Fx::from_int(5);
        }),
        (SHOT, Need::Shot(ProjectileKind::Arc), |w| {
            shell_mut(w, ProjectileKind::Arc).span = Fx::from_int(200);
        }),
        (SHOT, Need::Shot(ProjectileKind::Falling), |w| {
            shell_mut(w, ProjectileKind::Falling).span = Fx::ONE
        }),
        ("projectile slots of two lengths", Need::Early, |w| {
            w.arsenal.projectiles.corrupt_for_test(0);
        }),
        ("the projectile free list", Need::Early, |w| {
            w.arsenal.projectiles.corrupt_for_test(1);
        }),
        (
            "a free projectile slot is missing from the free list",
            Need::Early,
            |w| w.arsenal.projectiles.corrupt_for_test(2),
        ),
        // Wrecks and the tally.
        (WRECK, Need::Late, |w| {
            let k = wreck_of(w, WreckKind::Vehicle);
            w.destruction.wrecks[k].expires = w.destruction.wrecks[k].expires.next();
        }),
        (WRECK, Need::Late, |w| {
            let k = wreck_of(w, WreckKind::Vehicle);
            let t = w.tick;
            w.destruction.wrecks[k].born = t;
            w.destruction.wrecks[k].expires = t.saturating_add(600);
        }),
        (WRECK, Need::Old, |w| {
            // Expired on the last tick: the death stage removed it then.
            let k = wreck_of(w, WreckKind::Vehicle);
            let t = w.tick.0 - 1;
            w.destruction.wrecks[k].born = Tick(t - 600);
            w.destruction.wrecks[k].expires = Tick(t);
        }),
        (WRECK, Need::Late, |w| {
            let k = wreck_of(w, WreckKind::Vehicle);
            w.destruction.wrecks[k].unit = Kind(99);
        }),
        (WRECK, Need::Late, |w| {
            // A rifleman is too small to leave a wreck.
            let k = wreck_of(w, WreckKind::Vehicle);
            w.destruction.wrecks[k].unit = RIFLE;
        }),
        (WRECK, Need::Late, |w| {
            let k = wreck_of(w, WreckKind::Vehicle);
            w.destruction.wrecks[k].size = Fx::ONE;
        }),
        (WRECK, Need::Late, |w| {
            let k = wreck_of(w, WreckKind::Vehicle);
            w.destruction.wrecks[k].anchor = Some(Cell::new(1, 1));
        }),
        (WRECK, Need::Late, |w| {
            let k = wreck_of(w, WreckKind::Rubble);
            w.destruction.wrecks[k].anchor = Some(Cell::new(63, 1));
        }),
        (WRECK, Need::Late, |w| {
            let k = wreck_of(w, WreckKind::Rubble);
            w.destruction.wrecks[k].facing = Facing::new(64);
        }),
        (WRECK, Need::Late, |w| {
            let k = wreck_of(w, WreckKind::Rubble);
            w.destruction.wrecks[k].footprint = Some(Footprint::ONE);
        }),
        (WRECK, Need::Late, |w| {
            let k = wreck_of(w, WreckKind::Rubble);
            w.destruction.wrecks[k].unit = TANK;
        }),
        (WRECK, Need::Late, |w| {
            let k = wreck_of(w, WreckKind::Rubble);
            w.destruction.wrecks[k].pos = p(70, 2);
        }),
        ("the wrecks are out of order", Need::Late, |w| {
            w.destruction.wrecks.swap(0, 1);
        }),
        (TALLY, Need::Early, |w| w.destruction.tally.reverse()),
        (TALLY, Need::Early, |w| {
            let first = w.destruction.tally[0];
            w.destruction.tally.insert(0, first);
        }),
        (TALLY, Need::Early, |w| {
            w.destruction.tally.push((PlayerId(200), Tally::default()));
        }),
        ("unknown wreck kind", Need::Late, |w| {
            // Read back as a kind the file never writes: the tag is edited after the save below.
            w.destruction.wrecks[0].kind = WreckKind::Vehicle;
        }),
    ]
}

#[test]
fn every_impossible_combat_state_is_corrupt() {
    let late = base(Need::Late);
    assert!(
        late.destruction.wrecks[0].key() != late.destruction.wrecks[1].key(),
        "two wrecks with different keys"
    );
    for (k, (what, need, change)) in corruptions().into_iter().enumerate() {
        let mut w = base(need);
        assert!(
            World::restore(fresh(), &w.save().unwrap()).is_ok(),
            "{need:?}"
        );
        change(&mut w);
        let mut bytes = w.save().expect("save does not judge");
        if what == "unknown wreck kind" {
            let (_, start, _) = sections(&bytes)[4];
            bytes[start + 4] = 7;
        }
        assert_eq!(
            World::restore(fresh(), &bytes),
            Err(SaveError::Corrupt(what)),
            "case {k}"
        );
    }
    // A projectile with a byte the file never writes as its target.
    let mut bytes = base(Need::Shot(ProjectileKind::Straight)).save().unwrap();
    let (_, start, _) = sections(&bytes)[3];
    let slots = u32::from_le_bytes(bytes[start..start + 4].try_into().unwrap()) as usize;
    let rows = start + 4 + 4 * slots + 4;
    let first = (0..slots)
        .scan(rows, |at, _| {
            let here = *at;
            // A live row: flag, weapon, owner, player, target tag and id, aim, pos, prev_pos,
            // height, prev_height, heading, span, fuse.
            *at += if bytes[here] == 1 {
                1 + 2 + 8 + 1 + 1 + 8 + 16 * 3 + 8 * 2 + 1 + 8 + 4
            } else {
                1
            };
            Some(here)
        })
        .find(|&at| bytes[at] == 1)
        .unwrap();
    assert_eq!(
        bytes[first + 1 + 2 + 8 + 1],
        1,
        "the target tag of an entity"
    );
    bytes[first + 1 + 2 + 8 + 1] = 2;
    assert_eq!(
        World::restore(fresh(), &bytes),
        Err(SaveError::Corrupt("unknown projectile target"))
    );
}

/// Each new check refuses the smallest step past what the simulation reaches, and takes the
/// value at the bound: the counters at their most restore and step on.
#[test]
fn combat_state_at_its_bounds_restores_and_steps() {
    let mut w = base(Need::Early);
    let (i, s) = bursting(&w);
    w.entities.weapon[i][s].wait = 4;
    w.entities.weapon[i][s].burst_left = 2;
    let (i, s) = reloading(&w);
    let tail = w
        .arsenal
        .weapons_of(w.entities.def[i])
        .nth(s)
        .unwrap()
        .tail_ticks();
    w.entities.weapon[i][s].wait = tail;
    let i = calm(&w);
    w.entities.eng[i].cool = TargetingConfig::DEFAULT.scan_cooldown;
    w.entities.eng[i].held = true;
    w.entities.eng[i].anchor = p(1, 1);
    w.entities.eng[i].rest = TargetingConfig::DEFAULT.pursuit_reset - 1;
    // The tally at its most: the next kill saturates instead of overflowing, and a saturated tally
    // is not compared with the kills.
    for t in &mut w.destruction.tally {
        t.1 = Tally {
            kills: u32::MAX,
            assists: u32::MAX,
            losses: u32::MAX,
        };
    }
    let k = of_kind(&w, RIFLE);
    w.entities.kills[k] = u16::MAX;
    let mut back = World::restore(fresh(), &w.save().unwrap()).unwrap();
    for _ in 0..80 {
        back.step();
    }
    // An arc at its peak with the fuse it was launched with; a guided missile as far off as it
    // could have flown.
    let mut w = base(Need::Shot(ProjectileKind::Arc));
    let p = shell_mut(&mut w, ProjectileKind::Arc);
    p.height = Fx::from_int(3);
    p.fuse = crate::projectiles::fuse_for(&weapons()[2], p.span);
    let mut back = World::restore(fresh(), &w.save().unwrap()).unwrap();
    for _ in 0..80 {
        back.step();
    }
    let mut w = base(Need::Shot(ProjectileKind::Guided));
    let p = shell_mut(&mut w, ProjectileKind::Guided);
    p.fuse = 1;
    let launched = crate::projectiles::fuse_for(&weapons()[3], p.span);
    p.pos = Vec2::new(
        -Fx::from_ratio(1, 2) * Fx::from_int(launched as i32 - 1),
        Fx::ZERO,
    );
    let mut back = World::restore(fresh(), &w.save().unwrap()).unwrap();
    for _ in 0..20 {
        back.step();
    }
}

/// Saved on a tick a structure fell while a route was being planned: the job is behind the
/// service's epoch by the structures that fell. It restores without its memory being judged (the
/// next service replans it unread) and the run goes on like the original; one epoch more or less
/// is corrupt.
#[test]
fn a_job_behind_the_service_restores_and_replans() {
    let w = battle_until(0, |w| w.save_contents().job_behind);
    let fallen = w.destruction.rubble_born(Tick(w.tick.0 - 1));
    assert!(fallen >= 1);
    let mut straight = w.clone();
    let mut back = World::restore(fresh(), &w.save().unwrap()).unwrap();
    assert!(back.save_contents().job_behind);
    for _ in 0..120 {
        assert_eq!(back.step().checksum, straight.step().checksum);
    }
    for change in [1u8, 2] {
        let mut v = w.clone();
        if change == 1 {
            v.pathing.corrupt_for_test(22);
        } else {
            // The rubble of the last tick forgotten: the job is behind by more than fell.
            let last = Tick(v.tick.0 - 1);
            let k = v
                .destruction
                .wrecks
                .iter()
                .position(|r| r.kind == WreckKind::Rubble && r.born == last)
                .unwrap();
            v.destruction.wrecks.remove(k);
        }
        assert_eq!(
            World::restore(fresh(), &v.save().unwrap()),
            Err(SaveError::Corrupt(
                "a job of another epoch than the path service"
            )),
            "change {change}"
        );
    }
}

/// The battle's world without targeting or without destruction settings (the rest as `fresh`).
fn fresh_without(targeting: bool, destruction: bool) -> World {
    let mut w = World::with_terrain(WorldConfig::new(13).with_map_size(W, H), terrain())
        .unwrap()
        .with_units(table())
        .unwrap()
        .with_movement(movement())
        .unwrap()
        .with_path_budget(crate::path::MIN_NODE_BUDGET)
        .unwrap()
        .with_weapons(WeaponTable::new(weapons(), loadouts()).unwrap())
        .unwrap();
    if !targeting {
        w = w.with_targeting(TargetingConfig::DEFAULT).unwrap();
    }
    if !destruction {
        w = w.with_destruction(DestructionConfig::DEFAULT).unwrap();
    }
    w
}

/// Engagement memory, kills, wrecks or a tally in a world whose rules never make them: the saved
/// world has its settings taken away (so the rules digest is the one of a world without them) and
/// is restored onto such a world.
#[test]
fn combat_state_without_its_rules_is_corrupt() {
    type Strip = (&'static str, Need, bool, fn(&mut World));
    let cases: [Strip; 4] = [
        (
            "an engagement the targeting stage never leaves",
            Need::Early,
            true,
            |w| w.targeting.config = None,
        ),
        (
            "kills without a weapon or destruction",
            Need::Late,
            false,
            |w| {
                w.destruction.config = None;
            },
        ),
        (
            "a wreck the death stage never leaves",
            Need::Late,
            false,
            |w| {
                w.destruction.config = None;
                w.entities.kills.iter_mut().for_each(|k| *k = 0);
            },
        ),
        (
            "a tally out of order, all zero or without destruction",
            Need::Late,
            false,
            |w| {
                w.destruction.config = None;
                w.entities.kills.iter_mut().for_each(|k| *k = 0);
                w.destruction.wrecks.clear();
            },
        ),
    ];
    for (k, (what, need, targeting, strip)) in cases.into_iter().enumerate() {
        let mut w = base(need);
        strip(&mut w);
        let onto = fresh_without(targeting, !targeting);
        assert_eq!(w.rules_digest(), onto.rules_digest(), "case {k}");
        assert_eq!(
            World::restore(onto, &w.save().unwrap()),
            Err(SaveError::Corrupt(what)),
            "case {k}"
        );
    }
}

/// A different weapon, armor, targeting or destruction setting is another set of rules.
#[test]
fn other_combat_rules_are_refused() {
    let bytes = base(Need::Early).save().unwrap();
    let with = |weapons: Vec<WeaponStats>| {
        World::with_terrain(WorldConfig::new(13).with_map_size(W, H), terrain())
            .unwrap()
            .with_units(table())
            .unwrap()
            .with_movement(movement())
            .unwrap()
            .with_path_budget(crate::path::MIN_NODE_BUDGET)
            .unwrap()
            .with_weapons(WeaponTable::new(weapons, loadouts()).unwrap())
            .unwrap()
    };
    let mut more_damage = weapons();
    more_damage[1].damage += 1;
    let mut other_arc = weapons();
    other_arc[2].fire_arc = 64;
    let others = [
        with(more_damage)
            .with_targeting(TargetingConfig::DEFAULT)
            .unwrap()
            .with_destruction(DestructionConfig::DEFAULT)
            .unwrap(),
        with(other_arc)
            .with_targeting(TargetingConfig::DEFAULT)
            .unwrap()
            .with_destruction(DestructionConfig::DEFAULT)
            .unwrap(),
        with(weapons())
            .with_targeting(TargetingConfig::DEFAULT.with_pursuit_reset(99))
            .unwrap()
            .with_destruction(DestructionConfig::DEFAULT)
            .unwrap(),
        with(weapons())
            .with_targeting(TargetingConfig::from_cells(11, 6))
            .unwrap()
            .with_destruction(DestructionConfig::DEFAULT)
            .unwrap(),
        with(weapons())
            .with_targeting(TargetingConfig::DEFAULT)
            .unwrap()
            .with_destruction(DestructionConfig::from_tenths(601, 1200, 10))
            .unwrap(),
        with(weapons())
            .with_targeting(TargetingConfig::DEFAULT)
            .unwrap()
            .with_destruction(DestructionConfig::from_tenths(600, 1200, 11))
            .unwrap(),
        with(weapons())
            .with_destruction(DestructionConfig::DEFAULT)
            .unwrap(),
        with(weapons())
            .with_targeting(TargetingConfig::DEFAULT)
            .unwrap(),
    ];
    for (k, other) in others.into_iter().enumerate() {
        assert_eq!(World::restore(other, &bytes), Err(SaveError::Rules), "{k}");
    }
    assert!(World::restore(fresh(), &bytes).is_ok());
}

/// Single-bit flips in the combat sections of a real save (every bit of the first 64 bytes of `X`
/// and `D`, then one bit in every 97th byte of `E`): a world that restores steps four ticks, and
/// none panics. A sample, not a proof for every byte string.
#[test]
fn no_flip_in_the_combat_sections_panics() {
    let bytes = base(Need::Late).save().unwrap();
    let parts = sections(&bytes);
    let mut spots: Vec<(usize, u8)> = Vec::new();
    for &(tag, start, len) in &parts {
        match tag {
            b'X' | b'D' => {
                for at in start..start + len.min(64) {
                    spots.extend((0..8).map(|b| (at, b)));
                }
            }
            b'E' => {
                for at in (start..start + len).step_by(97) {
                    spots.push((at, (at % 8) as u8));
                }
            }
            _ => {}
        }
    }
    let template = fresh();
    for (at, bit) in spots {
        let mut d = bytes.clone();
        d[at] ^= 1 << bit;
        let r = std::panic::catch_unwind(|| {
            if let Ok(mut w) = World::restore(template.clone(), &d) {
                for _ in 0..4 {
                    w.step();
                }
            }
        });
        assert!(r.is_ok(), "flip {at}:{bit} panicked");
    }
}

/// The tags this WP writes into the file: the wreck kinds and the projectile targets.
#[test]
fn the_combat_tags_are_pinned() {
    let w = base(Need::Late);
    let mut bytes = ByteWriter::new();
    w.destruction.save(&mut bytes).unwrap();
    let bytes = bytes.finish();
    let first = w.destruction.wrecks[0].kind;
    assert_eq!(
        bytes[4],
        match first {
            WreckKind::Vehicle => 0,
            WreckKind::Rubble => 1,
        }
    );
    assert_eq!(WreckKind::Vehicle as u8, 0);
    assert_eq!(WreckKind::Rubble as u8, 1);
}
