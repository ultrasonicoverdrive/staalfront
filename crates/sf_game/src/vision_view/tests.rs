//! The debug view of shroud and fog (WP8.1): the runs that are drawn, and what the snapshot carries
//! of the local player's sight (the bitsets, the version, the `seen` flag of each view).

use sf_data::units::UnitDefs;
use sf_math::{Fx, Vec2};
use sf_sim::{Cell, Command, EntityId, Kind, PlayerId, Visibility, World, WorldConfig};

use super::*;
use crate::commands::LOCAL_PLAYER;
use crate::sim_snapshot::{Pace, SimSnapshot, Statics};

use Visibility::{Fog, Shroud, Visible};

const OTHER: PlayerId = PlayerId(1);

fn at(x: i32, y: i32) -> Vec2 {
    Vec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF)
}

fn world() -> (World, UnitDefs) {
    let defs = UnitDefs::embedded();
    let w = World::new(WorldConfig::new(5).with_map_size(48, 32))
        .unwrap()
        .with_units(sf_bridge::unit_table(&defs).unwrap())
        .unwrap();
    (crate::playfield::with_vision(w, &defs), defs)
}

fn kind(defs: &UnitDefs, role: &str) -> Kind {
    Kind(defs.kind_of(role).unwrap())
}

fn spawn(w: &mut World, owner: PlayerId, kind: Kind, x: i32, y: i32) {
    let t = w.tick();
    w.schedule(
        t,
        owner,
        Command::SpawnUnit {
            kind,
            pos: at(x, y),
            owner,
        },
    )
    .unwrap();
}

// ---- the runs -----------------------------------------------------------------------------------

#[test]
fn visible_cells_are_not_drawn_and_a_run_stops_at_a_change_of_state() {
    // Row 0: S S F F V S, row 1: all visible, row 2: F then visible.
    let state = |c: Cell| match (c.x, c.y) {
        (0 | 1, 0) | (5, 0) => Shroud,
        (2 | 3, 0) | (0, 2) => Fog,
        _ => Visible,
    };
    let got = runs(6, 3, 8, &state);
    assert_eq!(
        got,
        vec![
            Run {
                y: 0,
                x0: 0,
                x1: 1,
                state: Shroud
            },
            Run {
                y: 0,
                x0: 2,
                x1: 3,
                state: Fog
            },
            Run {
                y: 0,
                x0: 5,
                x1: 5,
                state: Shroud
            },
            Run {
                y: 2,
                x0: 0,
                x1: 0,
                state: Fog
            },
        ]
    );
}

#[test]
fn a_run_is_cut_at_the_longest_the_look_allows() {
    let got = runs(10, 1, 4, &|_| Shroud);
    let spans: Vec<(u32, u32)> = got.iter().map(|r| (r.x0, r.x1)).collect();
    assert_eq!(spans, vec![(0, 3), (4, 7), (8, 9)]);
    // A limit of zero is taken as one.
    assert_eq!(runs(3, 1, 0, &|_| Fog).len(), 3);
}

#[test]
fn a_fully_visible_map_draws_nothing_and_a_fully_dark_one_covers_every_cell_once() {
    assert!(runs(7, 5, 8, &|_| Visible).is_empty());
    let all = runs(7, 5, 3, &|_| Shroud);
    let cells: u32 = all.iter().map(|r| r.x1 - r.x0 + 1).sum();
    assert_eq!(cells, 35);
}

// ---- the snapshot -------------------------------------------------------------------------------

#[test]
fn a_world_without_vision_rules_has_no_fog_in_the_snapshot_and_everything_is_seen() {
    let defs = UnitDefs::embedded();
    let mut w = World::new(WorldConfig::new(5).with_map_size(48, 32))
        .unwrap()
        .with_units(sf_bridge::unit_table(&defs).unwrap())
        .unwrap();
    spawn(&mut w, OTHER, kind(&defs, "rifleman"), 40, 20);
    w.step();
    let snap = SimSnapshot::capture(&w);
    assert_eq!(snap.fog_version(), None);
    assert_eq!(snap.visibility(Cell::new(0, 0)), Visible);
    let id = snap.views()[0].id;
    assert!(snap.seen(id));
    assert!(runs(48, 32, 8, &|c| snap.visibility(c)).is_empty());
}

#[test]
fn the_snapshot_carries_what_the_local_player_sees() {
    let (mut w, defs) = world();
    spawn(&mut w, LOCAL_PLAYER, kind(&defs, "scout"), 10, 10); // sight 8
    spawn(&mut w, OTHER, kind(&defs, "rifleman"), 14, 10); // 4 away: seen
    spawn(&mut w, OTHER, kind(&defs, "rifleman"), 40, 25); // far away: not seen
    w.step();
    let snap = SimSnapshot::capture(&w);
    assert!(snap.fog_version().is_some());
    let ids: Vec<EntityId> = snap.views().iter().map(|v| v.id).collect();
    assert_eq!(ids.len(), 3);
    // Own objects are always seen; the near rifleman is, the far one is not.
    assert!(snap.seen(ids[0]));
    assert!(snap.seen(ids[1]));
    assert!(!snap.seen(ids[2]));
    // Every cell of the bitsets is what the world says.
    for y in 0..32 {
        for x in 0..48 {
            let c = Cell::new(x, y);
            assert_eq!(snap.visibility(c), w.visibility(LOCAL_PLAYER, c), "{c:?}");
        }
    }
    assert_eq!(snap.visibility(Cell::new(10, 10)), Visible);
    assert_eq!(snap.visibility(Cell::new(40, 25)), Shroud);
    assert_eq!(snap.visibility(Cell::new(48, 0)), Shroud, "outside the map");
    // The other player's sight is not in this snapshot: it sees its own riflemen's cells.
    assert_ne!(
        snap.visibility(Cell::new(40, 25)),
        w.visibility(OTHER, Cell::new(40, 25))
    );
}

#[test]
fn a_structure_is_seen_when_any_cell_of_its_footprint_is() {
    let (mut w, defs) = world();
    spawn(&mut w, LOCAL_PLAYER, kind(&defs, "scout"), 10, 10); // sight 8
    let t = w.tick();
    // A refinery is 3 by 4. Anchored at (17, 10) its first column is 7 cells from the scout; anchored
    // at (30, 10) it is far out.
    for cell in [(17u32, 10u32), (30, 10)] {
        w.schedule(
            t,
            OTHER,
            Command::PlaceStructure {
                kind: kind(&defs, "refinery"),
                cell: Cell::new(cell.0, cell.1),
                owner: OTHER,
            },
        )
        .unwrap();
    }
    w.step();
    let snap = SimSnapshot::capture(&w);
    let near = w.occupant_at(Cell::new(17, 10)).unwrap();
    let far = w.occupant_at(Cell::new(30, 10)).unwrap();
    assert!(
        snap.seen(near),
        "the first column is 7 cells from the scout"
    );
    assert!(!snap.seen(far));
    // A stale id is not seen.
    assert!(!snap.seen(EntityId::new(999, 0)));
}

#[test]
fn the_bitsets_are_copied_only_when_the_version_changes() {
    let (mut w, defs) = world();
    spawn(&mut w, LOCAL_PLAYER, kind(&defs, "scout"), 10, 10);
    w.step();
    let statics = Statics::new(&w);
    let mut snap = SimSnapshot::blank();
    snap.fill(&w, &statics, Pace::now());
    let v = snap.fog_version().unwrap();
    assert_eq!(snap.visibility(Cell::new(10, 10)), Visible);

    // Nothing moves, so a refill leaves the copy alone. Poison it to see: had the refill copied
    // again, the poison would be gone.
    snap.poison_fog_for_test();
    assert_eq!(snap.visibility(Cell::new(10, 10)), Shroud);
    w.step();
    snap.fill(&w, &statics, Pace::now());
    assert_eq!(
        snap.fog_version(),
        Some(v),
        "nothing changed in the picture"
    );
    assert_eq!(
        snap.visibility(Cell::new(10, 10)),
        Shroud,
        "the bitsets were not copied again"
    );

    // Now the scout moves a long way: the version changes and the copy is made.
    let scout = snap.views()[0].id;
    let t = w.tick();
    w.schedule(
        t,
        LOCAL_PLAYER,
        Command::MoveTo {
            ids: vec![scout],
            target: at(30, 20),
        },
    )
    .unwrap();
    for _ in 0..400 {
        w.step();
    }
    snap.fill(&w, &statics, Pace::now());
    assert_ne!(snap.fog_version(), Some(v));
    for y in 0..32 {
        for x in 0..48 {
            let c = Cell::new(x, y);
            assert_eq!(snap.visibility(c), w.visibility(LOCAL_PLAYER, c));
        }
    }
    // Where it started is fog now, and the explored part did not shrink.
    assert_eq!(snap.visibility(Cell::new(10, 10)), Fog);
}

#[test]
fn a_quad_off_the_screen_or_smeared_by_the_near_plane_is_not_drawn() {
    let vp = [1280.0, 720.0];
    let quad = |x0: f32, y0: f32, x1: f32, y1: f32| [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
    assert!(worth_drawing(&quad(100.0, 100.0, 200.0, 160.0), vp));
    // Partly on the screen counts; wholly off it does not.
    assert!(worth_drawing(&quad(-50.0, 100.0, 50.0, 160.0), vp));
    assert!(!worth_drawing(&quad(-300.0, 100.0, -200.0, 160.0), vp));
    assert!(!worth_drawing(&quad(100.0, 800.0, 200.0, 900.0), vp));
    // One corner thrown far away (a cell that crosses the near plane) is dropped.
    let mut smear = quad(100.0, 100.0, 200.0, 160.0);
    smear[2] = (9000.0, 100.0);
    assert!(!worth_drawing(&smear, vp));
}

#[test]
fn own_objects_are_always_seen_even_when_their_own_cell_is_not_in_sight() {
    let (mut w, _defs) = world();
    // A legacy structure never stands, so it sees nothing and its cell is only explored (fog); it is
    // the local player's own, and an own object is always seen.
    let t = w.tick();
    w.schedule(
        t,
        LOCAL_PLAYER,
        Command::Place {
            kind: Kind(900),
            cell: Cell::new(5, 5),
            owner: LOCAL_PLAYER,
        },
    )
    .unwrap();
    w.schedule(
        t,
        OTHER,
        Command::Place {
            kind: Kind(901),
            cell: Cell::new(20, 20),
            owner: OTHER,
        },
    )
    .unwrap();
    w.step();
    let snap = SimSnapshot::capture(&w);
    let own = w.occupant_at(Cell::new(5, 5)).unwrap();
    let theirs = w.occupant_at(Cell::new(20, 20)).unwrap();
    assert_eq!(snap.visibility(Cell::new(5, 5)), Fog);
    assert!(snap.seen(own));
    assert!(!snap.seen(theirs));
}

#[test]
fn a_structure_is_seen_through_a_cell_that_is_not_its_anchor() {
    let (mut w, defs) = world();
    // The anchor is the north-west cell. The scout (sight 8) stands south of a refinery (3 by 4)
    // anchored at (8, 10): the anchor is 10 rows away, the last row of the footprint (y = 13) is 7.
    spawn(&mut w, LOCAL_PLAYER, kind(&defs, "scout"), 10, 20);
    let t = w.tick();
    w.schedule(
        t,
        OTHER,
        Command::PlaceStructure {
            kind: kind(&defs, "refinery"),
            cell: Cell::new(8, 10),
            owner: OTHER,
        },
    )
    .unwrap();
    w.step();
    let snap = SimSnapshot::capture(&w);
    let id = w.occupant_at(Cell::new(8, 10)).unwrap();
    assert_eq!(
        snap.visibility(Cell::new(8, 10)),
        Shroud,
        "the anchor is out of sight"
    );
    assert_eq!(snap.visibility(Cell::new(9, 13)), Visible);
    assert!(snap.seen(id));
}
