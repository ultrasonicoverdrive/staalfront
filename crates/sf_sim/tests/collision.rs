//! Collisions and local avoidance (WP4.4), through the public API: the radius of
//! a body, the distance bodies keep, following and head-on passing, a unit
//! holding position that is never pushed, an idle friend that is, the bounds of
//! the push, the terrain and structures that stop it, a stuck unit and the
//! request for a new route, and the proof that the result does not depend on
//! the order the units were made in. The rules are in `docs/rules/collision.md`.

use sf_math::{Fx, Vec2};
use sf_sim::{
    Cell, CollisionConfig, Command, ConfigError, Cost, EntityId, Footprint, FormationConfig, Kind,
    MovementTable, Order, PlayerId, Terrain, TerrainDef, TerrainFlags, TerrainId, UnitStats,
    UnitTable, World, WorldConfig, per_tick,
};

const GRASS: TerrainId = TerrainId(0);
const ROCK: TerrainId = TerrainId(1);

// Movement classes (permanent kind numbers).
const WALKER: u16 = 0;
const TRACKED: u16 = 1;
const FLYER: u16 = 2;

// Unit kinds.
const RIFLE: Kind = Kind(1); // 0.4 wide, 1.4 cells a second
const TANK: Kind = Kind(2); // 1.2 wide, 2.4 cells a second
const SLOW: Kind = Kind(3); // 0.4 wide, 0.7 cells a second
const PLANE: Kind = Kind(4); // 1.0 wide, flies
const HUT: Kind = Kind(5); // a structure, 2 by 2
const BIG: Kind = Kind(6); // 1.5 wide, 1.2 cells a second
const SPRINT: Kind = Kind(7); // 0.4 wide, 4 cells a second

const MAP: (u32, u32) = (64, 48);

fn table() -> UnitTable {
    let mobile = |kind: Kind, class: u16, speed_tenths: i64, size_tenths: i64| UnitStats {
        kind,
        hp: 100,
        movement_class: Some(class),
        speed_per_tick: per_tick(Fx::from_ratio(speed_tenths, 10)),
        // A unit is lined up at once, so what a test sees is the collision and
        // not the turning.
        turn_per_tick: 255,
        size: Fx::from_ratio(size_tenths, 10),
        footprint: None,
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    };
    UnitTable::new(vec![
        mobile(RIFLE, WALKER, 14, 4),
        mobile(TANK, TRACKED, 24, 12),
        mobile(SLOW, WALKER, 7, 4),
        mobile(PLANE, FLYER, 40, 10),
        UnitStats {
            kind: HUT,
            hp: 100,
            movement_class: None,
            speed_per_tick: Fx::ZERO,
            turn_per_tick: 0,
            size: Fx::ZERO,
            footprint: Some(Footprint {
                width: 2,
                height: 2,
            }),
            weapon_range: Fx::ZERO,
            turret_per_tick: 0,
        },
        mobile(BIG, TRACKED, 12, 15),
        mobile(SPRINT, WALKER, 40, 4),
    ])
    .unwrap()
}

fn costs() -> MovementTable {
    use sf_sim::ClassCosts;
    let class = |class, list: [Cost; 2]| ClassCosts {
        class,
        costs: [GRASS, ROCK].into_iter().zip(list).collect(),
    };
    let x = Cost::Impassable;
    let s = Cost::Speed;
    MovementTable::new(vec![
        class(WALKER, [s(1000), x]),
        class(TRACKED, [s(1000), x]),
        class(FLYER, [s(1000), s(1000)]),
    ])
    .unwrap()
}

fn config() -> WorldConfig {
    WorldConfig::new(5).with_map_size(MAP.0, MAP.1)
}

/// Open ground, no terrain layer (structures do not block there).
fn open_world() -> World {
    World::new(config())
        .unwrap()
        .with_units(table())
        .unwrap()
        .with_movement(costs())
        .unwrap()
}

/// A world without a terrain layer and with the given collision settings.
fn world_with(collision: CollisionConfig) -> World {
    World::new(config())
        .unwrap()
        .with_units(table())
        .unwrap()
        .with_movement(costs())
        .unwrap()
        .with_collision(collision)
        .unwrap()
}

/// Grass with rock where `rock(x, y)` says so.
fn rock_world(rock: impl Fn(u32, u32) -> bool) -> World {
    let layer: Vec<u8> = (0..MAP.1)
        .flat_map(|y| (0..MAP.0).map(move |x| (x, y)))
        .map(|(x, y)| if rock(x, y) { ROCK.0 } else { GRASS.0 })
        .collect();
    let defs = vec![
        TerrainDef {
            id: GRASS,
            flags: TerrainFlags {
                buildable: true,
                ..TerrainFlags::OPEN
            },
        },
        TerrainDef {
            id: ROCK,
            flags: TerrainFlags::default(),
        },
    ];
    let terrain = Terrain::from_layer(MAP.0, MAP.1, &layer, &defs).unwrap();
    World::with_terrain(config(), terrain)
        .unwrap()
        .with_units(table())
        .unwrap()
        .with_movement(costs())
        .unwrap()
}

/// A fraction of a cell.
fn fx(num: i64, den: i64) -> Fx {
    Fx::from_ratio(num, den)
}

/// A point from thousandths of a cell.
fn at(x: i64, y: i64) -> Vec2 {
    Vec2::new(fx(x, 1000), fx(y, 1000))
}

/// The middle of a cell.
fn cells(x: i32, y: i32) -> Vec2 {
    Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF)
}

fn spawn(w: &mut World, kind: Kind, owner: u8, pos: Vec2) -> EntityId {
    let id = EntityId::new(w.slot_count() as u32, 0);
    let tick = w.tick();
    w.schedule(
        tick,
        PlayerId(0),
        Command::SpawnUnit {
            kind,
            pos,
            owner: PlayerId(owner),
        },
    )
    .unwrap();
    w.step();
    assert!(w.is_alive(id), "the unit was refused at {pos:?}");
    id
}

fn command(w: &mut World, c: Command) {
    let tick = w.tick();
    w.schedule(tick, PlayerId(0), c).unwrap();
}

fn go(w: &mut World, ids: &[EntityId], to: Vec2) {
    command(
        w,
        Command::Order {
            ids: ids.to_vec(),
            order: Order::Move(to),
            queued: false,
        },
    );
}

fn hold(w: &mut World, ids: &[EntityId]) {
    command(
        w,
        Command::Order {
            ids: ids.to_vec(),
            order: Order::HoldPosition,
            queued: false,
        },
    );
}

fn run(w: &mut World, ticks: u64) {
    for _ in 0..ticks {
        w.step();
    }
}

fn pos(w: &World, id: EntityId) -> Vec2 {
    w.get(id).unwrap().pos
}

/// The smallest distance the bodies of two units keep (the rule of
/// `docs/rules/collision.md`): the sum of the radii, three quarters of it for two
/// small ones.
fn keep(w: &World, a: EntityId, b: EntityId) -> Fx {
    let (ra, rb) = (
        w.collision_radius_of(a).unwrap(),
        w.collision_radius_of(b).unwrap(),
    );
    let small = |r: Fx| r <= fx(3, 8);
    if small(ra) && small(rb) {
        (ra + rb).mul_int(3).div_int(4)
    } else {
        ra + rb
    }
}

/// The worst overlap of any pair among `ids` right now, in cells (zero when
/// nobody overlaps).
fn worst_overlap(w: &World, ids: &[EntityId]) -> Fx {
    let mut worst = Fx::ZERO;
    for (k, &a) in ids.iter().enumerate() {
        for &b in &ids[k + 1..] {
            let d = (pos(w, a) - pos(w, b)).length();
            worst = worst.max(keep(w, a, b) - d);
        }
    }
    worst
}

/// The slack bodies are allowed before they are pushed apart, a thirty-second.
const SLACK: Fx = Fx::from_ratio(1, 32);

// --- the body ---

#[test]
fn a_body_is_half_the_size_of_the_definition_and_aircraft_and_structures_have_none() {
    let mut w = open_world();
    let rifle = spawn(&mut w, RIFLE, 0, cells(5, 5));
    let tank = spawn(&mut w, TANK, 0, cells(8, 5));
    let big = spawn(&mut w, BIG, 0, cells(11, 5));
    let plane = spawn(&mut w, PLANE, 0, cells(14, 5));
    assert_eq!(w.collision_radius_of(rifle), Some(fx(1, 5)));
    assert_eq!(w.collision_radius_of(tank), Some(fx(3, 5)));
    assert_eq!(w.collision_radius_of(big), Some(fx(3, 4)));
    // Aircraft fly over everything: no body.
    assert_eq!(w.collision_radius_of(plane), None);
    // A stale id has none either.
    assert_eq!(w.collision_radius_of(EntityId::new(99, 0)), None);
}

#[test]
fn the_settings_are_checked_and_given_before_the_world_starts() {
    let bad = CollisionConfig {
        wait_ticks: 0,
        ..CollisionConfig::DEFAULT
    };
    assert_eq!(
        World::new(config()).unwrap().with_collision(bad).err(),
        Some(ConfigError::Collision)
    );
    let stuck_before_wait = CollisionConfig {
        wait_ticks: 10,
        stuck_ticks: 10,
        ..CollisionConfig::DEFAULT
    };
    assert!(
        World::new(config())
            .unwrap()
            .with_collision(stuck_before_wait)
            .is_err()
    );
    let mut w = open_world();
    spawn(&mut w, RIFLE, 0, cells(5, 5));
    assert_eq!(
        w.with_collision(CollisionConfig::DEFAULT).err(),
        Some(ConfigError::Collision),
        "the world has started"
    );
    let off = CollisionConfig {
        enabled: false,
        ..CollisionConfig::DEFAULT
    };
    let w = World::new(config()).unwrap().with_collision(off).unwrap();
    assert!(!w.collision_config().enabled);
    assert_eq!(
        World::new(config()).unwrap().collision_config(),
        CollisionConfig::DEFAULT
    );
}

#[test]
fn infantry_stands_closer_than_vehicles_and_the_distance_is_kept() {
    // Two units walk side by side, `gap` thousandths apart, to points as far apart.
    let run_pair = |a: Kind, b: Kind, gap: i64| {
        let mut w = open_world();
        let (ua, ub) = (
            spawn(&mut w, a, 0, at(10_000, 20_000)),
            spawn(&mut w, b, 0, at(10_000, 20_000 + gap)),
        );
        go(&mut w, &[ua], at(30_000, 20_000));
        go(&mut w, &[ub], at(30_000, 20_000 + gap));
        run(&mut w, 40);
        ((pos(&w, ua) - pos(&w, ub)).length(), keep(&w, ua, ub))
    };
    // Two riflemen keep 0.3: at 0.35 nobody moves aside.
    let (d, k) = run_pair(RIFLE, RIFLE, 350);
    assert!((k - fx(3, 10)).abs() < fx(1, 1000), "{k:?}");
    assert!((d - fx(35, 100)).abs() < fx(1, 100), "{d:?}");
    // At 0.2 they are pushed to 0.3 (within the slack of a thirty-second).
    let (d, k) = run_pair(RIFLE, RIFLE, 200);
    assert!(d >= k - SLACK - fx(1, 500), "{d:?} {k:?}");
    assert!(d < k + fx(1, 10), "not flung apart: {d:?}");
    // A tank and a rifleman keep 0.8, a full sum, not the softer one.
    let (d, k) = run_pair(TANK, RIFLE, 500);
    assert!((k - fx(4, 5)).abs() < fx(1, 1000), "{k:?}");
    assert!(d >= k - SLACK - fx(1, 500), "{d:?}");
    // Two tanks keep 1.2.
    let (d, k) = run_pair(TANK, TANK, 600);
    assert!((k - fx(6, 5)).abs() < fx(1, 1000), "{k:?}");
    assert!(d >= k - SLACK - fx(1, 500), "{d:?}");
}

#[test]
fn a_crowd_walking_to_one_point_never_overlaps_beyond_the_slack_and_everyone_arrives() {
    let mut w = open_world();
    let mut ids = Vec::new();
    for k in 0..24 {
        let kind = [RIFLE, TANK, SLOW, BIG][k % 4];
        ids.push(spawn(
            &mut w,
            kind,
            0,
            cells(4 + (k % 6) as i32 * 2, 10 + (k / 6) as i32 * 3),
        ));
    }
    go(&mut w, &ids, cells(50, 24));
    let mut worst = Fx::ZERO;
    for _ in 0..2400 {
        w.step();
        let now = worst_overlap(&w, &ids);
        worst = worst.max(now);
    }
    for &id in &ids {
        assert_eq!(w.order_of(id), Some(Order::Idle), "{id:?} arrived");
        assert!(
            (pos(&w, id) - cells(50, 24)).length() < Fx::from_int(8),
            "{id:?} ended at {:?}",
            pos(&w, id)
        );
    }
    // The slack is a thirty-second; a packed crowd is a little over it at the worst
    // moment and settles. Never over the quarter of a cell the phase allows.
    assert!(worst < fx(1, 4), "worst overlap {worst:?}");
    assert!(
        worst_overlap(&w, &ids) < fx(1, 20),
        "at rest the crowd has settled"
    );
}

// --- following, passing, holding ---

#[test]
fn units_moving_the_same_way_follow_and_the_one_in_front_is_not_pushed_on() {
    // A fast tank comes up behind a slow rifleman on the same line.
    let line = |with_tank: bool| {
        let mut w = open_world();
        let rifle = spawn(&mut w, SLOW, 0, cells(10, 20));
        let tank = with_tank.then(|| spawn(&mut w, TANK, 0, cells(4, 20)));
        go(&mut w, &[rifle], cells(50, 20));
        if let Some(t) = tank {
            go(&mut w, &[t], cells(60, 20));
        }
        run(&mut w, 200);
        (pos(&w, rifle), tank.map(|t| pos(&w, t)))
    };
    let (alone, _) = line(false);
    let (with, tank) = line(true);
    let tank = tank.unwrap();
    // The rifleman walked exactly as if the tank were not there.
    assert_eq!(with, alone, "the leader is not pushed");
    // And the tank is behind it, a body's width back, at the rifleman's pace.
    let gap = with.x - tank.x;
    assert!(
        gap > fx(7, 10) && gap < Fx::ONE,
        "the tank follows at {gap:?}"
    );
    assert!((tank.y - with.y).abs() < fx(1, 5), "on the same line");
}

#[test]
fn units_that_meet_head_on_pass_each_on_its_own_right() {
    let mut w = open_world();
    let east = spawn(&mut w, TANK, 0, cells(10, 20));
    let west = spawn(&mut w, TANK, 0, cells(40, 20));
    go(&mut w, &[east], cells(40, 20));
    go(&mut w, &[west], cells(10, 20));
    let (mut east_y, mut west_y) = (Fx::ZERO, Fx::ZERO);
    let mut worst = Fx::ZERO;
    let (y0, mut passed) = (cells(10, 20).y, false);
    for _ in 0..600 {
        w.step();
        east_y = east_y.max(pos(&w, east).y - y0);
        west_y = west_y.min(pos(&w, west).y - y0);
        worst = worst.max(worst_overlap(&w, &[east, west]));
        passed |= pos(&w, east).x > pos(&w, west).x;
    }
    assert!(passed, "they got by each other");
    // Going east, the right hand is south (y grows downward): the eastbound tank
    // swung to higher y, the westbound to lower y. They did not pick the same
    // side.
    assert!(east_y > fx(3, 10), "eastbound swung {east_y:?}");
    assert!(west_y < fx(-3, 10), "westbound swung {west_y:?}");
    assert!(worst < fx(1, 5), "{worst:?}");
    assert_eq!(w.order_of(east), Some(Order::Idle));
    assert_eq!(w.order_of(west), Some(Order::Idle));
}

#[test]
fn the_same_meeting_with_the_roles_exchanged_gives_the_same_result() {
    let meet = |first: (i32, i32), second: (i32, i32)| {
        let mut w = open_world();
        let a = spawn(&mut w, TANK, 0, cells(first.0, first.1));
        let b = spawn(&mut w, TANK, 0, cells(second.0, second.1));
        go(&mut w, &[a], cells(second.0, second.1));
        go(&mut w, &[b], cells(first.0, first.1));
        run(&mut w, 600);
        (pos(&w, a), pos(&w, b))
    };
    // The unit that was a is now the one coming from the east.
    let (a1, b1) = meet((10, 20), (40, 20));
    let (b2, a2) = meet((40, 20), (10, 20));
    assert_eq!((a1, b1), (a2, b2));
}

#[test]
fn a_unit_holding_position_is_never_pushed_and_the_others_get_round_it() {
    let mut w = open_world();
    let post = cells(25, 20);
    let holder = spawn(&mut w, TANK, 0, post);
    hold(&mut w, &[holder]);
    let mut column = Vec::new();
    for k in 0..6 {
        column.push(spawn(&mut w, TANK, 0, cells(8, 18 + k)));
    }
    go(&mut w, &column, cells(45, 20));
    let mut worst = Fx::ZERO;
    let mut near = vec![holder];
    near.extend(&column);
    for _ in 0..1500 {
        w.step();
        assert_eq!(pos(&w, holder), post, "the holder never moves");
        worst = worst.max(worst_overlap(&w, &near));
    }
    assert_eq!(w.order_of(holder), Some(Order::HoldPosition));
    for &id in &column {
        assert_eq!(w.order_of(id), Some(Order::Idle), "{id:?} got through");
    }
    assert!(worst < fx(3, 10), "{worst:?}");
}

#[test]
fn a_unit_blocked_by_one_in_the_way_waits_then_detours_and_needs_no_sidestep() {
    let mut w = open_world();
    let holder = spawn(&mut w, BIG, 0, cells(25, 20));
    hold(&mut w, &[holder]);
    // Dead in line with it: nothing slides it off to either side.
    let mover = spawn(&mut w, TANK, 0, cells(10, 20));
    go(&mut w, &[mover], cells(40, 20));
    let mut waited = 0;
    for _ in 0..800 {
        w.step();
        waited = waited.max(w.stuck_ticks_of(mover).unwrap());
        if w.order_of(mover) == Some(Order::Idle) {
            break;
        }
    }
    assert_eq!(w.order_of(mover), Some(Order::Idle), "it got round");
    assert!(pos(&w, mover).x > Fx::from_int(39));
    assert_eq!(pos(&w, holder), cells(25, 20));
    // It waited the wait (six ticks as shipped) and then went round to its right
    // without ever being called stuck.
    let cfg = w.collision_config();
    assert!(waited >= u32::from(cfg.wait_ticks), "waited {waited}");
    assert_eq!(w.crowd_stats().sidesteps, 0);
    assert_eq!(w.replan_requested(mover), Some(false));
}

#[test]
fn an_idle_friend_is_pushed_aside_by_a_mover_but_an_enemy_is_not() {
    let mut w = open_world();
    let friend = spawn(&mut w, TANK, 0, cells(25, 20));
    let enemy = spawn(&mut w, TANK, 1, cells(25, 30));
    let m1 = spawn(&mut w, TANK, 0, cells(10, 20));
    let m2 = spawn(&mut w, TANK, 0, cells(10, 30));
    go(&mut w, &[m1], cells(40, 20));
    go(&mut w, &[m2], cells(40, 30));
    let (friend_start, enemy_start) = (pos(&w, friend), pos(&w, enemy));
    let mut away = Fx::ZERO;
    for _ in 0..900 {
        w.step();
        away = away.max((pos(&w, friend) - friend_start).length());
        assert_eq!(pos(&w, enemy), enemy_start, "an enemy is never pushed");
    }
    assert!(away > fx(3, 10), "the friend stepped aside by {away:?}");
    assert_eq!(w.order_of(m1), Some(Order::Idle));
    assert_eq!(w.order_of(m2), Some(Order::Idle));
    assert_eq!(
        w.order_of(friend),
        Some(Order::Idle),
        "its order is untouched"
    );
}

#[test]
fn the_push_is_bounded_in_a_tick_and_in_all() {
    let mut w = open_world();
    let friend = spawn(&mut w, TANK, 0, cells(25, 20));
    // A column of tanks one body apart, straight at it: one reaches it every few
    // ticks, so it is never left alone long enough for its allowance to come back.
    let mut column = Vec::new();
    for k in 0..8_i64 {
        column.push(spawn(&mut w, TANK, 0, at(12_000 - 1_300 * k, 20_500)));
    }
    go(&mut w, &column, cells(50, 20));
    let start = pos(&w, friend);
    let mut last = start;
    let mut farthest = Fx::ZERO;
    for _ in 0..600 {
        w.step();
        let now = pos(&w, friend);
        let step = (now - last).length();
        assert!(step <= fx(1, 8) + fx(1, 100), "pushed {step:?} in one tick");
        last = now;
        farthest = farthest.max((now - start).length());
    }
    // Three cells of push in all, while it is not left alone (SHOVE_LIMIT): the
    // later tanks of the column go round it.
    assert!(farthest > Fx::ONE, "it was pushed: {farthest:?}");
    assert!(farthest < fx(31, 10), "pushed as far as {farthest:?}");
    for &id in &column {
        assert_eq!(w.order_of(id), Some(Order::Idle), "the column got through");
    }
}

// --- terrain and structures stop a push ---

#[test]
fn an_idle_unit_is_not_pushed_into_rock_and_the_pusher_stops_short_of_it() {
    // A wall of rock from column 30 on.
    let mut w = rock_world(|x, _| x >= 30);
    let friend = spawn(&mut w, TANK, 0, at(29_200, 20_500));
    let mover = spawn(&mut w, TANK, 0, cells(10, 20));
    // It is told to go to the friend's own spot, which is a body away from where
    // it can be.
    let friend_at = pos(&w, friend);
    go(&mut w, &[mover], friend_at);
    for _ in 0..600 {
        w.step();
        let cell = w.cell_at(pos(&w, friend)).unwrap();
        assert!(
            w.passable(TRACKED, cell),
            "the friend is in rock at {cell:?}"
        );
        assert!(pos(&w, friend).x < Fx::from_int(30));
    }
    // The friend gave way as far as the terrain let it, the mover stopped at a
    // body's distance (it counts as arrived there, crowded).
    let d = (pos(&w, friend) - pos(&w, mover)).length();
    assert!(d >= fx(6, 5) - SLACK - fx(1, 100), "{d:?}");
    assert_eq!(w.order_of(mover), Some(Order::Idle));
}

#[test]
fn a_structure_stops_a_push_like_rock_does() {
    let mut w = rock_world(|_, _| false);
    command(
        &mut w,
        Command::PlaceStructure {
            kind: HUT,
            cell: Cell::new(31, 19),
            owner: PlayerId(0),
        },
    );
    w.step();
    assert!(w.is_occupied(Cell::new(31, 19)));
    let friend = spawn(&mut w, TANK, 0, at(30_400, 20_000));
    let mover = spawn(&mut w, TANK, 0, cells(10, 20));
    let friend_at = pos(&w, friend);
    go(&mut w, &[mover], friend_at);
    for _ in 0..600 {
        w.step();
        let cell = w.cell_at(pos(&w, friend)).unwrap();
        assert!(!w.is_occupied(cell), "the friend is inside the structure");
        let cell = w.cell_at(pos(&w, mover)).unwrap();
        assert!(!w.is_occupied(cell), "the mover is inside the structure");
    }
}

// --- stuck ---

#[test]
fn a_stuck_unit_steps_to_a_free_neighbour_and_then_asks_for_a_new_route() {
    let mut w = world_with(CollisionConfig {
        enabled: true,
        wait_ticks: 2,
        stuck_ticks: 8,
        stuck_attempts: 2,
    });
    // A wall of three big holders across the way, with no gap.
    let wall: Vec<EntityId> = (0..3)
        .map(|k| spawn(&mut w, BIG, 0, at(30_000, 18_000 + k * 1_550)))
        .collect();
    hold(&mut w, &wall);
    let starts: Vec<Vec2> = wall.iter().map(|&id| pos(&w, id)).collect();
    // The mover has a wall of holders straight ahead and room all round it behind.
    let mover = spawn(&mut w, RIFLE, 0, cells(20, 20));
    go(&mut w, &[mover], cells(40, 20));
    let mut asked = false;
    for _ in 0..400 {
        w.step();
        for (&id, &p) in wall.iter().zip(&starts) {
            assert_eq!(pos(&w, id), p, "holders do not move");
        }
        asked |= w.replan_requested(mover) == Some(true);
    }
    let stats = w.crowd_stats();
    assert!(stats.sidesteps >= 2, "{stats:?}");
    assert!(stats.replans >= 1, "{stats:?}");
    assert!(asked, "the request is marked on the unit");
    // The order is kept: nothing ended it.
    assert_eq!(w.order_of(mover), Some(Order::Move(cells(40, 20))));
}

// --- the result does not depend on the order the units were made in ---

/// Positions at the end of `ticks` ticks for the crowd made in `order` (a
/// permutation of indices into the crowd), listed by the label the unit was given.
/// Nobody starts on the same point, and nobody is routed (open ground), so nothing
/// but a slot number could tell two runs apart.
fn crowd_run(order: &[usize], ticks: u64) -> Vec<(usize, Vec2)> {
    let crowd: Vec<(Kind, u8, Vec2, Vec2)> = (0..28)
        .map(|k| {
            let kind = [RIFLE, TANK, SLOW, BIG, SPRINT][k % 5];
            let x = 4 + (k % 7) as i64 * 2;
            let y = 12 + (k / 7) as i64 * 4;
            // Two sides, so that some of them are friends and some enemies, and
            // some go against the stream.
            let to = if k % 3 == 0 {
                cells(8, 30)
            } else {
                cells(44, 24)
            };
            (
                kind,
                (k % 2) as u8,
                at(x * 1000 + 500 + k as i64 * 10, y * 1000 + 500),
                to,
            )
        })
        .collect();
    let mut w = open_world();
    let mut ids = vec![EntityId::new(0, 0); crowd.len()];
    for &label in order {
        let (kind, owner, start, _) = crowd[label];
        ids[label] = spawn(&mut w, kind, owner, start);
    }
    for (label, &id) in ids.iter().enumerate() {
        go(&mut w, &[id], crowd[label].3);
    }
    run(&mut w, ticks);
    (0..crowd.len())
        .map(|label| (label, pos(&w, ids[label])))
        .collect()
}

#[test]
fn the_outcome_does_not_depend_on_the_order_the_units_were_spawned_in() {
    let n = 28;
    let forward: Vec<usize> = (0..n).collect();
    let reversed: Vec<usize> = (0..n).rev().collect();
    // A fixed shuffle: the multiplication by a number coprime to 28 is a
    // permutation.
    let shuffled: Vec<usize> = (0..n).map(|k| (k * 9 + 5) % n).collect();
    let base = crowd_run(&forward, 500);
    // The crowd did push: somebody is not where a free walk would have put them.
    let free = crowd_run(&forward, 0);
    assert_ne!(base, free);
    assert_eq!(crowd_run(&reversed, 500), base);
    assert_eq!(crowd_run(&shuffled, 500), base);
}

/// One group order (flowfields, arrival slots) over the same units, with the ids
/// listed in the given order: where everybody ends and which slot each was given.
fn group_run(list: &[usize], ticks: u64) -> (Vec<Vec2>, Vec<Option<Vec2>>, u64) {
    let mut w = rock_world(|x, y| x == 41 && (18..=22).contains(&y));
    let ids: Vec<EntityId> = (0..24)
        .map(|k| {
            let kind = [RIFLE, TANK, TANK, SLOW][k % 4];
            spawn(
                &mut w,
                kind,
                0,
                cells(4 + (k % 6) as i32 * 2, 8 + (k / 6) as i32 * 3),
            )
        })
        .collect();
    let listed: Vec<EntityId> = list.iter().map(|&k| ids[k]).collect();
    go(&mut w, &listed, cells(40, 20));
    // The slots are read while the units are on their way (a unit that has arrived
    // no longer follows the field).
    run(&mut w, 60);
    let slots: Vec<Option<Vec2>> = ids.iter().map(|&id| w.arrival_slot_of(id)).collect();
    run(&mut w, ticks - 60);
    (
        ids.iter().map(|&id| pos(&w, id)).collect(),
        slots,
        w.checksum(),
    )
}

#[test]
fn a_group_order_gives_the_same_result_whatever_order_the_ids_are_listed_in() {
    let n = 24;
    let forward: Vec<usize> = (0..n).collect();
    let reversed: Vec<usize> = (0..n).rev().collect();
    // 7 is coprime to 24, so the multiplication is a permutation.
    let shuffled: Vec<usize> = (0..n).map(|k| (k * 7 + 3) % n).collect();
    let base = group_run(&forward, 900);
    // The order really made a group: everybody was given a slot, and they are
    // different slots.
    assert!(base.1.iter().all(Option::is_some), "{:?}", base.1);
    let mut distinct: Vec<Vec2> = base.1.iter().flatten().copied().collect();
    distinct.sort_by_key(|p| (p.x, p.y));
    distinct.dedup();
    assert_eq!(distinct.len(), n);
    assert_eq!(group_run(&reversed, 900), base);
    assert_eq!(group_run(&shuffled, 900), base);
}

#[test]
fn the_same_world_run_twice_gives_the_same_checksums_and_a_clone_goes_on_alike() {
    let build = || {
        let mut w = open_world();
        let ids: Vec<EntityId> = (0..20)
            .map(|k| {
                spawn(
                    &mut w,
                    [RIFLE, TANK][k % 2],
                    0,
                    cells(4 + (k % 5) as i32, 10 + (k / 5) as i32 * 2),
                )
            })
            .collect();
        go(&mut w, &ids, cells(40, 22));
        w
    };
    let (mut a, mut b) = (build(), build());
    run(&mut a, 300);
    run(&mut b, 300);
    assert_eq!(a.checksum_history(), b.checksum_history());
    let mut c = a.clone();
    run(&mut a, 200);
    run(&mut c, 200);
    assert_eq!(a, c);
    assert_eq!(a.checksum(), c.checksum());
}

// --- off, and what it leaves alone ---

#[test]
fn with_collision_off_units_pass_through_each_other() {
    let mut w = world_with(CollisionConfig {
        enabled: false,
        ..CollisionConfig::DEFAULT
    });
    let a = spawn(&mut w, TANK, 0, cells(10, 20));
    let b = spawn(&mut w, TANK, 0, cells(30, 20));
    go(&mut w, &[a], cells(30, 20));
    go(&mut w, &[b], cells(10, 20));
    let mut closest = Fx::MAX;
    for _ in 0..400 {
        w.step();
        closest = closest.min((pos(&w, a) - pos(&w, b)).length());
    }
    assert!(
        closest < fx(1, 5),
        "they went through each other: {closest:?}"
    );
    assert_eq!(w.crowd_stats(), sf_sim::CrowdStats::default());
}

#[test]
fn units_that_never_meet_hash_the_same_with_collision_on_and_off() {
    let run_world = |enabled: bool| {
        let mut w = world_with(CollisionConfig {
            enabled,
            ..CollisionConfig::DEFAULT
        });
        let a = spawn(&mut w, TANK, 0, cells(5, 5));
        let b = spawn(&mut w, RIFLE, 0, cells(5, 30));
        go(&mut w, &[a], cells(55, 5));
        go(&mut w, &[b], cells(55, 30));
        run(&mut w, 300);
        w.checksum_history().to_vec()
    };
    // The crowd section is written only while some unit remembers being in one, so
    // the pass costs a world that never meets one nothing in its checksum.
    let (on, off) = (run_world(true), run_world(false));
    assert_eq!(on, off);
}

#[test]
fn aircraft_and_the_legacy_walks_are_not_bodies() {
    let mut w = open_world();
    let plane = spawn(&mut w, PLANE, 0, cells(10, 20));
    let tank = spawn(&mut w, TANK, 0, cells(30, 20));
    go(&mut w, &[plane], cells(30, 20));
    go(&mut w, &[tank], cells(10, 20));
    let mut closest = Fx::MAX;
    for _ in 0..300 {
        w.step();
        closest = closest.min((pos(&w, plane) - pos(&w, tank)).length());
    }
    assert!(
        closest < fx(3, 10),
        "a plane flies over a tank: {closest:?}"
    );
    // The legacy MoveTo of a table unit is not a body either.
    let mut w = open_world();
    let a = spawn(&mut w, TANK, 0, cells(10, 20));
    let b = spawn(&mut w, TANK, 0, cells(30, 20));
    command(
        &mut w,
        Command::MoveTo {
            ids: vec![a],
            target: cells(30, 20),
        },
    );
    command(
        &mut w,
        Command::MoveTo {
            ids: vec![b],
            target: cells(10, 20),
        },
    );
    let mut closest = Fx::MAX;
    for _ in 0..300 {
        w.step();
        closest = closest.min((pos(&w, a) - pos(&w, b)).length());
    }
    assert!(
        closest < fx(3, 10),
        "legacy walkers pass through: {closest:?}"
    );
}

// --- arrival slots are made for the bodies ---

#[test]
fn the_slots_of_two_classes_in_one_order_are_all_different_and_a_body_apart() {
    // The goal has rock beside it, so that some of the points of the spiral are
    // left out for the classes and the stretches could run into each other.
    let mut w = rock_world(|x, y| (x == 41 && (18..=22).contains(&y)) || (x == 39 && y == 24));
    let mut riflemen = Vec::new();
    let mut tanks = Vec::new();
    for k in 0..16 {
        riflemen.push(spawn(&mut w, RIFLE, 0, cells(4 + k % 8, 10 + k / 8)));
    }
    for k in 0..12 {
        tanks.push(spawn(
            &mut w,
            TANK,
            0,
            cells(4 + (k % 4) * 2, 20 + (k / 4) * 2),
        ));
    }
    let all: Vec<EntityId> = riflemen.iter().chain(&tanks).copied().collect();
    go(&mut w, &all, cells(40, 20));
    run(&mut w, 120);
    let slots: Vec<(EntityId, Vec2)> = all
        .iter()
        .map(|&id| (id, w.arrival_slot_of(id).expect("a slot")))
        .collect();
    for (k, &(a, pa)) in slots.iter().enumerate() {
        for &(b, pb) in &slots[k + 1..] {
            assert_ne!(pa, pb, "{a:?} and {b:?} have one slot");
            // Two units keep a body apart at their slots: the sum of their radii
            // (or three quarters of it for two infantry), whatever the classes.
            let need = keep(&w, a, b);
            assert!(
                (pa - pb).length() >= need,
                "{a:?} {b:?}: slots {pa:?} {pb:?} are closer than {need:?}"
            );
        }
    }
}

#[test]
fn a_group_of_a_hundred_riflemen_fits_five_cells_and_a_hundred_tanks_need_far_more() {
    let spread = |kind: Kind, n: usize| {
        let mut w = open_world();
        let ids: Vec<EntityId> = (0..n)
            .map(|k| {
                spawn(
                    &mut w,
                    kind,
                    0,
                    cells(2 + (k % 10) as i32 * 2, 2 + (k / 10) as i32 * 2),
                )
            })
            .collect();
        go(&mut w, &ids, cells(50, 40));
        run(&mut w, 3000);
        let goal = cells(50, 40);
        ids.iter()
            .map(|&id| (pos(&w, id) - goal).length())
            .fold(Fx::ZERO, Fx::max)
    };
    let infantry = spread(RIFLE, 100);
    assert!(
        infantry < Fx::from_int(5),
        "a hundred riflemen end within {infantry:?}"
    );
    let tanks = spread(TANK, 100);
    assert!(
        tanks > Fx::from_int(5),
        "a hundred tanks cannot fit in five cells: {tanks:?}"
    );
}

// --- bodies placed on top of each other (WP4.4b) ---

/// Places `n` by `n` units of `kind` one cell apart, all on the same tick, and steps once.
fn place_block(w: &mut World, kind: Kind, n: i32) -> Vec<EntityId> {
    let tick = w.tick();
    let first = w.slot_count() as u32;
    let mut ids = Vec::new();
    for k in 0..n * n {
        ids.push(EntityId::new(first + k as u32, 0));
        w.schedule(
            tick,
            PlayerId(0),
            Command::SpawnUnit {
                kind,
                pos: cells(10 + k % n, 10 + k / n),
                owner: PlayerId(0),
            },
        )
        .unwrap();
    }
    w.step();
    ids
}

#[test]
fn a_block_of_wide_bodies_placed_a_cell_apart_is_clear_of_overlap_on_the_tick_it_is_placed() {
    // A body one and a half cells wide on every cell: each overlaps its neighbours by half
    // a cell, and the pushes of a neighbour on each side cancel, so only the edge of the
    // block gives way in a round. The block spreads until it is within the bound of the
    // crowd pass (three sixteenths of a cell) before the first tick is over.
    for n in [3, 6, 9] {
        let mut w = open_world();
        let ids = place_block(&mut w, BIG, n);
        let worst = worst_overlap(&w, &ids);
        assert!(
            worst <= fx(3, 16),
            "a block of {n} by {n} overlaps by {worst:?} on the tick it is placed"
        );
    }
}

#[test]
fn a_block_that_was_cleared_stays_where_it_is_and_units_placed_later_do_not_move_it() {
    let mut w = open_world();
    let ids = place_block(&mut w, BIG, 5);
    let after: Vec<Vec2> = ids.iter().map(|&id| pos(&w, id)).collect();
    // Nobody is touching it: it stays put, tick after tick...
    run(&mut w, 30);
    assert_eq!(after, ids.iter().map(|&id| pos(&w, id)).collect::<Vec<_>>());
    // ...and a unit placed far away on a later tick disturbs nobody.
    let far = spawn(&mut w, BIG, 0, cells(40, 40));
    assert_eq!(pos(&w, far), cells(40, 40));
    assert_eq!(after, ids.iter().map(|&id| pos(&w, id)).collect::<Vec<_>>());
}

#[test]
fn what_is_placed_on_the_tick_does_not_depend_on_the_order_the_commands_are_listed_in() {
    let run_block = |reverse: bool| {
        let mut w = open_world();
        let tick = w.tick();
        let first = w.slot_count() as u32;
        let mut order: Vec<i32> = (0..25).collect();
        if reverse {
            order.reverse();
        }
        for k in order {
            w.schedule(
                tick,
                PlayerId(0),
                Command::SpawnUnit {
                    kind: BIG,
                    pos: cells(10 + k % 5, 10 + k / 5),
                    owner: PlayerId(0),
                },
            )
            .unwrap();
        }
        w.step();
        let mut places: Vec<Vec2> = (0..25u32)
            .map(|k| pos(&w, EntityId::new(first + k, 0)))
            .collect();
        places.sort_by_key(|p| (p.x.raw(), p.y.raw()));
        places
    };
    // The set of places the block ends in is the same.
    assert_eq!(run_block(false), run_block(true));
}

// --- the arrival logic of a group (WP4.4b) ---

/// A world with terrain (so that group orders make flowfields and slots) and one
/// harmless rock far from where the tests walk.
fn group_world() -> World {
    rock_world(|x, y| x == 60 && y == 44)
}

/// Whether every unit of `ids` has no order left.
fn all_idle(w: &World, ids: &[EntityId]) -> bool {
    ids.iter().all(|&id| w.order_of(id) == Some(Order::Idle))
}

/// Steps until every unit of `ids` has arrived; panics if that takes more than `limit`.
fn run_until_arrived(w: &mut World, ids: &[EntityId], limit: u64) -> u64 {
    for t in 1..=limit {
        w.step();
        if all_idle(w, ids) {
            return t;
        }
    }
    panic!("not everybody had arrived after {limit} ticks");
}

#[test]
fn a_unit_takes_its_slot_when_it_comes_near_so_the_crowd_fills_in_the_order_it_arrives() {
    // Twelve tanks in a column, the one with the highest id nearest the goal. Slots by
    // rank would give the lowest id the slot nearest the goal, and it is the last to
    // come: it would have to push through the others.
    //
    // Formations off: this pins the claims of a group order (WP4.4b), which come in the order
    // the units arrive. Twelve tanks sent together are a formation since WP4.6, and a
    // formation keeps its column closed (the front units wait for their slots), so the
    // column reaches the slots as one block and the claims of one tick go in id order
    // (`path/claim.rs`): there is no first to come.
    let mut w = group_world().with_formation(FormationConfig::OFF).unwrap();
    let goal = cells(40, 20);
    let ids: Vec<EntityId> = (0..12)
        .map(|k| spawn(&mut w, TANK, 0, cells(4 + 2 * k, 20)))
        .collect();
    go(&mut w, &ids, goal);
    // Until the first of them is near, nobody has taken a slot.
    run(&mut w, 20);
    assert!(ids.iter().all(|&id| w.claimed_slot_of(id).is_none()));
    for _ in 0..600 {
        w.step();
        if ids.iter().all(|&id| w.claimed_slot_of(id).is_some()) {
            break;
        }
    }
    let slots: Vec<Vec2> = ids
        .iter()
        .map(|&id| w.claimed_slot_of(id).expect("every unit took a slot"))
        .collect();
    let radius = |p: Vec2| (p - goal).length();
    // The first to come took the innermost slot, the last to come an outer one.
    let nearest = ids.len() - 1;
    for (k, &p) in slots.iter().enumerate() {
        assert!(
            radius(slots[nearest]) <= radius(p),
            "unit {k} took a slot nearer the goal than the first to come"
        );
    }
    assert!(radius(slots[0]) > radius(slots[nearest]) + Fx::ONE);
    // All different, and a body apart.
    for (k, &a) in ids.iter().enumerate() {
        for (j, &b) in ids.iter().enumerate().skip(k + 1) {
            assert!(
                (slots[k] - slots[j]).length() >= keep(&w, a, b),
                "slots {k} and {j} are too near"
            );
        }
    }
}

#[test]
fn a_group_of_mixed_speeds_arrives_inside_the_disc_that_holds_its_bodies() {
    // A column of tanks with a slow rifleman at its head: the tanks catch up with it
    // and come to the goal before it does, in no order of their ids.
    let mut w = group_world();
    let goal = cells(40, 20);
    let mut ids = Vec::new();
    for k in 0..16 {
        let kind = if k == 15 { SLOW } else { TANK };
        ids.push(spawn(&mut w, kind, 0, cells(4 + 2 * k, 20)));
    }
    go(&mut w, &ids, goal);
    run_until_arrived(&mut w, &ids, 2400);
    run(&mut w, 100);
    let worst = worst_overlap(&w, &ids);
    assert!(
        worst < fx(1, 16),
        "at rest the crowd has settled, overlap {worst:?}"
    );
    // The disc whose area is a square of the distance the bodies keep, for each (355/113
    // is pi), and the 1.3 times its radius the criterion of the phase allows.
    let area = ids
        .iter()
        .map(|&id| keep(&w, id, id).square())
        .fold(Fx::ZERO, |a, b| a + b);
    let packing = area.mul_div(Fx::from_int(113), Fx::from_int(355)).sqrt();
    let limit = packing.mul_int(13).div_int(10);
    for &id in &ids {
        let at = (pos(&w, id) - goal).length();
        assert!(
            at <= limit,
            "{id:?} ended {at:?} cells from the goal, the disc that holds them is {packing:?}"
        );
    }
}

#[test]
fn a_settled_crowd_closes_up_again_toward_the_point_after_something_pushed_through_it() {
    let mut w = group_world();
    let goal = cells(40, 20);
    let riflemen: Vec<EntityId> = (0..40)
        .map(|k| spawn(&mut w, RIFLE, 0, cells(6 + (k % 8), 10 + (k / 8) * 2)))
        .collect();
    go(&mut w, &riflemen, goal);
    run_until_arrived(&mut w, &riflemen, 2400);
    run(&mut w, 200);
    let radius = |w: &World| {
        riflemen
            .iter()
            .map(|&id| (pos(w, id) - goal).length())
            .fold(Fx::ZERO, Fx::max)
    };
    let before = radius(&w);
    // They are at home: the point their order sent them to.
    for &id in &riflemen {
        assert_eq!(w.home_of(id), Some(goal));
    }
    // A tank drives through the middle of them and on.
    let tank = spawn(&mut w, TANK, 0, cells(20, 20));
    go(&mut w, &[tank], cells(60, 20));
    let mut widest = before;
    for _ in 0..500 {
        w.step();
        widest = widest.max(radius(&w));
    }
    assert!(
        widest > before + fx(1, 2),
        "the tank pushed some of them out: {before:?} then {widest:?}"
    );
    // It has gone; they close up again.
    run(&mut w, 800);
    let after = radius(&w);
    assert!(
        after <= before + fx(1, 2),
        "the crowd was {before:?} wide, {widest:?} at the widest and {after:?} after"
    );
    assert!(worst_overlap(&w, &riflemen) < fx(1, 16));
    // A new order lets go of home.
    go(&mut w, &riflemen[..1], cells(30, 30));
    w.step();
    assert_eq!(w.home_of(riflemen[0]), None);
}

#[test]
fn a_crowd_pressed_into_a_dead_end_never_overlaps_beyond_the_bound_of_the_phase() {
    // A corridor three cells wide that ends in rock; twenty-four vehicles and
    // riflemen all ordered to its far end, where there is room for a handful. The
    // ones behind press on the ones in front, which cannot give way any more.
    let mut w = rock_world(|x, y| (30..50).contains(&x) && !(20..=22).contains(&y) || x >= 50);
    let mut ids = Vec::new();
    for k in 0..24 {
        let kind = [TANK, RIFLE, RIFLE, BIG][k % 4];
        ids.push(spawn(
            &mut w,
            kind,
            0,
            cells(2 + (k % 6) as i32 * 2, 14 + (k / 6) as i32 * 3),
        ));
    }
    go(&mut w, &ids, cells(48, 21));
    let mut worst = Fx::ZERO;
    for _ in 0..2000 {
        w.step();
        worst = worst.max(worst_overlap(&w, &ids));
    }
    assert!(
        worst < fx(1, 4),
        "the crowd was pressed to an overlap of {worst:?}, the bound is a quarter of a cell"
    );
}
