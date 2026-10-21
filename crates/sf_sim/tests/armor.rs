//! The armor table and splash damage (WP5.3), through the public API: a hit takes the percentage of its
//! damage type against the victim's armor, a splash hurts everything near the detonation with a falloff,
//! the direct victim is hurt once, friendly fire is a flag of the weapon, the shooter is no exception, and
//! none of it depends on the order of the entities, the projectiles or the spatial query.
//!
//! The numbers are small and invented so a test can follow them by hand; the shipped set is
//! `assets/config`, which `tools/sf_headless/tests/armour_splash.rs` runs.
//!
//! Positions are given in half cells by [`hc`], so that the distances in the hand calculations are exact.
//! A body is a disc of half the unit's `size` (here one cell, so a radius of half a cell); the wall is a
//! structure of two by two cells and a radius of one.

use sf_math::{Fx, Vec2};
use sf_sim::armor::scaled_damage;
use sf_sim::{
    ArmorError, ArmorTable, Cell, Command, ConfigError, EntityId, Footprint, Impact, Kind, Loadout,
    Mount, Order, PlayerId, ProjectileKind, Shot, UnitStats, UnitTable, WeaponStats, WeaponTable,
    WeaponsError, World, WorldConfig, per_tick,
};

const MAP: u32 = 64;

// Damage types, with the numbers of the shipped file.
const BULLET: u16 = 0;
const BLAST: u16 = 4;
// Armor types, likewise.
const NONE: u16 = 0;
const SOFT: u16 = 1;
const HARD: u16 = 3;
const STRUCT: u16 = 4;

// Shooters.
const RIFLEMAN: Kind = Kind(1); // a rifle: 6 bullet damage
const PISTOLEER: Kind = Kind(2); // a pistol: 4 bullet damage
const THROWER: Kind = Kind(3); // 100 blast damage, splash 3 with 1 at full strength, hurts its own side
const CAREFUL: Kind = Kind(4); // the same, but spares its own side
const NUKER: Kind = Kind(5); // 100 blast damage, splash 6 falling from the start, hurts its own side; 30 hp
const CAREFUL_NUKER: Kind = Kind(6); // the same, sparing its own side
const BOMBER: Kind = Kind(7); // a falling bomb of 100 blast damage, splash 3 with 1 at full strength
const PEBBLER: Kind = Kind(8); // 3 blast damage, splash 3 falling from the start
const GLASS: Kind = Kind(9); // a thrower with 20 hit points
// Things that are shot at.
const TROOPER: Kind = Kind(20); // soft, 1000 hit points
const TANK: Kind = Kind(21); // hard, 1000
const WALL: Kind = Kind(22); // a structure of 2 by 2 cells, 1000
const GHOST: Kind = Kind(23); // an armor that takes nothing, 100 hit points
const ROCK: Kind = Kind(24); // an object that cannot be hurt (zero hit points)
const WALKER: Kind = Kind(25); // soft, 1000, walks half a cell a tick
const FRAGILE: Kind = Kind(26); // soft, 120

const RIFLE: u16 = 1;
const PISTOL: u16 = 2;
const GRENADE: u16 = 3;
const CAREFUL_GRENADE: u16 = 4;
const NUKE: u16 = 5;
const CAREFUL_NUKE: u16 = 6;
const BOMB: u16 = 7;
const PEBBLE: u16 = 8;

/// Half cells to cells.
fn hc(x: i64, y: i64) -> Vec2 {
    Vec2::new(Fx::from_ratio(x, 2), Fx::from_ratio(y, 2))
}

fn at(x: i32, y: i32) -> Vec2 {
    Vec2::from_ints(x, y)
}

fn mobile(kind: Kind, hp: u32, range: i32, speed: Fx) -> UnitStats {
    UnitStats {
        kind,
        hp,
        movement_class: Some(0),
        speed_per_tick: speed,
        turn_per_tick: 16,
        size: Fx::ONE,
        footprint: None,
        weapon_range: Fx::from_int(range),
        turret_per_tick: 0,
    }
}

fn still(kind: Kind, hp: u32, range: i32) -> UnitStats {
    mobile(kind, hp, range, per_tick(Fx::ONE))
}

fn units() -> UnitTable {
    UnitTable::new(vec![
        still(RIFLEMAN, 50, 6),
        still(PISTOLEER, 50, 4),
        still(THROWER, 50, 4),
        still(CAREFUL, 50, 4),
        still(NUKER, 30, 4),
        still(CAREFUL_NUKER, 30, 4),
        still(BOMBER, 50, 8),
        still(PEBBLER, 50, 4),
        still(GLASS, 20, 4),
        still(TROOPER, 1000, 0),
        still(TANK, 1000, 0),
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
        still(GHOST, 100, 0),
        UnitStats {
            movement_class: None,
            speed_per_tick: Fx::ZERO,
            turn_per_tick: 0,
            ..still(ROCK, 0, 0)
        },
        mobile(WALKER, 1000, 0, Fx::HALF),
        still(FRAGILE, 120, 0),
    ])
    .unwrap()
}

fn weapon(kind: u16, damage_type: u16, damage: u32, range: i32) -> WeaponStats {
    WeaponStats {
        kind,
        range: Fx::from_int(range),
        min_range: Fx::ZERO,
        reload_ticks: 20,
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

fn splash(w: WeaponStats, radius: i32, inner: i32) -> WeaponStats {
    WeaponStats {
        splash_radius: Fx::from_int(radius),
        splash_inner: Fx::from_int(inner),
        ..w
    }
}

fn weapons() -> Vec<WeaponStats> {
    vec![
        weapon(RIFLE, BULLET, 6, 6),
        weapon(PISTOL, BULLET, 4, 4),
        splash(weapon(GRENADE, BLAST, 100, 4), 3, 1),
        WeaponStats {
            friendly_fire: false,
            ..splash(weapon(CAREFUL_GRENADE, BLAST, 100, 4), 3, 1)
        },
        splash(weapon(NUKE, BLAST, 100, 4), 6, 0),
        WeaponStats {
            friendly_fire: false,
            ..splash(weapon(CAREFUL_NUKE, BLAST, 100, 4), 6, 0)
        },
        WeaponStats {
            projectile: ProjectileKind::Falling,
            projectile_speed_per_tick: Fx::from_ratio(1, 4),
            flight_height: Fx::from_int(3),
            ..splash(weapon(BOMB, BLAST, 100, 8), 3, 1)
        },
        splash(weapon(PEBBLE, BLAST, 3, 4), 3, 0),
    ]
}

fn loadouts() -> Vec<Loadout> {
    let l = |unit: Kind, w: u16| Loadout {
        unit,
        weapons: vec![w],
    };
    vec![
        l(RIFLEMAN, RIFLE),
        l(PISTOLEER, PISTOL),
        l(THROWER, GRENADE),
        l(CAREFUL, CAREFUL_GRENADE),
        l(NUKER, NUKE),
        l(CAREFUL_NUKER, CAREFUL_NUKE),
        l(BOMBER, BOMB),
        l(PEBBLER, PEBBLE),
        l(GLASS, GRENADE),
    ]
}

/// The armor type of each unit kind.
fn armor_of(kind: Kind) -> u16 {
    match kind {
        TANK => HARD,
        WALL => STRUCT,
        GHOST | ROCK => NONE,
        _ => SOFT,
    }
}

fn every_kind() -> Vec<Kind> {
    units().iter().map(|u| u.kind).collect()
}

/// Bullets: soft 100, hard 10, structure 50. Blast: soft 100, hard 50, structure 200. The `none` armor
/// takes nothing from either.
fn percent_cells() -> Vec<(u16, u16, u16)> {
    vec![
        (BULLET, NONE, 0),
        (BULLET, SOFT, 100),
        (BULLET, HARD, 10),
        (BULLET, STRUCT, 50),
        (BLAST, NONE, 0),
        (BLAST, SOFT, 100),
        (BLAST, HARD, 50),
        (BLAST, STRUCT, 200),
    ]
}

fn armor_table_with(cells: Vec<(u16, u16, u16)>) -> ArmorTable {
    ArmorTable::new(
        vec![BULLET, BLAST],
        vec![NONE, SOFT, HARD, STRUCT],
        cells,
        every_kind().into_iter().map(|k| (k, armor_of(k))).collect(),
    )
    .unwrap()
}

fn armor_table() -> ArmorTable {
    armor_table_with(percent_cells())
}

fn table() -> WeaponTable {
    WeaponTable::new(weapons(), loadouts())
        .unwrap()
        .with_armor(armor_table())
        .unwrap()
}

fn world_with(table: WeaponTable) -> World {
    World::new(WorldConfig::new(5).with_map_size(MAP, MAP))
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_weapons(table)
        .unwrap()
}

fn armed() -> World {
    world_with(table())
}

/// What to put in the world: a unit (kind, place, owner) or a wall (anchor cell, owner).
#[derive(Copy, Clone, Debug)]
enum Spawn {
    Unit(Kind, Vec2, u8),
    Wall(u32, u32, u8),
}

impl Spawn {
    fn command(self) -> Command {
        match self {
            Spawn::Unit(kind, pos, owner) => Command::SpawnUnit {
                kind,
                pos,
                owner: PlayerId(owner),
            },
            Spawn::Wall(x, y, owner) => Command::PlaceStructure {
                kind: WALL,
                cell: Cell { x, y },
                owner: PlayerId(owner),
            },
        }
    }
}

/// Puts the list in the world on one tick, in that order: the entity of the `i`-th item is slot `i`.
fn spawn(w: &mut World, list: &[Spawn]) -> Vec<EntityId> {
    let tick = w.tick();
    for s in list {
        w.schedule(tick, PlayerId(0), s.command()).unwrap();
    }
    w.step();
    let ids: Vec<EntityId> = (0..list.len())
        .map(|i| EntityId::new(i as u32, 0))
        .collect();
    for (id, s) in ids.iter().zip(list) {
        assert!(w.get(*id).is_some(), "{s:?} was not placed");
    }
    ids
}

fn attack(who: EntityId, victim: EntityId) -> Command {
    Command::Order {
        ids: vec![who],
        order: Order::Attack(victim),
        queued: false,
    }
}

/// Everything the ticks of a run reported.
#[derive(Default)]
struct Log {
    shots: Vec<Shot>,
    impacts: Vec<Impact>,
}

/// Gives the commands on the next tick and runs `n` ticks in all.
fn go(w: &mut World, commands: Vec<Command>, n: u32) -> Log {
    let tick = w.tick();
    for c in commands {
        w.schedule(tick, PlayerId(0), c).unwrap();
    }
    let mut log = Log::default();
    for _ in 0..n {
        w.step();
        log.shots.extend_from_slice(w.shots());
        log.impacts.extend_from_slice(w.impacts());
    }
    log
}

fn hp(w: &World, id: EntityId) -> u32 {
    w.hp_of(id).unwrap()
}

// ---- the armor table on a direct hit --------------------------------------------------------------

#[test]
fn a_direct_hit_takes_the_percentage_of_its_damage_type_against_the_armor() {
    let mut w = armed();
    let ids = spawn(
        &mut w,
        &[
            Spawn::Unit(RIFLEMAN, at(10, 10), 0),
            Spawn::Unit(RIFLEMAN, at(10, 20), 0),
            Spawn::Unit(RIFLEMAN, at(10, 30), 0),
            Spawn::Unit(TROOPER, at(14, 10), 1),
            Spawn::Unit(TANK, at(14, 20), 1),
            Spawn::Wall(13, 29, 1),
        ],
    );
    let log = go(
        &mut w,
        vec![
            attack(ids[0], ids[3]),
            attack(ids[1], ids[4]),
            attack(ids[2], ids[5]),
        ],
        1,
    );
    // The shots report the weapon's damage, before the armor.
    assert_eq!(log.shots.len(), 3);
    assert!(log.shots.iter().all(|s| s.damage == 6), "{:?}", log.shots);
    assert_eq!(hp(&w, ids[3]), 1000 - 6, "soft: 100 percent of 6");
    assert_eq!(
        hp(&w, ids[4]),
        1000 - 1,
        "hard: 10 percent of 6 is 3/5, which rounds to 1"
    );
    assert_eq!(hp(&w, ids[5]), 1000 - 3, "structure: 50 percent of 6");
}

#[test]
fn a_hit_never_rounds_down_to_nothing_but_an_immune_armor_takes_nothing() {
    let mut w = armed();
    let ids = spawn(
        &mut w,
        &[
            Spawn::Unit(PISTOLEER, at(10, 10), 0),
            Spawn::Unit(PISTOLEER, at(10, 20), 0),
            Spawn::Unit(PISTOLEER, at(10, 30), 0),
            Spawn::Unit(TANK, at(13, 10), 1),
            Spawn::Unit(GHOST, at(13, 20), 1),
            Spawn::Unit(TROOPER, at(13, 30), 1),
        ],
    );
    let log = go(
        &mut w,
        vec![
            attack(ids[0], ids[3]),
            attack(ids[1], ids[4]),
            attack(ids[2], ids[5]),
        ],
        1,
    );
    assert_eq!(log.shots.len(), 3, "all three fired");
    // 10 percent of 4 is 2/5: it would round to nothing, and the minimum makes it one.
    assert_eq!(hp(&w, ids[3]), 1000 - 1);
    // 0 percent: no damage, and no minimum.
    assert_eq!(hp(&w, ids[4]), 100);
    assert_eq!(hp(&w, ids[5]), 1000 - 4);
}

#[test]
fn a_world_without_an_armor_table_takes_every_hit_at_the_weapons_damage() {
    let plain = WeaponTable::new(weapons(), loadouts()).unwrap();
    assert!(plain.armor().is_none());
    let mut w = world_with(plain);
    let ids = spawn(
        &mut w,
        &[
            Spawn::Unit(RIFLEMAN, at(10, 10), 0),
            Spawn::Unit(TANK, at(14, 10), 1),
        ],
    );
    go(&mut w, vec![attack(ids[0], ids[1])], 1);
    assert_eq!(
        hp(&w, ids[1]),
        1000 - 6,
        "the tank takes all 6, as before WP5.3"
    );
}

#[test]
fn a_projectile_hit_goes_through_the_armor_too() {
    let mut w = armed();
    let ids = spawn(
        &mut w,
        &[
            Spawn::Unit(BOMBER, at(20, 10), 0),
            Spawn::Unit(TANK, at(14, 10), 1),
        ],
    );
    let log = go(&mut w, vec![attack(ids[0], ids[1])], 14);
    let hit = log.impacts[0];
    assert_eq!(hit.victim, Some(ids[1]));
    assert_eq!(
        hit.damage, 50,
        "50 percent of 100, as the Impact reports it"
    );
    assert_eq!(hit.splash_victims, 0);
    assert_eq!(hp(&w, ids[1]), 1000 - 50);
}

// ---- splash -----------------------------------------------------------------------------------------

/// The cast of the hand-worked blast: a shooter (outside the splash) and a trooper `T` that the blast is
/// centred on, with the bystanders at known distances. The grenade has a splash radius of 3 and full
/// strength out to 1, measured from the edge of a body (a radius of half a cell; the wall has one).
///
/// ```text
///   who                    centre distance  edge   falloff      percent  damage
///   T   trooper (direct)   -                -      -            100      100
///   B1  trooper            2                1.5    3/4          100      75
///   B6  tank               2                1.5    3/4          50       37.5, a tie, up: 38
///   B3  trooper            1.5              1      1 (inside)   100      100
///   B2  tank               2.5              2      1/2          50       25
///   W   wall (radius 1)    3                2      1/2          200      100
///   B4  tank               3.5              3      0 (the edge) 50       0
/// ```
fn blast_cast(shooter: Spawn) -> Vec<Spawn> {
    vec![
        shooter,                             // 0: outside the splash
        Spawn::Unit(TROOPER, at(10, 10), 1), // 1: T
        Spawn::Unit(TROOPER, at(12, 10), 1), // 2: B1
        Spawn::Unit(TANK, at(8, 10), 1),     // 3: B6
        Spawn::Unit(TROOPER, hc(20, 23), 1), // 4: B3 at (10, 11.5)
        Spawn::Unit(TANK, hc(20, 27), 1),    // 5: B4 at (10, 13.5)
        Spawn::Wall(9, 6, 1),                // 6: W, its middle at (10, 7)
        Spawn::Unit(TANK, hc(17, 16), 1),    // 7: B2 at (8.5, 8): 1.5 west and 2 north of T
    ]
}

#[test]
fn a_splash_falls_off_with_the_distance_from_the_edge_and_takes_the_armor_too() {
    let mut w = armed();
    // 4.3 cells from T, so its own edge is 3.8 from the blast.
    let ids = spawn(&mut w, &blast_cast(Spawn::Unit(THROWER, hc(27, 15), 0)));
    let shots = go(&mut w, vec![attack(ids[0], ids[1])], 1).shots;
    assert_eq!(shots.len(), 1);
    assert_eq!(hp(&w, ids[0]), 50, "the shooter is outside the splash");
    assert_eq!(hp(&w, ids[1]), 1000 - 100, "the direct victim: 100, once");
    assert_eq!(hp(&w, ids[2]), 1000 - 75, "B1: edge 1.5, 3/4 of 100");
    assert_eq!(
        hp(&w, ids[3]),
        1000 - 38,
        "B6: 100 * 3/4 * 1/2 = 37.5, a tie, rounded up"
    );
    assert_eq!(
        hp(&w, ids[4]),
        1000 - 100,
        "B3: edge 1, still full strength"
    );
    assert_eq!(
        hp(&w, ids[5]),
        1000,
        "B4: edge 3, the radius itself is outside"
    );
    assert_eq!(hp(&w, ids[6]), 1000 - 100, "W: edge 2, 1/2 of 200 percent");
    assert_eq!(hp(&w, ids[7]), 1000 - 25, "B2: edge 2, 1/2 of 50 percent");
}

#[test]
fn the_direct_victim_is_hurt_once_and_the_report_counts_the_others() {
    let mut w = armed();
    let ids = spawn(&mut w, &blast_cast(Spawn::Unit(BOMBER, at(16, 10), 0)));
    let log = go(&mut w, vec![attack(ids[0], ids[1])], 14);
    assert_eq!(log.impacts.len(), 1);
    let hit = log.impacts[0];
    assert_eq!(hit.victim, Some(ids[1]));
    assert_eq!(hit.damage, 100, "the direct hit alone: 100, not 200");
    // B1, B6, B3, the wall and B2 are hurt; B4 is outside the radius.
    assert_eq!(hit.splash_victims, 5);
    assert_eq!(hp(&w, ids[1]), 1000 - 100);
    assert_eq!(hp(&w, ids[2]), 1000 - 75);
    assert_eq!(hp(&w, ids[5]), 1000);
}

#[test]
fn a_hit_on_a_bystander_near_the_edge_of_the_radius_is_still_one_hit_point() {
    // The pebble does 3, the radius is 3 and the falloff starts at once. A trooper whose edge is at 2.9375
    // (centre distance 55/16): 3 * (1/16) / 3 = 1/16, which rounds to nothing; the minimum makes it 1.
    let mut w = armed();
    let ids = spawn(
        &mut w,
        &[
            Spawn::Unit(PEBBLER, at(10, 14), 0),
            Spawn::Unit(TROOPER, at(10, 10), 1),
            Spawn::Unit(
                TROOPER,
                Vec2::new(Fx::from_ratio(160 + 55, 16), Fx::from_int(10)),
                1,
            ),
        ],
    );
    go(&mut w, vec![attack(ids[0], ids[1])], 1);
    assert_eq!(hp(&w, ids[1]), 1000 - 3);
    assert_eq!(hp(&w, ids[2]), 1000 - 1);
}

#[test]
fn a_splash_that_misses_its_target_hurts_what_is_near_where_it_lands() {
    let mut w = armed();
    let ids = spawn(
        &mut w,
        &[
            Spawn::Unit(BOMBER, at(10, 16), 0),
            Spawn::Unit(WALKER, at(10, 10), 1),
            Spawn::Unit(TROOPER, at(8, 10), 1),
            Spawn::Unit(TROOPER, at(30, 30), 1),
        ],
    );
    let log = go(
        &mut w,
        vec![
            attack(ids[0], ids[1]),
            Command::Order {
                ids: vec![ids[1]],
                order: Order::Move(at(40, 10)),
                queued: false,
            },
        ],
        16,
    );
    let hit = log.impacts[0];
    // The bomb fell where the walker stood; the walker was gone.
    assert_eq!(hit.victim, None);
    assert_eq!(hit.damage, 0);
    assert_eq!(
        hp(&w, ids[1]),
        1000,
        "the walker is out of the radius by then"
    );
    // The bystander took the splash, from the landing point.
    let edge = (w.get(ids[2]).unwrap().pos - hit.at).length() - Fx::HALF;
    assert!(
        edge < Fx::from_int(3),
        "the bomb fell near the trooper: {edge:?}"
    );
    assert!(hp(&w, ids[2]) < 1000);
    assert_eq!(hit.splash_victims, 1);
    assert_eq!(hp(&w, ids[3]), 1000, "far away");
}

#[test]
fn friendly_fire_is_a_flag_of_the_weapon() {
    for (shooter, spared) in [(THROWER, false), (CAREFUL, true)] {
        let mut w = armed();
        let ids = spawn(
            &mut w,
            &[
                Spawn::Unit(shooter, at(14, 11), 0),
                Spawn::Unit(TROOPER, at(10, 10), 1), // the victim
                Spawn::Unit(TROOPER, at(12, 10), 0), // a friend of the shooter
                Spawn::Unit(TROOPER, at(10, 12), 1), // an enemy, as near
            ],
        );
        go(&mut w, vec![attack(ids[0], ids[1])], 1);
        assert_eq!(hp(&w, ids[1]), 900);
        assert_eq!(hp(&w, ids[3]), 1000 - 75, "the enemy is always hurt");
        let friend = if spared { 1000 } else { 1000 - 75 };
        assert_eq!(hp(&w, ids[2]), friend, "{shooter:?}");
    }
}

#[test]
fn a_direct_victim_of_the_shooters_own_side_is_hurt_even_when_the_splash_spares_it() {
    let mut w = armed();
    let ids = spawn(
        &mut w,
        &[
            Spawn::Unit(CAREFUL, at(14, 11), 0),
            Spawn::Unit(TROOPER, at(10, 10), 0), // ordered to be shot, though it is a friend
            Spawn::Unit(TROOPER, at(12, 10), 0),
        ],
    );
    go(&mut w, vec![attack(ids[0], ids[1])], 1);
    assert_eq!(hp(&w, ids[1]), 900);
    assert_eq!(hp(&w, ids[2]), 1000);
}

#[test]
fn the_shooter_is_hurt_by_its_own_splash() {
    // Four cells from its victim, edge 3.5 from the blast of radius 6: 100 * (6 - 3.5) / 6 = 41.67, which
    // is 42. The nuker has 30 hit points.
    let mut w = armed();
    let ids = spawn(
        &mut w,
        &[
            Spawn::Unit(NUKER, at(14, 10), 0),
            Spawn::Unit(TROOPER, at(10, 10), 1),
        ],
    );
    go(&mut w, vec![attack(ids[0], ids[1])], 1);
    assert_eq!(hp(&w, ids[1]), 900);
    assert_eq!(hp(&w, ids[0]), 0, "42 off 30 hit points");
    // The careful one spares itself, as it spares everything of its side.
    let mut w = armed();
    let ids = spawn(
        &mut w,
        &[
            Spawn::Unit(CAREFUL_NUKER, at(14, 10), 0),
            Spawn::Unit(TROOPER, at(10, 10), 1),
        ],
    );
    go(&mut w, vec![attack(ids[0], ids[1])], 1);
    assert_eq!(hp(&w, ids[0]), 30);
}

#[test]
fn a_unit_already_down_is_not_hurt_or_counted_by_the_next_burst() {
    // A bomber drops a bomb every 20 ticks on the trooper; the fragile unit two cells away takes 75 off its
    // 120 the first time, the rest the second time, and is down when the third bomb lands.
    let mut w = armed();
    let ids = spawn(
        &mut w,
        &[
            Spawn::Unit(BOMBER, at(16, 10), 0),
            Spawn::Unit(TROOPER, at(10, 10), 1),
            Spawn::Unit(FRAGILE, at(10, 12), 1),
        ],
    );
    let log = go(&mut w, vec![attack(ids[0], ids[1])], 60);
    assert_eq!(log.impacts.len(), 3);
    assert_eq!(
        log.impacts
            .iter()
            .map(|i| i.splash_victims)
            .collect::<Vec<_>>(),
        [1, 1, 0],
        "the third burst finds the fragile unit already at zero"
    );
    assert_eq!(hp(&w, ids[2]), 0);
    assert_eq!(hp(&w, ids[1]), 1000 - 300);
}

#[test]
fn what_cannot_be_hurt_or_is_down_is_not_in_a_splash() {
    let mut w = armed();
    let ids = spawn(
        &mut w,
        &[
            Spawn::Unit(BOMBER, at(16, 10), 0),
            Spawn::Unit(TROOPER, at(10, 10), 1),
            Spawn::Unit(GHOST, at(12, 10), 1), // takes 0 percent
            Spawn::Unit(ROCK, at(10, 12), 1),  // zero hit points
        ],
    );
    let log = go(&mut w, vec![attack(ids[0], ids[1])], 14);
    assert_eq!(hp(&w, ids[2]), 100);
    assert_eq!(hp(&w, ids[3]), 0);
    assert_eq!(log.impacts[0].splash_victims, 0, "nobody was hurt");
}

/// A deterministic spread of units on a lattice of 5/4 cells (so that no two bodies overlap), over
/// several buckets of the spatial index and across the borders between them.
fn lattice() -> Vec<Spawn> {
    let mut list = Vec::new();
    let mut state: u32 = 0x2545_f491;
    for gy in 0..10_i64 {
        for gx in 0..10_i64 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let pick = (state >> 16) % 8;
            let pos = Vec2::new(
                Fx::from_int(5) + Fx::from_ratio(5 * gx, 4),
                Fx::from_int(5) + Fx::from_ratio(5 * gy, 4),
            );
            let owner = ((state >> 8) as u8) % 2;
            match pick {
                0..=2 => list.push(Spawn::Unit(TROOPER, pos, owner)),
                3 | 4 => list.push(Spawn::Unit(TANK, pos, owner)),
                5 => list.push(Spawn::Unit(GHOST, pos, owner)),
                6 => list.push(Spawn::Unit(ROCK, pos, owner)),
                _ => {}
            }
        }
    }
    list
}

/// The damage a bystander at `pos` with `radius` and armor percentage `percent` takes from a blast of
/// `w` at `at`, worked out the plain way: no index, no shortcuts.
fn plain_splash(w: &WeaponStats, at: Vec2, pos: Vec2, radius: Fx, percent: u16) -> u32 {
    let edge = ((pos - at).length() - radius).max(Fx::ZERO);
    if edge >= w.splash_radius {
        return 0;
    }
    if edge <= w.splash_inner {
        return scaled_damage(w.damage, percent, 1, 1);
    }
    let out = (w.splash_radius - edge).raw() as u64;
    let width = (w.splash_radius - w.splash_inner).raw() as u64;
    scaled_damage(w.damage, percent, out, width)
}

#[test]
fn the_spatial_query_finds_exactly_what_a_plain_scan_finds() {
    // Each weapon is fired at a trooper in the middle of the lattice, four cells from the shooter, in a
    // world of its own, and every unit of the lattice is compared with the plain calculation. The lattice
    // lies over the borders between several buckets of the index.
    let centre = at(11, 11);
    let shooter_at = at(11, 15);
    let near = |p: Vec2, q: Vec2| (p - q).length() <= Fx::from_ratio(6, 5);
    for (weapon_kind, shooter) in [
        (GRENADE, THROWER),
        (NUKE, NUKER),
        (PEBBLE, PEBBLER),
        (CAREFUL_GRENADE, CAREFUL),
    ] {
        let stats = *table().weapon(weapon_kind).unwrap();
        let mut cast = vec![
            Spawn::Unit(shooter, shooter_at, 0),
            Spawn::Unit(TROOPER, centre, 1),
        ];
        cast.extend(lattice().into_iter().filter(|s| match s {
            Spawn::Unit(_, p, _) => !near(*p, centre) && !near(*p, shooter_at),
            Spawn::Wall(..) => true,
        }));
        let mut w = armed();
        let ids = spawn(&mut w, &cast);
        let start: Vec<u32> = ids.iter().map(|&i| hp(&w, i)).collect();
        let positions: Vec<Vec2> = ids.iter().map(|&i| w.get(i).unwrap().pos).collect();
        go(&mut w, vec![attack(ids[0], ids[1])], 1);
        assert_eq!(w.shots().len(), 1, "weapon {weapon_kind}");
        let armor = armor_table();
        let mut hurt = 0;
        for (n, &id) in ids.iter().enumerate().skip(1) {
            let Spawn::Unit(kind, _, owner) = cast[n] else {
                unreachable!("the lattice has no walls")
            };
            assert_eq!(w.get(id).unwrap().pos, positions[n], "nobody was pushed");
            let percent = armor.percent(BLAST, armor_of(kind)).unwrap();
            let expected = if n == 1 {
                scaled_damage(stats.damage, percent, 1, 1)
            } else if start[n] == 0 || (!stats.friendly_fire && owner == 0) {
                0
            } else {
                plain_splash(&stats, positions[1], positions[n], Fx::HALF, percent)
            };
            hurt += u32::from(n > 1 && expected > 0);
            assert_eq!(
                start[n] - hp(&w, id),
                expected,
                "weapon {weapon_kind}, entity {n} ({kind:?}) at {:?}",
                positions[n]
            );
        }
        // The shooter, four cells from the victim, is hurt by the blast of a weapon that does not spare it.
        let own = if stats.friendly_fire {
            plain_splash(&stats, positions[1], positions[0], Fx::HALF, 100)
        } else {
            0
        };
        assert_eq!(start[0] - hp(&w, ids[0]), own.min(start[0]), "the shooter");
        assert!(
            hurt > 5,
            "weapon {weapon_kind}: a spread that hurts something: {hurt}"
        );
    }
}

// ---- order independence ------------------------------------------------------------------------------

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

/// What a skirmish came to, by the position of each unit in the cast (not by its slot).
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    hp: Vec<u32>,
    /// (tick, shooter, target, damage) of every shot, sorted.
    shots: Vec<(u64, usize, usize, u32)>,
    /// (tick, owner, victim, damage, splash victims) of every impact, sorted.
    impacts: Vec<(u64, usize, Option<usize>, u32, u32)>,
}

/// Plays `cast` (spawned in `order`) with the attack `links` for `ticks`, and says what came of it.
fn skirmish(
    cast: &[Spawn],
    links: &[(usize, usize)],
    order: &[usize],
    ticks: u32,
) -> (Outcome, u64) {
    let mut w = armed();
    let list: Vec<Spawn> = order.iter().map(|&p| cast[p]).collect();
    let by_slot = spawn(&mut w, &list);
    let mut ids = vec![EntityId::new(0, 0); cast.len()];
    for (slot, &p) in order.iter().enumerate() {
        ids[p] = by_slot[slot];
    }
    let pos_of = |id: EntityId| ids.iter().position(|&i| i == id).unwrap();
    let commands = links.iter().map(|&(a, b)| attack(ids[a], ids[b])).collect();
    let log = go(&mut w, commands, ticks);
    let mut shots: Vec<_> = log
        .shots
        .iter()
        .map(|s| (s.tick.0, pos_of(s.attacker), pos_of(s.target), s.damage))
        .collect();
    shots.sort_unstable();
    let mut impacts: Vec<_> = log
        .impacts
        .iter()
        .map(|i| {
            (
                i.tick.0,
                pos_of(i.owner),
                i.victim.map(pos_of),
                i.damage,
                i.splash_victims,
            )
        })
        .collect();
    impacts.sort_unstable();
    let hp = ids.iter().map(|&i| w.hp_of(i).unwrap()).collect();
    (Outcome { hp, shots, impacts }, w.checksum())
}

/// Plays the cast in every spawn order, expects one outcome, and the same checksum when an order is
/// played again.
fn in_every_order(cast: &[Spawn], links: &[(usize, usize)], ticks: u32) -> Outcome {
    let mut seen: Option<Outcome> = None;
    for order in permutations(cast.len()) {
        let (outcome, checksum) = skirmish(cast, links, &order, ticks);
        assert_eq!(
            skirmish(cast, links, &order, ticks).1,
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
fn overlapping_splashes_add_up_in_every_spawn_order() {
    // Two throwers on opposite sides fire at one trooper on the same tick. Each blast reaches the
    // other thrower and the one that fired it (edge 2.5, a quarter of the damage each, 25 + 25 off 50),
    // the fragile unit (120 hit points: either blast alone leaves it standing, both together do not)
    // and the tank. Everything is summed per victim and taken off once.
    let cast = [
        Spawn::Unit(THROWER, at(13, 10), 0), // 0: 3 cells east of T
        Spawn::Unit(THROWER, at(7, 10), 1),  // 1: 3 cells west of T
        Spawn::Unit(TROOPER, at(10, 10), 1), // 2: T
        Spawn::Unit(FRAGILE, at(10, 12), 1), // 3: edge 1.5 from both blasts: 75 + 75
        Spawn::Unit(TANK, at(10, 8), 0),     // 4: edge 1.5 from both: 2 * 38
    ];
    let o = in_every_order(&cast, &[(0, 2), (1, 2)], 2);
    assert_eq!(o.hp, vec![0, 0, 800, 0, 1000 - 76]);
    assert_eq!(
        o.shots.len(),
        2,
        "both fired, and both were shot down on that tick"
    );
}

#[test]
fn a_mutual_kill_by_splash_alone_in_every_spawn_order() {
    // A and B (20 hit points each) shoot at X and Y. Each stands 2.5 cells from the centre of the other's
    // shot (edge 2, half strength, 50), so each is shot down by the splash of a shot at someone else.
    // Both fired on the tick they went down.
    let cast = [
        Spawn::Unit(GLASS, at(10, 10), 0),   // 0: A
        Spawn::Unit(GLASS, hc(28, 25), 1),   // 1: B at (14, 12.5)
        Spawn::Unit(TROOPER, at(14, 10), 1), // 2: X, A's victim
        Spawn::Unit(TROOPER, hc(20, 25), 0), // 3: Y, B's victim, at (10, 12.5)
        Spawn::Unit(TANK, hc(24, 22), 0),    // 4: Z at (12, 11), in both blasts
    ];
    let o = in_every_order(&cast, &[(0, 2), (1, 3)], 2);
    assert_eq!(&o.hp[..4], &[0, 0, 900, 900]);
    assert_eq!(o.shots.len(), 2);
    assert!(o.hp[4] < 1000, "the tank between them was hurt by both");
}

#[test]
fn a_splash_that_hits_the_shooter_itself_in_every_spawn_order() {
    let cast = |shooter: Kind| {
        [
            Spawn::Unit(shooter, at(14, 10), 0), // 0: 4 cells from X, edge 3.5: 42
            Spawn::Unit(TROOPER, at(10, 10), 1), // 1: X
            Spawn::Unit(TROOPER, at(14, 12), 0), // 2: a friend of the shooter
            Spawn::Unit(TANK, at(8, 8), 1),      // 3: an enemy
        ]
    };
    let o = in_every_order(&cast(NUKER), &[(0, 1)], 2);
    assert_eq!(o.hp[0], 0, "the shooter went down to its own blast");
    assert!(o.hp[2] < 1000, "and so did its friend");
    assert_eq!(o.hp[1], 900);
    assert!(o.hp[3] < 1000, "the enemy was in it too");
    let o = in_every_order(&cast(CAREFUL_NUKER), &[(0, 1)], 2);
    assert_eq!((o.hp[0], o.hp[2]), (30, 1000), "its own side is spared");
    assert_eq!(o.hp[1], 900);
    assert!(o.hp[3] < 1000, "the enemy was in it either way");
}

#[test]
fn bombs_that_land_together_add_up_in_every_spawn_order() {
    // Two bombers drop on two troopers whose blasts overlap on a fragile unit (75 + 75 = 150 off 120) in
    // the middle, on the corner between four buckets of the spatial index.
    let cast = [
        Spawn::Unit(BOMBER, at(22, 10), 0),  // 0
        Spawn::Unit(BOMBER, at(22, 14), 0),  // 1
        Spawn::Unit(TROOPER, at(16, 10), 1), // 2
        Spawn::Unit(TROOPER, at(16, 14), 1), // 3
        Spawn::Unit(FRAGILE, at(16, 12), 1), // 4: 2 cells from each, edge 1.5
    ];
    let o = in_every_order(&cast, &[(0, 2), (1, 3)], 16);
    assert_eq!(o.hp, vec![50, 50, 900, 900, 0]);
    assert_eq!(o.impacts.len(), 2);
    assert!(
        o.impacts.iter().all(|i| i.3 == 100 && i.4 == 1),
        "{:?}",
        o.impacts
    );
}

#[test]
fn bombs_that_land_together_count_a_victim_that_the_first_already_finishes_in_every_spawn_order() {
    // As above, with a 20 hit point unit in the middle: either bomb alone brings it to nothing, so a
    // splash that skipped a victim already at zero from an earlier hit of the same tick would count it
    // for one bomb only, and which one would depend on the spawn order. Both blasts are worked out
    // from the start of the tick: each counts it.
    let cast = [
        Spawn::Unit(BOMBER, at(22, 10), 0),  // 0
        Spawn::Unit(BOMBER, at(22, 14), 0),  // 1
        Spawn::Unit(TROOPER, at(16, 10), 1), // 2
        Spawn::Unit(TROOPER, at(16, 14), 1), // 3
        Spawn::Unit(GLASS, at(16, 12), 1),   // 4: 2 cells from each, edge 1.5: 75 off 20, twice
    ];
    let o = in_every_order(&cast, &[(0, 2), (1, 3)], 16);
    assert_eq!(o.hp, vec![50, 50, 900, 900, 0]);
    assert_eq!(
        o.impacts.iter().map(|i| (i.3, i.4)).collect::<Vec<_>>(),
        vec![(100, 1), (100, 1)],
        "{:?}",
        o.impacts
    );
}

#[test]
fn a_splash_rounds_the_armor_and_the_falloff_together_once() {
    // A grenade of 30 against a hard armor that takes 15 percent. A tank at edge 2 (half strength) takes
    // 30 * 15/100 * 1/2 = 2.25: 2. Rounding the armor first (4.5: 5) and then the half (2.5: 3) would
    // give 3. A tank at edge 1.5 (3/4): 3.375, 3; twice rounded 5 * 3/4 = 3.75, 4.
    let mut ws = weapons();
    ws[2].damage = 30;
    let mut cells = percent_cells();
    cells[6].2 = 15;
    let mut w = world_with(
        WeaponTable::new(ws, loadouts())
            .unwrap()
            .with_armor(armor_table_with(cells))
            .unwrap(),
    );
    let ids = spawn(&mut w, &blast_cast(Spawn::Unit(THROWER, hc(27, 15), 0)));
    go(&mut w, vec![attack(ids[0], ids[1])], 1);
    assert_eq!(
        hp(&w, ids[7]),
        1000 - 2,
        "B2: edge 2, 30 * 15/100 * 1/2 = 2.25"
    );
    assert_eq!(
        hp(&w, ids[3]),
        1000 - 3,
        "B6: edge 1.5, 30 * 15/100 * 3/4 = 3.375"
    );
    assert_eq!(hp(&w, ids[1]), 1000 - 30, "the direct victim is soft: 30");
}

// ---- what is refused and what the checksum covers -------------------------------------------------------

#[test]
fn a_weapon_with_a_damage_type_the_armor_table_lacks_is_refused() {
    let mut ws = weapons();
    ws[0].damage_type = 9;
    let t = WeaponTable::new(ws, loadouts()).unwrap();
    assert_eq!(
        t.with_armor(armor_table()).unwrap_err(),
        WeaponsError::UnknownDamageType {
            weapon: RIFLE,
            damage_type: 9
        }
    );
}

#[test]
fn a_world_refuses_an_armor_table_that_does_not_fit_its_units() {
    let make = |armored: Vec<(Kind, u16)>| {
        let armor = ArmorTable::new(
            vec![BULLET, BLAST],
            vec![NONE, SOFT, HARD, STRUCT],
            percent_cells(),
            armored,
        )
        .unwrap();
        let table = WeaponTable::new(weapons(), loadouts())
            .unwrap()
            .with_armor(armor)
            .unwrap();
        World::new(WorldConfig::new(5).with_map_size(MAP, MAP))
            .unwrap()
            .with_units(units())
            .unwrap()
            .with_weapons(table)
            .map(|_| ())
    };
    let all: Vec<(Kind, u16)> = every_kind().into_iter().map(|k| (k, armor_of(k))).collect();
    assert!(make(all.clone()).is_ok());
    let mut missing = all.clone();
    missing.retain(|&(k, _)| k != TANK);
    assert_eq!(
        make(missing).unwrap_err(),
        ConfigError::Weapons(WeaponsError::NoArmor(TANK))
    );
    let mut stranger = all;
    stranger.push((Kind(99), SOFT));
    assert_eq!(
        make(stranger).unwrap_err(),
        ConfigError::Weapons(WeaponsError::UnknownUnit(Kind(99)))
    );
}

#[test]
fn a_splash_that_does_not_fit_its_radius_is_refused() {
    let bad = |inner: Fx, radius: Fx| {
        let mut ws = weapons();
        ws[0].splash_radius = radius;
        ws[0].splash_inner = inner;
        WeaponTable::new(ws, loadouts()).unwrap_err()
    };
    let three = Fx::from_int(3);
    let e = WeaponsError::BadSplash(RIFLE);
    assert_eq!(bad(three, three), e, "not below the radius");
    assert_eq!(bad(Fx::from_int(4), three), e, "above the radius");
    assert_eq!(
        bad(Fx::ONE, Fx::ZERO),
        e,
        "an inner radius without a splash"
    );
    assert_eq!(bad(Fx::from_int(-1), three), e, "negative");
    assert_eq!(bad(Fx::ZERO, Fx::from_int(-1)), e, "a negative radius");
}

#[test]
fn an_armor_table_error_reads_in_words() {
    let e = ArmorTable::new(vec![BULLET], vec![SOFT], vec![], vec![]).unwrap_err();
    assert_eq!(
        e,
        ArmorError::MissingPercent {
            damage_type: BULLET,
            armor_type: SOFT
        }
    );
    assert_eq!(
        e.to_string(),
        "damage type 0 against armor type 1 has no percentage"
    );
}

#[test]
fn the_armor_table_and_the_splash_are_part_of_the_checksum() {
    let tick0 = |t: WeaponTable| {
        let mut w = world_with(t);
        w.step();
        w.checksum()
    };
    let with = tick0(table());
    let without = tick0(WeaponTable::new(weapons(), loadouts()).unwrap());
    assert_ne!(with, without, "an armor table, from the first tick");
    // One percentage.
    let mut cells = percent_cells();
    cells[2].2 = 11;
    let changed = tick0(
        WeaponTable::new(weapons(), loadouts())
            .unwrap()
            .with_armor(armor_table_with(cells))
            .unwrap(),
    );
    assert_ne!(with, changed, "one percentage");
    // The full-strength radius and the friendly-fire flag.
    let mut ws = weapons();
    ws[2].splash_inner = Fx::HALF;
    let inner = tick0(
        WeaponTable::new(ws, loadouts())
            .unwrap()
            .with_armor(armor_table())
            .unwrap(),
    );
    assert_ne!(with, inner, "the full-strength radius");
    let mut ws = weapons();
    ws[2].friendly_fire = false;
    let flag = tick0(
        WeaponTable::new(ws, loadouts())
            .unwrap()
            .with_armor(armor_table())
            .unwrap(),
    );
    assert_ne!(with, flag, "the friendly-fire flag");
}

#[test]
fn a_blast_run_repeats_and_a_clone_goes_on_the_same() {
    let run = || {
        let mut w = armed();
        let ids = spawn(&mut w, &blast_cast(Spawn::Unit(BOMBER, at(16, 10), 0)));
        let _ = go(&mut w, vec![attack(ids[0], ids[1])], 5);
        let mut copy = w.clone();
        let _ = go(&mut w, vec![], 12);
        let _ = go(&mut copy, vec![], 12);
        assert_eq!(w.checksum(), copy.checksum());
        w.checksum()
    };
    assert_eq!(run(), run());
}
