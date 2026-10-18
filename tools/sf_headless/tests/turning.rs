//! The `turning` scenario (WP4.5, `docs/rules/turning.md`): the hull turns before it
//! drives (a tank ordered behind itself pivots in place, a wheeled vehicle swings
//! round in an arc, a rifleman turns at once) and a turret turns apart from the hull
//! (it follows a moving target while the hull drives on, aims at the victim of an
//! attack order ahead of the hull, and goes back to hull-forward when let go). The
//! headings over time are asserted; the checksum is pinned in
//! `assets/scenarios/turning.expected` (see `pinned.rs`).
//!
//! The cast and the commands are written in `assets/scenarios/turning.ron`, which also
//! lists the entity ids used below. `state[t]` is the world after `t` ticks have run:
//! a command of tick `n` shows in `state[n + 1]`.

mod common;

use common::scenario_path;
use sf_headless::scenario::Scenario;
use sf_math::{Facing, Fx, Vec2};
use sf_sim::{ARRIVE_TOLERANCE, EntityId, EntityView, Order};

const T1: EntityId = EntityId::new(0, 0); // light tank: ordered behind itself
const W1: EntityId = EntityId::new(1, 0); // light transport: swings round in an arc
const I1: EntityId = EntityId::new(2, 0); // rifleman: turns at once
const T2: EntityId = EntityId::new(3, 0); // heavy tank: the turret follows W2
const W2: EntityId = EntityId::new(4, 0); // the moving target
const T3: EntityId = EntityId::new(5, 0); // light tank: attacks V
const V: EntityId = EntityId::new(6, 0); // the victim

/// The tick of the orders that send T1, W1 and I1 back.
const BACK: usize = 250;
/// The tick T2's turret is let go.
const LET_GO: usize = 600;

/// The world after every tick of the run.
struct Run {
    /// `state[t]`: every entity after `t` ticks, in slot order (`state[0]` is the empty
    /// world: the cast is spawned by the commands of tick 0).
    state: Vec<Vec<EntityView>>,
    world: sf_sim::World,
}

fn run() -> Run {
    let scenario = Scenario::load(&scenario_path("turning.ron")).unwrap();
    let mut world = scenario.build_world().unwrap();
    let mut state = vec![world.snapshot().iter().collect::<Vec<_>>()];
    for _ in 0..scenario.ticks {
        world.step();
        state.push(world.snapshot().iter().collect());
    }
    Run { state, world }
}

impl Run {
    fn at(&self, t: usize, id: EntityId) -> EntityView {
        self.state[t][id.index() as usize]
    }

    fn hull(&self, t: usize, id: EntityId) -> u8 {
        self.at(t, id).facing.raw()
    }

    fn turret(&self, t: usize, id: EntityId) -> u8 {
        self.at(t, id).turret.expect("a turret").facing.raw()
    }

    /// Whether the entity moved during tick `t - 1` (between `state[t - 1]` and `state[t]`).
    fn moved(&self, t: usize, id: EntityId) -> bool {
        self.at(t, id).pos != self.at(t - 1, id).pos
    }

    /// The facing that points from `from` to `to` at time `t`.
    fn aim(&self, t: usize, from: EntityId, to: EntityId) -> u8 {
        Facing::from_vec2(self.at(t, to).pos - self.at(t, from).pos)
            .unwrap()
            .raw()
    }
}

fn mid(x: i32, y: i32) -> Vec2 {
    Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF)
}

/// The size of the shortest turn from `a` to `b`, 0 to 128.
fn apart(a: u8, b: u8) -> u8 {
    Facing::new(a).abs_diff(Facing::new(b))
}

#[test]
fn the_scenario_is_the_cast_it_says() {
    let r = run();
    assert_eq!(r.state[1].len(), 7);
    let kinds: Vec<u16> = r.state[1].iter().map(|v| v.kind.0).collect();
    assert_eq!(kinds, vec![19, 24, 12, 20, 24, 19, 12]);
    // Only the tanks have a turret, and only the tanks' turret rate is in the table.
    let turrets: Vec<bool> = r.state[1].iter().map(|v| v.turret.is_some()).collect();
    assert_eq!(turrets, vec![true, false, false, true, false, true, false]);
    let rate = |id| r.world.stats_of(id).unwrap().turret_per_tick;
    assert_eq!((rate(T1), rate(T2), rate(T3)), (12, 8, 12));
    assert_eq!(r.world.stats_of(T1).unwrap().turn_per_tick, 10);
    // The profile of each class (the frozen definitions).
    let mode = |id| {
        let class = r.world.stats_of(id).unwrap().movement_class.unwrap();
        r.world.movement_table().turning(class).mode
    };
    use sf_sim::TurnMode::{Arc, Instant, Pivot};
    assert_eq!((mode(T1), mode(W1), mode(I1)), (Pivot, Arc, Instant));
}

#[test]
fn all_three_are_back_where_they_were_sent_and_face_east_when_the_orders_come() {
    let r = run();
    for (id, at) in [(T1, mid(24, 8)), (W1, mid(24, 16)), (I1, mid(14, 24))] {
        let v = r.at(BACK, id);
        assert_eq!(r.world.order_of(id), Some(Order::Idle), "{id:?}");
        assert!(
            v.pos.distance(at) <= ARRIVE_TOLERANCE,
            "{id:?} at {:?}",
            v.pos
        );
        assert_eq!(v.facing.raw(), 0, "{id:?} faces east, the way it came");
    }
}

#[test]
fn a_tank_ordered_behind_itself_pivots_in_place_and_then_drives() {
    let r = run();
    // Facing 0 (east), the order is to go west: exactly half a turn. The tie goes the
    // decreasing way, and the tank turns ten steps a tick (its turn rate).
    assert_eq!(r.hull(BACK, T1), 0);
    let turned: Vec<u8> = (BACK + 1..).take(13).map(|t| r.hull(t, T1)).collect();
    assert_eq!(
        turned,
        vec![
            246, 236, 226, 216, 206, 196, 186, 176, 166, 156, 146, 136, 128
        ],
        "ten steps a tick the decreasing way, stopping on 128"
    );
    // It drives only once it is within the tolerance (16 steps) of the heading it
    // wants: not while it is 128, 118, ... 18 steps off, from the first tick it is 8 off.
    let first_drive = (BACK + 1..BACK + 40)
        .find(|&t| r.moved(t, T1))
        .expect("it drives");
    assert_eq!(
        first_drive,
        BACK + 12,
        "after twelve ticks of turning on the spot"
    );
    for t in BACK + 1..first_drive {
        assert_eq!(
            r.at(t, T1).pos,
            r.at(BACK, T1).pos,
            "tick {t}: still on the spot"
        );
    }
    // Then it drives straight west at its speed.
    let step = r.world.stats_of(T1).unwrap().speed_per_tick;
    for t in first_drive + 1..first_drive + 60 {
        assert_eq!(r.hull(t, T1), 128);
        let (a, b) = (r.at(t - 1, T1).pos, r.at(t, T1).pos);
        assert_eq!(a.x - b.x, step, "tick {t}");
    }
    // And it gets there.
    assert_eq!(r.world.order_of(T1), Some(Order::Idle));
    assert!(r.at(1000, T1).pos.distance(mid(6, 8)) <= ARRIVE_TOLERANCE);
}

#[test]
fn a_wheeled_vehicle_has_to_move_to_turn_and_swings_round_in_an_arc() {
    let r = run();
    let profile = {
        let class = r.world.stats_of(W1).unwrap().movement_class.unwrap();
        r.world.movement_table().turning(class)
    };
    let step = r.world.stats_of(W1).unwrap().speed_per_tick;
    // The order is to a spot behind it and to the side: it does not turn on the spot, it
    // drives off on the very next tick and turns while it drives.
    let start = r.at(BACK, W1).pos;
    let swing: Vec<usize> = (BACK + 1..BACK + 37).collect();
    for &t in &swing {
        assert!(
            r.moved(t, W1),
            "tick {t}: a vehicle that has to move to turn keeps moving"
        );
    }
    // It turns by what a circle of its minimum radius allows for the distance it covers
    // and never at its turn rate: at most 4 steps a tick here (0.125 cells a tick, 1.5
    // cells of radius: 3.4), and at least 3 while it is far off its heading.
    let rate = r.world.stats_of(W1).unwrap().turn_per_tick;
    let allowed = (step.raw() * 40_744 / profile.min_radius.raw()) / 1000 + 1;
    let mut seen_most = 0;
    for &t in &swing[..30] {
        let turn = apart(r.hull(t - 1, W1), r.hull(t, W1));
        assert!(
            i64::from(turn) <= allowed,
            "tick {t}: turned {turn}, the circle allows {allowed}"
        );
        assert!(turn >= 3 && turn < rate, "tick {t}: turned {turn} steps");
        seen_most = seen_most.max(turn);
    }
    assert!(seen_most >= 3);
    // The swing carries it on east before it comes round: it goes beyond where it stood
    // by more than half its radius, which a unit that pivots would not.
    let farthest_east = swing.iter().map(|&t| r.at(t, W1).pos.x).max().unwrap();
    assert!(
        farthest_east - start.x > profile.min_radius.div_int(2),
        "went {:?} cells on east",
        farthest_east - start.x
    );
    // A half turn takes a half circle: its heading has come round (all but the last few
    // steps) after the arc, and the path it drove is the arc, not a straight line.
    let t = swing[swing.len() - 1];
    assert!(
        apart(r.hull(t, W1), 128) <= 16,
        "heading {} after the swing",
        r.hull(t, W1)
    );
    // It arrives.
    assert_eq!(r.world.order_of(W1), Some(Order::Idle));
    assert!(r.at(1000, W1).pos.distance(mid(14, 12)) <= ARRIVE_TOLERANCE);
}

#[test]
fn a_rifleman_turns_at_once() {
    let r = run();
    // Spawned with a heading of its own (63, from its random stream), it faces east on
    // the first tick it is ordered east and steps off on that very tick.
    assert_ne!(r.hull(1, I1), 0);
    assert_eq!(r.hull(2, I1), 0);
    assert!(r.moved(2, I1));
    // Ordered behind itself (exactly half a turn) it faces west at once and steps off.
    assert_eq!(r.hull(BACK, I1), 0);
    assert_eq!(r.hull(BACK + 1, I1), 128);
    assert!(r.moved(BACK + 1, I1));
    // And on to its goal.
    assert!(r.at(1000, I1).pos.distance(mid(8, 24)) <= ARRIVE_TOLERANCE);
}

#[test]
fn a_turret_follows_a_moving_target_while_the_hull_drives_elsewhere() {
    let r = run();
    let rate = r.world.stats_of(T2).unwrap().turret_per_tick;
    assert_eq!(rate, 8);
    // The hull drives east along row 36 from the moment it has pivoted, and never
    // turns while the turret sweeps.
    for t in 40..LET_GO {
        assert_eq!(r.hull(t, T2), 0, "tick {t}");
        assert!(r.moved(t, T2));
    }
    // The turret turns at most its rate in a tick, whatever it does...
    for t in 2..=1000 {
        let before = r.turret(t - 1, T2);
        assert!(apart(before, r.turret(t, T2)) <= rate, "tick {t}");
        // ...and the view carries the previous tick's facing for the renderer.
        assert_eq!(
            r.at(t, T2).turret.unwrap().prev_facing.raw(),
            before,
            "tick {t}"
        );
    }
    // It has found the target within the 16 ticks a half turn takes, and keeps on it,
    // at most two ticks of its own rate behind, while the target drives past.
    let mut widest = 0;
    for t in 20..LET_GO {
        let off = apart(r.turret(t, T2), r.aim(t, T2, W2));
        assert!(
            off <= 2 * rate,
            "tick {t}: the turret is {off} steps off the target"
        );
        widest = widest.max(apart(r.turret(t, T2), r.hull(t, T2)));
    }
    // The target passes from ahead-and-left of the tank to behind-and-left: the turret
    // sweeps a quarter turn and more away from the hull.
    assert!(widest >= 100, "the turret got {widest} steps off the hull");
    // Let go at tick 600, the turret comes back to hull-forward the shortest way (118
    // steps of the 142 it was off, in 8-step strides) and stays there.
    let off = apart(r.turret(LET_GO, T2), r.hull(LET_GO, T2));
    assert!(off > 100);
    let back = LET_GO + usize::from(off).div_ceil(usize::from(rate)) + 1;
    for t in back..=800 {
        assert_eq!(r.turret(t, T2), r.hull(t, T2), "tick {t}");
    }
    assert_ne!(r.turret(back - 3, T2), r.hull(back - 3, T2));
}

#[test]
fn the_turret_of_an_attacker_aims_at_the_victim_ahead_of_the_hull() {
    let r = run();
    let aim = |t| r.aim(t, T3, V);
    // The tank is ordered on tick 1 and its hull and turret both begin to turn toward
    // the victim; the turret (12 steps a tick) is on it before the hull (10 a tick).
    let first = |f: &dyn Fn(usize) -> u8| (2..60).find(|&t| apart(f(t), aim(t)) <= 1).unwrap();
    let turret_on = first(&|t| r.turret(t, T3));
    let hull_on = first(&|t| r.hull(t, T3));
    assert!(
        turret_on < hull_on,
        "turret on at tick {turret_on}, hull at {hull_on}"
    );
    // It drives up to its weapon reach (5 cells and the victim's radius) and stops.
    assert_eq!(r.world.order_of(T3), Some(Order::Attack(V)));
    let reach = r.world.stats_of(T3).unwrap().weapon_range + Fx::from_ratio(1, 5);
    let gap = r.at(1000, T3).pos.distance(r.at(1000, V).pos);
    assert!(
        gap <= reach && gap + Fx::from_ratio(1, 2) > reach,
        "{gap:?} from the victim"
    );
    // The turret is on the victim at the end, and the victim has not moved.
    assert!(apart(r.turret(1000, T3), aim(1000)) <= 1);
    assert_eq!(r.at(1000, V).pos, r.at(1, V).pos);
}

#[test]
fn a_tank_that_stands_still_keeps_its_turret_on_the_hull() {
    let r = run();
    // T1 never aims at anything: its turret is the hull's facing, except for the
    // few ticks a faster turret needs to catch up (here none: 12 against 10 steps).
    for t in 1..=1000 {
        assert_eq!(r.turret(t, T1), r.hull(t, T1), "tick {t}");
    }
}

#[test]
fn the_run_is_reproducible() {
    let (a, b) = (run(), run());
    assert_eq!(a.world.checksum_history(), b.world.checksum_history());
}
