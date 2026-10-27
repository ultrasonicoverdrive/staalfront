//! WP6.2: the ghost under the cursor is answered by the simulation, through the sim thread,
//! and the builds it refuses are reported.
//!
//! The presentation has no copy of the placement rules: `SimHandle::set_ghost` asks, the sim
//! thread calls `World::preview_placement` and the answer sits in the next snapshot. These tests
//! hold that path against the command: for each place, the verdict in the snapshot is whether
//! `BuildStructure` stands, and the refusal it reports is the reason in the snapshot.

use std::time::Duration;

use sf_data::units::UnitDefs;
use sf_game::playfield::with_buildings;
use sf_game::sim_snapshot::GhostQuery;
use sf_game::sim_thread::{SimConfig, SimHandle};
use sf_sim::{Cell, Command, Kind, PlacementError, PlayerId, Tick, World, WorldConfig};

const SIDE: u32 = 64;
const ME: PlayerId = PlayerId(0);
const ENEMY: PlayerId = PlayerId(1);

fn defs() -> UnitDefs {
    UnitDefs::embedded()
}

fn kind(defs: &UnitDefs, id: &str) -> Kind {
    Kind(defs.kind_of(id).unwrap())
}

/// Two bases on open ground: Meridian's anchor on (20, 20), Veyran's on (44, 44), with the
/// shipped rules and the two seats' factions.
fn world(defs: &UnitDefs) -> World {
    let mut world = with_buildings(
        World::new(WorldConfig::new(9).with_map_size(SIDE, SIDE))
            .unwrap()
            .with_units(sf_bridge::unit_table(defs).unwrap())
            .unwrap(),
        defs,
    );
    for (owner, x, y) in [(ME, 20, 20), (ENEMY, 44, 44)] {
        world
            .schedule(
                Tick(0),
                owner,
                Command::PlaceStructure {
                    kind: kind(defs, "construction_center"),
                    cell: Cell::new(x, y),
                    owner,
                },
            )
            .unwrap();
    }
    world.step();
    world
}

#[test]
fn the_snapshot_carries_the_simulations_answer_to_the_question_asked() {
    let defs = defs();
    let plant = kind(&defs, "power_plant");
    let reference = world(&defs);
    let mut sim = SimHandle::spawn(world(&defs), SimConfig::manual()).unwrap();
    // Nothing asked, nothing answered.
    sim.advance(1);
    let snap = sim
        .wait_for_tick(Tick(2), Duration::from_secs(5))
        .expect("tick 2");
    assert!(snap.ghost().is_none());

    for (n, (x, y)) in [(26, 20), (60, 60), (0, 0), (30, 30), (26, 21)]
        .into_iter()
        .enumerate()
    {
        let query = GhostQuery {
            kind: plant,
            cell: Cell::new(x, y),
            player: ME,
        };
        sim.set_ghost(Some(query));
        // In manual pacing the answer is published at once, with the tick of the moment.
        sim.advance(1);
        let tick = Tick(3 + n as u64);
        let snap = sim
            .wait_for_tick(tick, Duration::from_secs(5))
            .expect("the next tick");
        let answer = snap.ghost().expect("the question is answered");
        assert_eq!(answer.query, query);
        // The answer is the world's own: the same function, the same verdict.
        assert_eq!(
            answer.preview,
            reference.preview_placement(plant, Cell::new(x, y), ME),
            "({x}, {y})"
        );
    }
    // Stop asking: the snapshots stop carrying an answer.
    sim.set_ghost(None);
    sim.advance(1);
    let snap = sim
        .wait_for_tick(Tick(8), Duration::from_secs(5))
        .expect("tick 8");
    assert!(snap.ghost().is_none());
    sim.stop();
}

#[test]
fn what_the_ghost_shows_is_what_the_build_command_does() {
    let defs = defs();
    let plant = kind(&defs, "power_plant");
    let decoy = kind(&defs, "decoy_emplacement");
    let bunker = kind(&defs, "fortified_bunker");
    let tower = kind(&defs, "defence_tower");
    let mut sim = SimHandle::spawn(world(&defs), SimConfig::manual()).unwrap();
    // (player, kind, cell, expected reason)
    let cases = [
        (ME, plant, Cell::new(26, 20), None),
        (
            ME,
            plant,
            Cell::new(50, 20),
            Some(PlacementError::TooFarFromBase),
        ),
        (
            ME,
            bunker,
            Cell::new(26, 26),
            Some(PlacementError::WrongFaction),
        ),
        (
            ME,
            tower,
            Cell::new(26, 30),
            Some(PlacementError::MissingPrerequisite(kind(&defs, "barracks"))),
        ),
        (
            ENEMY,
            decoy,
            Cell::new(38, 44),
            Some(PlacementError::WrongFaction),
        ),
        (
            ENEMY,
            bunker,
            Cell::new(38, 44),
            Some(PlacementError::MissingPrerequisite(kind(&defs, "barracks"))),
        ),
        // The anchor of Meridian is in the way of the second plant.
        (ME, plant, Cell::new(21, 21), Some(PlacementError::Occupied)),
    ];
    let mut tick = 1_u64;
    for (player, k, cell, reason) in cases {
        // Ask, and read the answer.
        sim.set_ghost(Some(GhostQuery {
            kind: k,
            cell,
            player,
        }));
        sim.advance(1);
        tick += 1;
        let asked = sim
            .wait_for_tick(Tick(tick), Duration::from_secs(5))
            .unwrap();
        let verdict = asked.ghost().unwrap().preview.verdict;
        assert_eq!(verdict.err(), reason, "{k:?} at {cell}");
        // Build, in the next tick, and read what the world did.
        let before = sim.rejected_builds().0;
        sim.send_at(
            Tick(tick),
            player,
            Command::BuildStructure { kind: k, cell },
        );
        sim.advance(1);
        tick += 1;
        let after = sim
            .wait_for_tick(Tick(tick), Duration::from_secs(5))
            .unwrap();
        let (total, last) = sim.rejected_builds();
        match reason {
            None => {
                assert_eq!(total, before, "{k:?} at {cell} stood");
                assert!(after.structure_cells().contains(&(cell.x, cell.y)));
            }
            Some(expected) => {
                assert_eq!(total, before + 1, "{k:?} at {cell} was refused and said so");
                let r = last.last().unwrap();
                assert_eq!(
                    (r.player, r.kind, r.cell, r.reason),
                    (player, k, cell, expected)
                );
            }
        }
    }
    sim.stop();
}
