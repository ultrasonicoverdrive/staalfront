use sf_math::hash::Fnv1a64;

use super::*;

fn mv(x: i32) -> Order {
    Order::Move(Vec2::from_ints(x, 0))
}

#[test]
fn the_ring_is_first_in_first_out_and_bounded() {
    let mut q = OrderQueue::EMPTY;
    assert!(q.is_empty());
    assert_eq!(q.pop_front(), None);
    for k in 0..ORDER_QUEUE_CAP as i32 {
        assert!(q.push_back(mv(k)));
    }
    assert_eq!(q.len(), ORDER_QUEUE_CAP);
    assert!(!q.push_back(mv(99)));
    assert_eq!(q.len(), ORDER_QUEUE_CAP);
    assert_eq!(q.pop_front(), Some(mv(0)));
    assert_eq!(q.pop_front(), Some(mv(1)));
    // The ring wraps around the end of the array.
    assert!(q.push_back(mv(100)));
    assert!(q.push_back(mv(101)));
    assert!(!q.push_back(mv(102)));
    let all: Vec<Order> = q.iter().collect();
    let mut expected: Vec<Order> = (2..ORDER_QUEUE_CAP as i32).map(mv).collect();
    expected.extend([mv(100), mv(101)]);
    assert_eq!(all, expected);
    for want in expected {
        assert_eq!(q.pop_front(), Some(want));
    }
    assert_eq!(q.pop_front(), None);
}

#[test]
fn queues_with_the_same_orders_are_equal_wherever_the_ring_starts() {
    let mut a = OrderQueue::EMPTY;
    let mut b = OrderQueue::EMPTY;
    // Move b's ring on by a few places.
    for k in 0..5 {
        b.push_back(mv(k));
        b.pop_front();
    }
    for k in [3, 1, 4] {
        assert!(a.push_back(mv(k)));
        assert!(b.push_back(mv(k)));
    }
    assert_ne!(a.head, b.head);
    assert_eq!(a, b);
    b.pop_front();
    assert_ne!(a, b);
    // A vacated place keeps no stale order.
    let mut c = OrderQueue::EMPTY;
    c.push_back(mv(7));
    c.pop_front();
    assert_eq!(c.items[0], Order::Idle);
}

#[test]
fn every_order_hashes_differently_and_stably() {
    let id = EntityId::new(3, 1);
    let p = Vec2::from_ints(4, 5);
    let orders = [
        Order::Idle,
        Order::Move(p),
        Order::Attack(id),
        Order::AttackMove(p),
        Order::Guard(GuardTarget::Point(p)),
        Order::Guard(GuardTarget::Entity(id)),
        Order::Scatter,
        Order::HoldPosition,
    ];
    let hashes: Vec<u64> = orders
        .iter()
        .map(|o| {
            let mut h = Fnv1a64::new();
            o.write_hash(&mut h);
            h.finish()
        })
        .collect();
    for (i, a) in hashes.iter().enumerate() {
        for b in &hashes[i + 1..] {
            assert_ne!(a, b);
        }
    }
    // The tags are part of the checksum format: a change moves every
    // checksum that has an order in it.
    let mut h = Fnv1a64::new();
    h.write_u8(1);
    h.write_vec2(p);
    let mut m = Fnv1a64::new();
    Order::Move(p).write_hash(&mut m);
    assert_eq!(m.finish(), h.finish());
}

#[test]
fn the_scatter_step_is_pinned() {
    // Frozen values: the scatter of a given unit on a given tick is part of
    // the simulation's behaviour (replays and saves depend on it).
    let o = scatter_offset(EntityId::new(0, 0), Tick(0));
    let o2 = scatter_offset(EntityId::new(0, 0), Tick(1));
    let o3 = scatter_offset(EntityId::new(1, 0), Tick(0));
    assert_ne!(o, o2);
    assert_ne!(o, o3);
    for step in [o, o2, o3] {
        let len = step.length();
        assert!(
            len >= SCATTER_MIN - Fx::from_ratio(1, 100)
                && len <= SCATTER_MAX + Fx::from_ratio(1, 100)
        );
    }
    assert_eq!(
        (o.x.raw(), o.y.raw(), o2.x.raw(), o2.y.raw()),
        PINNED_SCATTER
    );
}

/// `scatter_offset(0v0, tick 0)` then `(0v0, tick 1)`, as raw `(x, y, x, y)`.
const PINNED_SCATTER: (i64, i64, i64, i64) = (-164575, 77837, 62669, 175153);

#[test]
fn the_slack_of_a_crowd_held_up_on_its_destination_grows_but_stays_inside_the_disc() {
    let cells = |n: i64| Fx::from_int(n as i32);
    // Without a disc to measure by, a few bodies fill little ground.
    assert_eq!(crowd_arrival_radius(Fx::ZERO, 0, Fx::ZERO), CROWD_ARRIVE);
    assert_eq!(
        crowd_arrival_radius(Fx::ZERO, 1, Fx::ZERO),
        CROWD_ARRIVE + CROWD_ARRIVE_STEP
    );
    assert_eq!(
        crowd_arrival_radius(Fx::ZERO, 1000, Fx::ZERO),
        CROWD_ARRIVE_ALONE
    );
    // With one: thirteen tenths of its radius, however long the unit has pressed.
    let disc = cells(5);
    let bound = disc.saturating_mul(CROWD_ARRIVE_DISC);
    assert_eq!(crowd_arrival_radius(Fx::ZERO, 1000, disc), bound);
    assert_eq!(crowd_arrival_radius(Fx::ZERO, u32::MAX, disc), bound);
    // It never shrinks as the unit presses on, and never passes the bound.
    let mut last = Fx::ZERO;
    for steps in 0..40 {
        let r = crowd_arrival_radius(Fx::ZERO, steps, disc);
        assert!(r >= last && r <= bound, "{steps} steps: {r:?}");
        last = r;
    }
    // The slot of a unit lies where it lies: its own slack round it counts even when
    // the slot is farther out than the bound (the outer ring of a big group).
    let far = cells(9);
    assert_eq!(crowd_arrival_radius(far, 0, disc), far + CROWD_ARRIVE);
    assert_eq!(crowd_arrival_radius(far, 1000, disc), far + CROWD_ARRIVE);
    // And a slot inside it does not let the slack carry the unit past the bound.
    let near = cells(3);
    assert_eq!(crowd_arrival_radius(near, 1000, disc), bound);
}
