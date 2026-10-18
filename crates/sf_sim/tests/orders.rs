//! Orders as state (WP3.4), through the public API: move, attack, attack-move,
//! guard, stop and scatter, the queue behind a unit's current order, the
//! refusals, and the sequence numbers that make two machines agree.

use sf_math::{Fx, Vec2};
use sf_sim::{
    ARRIVE_TOLERANCE, Cell, Command, EntityId, Footprint, GUARD_CLOSE, GUARD_LEASH, GuardTarget,
    Kind, ORDER_QUEUE_CAP, Order, PlayerId, SCATTER_MAX, SCATTER_MIN, ScheduleError, Tick,
    UnitStats, UnitTable, World, WorldConfig, per_tick, scatter_offset,
};

const MAP: u32 = 64;

// A slow, short-ranged infantry unit, a faster tank with a longer reach, an
// unarmed truck, a structure and an object.
const RIFLEMAN: Kind = Kind(1);
const TANK: Kind = Kind(2);
const TRUCK: Kind = Kind(3);
const HOUSE: Kind = Kind(4); // structure 2 x 3
const CRATE: Kind = Kind(5);

fn table() -> UnitTable {
    let mobile =
        |kind: Kind, per_second_tenths: i64, turn, size_tenths: i64, range: i64| UnitStats {
            kind,
            hp: 100,
            movement_class: Some(0),
            speed_per_tick: per_tick(Fx::from_ratio(per_second_tenths, 10)),
            turn_per_tick: turn,
            size: Fx::from_ratio(size_tenths, 10),
            footprint: None,
            weapon_range: Fx::from_int(range as i32),
        };
    UnitTable::new(vec![
        mobile(RIFLEMAN, 20, 16, 4, 4),
        mobile(TANK, 30, 10, 12, 5),
        mobile(TRUCK, 40, 12, 12, 0),
        UnitStats {
            kind: HOUSE,
            hp: 700,
            movement_class: None,
            speed_per_tick: Fx::ZERO,
            turn_per_tick: 0,
            size: Fx::ZERO,
            footprint: Some(Footprint {
                width: 2,
                height: 3,
            }),
            weapon_range: Fx::ZERO,
        },
        UnitStats {
            kind: CRATE,
            hp: 10,
            movement_class: None,
            speed_per_tick: Fx::ZERO,
            turn_per_tick: 0,
            size: Fx::ONE,
            footprint: None,
            weapon_range: Fx::ZERO,
        },
    ])
    .unwrap()
}

fn world() -> World {
    World::new(WorldConfig::new(11).with_map_size(MAP, MAP))
        .unwrap()
        .with_units(table())
        .unwrap()
}

fn pt(x: i32, y: i32) -> Vec2 {
    Vec2::from_ints(x, y)
}

/// Runs one tick with the commands given (all by player 0) and returns
/// `(executed, ignored)`.
fn tick_with(w: &mut World, commands: Vec<Command>) -> (u32, u32) {
    let tick = w.tick();
    for c in commands {
        w.schedule(tick, PlayerId(0), c).unwrap();
    }
    let r = w.step();
    (r.commands_executed, r.commands_ignored)
}

/// Spawns a unit on its own tick and returns its id (slots are handed out in
/// order, so the id is the number of entities before it).
fn spawn(w: &mut World, kind: Kind, x: i32, y: i32) -> EntityId {
    let id = EntityId::new(w.slot_count() as u32, 0);
    let (done, _) = tick_with(
        w,
        vec![Command::SpawnUnit {
            kind,
            pos: pt(x, y),
            owner: PlayerId(0),
        }],
    );
    assert_eq!(done, 1);
    assert!(w.is_alive(id));
    id
}

fn order(ids: &[EntityId], order: Order, queued: bool) -> Command {
    Command::Order {
        ids: ids.to_vec(),
        order,
        queued,
    }
}

fn stop(ids: &[EntityId]) -> Command {
    Command::Stop { ids: ids.to_vec() }
}

fn run(w: &mut World, ticks: u64) {
    for _ in 0..ticks {
        w.step();
    }
}

fn pos(w: &World, id: EntityId) -> Vec2 {
    w.get(id).unwrap().pos
}

fn dist(a: Vec2, b: Vec2) -> Fx {
    a.distance(b)
}

/// Steps until `done` holds, at most `limit` ticks; returns the ticks it took.
fn run_until(w: &mut World, limit: u64, mut done: impl FnMut(&World) -> bool) -> u64 {
    for n in 0..limit {
        if done(w) {
            return n;
        }
        w.step();
    }
    panic!("not done after {limit} ticks");
}

// ---- move --------------------------------------------------------------

#[test]
fn a_unit_starts_without_an_order() {
    let mut w = world();
    let u = spawn(&mut w, RIFLEMAN, 5, 5);
    assert_eq!(w.order_of(u), Some(Order::Idle));
    assert_eq!(w.queued_orders_of(u), Some(vec![]));
    assert_eq!(w.waypoint_of(u), None);
    // A stale id has no order.
    assert_eq!(w.order_of(EntityId::new(u.index(), 9)), None);
}

#[test]
fn move_walks_to_the_point_and_ends_idle_within_the_tolerance() {
    let mut w = world();
    let u = spawn(&mut w, TANK, 5, 5);
    let start = pos(&w, u);
    let goal = pt(30, 12);
    tick_with(&mut w, vec![order(&[u], Order::Move(goal), false)]);
    assert_eq!(w.order_of(u), Some(Order::Move(goal)));
    // Destination and route: the order keeps the destination, the unit steers
    // at the waypoint (today the same point).
    run(&mut w, 3);
    assert_eq!(w.order_of(u), Some(Order::Move(goal)));
    assert_eq!(w.waypoint_of(u), Some(goal));

    let took = run_until(&mut w, 600, |w| w.order_of(u) == Some(Order::Idle));
    assert!(dist(pos(&w, u), goal) <= ARRIVE_TOLERANCE);
    assert_eq!(w.waypoint_of(u), None);
    // The straight line at 3 cells a second: no faster than the speed, not
    // much slower (a turn on the spot first).
    let length = dist(start, goal).to_leptons();
    let step = per_tick(Fx::from_int(3)).to_leptons();
    assert!(i64::try_from(took).unwrap() * step >= length - 2 * step);
    assert!(i64::try_from(took).unwrap() * step <= length + 40 * step);
    // And it stays there.
    let at = pos(&w, u);
    run(&mut w, 20);
    assert_eq!(pos(&w, u), at);
}

#[test]
fn move_clamps_a_point_outside_the_map() {
    let mut w = world();
    let u = spawn(&mut w, RIFLEMAN, 60, 60);
    tick_with(&mut w, vec![order(&[u], Order::Move(pt(500, -20)), false)]);
    let Some(Order::Move(p)) = w.order_of(u) else {
        panic!("not moving");
    };
    assert!(p.x < Fx::from_int(MAP as i32) && p.y >= Fx::ZERO);
    run_until(&mut w, 2_000, |w| w.order_of(u) == Some(Order::Idle));
    let end = pos(&w, u);
    assert!(end.x < Fx::from_int(MAP as i32));
    assert!(end.y >= Fx::ZERO);
}

#[test]
fn a_replacing_order_drops_the_queue() {
    let mut w = world();
    let u = spawn(&mut w, RIFLEMAN, 5, 5);
    tick_with(
        &mut w,
        vec![
            order(&[u], Order::Move(pt(40, 5)), false),
            order(&[u], Order::Move(pt(40, 40)), true),
            order(&[u], Order::Move(pt(5, 40)), true),
        ],
    );
    assert_eq!(w.queued_orders_of(u).unwrap().len(), 2);
    tick_with(&mut w, vec![order(&[u], Order::Move(pt(5, 20)), false)]);
    assert_eq!(w.order_of(u), Some(Order::Move(pt(5, 20))));
    assert_eq!(w.queued_orders_of(u), Some(vec![]));
}

// ---- the queue -----------------------------------------------------------

#[test]
fn queued_orders_run_in_order_and_each_ends_before_the_next_starts() {
    let mut w = world();
    let u = spawn(&mut w, TANK, 5, 5);
    let (a, b, c) = (pt(15, 5), pt(15, 15), pt(5, 15));
    tick_with(
        &mut w,
        vec![
            order(&[u], Order::Move(a), false),
            order(&[u], Order::Move(b), true),
            order(&[u], Order::Move(c), true),
        ],
    );
    assert_eq!(
        w.queued_orders_of(u),
        Some(vec![Order::Move(b), Order::Move(c)])
    );
    let mut seen = Vec::new();
    for _ in 0..2_000 {
        let current = w.order_of(u).unwrap();
        if seen.last() != Some(&current) {
            seen.push(current);
        }
        if current == Order::Idle {
            break;
        }
        w.step();
    }
    assert_eq!(
        seen,
        vec![Order::Move(a), Order::Move(b), Order::Move(c), Order::Idle]
    );
    assert!(dist(pos(&w, u), c) <= ARRIVE_TOLERANCE);
    assert_eq!(w.queued_orders_of(u), Some(vec![]));
}

#[test]
fn a_queued_order_on_an_idle_unit_starts_at_once() {
    let mut w = world();
    let u = spawn(&mut w, RIFLEMAN, 5, 5);
    tick_with(&mut w, vec![order(&[u], Order::Move(pt(9, 5)), true)]);
    assert_eq!(w.order_of(u), Some(Order::Move(pt(9, 5))));
    assert_eq!(w.queued_orders_of(u), Some(vec![]));
}

#[test]
fn the_queue_is_bounded_and_the_ring_wraps() {
    let mut w = world();
    let u = spawn(&mut w, TANK, 2, 2);
    // The current order plus a full queue.
    tick_with(&mut w, vec![order(&[u], Order::Move(pt(3, 2)), false)]);
    for k in 0..ORDER_QUEUE_CAP {
        let (done, ignored) = tick_with(
            &mut w,
            vec![order(&[u], Order::Move(pt(4 + k as i32, 2)), true)],
        );
        assert_eq!((done, ignored), (1, 0), "append {k}");
    }
    assert_eq!(w.queued_orders_of(u).unwrap().len(), ORDER_QUEUE_CAP);
    // One more is refused for this unit, deterministically: the command
    // changed nothing.
    let (done, ignored) = tick_with(&mut w, vec![order(&[u], Order::Move(pt(50, 2)), true)]);
    assert_eq!((done, ignored), (0, 1));
    assert_eq!(w.queued_orders_of(u).unwrap().len(), ORDER_QUEUE_CAP);
    // Let it pop a few, then fill the freed places: the ring wraps and keeps
    // the order.
    run_until(&mut w, 600, |w| w.queued_orders_of(u).unwrap().len() <= 5);
    let before = w.queued_orders_of(u).unwrap();
    let free = ORDER_QUEUE_CAP - before.len();
    assert!(free >= 3);
    let mut expected = before;
    for k in 0..free {
        let p = pt(30 + k as i32, 3);
        let (done, _) = tick_with(&mut w, vec![order(&[u], Order::Move(p), true)]);
        assert_eq!(done, 1);
        expected.push(Order::Move(p));
    }
    // The tank may have popped one more in the meantime; what is left is the
    // tail of what was queued, in order.
    let now = w.queued_orders_of(u).unwrap();
    assert!(now.len() <= ORDER_QUEUE_CAP);
    assert!(expected.ends_with(&now), "{now:?} vs {expected:?}");
}

#[test]
fn stop_clears_the_order_and_the_queue_and_the_unit_stands_where_it_is() {
    let mut w = world();
    let u = spawn(&mut w, TANK, 5, 5);
    tick_with(
        &mut w,
        vec![
            order(&[u], Order::Move(pt(50, 5)), false),
            order(&[u], Order::Move(pt(50, 50)), true),
        ],
    );
    run(&mut w, 20);
    let at = pos(&w, u);
    assert_ne!(at, pt(5, 5));
    let (done, ignored) = tick_with(&mut w, vec![stop(&[u])]);
    assert_eq!((done, ignored), (1, 0));
    assert_eq!(w.order_of(u), Some(Order::Idle));
    assert_eq!(w.queued_orders_of(u), Some(vec![]));
    assert_eq!(w.waypoint_of(u), None);
    // The stop ran at the start of its tick: the unit did not move on.
    assert_eq!(pos(&w, u), at);
    run(&mut w, 30);
    assert_eq!(pos(&w, u), at);
}

// ---- refusals --------------------------------------------------------------

#[test]
fn structures_objects_and_stale_ids_refuse_orders() {
    let mut w = world();
    let (done, _) = tick_with(
        &mut w,
        vec![Command::PlaceStructure {
            kind: HOUSE,
            cell: Cell::new(20, 20),
            owner: PlayerId(0),
        }],
    );
    assert_eq!(done, 1);
    let house = EntityId::new(0, 0);
    let krate = spawn(&mut w, CRATE, 30, 30);
    let u = spawn(&mut w, RIFLEMAN, 5, 5);
    let gone = EntityId::new(u.index(), 7);
    let before = w.checksum();
    for o in [
        Order::Move(pt(1, 1)),
        Order::AttackMove(pt(1, 1)),
        Order::Scatter,
        Order::Guard(GuardTarget::Point(pt(1, 1))),
    ] {
        let (done, ignored) = tick_with(&mut w, vec![order(&[house, krate, gone], o, false)]);
        assert_eq!((done, ignored), (0, 1), "{o:?}");
        let (done, _) = tick_with(&mut w, vec![order(&[house, krate, gone], o, true)]);
        assert_eq!(done, 0);
    }
    let (done, _) = tick_with(&mut w, vec![stop(&[house, krate, gone])]);
    assert_eq!(done, 0);
    assert_eq!(w.order_of(house), Some(Order::Idle));
    assert_eq!(w.order_of(krate), Some(Order::Idle));
    assert_eq!(
        pos(&w, house),
        Vec2::new(Fx::from_int(21), Fx::from_ratio(43, 2))
    );
    // Nothing but the tick counter and the history moved.
    assert_ne!(before, w.checksum());
    // A mixed group: the unit takes it, the structure is skipped.
    let (done, ignored) = tick_with(
        &mut w,
        vec![order(&[house, u], Order::Move(pt(9, 5)), false)],
    );
    assert_eq!((done, ignored), (1, 0));
    assert_eq!(w.order_of(u), Some(Order::Move(pt(9, 5))));
    assert_eq!(w.order_of(house), Some(Order::Idle));
}

#[test]
fn idle_is_not_an_order_to_give() {
    let mut w = world();
    let u = spawn(&mut w, RIFLEMAN, 5, 5);
    tick_with(&mut w, vec![order(&[u], Order::Move(pt(40, 5)), false)]);
    let (done, ignored) = tick_with(&mut w, vec![order(&[u], Order::Idle, false)]);
    assert_eq!((done, ignored), (0, 1));
    assert_eq!(w.order_of(u), Some(Order::Move(pt(40, 5))));
}

#[test]
fn attack_needs_a_weapon_a_live_target_and_another_entity() {
    let mut w = world();
    let tank = spawn(&mut w, TANK, 5, 5);
    let truck = spawn(&mut w, TRUCK, 7, 5);
    let foe = spawn(&mut w, RIFLEMAN, 40, 40);
    // Unarmed: refused. Armed: accepted.
    let (done, ignored) = tick_with(&mut w, vec![order(&[truck], Order::Attack(foe), false)]);
    assert_eq!((done, ignored), (0, 1));
    assert_eq!(w.order_of(truck), Some(Order::Idle));
    // Itself: refused.
    let (done, _) = tick_with(&mut w, vec![order(&[tank], Order::Attack(tank), false)]);
    assert_eq!(done, 0);
    // A stale target: refused.
    let stale = EntityId::new(foe.index(), 3);
    let (done, _) = tick_with(&mut w, vec![order(&[tank], Order::Attack(stale), false)]);
    assert_eq!(done, 0);
    assert_eq!(w.order_of(tank), Some(Order::Idle));
    let (done, _) = tick_with(
        &mut w,
        vec![order(&[tank, truck], Order::Attack(foe), false)],
    );
    assert_eq!(done, 1);
    assert_eq!(w.order_of(tank), Some(Order::Attack(foe)));
    assert_eq!(w.order_of(truck), Some(Order::Idle));
    // A guard of itself or of a stale id is refused too; the unarmed may guard.
    let (done, _) = tick_with(
        &mut w,
        vec![order(
            &[truck],
            Order::Guard(GuardTarget::Entity(truck)),
            false,
        )],
    );
    assert_eq!(done, 0);
    let (done, _) = tick_with(
        &mut w,
        vec![order(
            &[truck],
            Order::Guard(GuardTarget::Entity(stale)),
            false,
        )],
    );
    assert_eq!(done, 0);
    let (done, _) = tick_with(
        &mut w,
        vec![order(
            &[truck],
            Order::Guard(GuardTarget::Entity(tank)),
            false,
        )],
    );
    assert_eq!(done, 1);
}

// ---- attack ------------------------------------------------------------------

#[test]
fn attack_approaches_until_the_target_is_in_weapon_reach_and_then_faces_it() {
    let mut w = world();
    let tank = spawn(&mut w, TANK, 5, 5);
    let foe = spawn(&mut w, RIFLEMAN, 40, 30);
    tick_with(&mut w, vec![order(&[tank], Order::Attack(foe), false)]);
    // Reach: the tank's range of 5 cells plus the foe's radius (0.4 / 2).
    let reach = Fx::from_int(5) + Fx::from_ratio(1, 5);
    let mut was_in_reach = false;
    for _ in 0..1_500 {
        w.step();
        let d = dist(pos(&w, tank), pos(&w, foe));
        if d <= reach {
            was_in_reach = true;
            break;
        }
        // Still out of reach: it keeps walking at the target.
        assert!(w.waypoint_of(tank).is_some());
    }
    assert!(was_in_reach);
    // From here it stands: in reach, facing the foe, the order still Attack.
    run(&mut w, 40);
    let d = dist(pos(&w, tank), pos(&w, foe));
    assert!(d <= reach, "{d:?}");
    // It did not walk through the target: it stopped near the edge of its reach.
    assert!(d > reach - Fx::ONE, "{d:?}");
    assert_eq!(w.order_of(tank), Some(Order::Attack(foe)));
    assert_eq!(w.waypoint_of(tank), None);
    let wanted = (pos(&w, foe) - pos(&w, tank)).facing().unwrap();
    assert_eq!(w.get(tank).unwrap().facing, wanted);
}

#[test]
fn attack_follows_a_moving_target() {
    let mut w = world();
    let tank = spawn(&mut w, TANK, 5, 5);
    let foe = spawn(&mut w, RIFLEMAN, 30, 5);
    tick_with(
        &mut w,
        vec![
            order(&[tank], Order::Attack(foe), false),
            // The foe walks away along the row.
            order(&[foe], Order::Move(pt(60, 5)), false),
        ],
    );
    run_until(&mut w, 2_000, |w| w.order_of(foe) == Some(Order::Idle));
    // The tank is faster (3 cells a second against 2) and catches up.
    run(&mut w, 200);
    let d = dist(pos(&w, tank), pos(&w, foe));
    assert!(d <= Fx::from_int(6), "{d:?}");
    assert_eq!(w.order_of(tank), Some(Order::Attack(foe)));
    assert!(pos(&w, tank).x > Fx::from_int(40));
}

#[test]
fn attack_falls_back_to_idle_when_the_target_dies() {
    let mut w = world();
    let tank = spawn(&mut w, TANK, 5, 5);
    let foe = spawn(&mut w, RIFLEMAN, 40, 5);
    tick_with(&mut w, vec![order(&[tank], Order::Attack(foe), false)]);
    run(&mut w, 20);
    assert_eq!(w.order_of(tank), Some(Order::Attack(foe)));
    tick_with(&mut w, vec![Command::Despawn { id: foe }]);
    // The order ends on the next behaviour pass.
    assert_eq!(w.order_of(tank), Some(Order::Idle));
    let at = pos(&w, tank);
    run(&mut w, 20);
    assert_eq!(pos(&w, tank), at);
    assert_eq!(w.waypoint_of(tank), None);
}

#[test]
fn a_new_entity_in_the_dead_targets_slot_is_not_attacked() {
    let mut w = world();
    let tank = spawn(&mut w, TANK, 5, 5);
    let foe = spawn(&mut w, RIFLEMAN, 40, 5);
    tick_with(&mut w, vec![order(&[tank], Order::Attack(foe), false)]);
    // The foe dies and a new rifleman takes its slot in the same tick.
    tick_with(
        &mut w,
        vec![
            Command::Despawn { id: foe },
            Command::SpawnUnit {
                kind: RIFLEMAN,
                pos: pt(41, 5),
                owner: PlayerId(1),
            },
        ],
    );
    let newcomer = EntityId::new(foe.index(), 1);
    assert!(w.is_alive(newcomer));
    assert_eq!(w.order_of(tank), Some(Order::Idle));
}

#[test]
fn a_dead_target_hands_over_to_the_next_queued_order() {
    let mut w = world();
    let tank = spawn(&mut w, TANK, 5, 5);
    let foe = spawn(&mut w, RIFLEMAN, 40, 5);
    tick_with(
        &mut w,
        vec![
            order(&[tank], Order::Attack(foe), false),
            order(&[tank], Order::Move(pt(5, 30)), true),
        ],
    );
    run(&mut w, 10);
    tick_with(&mut w, vec![Command::Despawn { id: foe }]);
    run(&mut w, 1);
    assert_eq!(w.order_of(tank), Some(Order::Move(pt(5, 30))));
    run_until(&mut w, 1_000, |w| w.order_of(tank) == Some(Order::Idle));
    assert!(dist(pos(&w, tank), pt(5, 30)) <= ARRIVE_TOLERANCE);
}

#[test]
fn a_structure_can_be_attacked_from_the_edge_of_the_reach() {
    let mut w = world();
    tick_with(
        &mut w,
        vec![Command::PlaceStructure {
            kind: HOUSE,
            cell: Cell::new(30, 30),
            owner: PlayerId(1),
        }],
    );
    let house = EntityId::new(0, 0);
    let tank = spawn(&mut w, TANK, 5, 5);
    tick_with(&mut w, vec![order(&[tank], Order::Attack(house), false)]);
    run(&mut w, 800);
    // Reach: 5 cells plus the shorter half side (one cell) from the middle.
    let d = dist(pos(&w, tank), pos(&w, house));
    assert!(d <= Fx::from_int(6) && d > Fx::from_int(5), "{d:?}");
}

// ---- attack-move and guard ------------------------------------------------

#[test]
fn attack_move_walks_to_the_point_and_ends() {
    let mut w = world();
    let u = spawn(&mut w, RIFLEMAN, 5, 5);
    let goal = pt(25, 25);
    tick_with(&mut w, vec![order(&[u], Order::AttackMove(goal), false)]);
    assert_eq!(w.order_of(u), Some(Order::AttackMove(goal)));
    run_until(&mut w, 1_000, |w| w.order_of(u) == Some(Order::Idle));
    assert!(dist(pos(&w, u), goal) <= ARRIVE_TOLERANCE);
    // An unarmed unit may attack-move: it simply walks.
    let t = spawn(&mut w, TRUCK, 5, 5);
    let (done, _) = tick_with(&mut w, vec![order(&[t], Order::AttackMove(goal), false)]);
    assert_eq!(done, 1);
}

#[test]
fn a_point_guard_goes_to_its_post_and_stays_and_blocks_the_queue() {
    let mut w = world();
    let u = spawn(&mut w, RIFLEMAN, 5, 5);
    let post = pt(20, 8);
    tick_with(
        &mut w,
        vec![
            order(&[u], Order::Guard(GuardTarget::Point(post)), false),
            order(&[u], Order::Move(pt(1, 1)), true),
        ],
    );
    run_until(&mut w, 600, |w| dist(pos(w, u), post) <= ARRIVE_TOLERANCE);
    run(&mut w, 100);
    // Still guarding; the queued move waits behind it.
    assert_eq!(w.order_of(u), Some(Order::Guard(GuardTarget::Point(post))));
    assert_eq!(w.queued_orders_of(u), Some(vec![Order::Move(pt(1, 1))]));
    assert!(dist(pos(&w, u), post) <= ARRIVE_TOLERANCE);
    assert_eq!(w.waypoint_of(u), None);
    // A new order replaces it.
    tick_with(&mut w, vec![order(&[u], Order::Move(pt(1, 1)), false)]);
    assert_eq!(w.order_of(u), Some(Order::Move(pt(1, 1))));
}

#[test]
fn an_entity_guard_follows_at_a_distance_and_ends_with_the_entity() {
    let mut w = world();
    let boss = spawn(&mut w, TANK, 10, 10);
    let guard = spawn(&mut w, RIFLEMAN, 10, 14);
    tick_with(
        &mut w,
        vec![
            order(&[guard], Order::Guard(GuardTarget::Entity(boss)), false),
            order(&[boss], Order::Move(pt(50, 10)), false),
        ],
    );
    let radius = Fx::from_ratio(3, 5); // half the tank's size of 1.2
    // The guard is slower than the boss and trails behind it on the way.
    run_until(&mut w, 2_000, |w| w.order_of(boss) == Some(Order::Idle));
    let trailing = dist(pos(&w, guard), pos(&w, boss));
    assert!(trailing > GUARD_LEASH + radius, "{trailing:?}");
    // It catches up, and stops within the close distance of the boss's edge.
    run(&mut w, 400);
    let d = dist(pos(&w, guard), pos(&w, boss));
    assert!(d <= GUARD_CLOSE + radius, "{d:?}");
    assert_eq!(w.waypoint_of(guard), None);
    // Standing near, it stays: no shuffling while the boss stands still.
    let at = pos(&w, guard);
    run(&mut w, 100);
    assert_eq!(pos(&w, guard), at);
    assert_eq!(
        w.order_of(guard),
        Some(Order::Guard(GuardTarget::Entity(boss)))
    );
    // The boss dies: the guard falls back to idle.
    tick_with(&mut w, vec![Command::Despawn { id: boss }]);
    run(&mut w, 1);
    assert_eq!(w.order_of(guard), Some(Order::Idle));
}

// ---- scatter -----------------------------------------------------------------

#[test]
fn scatter_is_a_short_deterministic_step_away() {
    let run_one = || {
        let mut w = world();
        let u = spawn(&mut w, RIFLEMAN, 30, 30);
        let start = pos(&w, u);
        tick_with(&mut w, vec![order(&[u], Order::Scatter, false)]);
        // It became the move to the spot the pure function names.
        let spot = start + scatter_offset(u, Tick(w.tick().0 - 1));
        assert_eq!(w.order_of(u), Some(Order::Move(spot)));
        let length = dist(start, spot);
        assert!(length >= SCATTER_MIN && length <= SCATTER_MAX + Fx::from_ratio(1, 100));
        run_until(&mut w, 400, |w| w.order_of(u) == Some(Order::Idle));
        assert!(dist(pos(&w, u), spot) <= ARRIVE_TOLERANCE);
        (spot, w.checksum())
    };
    assert_eq!(run_one(), run_one());
}

#[test]
fn scatter_sends_a_group_in_different_directions() {
    let mut w = world();
    let group: Vec<EntityId> = (0..12)
        .map(|k| spawn(&mut w, RIFLEMAN, 30 + (k % 4), 30 + (k / 4)))
        .collect();
    tick_with(&mut w, vec![order(&group, Order::Scatter, false)]);
    let mut spots: Vec<Vec2> = group
        .iter()
        .map(|&id| match w.order_of(id).unwrap() {
            Order::Move(p) => p,
            other => panic!("{other:?}"),
        })
        .collect();
    // Offsets differ per unit.
    let offsets: Vec<Vec2> = group
        .iter()
        .zip(&spots)
        .map(|(&id, &spot)| spot - pos(&w, id))
        .collect();
    spots.sort_by_key(|p| (p.x, p.y));
    spots.dedup();
    assert!(spots.len() >= 10);
    assert!(offsets.iter().any(|o| o.x > Fx::ZERO) && offsets.iter().any(|o| o.x < Fx::ZERO));
}

#[test]
fn a_queued_scatter_waits_in_the_queue_and_is_picked_when_it_starts() {
    let mut w = world();
    let u = spawn(&mut w, RIFLEMAN, 10, 10);
    tick_with(
        &mut w,
        vec![
            order(&[u], Order::Move(pt(20, 10)), false),
            order(&[u], Order::Scatter, true),
        ],
    );
    assert_eq!(w.queued_orders_of(u), Some(vec![Order::Scatter]));
    let mut started_at = None;
    for _ in 0..600 {
        w.step();
        if w.queued_orders_of(u).unwrap().is_empty() {
            started_at = Some(w.tick());
            break;
        }
    }
    let started_at = started_at.expect("the move ended");
    // The queued scatter started at the end of the move, from where it stood
    // (near (20, 10)), on the tick it was popped.
    let Some(Order::Move(spot)) = w.order_of(u) else {
        panic!("not moving");
    };
    let from = pt(20, 10);
    assert!(
        dist(from, spot) <= SCATTER_MAX + ARRIVE_TOLERANCE + Fx::ONE,
        "{started_at}"
    );
    assert!(dist(from, spot) >= SCATTER_MIN - ARRIVE_TOLERANCE);
}

#[test]
fn scatter_stays_on_the_map() {
    let mut w = world();
    let corner = spawn(&mut w, RIFLEMAN, 0, 0);
    for _ in 0..40 {
        tick_with(&mut w, vec![order(&[corner], Order::Scatter, false)]);
        // A spot that the map edge clamps onto the unit's own place is reached
        // at once; otherwise the unit is on its way to a spot on the map.
        match w.order_of(corner).unwrap() {
            Order::Move(p) => assert!(p.x >= Fx::ZERO && p.y >= Fx::ZERO),
            Order::Idle => {}
            other => panic!("{other:?}"),
        }
        run(&mut w, 15);
        let p = pos(&w, corner);
        assert!(p.x >= Fx::ZERO && p.y >= Fx::ZERO);
    }
}

#[test]
fn the_scatter_step_is_a_pure_function_of_id_and_tick() {
    let id = EntityId::new(17, 2);
    assert_eq!(scatter_offset(id, Tick(40)), scatter_offset(id, Tick(40)));
    let mut seen = std::collections::BTreeSet::new();
    for t in 0..64 {
        let o = scatter_offset(id, Tick(t));
        let len = o.length();
        assert!(len >= SCATTER_MIN - Fx::from_ratio(1, 100));
        assert!(len <= SCATTER_MAX + Fx::from_ratio(1, 100));
        seen.insert((o.x.raw(), o.y.raw()));
    }
    assert!(seen.len() > 50);
    let mut others = std::collections::BTreeSet::new();
    for index in 0..64 {
        others.insert(scatter_offset(EntityId::new(index, 0), Tick(40)).x.raw());
    }
    assert!(others.len() > 50);
}

// ---- the legacy commands ------------------------------------------------------

#[test]
fn the_legacy_commands_take_over_from_an_order() {
    let mut w = world();
    let u = spawn(&mut w, RIFLEMAN, 5, 5);
    tick_with(
        &mut w,
        vec![
            order(&[u], Order::Move(pt(40, 5)), false),
            order(&[u], Order::Move(pt(40, 40)), true),
        ],
    );
    tick_with(
        &mut w,
        vec![Command::MoveTo {
            ids: vec![u],
            target: pt(5, 40),
        }],
    );
    assert_eq!(w.order_of(u), Some(Order::Idle));
    assert_eq!(w.queued_orders_of(u), Some(vec![]));
    run_until(&mut w, 800, |w| {
        dist(pos(w, u), pt(5, 40)) <= Fx::from_ratio(1, 4)
    });
    // And an order takes over from a wander.
    tick_with(
        &mut w,
        vec![Command::SetWanderArea {
            ids: vec![u],
            area: sf_sim::Area::from_cells(0, 0, 20, 20),
        }],
    );
    tick_with(&mut w, vec![order(&[u], Order::Move(pt(30, 30)), false)]);
    run_until(&mut w, 1_500, |w| w.order_of(u) == Some(Order::Idle));
    assert!(dist(pos(&w, u), pt(30, 30)) <= ARRIVE_TOLERANCE);
}

#[test]
fn a_legacy_wanderer_can_be_ordered_with_the_worlds_speed() {
    let mut w = world();
    let (done, _) = tick_with(
        &mut w,
        vec![Command::Spawn {
            kind: Kind(77),
            pos: pt(5, 5),
            owner: PlayerId(0),
        }],
    );
    assert_eq!(done, 1);
    let wanderer = EntityId::new(0, 0);
    let (done, _) = tick_with(
        &mut w,
        vec![order(&[wanderer], Order::Move(pt(20, 5)), false)],
    );
    assert_eq!(done, 1);
    run_until(&mut w, 400, |w| w.order_of(wanderer) == Some(Order::Idle));
    assert!(dist(pos(&w, wanderer), pt(20, 5)) <= ARRIVE_TOLERANCE);
    // It has no weapon: no attack.
    let (done, _) = tick_with(
        &mut w,
        vec![order(
            &[wanderer],
            Order::Attack(EntityId::new(1, 0)),
            false,
        )],
    );
    assert_eq!(done, 0);
}

// ---- the checksum and the sequence numbers --------------------------------------

/// A little script: two units, a few orders from two players.
fn scripted(arrival: &[usize]) -> World {
    let mut w = world();
    let a = spawn(&mut w, TANK, 5, 5);
    let b = spawn(&mut w, RIFLEMAN, 50, 50);
    let tick = w.tick();
    // (player, sequence, command): player 0 moves a, then queues a second
    // move, then stops it; player 1 moves b, then scatters it.
    let script = [
        (0u8, 1u64, order(&[a], Order::Move(pt(30, 5)), false)),
        (0, 2, order(&[a], Order::Move(pt(30, 30)), true)),
        (0, 3, stop(&[a])),
        (1, 1, order(&[b], Order::Move(pt(10, 50)), false)),
        (1, 2, order(&[b], Order::Scatter, true)),
    ];
    for &k in arrival {
        let (player, seq, command) = script[k].clone();
        w.schedule_sequenced(tick, PlayerId(player), seq, command)
            .unwrap();
    }
    run(&mut w, 60);
    w
}

#[test]
fn two_machines_agree_whatever_order_the_commands_arrive_in() {
    let straight = scripted(&[0, 1, 2, 3, 4]);
    let reversed = scripted(&[4, 3, 2, 1, 0]);
    let shuffled = scripted(&[2, 4, 0, 3, 1]);
    assert_eq!(straight.checksum_history(), reversed.checksum_history());
    assert_eq!(straight.checksum_history(), shuffled.checksum_history());
    // The stop (sequence 3) ran last for player 0: unit a stands where the
    // two earlier orders put it, not on its way anywhere.
    let a = EntityId::new(0, 0);
    assert_eq!(straight.order_of(a), Some(Order::Idle));
    // A different sequence changes the outcome: stop first, then the moves.
    let mut w = world();
    let a = spawn(&mut w, TANK, 5, 5);
    let tick = w.tick();
    w.schedule_sequenced(
        tick,
        PlayerId(0),
        3,
        order(&[a], Order::Move(pt(30, 5)), false),
    )
    .unwrap();
    w.schedule_sequenced(tick, PlayerId(0), 2, stop(&[a]))
        .unwrap();
    w.step();
    assert_eq!(w.order_of(a), Some(Order::Move(pt(30, 5))));
    // Numbered commands refuse a reused number.
    let err = w
        .schedule_sequenced(w.tick(), PlayerId(0), 8, stop(&[a]))
        .and_then(|()| w.schedule_sequenced(w.tick(), PlayerId(0), 8, stop(&[a])))
        .unwrap_err();
    assert!(matches!(
        err,
        ScheduleError::DuplicateSequence { sequence: 8, .. }
    ));
}

#[test]
fn orders_are_part_of_the_checksum() {
    let mut a = world();
    let mut b = world();
    let ua = spawn(&mut a, TANK, 5, 5);
    let ub = spawn(&mut b, TANK, 5, 5);
    assert_eq!(a.checksum(), b.checksum());
    // Same destination, one of them queues a second order.
    tick_with(
        &mut a,
        vec![order(
            &[ua],
            Order::Guard(GuardTarget::Point(pt(5, 5))),
            false,
        )],
    );
    tick_with(
        &mut b,
        vec![order(
            &[ub],
            Order::Guard(GuardTarget::Point(pt(5, 5))),
            false,
        )],
    );
    assert_eq!(a.checksum(), b.checksum());
    tick_with(&mut a, vec![order(&[ua], Order::Move(pt(9, 9)), true)]);
    tick_with(&mut b, vec![]);
    assert_ne!(a.checksum(), b.checksum());
    // A world that never saw an order hashes without the orders section: the
    // clone of a world compares equal to it.
    assert_eq!(a.clone(), a);
}

#[test]
fn an_attack_hashes_the_weapon_reach_of_the_table() {
    // The same script on two tables that differ only in the tank's weapon
    // range: the positions soon differ, and so does the checksum at once,
    // through the reach digest.
    let build = |range: i32| {
        let mut stats: Vec<UnitStats> = table().iter().copied().collect();
        for s in &mut stats {
            if s.kind == TANK {
                s.weapon_range = Fx::from_int(range);
            }
        }
        let mut w = World::new(WorldConfig::new(11).with_map_size(MAP, MAP))
            .unwrap()
            .with_units(UnitTable::new(stats).unwrap())
            .unwrap();
        let tank = spawn(&mut w, TANK, 5, 5);
        let foe = spawn(&mut w, RIFLEMAN, 40, 5);
        tick_with(&mut w, vec![order(&[tank], Order::Attack(foe), false)]);
        w
    };
    assert_ne!(build(5).checksum(), build(6).checksum());
    assert_eq!(build(5).checksum(), build(5).checksum());
}
