//! Turning the hull and the turret (WP4.5), through the public API: the three turn
//! modes, an arc that always arrives, the turret that follows a target and returns to
//! the hull, and what the checksum covers.
//!
//! A unit is spawned with a heading of its own, so the tests work relative to the heading
//! a unit has when the order comes (`at_angle`), not to a compass direction.

use sf_math::{Facing, Fx, Vec2};
use sf_sim::{
    ALIGN_TOLERANCE, ARRIVE_TOLERANCE, ClassCosts, Command, EntityId, Kind, MovementTable,
    MovementTableError, Order, PlayerId, TurnMode, Turning, TurningError, UnitStats, UnitTable,
    World, WorldConfig, per_tick,
};

const MAP: u32 = 128;

// Movement classes: 0 pivots (tracked), 1 has to move to turn (wheeled), 2 turns at once (foot).
const PIVOT: u16 = 0;
const ARC: u16 = 1;
const INSTANT: u16 = 2;

// Kinds: a tank with a turret, the same without one, a wheeled truck, a rifleman.
const TANK: Kind = Kind(1);
const PLAIN_TANK: Kind = Kind(2);
const TRUCK: Kind = Kind(3);
const RIFLEMAN: Kind = Kind(4);

fn table() -> UnitTable {
    let mobile = |kind: Kind, class: u16, speed_tenths: i64, turn: u8, turret: u8| UnitStats {
        kind,
        hp: 100,
        movement_class: Some(class),
        speed_per_tick: per_tick(Fx::from_ratio(speed_tenths, 10)),
        turn_per_tick: turn,
        size: Fx::from_ratio(12, 10),
        footprint: None,
        weapon_range: Fx::from_int(5),
        turret_per_tick: turret,
    };
    UnitTable::new(vec![
        mobile(TANK, PIVOT, 20, 10, 12),
        mobile(PLAIN_TANK, PIVOT, 20, 10, 0),
        mobile(TRUCK, ARC, 25, 8, 0),
        mobile(RIFLEMAN, INSTANT, 7, 16, 0),
    ])
    .unwrap()
}

fn movement() -> MovementTable {
    let classes = [PIVOT, ARC, INSTANT]
        .into_iter()
        .map(|class| ClassCosts {
            class,
            costs: Vec::new(),
        })
        .collect();
    MovementTable::new(classes).unwrap()
}

/// The arc of a truck: a radius of two cells, the pivot share a quarter.
fn truck_turning() -> Turning {
    Turning::arc(Fx::from_int(2), 16, 250)
}

fn profiles() -> Vec<(u16, Turning)> {
    vec![
        (PIVOT, Turning::pivot(16)),
        (ARC, truck_turning()),
        (INSTANT, Turning::instant()),
    ]
}

fn world_with(movement: MovementTable) -> World {
    World::new(WorldConfig::new(5).with_map_size(MAP, MAP))
        .unwrap()
        .with_units(table())
        .unwrap()
        .with_movement(movement)
        .unwrap()
}

fn world() -> World {
    world_with(movement().with_turning(profiles()).unwrap())
}

fn pt(x: i32, y: i32) -> Vec2 {
    Vec2::from_ints(x, y)
}

fn tick_with(w: &mut World, commands: Vec<Command>) {
    let tick = w.tick();
    for c in commands {
        w.schedule(tick, PlayerId(0), c).unwrap();
    }
    w.step();
}

fn spawn(w: &mut World, kind: Kind, pos: Vec2) -> EntityId {
    let id = EntityId::new(w.slot_count() as u32, 0);
    tick_with(
        w,
        vec![Command::SpawnUnit {
            kind,
            pos,
            owner: PlayerId(0),
        }],
    );
    id
}

fn go(w: &mut World, id: EntityId, to: Vec2) {
    tick_with(
        w,
        vec![Command::Order {
            ids: vec![id],
            order: Order::Move(to),
            queued: false,
        }],
    );
}

fn run_until_idle(w: &mut World, id: EntityId, limit: u32) -> u32 {
    for n in 0..limit {
        if w.order_of(id) == Some(Order::Idle) {
            return n;
        }
        w.step();
    }
    panic!("{id:?} did not arrive in {limit} ticks");
}

fn facing(w: &World, id: EntityId) -> u8 {
    w.get(id).unwrap().facing.raw()
}

fn pos(w: &World, id: EntityId) -> Vec2 {
    w.get(id).unwrap().pos
}

/// A unit that has driven to `at` and stands still (with whatever heading that left it).
fn settled(w: &mut World, kind: Kind, at: Vec2) -> EntityId {
    let id = spawn(w, kind, at - pt(20, 0));
    go(w, id, at);
    run_until_idle(w, id, 1500);
    for _ in 0..40 {
        w.step();
    }
    id
}

/// The point `tenths / 10` cells away from the unit, `turn` facing steps to the left of
/// the way it faces (positive steps turn toward +y).
fn at_angle(w: &World, id: EntityId, turn: u8, tenths: i64) -> Vec2 {
    let heading = Facing::new(facing(w, id).wrapping_add(turn));
    pos(w, id) + heading.to_vec2().scale(Fx::from_ratio(tenths, 10))
}

// --- the table ---

#[test]
fn a_table_without_profiles_or_with_the_old_one_hashes_as_before() {
    let plain = movement();
    let same = movement()
        .with_turning(vec![(PIVOT, Turning::LEGACY), (ARC, Turning::LEGACY)])
        .unwrap();
    assert_eq!(plain, same);
    assert_eq!(plain.digest(), same.digest());
    assert_eq!(plain.turning(ARC), Turning::LEGACY);
    // A class the table does not have turns as the walk always did.
    assert_eq!(plain.turning(99), Turning::LEGACY);
    // Any other profile is in the digest, each part of it.
    let base = movement().with_turning(profiles()).unwrap().digest();
    assert_ne!(base, plain.digest());
    let with = |t: Turning| movement().with_turning(vec![(ARC, t)]).unwrap().digest();
    let a = with(truck_turning());
    assert_ne!(a, with(Turning::arc(Fx::from_int(3), 16, 250)));
    assert_ne!(a, with(Turning::arc(Fx::from_int(2), 8, 250)));
    assert_ne!(a, with(Turning::arc(Fx::from_int(2), 16, 500)));
    assert_ne!(a, with(Turning::instant()));
    assert_ne!(with(Turning::pivot(8)), with(Turning::pivot(16)));
}

#[test]
fn bad_profiles_are_refused_with_their_reason() {
    let with = |p: Vec<(u16, Turning)>| movement().with_turning(p).unwrap_err();
    assert_eq!(
        with(vec![(7, Turning::instant())]),
        MovementTableError::UnknownTurningClass(7)
    );
    assert_eq!(
        with(vec![(ARC, truck_turning()), (ARC, truck_turning())]),
        MovementTableError::DuplicateTurning(ARC)
    );
    assert_eq!(
        with(vec![(ARC, Turning::arc(Fx::from_ratio(1, 8), 16, 250))]),
        MovementTableError::BadTurning {
            class: ARC,
            problem: TurningError::Radius
        }
    );
    assert_eq!(
        with(vec![(ARC, Turning::arc(Fx::ONE, 16, 0))]),
        MovementTableError::BadTurning {
            class: ARC,
            problem: TurningError::Pivot
        }
    );
    assert_eq!(
        with(vec![(PIVOT, Turning::pivot(65))]),
        MovementTableError::BadTurning {
            class: PIVOT,
            problem: TurningError::Tolerance
        }
    );
    assert!(!with(vec![(7, Turning::instant())]).to_string().is_empty());
}

#[test]
fn the_unit_table_hashes_the_turret_apart_from_the_rest() {
    let a = table();
    let mut stats: Vec<UnitStats> = a.iter().copied().collect();
    stats[0].turret_per_tick = 9;
    let b = UnitTable::new(stats).unwrap();
    // The main digest is what the pinned `units_spawn` scenario fixes: it does not move.
    assert_eq!(a.digest(), b.digest());
    assert_ne!(a.turret_digest(), b.turret_digest());
    assert_eq!(UnitTable::empty().turret_digest(), 0);
}

// --- the hull ---

#[test]
fn a_tracked_unit_turns_in_place_the_shortest_way_with_the_tie_at_the_decreasing_side() {
    let mut w = world();
    let id = settled(&mut w, PLAIN_TANK, pt(40, 40));
    let (start, f0) = (pos(&w, id), facing(&w, id));
    // Straight behind: the tie. Ten steps a tick, the decreasing way, in place; the
    // first turn is in the tick of the order.
    let behind = at_angle(&w, id, 128, 100);
    go(&mut w, id, behind);
    for k in 1..=11_u8 {
        assert_eq!(facing(&w, id), f0.wrapping_sub(10 * k), "turn {k}");
        assert_eq!(pos(&w, id), start, "it has not moved");
        w.step();
    }
    // A step more than half a turn round (the decreasing way is the short one), and a
    // step less (the increasing way is).
    for (turn, expect) in [(136_u8, -10_i16), (120, 10)] {
        let mut w = world();
        let id = settled(&mut w, PLAIN_TANK, pt(40, 40));
        let f0 = i16::from(facing(&w, id));
        let target = at_angle(&w, id, turn, 100);
        go(&mut w, id, target);
        assert_eq!(
            i16::from(facing(&w, id)),
            (f0 + expect).rem_euclid(256),
            "turn {turn}"
        );
    }
}

#[test]
fn a_unit_that_turns_at_once_faces_its_goal_on_the_first_tick() {
    let mut w = world();
    let id = settled(&mut w, RIFLEMAN, pt(40, 40));
    let (start, f0) = (pos(&w, id), facing(&w, id));
    let behind = at_angle(&w, id, 128, 100);
    go(&mut w, id, behind);
    assert_eq!(facing(&w, id), f0.wrapping_add(128));
    // And it steps off in that same tick.
    assert_ne!(pos(&w, id), start);
}

#[test]
fn a_unit_that_has_to_move_to_turn_drives_off_and_never_stands_to_turn() {
    let mut w = world();
    let id = settled(&mut w, TRUCK, pt(60, 60));
    // Behind it and to the side, far enough off for an arc.
    let goal = at_angle(&w, id, 140, 120);
    go(&mut w, id, goal);
    let mut last = pos(&w, id);
    let mut most = 0;
    for _ in 0..30 {
        let before = facing(&w, id);
        w.step();
        let now = w.get(id).unwrap();
        assert_ne!(now.pos, last, "it keeps moving while it turns");
        last = now.pos;
        most = most.max(Facing::new(before).abs_diff(now.facing));
    }
    // Two cells of radius at 0.125 cells a tick: a bit over 2.5 steps a tick, well below
    // its turn rate of 8.
    assert!(
        (2..=3).contains(&most),
        "turned at most {most} steps a tick"
    );
    run_until_idle(&mut w, id, 600);
    assert!(pos(&w, id).distance(goal) <= ARRIVE_TOLERANCE);
}

#[test]
fn a_target_inside_the_turning_circle_is_reached_by_a_turn_on_the_spot() {
    let mut w = world();
    let id = settled(&mut w, TRUCK, pt(60, 60));
    // A cell to the left, abreast of the truck: inside the circle of radius two. The
    // truck turns on the spot, at a quarter of its turn rate (2 steps a tick), then
    // drives the cell.
    let goal = at_angle(&w, id, 64, 10);
    go(&mut w, id, goal);
    let mut still = 0;
    for _ in 0..100 {
        let before = pos(&w, id);
        w.step();
        if pos(&w, id) == before {
            still += 1;
        }
        if w.order_of(id) == Some(Order::Idle) {
            break;
        }
    }
    assert_eq!(w.order_of(id), Some(Order::Idle));
    assert!(still >= 20, "it stood {still} ticks turning");
}

#[test]
fn an_arc_always_arrives_wherever_the_goal_lies() {
    // Goals on rings round a truck, near and far, in 16 directions each: none is
    // circled for ever.
    for radius_tenths in [3, 8, 15, 30, 60, 120] {
        for k in 0..16u8 {
            let mut w = world();
            let id = settled(&mut w, TRUCK, pt(60, 60));
            let goal = at_angle(&w, id, k * 16, radius_tenths);
            go(&mut w, id, goal);
            run_until_idle(&mut w, id, 700);
            assert!(
                pos(&w, id).distance(goal) <= ARRIVE_TOLERANCE,
                "radius {radius_tenths} direction {k}"
            );
        }
    }
}

// --- the turret ---

fn turret(w: &World, id: EntityId) -> u8 {
    w.turret_of(id).unwrap().raw()
}

/// A tank with a turret standing still, and a rifleman exactly `turn` steps round from the
/// way its hull faces, ten cells off.
fn tank_and_victim(w: &mut World, turn: u8) -> (EntityId, EntityId) {
    let tank = settled(w, TANK, pt(40, 40));
    let at = at_angle(w, tank, turn, 100);
    let victim = spawn(w, RIFLEMAN, at);
    (tank, victim)
}

#[test]
fn only_a_kind_with_a_turret_has_one() {
    let mut w = world();
    let tank = spawn(&mut w, TANK, pt(20, 20));
    let plain = spawn(&mut w, PLAIN_TANK, pt(30, 20));
    assert!(w.get(tank).unwrap().turret.is_some());
    assert!(w.get(plain).unwrap().turret.is_none());
    assert_eq!(w.turret_of(plain), None);
    assert_eq!(w.turret_target_of(plain), None);
    // A new unit's turret points along its hull.
    assert_eq!(turret(&w, tank), facing(&w, tank));
    // An aim command skips what has no turret, and counts as ignored then.
    let tick = w.tick();
    w.schedule(
        tick,
        PlayerId(0),
        Command::AimTurret {
            ids: vec![plain],
            target: Some(tank),
        },
    )
    .unwrap();
    assert_eq!(w.step().commands_ignored, 1);
}

#[test]
fn the_turret_follows_the_target_at_its_own_rate_while_the_hull_stays() {
    let mut w = world();
    // A quarter turn round the decreasing way: 192 steps on from the hull.
    let (tank, victim) = tank_and_victim(&mut w, 192);
    let hull = facing(&w, tank);
    assert_eq!(turret(&w, tank), hull);
    tick_with(
        &mut w,
        vec![Command::AimTurret {
            ids: vec![tank],
            target: Some(victim),
        }],
    );
    assert_eq!(w.turret_target_of(tank), Some(victim));
    // Twelve steps a tick, the short way (the decreasing one, 64 steps), then on it.
    let mut seen = vec![turret(&w, tank)];
    for _ in 0..6 {
        w.step();
        seen.push(turret(&w, tank));
    }
    let want: Vec<u8> = [12, 24, 36, 48, 60, 64, 64]
        .iter()
        .map(|d| hull.wrapping_sub(*d))
        .collect();
    assert_eq!(seen, want);
    assert_eq!(facing(&w, tank), hull, "the hull stays");
    // The victim goes off to the far side of the tank: the turret follows it round.
    let away = at_angle(&w, tank, 0, 200);
    tick_with(
        &mut w,
        vec![Command::Order {
            ids: vec![victim],
            order: Order::Move(away),
            queued: false,
        }],
    );
    for _ in 0..800 {
        w.step();
        let aim = Facing::from_vec2(pos(&w, victim) - pos(&w, tank))
            .unwrap()
            .raw();
        assert!(Facing::new(aim).abs_diff(Facing::new(turret(&w, tank))) <= 24);
    }
    assert!(Facing::new(turret(&w, tank)).abs_diff(Facing::new(hull)) <= 2);
}

#[test]
fn stop_a_gone_target_and_none_send_the_turret_back_to_the_hull() {
    let mut w = world();
    let (tank, victim) = tank_and_victim(&mut w, 192);
    let hull = facing(&w, tank);
    let north = hull.wrapping_sub(64);
    let aim = |w: &mut World, target| {
        tick_with(
            w,
            vec![Command::AimTurret {
                ids: vec![tank],
                target,
            }],
        );
        for _ in 0..10 {
            w.step();
        }
    };
    aim(&mut w, Some(victim));
    assert_eq!(turret(&w, tank), north);
    // None: back to the hull, 64 steps at 12 a tick.
    aim(&mut w, None);
    assert_eq!(turret(&w, tank), hull);
    assert_eq!(w.turret_target_of(tank), None);
    // Stop lets go.
    aim(&mut w, Some(victim));
    assert_eq!(turret(&w, tank), north);
    tick_with(&mut w, vec![Command::Stop { ids: vec![tank] }]);
    assert_eq!(w.turret_target_of(tank), None);
    for _ in 0..10 {
        w.step();
    }
    assert_eq!(turret(&w, tank), hull);
    // A target that is gone lets go by itself.
    aim(&mut w, Some(victim));
    assert_eq!(turret(&w, tank), north);
    tick_with(&mut w, vec![Command::Despawn { id: victim }]);
    w.step();
    assert_eq!(w.turret_target_of(tank), None);
    for _ in 0..10 {
        w.step();
    }
    assert_eq!(turret(&w, tank), hull);
    // A unit cannot aim at itself, and a stale id is no target.
    tick_with(
        &mut w,
        vec![Command::AimTurret {
            ids: vec![tank],
            target: Some(tank),
        }],
    );
    assert_eq!(w.turret_target_of(tank), None);
    tick_with(
        &mut w,
        vec![Command::AimTurret {
            ids: vec![tank],
            target: Some(victim),
        }],
    );
    assert_eq!(w.turret_target_of(tank), None);
}

#[test]
fn the_tie_at_half_a_turn_goes_the_decreasing_way_for_the_turret_too() {
    let mut w = world();
    let (tank, victim) = tank_and_victim(&mut w, 128); // exactly behind
    let hull = facing(&w, tank);
    tick_with(
        &mut w,
        vec![Command::AimTurret {
            ids: vec![tank],
            target: Some(victim),
        }],
    );
    assert_eq!(turret(&w, tank), hull.wrapping_sub(12));
}

#[test]
fn an_attack_order_aims_the_turret_at_the_victim_and_a_turret_leaves_the_hull_alone() {
    // Two tanks of the same speed and turn rate attack a unit each, side by side, one
    // with a turret and one without: when they stand in reach, the one without has
    // turned its hull toward the victim, the one with has turned its turret.
    let mut w = world();
    let armed = settled(&mut w, TANK, pt(40, 20));
    let plain = settled(&mut w, PLAIN_TANK, pt(40, 40));
    // The victims stand 8 cells off, a quarter turn round from each hull's heading.
    let v1 = at_angle(&w, armed, 64, 80);
    let v2 = at_angle(&w, plain, 64, 80);
    let victim = spawn(&mut w, RIFLEMAN, v1);
    let victim2 = spawn(&mut w, RIFLEMAN, v2);
    let hull_before = facing(&w, armed);
    tick_with(
        &mut w,
        vec![
            Command::Order {
                ids: vec![armed],
                order: Order::Attack(victim),
                queued: false,
            },
            Command::Order {
                ids: vec![plain],
                order: Order::Attack(victim2),
                queued: false,
            },
        ],
    );
    // The turret aims from the very tick of the order.
    assert_ne!(turret(&w, armed), hull_before);
    for _ in 0..300 {
        w.step();
    }
    let reach = Fx::from_int(5) + Fx::from_ratio(3, 5) + Fx::from_ratio(1, 20);
    assert!(pos(&w, armed).distance(pos(&w, victim)) <= reach);
    assert!(pos(&w, plain).distance(pos(&w, victim2)) <= reach);
    let aim = |from, to| {
        Facing::from_vec2(pos(&w, to) - pos(&w, from))
            .unwrap()
            .raw()
    };
    let near = |a: u8, b: u8| Facing::new(a).abs_diff(Facing::new(b)) <= 2;
    assert!(near(turret(&w, armed), aim(armed, victim)));
    assert!(near(facing(&w, plain), aim(plain, victim2)));
    // The armed tank's hull was left as the drive toward the victim made it: it did not
    // turn on to the victim in reach, which the unarmed one's did: the hull of the armed
    // tank is the way it drove, within the drive tolerance of the aim.
    assert!(Facing::new(facing(&w, armed)).abs_diff(Facing::new(aim(armed, victim))) <= 16);
}

#[test]
fn the_turret_is_in_the_checksum_and_a_world_without_one_hashes_as_before() {
    // Two worlds that differ only in where a turret points hash differently.
    let build = |aim: bool| {
        let mut w = world();
        let (tank, victim) = tank_and_victim(&mut w, 192);
        tick_with(
            &mut w,
            vec![Command::AimTurret {
                ids: vec![tank],
                target: aim.then_some(victim),
            }],
        );
        for _ in 0..3 {
            w.step();
        }
        w.checksum()
    };
    let (a, b) = (build(true), build(false));
    assert_ne!(a, b);
    assert_eq!(a, build(true));
    // Without a turret or an arc turn in the world there is no turning section: a table
    // with the old walk spelled out hashes as the table without any profile.
    let hash = |profiled: bool| {
        let movement = if profiled {
            movement()
                .with_turning(vec![(PIVOT, Turning::pivot(ALIGN_TOLERANCE))])
                .unwrap()
        } else {
            movement()
        };
        let mut w = world_with(movement);
        let id = spawn(&mut w, PLAIN_TANK, pt(40, 40));
        go(&mut w, id, pt(50, 40));
        for _ in 0..20 {
            w.step();
        }
        w.checksum()
    };
    assert_eq!(hash(true), hash(false));
}

#[test]
fn the_drive_tolerance_decides_when_a_pivoting_unit_sets_off() {
    let run = |tolerance: u8| {
        let mut w = world_with(
            movement()
                .with_turning(vec![(PIVOT, Turning::pivot(tolerance))])
                .unwrap(),
        );
        let id = settled(&mut w, PLAIN_TANK, pt(40, 40));
        let start = pos(&w, id);
        let target = at_angle(&w, id, 64, 100); // a quarter turn off: 64 steps
        go(&mut w, id, target);
        let mut n = 0;
        while pos(&w, id) == start {
            w.step();
            n += 1;
        }
        n
    };
    // 64 steps off at 10 a tick, the first turn in the tick of the order: after the 5th
    // turn it is 14 off, after the 6th 4 off, after the 7th on.
    assert_eq!(run(16), 4);
    assert_eq!(run(4), 5);
    assert_eq!(run(0), 6);
    assert_eq!(TurnMode::Pivot, Turning::pivot(0).mode);
}
