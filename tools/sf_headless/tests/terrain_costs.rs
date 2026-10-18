//! The `terrain_costs` scenario (WP4.1, `docs/rules/movement.md`): units of
//! every movement class cross road, grass, ore and a river at a bridge, and
//! are kept from rock and water. Arrival ticks, positions and the states of the
//! routes are asserted; the checksum is pinned in
//! `assets/scenarios/terrain_costs.expected` (see `pinned.rs`).
//!
//! Since WP4.2 an order to move finds its way: the land classes cross the river
//! on the bridge (also when they were sent straight at the water), everybody
//! goes round the rock wall, and only the gunboat sent onto the bank, whose
//! destination cannot be reached, stands at the nearest cell. Every unit with a
//! terrain-bound class asks for its route on the tick of its order and moves
//! from the next one, which is the one tick later than at WP4.1.
//!
//! The map, the lanes and the entity ids are written in
//! `assets/scenarios/terrain_costs.ron`: slot = 4 * lane + class, lanes in the
//! order grass, road, ore, bridge, water, rock and classes in the order foot,
//! wheeled, tracked, hover; the gunboats are 24 and 25, the drone 26.

mod common;

use common::scenario_path;
use sf_headless::scenario::Scenario;
use sf_math::{Fx, Vec2};
use sf_sim::{Cell, Cost, EntityId, Order, RouteStatus, World, per_tick, scale_step};

const GRASS: usize = 0;
const ROAD: usize = 1;
const ORE: usize = 2;
const BRIDGE: usize = 3;
const WATER: usize = 4;
const ROCK: usize = 5;

const FOOT: usize = 0;
const WHEELED: usize = 1;
const TRACKED: usize = 2;
const HOVER: usize = 3;

const BOAT_DOWN: EntityId = EntityId::new(24, 0);
const BOAT_BANK: EntityId = EntityId::new(25, 0);
const DRONE: EntityId = EntityId::new(26, 0);

fn unit(lane: usize, class: usize) -> EntityId {
    EntityId::new((4 * lane + class) as u32, 0)
}

fn mid(x: i32, y: i32) -> Vec2 {
    Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF)
}

/// Everything the scenario does, recorded tick by tick.
struct Run {
    world: World,
    ticks: u64,
    /// The first tick (from 2 on) at which a unit had no order any more.
    arrived: Vec<Option<u64>>,
    /// The cells each unit stood in at the end of every tick.
    cells: Vec<Vec<Cell>>,
    /// Positions of all 27 units at the end of tick 499.
    at_500: Vec<Vec2>,
}

fn run() -> Run {
    let scenario = Scenario::load(&scenario_path("terrain_costs.ron")).unwrap();
    let mut world = scenario.build_world().unwrap();
    let mut arrived = vec![None; 27];
    let mut cells = vec![Vec::new(); 27];
    let mut at_500 = Vec::new();
    for _ in 0..scenario.ticks {
        let report = world.step();
        for slot in 0..27_u32 {
            let id = EntityId::new(slot, 0);
            let at = world.cell_at(world.get(id).unwrap().pos).unwrap();
            cells[slot as usize].push(at);
            if report.tick.0 >= 2
                && arrived[slot as usize].is_none()
                && world.order_of(id) == Some(Order::Idle)
            {
                arrived[slot as usize] = Some(report.tick.0);
            }
        }
        if report.tick.0 == 499 {
            at_500 = (0..27)
                .map(|slot| world.get(EntityId::new(slot, 0)).unwrap().pos)
                .collect();
        }
    }
    Run {
        ticks: scenario.ticks,
        world,
        arrived,
        cells,
        at_500,
    }
}

impl Run {
    fn pos(&self, id: EntityId) -> Vec2 {
        self.world.get(id).unwrap().pos
    }

    fn cell(&self, id: EntityId) -> Cell {
        self.world.cell_at(self.pos(id)).unwrap()
    }

    fn arrived(&self, id: EntityId) -> Option<u64> {
        self.arrived[id.index() as usize]
    }

    /// The per-tick step of a unit on a terrain with this multiplier.
    fn step_of(&self, id: EntityId, permille: u16) -> Fx {
        let stats = self.world.stats_of(id).unwrap();
        scale_step(stats.speed_per_tick, permille)
    }

    /// The multiplier of the cell a unit starts in (a lane is one terrain).
    fn permille_at(&self, id: EntityId, cell: Cell) -> u16 {
        let class = self.world.stats_of(id).unwrap().movement_class.unwrap();
        match self.world.cost(class, cell) {
            Cost::Speed(p) => p,
            Cost::Impassable => panic!("{id:?} cannot be on {cell}"),
        }
    }

    /// Ticks it takes to cover `cells` cells at `permille`, to within the
    /// arrival tolerance, if the unit does not have to turn first and has its
    /// route (WP4.2: the order is given on tick 1, the unit asks for the route
    /// on that tick and moves from tick 2).
    fn straight_ticks(&self, id: EntityId, permille: u16, cells: i32) -> u64 {
        let reach = Fx::from_int(cells) - sf_sim::ARRIVE_TOLERANCE;
        let step = self.step_of(id, permille);
        let n = (reach.raw() + step.raw() - 1) / step.raw();
        n as u64
    }

    /// The most ticks a unit can spend turning toward its goal first: half a
    /// turn at its turn rate.
    fn turning(&self, id: EntityId) -> u64 {
        let turn = u64::from(self.world.stats_of(id).unwrap().turn_per_tick);
        128_u64.div_ceil(turn)
    }
}

// --- the speed a terrain gives ---

#[test]
fn a_straight_run_takes_the_ticks_the_cost_table_says() {
    let r = run();
    // Fifteen cells from the middle of column 2 to the middle of column 17, on
    // one terrain: the unit's own speed times the multiplier of that terrain.
    for lane in [GRASS, ROAD, ORE] {
        for class in [FOOT, WHEELED, TRACKED, HOVER] {
            let id = unit(lane, class);
            let permille = r.permille_at(id, Cell::new(2, 2 * lane as u32 + 2));
            let want = r.straight_ticks(id, permille, 15);
            let got = r
                .arrived(id)
                .unwrap_or_else(|| panic!("{id:?} never arrived"));
            // The order is given on tick 1, the unit asks for its route on that
            // tick and steps from tick 2; it may first have to turn. (The route
            // of a run of fifteen cells in the open is one waypoint: the goal.)
            assert!(
                got > want && got <= want + r.turning(id) + 2,
                "lane {lane} class {class}: arrived on tick {got}, a straight run of {want} \
                 steps at {permille} permille after the tick of the request, plus at most {} \
                 ticks of turning",
                r.turning(id)
            );
        }
    }
}

#[test]
fn road_is_faster_than_grass_and_ore_slower() {
    let r = run();
    for class in [FOOT, WHEELED, TRACKED, HOVER] {
        let ticks = |lane| r.arrived(unit(lane, class)).unwrap();
        assert!(
            ticks(ROAD) < ticks(GRASS) && ticks(GRASS) < ticks(ORE),
            "class {class}: road {} grass {} ore {}",
            ticks(ROAD),
            ticks(GRASS),
            ticks(ORE)
        );
    }
    // And the classes differ: wheeled vehicles gain more on a road and lose
    // more on ore than tracked ones do (1200 and 600 against 1200 and 800, from
    // their own base speeds), so the ratio of ore time to road time is larger.
    let ticks = |lane, class| r.arrived(unit(lane, class)).unwrap();
    assert!(
        ticks(ORE, WHEELED) * ticks(ROAD, TRACKED) > ticks(ORE, TRACKED) * ticks(ROAD, WHEELED)
    );
}

#[test]
fn the_pinned_arrival_ticks() {
    // The same runs as numbers: a change here is a change of the movement
    // rules, the cost table or the speeds, and so is deliberate. At WP4.2 the
    // lanes of open ground, the bridge and the hover craft in the water lane
    // each got one tick (the request for the route); the land classes in the
    // water lane and everyone in the rock lane take a route round the obstacle.
    let r = run();
    let table: [(usize, usize, u64); 24] = [
        (GRASS, FOOT, 433),
        (GRASS, WHEELED, 157),
        (GRASS, TRACKED, 155),
        (GRASS, HOVER, 205),
        (ROAD, FOOT, 392),
        (ROAD, WHEELED, 103),
        (ROAD, TRACKED, 133),
        (ROAD, HOVER, 198),
        (ORE, FOOT, 538),
        (ORE, WHEELED, 208),
        (ORE, TRACKED, 197),
        (ORE, HOVER, 232),
        (BRIDGE, FOOT, 280),
        (BRIDGE, WHEELED, 95),
        (BRIDGE, TRACKED, 95),
        (BRIDGE, HOVER, 129),
        (WATER, FOOT, 406),
        (WATER, WHEELED, 143),
        (WATER, TRACKED, 152),
        (WATER, HOVER, 198),
        (ROCK, FOOT, 265),
        (ROCK, WHEELED, 102),
        (ROCK, TRACKED, 95),
        (ROCK, HOVER, 137),
    ];
    for (lane, class, tick) in table {
        assert_eq!(
            r.arrived(unit(lane, class)),
            Some(tick),
            "lane {lane} class {class}"
        );
    }
    assert_eq!(r.arrived(BOAT_DOWN), Some(182));
    assert_eq!(r.arrived(BOAT_BANK), None);
    assert_eq!(r.arrived(DRONE), Some(234), "aircraft ask for no route");
}

// --- the bridge ---

#[test]
fn land_crosses_the_river_on_the_bridge_and_stands_on_the_far_bank() {
    let r = run();
    for class in [FOOT, WHEELED, TRACKED, HOVER] {
        let id = unit(BRIDGE, class);
        assert_eq!(r.world.order_of(id), Some(Order::Idle), "class {class}");
        assert_eq!(r.cell(id), Cell::new(27, 8), "class {class}");
        // It walked over every bridge cell (columns 20 to 23 of row 8).
        for x in 20..=23 {
            assert!(
                r.cells[id.index() as usize].contains(&Cell::new(x, 8)),
                "class {class} never stood on the bridge cell ({x}, 8)"
            );
        }
    }
}

#[test]
fn a_vessel_passes_under_the_bridge_and_stops_at_the_nearest_water() {
    let r = run();
    // Down the river from row 1 to row 18, through the bridge cell (21, 8).
    assert_eq!(r.world.order_of(BOAT_DOWN), Some(Order::Idle));
    assert_eq!(r.cell(BOAT_DOWN), Cell::new(21, 18));
    assert!(r.cells[BOAT_DOWN.index() as usize].contains(&Cell::new(21, 8)));
    // Asked to go onto the bank, it cannot: the route leads to the water cell
    // nearest the spot, the river's edge cell (20, 14), where it stands in the
    // middle of the cell with its order kept and the mark that the destination
    // cannot be reached.
    let goal = mid(10, 14);
    assert_eq!(r.world.order_of(BOAT_BANK), Some(Order::Move(goal)));
    assert_eq!(r.world.waypoint_of(BOAT_BANK), None);
    assert_eq!(r.cell(BOAT_BANK), Cell::new(20, 14));
    assert_eq!(r.pos(BOAT_BANK), mid(20, 14));
    let route = r.world.route_of(BOAT_BANK).unwrap();
    assert_eq!(route.status, RouteStatus::Stranded);
    assert!(route.unreachable);
    assert_eq!(route.goal, Some(Cell::new(20, 14)));
}

// --- blocked by water and rock ---

#[test]
fn land_sent_at_the_water_takes_the_bridge() {
    // Asked to go to the far bank along a lane that runs into the river, the
    // land classes go up to the bridge in row 8, cross it, and come back down to
    // the spot: the destination can be reached, so nobody stops at the bank.
    let r = run();
    let goal = Cell::new(27, 10);
    for class in [FOOT, WHEELED, TRACKED] {
        let id = unit(WATER, class);
        assert!(r.arrived(id).is_some(), "class {class} must arrive");
        assert_eq!(r.cell(id), goal, "class {class}");
        assert_eq!(r.world.order_of(id), Some(Order::Idle), "class {class}");
        let seen = &r.cells[id.index() as usize];
        for x in 20..=23 {
            assert!(
                seen.contains(&Cell::new(x, 8)),
                "class {class} never stood on the bridge cell ({x}, 8)"
            );
        }
        // And it was never in the water of the river.
        assert!(
            seen.iter().all(|c| !(20..=23).contains(&c.x) || c.y == 8),
            "class {class} left the bridge in the river"
        );
    }
}

#[test]
fn hover_crosses_the_river_at_the_speed_of_the_water_cell() {
    let r = run();
    let id = unit(WATER, HOVER);
    assert_eq!(r.cell(id), Cell::new(27, 10));
    assert_eq!(r.world.order_of(id), Some(Order::Idle));
    // It is slower exactly while it is in the river (the hover class has 800 on
    // water, which is the amphibious transport's 1.2 cells per second against
    // its 1.5 on land): four cells of water take 4 / (step * 0.8) ticks, the
    // three grass cells after it 3 / step.
    let cells = &r.cells[id.index() as usize];
    let ticks_in = |columns: std::ops::RangeInclusive<u32>| {
        cells.iter().filter(|c| columns.contains(&c.x)).count() as i64
    };
    let expect = |cells: i64, permille: u16| {
        let step = r.step_of(id, permille).raw();
        (cells * 65_536 + step - 1) / step
    };
    let (river, grass) = (ticks_in(20..=23), ticks_in(24..=26));
    assert!(
        (river - expect(4, 800)).abs() <= 2,
        "{river} ticks in the river"
    );
    assert!(
        (grass - expect(3, 1000)).abs() <= 2,
        "{grass} ticks on the grass"
    );
    assert!(
        river * 3 > grass * 4 + 20,
        "the river is slower than the grass"
    );
}

#[test]
fn everyone_but_air_goes_round_the_rock_wall() {
    let r = run();
    for class in [FOOT, WHEELED, TRACKED, HOVER] {
        let id = unit(ROCK, class);
        assert!(r.arrived(id).is_some(), "class {class} must arrive");
        assert_eq!(r.cell(id), Cell::new(35, 12), "class {class}");
        assert_eq!(r.world.order_of(id), Some(Order::Idle), "class {class}");
        // The wall is the two cells (30, 12) and (31, 12): it passed above or
        // below them.
        let seen = &r.cells[id.index() as usize];
        assert!(
            seen.iter().any(|c| (30..=31).contains(&c.x) && c.y != 12),
            "class {class} did not go round the wall"
        );
    }
    // The drone flies over the river and the wall on row 16 and arrives.
    assert_eq!(r.world.order_of(DRONE), Some(Order::Idle));
    assert_eq!(r.cell(DRONE), Cell::new(37, 16));
    let seen = &r.cells[DRONE.index() as usize];
    assert!(seen.contains(&Cell::new(21, 16)), "over the river");
    assert!(seen.contains(&Cell::new(30, 16)) && seen.contains(&Cell::new(31, 16)));
}

#[test]
fn a_stranded_unit_stays_where_its_route_ended() {
    // The gunboat whose destination cannot be reached does not creep, wander off
    // or give up: the position at tick 500 is the position at the end, and the
    // order is kept.
    let r = run();
    assert_eq!(r.at_500[BOAT_BANK.index() as usize], r.pos(BOAT_BANK));
    assert!(matches!(r.world.order_of(BOAT_BANK), Some(Order::Move(_))));
    assert!(r.ticks > 500);
}

// --- never on ground the class cannot enter ---

#[test]
fn no_unit_ever_ends_a_tick_on_ground_its_class_cannot_enter() {
    let r = run();
    for slot in 0..27_u32 {
        let id = EntityId::new(slot, 0);
        let class = r.world.stats_of(id).unwrap().movement_class.unwrap();
        for (tick, cell) in r.cells[slot as usize].iter().enumerate() {
            assert!(
                r.world.passable(class, *cell),
                "slot {slot} stood on {cell} (class {class}) after tick {tick}"
            );
        }
    }
}

#[test]
fn the_costs_come_from_the_frozen_definitions() {
    // The world's table has all six classes and the speeds of the frozen file.
    let r = run();
    assert_eq!(r.world.movement_table().len(), 6);
    let tank = r.world.stats_of(unit(GRASS, TRACKED)).unwrap();
    assert_eq!(tank.speed_per_tick, per_tick(Fx::from_int(2)));
}

#[test]
fn the_run_is_reproducible() {
    let (a, b) = (run(), run());
    assert_eq!(a.world.checksum_history(), b.world.checksum_history());
    assert_eq!(a.arrived, b.arrived);
}
