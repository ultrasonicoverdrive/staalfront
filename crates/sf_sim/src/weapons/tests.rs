use super::*;
use crate::ids::PlayerId;
use crate::units::UnitStats;
use crate::world::{World, WorldConfig};
use sf_math::Vec2;

const GUNNER: Kind = Kind(1);
const GUN: u16 = 3;

fn stats() -> WeaponStats {
    WeaponStats {
        kind: GUN,
        range: Fx::from_int(4),
        min_range: Fx::ZERO,
        reload_ticks: 20,
        burst: 3,
        burst_delay_ticks: 4,
        damage: 5,
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

fn world() -> World {
    let unit = UnitStats {
        kind: GUNNER,
        hp: 50,
        movement_class: Some(0),
        speed_per_tick: Fx::from_ratio(1, 10),
        turn_per_tick: 16,
        size: Fx::ONE,
        footprint: None,
        weapon_range: Fx::from_int(4),
        turret_per_tick: 0,
    };
    let table = WeaponTable::new(
        vec![stats()],
        vec![Loadout {
            unit: GUNNER,
            weapons: vec![GUN],
        }],
    )
    .unwrap();
    World::new(WorldConfig::new(1).with_map_size(32, 32))
        .unwrap()
        .with_units(UnitTable::new(vec![unit]).unwrap())
        .unwrap()
        .with_weapons(table)
        .unwrap()
}

fn spawn(w: &mut World) -> usize {
    let tick = w.tick();
    w.schedule(
        tick,
        PlayerId(0),
        crate::Command::SpawnUnit {
            kind: GUNNER,
            pos: Vec2::from_ints(5, 5),
            owner: PlayerId(0),
        },
    )
    .unwrap();
    w.step();
    w.slot_count() - 1
}

#[test]
fn the_tail_is_what_is_left_of_the_reload_after_the_burst() {
    let w = stats();
    // Three shots, two gaps of four: shot one at tick 0, shot three at tick 8, the next burst at tick 20
    assert_eq!(w.tail_ticks(), 12);
    assert_eq!(2 * w.burst_delay_ticks + w.tail_ticks(), w.reload_ticks);
    let single = WeaponStats {
        burst: 1,
        burst_delay_ticks: 0,
        ..stats()
    };
    assert_eq!(single.tail_ticks(), single.reload_ticks);
}

#[test]
fn rest_is_ready_with_no_burst() {
    assert!(at_rest(&REST));
    let mut s = REST;
    s[2].wait = 1;
    assert!(!at_rest(&s));
    let mut s = REST;
    s[0].burst_left = 1;
    assert!(!at_rest(&s));
    assert_eq!(SlotState::default(), REST[0]);
}

#[test]
fn every_part_of_the_state_is_in_the_checksum_and_putting_it_back_puts_the_checksum_back() {
    let mut w = world();
    let i = spawn(&mut w);
    let rest = w.checksum();
    // At rest the section holds the table and nobody: the checksum differs from the world that
    // has no table, from the first tick.
    let mut unarmed = World::new(WorldConfig::new(1).with_map_size(32, 32))
        .unwrap()
        .with_units(w.unit_table().clone())
        .unwrap();
    spawn(&mut unarmed);
    assert_ne!(rest, unarmed.checksum());
    // Each field of each slot moves it, and putting it back puts the checksum back.
    let mut seen = vec![rest];
    for (slot, wait, burst_left) in [
        (0, 1, 0),
        (0, 2, 0),
        (0, 0, 1),
        (0, 0, 2),
        (1, 1, 0),
        (3, 0, 1),
    ] {
        w.entities.weapon[i][slot].wait = wait;
        w.entities.weapon[i][slot].burst_left = burst_left;
        let c = w.checksum();
        assert!(
            !seen.contains(&c),
            "slot {slot} wait {wait} burst {burst_left}"
        );
        seen.push(c);
        w.entities.weapon[i] = REST;
        assert_eq!(w.checksum(), rest);
    }
    // The table is in it too.
    w.entities.weapon[i][0].wait = 1;
    let with_table = w.checksum();
    let mut other = stats();
    other.damage += 1;
    let table = WeaponTable::new(
        vec![other],
        vec![Loadout {
            unit: GUNNER,
            weapons: vec![GUN],
        }],
    )
    .unwrap();
    w.arsenal = Arsenal::new(table, &w.units).unwrap();
    assert_ne!(w.checksum(), with_table);
}

#[test]
fn a_new_entity_in_a_used_slot_starts_ready() {
    let mut w = world();
    let i = spawn(&mut w);
    w.entities.weapon[i][0].wait = 7;
    w.entities.weapon[i][0].burst_left = 2;
    let id = EntityId::new(i as u32, w.entities.generation[i]);
    let tick = w.tick();
    w.schedule(tick, PlayerId(0), crate::Command::Despawn { id })
        .unwrap();
    w.step();
    assert!(at_rest(&w.entities.weapon[i]));
    spawn(&mut w);
    assert!(at_rest(&w.entities.weapon[i]));
}
