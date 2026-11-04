//! What armed structures cost per tick (WP6.7, `docs/rules/defences.md`, "Cost"): 40 defence towers with
//! 200 enemy units standing in their reach, 2,000 ticks, timed in microseconds per tick. Run with
//! `cargo test --release -p sf_sim --test defence_cost -- --ignored --nocapture`. It uses only the public
//! API that exists on the base too (where the towers take no part), so the same file builds on both and
//! the two binaries can be run alternately. Not a pass or fail test: the numbers are printed.

use std::time::Instant;

use sf_math::{Fx, Vec2};
use sf_sim::{
    ArmorTable, BuildingFunction, BuildingStats, BuildingTable, Cell, Command, Footprint, Kind,
    Loadout, Mount, PlayerId, PowerRules, ProjectileKind, TargetingConfig, UnitStats, UnitTable,
    WeaponStats, WeaponTable, World, WorldConfig, per_tick,
};

const TOWER: Kind = Kind(22);
const PLANT: Kind = Kind(21);
const DUMMY: Kind = Kind(30);
const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn units() -> UnitTable {
    let structure = |kind: Kind, hp: u32, w: u8, turret: u8, range: i32| UnitStats {
        kind,
        hp,
        movement_class: None,
        speed_per_tick: Fx::ZERO,
        turn_per_tick: 0,
        size: Fx::ZERO,
        footprint: Some(Footprint {
            width: w,
            height: w,
        }),
        weapon_range: Fx::from_int(range),
        turret_per_tick: turret,
    };
    UnitTable::new(vec![
        structure(PLANT, 600, 2, 0, 0),
        structure(TOWER, 600, 1, 12, 6),
        UnitStats {
            kind: DUMMY,
            hp: 1_000_000,
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

fn world() -> World {
    let gun = WeaponStats {
        kind: 1,
        range: Fx::from_int(6),
        min_range: Fx::ZERO,
        reload_ticks: 10,
        burst: 1,
        burst_delay_ticks: 0,
        damage: 6,
        damage_type: 0,
        projectile: ProjectileKind::Instant,
        projectile_speed_per_tick: Fx::ZERO,
        flight_height: Fx::ZERO,
        guide_turn_per_tick: 0,
        splash_radius: Fx::ZERO,
        splash_inner: Fx::ZERO,
        friendly_fire: true,
        mount: Mount::Turret,
        fire_arc: 8,
    };
    let armor = ArmorTable::new(
        vec![0],
        vec![0, 1],
        vec![(0, 0, 0), (0, 1, 100)],
        vec![(PLANT, 0), (TOWER, 0), (DUMMY, 1)],
    )
    .unwrap();
    let weapons = WeaponTable::new(
        vec![gun],
        vec![Loadout {
            unit: TOWER,
            weapons: vec![1],
        }],
    )
    .unwrap()
    .with_armor(armor)
    .unwrap();
    let entry = |kind: Kind, size: u8, function: BuildingFunction, power: i32| BuildingStats {
        kind,
        footprint: Footprint {
            width: size,
            height: size,
        },
        function,
        exit_side: None,
        build_ticks: 10,
        cost: Some(100),
        power,
        factions: 1,
        requires: vec![],
        build_radius: 0,
    };
    World::new(WorldConfig::new(3).with_map_size(128, 128))
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_buildings(
            BuildingTable::new(vec![
                entry(PLANT, 2, BuildingFunction::PowerPlant, 1000),
                entry(TOWER, 1, BuildingFunction::Defence, -20),
            ])
            .unwrap(),
        )
        .unwrap()
        .with_weapons(weapons)
        .unwrap()
        .with_targeting(TargetingConfig::DEFAULT)
        .unwrap()
        .with_power(PowerRules {
            shortage_floor_permille: 250,
            defence_cutoff_permille: 1000,
        })
        .unwrap()
}

/// 40 towers in two rows of 20 (every third cell), 200 dummies in the band between them.
fn scene() -> World {
    let mut w = world();
    let tick = w.tick();
    w.schedule(
        tick,
        P0,
        Command::PlaceStructure {
            kind: PLANT,
            cell: Cell::new(2, 2),
            owner: P0,
        },
    )
    .unwrap();
    for k in 0..20 {
        for row in [40u32, 47] {
            w.schedule(
                tick,
                P0,
                Command::PlaceStructure {
                    kind: TOWER,
                    cell: Cell::new(10 + 3 * k, row),
                    owner: P0,
                },
            )
            .unwrap();
        }
    }
    for i in 0..200 {
        let (x, y) = (10 + i % 50, 42 + i / 50);
        w.schedule(
            tick,
            P1,
            Command::SpawnUnit {
                kind: DUMMY,
                pos: Vec2::from_ints(x, y),
                owner: P1,
            },
        )
        .unwrap();
    }
    w.step();
    w
}

#[test]
#[ignore = "timing run"]
fn cost_of_forty_armed_structures_with_two_hundred_units_in_reach() {
    let mut runs = Vec::new();
    for _ in 0..5 {
        let mut w = scene();
        let start = Instant::now();
        let mut shots = 0usize;
        for _ in 0..2000 {
            w.step();
            shots += w.shots().len();
        }
        let micros = start.elapsed().as_secs_f64() * 1e6 / 2000.0;
        runs.push(micros);
        println!(
            "run: {micros:.2} us/tick, {shots} shots, checksum {:016x}",
            w.checksum()
        );
    }
    runs.sort_by(f64::total_cmp);
    println!("MEDIAN {:.2} us/tick", runs[runs.len() / 2]);
}

/// The same world with no structure at all and 5,000 idle units: what the defence code costs a world that
/// has none (the hook in the checksum and in the target update).
#[test]
#[ignore = "timing run"]
fn cost_of_the_hooks_in_a_world_with_no_structure() {
    let mut runs = Vec::new();
    for _ in 0..5 {
        let mut w = world();
        let tick = w.tick();
        for i in 0..5000 {
            w.schedule(
                tick,
                P1,
                Command::SpawnUnit {
                    kind: DUMMY,
                    pos: Vec2::from_ints(2 + i % 100, 2 + i / 100 * 2),
                    owner: P1,
                },
            )
            .unwrap();
        }
        w.step();
        let start = Instant::now();
        for _ in 0..500 {
            w.step();
        }
        let micros = start.elapsed().as_secs_f64() * 1e6 / 500.0;
        runs.push(micros);
        println!("run: {micros:.2} us/tick, checksum {:016x}", w.checksum());
    }
    runs.sort_by(f64::total_cmp);
    println!("MEDIAN {:.2} us/tick", runs[runs.len() / 2]);
}
