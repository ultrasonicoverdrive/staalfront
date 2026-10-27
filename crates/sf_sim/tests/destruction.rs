//! Destruction (WP5.5, `docs/rules/destruction.md`), through the public API: when a unit or structure dies,
//! that the cells are free and the routes see them, the wrecks and the rubble and when they go, who is
//! credited with a kill (never read from the order of the shots), what becomes of the orders, engagements
//! and projectiles that pointed at the dead, and that nothing depends on the order the cast was spawned in.
//!
//! The numbers are small and invented so a test can follow them tick by tick. The shipped data runs in
//! `tools/sf_headless/tests/tanks_vs_infantry.rs` and `destruction.rs`.

use sf_math::{Fx, Vec2};
use sf_sim::{
    Cell, ClassCosts, Command, ConfigError, Cost, DestructionConfig, EntityId, Footprint,
    GuardTarget, Kind, Loadout, Mount, MovementTable, Order, PlayerId, ProjectileKind, Shot,
    TargetingConfig, Terrain, TerrainDef, TerrainFlags, TerrainId, UnitClass, UnitStats, UnitTable,
    WeaponStats, WeaponTable, World, WorldConfig, WreckKind, per_tick,
};

const MAP: u32 = 40;

// Units.
const PAWN: Kind = Kind(1); // 6 hit points, a rifle of 6; small (no wreck)
const GUNNER: Kind = Kind(2); // 50 hit points, a rifle of 6
const BRUISER: Kind = Kind(3); // 50 hit points, a gun of 20
const MID: Kind = Kind(4); // 50 hit points, a gun of 10
const TANK: Kind = Kind(5); // 100 hit points, big (a wreck), a rifle of 6
const TARGET: Kind = Kind(6); // 30 hit points, big (a wreck), cannot shoot
const SMALL: Kind = Kind(7); // 12 hit points, small, cannot shoot
const WALL: Kind = Kind(8); // a structure of 2 by 2 cells, 40 hit points
const HUT: Kind = Kind(9); // a structure of 1 by 1 cell, 40 hit points
const ROCK: Kind = Kind(10); // an object that cannot be hurt (zero hit points)
const BOMBER: Kind = Kind(11); // 50 hit points, a blast of 60 with a radius of 4
const LAUNCHER: Kind = Kind(12); // 50 hit points, shoots slow shells of 40
const ROCKETEER: Kind = Kind(13); // 50 hit points, a guided rocket of 40

const RIFLE: u16 = 1;
const GUN20: u16 = 2;
const GUN10: u16 = 3;
const BLAST: u16 = 4;
const SHELL: u16 = 5;
const ROCKET: u16 = 6;

fn at(x: i32, y: i32) -> Vec2 {
    Vec2::from_ints(x, y)
}

fn mid(x: i32, y: i32) -> Vec2 {
    Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF)
}

fn unit(kind: Kind, hp: u32, size_tenths: i64) -> UnitStats {
    UnitStats {
        kind,
        hp,
        movement_class: Some(0),
        speed_per_tick: per_tick(Fx::from_int(2)),
        // Any heading in a tick: the facing a unit is born with (it comes from the order it was
        // spawned in) must not decide when it sets off.
        turn_per_tick: 255,
        size: Fx::from_ratio(size_tenths, 10),
        footprint: None,
        weapon_range: Fx::from_int(6),
        turret_per_tick: 0,
    }
}

fn unarmed(kind: Kind, hp: u32, size_tenths: i64) -> UnitStats {
    UnitStats {
        weapon_range: Fx::ZERO,
        ..unit(kind, hp, size_tenths)
    }
}

fn structure(kind: Kind, w: u8, h: u8) -> UnitStats {
    UnitStats {
        kind,
        hp: 40,
        movement_class: None,
        speed_per_tick: Fx::ZERO,
        turn_per_tick: 0,
        size: Fx::ZERO,
        footprint: Some(Footprint {
            width: w,
            height: h,
        }),
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    }
}

fn units() -> UnitTable {
    UnitTable::new(vec![
        unit(PAWN, 6, 4),
        unit(GUNNER, 50, 4),
        unit(BRUISER, 50, 4),
        unit(MID, 50, 4),
        unit(TANK, 100, 15),
        unarmed(TARGET, 30, 15),
        unarmed(SMALL, 12, 4),
        structure(WALL, 2, 2),
        structure(HUT, 1, 1),
        UnitStats {
            movement_class: None,
            speed_per_tick: Fx::ZERO,
            turn_per_tick: 0,
            ..unarmed(ROCK, 0, 4)
        },
        unit(BOMBER, 50, 4),
        unit(LAUNCHER, 50, 4),
        unit(ROCKETEER, 50, 4),
    ])
    .unwrap()
}

fn weapon(kind: u16, projectile: ProjectileKind, damage: u32, reload: u32) -> WeaponStats {
    WeaponStats {
        kind,
        range: Fx::from_int(6),
        min_range: Fx::ZERO,
        reload_ticks: reload,
        burst: 1,
        burst_delay_ticks: 0,
        damage,
        damage_type: 0,
        projectile,
        projectile_speed_per_tick: if projectile == ProjectileKind::Instant {
            Fx::ZERO
        } else {
            Fx::from_ratio(1, 2)
        },
        flight_height: Fx::ZERO,
        guide_turn_per_tick: if projectile == ProjectileKind::Guided {
            8
        } else {
            0
        },
        splash_radius: Fx::ZERO,
        splash_inner: Fx::ZERO,
        friendly_fire: true,
        mount: Mount::Hull,
        fire_arc: 128,
    }
}

fn weapons() -> Vec<WeaponStats> {
    use ProjectileKind::{Guided, Instant, Straight};
    vec![
        weapon(RIFLE, Instant, 6, 10),
        weapon(GUN20, Instant, 20, 100),
        weapon(GUN10, Instant, 10, 100),
        WeaponStats {
            splash_radius: Fx::from_int(4),
            splash_inner: Fx::from_int(3),
            ..weapon(BLAST, Instant, 60, 100)
        },
        weapon(SHELL, Straight, 40, 100),
        weapon(ROCKET, Guided, 40, 100),
    ]
}

fn loadouts() -> Vec<Loadout> {
    let l = |unit: Kind, w: u16| Loadout {
        unit,
        weapons: vec![w],
    };
    vec![
        l(PAWN, RIFLE),
        l(GUNNER, RIFLE),
        l(BRUISER, GUN20),
        l(MID, GUN10),
        l(TANK, RIFLE),
        l(BOMBER, BLAST),
        l(LAUNCHER, SHELL),
        l(ROCKETEER, ROCKET),
    ]
}

fn table() -> WeaponTable {
    WeaponTable::new(weapons(), loadouts()).unwrap()
}

fn base() -> World {
    World::new(WorldConfig::new(7).with_map_size(MAP, MAP))
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_weapons(table())
        .unwrap()
}

/// A world where what is shot to zero is destroyed.
fn doomed() -> World {
    base().with_destruction(DestructionConfig::DEFAULT).unwrap()
}

/// The same, whose units also choose targets.
fn doomed_and_fought() -> World {
    base()
        .with_targeting(TargetingConfig::DEFAULT)
        .unwrap()
        .with_destruction(DestructionConfig::DEFAULT)
        .unwrap()
}

type Spawn = (Kind, u8, Vec2);

/// Puts the list in the world of a fresh world on one tick, in that order, and runs that tick: the ids
/// are the slots `0..`, and some of the cast may be dead already (a world whose units choose targets
/// shoots on the tick they appear).
fn spawn_fresh(w: &mut World, list: &[Spawn]) -> Vec<EntityId> {
    assert_eq!(w.slot_count(), 0);
    let tick = w.tick();
    for &(kind, owner, pos) in list {
        let command = if kind == WALL || kind == HUT {
            Command::PlaceStructure {
                kind,
                cell: Cell::new(pos.x.floor_int() as u32, pos.y.floor_int() as u32),
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
    let report = w.step();
    assert_eq!(report.commands_ignored, 0, "{list:?}");
    (0..list.len() as u32)
        .map(|i| EntityId::new(i, 0))
        .collect()
}

/// Puts the list in the world on one tick, in that order, and runs that tick. The ids come back in the
/// order of the list; each is found by what and where it is (a slot freed by a death is reused, with a
/// new generation).
fn spawn(w: &mut World, list: &[Spawn]) -> Vec<EntityId> {
    let before: Vec<EntityId> = w.snapshot().iter().map(|v| v.id).collect();
    let tick = w.tick();
    for &(kind, owner, pos) in list {
        let command = if kind == WALL || kind == HUT {
            Command::PlaceStructure {
                kind,
                cell: Cell::new(pos.x.floor_int() as u32, pos.y.floor_int() as u32),
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
    let report = w.step();
    assert_eq!(report.commands_ignored, 0, "{list:?}");
    let fresh: Vec<_> = w
        .snapshot()
        .iter()
        .filter(|v| !before.contains(&v.id))
        .collect();
    assert_eq!(fresh.len(), list.len(), "{list:?}");
    list.iter()
        .map(|&(kind, owner, pos)| {
            let centre = match kind {
                WALL => pos + Vec2::from_ints(1, 1),
                HUT => pos + Vec2::new(Fx::HALF, Fx::HALF),
                _ => pos,
            };
            fresh
                .iter()
                .find(|v| v.kind == kind && v.owner == PlayerId(owner) && v.pos == centre)
                .unwrap_or_else(|| panic!("{kind:?} was not placed"))
                .id
        })
        .collect()
}

fn attack(who: EntityId, victim: EntityId) -> Command {
    Command::Order {
        ids: vec![who],
        order: Order::Attack(victim),
        queued: false,
    }
}

fn order(who: EntityId, order: Order) -> Command {
    Command::Order {
        ids: vec![who],
        order,
        queued: false,
    }
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

fn run(w: &mut World, n: u32) {
    go(w, vec![], n);
}

// ---- the settings --------------------------------------------------------------------------------------

#[test]
fn the_settings_are_checked_and_only_given_before_the_start() {
    let plain = base();
    assert_eq!(plain.destruction_config(), None);
    let ok = DestructionConfig::DEFAULT;
    assert!(ok.is_valid());
    assert_eq!(
        base().with_destruction(ok).unwrap().destruction_config(),
        Some(ok)
    );
    let bad = |c: DestructionConfig| base().with_destruction(c).unwrap_err();
    for c in [
        DestructionConfig {
            wreck_ticks: 0,
            ..ok
        },
        DestructionConfig {
            rubble_ticks: 0,
            ..ok
        },
        DestructionConfig {
            wreck_ticks: sf_sim::MAX_WRECK_TICKS + 1,
            ..ok
        },
        DestructionConfig {
            wreck_min_size: Fx::from_int(-1),
            ..ok
        },
        DestructionConfig {
            wreck_min_size: Fx::from_int(9),
            ..ok
        },
    ] {
        assert_eq!(bad(c), ConfigError::Destruction, "{c:?}");
    }
    let mut late = base();
    spawn(&mut late, &[(GUNNER, 0, at(5, 5))]);
    assert_eq!(
        late.with_destruction(ok).unwrap_err(),
        ConfigError::Destruction
    );
}

#[test]
fn the_settings_are_part_of_the_checksum() {
    let a = doomed();
    let b = base()
        .with_destruction(DestructionConfig::from_tenths(601, 1200, 10))
        .unwrap();
    let c = base();
    assert_ne!(a.checksum(), b.checksum());
    assert_ne!(a.checksum(), c.checksum());
}

// ---- death ---------------------------------------------------------------------------------------------

#[test]
fn a_world_without_the_settings_leaves_a_unit_at_zero_where_it_fell() {
    let mut w = base();
    let ids = spawn(&mut w, &[(GUNNER, 0, at(10, 10)), (PAWN, 1, at(13, 10))]);
    go(&mut w, vec![attack(ids[0], ids[1])], 3);
    assert_eq!(w.hp_of(ids[1]), Some(0), "shot to zero");
    assert!(w.is_alive(ids[1]), "and still there");
    assert!(w.deaths().is_empty() && w.wrecks().is_empty());
}

#[test]
fn a_unit_shot_to_zero_is_removed_at_the_end_of_the_tick_it_was_shot() {
    let mut w = doomed();
    let ids = spawn(&mut w, &[(GUNNER, 0, at(10, 10)), (PAWN, 1, at(13, 10))]);
    w.schedule(w.tick(), PlayerId(0), attack(ids[0], ids[1]))
        .unwrap();
    let report = w.step();
    // One rifle shot of 6 against 6 hit points: dead on the tick of the shot.
    assert_eq!(w.shots().len(), 1);
    assert_eq!(report.deaths, 1);
    assert!(!w.is_alive(ids[1]));
    assert_eq!(w.hp_of(ids[1]), None);
    assert_eq!(w.snapshot().len(), 1);
    let death = &w.deaths()[0];
    assert_eq!(
        (death.victim, death.kind, death.owner, death.pos, death.tick),
        (
            ids[1],
            PAWN,
            PlayerId(1),
            at(13, 10),
            sf_sim::Tick(w.tick().0 - 1)
        )
    );
    // The slot is reusable with a new generation: the old id stays stale.
    let again = spawn(&mut w, &[(PAWN, 1, at(20, 20))]);
    assert_eq!(again[0], EntityId::new(ids[1].index(), 1));
    assert!(!w.is_alive(ids[1]));
}

#[test]
fn damage_of_the_whole_tick_is_in_before_anyone_dies_so_a_mutual_kill_kills_both() {
    // Two pawns of 6 hit points shoot each other with rifles of 6 on the same tick: the two-pass rule
    // keeps both shots, and both die.
    let mut w = doomed();
    let ids = spawn(&mut w, &[(PAWN, 0, at(10, 10)), (PAWN, 1, at(13, 10))]);
    let cmds = vec![attack(ids[0], ids[1]), attack(ids[1], ids[0])];
    let tick = w.tick();
    for c in cmds {
        w.schedule(tick, PlayerId(0), c).unwrap();
    }
    let report = w.step();
    assert_eq!(report.shots, 2);
    assert_eq!(report.deaths, 2);
    assert_eq!(w.snapshot().len(), 0);
    // Each is credited with the other's death.
    assert_eq!(w.tally_of(PlayerId(0)).kills, 1);
    assert_eq!(w.tally_of(PlayerId(1)).kills, 1);
    assert_eq!(w.tally_of(PlayerId(0)).losses, 1);
    assert_eq!(w.tally_of(PlayerId(1)).losses, 1);
}

#[test]
fn something_that_cannot_be_hurt_is_never_destroyed() {
    let mut w = doomed();
    let ids = spawn(&mut w, &[(GUNNER, 0, at(10, 10)), (ROCK, 2, at(13, 10))]);
    assert_eq!(w.hp_of(ids[1]), Some(0));
    go(&mut w, vec![attack(ids[0], ids[1])], 40);
    assert!(
        w.is_alive(ids[1]),
        "zero hit points of a rock are not a death"
    );
    assert!(w.shots().is_empty() && w.deaths().is_empty());
}

#[test]
fn a_legacy_entity_without_a_table_entry_is_never_destroyed() {
    let mut w = doomed();
    w.schedule(
        w.tick(),
        PlayerId(0),
        Command::Spawn {
            kind: Kind(99),
            pos: at(3, 3),
            owner: PlayerId(0),
        },
    )
    .unwrap();
    run(&mut w, 5);
    assert_eq!(w.snapshot().len(), 1);
    assert!(w.deaths().is_empty());
}

// ---- cells -------------------------------------------------------------------------------------------

#[test]
fn a_destroyed_structure_frees_every_cell_of_its_footprint() {
    let mut w = doomed();
    let ids = spawn(
        &mut w,
        &[
            (WALL, 1, at(20, 20)),
            (BRUISER, 0, at(16, 20)),
            (BRUISER, 0, at(16, 21)),
        ],
    );
    let cells = [(20, 20), (21, 20), (20, 21), (21, 21)];
    for (x, y) in cells {
        assert!(w.is_occupied(Cell::new(x, y)));
    }
    assert_eq!(w.occupied_cells(), 4);
    // Two shots of 20 against 40 hit points: dead on the first tick both fire.
    go(
        &mut w,
        vec![attack(ids[1], ids[0]), attack(ids[2], ids[0])],
        1,
    );
    assert!(!w.is_alive(ids[0]));
    for (x, y) in cells {
        assert!(!w.is_occupied(Cell::new(x, y)), "({x}, {y})");
        assert_eq!(w.occupant_at(Cell::new(x, y)), None);
    }
    assert_eq!(w.occupied_cells(), 0);
    // The cells can be built on again at once.
    let new = spawn(&mut w, &[(WALL, 0, at(20, 20))]);
    assert!(w.is_alive(new[0]));
    assert_eq!(w.occupied_cells(), 4);
}

// A wall of rock down columns 20 and 21 of a 40 x 20 map with a gap in rows 8 to 11, which two walls of
// 2 by 2 cells close.
fn gap_world() -> World {
    const GRASS: TerrainId = TerrainId(0);
    const ROCKY: TerrainId = TerrainId(1);
    let layer: Vec<u8> = (0..20u32)
        .flat_map(|y| {
            (0..MAP).map(move |x| {
                if (20..=21).contains(&x) && !(8..=11).contains(&y) {
                    ROCKY.0
                } else {
                    GRASS.0
                }
            })
        })
        .collect();
    let defs = vec![
        TerrainDef {
            id: GRASS,
            flags: TerrainFlags::OPEN,
        },
        TerrainDef {
            id: ROCKY,
            flags: TerrainFlags::default(),
        },
    ];
    let terrain = Terrain::from_layer(MAP, 20, &layer, &defs).unwrap();
    let costs = MovementTable::new(vec![ClassCosts {
        class: 0,
        costs: vec![(GRASS, Cost::Speed(1000)), (ROCKY, Cost::Impassable)],
    }])
    .unwrap();
    World::with_terrain(WorldConfig::new(7).with_map_size(MAP, 20), terrain)
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_weapons(table())
        .unwrap()
        .with_movement(costs)
        .unwrap()
        .with_destruction(DestructionConfig::DEFAULT)
        .unwrap()
}

#[test]
fn routes_and_a_walking_unit_use_the_cells_of_a_destroyed_structure() {
    let mut w = gap_world();
    let ids = spawn(
        &mut w,
        &[
            (WALL, 1, at(20, 8)),
            (WALL, 1, at(20, 10)),
            (BRUISER, 0, at(15, 8)),
            (BRUISER, 0, at(15, 9)),
            (GUNNER, 0, at(5, 9)),
        ],
    );
    let (from, to) = (mid(5, 9), mid(35, 9));
    let shut = w
        .plan_route(0, from, to)
        .expect("a route to the nearest reachable spot");
    assert!(shut.unreachable, "the gap is shut");
    let epoch = w.path_stats().sectors_rebuilt;
    // Four shots of 20 destroy both walls (40 each), two shots each.
    go(
        &mut w,
        vec![attack(ids[2], ids[0]), attack(ids[3], ids[0])],
        1,
    );
    assert!(!w.is_alive(ids[0]));
    assert!(!w.is_occupied(Cell::new(20, 8)));
    assert!(
        w.is_occupied(Cell::new(20, 10)),
        "the other wall still stands"
    );
    let half = w.plan_route(0, from, to).unwrap();
    assert!(!half.unreachable, "two rows of the gap are free now");
    assert!(
        half.waypoints.iter().all(|p| {
            let c = w.cell_at(*p).unwrap();
            !(c.x == 20 && (10..=11).contains(&c.y))
        }),
        "and the other wall is still in the way"
    );
    go(
        &mut w,
        vec![attack(ids[2], ids[1]), attack(ids[3], ids[1])],
        102,
    );
    assert!(!w.is_alive(ids[1]));
    assert_eq!(w.occupied_cells(), 0);
    assert!(
        w.path_stats().sectors_rebuilt > epoch,
        "the sectors over the cells were refreshed"
    );
    let open = w.plan_route(0, from, to).expect("the gap is open");
    assert!(!open.unreachable, "the gap is open");
    // And a unit sent through it gets there, passing over the cells the wall stood on.
    go(&mut w, vec![order(ids[4], Order::Move(mid(35, 9)))], 1);
    let mut crossed = false;
    for _ in 0..600 {
        w.step();
        let c = w.cell_at(w.get(ids[4]).unwrap().pos).unwrap();
        crossed |= c.x == 20 && (8..=11).contains(&c.y);
        if w.order_of(ids[4]) == Some(Order::Idle) {
            break;
        }
    }
    assert!(crossed, "it went through the freed cells");
    assert_eq!(w.order_of(ids[4]), Some(Order::Idle), "it arrived");
    assert_eq!(
        w.cell_at(w.get(ids[4]).unwrap().pos),
        Some(Cell::new(35, 9))
    );
}

#[test]
fn a_unit_walking_toward_a_shut_gap_goes_through_it_when_the_walls_fall() {
    // A unit is ordered while the gap is shut: it cannot get there. The walls fall; a unit given the
    // order afterwards arrives (the first has given up by then or waits: it must not crash or stick the
    // world).
    let mut w = gap_world();
    let ids = spawn(
        &mut w,
        &[
            (WALL, 1, at(20, 8)),
            (WALL, 1, at(20, 10)),
            (GUNNER, 0, at(5, 9)),
        ],
    );
    go(&mut w, vec![order(ids[2], Order::Move(mid(35, 9)))], 60);
    let stuck = w.get(ids[2]).unwrap().pos;
    assert!(stuck.x < Fx::from_int(20), "it cannot pass the wall");
    go(
        &mut w,
        vec![
            Command::Despawn { id: ids[0] },
            Command::Despawn { id: ids[1] },
        ],
        1,
    );
    assert_eq!(w.occupied_cells(), 0);
    go(&mut w, vec![order(ids[2], Order::Move(mid(35, 9)))], 600);
    assert_eq!(
        w.cell_at(w.get(ids[2]).unwrap().pos),
        Some(Cell::new(35, 9))
    );
}

// ---- wrecks and rubble -------------------------------------------------------------------------------

#[test]
fn a_vehicle_leaves_a_wreck_and_a_small_unit_leaves_none() {
    let mut w = doomed();
    let ids = spawn(
        &mut w,
        &[
            (BRUISER, 0, at(10, 10)),
            (BRUISER, 0, at(10, 12)),
            (TARGET, 1, at(14, 11)),
            (SMALL, 1, at(15, 13)),
        ],
    );
    go(
        &mut w,
        vec![attack(ids[0], ids[2]), attack(ids[1], ids[2])],
        1,
    );
    assert!(!w.is_alive(ids[2]));
    let wrecks = w.wrecks();
    assert_eq!(wrecks.len(), 1);
    let wreck = wrecks[0];
    assert_eq!(wreck.kind, WreckKind::Vehicle);
    assert_eq!(
        (wreck.unit, wreck.owner, wreck.pos),
        (TARGET, PlayerId(1), at(14, 11))
    );
    assert_eq!(wreck.size, Fx::from_ratio(15, 10));
    assert_eq!((wreck.footprint, wreck.anchor), (None, None));
    // A small unit dies and leaves nothing (but is a death all the same).
    let next = w.tick();
    go(
        &mut w,
        vec![attack(ids[0], ids[3]), attack(ids[1], ids[3])],
        100,
    );
    assert!(!w.is_alive(ids[3]));
    assert_eq!(w.wrecks().len(), 1, "no second wreck");
    assert!(next.0 > 0);
}

#[test]
fn a_structure_leaves_rubble_the_size_of_its_footprint() {
    let mut w = doomed();
    // The hut stands in the first slot but lies further south than the wall: the rubble is listed by
    // where it lies, not by the slot of what it was.
    let ids = spawn(
        &mut w,
        &[
            (HUT, 1, at(25, 25)),
            (WALL, 1, at(20, 20)),
            (BRUISER, 0, at(16, 20)),
            (BRUISER, 0, at(16, 21)),
            (BRUISER, 0, at(21, 25)),
            (BRUISER, 0, at(21, 26)),
        ],
    );
    go(
        &mut w,
        vec![
            attack(ids[2], ids[1]),
            attack(ids[3], ids[1]),
            attack(ids[4], ids[0]),
            attack(ids[5], ids[0]),
        ],
        1,
    );
    let rubble = w.wrecks();
    assert_eq!(rubble.len(), 2);
    let (wall, hut) = (&rubble[0], &rubble[1]);
    assert_eq!(
        (wall.unit, hut.unit),
        (WALL, HUT),
        "listed by where they lie"
    );
    assert_eq!(wall.kind, WreckKind::Rubble);
    assert_eq!(
        (wall.footprint, wall.anchor, wall.pos),
        (
            Some(Footprint {
                width: 2,
                height: 2
            }),
            Some(Cell::new(20, 20)),
            at(21, 21)
        )
    );
    assert_eq!(
        (hut.footprint, hut.anchor),
        (Some(Footprint::ONE), Some(Cell::new(25, 25)))
    );
    // The report of the deaths is in the same order.
    let dead: Vec<Kind> = w.deaths().iter().map(|d| d.kind).collect();
    assert_eq!(dead, vec![WALL, HUT]);
}

#[test]
fn wrecks_and_rubble_last_the_number_of_ticks_the_settings_give_and_not_one_more() {
    let mut w = base()
        .with_destruction(DestructionConfig::from_tenths(7, 12, 10))
        .unwrap();
    let ids = spawn(
        &mut w,
        &[
            (BRUISER, 0, at(10, 10)),
            (BRUISER, 0, at(10, 12)),
            (TARGET, 1, at(14, 11)),
            (HUT, 1, at(25, 25)),
            (BRUISER, 0, at(21, 25)),
            (BRUISER, 0, at(21, 26)),
        ],
    );
    let tick = w.tick();
    go(
        &mut w,
        vec![
            attack(ids[0], ids[2]),
            attack(ids[1], ids[2]),
            attack(ids[4], ids[3]),
            attack(ids[5], ids[3]),
        ],
        1,
    );
    let died = sf_sim::Tick(tick.0);
    assert_eq!(w.wrecks().len(), 2);
    let v = w
        .wrecks()
        .iter()
        .find(|x| x.kind == WreckKind::Vehicle)
        .unwrap();
    assert_eq!((v.born, v.expires), (died, sf_sim::Tick(died.0 + 7)));
    // Present for 7 ticks counting the one it was made on, gone on the 8th... that is: still there after
    // 6 more steps, gone after the 7th.
    run(&mut w, 6);
    assert_eq!(w.wrecks().len(), 2, "after {} ticks", 7);
    let view: Vec<_> = w.snapshot().wrecks().collect();
    let vehicle = view.iter().find(|x| x.kind == WreckKind::Vehicle).unwrap();
    assert_eq!((vehicle.ticks_left, vehicle.lifetime), (1, 7));
    run(&mut w, 1);
    assert_eq!(
        w.wrecks().len(),
        1,
        "the vehicle wreck is gone after its 7 ticks"
    );
    assert_eq!(w.wrecks()[0].kind, WreckKind::Rubble);
    run(&mut w, 4);
    assert_eq!(w.wrecks().len(), 1, "the rubble lasts 12");
    run(&mut w, 1);
    assert!(w.wrecks().is_empty());
}

#[test]
fn a_wreck_blocks_nothing_a_unit_walks_over_it_and_a_building_clears_it_away() {
    let mut w = doomed();
    let ids = spawn(
        &mut w,
        &[
            (BRUISER, 0, at(10, 10)),
            (BRUISER, 0, at(10, 12)),
            (TARGET, 1, at(14, 11)),
            (GUNNER, 0, at(8, 11)),
        ],
    );
    go(
        &mut w,
        vec![attack(ids[0], ids[2]), attack(ids[1], ids[2])],
        1,
    );
    let wreck = w.wrecks()[0];
    assert_eq!(wreck.pos, at(14, 11));
    assert!(!w.is_occupied(Cell::new(14, 11)));
    // A walker goes right over it.
    go(&mut w, vec![order(ids[3], Order::Move(at(20, 11)))], 1);
    let mut over = false;
    for _ in 0..200 {
        w.step();
        let p = w.get(ids[3]).unwrap().pos;
        over |= (p - wreck.pos).length() < Fx::HALF;
        if w.order_of(ids[3]) == Some(Order::Idle) {
            break;
        }
    }
    assert!(over, "it walked through the wreck");
    assert_eq!(w.order_of(ids[3]), Some(Order::Idle));
    assert_eq!(w.wrecks().len(), 1, "the wreck is still there");
    // A building on the spot clears the wreck away.
    spawn(&mut w, &[(HUT, 0, at(14, 11))]);
    assert!(w.wrecks().is_empty());
}

#[test]
fn a_building_on_rubble_clears_the_rubble_that_overlaps_it_and_leaves_the_rest() {
    let mut w = doomed();
    let ids = spawn(
        &mut w,
        &[
            (WALL, 1, at(20, 20)),
            (WALL, 1, at(30, 20)),
            (BRUISER, 0, at(16, 20)),
            (BRUISER, 0, at(16, 21)),
            (BRUISER, 0, at(26, 20)),
            (BRUISER, 0, at(26, 21)),
        ],
    );
    go(
        &mut w,
        vec![
            attack(ids[2], ids[0]),
            attack(ids[3], ids[0]),
            attack(ids[4], ids[1]),
            attack(ids[5], ids[1]),
        ],
        1,
    );
    assert_eq!(w.wrecks().len(), 2);
    spawn(&mut w, &[(HUT, 0, at(21, 21))]);
    assert_eq!(w.wrecks().len(), 1);
    assert_eq!(w.wrecks()[0].anchor, Some(Cell::new(30, 20)));
}

// ---- kill credit -------------------------------------------------------------------------------------

#[test]
fn the_biggest_share_of_the_damage_of_the_killing_tick_is_the_kill_and_the_rest_are_assists() {
    // The victim has 30 hit points; the bruiser does 20, the two gunners 6 each (32 in all, so the
    // tick kills).
    let mut w = doomed();
    let ids = spawn(
        &mut w,
        &[
            (TARGET, 9, at(20, 20)),
            (GUNNER, 3, at(20, 16)),
            (BRUISER, 4, at(24, 20)),
            (GUNNER, 5, at(16, 20)),
        ],
    );
    go(
        &mut w,
        vec![
            attack(ids[1], ids[0]),
            attack(ids[2], ids[0]),
            attack(ids[3], ids[0]),
        ],
        1,
    );
    let d = &w.deaths()[0];
    let killer = d.killer.unwrap();
    assert_eq!(
        (killer.shooter, killer.owner, killer.damage),
        (ids[2], PlayerId(4), 20)
    );
    // Assists: equal damage, so the one that stood further north first, and then the one further west.
    let who: Vec<_> = d.assists.iter().map(|a| (a.shooter, a.damage)).collect();
    assert_eq!(who, vec![(ids[1], 6), (ids[3], 6)]);
    assert_eq!(w.tally_of(PlayerId(4)).kills, 1);
    assert_eq!(w.tally_of(PlayerId(3)).assists, 1);
    assert_eq!(w.tally_of(PlayerId(5)).assists, 1);
    assert_eq!(w.tally_of(PlayerId(9)).losses, 1);
    assert_eq!(w.kills_of(ids[2]), Some(1));
    assert_eq!(w.kills_of(ids[1]), Some(0));
}

#[test]
fn equal_damage_is_decided_by_where_the_shooters_stand_in_every_spawn_order() {
    // Three shooters of 10 against 30 hit points: the killer is the northernmost, the westernmost of
    // those in a row; the assists follow in that order. Whatever slot each has.
    let cast: [Spawn; 4] = [
        (TARGET, 9, at(12, 14)),
        (MID, 3, at(10, 12)),
        (MID, 4, at(14, 10)),
        (MID, 5, at(12, 10)),
    ];
    let mut seen = None;
    for order in permutations(cast.len()) {
        let (killer, assists, tally, kills) = credit_run(&cast, &order);
        // By start position: the killer is cast[3] (12, 10), then (14, 10), then (10, 12).
        assert_eq!(killer, Some(3), "{order:?}");
        assert_eq!(assists, vec![2, 1], "{order:?}");
        let outcome = (tally, kills);
        match &seen {
            Some(first) => assert_eq!(&outcome, first, "{order:?}"),
            None => seen = Some(outcome),
        }
    }
}

/// The killer, the assists, the tallies `(player, kills, assists, losses)` and the kills of each cast member.
type CreditRun = (
    Option<usize>,
    Vec<usize>,
    Vec<(u8, u32, u32, u32)>,
    Vec<Option<u16>>,
);

/// Plays the cast in the given spawn order, all shooters attacking the first (cast index 0, the
/// victim); returns the killer and assists as indices of the cast, the tallies of the players and the
/// kills of each cast member.
fn credit_run(cast: &[Spawn], order: &[usize]) -> CreditRun {
    let mut w = doomed();
    let list: Vec<Spawn> = order.iter().map(|&i| cast[i]).collect();
    let ids = spawn(&mut w, &list);
    let id_of = |cast_index: usize| ids[order.iter().position(|&o| o == cast_index).unwrap()];
    let cast_of = |id: EntityId| order[ids.iter().position(|&x| x == id).unwrap()];
    let commands: Vec<Command> = (1..cast.len())
        .map(|i| attack(id_of(i), id_of(0)))
        .collect();
    go(&mut w, commands, 1);
    assert_eq!(w.deaths().len(), 1, "{order:?}");
    let d = &w.deaths()[0];
    let killer = d.killer.map(|k| cast_of(k.shooter));
    let assists = d.assists.iter().map(|a| cast_of(a.shooter)).collect();
    let tally = (0..12u8)
        .map(|p| {
            let t = w.tally_of(PlayerId(p));
            (p, t.kills, t.assists, t.losses)
        })
        .filter(|t| t.1 + t.2 + t.3 > 0)
        .collect();
    let kills = (0..cast.len()).map(|i| w.kills_of(id_of(i))).collect();
    (killer, assists, tally, kills)
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

#[test]
fn the_shooter_with_the_most_damage_is_the_killer_whatever_its_slot_and_whatever_hp_after_says() {
    // The small shooter has the lowest slot and the shot list puts it first; the big one has the
    // largest slot. The shots report the same `hp_after` for both.
    for order in permutations(3) {
        let cast: [Spawn; 3] = [
            (TARGET, 9, at(20, 20)),
            (GUNNER, 3, at(20, 16)),
            (BRUISER, 4, at(24, 20)),
        ];
        let mut w = doomed();
        let list: Vec<Spawn> = order.iter().map(|&i| cast[i]).collect();
        let ids = spawn(&mut w, &list);
        let id_of = |i: usize| ids[order.iter().position(|&o| o == i).unwrap()];
        // 6 + 20 = 26 is not enough: add the target at 25 hit points by a first rifle shot... the
        // victim here has 30, so give the gunner a head start of two shots first.
        let shots = go(&mut w, vec![attack(id_of(1), id_of(0))], 11);
        assert_eq!(shots.len(), 2);
        assert_eq!(w.hp_of(id_of(0)), Some(18));
        // Now both fire in one tick: the bruiser's 20 kills. (The gunner reloads at the same tick.)
        let tick = w.tick();
        w.schedule(tick, PlayerId(0), attack(id_of(2), id_of(0)))
            .unwrap();
        let mut dead = false;
        for _ in 0..12 {
            w.step();
            if let Some(d) = w.deaths().first() {
                let killer = d.killer.unwrap();
                assert_eq!(killer.shooter, id_of(2), "{order:?}");
                let hp_afters: Vec<u32> = w.shots().iter().map(|s| s.hp_after).collect();
                assert!(hp_afters.iter().all(|&h| h == 0), "{hp_afters:?}");
                dead = true;
                break;
            }
        }
        assert!(dead, "{order:?}");
    }
}

#[test]
fn damage_from_the_victims_own_side_is_no_kill() {
    let mut w = doomed();
    let ids = spawn(&mut w, &[(GUNNER, 0, at(10, 10)), (PAWN, 0, at(13, 10))]);
    go(&mut w, vec![attack(ids[0], ids[1])], 1);
    assert!(!w.is_alive(ids[1]));
    let d = &w.deaths()[0];
    assert_eq!((d.killer, d.assists.len()), (None, 0));
    let t = w.tally_of(PlayerId(0));
    assert_eq!((t.kills, t.assists, t.losses), (0, 0, 1));
    assert_eq!(w.kills_of(ids[0]), Some(0));
}

#[test]
fn a_blast_that_kills_its_own_shooter_credits_the_kill_and_counts_the_loss() {
    // The bomber (50 hit points) blasts 60 around a target 3 cells away (radius 4 at full strength):
    // the target dies, and so does the bomber, in the same tick.
    let mut w = doomed();
    let ids = spawn(&mut w, &[(BOMBER, 0, at(10, 10)), (TARGET, 1, at(13, 10))]);
    go(&mut w, vec![attack(ids[0], ids[1])], 1);
    assert_eq!(w.deaths().len(), 2, "both are gone");
    assert_eq!(w.snapshot().len(), 0);
    let t0 = w.tally_of(PlayerId(0));
    let t1 = w.tally_of(PlayerId(1));
    assert_eq!((t0.kills, t0.losses), (1, 1), "its own blast is no kill");
    assert_eq!((t1.kills, t1.losses), (0, 1));
    let bomber = w.deaths().iter().find(|d| d.kind == BOMBER).unwrap();
    assert_eq!(bomber.killer, None);
}

#[test]
fn a_shell_that_outlives_its_gunner_still_scores_for_his_side() {
    let mut w = doomed();
    let ids = spawn(
        &mut w,
        &[(LAUNCHER, 0, at(10, 10)), (TARGET, 1, at(16, 10))],
    );
    go(&mut w, vec![attack(ids[0], ids[1])], 2);
    assert_eq!(w.projectile_count(), 1, "the shell is in the air");
    // The gunner goes before it lands.
    go(&mut w, vec![Command::Despawn { id: ids[0] }], 1);
    let mut guard = 0;
    while w.deaths().is_empty() {
        w.step();
        guard += 1;
        assert!(guard < 60, "the shell never landed");
    }
    assert!(!w.is_alive(ids[1]));
    let d = &w.deaths()[0];
    let k = d.killer.unwrap();
    assert_eq!((k.shooter, k.owner, k.damage), (ids[0], PlayerId(0), 40));
    assert_eq!(w.kills_of(ids[0]), None, "gone");
    assert_eq!(w.tally_of(PlayerId(0)).kills, 1);
}

// ---- what pointed at the dead --------------------------------------------------------------------------

#[test]
fn an_attack_order_on_something_that_died_ends_and_the_attacker_goes_idle() {
    let mut w = doomed();
    let ids = spawn(
        &mut w,
        &[
            (GUNNER, 0, at(10, 10)),
            (PAWN, 1, at(13, 10)),
            (TARGET, 1, at(14, 12)),
        ],
    );
    // A second, unrelated unit is told to attack the pawn that the first one kills.
    let more = spawn(&mut w, &[(GUNNER, 0, at(8, 12))]);
    let tick = w.tick();
    w.schedule(tick, PlayerId(0), attack(ids[0], ids[1]))
        .unwrap();
    w.schedule(tick, PlayerId(0), attack(more[0], ids[1]))
        .unwrap();
    w.step();
    assert!(!w.is_alive(ids[1]));
    run(&mut w, 2);
    assert_eq!(w.order_of(ids[0]), Some(Order::Idle));
    assert_eq!(w.order_of(more[0]), Some(Order::Idle));
    // No shot is fired at the dead, and an order against the stale id is ignored.
    let tick = w.tick();
    w.schedule(tick, PlayerId(0), attack(ids[0], ids[1]))
        .unwrap();
    let report = w.step();
    assert_eq!((report.commands_executed, report.commands_ignored), (0, 1));
    assert!(w.shots().is_empty());
}

#[test]
fn a_turret_aim_and_a_guard_on_something_that_died_let_go() {
    let mut w = doomed();
    let ids = spawn(
        &mut w,
        &[
            (GUNNER, 0, at(10, 10)),
            (PAWN, 1, at(13, 10)),
            (GUNNER, 0, at(8, 12)),
        ],
    );
    go(
        &mut w,
        vec![
            order(ids[2], Order::Guard(GuardTarget::Entity(ids[1]))),
            attack(ids[0], ids[1]),
        ],
        1,
    );
    assert!(!w.is_alive(ids[1]));
    run(&mut w, 30);
    // The guard of something that is gone ends its order: it stands idle, not walking to a ghost.
    assert_eq!(w.order_of(ids[2]), Some(Order::Idle));
}

#[test]
fn an_engagement_on_a_victim_that_died_ends_and_the_unit_looks_for_the_next() {
    let mut w = doomed_and_fought();
    let ids = spawn_fresh(
        &mut w,
        &[
            (GUNNER, 0, at(10, 10)),
            (PAWN, 1, at(13, 10)),
            (PAWN, 1, at(10, 15)),
        ],
    );
    // The gunner picks a pawn on the first tick and kills it with one shot; it must pick the other
    // afterwards (within the scan cooldown) and walk to it or shoot it.
    assert_eq!(
        w.deaths().len(),
        1,
        "the nearer pawn fell on the tick it appeared"
    );
    let shots = go(&mut w, vec![], 60);
    assert!(!shots.is_empty());
    assert!(!w.is_alive(ids[1]) && !w.is_alive(ids[2]), "both pawns die");
    assert_eq!(w.tally_of(PlayerId(0)).kills, 2);
    assert_eq!(w.engagement_of(ids[0]), None);
    assert_eq!(w.order_of(ids[0]), Some(Order::Idle));
    // Nobody is left to shoot at.
    let n = go(&mut w, vec![], 40).len();
    assert_eq!(n, 0);
}

#[test]
fn a_guided_rocket_whose_target_was_destroyed_by_someone_else_goes_off_where_it_was_headed() {
    let mut w = doomed();
    let ids = spawn(
        &mut w,
        &[
            (ROCKETEER, 0, at(10, 10)),
            (TARGET, 1, at(16, 10)),
            (BRUISER, 2, at(16, 14)),
            (BRUISER, 2, at(17, 14)),
        ],
    );
    go(&mut w, vec![attack(ids[0], ids[1])], 2);
    assert_eq!(w.projectile_count(), 1);
    // Another side destroys the target while the rocket is up.
    go(
        &mut w,
        vec![attack(ids[2], ids[1]), attack(ids[3], ids[1])],
        1,
    );
    assert!(!w.is_alive(ids[1]));
    assert_eq!(w.projectile_count(), 1, "the rocket still flies");
    let mut impact = None;
    for _ in 0..80 {
        w.step();
        if let Some(i) = w.impacts().first() {
            impact = Some(*i);
            break;
        }
    }
    let impact = impact.expect("the rocket went off");
    assert_eq!(impact.victim, None, "nobody to hit");
    assert_eq!(w.projectile_count(), 0);
    // The rocket's owner did not score: the kill was the bruisers'.
    assert_eq!(w.tally_of(PlayerId(0)).kills, 0);
    assert_eq!(w.tally_of(PlayerId(2)).kills, 1);
}

#[test]
fn a_formation_goes_on_when_one_of_its_members_is_destroyed() {
    let mut w = gap_world();
    let mut list: Vec<Spawn> = (0..4).map(|i| (PAWN, 0, at(4, 2 + i * 2))).collect();
    list.push((GUNNER, 1, at(8, 4)));
    let ids = spawn(&mut w, &list);
    let march = Command::Order {
        ids: ids[..4].to_vec(),
        order: Order::Move(at(16, 6)),
        queued: false,
    };
    go(&mut w, vec![march], 5);
    assert!(
        w.formation_of(ids[1]).is_some(),
        "they march as a formation"
    );
    // One is shot dead on the way.
    go(&mut w, vec![attack(ids[4], ids[0])], 1);
    assert!(!w.is_alive(ids[0]), "the first member is dead");
    run(&mut w, 900);
    for id in &ids[1..4] {
        assert_eq!(w.order_of(*id), Some(Order::Idle), "{id:?} arrived");
        let p = w.get(*id).unwrap().pos;
        assert!(p.x > Fx::from_int(13), "{p:?}");
    }
}

// ---- reports -----------------------------------------------------------------------------------------

#[test]
fn the_snapshot_lists_the_deaths_of_the_last_tick_with_where_what_and_how_big() {
    let mut w = doomed();
    let ids = spawn(
        &mut w,
        &[
            (BRUISER, 0, at(10, 10)),
            (BRUISER, 0, at(10, 12)),
            (TARGET, 1, at(14, 11)),
            (WALL, 1, at(30, 30)),
        ],
    );
    go(
        &mut w,
        vec![attack(ids[0], ids[2]), attack(ids[1], ids[2])],
        1,
    );
    let deaths = w.snapshot().deaths();
    assert_eq!(deaths.len(), 1);
    let d = &deaths[0];
    assert_eq!(
        (d.kind, d.pos, d.size, d.class),
        (
            TARGET,
            at(14, 11),
            Fx::from_ratio(15, 10),
            UnitClass::Mobile
        )
    );
    assert_eq!(d.wreck, Some(WreckKind::Vehicle));
    assert!(d.killer.is_some());
    // The next tick replaces the list.
    run(&mut w, 1);
    assert!(w.snapshot().deaths().is_empty());
}

#[test]
fn a_destroyed_structure_reports_its_footprint_and_no_size() {
    let mut w = doomed();
    let ids = spawn(
        &mut w,
        &[
            (WALL, 1, at(30, 30)),
            (BRUISER, 0, at(26, 30)),
            (BRUISER, 0, at(26, 31)),
        ],
    );
    go(
        &mut w,
        vec![attack(ids[1], ids[0]), attack(ids[2], ids[0])],
        1,
    );
    let d = &w.snapshot().deaths()[0];
    assert_eq!(
        (d.kind, d.class, d.size, d.footprint, d.pos),
        (
            WALL,
            UnitClass::Structure,
            Fx::ZERO,
            Some(Footprint {
                width: 2,
                height: 2
            }),
            at(31, 31)
        )
    );
    assert_eq!(d.wreck, Some(WreckKind::Rubble));
}

// ---- order independence ------------------------------------------------------------------------------

/// What a fight came to, by where each unit started: who lived (with its hit points and kills), the
/// wrecks, the tallies, the deaths with their killers and assists.
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    alive: Vec<(usize, u32, u16)>,
    wrecks: Vec<(WreckKind, Kind, i64, i64)>,
    tally: Vec<(u8, u32, u32, u32)>,
    deaths: Vec<(usize, Option<usize>, Vec<usize>, u64)>,
    free_cells: usize,
}

fn fight(cast: &[Spawn], order: &[usize], ticks: u32) -> (Outcome, u64) {
    let mut w = doomed_and_fought();
    let list: Vec<Spawn> = order.iter().map(|&i| cast[i]).collect();
    let ids = spawn_fresh(&mut w, &list);
    let cast_of = |id: EntityId| order[ids.iter().position(|&x| x == id).unwrap()];
    let mut deaths: Vec<(usize, Option<usize>, Vec<usize>, u64)> = w
        .deaths()
        .iter()
        .map(|d| {
            (
                cast_of(d.victim),
                d.killer.map(|k| cast_of(k.shooter)),
                d.assists
                    .iter()
                    .map(|a| cast_of(a.shooter))
                    .collect::<Vec<_>>(),
                d.tick.0,
            )
        })
        .collect();
    for _ in 0..ticks {
        w.step();
        for d in w.deaths() {
            deaths.push((
                cast_of(d.victim),
                d.killer.map(|k| cast_of(k.shooter)),
                d.assists
                    .iter()
                    .map(|a| cast_of(a.shooter))
                    .collect::<Vec<_>>(),
                d.tick.0,
            ));
        }
    }
    deaths.sort();
    let mut alive: Vec<(usize, u32, u16)> = ids
        .iter()
        .filter(|&&id| w.is_alive(id))
        .map(|&id| (cast_of(id), w.hp_of(id).unwrap(), w.kills_of(id).unwrap()))
        .collect();
    alive.sort();
    let mut wrecks: Vec<_> = w
        .wrecks()
        .iter()
        .map(|x| (x.kind, x.unit, x.pos.x.raw(), x.pos.y.raw()))
        .collect();
    wrecks.sort();
    let tally = (0..12u8)
        .map(|p| {
            let t = w.tally_of(PlayerId(p));
            (p, t.kills, t.assists, t.losses)
        })
        .filter(|t| t.1 + t.2 + t.3 > 0)
        .collect();
    let outcome = Outcome {
        alive,
        wrecks,
        tally,
        deaths,
        free_cells: usize::from(w.occupied_cells() == 0),
    };
    (outcome, w.checksum())
}

fn in_every_order(cast: &[Spawn], ticks: u32) -> Outcome {
    let mut seen: Option<Outcome> = None;
    for order in permutations(cast.len()) {
        let (outcome, checksum) = fight(cast, &order, ticks);
        assert_eq!(fight(cast, &order, ticks).1, checksum, "{order:?} repeats");
        match &seen {
            Some(first) => assert_eq!(&outcome, first, "spawn order {order:?}"),
            None => seen = Some(outcome),
        }
    }
    seen.unwrap()
}

#[test]
fn a_fight_with_deaths_comes_out_the_same_in_every_spawn_order() {
    // Two gunners and a tank against a gunner and a pawn, all within each other's reach; they pick
    // their own targets and the first dead fall within a second.
    let cast: [Spawn; 5] = [
        (GUNNER, 0, at(10, 10)),
        (GUNNER, 0, at(12, 14)),
        (TANK, 0, at(8, 12)),
        (GUNNER, 1, at(14, 11)),
        (PAWN, 1, at(11, 13)),
    ];
    let o = in_every_order(&cast, 300);
    assert!(!o.deaths.is_empty(), "{o:?}");
    assert!(o.tally.iter().map(|t| t.1).sum::<u32>() > 0);
}

#[test]
fn structures_that_fall_on_the_same_tick_free_the_same_cells_in_every_spawn_order() {
    // Two walls side by side shot down together by four bruisers: afterwards the same cells are free,
    // the same rubble lies there and the same shooters are credited.
    let cast: [Spawn; 6] = [
        (WALL, 1, at(20, 20)),
        (WALL, 1, at(22, 20)),
        (BRUISER, 0, at(16, 20)),
        (BRUISER, 0, at(16, 21)),
        (BRUISER, 0, at(26, 20)),
        (BRUISER, 0, at(26, 21)),
    ];
    let mut seen: Option<(Vec<_>, Vec<_>, Vec<_>)> = None;
    for order in permutations(cast.len()) {
        let mut w = doomed();
        let list: Vec<Spawn> = order.iter().map(|&i| cast[i]).collect();
        let ids = spawn(&mut w, &list);
        let id_of = |i: usize| ids[order.iter().position(|&o| o == i).unwrap()];
        let cast_of = |id: EntityId| order[ids.iter().position(|&x| x == id).unwrap()];
        go(
            &mut w,
            vec![
                attack(id_of(2), id_of(0)),
                attack(id_of(3), id_of(0)),
                attack(id_of(4), id_of(1)),
                attack(id_of(5), id_of(1)),
            ],
            1,
        );
        assert_eq!(w.occupied_cells(), 0, "{order:?}");
        let mut killers: Vec<_> = w
            .deaths()
            .iter()
            .map(|d| (cast_of(d.victim), d.killer.map(|k| cast_of(k.shooter))))
            .collect();
        killers.sort();
        let wrecks: Vec<_> = w
            .wrecks()
            .iter()
            .map(|r| (r.anchor, r.footprint, r.unit.0))
            .collect();
        let free: Vec<_> = (18..26)
            .flat_map(|x| (18..24).map(move |y| (x, y)))
            .map(|(x, y)| w.is_occupied(Cell::new(x, y)))
            .collect();
        let out = (killers, wrecks, free);
        match &seen {
            Some(first) => assert_eq!(&out, first, "{order:?}"),
            None => seen = Some(out),
        }
    }
}

#[test]
fn the_same_fight_twice_gives_the_same_checksum_every_tick() {
    let cast: [Spawn; 4] = [
        (GUNNER, 0, at(10, 10)),
        (TANK, 0, at(8, 12)),
        (GUNNER, 1, at(14, 11)),
        (PAWN, 1, at(11, 13)),
    ];
    let run_once = || {
        let mut w = doomed_and_fought();
        spawn_fresh(&mut w, &cast);
        run(&mut w, 200);
        w.checksum_history().to_vec()
    };
    assert_eq!(run_once(), run_once());
}
