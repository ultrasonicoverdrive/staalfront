//! The `orders` scenario (WP3.4): every order type, a shift queue, stop,
//! scatter, an attack on a target that walks away and an attack on a target
//! that dies. The states and positions along the way are asserted; the
//! checksum is pinned in `assets/scenarios/orders.expected` (see `pinned.rs`).
//!
//! The cast and the commands are written in `assets/scenarios/orders.ron`,
//! which also lists the entity ids used below.

mod common;

use common::scenario_path;
use sf_headless::scenario::Scenario;
use sf_math::{Fx, Vec2};
use sf_sim::{
    ARRIVE_TOLERANCE, EntityId, GUARD_CLOSE, GuardTarget, Order, SCATTER_MAX, StepReport, Tick,
    World, scatter_offset,
};

const PLANT: EntityId = EntityId::new(0, 0);
const A: EntityId = EntityId::new(1, 0);
const B: EntityId = EntityId::new(2, 0);
const R1: EntityId = EntityId::new(3, 0);
const R2: EntityId = EntityId::new(4, 0);
const S: EntityId = EntityId::new(5, 0);
const T: EntityId = EntityId::new(6, 0);
const H: EntityId = EntityId::new(7, 0);
const W: EntityId = EntityId::new(8, 0);
const F: EntityId = EntityId::new(9, 0);
const CRATE: EntityId = EntityId::new(10, 0);
const G: EntityId = EntityId::new(11, 0);

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("orders.ron")).unwrap()
}

/// The middle of a cell.
fn mid(x: i32, y: i32) -> Vec2 {
    Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF)
}

/// The world after `ticks` ticks (state "after tick `ticks - 1`") and the report
/// of every one of them.
struct Run {
    world: World,
    reports: Vec<StepReport>,
}

impl Run {
    fn new() -> Run {
        Run {
            world: scenario().build_world().unwrap(),
            reports: Vec::new(),
        }
    }

    /// Steps until `ticks` ticks have run in all.
    fn to(&mut self, ticks: u64) -> &World {
        while self.world.tick().0 < ticks {
            self.reports.push(self.world.step());
        }
        &self.world
    }
}

fn pos(w: &World, id: EntityId) -> Vec2 {
    w.get(id).unwrap().pos
}

fn dist(w: &World, a: EntityId, b: EntityId) -> Fx {
    pos(w, a).distance(pos(w, b))
}

fn order(w: &World, id: EntityId) -> Order {
    w.order_of(id).unwrap()
}

fn queue(w: &World, id: EntityId) -> Vec<Order> {
    w.queued_orders_of(id).unwrap()
}

#[test]
fn the_scenario_is_the_cast_it_says() {
    let s = scenario();
    assert_eq!((s.map.width, s.map.height), (64, 64));
    assert_eq!(s.unit_defs.as_deref(), Some("orders_defs"));
    let mut r = Run::new();
    let w = r.to(1);
    assert_eq!(w.len(), 12);
    // The kinds as the scenario's comment lists them (the frozen definitions).
    let kinds: Vec<u16> = (0..12)
        .map(|i| w.get(EntityId::new(i, 0)).unwrap().kind.0)
        .collect();
    assert_eq!(kinds, vec![1, 19, 19, 12, 12, 15, 22, 20, 12, 12, 35, 15]);
    // The structure and the object cannot be ordered, ever.
    assert!(w.stats_of(PLANT).unwrap().footprint.is_some());
    assert!(w.stats_of(CRATE).unwrap().movement_class.is_none());
}

#[test]
fn commands_arrive_as_state_on_their_tick_and_refusals_are_counted() {
    let mut r = Run::new();
    // Tick 10: A's three moves, W's move; the move of the structure and the
    // object and the truck's attack change nothing.
    let w = r.to(11);
    assert_eq!(order(w, A), Order::Move(mid(24, 4)));
    assert_eq!(
        queue(w, A),
        vec![Order::Move(mid(24, 16)), Order::Move(mid(8, 16))]
    );
    assert_eq!(order(w, W), Order::Move(mid(58, 58)));
    assert_eq!(order(w, PLANT), Order::Idle);
    assert_eq!(order(w, CRATE), Order::Idle);
    assert_eq!(order(w, T), Order::Idle);
    let report = r.reports[10];
    assert_eq!((report.commands_executed, report.commands_ignored), (4, 2));
    // Tick 0 spawned the twelve of the cast and refused nothing.
    assert_eq!(
        (
            r.reports[0].commands_executed,
            r.reports[0].commands_ignored
        ),
        (12, 0)
    );
    assert!(
        r.reports[1..10]
            .iter()
            .all(|rep| rep.commands_executed == 0 && rep.commands_ignored == 0)
    );
}

#[test]
fn the_orders_of_tick_twenty_to_forty_are_in_place() {
    let mut r = Run::new();
    let w = r.to(41);
    assert_eq!(order(w, B), Order::Attack(W));
    assert_eq!(order(w, R1), Order::AttackMove(mid(24, 12)));
    assert_eq!(order(w, R2), Order::Guard(GuardTarget::Point(mid(8, 14))));
    assert_eq!(order(w, H), Order::Attack(F));
    assert_eq!(queue(w, H), vec![Order::Move(mid(12, 44))]);
    // The truck guards tank A from tick 40.
    assert_eq!(order(w, T), Order::Guard(GuardTarget::Entity(A)));
    // Scatter: S and G became the move to the spot the pure function names
    // (the scatter started on tick 30 from where each stood).
    let spot = |start: Vec2, id| start + scatter_offset(id, Tick(30));
    assert_eq!(order(w, S), Order::Move(spot(mid(4, 16), S)));
    assert_eq!(queue(w, S), vec![Order::Move(mid(30, 20))]);
    assert_eq!(order(w, G), Order::Move(spot(mid(10, 50), G)));
    let step = Fx::from_ratio(1, 100);
    for (id, start) in [(S, mid(4, 16)), (G, mid(10, 50))] {
        let Order::Move(p) = order(w, id) else {
            unreachable!()
        };
        let length = start.distance(p);
        assert!(length >= Fx::ONE - step && length <= SCATTER_MAX + step);
    }
    // S and G took different steps.
    assert_ne!(
        scatter_offset(S, Tick(30)),
        scatter_offset(G, Tick(30)),
        "different units scatter differently"
    );
}

#[test]
fn the_shift_queue_runs_a_moves_in_order() {
    let mut r = Run::new();
    let mut seen: Vec<(Order, usize)> = Vec::new();
    for t in 1..=800 {
        let w = r.to(t);
        let now = (order(w, A), queue(w, A).len());
        if seen.last() != Some(&now) && t > 10 {
            seen.push(now);
        }
    }
    assert_eq!(
        seen,
        vec![
            (Order::Move(mid(24, 4)), 2),
            (Order::Move(mid(24, 16)), 1),
            (Order::Move(mid(8, 16)), 0),
            (Order::Idle, 0),
        ]
    );
    let w = r.to(800);
    assert!(pos(w, A).distance(mid(8, 16)) <= ARRIVE_TOLERANCE);
}

#[test]
fn attack_move_ends_at_its_point() {
    let mut r = Run::new();
    let w = r.to(700);
    assert_eq!(order(w, R1), Order::Idle);
    assert!(pos(w, R1).distance(mid(24, 12)) <= ARRIVE_TOLERANCE);
}

#[test]
fn a_stop_clears_a_scatter_and_its_queue() {
    let mut r = Run::new();
    // Just before the stop S is on its way to (30, 20) with nothing left
    // behind it.
    let before = r.to(250);
    assert_eq!(order(before, S), Order::Move(mid(30, 20)));
    let en_route = pos(before, S);
    let w = r.to(251);
    assert_eq!(order(w, S), Order::Idle);
    assert!(queue(w, S).is_empty());
    assert_eq!(w.waypoint_of(S), None);
    // It stands where the stop found it (one tick of walking at most).
    assert!(pos(w, S).distance(en_route) < Fx::from_ratio(1, 10));
    let stopped_at = pos(w, S);
    let end = r.to(1_600);
    assert_eq!(pos(end, S), stopped_at);
}

#[test]
fn a_point_guard_holds_its_post_and_blocks_the_queue_until_stop() {
    let mut r = Run::new();
    let w = r.to(300);
    assert!(pos(w, R2).distance(mid(8, 14)) <= ARRIVE_TOLERANCE);
    let post = pos(w, R2);
    // A queued move waits behind the guard from tick 600.
    let w = r.to(899);
    assert_eq!(order(w, R2), Order::Guard(GuardTarget::Point(mid(8, 14))));
    assert_eq!(queue(w, R2), vec![Order::Move(mid(8, 40))]);
    assert_eq!(pos(w, R2), post);
    // The stop at tick 900 clears the guard and the queue.
    let w = r.to(901);
    assert_eq!(order(w, R2), Order::Idle);
    assert!(queue(w, R2).is_empty());
    assert_eq!(pos(r.to(1_600), R2), post);
}

#[test]
fn an_attack_follows_a_target_that_walks_away() {
    let mut r = Run::new();
    // Tank B is faster than rifleman W and closes in on it while W walks.
    let reach = Fx::from_int(5) + Fx::from_ratio(1, 5);
    let w = r.to(300);
    assert!(dist(w, B, W) > reach);
    let gap_early = dist(w, B, W);
    let w = r.to(600);
    assert!(dist(w, B, W) < gap_early);
    assert!(matches!(order(w, W), Order::Move(_)));
    // Later it is within reach and keeps following while W still walks.
    let mut within = 0;
    for t in 800..940 {
        let w = r.to(t);
        assert_eq!(order(w, B), Order::Attack(W), "tick {t}");
        if matches!(order(w, W), Order::Move(_)) && dist(w, B, W) <= reach + Fx::from_ratio(1, 5) {
            within += 1;
        }
    }
    assert!(within > 100, "{within}");
    // W has arrived and B stands in reach of it, still on the attack.
    let end = r.to(1_600);
    assert_eq!(order(end, W), Order::Idle);
    assert!(pos(end, W).distance(mid(58, 58)) <= ARRIVE_TOLERANCE);
    assert!(dist(end, B, W) <= reach);
    assert_eq!(order(end, B), Order::Attack(W));
    // It stands facing its target.
    let wanted = (pos(end, W) - pos(end, B)).facing().unwrap();
    assert_eq!(end.get(B).unwrap().facing, wanted);
}

#[test]
fn an_attack_ends_when_the_target_dies_and_the_queue_goes_on() {
    let mut r = Run::new();
    // Heavy tank H has reached F's weapon reach (5 cells plus F's radius) and
    // stands there, still attacking.
    let reach = Fx::from_int(5) + Fx::from_ratio(1, 5);
    let w = r.to(700);
    assert_eq!(order(w, H), Order::Attack(F));
    assert!(dist(w, H, F) <= reach);
    assert_eq!(queue(w, H), vec![Order::Move(mid(12, 44))]);
    let stood = pos(w, H);
    // F is despawned at the start of tick 700: H's attack ends in that tick
    // and the queued move takes over.
    assert!(!r.to(701).is_alive(F));
    assert_eq!(order(r.to(701), H), Order::Move(mid(12, 44)));
    assert!(queue(r.to(701), H).is_empty());
    assert_eq!(pos(r.to(701), H), stood);
    let end = r.to(1_600);
    assert_eq!(order(end, H), Order::Idle);
    assert!(pos(end, H).distance(mid(12, 44)) <= ARRIVE_TOLERANCE);
}

#[test]
fn the_truck_guards_the_tank_from_a_distance() {
    let mut r = Run::new();
    let w = r.to(1_600);
    assert_eq!(order(w, T), Order::Guard(GuardTarget::Entity(A)));
    // A stands still at its last stop; T stands near it, no closer than it
    // needs (tank radius is 0.6) and no farther than the close distance.
    let radius = Fx::from_ratio(3, 5);
    assert!(dist(w, T, A) <= GUARD_CLOSE + radius);
    assert!(dist(w, T, A) > radius);
    assert_eq!(w.waypoint_of(T), None);
}

#[test]
fn a_queued_scatter_starts_when_it_comes_up() {
    let mut r = Run::new();
    // G scatters twice: the second, queued at tick 130 behind the first (which
    // started at once, G being idle), starts on a tick of its own and from
    // where G stood then.
    let w = r.to(131);
    let first = pos(w, G);
    assert!(matches!(order(w, G), Order::Move(_)));
    // Both were given at tick 130 to an idle unit: the first started at once,
    // the second waits as a scatter in the queue.
    assert_eq!(queue(w, G), vec![Order::Scatter]);
    let end = r.to(1_600);
    assert_eq!(order(end, G), Order::Idle);
    assert!(queue(end, G).is_empty());
    // Two steps of at most three cells each, from (7.63, 48.66)'s neighbourhood.
    assert!(pos(end, G).distance(first) <= SCATTER_MAX + SCATTER_MAX);
}

#[test]
fn the_structure_and_the_object_never_move_or_take_orders() {
    let mut r = Run::new();
    let w = r.to(1_600);
    assert_eq!(
        pos(w, PLANT),
        Vec2::new(Fx::from_int(51), Fx::from_ratio(23, 2))
    );
    assert_eq!(pos(w, CRATE), mid(20, 20));
    assert_eq!(order(w, PLANT), Order::Idle);
    assert_eq!(order(w, CRATE), Order::Idle);
}

#[test]
fn the_run_is_reproducible() {
    let run = || {
        let mut r = Run::new();
        r.to(1_600);
        r.world.checksum_history().to_vec()
    };
    assert_eq!(run(), run());
}
