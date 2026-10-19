//! Replanning (WP4.7), through the public API: a unit stuck behind a body that stands
//! still asks for a new route that goes round it, a request that finds no way round ends in
//! giving the order up after a few tries, a structure built on the way, a target that
//! moves or disappears or cannot be reached, and a group whose goal is built over. The
//! rules are in `docs/rules/pathfinding.md` ("Replanning"), `docs/rules/orders.md` and
//! `docs/rules/collision.md`.

use sf_math::{Fx, Vec2};
use sf_sim::{
    Cell, Command, ConfigError, Cost, EntityId, Footprint, Kind, MovementTable, Order, PlayerId,
    ReplanConfig, RouteStatus, Terrain, TerrainDef, TerrainFlags, TerrainId, UnitStats, UnitTable,
    World, WorldConfig, per_tick,
};

const GRASS: TerrainId = TerrainId(0);
const ROCK: TerrainId = TerrainId(1);

const WALKER: u16 = 0;
const TRACKED: u16 = 1;

const RIFLE: Kind = Kind(1); // 0.4 wide, 0.7 cells a second, reach 4
const TANK: Kind = Kind(2); // 1.2 wide, 2 cells a second, reach 5
const HUT: Kind = Kind(3); // a structure, 1 by 1
const BIG: Kind = Kind(4); // 1.5 wide, 1.1 cells a second
const SCOUT: Kind = Kind(5); // 0.4 wide, 1.3 cells a second, no weapon

const MAP: (u32, u32) = (64, 40);

fn table() -> UnitTable {
    let mobile = |kind: Kind, class: u16, speed_tenths: i64, size_tenths: i64, range: i64| {
        UnitStats {
            kind,
            hp: 100,
            movement_class: Some(class),
            speed_per_tick: per_tick(Fx::from_ratio(speed_tenths, 10)),
            // Lined up at once, so what a test sees is the replan and not the turning.
            turn_per_tick: 255,
            size: Fx::from_ratio(size_tenths, 10),
            footprint: None,
            weapon_range: Fx::from_int(range as i32),
            turret_per_tick: 0,
        }
    };
    UnitTable::new(vec![
        mobile(RIFLE, WALKER, 7, 4, 4),
        mobile(TANK, TRACKED, 20, 12, 5),
        UnitStats {
            kind: HUT,
            hp: 100,
            movement_class: None,
            speed_per_tick: Fx::ZERO,
            turn_per_tick: 0,
            size: Fx::ZERO,
            footprint: Some(Footprint {
                width: 1,
                height: 1,
            }),
            weapon_range: Fx::ZERO,
            turret_per_tick: 0,
        },
        mobile(BIG, TRACKED, 11, 15, 5),
        mobile(SCOUT, WALKER, 13, 4, 0),
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
    ])
    .unwrap()
}

fn config() -> WorldConfig {
    WorldConfig::new(7).with_map_size(MAP.0, MAP.1)
}

/// Grass with rock where `rock(x, y)` says so.
fn terrain(rock: impl Fn(u32, u32) -> bool) -> Terrain {
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
    Terrain::from_layer(MAP.0, MAP.1, &layer, &defs).unwrap()
}

fn world_with(rock: impl Fn(u32, u32) -> bool, replan: ReplanConfig) -> World {
    World::with_terrain(config(), terrain(rock))
        .unwrap()
        .with_replan(replan)
        .unwrap()
        .with_units(table())
        .unwrap()
        .with_movement(costs())
        .unwrap()
}

fn world_on(rock: impl Fn(u32, u32) -> bool) -> World {
    world_with(rock, ReplanConfig::DEFAULT)
}

/// Rooms and corridors: a west chamber (columns 0 to 5) and an east chamber (columns 40 to
/// 45) joined by a corridor one cell wide along row 8 and, with `lower`, a second one
/// along row 24; rows 1 to 30; everything else is rock.
fn corridor_rock(lower: bool) -> impl Fn(u32, u32) -> bool {
    move |x, y| {
        let chamber = x <= 5 || (40..=45).contains(&x);
        let corridor = (6..40).contains(&x) && (y == 8 || (lower && y == 24));
        !((chamber && (1..=30).contains(&y)) || corridor)
    }
}

fn corridors(lower: bool) -> World {
    world_on(corridor_rock(lower))
}

/// Two rooms joined by a wall with two gaps one cell wide, at rows 8 and 24: the wall is
/// column 30, the rooms are columns 1 to 29 and 31 to 62, rows 1 to 30.
fn gates() -> World {
    world_on(|x, y| {
        let wall = x == 30 && y != 8 && y != 24;
        x == 0 || x == 63 || y == 0 || y > 30 || wall
    })
}

/// Open ground inside a ring of rock.
fn open_rock(x: u32, y: u32) -> bool {
    x == 0 || x == 63 || y == 0 || y > 38
}

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

fn order(w: &mut World, ids: &[EntityId], order: Order, queued: bool) {
    command(
        w,
        Command::Order {
            ids: ids.to_vec(),
            order,
            queued,
        },
    );
}

fn go(w: &mut World, ids: &[EntityId], to: Vec2) {
    order(w, ids, Order::Move(to), false);
}

fn hold(w: &mut World, ids: &[EntityId]) {
    order(w, ids, Order::HoldPosition, false);
}

fn pos(w: &World, id: EntityId) -> Vec2 {
    w.get(id).unwrap().pos
}

fn run(w: &mut World, ticks: u64) {
    for _ in 0..ticks {
        w.step();
    }
}

/// The ticks, in the order they happened, at which a count went up.
struct Counter<F: Fn(&World) -> u64> {
    read: F,
    last: u64,
    at: Vec<u64>,
}

impl<F: Fn(&World) -> u64> Counter<F> {
    fn new(w: &World, read: F) -> Counter<F> {
        let last = read(w);
        Counter {
            read,
            last,
            at: Vec::new(),
        }
    }

    fn watch(&mut self, w: &World) {
        let now = (self.read)(w);
        for _ in self.last..now {
            self.at.push(w.tick().0);
        }
        self.last = now;
    }
}

fn never_in_rock(w: &World, ids: &[EntityId], class: u16) {
    for &id in ids {
        let cell = w.cell_at(pos(w, id)).unwrap();
        assert!(w.passable(class, cell), "{id:?} is in {cell:?}");
    }
}

// --- the settings ---

#[test]
fn the_settings_are_checked_and_given_before_the_world_starts() {
    let base = || World::new(config()).unwrap();
    let ok = ReplanConfig {
        cooldown: 50,
        max: 2,
        retarget_cooldown: 10,
    };
    assert_eq!(base().with_replan(ok).unwrap().replan_config(), ok);
    assert_eq!(
        base().replan_config(),
        ReplanConfig::DEFAULT,
        "a world that is given none uses the defaults"
    );
    for bad in [
        ReplanConfig { cooldown: 0, ..ok },
        ReplanConfig { max: 0, ..ok },
        ReplanConfig {
            retarget_cooldown: 0,
            ..ok
        },
    ] {
        assert_eq!(base().with_replan(bad).err(), Some(ConfigError::Replan));
    }
    let mut started = base();
    started.step();
    assert_eq!(
        started.with_replan(ok).err(),
        Some(ConfigError::Replan),
        "too late"
    );
    // The tables can be given after the settings: the settings stay.
    let w = base()
        .with_replan(ok)
        .unwrap()
        .with_units(table())
        .unwrap()
        .with_movement(costs())
        .unwrap();
    assert_eq!(w.replan_config(), ok);
}

// --- a body that stands still in the way ---

#[test]
fn a_unit_stuck_behind_a_holder_goes_round_by_the_other_corridor() {
    let mut w = corridors(true);
    let holder = spawn(&mut w, BIG, 1, cells(20, 8));
    hold(&mut w, &[holder]);
    let tank = spawn(&mut w, TANK, 0, cells(3, 8));
    let goal = cells(43, 8);
    go(&mut w, &[tank], goal);
    let held_at = pos(&w, holder);
    let mut most = 0;
    let mut reached_the_holder = false;
    let mut went_round = false;
    for _ in 0..2400 {
        w.step();
        most = most.max(w.replans_of(tank).unwrap());
        reached_the_holder |=
            pos(&w, tank).x > Fx::from_int(17) && pos(&w, tank).y < Fx::from_int(10);
        // Round: through the lower corridor, row 24.
        went_round |= (pos(&w, tank) - cells(20, 24)).length() < Fx::from_int(3);
        assert_eq!(pos(&w, holder), held_at, "the holder never moves");
        never_in_rock(&w, &[tank, holder], TRACKED);
        if w.order_of(tank) == Some(Order::Idle) {
            break;
        }
    }
    assert!(reached_the_holder, "it drove up to the holder first");
    assert_eq!(w.order_of(tank), Some(Order::Idle), "it got there");
    assert!((pos(&w, tank) - goal).length() < Fx::ONE);
    assert!(went_round, "by the lower corridor");
    let stats = w.path_stats();
    assert_eq!(most, 1, "one replan did it");
    assert_eq!(stats.stuck_replans, 1);
    assert_eq!(stats.overlay_replans, 1, "with the holder blocked");
    assert_eq!(stats.detours_failed, 0);
    assert_eq!(w.crowd_stats().gave_up, 0);
    assert_eq!(w.replans_of(tank), Some(0), "the count is the order's");
}

#[test]
fn the_overlay_is_for_one_request_and_the_graph_is_the_same_for_everybody() {
    let mut w = corridors(true);
    let holder = spawn(&mut w, BIG, 1, cells(20, 8));
    hold(&mut w, &[holder]);
    let tank = spawn(&mut w, TANK, 0, cells(3, 8));
    let (from, to) = (cells(3, 8), cells(43, 8));
    go(&mut w, &[tank], to);
    let before = w.plan_route(TRACKED, from, to).unwrap();
    // A route for anyone else goes straight along the upper corridor: the graph does not
    // know the holder.
    assert!(
        before
            .waypoints
            .iter()
            .all(|p| p.y < Fx::from_int(10) || p.x > Fx::from_int(40)),
        "{:?}",
        before.waypoints
    );
    let mut watch = Counter::new(&w, |w| w.path_stats().overlay_replans);
    let mut late_comer = None;
    for _ in 0..1500 {
        w.step();
        watch.watch(&w);
        // The cells of the holder are open for every other request at every tick,
        // during the ticks of the overlay request included.
        assert!(w.sight_clear(TRACKED, cells(17, 8), cells(23, 8)));
        assert_eq!(w.plan_route(TRACKED, from, to).unwrap(), before);
        if !watch.at.is_empty() && late_comer.is_none() {
            // A second tank that sets off now is routed by the graph as it is: through
            // the upper corridor. The overlay was the first tank's alone.
            let t2 = spawn(&mut w, TANK, 0, cells(3, 10));
            go(&mut w, &[t2], to);
            late_comer = Some(t2);
        }
        if let Some(t2) = late_comer
            && let Some(p) = w.waypoint_of(t2)
        {
            assert!(p.y < Fx::from_int(11) || p.x < Fx::from_int(7), "{p:?}");
        }
    }
    assert!(late_comer.is_some(), "the first tank asked for a replan");
}

#[test]
fn with_no_way_round_the_order_is_given_up_after_k_replans_and_the_queue_goes_on() {
    let mut w = corridors(false);
    let holder = spawn(&mut w, BIG, 1, cells(20, 8));
    hold(&mut w, &[holder]);
    let tank = spawn(&mut w, TANK, 0, cells(3, 8));
    let goal = cells(43, 8);
    let back = cells(3, 20);
    go(&mut w, &[tank], goal);
    order(&mut w, &[tank], Order::Move(back), true);
    let k = w.replan_config().max;
    assert_eq!(w.gave_up_at(tank), None);
    let mut most = 0;
    let mut gave_up_at = None;
    let mut asks = Counter::new(&w, |w| w.path_stats().stuck_replans);
    for _ in 0..4000 {
        w.step();
        asks.watch(&w);
        most = most.max(w.replans_of(tank).unwrap());
        if gave_up_at.is_none() && w.order_of(tank) != Some(Order::Move(goal)) {
            gave_up_at = Some(w.tick().0);
            // The record of it: the tick of the give-up, set as the order ended.
            assert_eq!(w.gave_up_at(tank).map(|t| t.0 + 1), gave_up_at);
            // The next order of the queue started, with a new count.
            assert_eq!(w.order_of(tank), Some(Order::Move(back)));
            assert_eq!(w.replans_of(tank), Some(0));
            // And it was not before the last of the replans had been tried.
            assert_eq!(asks.at.len(), usize::from(k));
        }
        never_in_rock(&w, &[tank], TRACKED);
    }
    assert_eq!(most, k, "it asked k times and no more");
    assert_eq!(asks.at.len(), usize::from(k));
    assert!(gave_up_at.is_some());
    assert_eq!(w.crowd_stats().gave_up, 1);
    assert_eq!(
        w.path_stats().detours_failed,
        u64::from(k),
        "every request found nothing"
    );
    // The queued order was carried out.
    assert_eq!(w.order_of(tank), Some(Order::Idle));
    assert!((pos(&w, tank) - back).length() < Fx::ONE);
    // The record outlives the order that was given up, and the queued one starting.
    assert_eq!(w.gave_up_at(tank).map(|t| t.0 + 1), gave_up_at);
    // It is in the checksum: the same world with the record cleared hashes otherwise.
    let with = w.clone();
    let mut without = w.clone();
    order(&mut without, &[tank], Order::HoldPosition, false);
    without.step();
    assert_eq!(without.gave_up_at(tank), None, "a new order clears it");
    let mut also = with.clone();
    also.step();
    assert_eq!(also.gave_up_at(tank).map(|t| t.0 + 1), gave_up_at);
    assert_ne!(
        also.checksum_history().last(),
        without.checksum_history().last()
    );
    // Stop clears it too.
    let mut stopped = with.clone();
    command(&mut stopped, Command::Stop { ids: vec![tank] });
    stopped.step();
    assert_eq!(stopped.gave_up_at(tank), None);
    assert_eq!(
        w.gave_up_at(EntityId::new(99, 0)),
        None,
        "a stale id has none"
    );
    // Not a storm: the replans are a cooldown apart, at least.
    let cooldown = u64::from(w.replan_config().cooldown);
    for pair in asks.at.windows(2) {
        assert!(pair[1] - pair[0] >= cooldown, "{:?}", asks.at);
    }
}

#[test]
fn a_unit_with_nothing_queued_stands_idle_where_it_gave_up() {
    let mut w = corridors(false);
    let holder = spawn(&mut w, BIG, 1, cells(20, 8));
    hold(&mut w, &[holder]);
    let tank = spawn(&mut w, TANK, 0, cells(3, 8));
    go(&mut w, &[tank], cells(43, 8));
    run(&mut w, 3000);
    assert_eq!(w.order_of(tank), Some(Order::Idle));
    assert_eq!(w.route_of(tank).unwrap().status, RouteStatus::None);
    // Still a body's distance from the holder: it did not walk through it.
    assert!(pos(&w, tank).x < Fx::from_int(19));
    assert_eq!(w.queued_orders_of(tank), Some(Vec::new()));
    assert_eq!(w.order_of(holder), Some(Order::HoldPosition));
}

#[test]
fn the_number_of_replans_and_the_cooldown_are_data() {
    let run_with = |max: u8, cooldown: u16| {
        let mut w = world_with(
            corridor_rock(false),
            ReplanConfig {
                cooldown,
                max,
                retarget_cooldown: 20,
            },
        );
        let holder = spawn(&mut w, BIG, 1, cells(20, 8));
        hold(&mut w, &[holder]);
        let tank = spawn(&mut w, TANK, 0, cells(3, 8));
        go(&mut w, &[tank], cells(43, 8));
        let mut asks = Counter::new(&w, |w| w.path_stats().stuck_replans);
        for _ in 0..5000 {
            w.step();
            asks.watch(&w);
        }
        (asks.at, w.crowd_stats().gave_up, w.order_of(tank))
    };
    // One replan: the order is given up when it would ask a second time.
    let (asks, gave_up, order) = run_with(1, 100);
    assert_eq!((asks.len(), gave_up, order), (1, 1, Some(Order::Idle)));
    // Five: it asks five times first.
    let (asks, gave_up, _) = run_with(5, 100);
    assert_eq!((asks.len(), gave_up), (5, 1));
    // A cooldown longer than the stuck periods holds the next one back.
    let (asks, _, _) = run_with(3, 600);
    assert_eq!(asks.len(), 3);
    for pair in asks.windows(2) {
        assert!(pair[1] - pair[0] >= 600, "{asks:?}");
    }
}

#[test]
fn a_new_order_forgets_the_count() {
    let mut w = corridors(false);
    let holder = spawn(&mut w, BIG, 1, cells(20, 8));
    hold(&mut w, &[holder]);
    let tank = spawn(&mut w, TANK, 0, cells(3, 8));
    go(&mut w, &[tank], cells(43, 8));
    // Until it has asked once.
    let mut ticks = 0;
    while w.replans_of(tank) == Some(0) {
        w.step();
        ticks += 1;
        assert!(ticks < 2000);
    }
    assert_eq!(w.replans_of(tank), Some(1));
    go(&mut w, &[tank], cells(3, 20));
    w.step();
    assert_eq!(w.replans_of(tank), Some(0));
    assert_eq!(w.replan_requested(tank), Some(false));
}

#[test]
fn an_enemy_that_stands_still_is_a_body_to_go_round_too() {
    // The same corridors, the blocker an enemy with no order instead of a holder.
    let mut w = corridors(true);
    let enemy = spawn(&mut w, BIG, 1, cells(20, 8));
    let tank = spawn(&mut w, TANK, 0, cells(3, 8));
    go(&mut w, &[tank], cells(43, 8));
    run(&mut w, 2400);
    assert_eq!(w.order_of(tank), Some(Order::Idle));
    assert!((pos(&w, tank) - cells(43, 8)).length() < Fx::ONE);
    assert_eq!(w.path_stats().overlay_replans, 1);
    assert_eq!(w.order_of(enemy), Some(Order::Idle));
}

// --- a change of the map ---

#[test]
fn a_structure_built_in_the_corridor_sends_the_unit_by_the_other_one() {
    let mut w = corridors(true);
    let tank = spawn(&mut w, TANK, 0, cells(3, 8));
    let goal = cells(43, 8);
    go(&mut w, &[tank], goal);
    run(&mut w, 100);
    assert!(pos(&w, tank).x > Fx::from_int(8), "on its way");
    command(
        &mut w,
        Command::PlaceStructure {
            kind: HUT,
            cell: Cell::new(18, 8),
            owner: PlayerId(1),
        },
    );
    w.step();
    assert!(w.is_occupied(Cell::new(18, 8)));
    // The graph knows the structure: the unit asks again at once, it is not stuck.
    let stats = w.path_stats();
    assert_eq!(stats.map_replans, 1);
    assert_eq!(w.route_of(tank).unwrap().status, RouteStatus::Waiting);
    let mut went_round = false;
    for _ in 0..2400 {
        w.step();
        went_round |= (pos(&w, tank) - cells(20, 24)).length() < Fx::from_int(3);
        let cell = w.cell_at(pos(&w, tank)).unwrap();
        assert!(!w.is_occupied(cell), "inside the structure");
        if w.order_of(tank) == Some(Order::Idle) {
            break;
        }
    }
    assert!(went_round);
    assert_eq!(w.order_of(tank), Some(Order::Idle));
    assert!((pos(&w, tank) - goal).length() < Fx::ONE);
    let stats = w.path_stats();
    assert_eq!(
        (
            stats.stuck_replans,
            stats.overlay_replans,
            stats.detours_failed
        ),
        (0, 0, 0),
        "no stuck replan was needed"
    );
}

#[test]
fn a_structure_that_seals_the_only_corridor_leaves_the_unit_at_the_nearest_cell_and_it_waits() {
    let mut w = corridors(false);
    let tank = spawn(&mut w, TANK, 0, cells(3, 8));
    let goal = cells(43, 8);
    go(&mut w, &[tank], goal);
    run(&mut w, 100);
    command(
        &mut w,
        Command::PlaceStructure {
            kind: HUT,
            cell: Cell::new(18, 8),
            owner: PlayerId(1),
        },
    );
    run(&mut w, 1500);
    // The graph is right: no way through, so the nearest cell it can reach, a wait there,
    // the order kept; the structure is not a body that will go away on its own, but the
    // unit does not give up either (it is not stuck, it is where the route ends).
    let view = w.route_of(tank).unwrap();
    assert!(view.unreachable);
    assert_eq!(view.status, RouteStatus::Stranded);
    assert_eq!(w.order_of(tank), Some(Order::Move(goal)));
    assert!((pos(&w, tank).x - Fx::from_ratio(35, 2)).abs() < Fx::ONE);
    assert_eq!(w.crowd_stats().gave_up, 0);
    assert_eq!(w.path_stats().stuck_replans, 0);
}

// --- a target that moves, disappears or cannot be reached ---

#[test]
fn a_target_that_keeps_moving_is_searched_for_again_at_most_once_in_the_cooldown() {
    let chase = |cooldown: u16| {
        let mut w = world_with(
            open_rock,
            ReplanConfig {
                retarget_cooldown: cooldown,
                ..ReplanConfig::DEFAULT
            },
        );
        let target = spawn(&mut w, SCOUT, 1, cells(5, 5));
        let hunter = spawn(&mut w, RIFLE, 0, cells(5, 12));
        go(&mut w, &[target], cells(58, 30));
        order(&mut w, &[hunter], Order::Attack(target), false);
        let mut asks = Counter::new(&w, |w| w.path_stats().target_replans);
        for _ in 0..1000 {
            w.step();
            asks.watch(&w);
        }
        assert_eq!(w.order_of(hunter), Some(Order::Attack(target)));
        (asks.at, (pos(&w, hunter) - pos(&w, target)).length())
    };
    let (quick, gap_quick) = chase(5);
    let (slow, gap_slow) = chase(200);
    for (asks, cooldown) in [(&quick, 5), (&slow, 200)] {
        for pair in asks.windows(2) {
            assert!(pair[1] - pair[0] >= cooldown, "{asks:?}");
        }
    }
    assert!(quick.len() >= 5, "it follows: {quick:?}");
    assert!(slow.len() <= 5, "{slow:?}");
    assert!(quick.len() > slow.len());
    // Following it is the point: with the long cooldown it keeps on its old route and
    // ends up farther behind, and neither gives up.
    assert!(gap_quick < Fx::from_int(40), "{gap_quick:?}");
    assert!(
        gap_quick <= gap_slow + Fx::ONE,
        "{gap_quick:?} {gap_slow:?}"
    );
}

#[test]
fn a_target_that_dies_ends_the_attack_and_the_next_order_starts() {
    let mut w = world_on(open_rock);
    let victim = spawn(&mut w, RIFLE, 1, cells(30, 20));
    let first = spawn(&mut w, TANK, 0, cells(10, 28));
    let second = spawn(&mut w, TANK, 0, cells(50, 28));
    order(&mut w, &[first, second], Order::Attack(victim), false);
    order(&mut w, &[first], Order::Move(cells(20, 30)), true);
    run(&mut w, 20);
    assert_eq!(w.order_of(second), Some(Order::Attack(victim)));
    command(&mut w, Command::Despawn { id: victim });
    w.step();
    // Both are done with it at once: one stands idle, the other goes on to the next order.
    assert_eq!(w.order_of(second), Some(Order::Idle));
    assert_eq!(w.order_of(first), Some(Order::Move(cells(20, 30))));
    let stopped_at = pos(&w, second);
    // Another unit takes the slot of the victim: a new generation, not the target.
    let newcomer = EntityId::new(victim.index(), victim.generation() + 1);
    command(
        &mut w,
        Command::SpawnUnit {
            kind: RIFLE,
            pos: cells(30, 10),
            owner: PlayerId(1),
        },
    );
    w.step();
    assert!(w.is_alive(newcomer), "the slot is used again");
    assert!(!w.is_alive(victim));
    go(&mut w, &[newcomer], cells(30, 36));
    run(&mut w, 400);
    assert_eq!(w.order_of(second), Some(Order::Idle));
    assert!((pos(&w, second) - stopped_at).length() < Fx::from_ratio(1, 4));
    assert_eq!(w.order_of(first), Some(Order::Idle));
    assert!((pos(&w, first) - cells(20, 30)).length() < Fx::ONE);
    // And a guard of an entity that is gone ends the same way.
    let watched = spawn(&mut w, RIFLE, 0, cells(40, 10));
    let guard = spawn(&mut w, TANK, 0, cells(44, 10));
    order(
        &mut w,
        &[guard],
        Order::Guard(sf_sim::GuardTarget::Entity(watched)),
        false,
    );
    run(&mut w, 5);
    command(&mut w, Command::Despawn { id: watched });
    run(&mut w, 2);
    assert_eq!(w.order_of(guard), Some(Order::Idle));
}

#[test]
fn a_hunter_that_keeps_stepping_into_and_out_of_reach_does_not_ask_for_a_route_each_time() {
    // A tank is faster than the rifleman it goes for: it catches up, stands in reach, the
    // rifleman walks on, out of reach, and so over and over. The route it has is kept
    // while it stands, and a request is made only when the target has moved far (and then
    // once in the cooldown): a request for every step out of reach, which is what this
    // used to be, is a storm.
    let mut w = world_on(open_rock);
    let target = spawn(&mut w, RIFLE, 1, cells(5, 5));
    let hunter = spawn(&mut w, TANK, 0, cells(5, 12));
    go(&mut w, &[target], cells(58, 30));
    order(&mut w, &[hunter], Order::Attack(target), false);
    let mut in_reach_ticks = 0;
    let reach = w.stats_of(hunter).unwrap().weapon_range + Fx::from_ratio(1, 5);
    for _ in 0..1500 {
        w.step();
        if (pos(&w, hunter) - pos(&w, target)).length() <= reach {
            in_reach_ticks += 1;
        }
    }
    let stats = w.path_stats();
    // About 1,500 ticks: it used to be a job every four ticks while the chase lasted.
    assert!(stats.jobs_done <= 30, "{stats:?}");
    assert!(stats.target_replans <= 1500 / 20, "{stats:?}");
    // And it does follow: in reach of the rifleman for most of the time, and still at it.
    assert!(in_reach_ticks > 1000, "{in_reach_ticks}");
    assert_eq!(w.order_of(hunter), Some(Order::Attack(target)));
    assert_eq!(w.crowd_stats().gave_up, 0);
}

/// Open ground with a pocket of grass sealed off by rock: the ring is columns 20 to 45, rows
/// 6 to 31, one cell thick, and everything inside is grass.
fn island_rock(x: u32, y: u32) -> bool {
    let ring = (20..=45).contains(&x)
        && (6..=31).contains(&y)
        && (x == 20 || x == 45 || y == 6 || y == 31);
    open_rock(x, y) || ring
}

#[test]
fn a_target_that_cannot_be_reached_leaves_the_hunter_at_the_nearest_cell_and_it_waits() {
    let mut w = world_with(
        island_rock,
        ReplanConfig {
            retarget_cooldown: 40,
            ..ReplanConfig::DEFAULT
        },
    );
    let target = spawn(&mut w, SCOUT, 1, cells(28, 14));
    let hunter = spawn(&mut w, TANK, 0, cells(5, 18));
    // The target paces up and down the island, more than three cells at a time and always
    // farther from the ring than the hunter's reach.
    for k in 0..6 {
        let to = if k % 2 == 0 {
            cells(37, 23)
        } else {
            cells(28, 14)
        };
        order(&mut w, &[target], Order::Move(to), k > 0);
    }
    order(&mut w, &[hunter], Order::Attack(target), false);
    let mut asks = Counter::new(&w, |w| w.path_stats().target_replans);
    for _ in 0..2400 {
        w.step();
        asks.watch(&w);
    }
    let view = w.route_of(hunter).unwrap();
    assert!(view.unreachable, "{view:?}");
    assert_eq!(view.status, RouteStatus::Stranded);
    assert_eq!(w.order_of(hunter), Some(Order::Attack(target)), "waits");
    // At the ring: outside it, on the nearest cell it can reach.
    let (x, y) = (pos(&w, hunter).x, pos(&w, hunter).y);
    let outside =
        x < Fx::from_int(20) || x > Fx::from_int(46) || y < Fx::from_int(6) || y > Fx::from_int(32);
    assert!(outside, "{x:?} {y:?}");
    let to_ring = (Fx::from_int(20) - x)
        .max(x - Fx::from_int(46))
        .max(Fx::from_int(6) - y)
        .max(y - Fx::from_int(32));
    assert!(to_ring < Fx::from_int(3), "{to_ring:?}");
    // The target moved a lot; the searches for it are a cooldown apart.
    assert!(asks.at.len() <= 2400 / 40 + 1, "{:?}", asks.at);
    assert!(!asks.at.is_empty(), "it was searched for again");
    for pair in asks.at.windows(2) {
        assert!(pair[1] - pair[0] >= 40, "{:?}", asks.at);
    }
    assert_eq!(w.crowd_stats().gave_up, 0);
    assert_eq!(w.path_stats().stuck_replans, 0);
}

// --- a group ---

#[test]
fn a_member_of_a_field_that_a_holder_stops_leaves_it_for_a_route_of_its_own() {
    let mut w = gates();
    let holder = spawn(&mut w, BIG, 1, cells(30, 8));
    hold(&mut w, &[holder]);
    let group: Vec<EntityId> = (0..8)
        .map(|k| spawn(&mut w, SCOUT, 0, cells(3 + k % 4, 7 + 2 * (k / 4))))
        .collect();
    let goal = cells(50, 8);
    go(&mut w, &group, goal);
    w.step();
    assert!(
        group.iter().all(|&id| w.route_of(id).unwrap().field),
        "one order, one field"
    );
    let mut left = vec![false; group.len()];
    let mut most = 0;
    for _ in 0..6000 {
        w.step();
        for (k, &id) in group.iter().enumerate() {
            if w.order_of(id) != Some(Order::Idle) && !w.route_of(id).unwrap().field {
                left[k] = true;
            }
            most = most.max(w.replans_of(id).unwrap());
        }
        never_in_rock(&w, &group, WALKER);
        if group.iter().all(|&id| w.order_of(id) == Some(Order::Idle)) {
            break;
        }
    }
    assert!(
        group.iter().all(|&id| w.order_of(id) == Some(Order::Idle)),
        "everybody got there"
    );
    for &id in &group {
        assert!((pos(&w, id) - goal).length() < Fx::from_int(6), "{id:?}");
    }
    assert!(left.iter().any(|&l| l), "some member left the field");
    let stats = w.path_stats();
    assert!(stats.overlay_replans >= 1);
    assert_eq!(w.crowd_stats().gave_up, 0);
    assert!(most <= w.replan_config().max);
    // Not a storm: a handful of requests for the whole group.
    assert!(
        stats.stuck_replans <= 8 * u64::from(w.replan_config().max),
        "{stats:?}"
    );
}

#[test]
fn a_group_whose_goal_is_built_over_goes_to_the_nearest_cells_and_takes_its_places_there() {
    let mut w = world_on(open_rock);
    let group: Vec<EntityId> = (0..10)
        .map(|k| spawn(&mut w, TANK, 0, cells(3 + 2 * (k % 3), 10 + 2 * (k / 3))))
        .collect();
    let goal = cells(40, 20);
    go(&mut w, &group, goal);
    run(&mut w, 150);
    assert_eq!(w.path_stats().fields_built, 1);
    assert!(group.iter().all(|&id| w.route_of(id).unwrap().field));
    command(
        &mut w,
        Command::PlaceStructure {
            kind: HUT,
            cell: Cell::new(40, 20),
            owner: PlayerId(1),
        },
    );
    let mut most = 0;
    for _ in 0..1500 {
        w.step();
        for &id in &group {
            most = most.max(w.replans_of(id).unwrap());
            assert!(
                !w.is_occupied(w.cell_at(pos(&w, id)).unwrap()),
                "inside the structure"
            );
        }
        if group.iter().all(|&id| w.order_of(id) == Some(Order::Idle)) {
            break;
        }
    }
    assert!(
        group.iter().all(|&id| w.order_of(id) == Some(Order::Idle)),
        "everybody arrived"
    );
    // A successor was built with the nearest free cell for a goal, and they stand round it.
    assert!(w.path_stats().fields_built >= 2);
    for &id in &group {
        let d = (pos(&w, id) - goal).length();
        assert!(d < Fx::from_int(5), "{id:?} ended {d:?} from the goal");
    }
    assert_eq!(w.crowd_stats().gave_up, 0);
    assert!(most <= 1, "nobody had to ask for a replan: {most}");
}
