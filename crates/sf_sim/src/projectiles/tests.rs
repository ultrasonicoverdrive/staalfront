use super::*;
use crate::ids::Tick;
use crate::units::{UnitStats, UnitTable};
use crate::weapons::{Loadout, Mount, WeaponTable};
use crate::world::{World, WorldConfig};
use crate::{Command, Kind};

const TARGET: Kind = Kind(1);
const STRAIGHT: u16 = 1;
const ARC: u16 = 2;
const GUIDED: u16 = 3;
const FALLING: u16 = 4;

fn weapon(kind: u16, projectile: ProjectileKind, damage: u32) -> WeaponStats {
    WeaponStats {
        kind,
        range: Fx::from_int(30),
        min_range: Fx::ZERO,
        reload_ticks: 10,
        burst: 1,
        burst_delay_ticks: 0,
        damage,
        damage_type: 0,
        projectile,
        projectile_speed_per_tick: Fx::HALF,
        flight_height: match projectile {
            ProjectileKind::Arc => Fx::from_int(4),
            ProjectileKind::Falling => Fx::from_int(2),
            _ => Fx::ZERO,
        },
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
    vec![
        weapon(STRAIGHT, ProjectileKind::Straight, 10),
        weapon(ARC, ProjectileKind::Arc, 20),
        weapon(GUIDED, ProjectileKind::Guided, 5),
        weapon(FALLING, ProjectileKind::Falling, 40),
    ]
}

fn stats(kind: u16) -> WeaponStats {
    *weapons().iter().find(|w| w.kind == kind).unwrap()
}

fn world() -> World {
    let unit = UnitStats {
        kind: TARGET,
        hp: 1000,
        movement_class: None,
        speed_per_tick: Fx::ZERO,
        turn_per_tick: 0,
        size: Fx::ONE,
        footprint: None,
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    };
    let table = WeaponTable::new(weapons(), Vec::<Loadout>::new()).unwrap();
    World::new(WorldConfig::new(1).with_map_size(48, 48))
        .unwrap()
        .with_units(UnitTable::new(vec![unit]).unwrap())
        .unwrap()
        .with_weapons(table)
        .unwrap()
}

fn spawn(w: &mut World, x: i32, y: i32) -> (EntityId, usize) {
    let tick = w.tick();
    w.schedule(
        tick,
        PlayerId(0),
        Command::SpawnUnit {
            kind: TARGET,
            pos: Vec2::from_ints(x, y),
            owner: PlayerId(1),
        },
    )
    .unwrap();
    w.step();
    let i = w.slot_count() - 1;
    (EntityId::new(i as u32, w.entities.generation[i]), i)
}

fn shell(weapon: u16, from: Vec2, target: EntityId, aim: Vec2) -> Projectile {
    launch(
        &stats(weapon),
        EntityId::new(900 + u32::from(weapon), 0),
        PlayerId(0),
        from,
        Facing::PLUS_X,
        target,
        aim,
    )
}

// ---- the numbers ----------------------------------------------------------------------------

#[test]
fn the_ticks_to_cover_a_distance_round_up() {
    let t = |d: i64, v: i64| ticks_to_cover(Fx::from_raw(d), Fx::from_raw(v));
    assert_eq!(t(0, 5), 0);
    assert_eq!(t(1, 5), 1);
    assert_eq!(t(5, 5), 1);
    assert_eq!(t(6, 5), 2);
    assert_eq!(t(10, 5), 2);
    assert_eq!(t(11, 5), 3);
    // Never negative, and never a division by nothing.
    assert_eq!(t(-4, 5), 0);
    assert_eq!(t(4, 0), u32::MAX);
    assert_eq!(t(4, -3), u32::MAX);
    assert_eq!(
        ticks_to_cover(Fx::from_int(10), Fx::HALF),
        20,
        "ten cells at half a cell a tick"
    );
}

#[test]
fn the_fuse_of_each_kind() {
    // Straight, arc: the time to cover the ground plus the backstop tick. 10 cells at half a cell make 20 ticks.
    let span = Fx::from_int(10);
    assert_eq!(fuse_for(&stats(STRAIGHT), span), 21);
    assert_eq!(fuse_for(&stats(ARC), span), 21);
    // Falling: the drop, whatever the ground distance: 2 cells at half a cell, 4, and the tick.
    assert_eq!(fuse_for(&stats(FALLING), span), 5);
    assert_eq!(fuse_for(&stats(FALLING), Fx::ZERO), 5);
    // Guided: twice the time to fly the weapon's range: 30 cells at half a cell is 60 ticks, twice that makes 120 ticks
    assert_eq!(fuse_for(&stats(GUIDED), span), 120);
    assert_eq!(
        fuse_for(&stats(GUIDED), Fx::ZERO),
        GUIDED_FLIGHT_FACTOR * 60
    );
    // An instant weapon has no fuse.
    assert_eq!(fuse_for(&weapon(9, ProjectileKind::Instant, 1), span), 0);
}

#[test]
fn the_height_of_an_arc_is_a_parabola_that_peaks_in_the_middle() {
    let peak = Fx::from_int(3);
    let span = Fx::from_int(8);
    let h = |t: Fx| arc_height(peak, span, t);
    assert_eq!(h(Fx::ZERO), Fx::ZERO);
    assert_eq!(h(span), Fx::ZERO);
    assert_eq!(h(Fx::from_int(4)), peak);
    // A quarter of the way it is three quarters of the peak (4 t (s - t) / s^2 = 3/4).
    assert_eq!(h(Fx::from_int(2)), Fx::from_ratio(9, 4));
    assert_eq!(h(Fx::from_int(6)), Fx::from_ratio(9, 4));
    // Symmetric, rising, then falling, on every raw step of a short span.
    let short = Fx::from_raw(1000);
    let heights: Vec<Fx> = (0..=1000)
        .map(|t| arc_height(peak, short, Fx::from_raw(t)))
        .collect();
    for t in 0..=1000usize {
        assert_eq!(heights[t], heights[1000 - t], "at {t}");
    }
    assert!(heights[..=500].windows(2).all(|p| p[0] <= p[1]));
    assert!(heights[500..].windows(2).all(|p| p[0] >= p[1]));
    assert_eq!(heights[500], peak);
    // Outside the span it stays on the ground; a span of nothing is flat.
    assert_eq!(h(Fx::from_int(-3)), Fx::ZERO);
    assert_eq!(h(Fx::from_int(20)), Fx::ZERO);
    assert_eq!(arc_height(peak, Fx::ZERO, Fx::ZERO), Fx::ZERO);
    assert_eq!(arc_height(peak, Fx::from_raw(-5), Fx::ZERO), Fx::ZERO);
    // Tiny spans do not divide by nothing.
    assert_eq!(arc_height(peak, Fx::EPSILON, Fx::EPSILON), Fx::ZERO);
    // The largest legal peak over the longest legal span does not overflow.
    let top = arc_height(Fx::from_int(32), Fx::from_int(64), Fx::from_int(32));
    assert_eq!(top, Fx::from_int(32));
}

#[test]
fn a_step_that_passes_through_a_small_target_still_touches_it() {
    let a = Vec2::from_ints(0, 0);
    let b = Vec2::from_ints(10, 0);
    let c = Vec2::new(Fx::from_int(5), Fx::from_ratio(1, 2));
    let near = touches(a, b, c, Fx::from_ratio(3, 4)).unwrap();
    assert_eq!(near, Vec2::from_ints(5, 0));
    // Just out of reach, and exactly at the edge.
    assert!(touches(a, b, c, Fx::from_ratio(1, 2) - Fx::EPSILON).is_none());
    assert!(touches(a, b, c, Fx::from_ratio(1, 2)).is_some());
    // Behind the start and beyond the end of the step do not count; the nearest end does.
    let behind = Vec2::from_ints(-1, 0);
    assert!(touches(a, b, behind, Fx::HALF).is_none());
    assert_eq!(touches(a, b, behind, Fx::ONE), Some(a));
    let beyond = Vec2::from_ints(11, 0);
    assert_eq!(touches(a, b, beyond, Fx::ONE), Some(b));
    // A step of no length is a point.
    assert_eq!(touches(a, a, Vec2::from_ints(0, 1), Fx::ONE), Some(a));
    assert!(touches(a, a, Vec2::from_ints(0, 2), Fx::ONE).is_none());
}

// ---- launching ------------------------------------------------------------------------------

#[test]
fn a_launch_starts_each_kind_where_it_belongs() {
    let from = Vec2::from_ints(10, 10);
    let aim = Vec2::from_ints(10, 20);
    let target = EntityId::new(7, 0);
    let make = |weapon: u16, pointing: Facing| {
        launch(
            &stats(weapon),
            EntityId::new(1, 0),
            PlayerId(3),
            from,
            pointing,
            target,
            aim,
        )
    };
    // A straight shell and an arc start at the muzzle, aimed at the point: heading is the bearing.
    for kind in [STRAIGHT, ARC] {
        let p = make(kind, Facing::PLUS_X);
        assert_eq!((p.pos, p.prev_pos, p.height), (from, from, Fx::ZERO));
        assert_eq!(p.heading, Facing::PLUS_Y);
        assert_eq!(p.span, Fx::from_int(10));
        assert_eq!((p.aim, p.target), (aim, Target::Entity(target)));
        assert_eq!((p.owner, p.player), (EntityId::new(1, 0), PlayerId(3)));
        assert_eq!(p.fuse, 21);
    }
    // A rocket leaves the way the launcher points.
    let p = make(GUIDED, Facing::MINUS_X);
    assert_eq!(
        (p.pos, p.heading, p.height),
        (from, Facing::MINUS_X, Fx::ZERO)
    );
    // A falling shell starts above its aim point, at its height.
    let p = make(FALLING, Facing::PLUS_X);
    assert_eq!(
        (p.pos, p.prev_pos, p.height, p.prev_height),
        (aim, aim, Fx::from_int(2), Fx::from_int(2))
    );
    assert_eq!(p.span, Fx::ZERO);
    // On top of the target there is no bearing: the launcher's direction is kept.
    let p = launch(
        &stats(STRAIGHT),
        EntityId::new(1, 0),
        PlayerId(0),
        from,
        Facing::new(77),
        target,
        from,
    );
    assert_eq!((p.heading, p.span), (Facing::new(77), Fx::ZERO));
}

// ---- the store ------------------------------------------------------------------------------

fn filler() -> Projectile {
    shell(
        STRAIGHT,
        Vec2::from_ints(1, 1),
        EntityId::new(0, 0),
        Vec2::from_ints(2, 2),
    )
}

#[test]
fn the_store_reuses_the_last_freed_slot_and_bumps_the_generation() {
    let mut s = Projectiles::default();
    assert_eq!((s.len(), s.slot_count()), (0, 0));
    let a = s.insert(filler());
    let b = s.insert(filler());
    let c = s.insert(filler());
    assert_eq!(
        [a, b, c],
        [
            ProjectileId::new(0, 0),
            ProjectileId::new(1, 0),
            ProjectileId::new(2, 0)
        ]
    );
    s.remove(1);
    s.remove(0);
    assert_eq!((s.len(), s.slot_count()), (1, 3));
    assert!(s.get(a).is_none() && s.get(b).is_none() && s.get(c).is_some());
    // The last freed first: slot 0, then slot 1, then a new slot.
    let d = s.insert(filler());
    let e = s.insert(filler());
    let f = s.insert(filler());
    assert_eq!(
        [d, e, f],
        [
            ProjectileId::new(0, 1),
            ProjectileId::new(1, 1),
            ProjectileId::new(3, 0)
        ]
    );
    // The old ids are stale.
    assert!(s.get(a).is_none() && s.get(b).is_none());
    assert!(s.get(d).is_some());
    // Iteration is in slot order whatever the history.
    let order: Vec<u32> = s.iter().map(|(id, _)| id.index()).collect();
    assert_eq!(order, [0, 1, 2, 3]);
    // Removing a free slot, or one that never existed, changes nothing.
    let before = s.clone();
    s.remove(99);
    s.remove(1);
    s.remove(1);
    assert_eq!(s.len(), 3);
    assert_ne!(s, before);
    let mut t = before.clone();
    t.remove(99);
    assert_eq!(t, before);
    assert!(s.row_mut(1).is_none() && s.row_mut(99).is_none() && s.row_mut(0).is_some());
}

#[test]
fn a_slot_whose_generation_is_used_up_is_retired_and_its_ids_stay_stale() {
    let mut s = Projectiles::default();
    s.insert(filler());
    s.insert(filler());
    // Age slot 0 to one before the last generation, as a very long game would.
    s.generation[0] = u32::MAX - 1;
    let old = ProjectileId::new(0, u32::MAX - 1);
    assert!(s.get(old).is_some());
    s.remove(0);
    // The last generation is still usable: the slot is freed and reused once more.
    assert_eq!(s.free, [0]);
    let last = s.insert(filler());
    assert_eq!(last, ProjectileId::new(0, u32::MAX));
    assert!(s.get(old).is_none());
    // Freeing it cannot bump the generation any further: the slot is retired, not put on the free
    // list, and the id of its last projectile is stale for good.
    s.remove(0);
    assert!(s.free.is_empty());
    assert!(s.get(last).is_none());
    assert_eq!(s.len(), 1);
    // The next launches take new slots; none of them can alias the retired id.
    let next = s.insert(filler());
    let after = s.insert(filler());
    assert_eq!([next.index(), after.index()], [2, 3]);
    assert!(s.get(last).is_none() && s.get(old).is_none());
    assert_eq!(s.slot_count(), 4);
    // Removing the retired slot again does nothing.
    let before = s.clone();
    s.remove(0);
    assert_eq!(s, before);
    // A retired slot is never iterated or hashed as live.
    assert!(s.iter().all(|(id, _)| id.index() != 0));
}

#[test]
fn every_part_of_a_projectile_is_in_the_hash_and_putting_it_back_puts_the_hash_back() {
    let hash = |s: &Projectiles| {
        let mut h = Fnv1a64::new();
        s.write_hash(&mut h);
        h.finish()
    };
    let mut s = Projectiles::default();
    s.insert(filler());
    s.insert(filler());
    let rest = hash(&s);
    type Edit = Box<dyn Fn(&mut Projectile)>;
    let edits: Vec<(&str, Edit)> = vec![
        ("weapon", Box::new(|p| p.weapon += 1)),
        ("owner", Box::new(|p| p.owner = EntityId::new(5, 0))),
        (
            "owner generation",
            Box::new(|p| p.owner = EntityId::new(900, 1)),
        ),
        ("player", Box::new(|p| p.player = PlayerId(4))),
        ("target", Box::new(|p| p.target = Target::Point)),
        (
            "target id",
            Box::new(|p| p.target = Target::Entity(EntityId::new(0, 1))),
        ),
        ("aim", Box::new(|p| p.aim = Vec2::from_ints(9, 9))),
        ("pos", Box::new(|p| p.pos = Vec2::from_ints(9, 9))),
        ("height", Box::new(|p| p.height = Fx::EPSILON)),
        ("heading", Box::new(|p| p.heading = Facing::new(3))),
        ("span", Box::new(|p| p.span = Fx::EPSILON)),
        ("fuse", Box::new(|p| p.fuse += 1)),
    ];
    let mut seen = vec![rest];
    for (name, edit) in &edits {
        let original = *s.row_mut(1).unwrap();
        edit(s.row_mut(1).unwrap());
        let h = hash(&s);
        assert!(!seen.contains(&h), "{name}");
        seen.push(h);
        *s.row_mut(1).unwrap() = original;
        assert_eq!(hash(&s), rest, "{name}");
    }
    // The previous place and height are for interpolation and are not hashed.
    let p = s.row_mut(1).unwrap();
    p.prev_pos = Vec2::from_ints(30, 30);
    p.prev_height = Fx::from_int(3);
    assert_eq!(hash(&s), rest);
    // The free list and the slot count are.
    let mut with_hole = s.clone();
    with_hole.remove(0);
    assert_ne!(hash(&with_hole), rest);
    let mut a = Projectiles::default();
    let mut b = Projectiles::default();
    for s in [&mut a, &mut b] {
        s.insert(filler());
        s.insert(filler());
        s.insert(filler());
    }
    a.remove(0);
    a.remove(1);
    b.remove(1);
    b.remove(0);
    // Same live projectile, same holes, but the next launch goes to different slots.
    assert_ne!(hash(&a), hash(&b));
}

// ---- the order of the slots -------------------------------------------------------------------

/// A world with three targets and a set of shells aimed at them that go off on overlapping ticks, two
/// of them on the same victim on the same tick, a victim that either of them alone would kill.
fn crossfire() -> (World, Vec<Projectile>, [usize; 3]) {
    let mut w = world();
    let (t0, i0) = spawn(&mut w, 20, 20);
    w.entities.hp[i0] = 10;
    let (t1, i1) = spawn(&mut w, 30, 30);
    let (t2, i2) = spawn(&mut w, 10, 40);
    let at = |t: EntityId| w.get(t).unwrap().pos;
    let shells = vec![
        // Two straight shells from opposite sides arrive on the same tick at t0.
        shell(STRAIGHT, Vec2::from_ints(10, 20), t0, at(t0)),
        shell(STRAIGHT, Vec2::from_ints(30, 20), t0, at(t0)),
        // An arc and a falling shell come down on t1; the arc is slower.
        shell(ARC, Vec2::from_ints(25, 25), t1, at(t1)),
        shell(FALLING, Vec2::ZERO, t1, at(t1)),
        // A rocket chases t2, and a straight shell is aimed where t2 does not stand.
        shell(GUIDED, Vec2::from_ints(10, 30), t2, at(t2)),
        shell(
            STRAIGHT,
            Vec2::from_ints(10, 30),
            t2,
            Vec2::from_ints(14, 40),
        ),
    ];
    (w, shells, [i0, i1, i2])
}

/// The outcome of a crossfire in terms of the targets: their hit points and the impacts as (tick, weapon,
/// victim position, cause), sorted.
type Crossfire = (Vec<u32>, Vec<(u64, u16, Option<usize>, ImpactCause)>);

fn crossfire_outcome(order: &[usize], scramble: &[u32]) -> Crossfire {
    let (mut w, shells, slots) = crossfire();
    // Fill the store with fillers and free them in the given order, so that the free list (and so the
    // slot each shell gets) differs between runs.
    let ids: Vec<ProjectileId> = scramble
        .iter()
        .map(|_| w.arsenal.projectiles.insert(filler()))
        .collect();
    for &k in scramble {
        w.arsenal.projectiles.remove(ids[k as usize].index());
    }
    for &k in order {
        w.arsenal.projectiles.insert(shells[k]);
    }
    let mut impacts = Vec::new();
    for _ in 0..80 {
        w.step();
        for i in w.impacts() {
            let victim = i
                .victim
                .map(|v| slots.iter().position(|&s| s == v.index() as usize).unwrap());
            impacts.push((i.tick.0, i.weapon, victim, i.cause));
        }
    }
    impacts.sort_unstable_by_key(|i| (i.0, i.1, i.2, i.3 as u8));
    let hp = slots.iter().map(|&i| w.entities.hp[i]).collect();
    assert_eq!(w.projectile_count(), 0);
    (hp, impacts)
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
fn the_order_projectiles_are_created_in_and_the_slots_they_get_decide_nothing() {
    let reference = crossfire_outcome(&[0, 1, 2, 3, 4, 5], &[]);
    // The shells really do what the setup says.
    let (hp, impacts) = &reference;
    assert_eq!(impacts.len(), 6, "{impacts:?}");
    assert_eq!(hp, &[0, 1000 - 20 - 40, 1000 - 5]);
    // The two straight shells on t0 land on the same tick, and both count though one is enough: a
    // decision about who is hurt is made from the hit points at the start of the tick, not after the
    // first shell of the pass.
    let on_t0: Vec<_> = impacts.iter().filter(|i| i.2 == Some(0)).collect();
    assert_eq!(on_t0.len(), 2);
    assert_eq!(on_t0[0].0, on_t0[1].0);
    // The misaimed straight shell goes off at its aim point, a miss.
    assert_eq!(
        impacts.iter().filter(|i| i.2.is_none()).count(),
        1,
        "{impacts:?}"
    );
    // Every order of creation, with the free list in every state from tidy to scrambled.
    let scrambles: [&[u32]; 4] = [&[], &[0, 1, 2], &[2, 0, 1], &[3, 1, 0, 2]];
    let mut runs = 0;
    for order in permutations(6) {
        for scramble in scrambles {
            assert_eq!(
                crossfire_outcome(&order, scramble),
                reference,
                "created in order {order:?}, free list {scramble:?}"
            );
            runs += 1;
        }
    }
    assert_eq!(runs, 720 * 4);
}

#[test]
fn a_projectile_of_a_weapon_the_table_lacks_is_dropped() {
    let (mut w, shells, _) = crossfire();
    let mut odd = shells[0];
    odd.weapon = 77;
    w.arsenal.projectiles.insert(odd);
    assert_eq!(w.projectile_count(), 1);
    w.step();
    assert_eq!(w.projectile_count(), 0);
    assert!(w.impacts().is_empty());
}

// ---- the fuse and the kinds' details ----------------------------------------------------------

#[test]
fn a_straight_shell_aimed_at_the_ground_goes_off_there_after_the_distance_over_the_speed() {
    let mut w = world();
    let mut p = shell(
        STRAIGHT,
        Vec2::from_ints(5, 5),
        EntityId::new(0, 0),
        Vec2::from_ints(5, 15),
    );
    p.target = Target::Point;
    w.arsenal.projectiles.insert(p);
    let mut gone = None;
    for _ in 0..40 {
        w.step();
        if let Some(i) = w.impacts().first() {
            gone = Some(*i);
            break;
        }
    }
    let i = gone.expect("it arrives");
    assert_eq!(
        (i.cause, i.victim, i.at),
        (ImpactCause::Arrived, None, Vec2::from_ints(5, 15))
    );
    // Ten cells at half a cell a tick: the shell exists from the tick it was inserted (not stepped), so
    // twenty steps later it is there.
    assert_eq!(i.tick, Tick(19));
}

#[test]
fn a_shell_with_nothing_but_fuse_left_goes_off_where_it_is() {
    let mut w = world();
    let mut p = shell(
        STRAIGHT,
        Vec2::from_ints(5, 5),
        EntityId::new(0, 0),
        Vec2::from_ints(5, 25),
    );
    p.target = Target::Point;
    p.fuse = 4;
    w.arsenal.projectiles.insert(p);
    for _ in 0..3 {
        w.step();
        assert!(w.impacts().is_empty());
    }
    w.step();
    let i = w.impacts()[0];
    // Four steps of half a cell along +y.
    assert_eq!((i.cause, i.at), (ImpactCause::Fuse, Vec2::from_ints(5, 7)));
}

#[test]
fn a_guided_projectile_orbits_a_point_it_cannot_reach_and_the_fuse_ends_it() {
    // A rocket with a turn radius larger than the distance to a point beside it can circle the point
    // without touching it. It must not fly forever.
    let mut w = world();
    let mut p = shell(
        GUIDED,
        Vec2::from_ints(20, 20),
        EntityId::new(0, 0),
        Vec2::from_ints(20, 20) + Vec2::new(Fx::ZERO, Fx::from_ratio(1, 2)),
    );
    p.target = Target::Point;
    p.heading = Facing::PLUS_X;
    p.fuse = 200;
    let mut slow = stats(GUIDED);
    slow.guide_turn_per_tick = 1;
    let table = WeaponTable::new(vec![slow, stats(STRAIGHT)], Vec::<Loadout>::new()).unwrap();
    w.arsenal = crate::weapons::Arsenal::new(table, &w.units).unwrap();
    w.arsenal.projectiles.insert(p);
    let mut ended = None;
    for k in 0..300 {
        w.step();
        if let Some(i) = w.impacts().first() {
            ended = Some((k, *i));
            break;
        }
    }
    let (k, i) = ended.expect("it ends");
    // Two hundred steps of fuse, and then it goes off where it is: a miss.
    assert_eq!((k, i.cause, i.victim), (199, ImpactCause::Fuse, None));
    assert_eq!(w.projectile_count(), 0);
}
