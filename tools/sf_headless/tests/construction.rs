//! The `construction` scenario (WP6.3): a structure that is built goes up for its build time and is
//! no prerequisite until it is complete; then it is. One Meridian base on the 256 by 256 cell
//! playfield, the definitions are the frozen copy `placement_defs`.

mod common;

use common::scenario_path;
use sf_headless::scenario::Scenario;
use sf_sim::{Cell, Kind, PlacementError, World};

const POWER_PLANT: Kind = Kind(1);
const REFINERY: Kind = Kind(2);
const DECOY: Kind = Kind(9);

fn scenario() -> Scenario {
    Scenario::load(&scenario_path("construction.ron")).unwrap()
}

/// What a run saw.
struct Seen {
    world: World,
    /// `(tick, kind, reason)` of every refused build.
    refused: Vec<(u64, u16, PlacementError)>,
    /// `(tick, kind)` of every completion.
    done: Vec<(u64, u16)>,
    /// `(tick, ticks left, thousandths)` of the refinery while it goes up.
    going: Vec<(u64, u32, u16)>,
}

fn run() -> Seen {
    let s = scenario();
    let mut world = s.build_world().unwrap();
    let (mut refused, mut done, mut going) = (Vec::new(), Vec::new(), Vec::new());
    let refinery_cell = Cell::new(22, 49);
    for _ in 0..s.ticks {
        let report = world.step();
        for r in world.placement_rejections() {
            refused.push((r.tick.0, r.kind.0, r.reason));
        }
        for c in world.completions() {
            done.push((c.tick.0, c.kind.0));
        }
        assert_eq!(
            world.completions().len(),
            report.structures_completed as usize
        );
        // The refinery's progress, once it goes up, tick by tick: the state the picture reads.
        if let Some(id) = world.occupant_at(refinery_cell)
            && let Some(c) = world.construction_of(id)
        {
            going.push((report.tick.0, c.left, c.permille()));
        }
    }
    Seen {
        world,
        refused,
        done,
        going,
    }
}

#[test]
fn a_structure_is_no_prerequisite_until_the_tick_it_completes() {
    let seen = run();
    // The refinery needs the power plant. The plant goes up at tick 1 (300 ticks): refused at tick 2
    // and at tick 300 (one tick to go), accepted at tick 301, the tick the plant completes.
    assert_eq!(
        seen.refused,
        vec![
            (
                2,
                REFINERY.0,
                PlacementError::MissingPrerequisite(POWER_PLANT)
            ),
            (
                300,
                REFINERY.0,
                PlacementError::MissingPrerequisite(POWER_PLANT)
            ),
        ]
    );
    // The completions, by the build seconds of the frozen definitions at 20 ticks a second:
    // decoy 160 (1 -> 161), power plant 300 (1 -> 301), refinery 800 (301 -> 1101).
    assert_eq!(
        seen.done,
        vec![(161, DECOY.0), (301, POWER_PLANT.0), (1101, REFINERY.0)]
    );
    assert_eq!(seen.world.under_construction(), 0);
}

#[test]
fn the_decoy_needs_only_the_barracks_that_was_placed_complete() {
    // The barracks of the start state stands from tick 0 (PlaceStructure), so the decoy of tick 1 is
    // accepted at once, and is going up while the plant is.
    let mut world = scenario().build_world().unwrap();
    world.step();
    let r = world.step();
    assert_eq!(r.tick.0, 1);
    assert_eq!(r.placements_rejected, 0);
    let barracks = world.occupant_at(Cell::new(26, 49)).unwrap();
    assert!(world.stands(barracks));
    assert_eq!(world.construction_of(barracks), None);
    let decoy = world.occupant_at(Cell::new(30, 48)).unwrap();
    let c = world.construction_of(decoy).unwrap();
    assert_eq!((c.left, c.total, c.permille()), (160, 160, 0));
    assert!(!world.stands(decoy));
}

#[test]
fn the_progress_the_picture_reads_rises_tick_by_tick_to_the_completion() {
    let seen = run();
    // The refinery is going up from tick 301 to 1100: 800 ticks, and in the step of tick 1101 it
    // completes (it is no longer listed).
    assert_eq!(seen.going.first(), Some(&(301, 800, 0)));
    assert_eq!(seen.going.last(), Some(&(1100, 1, 998)));
    assert_eq!(seen.going.len(), 800);
    assert!(
        seen.going
            .windows(2)
            .all(|w| w[1].1 + 1 == w[0].1 && w[1].2 >= w[0].2)
    );
}

#[test]
fn the_end_state_has_everything_standing() {
    let seen = run();
    // Anchor, barracks, decoy, plant, refinery.
    assert_eq!(seen.world.len(), 5);
    for (x, y) in [(22, 44), (26, 49), (30, 48), (30, 40), (22, 49)] {
        let id = seen.world.occupant_at(Cell::new(x, y)).unwrap();
        assert!(seen.world.stands(id), "({x}, {y})");
    }
}

#[test]
fn the_run_is_deterministic() {
    let sums = |()| run().world.checksum_history().to_vec();
    assert_eq!(sums(()), sums(()));
}
