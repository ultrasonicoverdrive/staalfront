use super::*;
use crate::orders::GuardTarget;
use crate::weapons::{Mount, ProjectileKind, WeaponStats, weapon_score};

fn weapon(damage: u32, burst: u8, delay: u32, reload: u32) -> WeaponStats {
    WeaponStats {
        kind: 1,
        range: Fx::from_int(5),
        min_range: Fx::ZERO,
        reload_ticks: reload,
        burst,
        burst_delay_ticks: delay,
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

#[test]
fn the_default_settings_are_the_constants_and_valid() {
    let d = TargetingConfig::DEFAULT;
    assert_eq!(d.scan_cooldown, DEFAULT_SCAN_COOLDOWN);
    assert_eq!(d.pursuit, Fx::from_int(i32::from(DEFAULT_PURSUIT_CELLS)));
    assert!(d.is_valid());
    assert_eq!(TargetingConfig::default(), d);
    assert_eq!(TargetingConfig::from_cells(10, 6), d);
}

#[test]
fn the_settings_are_valid_between_their_bounds_and_not_outside() {
    let ok = |cooldown, cells| TargetingConfig::from_cells(cooldown, cells).is_valid();
    assert!(ok(1, 0));
    assert!(ok(MAX_SCAN_COOLDOWN, MAX_PURSUIT_CELLS));
    assert!(!ok(0, 6));
    assert!(!ok(MAX_SCAN_COOLDOWN + 1, 6));
    assert!(!ok(10, MAX_PURSUIT_CELLS + 1));
    let reset = |t| TargetingConfig::DEFAULT.with_pursuit_reset(t).is_valid();
    assert!(reset(1) && reset(MAX_PURSUIT_RESET_TICKS));
    assert!(!reset(0) && !reset(MAX_PURSUIT_RESET_TICKS + 1));
    let negative = TargetingConfig {
        pursuit: Fx::from_int(-1),
        ..TargetingConfig::DEFAULT
    };
    assert!(!negative.is_valid());
    // A fraction of a cell is fine: the file gives whole cells, the world takes any in range.
    let part = TargetingConfig {
        pursuit: Fx::from_ratio(5, 2),
        ..TargetingConfig::DEFAULT
    };
    assert!(part.is_valid());
}

#[test]
fn the_settings_hash_differently_when_they_differ() {
    let hash = |c: TargetingConfig| {
        let mut h = Fnv1a64::new();
        c.write_hash(&mut h);
        h.finish()
    };
    let base = hash(TargetingConfig::DEFAULT);
    assert_eq!(base, hash(TargetingConfig::from_cells(10, 6)));
    assert_ne!(base, hash(TargetingConfig::from_cells(11, 6)));
    assert_ne!(base, hash(TargetingConfig::from_cells(10, 7)));
    assert_ne!(
        base,
        hash(TargetingConfig::DEFAULT.with_pursuit_reset(DEFAULT_PURSUIT_RESET_TICKS + 1))
    );
}

#[test]
fn forgetting_an_engagement_keeps_the_cooldown_and_nothing_else() {
    let mut e = Engage {
        resume: Some(Order::Guard(GuardTarget::Point(Vec2::from_ints(3, 4)))),
        chase: true,
        anchor: Vec2::from_ints(1, 2),
        held: true,
        rest: 3,
        aim: Vec2::from_ints(5, 6),
        reach: Fx::from_int(4),
        cool: 7,
    };
    assert_ne!(e, Engage::REST);
    e.forget();
    assert_eq!(
        e,
        Engage {
            cool: 7,
            ..Engage::REST
        }
    );
    e.cool = 0;
    assert_eq!(e, Engage::REST);
}

#[test]
fn the_score_follows_the_damage_the_burst_the_percent_and_the_reload() {
    let w = weapon(10, 1, 0, 20);
    // 10 damage, 100 percent, a shot every 20 ticks: 10 * 100 * 1000, divided by the 20 ticks.
    assert_eq!(weapon_score(&w, 100), 50_000);
    assert_eq!(
        weapon_score(&w, 0),
        0,
        "a weapon that does nothing scores nothing"
    );
    assert!(weapon_score(&w, 130) > weapon_score(&w, 100));
    assert!(weapon_score(&w, 100) > weapon_score(&w, 40));
    // More damage, a burst, or a shorter reload each score more; a longer reload less.
    assert!(weapon_score(&weapon(11, 1, 0, 20), 100) > weapon_score(&w, 100));
    assert!(weapon_score(&weapon(10, 2, 3, 20), 100) > weapon_score(&w, 100));
    assert!(weapon_score(&weapon(10, 1, 0, 19), 100) > weapon_score(&w, 100));
    assert!(weapon_score(&weapon(10, 1, 0, 21), 100) < weapon_score(&w, 100));
    // The largest the loader allows (damage 10^6, burst 16, 400 percent) fits with room to spare.
    let big = weapon(1_000_000, 16, 1, 600);
    assert_eq!(
        weapon_score(&big, 400),
        1_000_000_u64 * 16 * 400 * 1000 / 600
    );
}

#[test]
fn the_key_is_a_strict_total_order_that_tells_candidates_apart_by_place_before_slot() {
    let mut e = Entities::default();
    for (k, (x, y)) in [(10, 10), (12, 10), (10, 12), (10, 10)]
        .into_iter()
        .enumerate()
    {
        let i = e.push_free_slot();
        assert_eq!(i, k);
        e.pos[i] = Vec2::from_ints(x, y);
        e.generation[i] = 0;
    }
    let d2 = Fx::from_int(4);
    // Same score and distance: the smaller y, then the smaller x.
    let by_place = |j| key_of(&e, j, 0, 1000, d2);
    assert!(by_place(0) < by_place(2), "y 10 before y 12");
    assert!(by_place(0) < by_place(1), "x 10 before x 12, same y");
    assert!(by_place(1) < by_place(2), "y 10 before y 12");
    // Two bodies on one point fall back to the slot.
    assert!(by_place(0) < by_place(3));
    // A higher score beats a nearer one; in reach beats to be followed.
    assert!(key_of(&e, 2, 0, 2000, Fx::from_int(9)) < key_of(&e, 0, 0, 1000, Fx::from_int(1)));
    assert!(key_of(&e, 2, 0, 1, Fx::from_int(9)) < key_of(&e, 0, 1, 99_999, Fx::from_int(1)));
    // Nearer beats farther at an equal score.
    assert!(key_of(&e, 2, 0, 1000, Fx::from_int(1)) < key_of(&e, 0, 0, 1000, Fx::from_int(2)));
    // The same candidate has the same key, and no two keys of different slots are equal.
    for a in 0..4 {
        assert_eq!(by_place(a), by_place(a));
        for b in 0..4 {
            if a != b {
                assert_ne!(by_place(a), by_place(b), "{a} {b}");
            }
        }
    }
}

#[test]
fn only_an_idle_unit_a_hold_a_guard_and_an_attack_move_have_a_stance() {
    let mut e = Entities::default();
    let i = e.push_free_slot();
    e.alive[i] = true;
    let victim = EntityId::new(0, 0);
    let p = Vec2::from_ints(1, 1);
    let stance = |e: &Entities| stance_of(e, 0);
    // No order and idle behavior.
    assert_eq!(stance(&e), Some(Stance::Idle));
    // No order but a legacy walk: not a unit that stands about.
    e.behavior[i] = Behavior::Wander;
    assert_eq!(stance(&e), None);
    e.behavior[i] = Behavior::Ordered;
    for (order, expected) in [
        (Order::HoldPosition, Some(Stance::Hold)),
        (Order::Guard(GuardTarget::Point(p)), Some(Stance::Guard)),
        (
            Order::Guard(GuardTarget::Entity(victim)),
            Some(Stance::Guard),
        ),
        (Order::AttackMove(p), Some(Stance::Advance)),
        (Order::Move(p), None),
        (Order::Attack(victim), None),
        (Order::Pursue(victim), None),
        (Order::Scatter, None),
    ] {
        e.order[i] = order;
        assert_eq!(stance(&e), expected, "{order:?}");
    }
    assert!(Stance::Idle.follows() && Stance::Guard.follows());
    assert!(!Stance::Hold.follows() && !Stance::Advance.follows());
}

/// A structure kind for the defences hash test: 1 x 1, with or without a weapon (range 5).
fn tower_stats(kind: u16, armed: bool) -> crate::units::UnitStats {
    crate::units::UnitStats {
        kind: crate::ids::Kind(kind),
        hp: 100,
        movement_class: None,
        speed_per_tick: Fx::ZERO,
        turn_per_tick: 0,
        size: Fx::ZERO,
        footprint: Some(crate::units::Footprint {
            width: 1,
            height: 1,
        }),
        weapon_range: if armed { Fx::from_int(5) } else { Fx::ZERO },
        turret_per_tick: if armed { 9 } else { 0 },
    }
}

#[test]
fn the_defences_section_is_written_only_for_a_world_with_an_armed_structure() {
    use crate::weapons::{Loadout, WeaponTable};
    let units = UnitTable::new(vec![tower_stats(1, true), tower_stats(2, false)]).unwrap();
    let mut table_weapon = weapon(5, 1, 0, 10);
    table_weapon.mount = Mount::Turret;
    let table = WeaponTable::new(
        vec![table_weapon],
        vec![Loadout {
            unit: crate::ids::Kind(1),
            weapons: vec![1],
        }],
    )
    .unwrap();
    let arsenal = Arsenal::new(table, &units).unwrap();
    let bytes = |e: &Entities| {
        let mut h = Fnv1a64::new();
        write_defence_hash(&mut h, e, &units, &arsenal);
        h.finish()
    };
    let nothing = Fnv1a64::new().finish();
    let mut e = Entities::default();
    // A weaponless structure (kind 2, table position 1) and an empty slot: nothing is written.
    let bunker = e.push_free_slot();
    e.alive[bunker] = true;
    e.def[bunker] = 1;
    let free = e.push_free_slot();
    assert_eq!(free, 1);
    assert_eq!(bytes(&e), nothing, "no armed structure, no section");
    // An armed structure (kind 1, table position 0): the section is there, and its target is in it.
    let tower = e.push_free_slot();
    e.alive[tower] = true;
    e.def[tower] = 0;
    let idle = bytes(&e);
    assert_ne!(idle, nothing);
    e.turret_target[tower] = Some(EntityId::new(0, 0));
    let aiming = bytes(&e);
    assert_ne!(aiming, idle, "the target is state");
    e.turret_target[tower] = Some(EntityId::new(0, 1));
    assert_ne!(bytes(&e), aiming, "and so is the generation of the target");
    // A dead structure does not count.
    e.alive[tower] = false;
    assert_eq!(bytes(&e), nothing);
}

/// What `defence_check` says about one structure, with the world's pieces made by hand: a tower (kind 1,
/// table position 0, uses 20 power, armed) of player 0 on slot 0 and an enemy (slot 1, table position 1).
/// `change` edits the entities before the check, `dark` lists the players without power.
fn check_defence(
    change: impl Fn(&mut Entities),
    dark: &[crate::ids::PlayerId],
    power: i32,
) -> (bool, Option<EntityId>) {
    use crate::buildings::{BuildingFunction, BuildingStats, BuildingTable};
    use crate::weapons::{Loadout, WeaponTable};
    let units = UnitTable::new(vec![tower_stats(1, true), tower_stats(2, false)]).unwrap();
    let mut gun = weapon(5, 1, 0, 10);
    gun.mount = Mount::Turret;
    let table = WeaponTable::new(
        vec![gun],
        vec![Loadout {
            unit: crate::ids::Kind(1),
            weapons: vec![1],
        }],
    )
    .unwrap();
    let arsenal = Arsenal::new(table, &units).unwrap();
    let buildings = BuildingTable::new(vec![BuildingStats {
        kind: crate::ids::Kind(1),
        footprint: crate::units::Footprint {
            width: 1,
            height: 1,
        },
        function: BuildingFunction::Defence,
        exit_side: None,
        build_ticks: 10,
        cost: Some(100),
        power,
        factions: 1,
        requires: vec![],
        build_radius: 0,
    }])
    .unwrap();
    let mut e = Entities::default();
    for (i, (kind, def, owner, x)) in [(1u16, 0u32, 0u8, 10), (2, 1, 1, 12)]
        .into_iter()
        .enumerate()
    {
        let slot = e.push_free_slot();
        assert_eq!(slot, i);
        e.alive[slot] = true;
        e.kind[slot] = crate::ids::Kind(kind);
        e.def[slot] = def;
        e.owner[slot] = crate::ids::PlayerId(owner);
        e.pos[slot] = Vec2::from_ints(x, 10);
        e.hp[slot] = 100;
    }
    e.turret_target[0] = Some(EntityId::new(1, 0));
    change(&mut e);
    let cx = Context {
        units: &units,
        arsenal: &arsenal,
        config: TargetingConfig::DEFAULT,
        tick: Tick(0),
        bounds: (Vec2::ZERO, Vec2::from_ints(64, 64)),
        map: (64, 64),
        buildings: &buildings,
        unpowered: dark,
        gate: None,
    };
    let may_look = defence_check(&mut e, 0, &cx);
    (may_look, e.turret_target[0])
}

#[test]
fn a_structure_keeps_a_target_in_reach_and_loses_it_when_it_may_not_fire() {
    use crate::ids::PlayerId;
    let victim = Some(EntityId::new(1, 0));
    // Standing, powered, the enemy 2 cells away in a range of 5: it keeps the target and does not look.
    assert_eq!(check_defence(|_| {}, &[], -20), (false, victim));
    // Without a target it may look round.
    assert_eq!(
        check_defence(|e| e.turret_target[0] = None, &[], -20),
        (true, None)
    );
    // The enemy out of reach (20 cells), at zero hit points, or gone: the target goes, and it looks again.
    assert_eq!(
        check_defence(|e| e.pos[1] = Vec2::from_ints(30, 10), &[], -20),
        (true, None)
    );
    assert_eq!(check_defence(|e| e.hp[1] = 0, &[], -20), (true, None));
    assert_eq!(
        check_defence(|e| e.alive[1] = false, &[], -20),
        (true, None)
    );
    // The enemy is the owner's own: not a target.
    assert_eq!(
        check_defence(|e| e.owner[1] = PlayerId(0), &[], -20),
        (true, None)
    );
    // Each way a structure may not fire: it goes dark (its owner is on the list and it uses power), it is
    // under construction, or it has no hit points left. The target is cleared and it does not look.
    assert_eq!(check_defence(|_| {}, &[PlayerId(0)], -20), (false, None));
    assert_eq!(
        check_defence(|e| e.build_left[0] = 5, &[], -20),
        (false, None)
    );
    assert_eq!(check_defence(|e| e.hp[0] = 0, &[], -20), (false, None));
    // The power list is the owner's: another player being dark changes nothing for this one.
    assert_eq!(check_defence(|_| {}, &[PlayerId(1)], -20), (false, victim));
    // A defence that uses no power (power 0) or makes some is not switched off by the list.
    assert_eq!(check_defence(|_| {}, &[PlayerId(0)], 0), (false, victim));
    assert_eq!(check_defence(|_| {}, &[PlayerId(0)], 30), (false, victim));
}

#[test]
fn a_weaponless_structure_is_left_alone_by_the_update() {
    use crate::buildings::BuildingTable;
    use crate::weapons::{Loadout, WeaponTable};
    // Kind 1 is armed (position 0), kind 2 (position 1) is a bunker: no weapon, no scan, no cooldown.
    let units = UnitTable::new(vec![tower_stats(1, true), tower_stats(2, false)]).unwrap();
    let mut gun = weapon(5, 1, 0, 10);
    gun.mount = Mount::Turret;
    let table = WeaponTable::new(
        vec![gun],
        vec![Loadout {
            unit: crate::ids::Kind(1),
            weapons: vec![1],
        }],
    )
    .unwrap();
    let arsenal = Arsenal::new(table, &units).unwrap();
    let buildings = BuildingTable::empty();
    let mut e = Entities::default();
    for (i, (kind, def, owner)) in [(2u16, 1u32, 0u8), (1, 0, 1)].into_iter().enumerate() {
        let slot = e.push_free_slot();
        assert_eq!(slot, i);
        e.alive[slot] = true;
        e.kind[slot] = crate::ids::Kind(kind);
        e.def[slot] = def;
        e.owner[slot] = crate::ids::PlayerId(owner);
        e.pos[slot] = Vec2::from_ints(10 + 2 * slot as i32, 10);
        e.hp[slot] = 100;
    }
    let cx = Context {
        units: &units,
        arsenal: &arsenal,
        config: TargetingConfig::DEFAULT,
        tick: Tick(0),
        bounds: (Vec2::ZERO, Vec2::from_ints(64, 64)),
        map: (64, 64),
        buildings: &buildings,
        unpowered: &[],
        gate: None,
    };
    let mut t = Targeting::default();
    for _ in 0..30 {
        t.update(&mut e, &cx);
    }
    assert_eq!(
        e.eng[0],
        Engage::REST,
        "the bunker has no cooldown to count"
    );
    assert_eq!(e.turret_target[0], None);
    // The armed structure of the other player, 2 cells away and in range 5, does look and picks the bunker.
    assert_eq!(e.turret_target[1], Some(EntityId::new(0, 0)));
}
