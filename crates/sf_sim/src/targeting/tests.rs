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
}

#[test]
fn forgetting_an_engagement_keeps_the_cooldown_and_nothing_else() {
    let mut e = Engage {
        resume: Some(Order::Guard(GuardTarget::Point(Vec2::from_ints(3, 4)))),
        chase: true,
        anchor: Vec2::from_ints(1, 2),
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
