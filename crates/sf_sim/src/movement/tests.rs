use super::*;
use crate::terrain::{TerrainDef, TerrainFlags};
use sf_math::Vec2;

const GRASS: TerrainId = TerrainId(0);
const WATER: TerrainId = TerrainId(2);

fn class(class: u16, costs: &[(TerrainId, Cost)]) -> ClassCosts {
    ClassCosts {
        class,
        costs: costs.to_vec(),
    }
}

fn walker() -> ClassCosts {
    class(3, &[(GRASS, Cost::Speed(1000)), (WATER, Cost::Impassable)])
}

#[test]
fn the_input_order_does_not_matter() {
    let boat = class(8, &[(GRASS, Cost::Impassable), (WATER, Cost::Speed(900))]);
    let a = MovementTable::new(vec![walker(), boat.clone()]).unwrap();
    let mut flipped = boat;
    flipped.costs.reverse();
    let mut also = walker();
    also.costs.reverse();
    let b = MovementTable::new(vec![flipped, also]).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.digest(), b.digest());
    assert_ne!(a.digest(), 0);
    assert_eq!(a.len(), 2);
    assert!(a.has_class(3) && a.has_class(8) && !a.has_class(4));
    assert_eq!(a.get(3, GRASS), Some(Some(Cost::Speed(1000))));
    assert_eq!(
        a.get(3, TerrainId(9)),
        Some(None),
        "a type it does not list"
    );
    assert_eq!(a.get(4, GRASS), None, "a class it does not have");
}

#[test]
fn an_empty_table_has_no_digest() {
    let t = MovementTable::new(Vec::new()).unwrap();
    assert!(t.is_empty());
    assert_eq!(t.digest(), 0);
    assert_eq!(t, MovementTable::empty());
}

#[test]
fn every_cost_changes_the_digest() {
    let base = MovementTable::new(vec![walker()]).unwrap().digest();
    let changed = |f: fn(&mut ClassCosts)| {
        let mut c = walker();
        f(&mut c);
        MovementTable::new(vec![c]).unwrap().digest()
    };
    assert_ne!(base, changed(|c| c.costs[0].1 = Cost::Speed(1001)));
    assert_ne!(base, changed(|c| c.costs[1].1 = Cost::Speed(100)));
    assert_ne!(base, changed(|c| c.class = 4));
    assert_ne!(base, changed(|c| c.costs.push((TerrainId(5), Cost::OPEN))));
}

#[test]
fn bad_tables_are_refused() {
    let err = |v| MovementTable::new(v).unwrap_err();
    assert_eq!(
        err(vec![walker(), walker()]),
        MovementTableError::DuplicateClass(3)
    );
    assert_eq!(
        err(vec![class(1, &[(GRASS, Cost::OPEN), (GRASS, Cost::OPEN)])]),
        MovementTableError::DuplicateTerrain {
            class: 1,
            terrain: GRASS
        }
    );
    assert_eq!(
        err(vec![class(1, &[(WATER, Cost::Speed(0))])]),
        MovementTableError::ZeroSpeed {
            class: 1,
            terrain: WATER
        }
    );
    assert!(
        MovementTableError::ZeroSpeed {
            class: 1,
            terrain: WATER
        }
        .to_string()
        .contains("Impassable")
    );
}

#[test]
fn scaling_a_step_rounds_once_and_keeps_it_between_one_raw_unit_and_the_cap() {
    let step = Fx::from_raw(3_604);
    // Normal speed is the step itself, bit for bit.
    assert_eq!(scale_step(step, 1000), step);
    assert_eq!(scale_step(step, 1200), Fx::from_raw(4_325)); // rounded up from just under half a unit
    assert_eq!(scale_step(step, 500), Fx::from_raw(1_802));
    assert_eq!(scale_step(step, 800), Fx::from_raw(2_883)); // rounded down
    // A step never vanishes: it would never arrive.
    assert_eq!(scale_step(Fx::from_raw(3), 100), Fx::from_raw(1));
    // Nothing moves by nothing, and nothing moves more than the cap.
    assert_eq!(scale_step(Fx::ZERO, 1200), Fx::ZERO);
    assert_eq!(scale_step(Fx::from_int(5), 1000), MAX_STEP);
    assert_eq!(scale_step(Fx::from_ratio(1, 2), 2000), MAX_STEP);
    assert!(MAX_STEP < Fx::ONE, "a step reaches at most the next cell");
}

fn ground_parts() -> (Terrain, MovementTable) {
    // 4 x 3: grass, with water at (2, 1) and (1, 2).
    let defs = [
        TerrainDef {
            id: GRASS,
            flags: TerrainFlags::OPEN,
        },
        TerrainDef {
            id: WATER,
            flags: TerrainFlags::default(),
        },
    ];
    let ids = [0, 0, 0, 0, 0, 0, 2, 0, 0, 2, 0, 0];
    let terrain = Terrain::from_layer(4, 3, &ids, &defs).unwrap();
    (terrain, MovementTable::new(vec![walker()]).unwrap())
}

fn pos(x: i32, y: i32) -> Vec2 {
    Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF)
}

#[test]
fn the_cost_of_a_cell_follows_the_rules_in_order() {
    let (terrain, table) = ground_parts();
    let g = Ground {
        terrain: &terrain,
        movement: &table,
    };
    assert_eq!(g.cost(3, Cell::new(0, 0)), Cost::Speed(1000));
    assert_eq!(g.cost(3, Cell::new(2, 1)), Cost::Impassable);
    // Off the map: nobody, whatever the class.
    assert_eq!(g.cost(3, Cell::new(4, 0)), Cost::Impassable);
    assert_eq!(g.cost(99, Cell::new(0, 3)), Cost::Impassable);
    // A class the table does not have is never blocked.
    assert_eq!(g.cost(99, Cell::new(2, 1)), Cost::OPEN);
    // No table: open ground.
    let none = MovementTable::empty();
    let g = Ground {
        terrain: &terrain,
        movement: &none,
    };
    assert_eq!(g.cost(3, Cell::new(2, 1)), Cost::OPEN);
    // No layer: open ground, whatever the table says.
    let open = Terrain::open(4, 3);
    let g = Ground {
        terrain: &open,
        movement: &table,
    };
    assert_eq!(g.cost(3, Cell::new(2, 1)), Cost::OPEN);
    assert_eq!(g.cost(3, Cell::new(4, 1)), Cost::Impassable);
}

#[test]
fn a_step_needs_a_passable_cell_and_does_not_cut_a_blocked_corner() {
    let (terrain, table) = ground_parts();
    let g = Ground {
        terrain: &terrain,
        movement: &table,
    };
    // Within one cell: always.
    assert!(g.may_step(
        3,
        pos(0, 0),
        pos(0, 0) + Vec2::new(Fx::from_ratio(1, 4), Fx::ZERO)
    ));
    // Into water: no. Along the grass: yes.
    assert!(!g.may_step(3, pos(1, 1), pos(2, 1)));
    assert!(g.may_step(3, pos(0, 1), pos(1, 1)));
    // Diagonally from (1, 1) to (2, 2): the target is grass but the two cells
    // it cuts across, (2, 1) and (1, 2), are water.
    assert!(!g.may_step(3, pos(1, 1), pos(2, 2)));
    // Diagonally from (0, 0) to (1, 1): both neighbours are grass.
    assert!(g.may_step(3, pos(0, 0), pos(1, 1)));
    // From (1, 0) to (2, 1) is water itself; from (2, 0) to (3, 1) cuts
    // across (3, 0) grass and (2, 1) water: no.
    assert!(!g.may_step(3, pos(2, 0), pos(3, 1)));
    // Off the map.
    assert!(!g.may_step(3, pos(0, 0), Vec2::new(Fx::from_int(-1), Fx::ZERO)));
}

#[test]
fn a_unit_in_a_cell_it_could_not_enter_walks_out_at_normal_speed() {
    let (terrain, table) = ground_parts();
    let g = Ground {
        terrain: &terrain,
        movement: &table,
    };
    assert_eq!(g.permille_at(3, pos(0, 0)), 1000);
    assert_eq!(g.permille_at(3, pos(2, 1)), FULL_PERMILLE, "in the water");
    assert_eq!(g.permille_at(3, Vec2::from_ints(-5, 0)), FULL_PERMILLE);
    // Moving within the water cell, or out of it, is allowed.
    assert!(g.may_step(
        3,
        pos(2, 1),
        pos(2, 1) + Vec2::new(Fx::from_ratio(1, 8), Fx::ZERO)
    ));
    assert!(g.may_step(3, pos(2, 1), pos(3, 1)));
}

#[test]
fn the_first_gap_names_the_class_and_the_terrain_type() {
    let t = MovementTable::new(vec![walker(), class(1, &[(GRASS, Cost::OPEN)])]).unwrap();
    let defined = || [GRASS, WATER].into_iter();
    assert_eq!(t.first_gap(defined()), Some((1, WATER)));
    let full = MovementTable::new(vec![walker()]).unwrap();
    assert_eq!(full.first_gap(defined()), None);
}
