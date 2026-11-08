//! The targeting gate measured to the edge of the target (WP7.7, `docs/rules/vision.md`): a unit whose
//! sight equals its weapon reach still picks a target at the limit of its reach.
//!
//! The shooter (player 0) sees 5 cells and has a weapon of range 5, measured to the edge of the
//! target (`targeting.md`). It stands in cell (10, 10), its position the middle of the cell. The target
//! (player 1) stands at offsets from it, sees nothing and shoots nothing. Before WP7.7 the gate asked
//! whether the target's own cell was in the disc of sight (`(dx, dy)` with `dx^2 + dy^2 <= 25`), a test
//! that is cell against cell, so the tower at the offset (5, 1) (26 squared, in reach: `5 + 0.5`
//! squared is 30.25) was never picked.

use sf_math::{Fx, Vec2};
use sf_sim::{
    Cell, Command, DestructionConfig, EntityId, Footprint, Kind, Loadout, Mount, PlayerId,
    ProjectileKind, Tick, TargetingConfig, UnitStats, UnitTable, VisionRules, WeaponStats,
    WeaponTable, World, WorldConfig, per_tick,
};

const SHOOTER: Kind = Kind(1); // mobile, sight 5, weapon range 5
const DUMMY: Kind = Kind(2); // mobile, size 0.4 (radius 0.2), no weapon, sight 0
const TOWER: Kind = Kind(3); // structure 1 by 1, no weapon, sight 0
const WALL: Kind = Kind(4); // structure 2 by 2, no weapon, sight 0

const RIFLE: u16 = 1;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn mobile(kind: Kind, range: i32) -> UnitStats {
    UnitStats {
        kind,
        hp: 100_000,
        movement_class: Some(0),
        speed_per_tick: per_tick(Fx::from_int(2)),
        turn_per_tick: 255,
        size: Fx::from_ratio(4, 10),
        footprint: None,
        weapon_range: Fx::from_int(range),
        turret_per_tick: 0,
    }
}

fn structure(kind: Kind, side: u8) -> UnitStats {
    UnitStats {
        kind,
        hp: 100_000,
        movement_class: None,
        speed_per_tick: Fx::ZERO,
        turn_per_tick: 0,
        size: Fx::ZERO,
        footprint: Some(Footprint {
            width: side,
            height: side,
        }),
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    }
}

fn world(gate: bool) -> World {
    let units = UnitTable::new(vec![
        mobile(SHOOTER, 5),
        mobile(DUMMY, 0),
        structure(TOWER, 1),
        structure(WALL, 2),
    ])
    .unwrap();
    let rifle = WeaponStats {
        kind: RIFLE,
        range: Fx::from_int(5),
        min_range: Fx::ZERO,
        reload_ticks: 10,
        burst: 1,
        burst_delay_ticks: 0,
        damage: 1,
        damage_type: 0,
        projectile: ProjectileKind::Instant,
        projectile_speed_per_tick: Fx::ZERO,
        flight_height: Fx::ZERO,
        guide_turn_per_tick: 0,
        splash_radius: Fx::ZERO,
        splash_inner: Fx::ZERO,
        friendly_fire: true,
        mount: Mount::Hull,
        fire_arc: 128,
    };
    let weapons = WeaponTable::new(
        vec![rifle],
        vec![Loadout {
            unit: SHOOTER,
            weapons: vec![RIFLE],
        }],
    )
    .unwrap();
    World::new(WorldConfig::new(7).with_map_size(40, 40))
        .unwrap()
        .with_units(units)
        .unwrap()
        .with_weapons(weapons)
        .unwrap()
        .with_destruction(DestructionConfig::DEFAULT)
        .unwrap()
        .with_targeting(TargetingConfig::DEFAULT)
        .unwrap()
        .with_vision(
            VisionRules::new(
                0,
                vec![(SHOOTER, 5000), (DUMMY, 0), (TOWER, 0), (WALL, 0)],
            )
            .with_acquire_needs_sight(gate),
        )
        .unwrap()
}

/// The middle of cell `(x, y)` plus `(dx, dy)` thousandths of a cell.
fn pos(x: i32, y: i32, dx: i32, dy: i32) -> Vec2 {
    Vec2::new(
        Fx::from_int(x) + Fx::HALF + Fx::from_ratio(i64::from(dx), 1000),
        Fx::from_int(y) + Fx::HALF + Fx::from_ratio(i64::from(dy), 1000),
    )
}

/// What the shooter does about a target placed as `place` says: whether it has picked it after 30
/// ticks, with the gate `gate`.
fn picks(gate: bool, place: impl Fn(&mut World)) -> bool {
    let mut w = world(gate);
    w.schedule(
        Tick(0),
        P0,
        Command::SpawnUnit {
            kind: SHOOTER,
            pos: pos(10, 10, 0, 0),
            owner: P0,
        },
    )
    .unwrap();
    place(&mut w);
    for _ in 0..30 {
        w.step();
    }
    let shooter = EntityId::new(0, 0);
    w.engagement_of(shooter).is_some() || !w.shots().is_empty()
}

fn tower_at(x: u32, y: u32) -> impl Fn(&mut World) {
    move |w| {
        w.schedule(
            Tick(0),
            P1,
            Command::PlaceStructure {
                kind: TOWER,
                cell: Cell::new(x, y),
                owner: P1,
            },
        )
        .unwrap();
    }
}

fn dummy_at(p: Vec2) -> impl Fn(&mut World) {
    move |w| {
        w.schedule(
            Tick(0),
            P1,
            Command::SpawnUnit {
                kind: DUMMY,
                pos: p,
                owner: P1,
            },
        )
        .unwrap();
    }
}

/// The case of the first match: sight 5 and reach 5, a tower one cell wide at the offset (5, 1). Its
/// own cell is 26 (squared) from the shooter's, outside the disc of 25, and the cell beside it, (4, 1),
/// inside; the tower is in reach. The edge test picks it, the old cell test did not.
#[test]
fn a_structure_at_the_limit_of_the_reach_is_picked() {
    // Control: without the gate the tower is picked, so it is in reach.
    assert!(picks(false, tower_at(15, 11)));
    assert!(picks(true, tower_at(15, 11)), "26 squared, in reach");
    // The same on the other side and on the other axis.
    assert!(picks(true, tower_at(5, 11)));
    assert!(picks(true, tower_at(11, 15)));
    assert!(picks(true, tower_at(9, 5)));
}

/// A structure two cells wide, in reach at (5, 1) from the shooter's cell (its middle is 32.5 squared
/// away, `5 + 1` squared is 36): none of its four cells is in the disc (26, 37, 29 and 40 squared), the
/// cell beside it on the near side, (4, 1), is.
#[test]
fn a_two_cell_structure_at_the_limit_is_picked() {
    let wall = |x: u32, y: u32| {
        move |w: &mut World| {
            w.schedule(
                Tick(0),
                P1,
                Command::PlaceStructure {
                    kind: WALL,
                    cell: Cell::new(x, y),
                    owner: P1,
                },
            )
            .unwrap();
        }
    };
    assert!(picks(false, wall(15, 11)));
    assert!(picks(true, wall(15, 11)));
}

/// Out of reach stays out of reach: the edge test makes a target visible, never reachable. A tower at
/// (6, 0) is 36 squared away, beyond `5 + 0.5` (30.25): not picked with the gate off either.
#[test]
fn what_is_out_of_reach_is_not_picked_with_the_edge_test_either() {
    assert!(!picks(false, tower_at(16, 10)));
    assert!(!picks(true, tower_at(16, 10)));
}

/// A tower at (5, 2) (29 squared, in reach): its cell is outside the disc, the cell beside it over a
/// side, (4, 2), is inside (20 squared). (That a cell touching only at a corner does not count is
/// tested on the sight alone, in `vision/tests.rs`.)
#[test]
fn a_tower_whose_neighbour_is_visible_over_a_side_is_picked() {
    assert!(picks(false, tower_at(15, 12)));
    assert!(picks(true, tower_at(15, 12)));
}

/// A unit target is seen through its own cell, and through a neighbour cell its body reaches. A dummy of
/// radius 0.2 in the middle of cell (5, 1) (26 squared, in reach: `5.2` squared is 27.04) is **not**
/// seen: its body does not reach the cell beside it (that is 0.5 away, the body 0.2). This is the rule
/// and not an accident: the edge of a unit is its body.
#[test]
fn a_unit_in_the_middle_of_a_cell_outside_the_disc_is_not_picked_and_that_is_the_rule() {
    assert!(picks(false, dummy_at(pos(15, 11, 0, 0))), "in reach");
    assert!(
        !picks(true, dummy_at(pos(15, 11, 0, 0))),
        "its cell is outside the disc and its body reaches no other cell"
    );
}

/// The same dummy 0.4 of a cell nearer to the shooter (0.1 from the west border of its cell): its body
/// (radius 0.2) reaches the cell beside it, (4, 1), which is inside the disc: seen, and picked. At 0.25
/// from the border it does not.
#[test]
fn a_unit_whose_body_reaches_a_visible_cell_is_picked() {
    assert!(picks(false, dummy_at(pos(15, 11, -400, 0))), "in reach");
    assert!(picks(true, dummy_at(pos(15, 11, -400, 0))));
    assert!(picks(false, dummy_at(pos(15, 11, -250, 0))), "in reach");
    assert!(!picks(true, dummy_at(pos(15, 11, -250, 0))));
}
