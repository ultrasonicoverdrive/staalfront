//! Formations (WP4.6), through the public API: one move order that sends a group of
//! units across the same ground makes them a formation (the shape they stand in, at the
//! pace of the slowest), the shape breaks up in narrow places and forms up again, and the
//! formation is let go at the goal. The rules are in `docs/rules/formations.md`; the
//! headless scenario `formation_move` has the long crossing.

use sf_math::{Facing, Fx, Vec2};
use sf_sim::{
    ClassCosts, Command, ConfigError, Cost, EntityId, FormationConfig, FormationState, Kind,
    MemberMode, MovementTable, Order, PlayerId, Terrain, TerrainDef, TerrainFlags, TerrainId,
    UnitStats, UnitTable, World, WorldConfig, per_tick,
};

const GRASS: TerrainId = TerrainId(0);
const WATER: TerrainId = TerrainId(2);
const ROCK: TerrainId = TerrainId(3);

// Movement classes (permanent kind numbers).
const WALKER: u16 = 0;
const BOAT: u16 = 1;
const FLYER: u16 = 2;
const HOVER: u16 = 3;
const WHEELED: u16 = 4;

// Unit kinds.
const SOLDIER: Kind = Kind(1); // walker, 0.7 cells a second, 0.4 across
const TANK: Kind = Kind(2); // walker, 2.0, 1.2 across
const TRUCK: Kind = Kind(3); // wheeled, 2.5, 1.2 across
const GUNBOAT: Kind = Kind(4);
const FERRY: Kind = Kind(5);
const PLANE: Kind = Kind(6);

const MAP: (u32, u32) = (96, 40);

/// Grass, a wall of rock in columns 40 and 41 with a gap in rows 18 to 20 (three cells
/// wide, two long), and a lake in columns 8 to 30, rows 30 to 38.
fn layer() -> Vec<u8> {
    (0..MAP.1)
        .flat_map(|y| {
            (0..MAP.0).map(move |x| {
                match (x, y) {
                    (40..=41, 18..=20) => GRASS,
                    (40..=41, _) => ROCK,
                    (8..=30, 30..=38) => WATER,
                    _ => GRASS,
                }
                .0
            })
        })
        .collect()
}

fn defs() -> Vec<TerrainDef> {
    let open = |id| TerrainDef {
        id,
        flags: TerrainFlags::OPEN,
    };
    let blocked = |id| TerrainDef {
        id,
        flags: TerrainFlags::default(),
    };
    vec![
        open(GRASS),
        open(TerrainId(1)),
        blocked(WATER),
        blocked(ROCK),
        open(TerrainId(4)),
        open(TerrainId(5)),
    ]
}

fn costs() -> MovementTable {
    let class = |class, list: [Cost; 6]| ClassCosts {
        class,
        costs: (0_u8..6).map(TerrainId).zip(list).collect(),
    };
    let x = Cost::Impassable;
    let s = Cost::Speed;
    MovementTable::new(vec![
        class(WALKER, [s(1000), s(1000), x, x, s(1000), s(1000)]),
        class(BOAT, [x, x, s(1000), x, x, s(1000)]),
        class(FLYER, [s(1000); 6]),
        class(HOVER, [s(1000), s(1000), s(800), x, s(1000), s(1000)]),
        class(WHEELED, [s(1000), s(1000), x, x, s(1000), s(1000)]),
    ])
    .unwrap()
}

fn table() -> UnitTable {
    let mobile = |kind: Kind, class: u16, speed_tenths: i64, size_tenths: i64| UnitStats {
        kind,
        hp: 100,
        movement_class: Some(class),
        speed_per_tick: per_tick(Fx::from_ratio(speed_tenths, 10)),
        turn_per_tick: 255,
        size: Fx::from_ratio(size_tenths, 10),
        footprint: None,
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    };
    UnitTable::new(vec![
        mobile(SOLDIER, WALKER, 7, 4),
        mobile(TANK, WALKER, 20, 12),
        mobile(TRUCK, WHEELED, 25, 12),
        mobile(GUNBOAT, BOAT, 20, 12),
        mobile(FERRY, HOVER, 15, 12),
        mobile(PLANE, FLYER, 60, 4),
    ])
    .unwrap()
}

fn config() -> WorldConfig {
    WorldConfig::new(46).with_map_size(MAP.0, MAP.1)
}

fn world() -> World {
    let terrain = Terrain::from_layer(MAP.0, MAP.1, &layer(), &defs()).unwrap();
    World::with_terrain(config(), terrain)
        .unwrap()
        .with_units(table())
        .unwrap()
        .with_movement(costs())
        .unwrap()
}

fn mid(x: i32, y: i32) -> Vec2 {
    Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF)
}

fn spawn(w: &mut World, kind: Kind, x: i32, y: i32) -> EntityId {
    let before = w.slot_count();
    let tick = w.tick();
    w.schedule(
        tick,
        PlayerId(0),
        Command::SpawnUnit {
            kind,
            pos: mid(x, y),
            owner: PlayerId(0),
        },
    )
    .unwrap();
    let report = w.step();
    assert_eq!(report.commands_ignored, 0, "{kind:?} at ({x}, {y})");
    EntityId::new(before as u32, 0)
}

fn command(w: &mut World, c: Command) {
    let tick = w.tick();
    w.schedule(tick, PlayerId(0), c).unwrap();
}

fn order(w: &mut World, ids: &[EntityId], order: Order) {
    command(
        w,
        Command::Order {
            ids: ids.to_vec(),
            order,
            queued: false,
        },
    );
}

fn go(w: &mut World, ids: &[EntityId], to: (i32, i32)) {
    order(w, ids, Order::Move(mid(to.0, to.1)));
}

fn run(w: &mut World, ticks: u32) {
    for _ in 0..ticks {
        w.step();
    }
}

fn pos(w: &World, id: EntityId) -> Vec2 {
    w.get(id).unwrap().pos
}

/// A number of cells with a fraction, as thousandths: `fx(1250)` is a cell and a quarter.
fn fx(thousandths: i64) -> Fx {
    Fx::from_ratio(thousandths, 1000)
}

/// A block of `n` units in a row, one cell apart, starting at `(x, y)`.
fn row(w: &mut World, kinds: &[Kind], x: i32, y: i32) -> Vec<EntityId> {
    kinds
        .iter()
        .enumerate()
        .map(|(k, &kind)| spawn(w, kind, x + k as i32 * 2, y))
        .collect()
}

/// Five walkers of mixed speed: two soldiers, two tanks and a truck (a different class that
/// crosses the same ground).
fn mixed_five(w: &mut World, x: i32, y: i32) -> Vec<EntityId> {
    row(w, &[SOLDIER, TANK, SOLDIER, TRUCK, TANK], x, y)
}

// ---- who forms a formation --------------------------------------------------------------

#[test]
fn a_move_order_for_a_group_makes_one_formation_with_one_leader() {
    let mut w = world();
    let group = mixed_five(&mut w, 6, 10);
    go(&mut w, &group, (30, 10));
    w.step();
    let views: Vec<_> = group.iter().map(|&u| w.formation_of(u).unwrap()).collect();
    assert!(views.iter().all(|v| v.serial == views[0].serial));
    assert_eq!(views.iter().filter(|v| v.leader).count(), 1);
    assert!(views.iter().all(|v| v.members == 5));
    assert_eq!(w.formation_stats().formed, 1);
    // The speeds are mixed, the ground is the same: foot and wheeled units are one family.
    let leader = views.iter().find(|v| v.leader).unwrap();
    assert!(group.contains(&leader.leader_id));
}

#[test]
fn single_orders_and_the_other_orders_keep_their_behaviour() {
    let mut w = world();
    let group = mixed_five(&mut w, 6, 10);
    // One unit alone; a pair is below the minimum of three.
    go(&mut w, &group[..1], (30, 10));
    go(&mut w, &group[1..3], (30, 12));
    w.step();
    assert!(group.iter().all(|&u| w.formation_of(u).is_none()));
    // Attack-move, hold, stop and scatter never form a formation.
    order(&mut w, &group, Order::AttackMove(mid(30, 10)));
    w.step();
    assert!(group.iter().all(|&u| w.formation_of(u).is_none()));
    order(&mut w, &group, Order::HoldPosition);
    w.step();
    assert!(group.iter().all(|&u| w.formation_of(u).is_none()));
    command(&mut w, Command::Stop { ids: group.clone() });
    w.step();
    order(&mut w, &group, Order::Scatter);
    w.step();
    assert!(group.iter().all(|&u| w.formation_of(u).is_none()));
    assert_eq!(w.formation_stats().formed, 0);
    // A queued move behind another order is not a start: nothing forms until it begins.
    order(
        &mut w,
        &group,
        Order::Guard(sf_sim::GuardTarget::Point(mid(12, 12))),
    );
    command(
        &mut w,
        Command::Order {
            ids: group.clone(),
            order: Order::Move(mid(30, 10)),
            queued: true,
        },
    );
    w.step();
    assert!(group.iter().all(|&u| w.formation_of(u).is_none()));
}

#[test]
fn formations_can_be_turned_off_and_are_checked_when_given() {
    let mut w = world().with_formation(FormationConfig::OFF).unwrap();
    let group = mixed_five(&mut w, 6, 10);
    go(&mut w, &group, (30, 10));
    run(&mut w, 5);
    assert!(group.iter().all(|&u| w.formation_of(u).is_none()));
    assert!(!w.formation_config().enabled);
    // The numbers have ranges, and the config comes before the world starts.
    let c = FormationConfig::DEFAULT;
    for bad in [
        FormationConfig { min_units: 1, ..c },
        FormationConfig { max_units: 2, ..c },
        FormationConfig {
            spacing_permille: 900,
            ..c
        },
        FormationConfig {
            pace_permille: 400,
            ..c
        },
        FormationConfig {
            catchup_permille: 800,
            ..c
        },
        FormationConfig {
            reform_tenths: 0,
            ..c
        },
        FormationConfig {
            patience_ticks: 5,
            ..c
        },
    ] {
        assert_eq!(
            world().with_formation(bad).err(),
            Some(ConfigError::Formation)
        );
    }
    let mut started = world();
    started.step();
    assert_eq!(
        started.with_formation(c).err(),
        Some(ConfigError::Formation)
    );
}

#[test]
fn units_that_cross_other_ground_do_not_join_and_a_group_beyond_the_limit_is_no_formation() {
    let mut w = world();
    // Three walkers and a truck on the grass, three gunboats on the lake, two hovercraft and a
    // plane: the grass family is four, the boats three, the hovercraft too few.
    let land = row(&mut w, &[SOLDIER, TANK, TRUCK, SOLDIER], 6, 10);
    let boats: Vec<EntityId> = (0..3)
        .map(|k| spawn(&mut w, GUNBOAT, 10 + k * 3, 34))
        .collect();
    let others = vec![
        spawn(&mut w, FERRY, 6, 20),
        spawn(&mut w, FERRY, 8, 20),
        spawn(&mut w, PLANE, 10, 20),
    ];
    let everyone: Vec<EntityId> = land.iter().chain(&boats).chain(&others).copied().collect();
    // One order for everybody to a point on the grass: the boats cannot get there.
    go(&mut w, &everyone, (30, 10));
    w.step();
    let serial = w.formation_of(land[0]).unwrap().serial;
    assert!(
        land.iter()
            .all(|&u| w.formation_of(u).unwrap().serial == serial)
    );
    assert!(boats.iter().all(|&u| w.formation_of(u).is_none()));
    assert!(others.iter().all(|&u| w.formation_of(u).is_none()));
    // The boats sent to a point in the lake do form their own.
    go(&mut w, &boats, (28, 34));
    w.step();
    let fleet = w.formation_of(boats[0]).unwrap();
    assert_ne!(fleet.serial, serial);
    assert_eq!(fleet.members, 3);
    // More than the limit is a crowd.
    let mut w = world()
        .with_formation(FormationConfig {
            max_units: 4,
            ..FormationConfig::DEFAULT
        })
        .unwrap();
    let five = mixed_five(&mut w, 6, 10);
    go(&mut w, &five, (30, 10));
    w.step();
    assert!(five.iter().all(|&u| w.formation_of(u).is_none()));
    // The limit counts the units the order sends, not the units of a family: three boats are a
    // fleet of their own, but sent with five walkers (eight in all, over the limit of six) they
    // are part of a crowd, and nobody forms up. Sent without the walkers they do.
    let mut w = world()
        .with_formation(FormationConfig {
            max_units: 6,
            ..FormationConfig::DEFAULT
        })
        .unwrap();
    let walkers = mixed_five(&mut w, 6, 10);
    let boats: Vec<EntityId> = (0..3)
        .map(|k| spawn(&mut w, GUNBOAT, 10 + k * 3, 34))
        .collect();
    let crowd: Vec<EntityId> = walkers.iter().chain(&boats).copied().collect();
    go(&mut w, &crowd, (28, 34));
    w.step();
    assert!(crowd.iter().all(|&u| w.formation_of(u).is_none()));
    go(&mut w, &boats, (28, 34));
    w.step();
    assert_eq!(w.formation_of(boats[0]).map(|f| f.members), Some(3));
}

// ---- the shape --------------------------------------------------------------------------

/// The distance between two slots of a formation, in cells.
fn slot_gap(w: &World, a: EntityId, b: EntityId) -> Fx {
    (w.formation_of(a).unwrap().slot - w.formation_of(b).unwrap().slot).length()
}

#[test]
fn the_slots_are_the_layout_of_the_group_on_a_lattice_with_room_for_the_bodies() {
    let mut w = world();
    // A line of four tanks, two cells apart, one across the other: a column across the way.
    let tanks: Vec<EntityId> = (0..4).map(|k| spawn(&mut w, TANK, 10, 6 + k * 2)).collect();
    go(&mut w, &tanks, (60, 8));
    w.step();
    let spacing = (Fx::from_ratio(12, 10) + Fx::from_ratio(1, 16))
        .mul_int(5)
        .div_int(4);
    for a in 0..tanks.len() {
        for b in a + 1..tanks.len() {
            let gap = slot_gap(&w, tanks[a], tanks[b]);
            assert!(
                gap >= spacing - fx(10),
                "slots {a} and {b} are {gap:?} apart"
            );
        }
        // Heading east, the line runs across it: all the slots lie in one column.
        let v = w.formation_of(tanks[a]).unwrap();
        assert_eq!(v.heading, Facing::PLUS_X);
        assert!((v.slot.x - pos(&w, tanks[0]).x).abs() < fx(100));
    }
    // The neighbours in the line keep their order along it.
    let ys: Vec<Fx> = tanks
        .iter()
        .map(|&t| w.formation_of(t).unwrap().slot.y)
        .collect();
    assert!(ys.windows(2).all(|p| p[0] < p[1]), "{ys:?}");
}

#[test]
fn a_scattered_group_is_pulled_in_and_a_tight_one_is_given_room() {
    let mut w = world();
    // Four soldiers 30 cells apart: the lattice pulls them in to a few cells.
    let far = [(5, 5), (35, 5), (5, 35), (35, 25)];
    let scattered: Vec<EntityId> = far
        .iter()
        .map(|&(x, y)| spawn(&mut w, SOLDIER, x, y))
        .collect();
    go(&mut w, &scattered, (80, 10));
    w.step();
    let mut widest = Fx::ZERO;
    for &a in &scattered {
        for &b in &scattered {
            widest = widest.max(slot_gap(&w, a, b));
        }
    }
    assert!(
        widest < Fx::from_int(8),
        "the slots of the scattered group are {widest} apart"
    );
    let extent = w.formation_of(scattered[0]).unwrap().extent;
    assert!(extent < Fx::from_int(6), "{extent:?}");
    // Four tanks on top of each other, a cell apart, get the room their bodies need.
    let mut w = world();
    let block: Vec<EntityId> = (0..4)
        .map(|k| spawn(&mut w, TANK, 10 + k % 2, 10 + k / 2))
        .collect();
    go(&mut w, &block, (80, 10));
    w.step();
    for a in 0..4 {
        for b in a + 1..4 {
            assert!(
                slot_gap(&w, block[a], block[b]) >= fx(1200),
                "tanks {a} and {b}"
            );
        }
    }
}

#[test]
fn the_offsets_turn_with_the_heading_of_the_way() {
    // A wall with a gap lies between the group and its goal, so the leader's way bends: south
    // to the gap, east through it, north again on the other side.
    let mut w = world();
    let group = row(&mut w, &[TANK, TANK, TANK, TANK], 20, 4);
    go(&mut w, &group, (60, 4));
    w.step();
    let start = w.formation_of(group[0]).unwrap();
    assert_eq!(
        start.heading,
        Facing::PLUS_X,
        "the first heading is toward the goal"
    );
    let leader = start.leader_id;
    let other = group.iter().copied().find(|&u| u != leader).unwrap();
    // Where the other member's slot lies, as an angle from the leader's slot, minus the
    // heading: the shape, which does not turn on its own.
    let shape = |w: &World| {
        let (l, o) = (
            w.formation_of(leader).unwrap(),
            w.formation_of(other).unwrap(),
        );
        let a = (o.slot - l.slot).facing().unwrap().raw();
        a.wrapping_sub(l.heading.raw())
    };
    let first_shape = shape(&w);
    let mut headings = vec![start.heading];
    let mut shapes = vec![first_shape];
    for _ in 0..1500 {
        w.step();
        let Some(v) = w.formation_of(leader) else {
            break;
        };
        headings.push(v.heading);
        shapes.push(shape(&w));
    }
    // The heading never jumps (one step at a time), and it did turn: toward the gap and away.
    assert!(headings.windows(2).all(|p| p[0].abs_diff(p[1]) <= 1));
    let signed = |h: &Facing| {
        let r = i16::from(h.raw());
        if r > 128 { r - 256 } else { r }
    };
    let lo = headings.iter().map(signed).min().unwrap();
    let hi = headings.iter().map(signed).max().unwrap();
    assert!(hi - lo >= 10, "the heading ranged only from {lo} to {hi}");
    // And the shape turned with it: the angle between the slots and the heading stayed what it
    // was (to a step of rounding).
    for (t, s) in shapes.iter().enumerate() {
        let d = s.wrapping_sub(first_shape);
        assert!(
            d <= 2 || d >= 254,
            "tick {t}: the shape turned by {d} steps"
        );
    }
}

// ---- the pace ---------------------------------------------------------------------------

#[test]
fn the_leader_goes_at_the_pace_of_the_slowest_and_the_others_use_their_own_speed_to_keep_up() {
    let mut w = world();
    let group = mixed_five(&mut w, 6, 10);
    go(&mut w, &group, (80, 10));
    // The pace is the slowest member's speed while every member is on its slot or ahead of it,
    // and `pace_permille` (900) of it while one is behind and has to catch up: never more.
    let slowest = per_tick(Fx::from_ratio(7, 10));
    let kept_back = slowest.mul_int(900).div_int(1000);
    let near = |a: Fx, b: Fx| (a - b).abs() <= Fx::from_raw(2);
    let (mut at_full, mut at_kept_back) = (0, 0);
    let mut fastest = Fx::ZERO;
    let mut last: Vec<Vec2> = group.iter().map(|&u| pos(&w, u)).collect();
    let mut leader_moved = 0;
    for t in 0..550 {
        w.step();
        for (k, &u) in group.iter().enumerate() {
            let now = pos(&w, u);
            let step = (now - last[k]).length();
            let v = w.formation_of(u).unwrap();
            let own = w.stats_of(u).unwrap().speed_per_tick;
            // Nobody ever goes faster than its own speed, whatever it does to catch up.
            assert!(
                step <= own + Fx::from_raw(64),
                "unit {k} stepped {step:?} > {own:?}"
            );
            if v.leader {
                // Form up first (150 ticks), then watch a stretch of open ground.
                if t >= 150 {
                    fastest = fastest.max(step);
                    if step > Fx::ZERO {
                        leader_moved += 1;
                    }
                }
                assert!(
                    near(v.pace, slowest) || near(v.pace, kept_back),
                    "tick {t}: the pace {:?} is neither {slowest:?} nor {kept_back:?}",
                    v.pace
                );
                at_full += u32::from(near(v.pace, slowest));
                at_kept_back += u32::from(near(v.pace, kept_back));
            }
            last[k] = now;
        }
    }
    // The leader never outpaces the slowest member (a hair for the rounding of the steps).
    assert!(fastest <= slowest + Fx::from_raw(2), "{fastest:?}");
    assert!(
        leader_moved > 100,
        "the leader moved on {leader_moved} ticks"
    );
    // Both happen: the group forms up (members behind their slots, the pace kept back) and
    // then marches at the speed of its slowest.
    assert!(
        at_full > 100 && at_kept_back > 0,
        "{at_full} ticks at the slowest's speed, {at_kept_back} kept back"
    );
}

#[test]
fn on_open_ground_every_member_stays_near_its_slot_and_the_group_is_not_strung_out() {
    let mut w = world();
    let group = mixed_five(&mut w, 6, 10);
    go(&mut w, &group, (80, 10));
    run(&mut w, 200);
    let reform = Fx::from_int(3);
    let mut worst = Fx::ZERO;
    let mut ticks_in_shape = 0;
    let mut ticks = 0;
    for _ in 0..800 {
        w.step();
        let Some(leader) = group
            .iter()
            .copied()
            .find(|&u| w.formation_of(u).is_some_and(|v| v.leader))
        else {
            break;
        };
        let lead_x = pos(&w, leader).x;
        if lead_x > Fx::from_int(36) {
            break;
        }
        ticks += 1;
        let mut all_near = true;
        for &u in &group {
            let v = w.formation_of(u).unwrap();
            let off = (pos(&w, u) - v.slot).length();
            worst = worst.max(off);
            all_near &= off <= reform;
        }
        if all_near {
            ticks_in_shape += 1;
        }
    }
    assert!(ticks > 300, "{ticks} ticks in the open field");
    assert!(
        ticks_in_shape * 100 >= ticks * 95,
        "{ticks_in_shape} of {ticks} ticks in shape, worst {worst}"
    );
    // Pace: after 800 ticks the soldiers, at 0.7 cells a second, have come as far as the group.
    let mean_x = group
        .iter()
        .fold(Fx::ZERO, |sum, &u| sum + pos(&w, u).x)
        .div_int(5);
    let soldier_x = pos(&w, group[0]).x;
    assert!(
        (mean_x - soldier_x).abs() < Fx::from_int(6),
        "mean {mean_x}, soldier {soldier_x}"
    );
}

// ---- narrow places ----------------------------------------------------------------------

#[test]
fn a_gap_breaks_the_formation_up_and_it_forms_up_again_beyond_it() {
    let mut w = world();
    let group = mixed_five(&mut w, 6, 19);
    go(&mut w, &group, (85, 19));
    let mut loose_seen = false;
    let mut flow_seen = false;
    let mut formed_after = false;
    for t in 0..4000 {
        w.step();
        let Some(v) = group.iter().find_map(|&u| w.formation_of(u)) else {
            break;
        };
        loose_seen |= v.loose;
        flow_seen |= group.iter().any(|&u| {
            w.formation_of(u)
                .is_some_and(|v| v.mode == MemberMode::Flow)
        });
        // Past the wall, on open ground, and every member back at its slot.
        let leader = group
            .iter()
            .copied()
            .find(|&u| w.formation_of(u).unwrap().leader)
            .unwrap();
        if pos(&w, leader).x > Fx::from_int(52)
            && group.iter().all(|&u| {
                let v = w.formation_of(u).unwrap();
                v.mode == MemberMode::Direct || v.leader
            })
            && group
                .iter()
                .all(|&u| (pos(&w, u) - w.formation_of(u).unwrap().slot).length() < Fx::ONE)
        {
            formed_after = true;
            let _ = t;
            break;
        }
    }
    assert!(loose_seen, "the formation never let go at the gap");
    assert!(
        flow_seen,
        "nobody followed the order's own way through the gap"
    );
    assert!(
        formed_after,
        "the formation never formed up again beyond the gap"
    );
}

#[test]
fn everybody_arrives_through_the_gap_and_the_formation_is_let_go_at_the_goal() {
    let mut w = world();
    let group = mixed_five(&mut w, 6, 19);
    go(&mut w, &group, (85, 19));
    let mut released_at = None;
    for t in 0..5000_u32 {
        w.step();
        if released_at.is_none() && group.iter().all(|&u| w.formation_of(u).is_none()) {
            released_at = Some(t);
        }
        if group.iter().all(|&u| w.order_of(u) == Some(Order::Idle)) {
            break;
        }
    }
    let released = released_at.expect("the formation was let go");
    assert!(
        group.iter().all(|&u| w.order_of(u) == Some(Order::Idle)),
        "all arrive"
    );
    assert_eq!(w.formation_stats().released, 1);
    assert!(w.tick().0 > u64::from(released));
    // And nobody is stuck at the end.
    for &u in &group {
        assert_eq!(w.stuck_ticks_of(u), Some(0));
        assert!(
            (pos(&w, u) - mid(85, 19)).length() < Fx::from_int(4),
            "{u:?} ended at {:?}",
            pos(&w, u)
        );
    }
}

// ---- leaving a formation ----------------------------------------------------------------

#[test]
fn another_order_takes_a_unit_out_and_a_leader_that_goes_is_replaced() {
    let mut w = world();
    let group = mixed_five(&mut w, 6, 10);
    go(&mut w, &group, (80, 10));
    run(&mut w, 30);
    let view = w.formation_of(group[0]).unwrap();
    let leader = view.leader_id;
    // The leader gets another order: the others go on with a new one.
    go(&mut w, &[leader], (6, 30));
    run(&mut w, 3);
    assert!(w.formation_of(leader).is_none());
    let rest: Vec<EntityId> = group.iter().copied().filter(|&u| u != leader).collect();
    let new_leader = w.formation_of(rest[0]).unwrap().leader_id;
    assert_ne!(new_leader, leader);
    assert!(
        rest.iter()
            .all(|&u| w.formation_of(u).unwrap().members == 4)
    );
    // A stop takes a unit out too; with fewer than two members left the formation is gone.
    command(
        &mut w,
        Command::Stop {
            ids: rest[..3].to_vec(),
        },
    );
    run(&mut w, 3);
    assert!(rest.iter().all(|&u| w.formation_of(u).is_none()));
    assert!(w.formation_stats().dissolved >= 1);
    // A unit that dies leaves too.
    let mut w = world();
    let group = mixed_five(&mut w, 6, 10);
    go(&mut w, &group, (80, 10));
    run(&mut w, 10);
    let doomed = w.formation_of(group[1]).unwrap().leader_id;
    command(&mut w, Command::Despawn { id: doomed });
    run(&mut w, 3);
    let left: Vec<EntityId> = group.iter().copied().filter(|&u| u != doomed).collect();
    assert!(
        left.iter()
            .all(|&u| w.formation_of(u).unwrap().members == 4)
    );
    assert_ne!(w.formation_of(left[0]).unwrap().leader_id, doomed);
}

/// Four tanks and a soldier on open ground, the soldier shut in a pen of four tanks on
/// hold position (a plus of cells, a body's width too narrow to get out of anywhere): it
/// cannot get to its slot, whatever it tries. Returns the world (with a short patience), the
/// four tanks of the group and the soldier.
fn pen_world() -> (World, Vec<EntityId>, EntityId) {
    let mut w = world()
        .with_formation(FormationConfig {
            patience_ticks: 40,
            ..FormationConfig::DEFAULT
        })
        .unwrap();
    let group = row(&mut w, &[TANK, TANK, TANK, TANK], 6, 4);
    let soldier = spawn(&mut w, SOLDIER, 10, 14);
    let pen: Vec<EntityId> = [(9, 14), (11, 14), (10, 13), (10, 15)]
        .iter()
        .map(|&(x, y)| spawn(&mut w, TANK, x, y))
        .collect();
    order(&mut w, &pen, Order::HoldPosition);
    w.step();
    let mut all = group.clone();
    all.push(soldier);
    go(&mut w, &all, (80, 4));
    (w, group, soldier)
}

#[test]
fn a_straggler_that_cannot_come_holds_the_leader_for_its_patience_and_then_the_formation_goes_on_without_it()
 {
    let (mut w, group, soldier) = pen_world();
    run(&mut w, 5);
    assert!(
        w.formation_of(soldier).is_some(),
        "the soldier starts in the formation"
    );
    let leader = w.formation_of(group[0]).unwrap().leader_id;
    // The leader waits (on open ground) for the soldier who cannot come; the patience of 40
    // ticks runs out at full speed because the soldier is not getting closer, the formation
    // is broken and the leader goes on; the soldier leaves after the same patience.
    let (mut broke_at, mut left_at, mut leader_waited) = (None, None, 0);
    for t in 0..600_u32 {
        w.step();
        let stats = w.formation_stats();
        leader_waited += u32::from(w.formation_of(leader).is_some_and(|v| v.waiting));
        if broke_at.is_none() && stats.breaks > 0 {
            broke_at = Some(t);
        }
        if left_at.is_none() && stats.expelled > 0 {
            left_at = Some(t);
        }
    }
    let broke = broke_at.expect("the formation broke: the leader's patience ran out");
    let left = left_at.expect("the straggler that never got closer left the formation");
    assert!(
        leader_waited > 20,
        "the leader waited only {leader_waited} ticks"
    );
    assert!(broke < 200, "the formation broke on tick {broke}");
    assert!(left < 300, "the straggler left on tick {left}");
    assert!(
        w.formation_of(soldier).is_none(),
        "the soldier is out of the formation"
    );
    // The tanks went on, in formation, while the soldier is still penned in.
    assert!(w.formation_stats().expelled == 1);
    assert!(
        pos(&w, leader).x > Fx::from_int(20),
        "the leader is at {:?}",
        pos(&w, leader)
    );
    assert!(
        pos(&w, soldier).x < Fx::from_int(12),
        "the soldier is shut in"
    );
    assert!(
        group
            .iter()
            .all(|&t| w.formation_of(t).is_some_and(|v| v.members == 4))
    );
}

#[test]
fn a_broken_formation_is_whole_again_only_after_a_calm_stretch_and_waits_again_only_then() {
    // Ten tanks in a column across the way and one soldier shut in: stragglers are 1 of 11,
    // a tenth, so the formation is whole again when it is calm (60 ticks) and not before.
    let mut w = world()
        .with_formation(FormationConfig {
            patience_ticks: 40,
            ..FormationConfig::DEFAULT
        })
        .unwrap();
    let tanks: Vec<EntityId> = (0..10).map(|k| spawn(&mut w, TANK, 6, 2 + k * 2)).collect();
    let soldier = spawn(&mut w, SOLDIER, 10, 26);
    let pen: Vec<EntityId> = [(9, 26), (11, 26), (10, 25), (10, 27)]
        .iter()
        .map(|&(x, y)| spawn(&mut w, TANK, x, y))
        .collect();
    order(&mut w, &pen, Order::HoldPosition);
    w.step();
    let mut all = tanks.clone();
    all.push(soldier);
    go(&mut w, &all, (80, 10));
    let mut states = Vec::new();
    for _ in 0..700 {
        w.step();
        if let Some(v) = w.formation_of(tanks[0]) {
            states.push((v.state, v.waiting));
        }
    }
    // Once broken it stays broken until it has been calm for 60 ticks in a row: a broken
    // stretch is never shorter than that (the first version flipped back the next tick).
    let mut runs: Vec<usize> = Vec::new();
    let mut length = 0;
    for &(state, _) in &states {
        if state == FormationState::Broken {
            length += 1;
        } else if length > 0 {
            runs.push(length);
            length = 0;
        }
    }
    assert!(w.formation_stats().breaks >= 1);
    assert!(runs.iter().all(|&r| r >= 60), "broken stretches: {runs:?}");
    // And it did not wait for ever: the patience of 40 ticks bounds what the leader waits
    // for a straggler that does not come, however often the count of stragglers touches zero.
    let waited = states.iter().filter(|s| s.1).count();
    assert!(
        waited <= 40 * (1 + w.formation_stats().reforms as usize) + 40,
        "{waited}"
    );
}

#[test]
fn the_heading_follows_the_leaders_own_steps_and_not_the_pushes_it_gets_while_it_waits() {
    // Twelve tanks packed east of the wall and a soldier far to the west: the leader waits
    // for the soldier, with its members pressing on it. A push is not a direction of travel.
    let mut w = world();
    let tanks: Vec<EntityId> = (0..12)
        .map(|k| spawn(&mut w, TANK, 60 + k % 4, 18 + k / 4))
        .collect();
    let soldier = spawn(&mut w, SOLDIER, 6, 19);
    let mut all = tanks.clone();
    all.push(soldier);
    go(&mut w, &all, (90, 19));
    w.step();
    let first = w.formation_of(tanks[0]).unwrap();
    let leader = first.leader_id;
    let mut waited = 0;
    // A tank of nobody's group walks west through the waiting formation, and pushes the
    // leader and its members west as it goes.
    let walker = spawn(&mut w, TANK, 80, 19);
    go(&mut w, &[walker], (50, 19));
    for _ in 0..700 {
        w.step();
        let Some(v) = w.formation_of(leader) else {
            break;
        };
        waited += u32::from(v.waiting);
        // The way is east, a few steps either way at most; never half a turn.
        let off = v.heading.abs_diff(first.heading);
        assert!(
            off <= 24,
            "the heading turned {off} steps (to {:?})",
            v.heading
        );
    }
    assert!(waited > 50, "the leader waited only {waited} ticks");
}

// ---- state and checksum -----------------------------------------------------------------

#[test]
fn a_formation_is_in_the_checksum_and_a_run_is_reproducible() {
    let make = |on: bool| {
        let mut w = if on {
            world()
        } else {
            world().with_formation(FormationConfig::OFF).unwrap()
        };
        let group = mixed_five(&mut w, 6, 10);
        go(&mut w, &group, (80, 10));
        run(&mut w, 100);
        (w, group)
    };
    let (a, _) = make(true);
    let (b, _) = make(true);
    let (c, _) = make(false);
    assert_eq!(a.checksum(), b.checksum());
    assert_ne!(a.checksum(), c.checksum(), "a formation changes the state");
    // The same order with the ids listed in another order is the same run.
    let mut w1 = world();
    let g1 = mixed_five(&mut w1, 6, 10);
    go(&mut w1, &g1, (80, 10));
    run(&mut w1, 200);
    let mut w2 = world();
    let g2 = mixed_five(&mut w2, 6, 10);
    let mut reversed = g2.clone();
    reversed.reverse();
    go(&mut w2, &reversed, (80, 10));
    run(&mut w2, 200);
    assert_eq!(w1.checksum(), w2.checksum());
    // A formation that never forms leaves the checksum as it was: a group below the minimum.
    let pair = |cfg: FormationConfig| {
        let mut w = world().with_formation(cfg).unwrap();
        let g = row(&mut w, &[TANK, TANK], 6, 10);
        go(&mut w, &g, (60, 10));
        run(&mut w, 50);
        w.checksum()
    };
    assert_eq!(pair(FormationConfig::DEFAULT), pair(FormationConfig::OFF));
}

#[test]
fn the_state_of_a_formation_can_be_read_back() {
    let mut w = world();
    let group = mixed_five(&mut w, 6, 10);
    go(&mut w, &group, (80, 10));
    run(&mut w, 20);
    let leader = w.formation_of(group[0]).unwrap().leader_id;
    let v = w.formation_of(leader).unwrap();
    assert!(v.leader);
    assert!(v.mode == MemberMode::Lead || v.mode == MemberMode::Wait);
    assert_eq!(v.state, FormationState::Whole);
    assert!(v.extent > Fx::ZERO);
    assert!(w.formation_of(EntityId::new(999, 0)).is_none());
}
