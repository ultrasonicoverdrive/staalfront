//! The ghost without a GPU: the tints, the words and what gets drawn on the world layer for an
//! answer of the simulation. The picture itself is checked by `tests/placement_screenshot.rs`.

use super::*;
use crate::playfield::with_buildings;
use crate::sim_snapshot::{GhostQuery, SimSnapshot};
use sf_data::units::UnitDefs;
use sf_render::camera::{Camera, CameraConfig, DVec2};
use sf_sim::{Command, PlayerId, World, WorldConfig};
use sf_ui::{Ui, UiConfig, UiInput};

const W: f32 = 960.0;
const H: f32 = 540.0;

fn frame() -> GhostFrame {
    frame_at((200.0, 200.0))
}

fn frame_at(focus: (f64, f64)) -> GhostFrame {
    let cfg = CameraConfig {
        map_size: DVec2::new(256.0, 256.0),
        ..CameraConfig::default()
    };
    let mut c = Camera::new(cfg, W as u32, H as u32);
    c.set_zoom(0.15);
    c.set_focus(DVec2::new(focus.0, focus.1));
    GhostFrame {
        view_proj: c.view_proj(),
        viewport: [W, H],
        ui_scale: 1.0,
    }
}

fn flat(_x: f32, _z: f32) -> f32 {
    0.0
}

/// A 64 x 64 world with the shipped rules, Meridian's anchor on (40, 40) (cells 40..44, which is
/// metres 160 to 176, under the camera above).
fn world() -> (World, UnitDefs) {
    let defs = UnitDefs::embedded();
    let mut world = with_buildings(
        World::new(WorldConfig::new(1).with_map_size(64, 64))
            .unwrap()
            .with_units(sf_bridge::unit_table(&defs).unwrap())
            .unwrap(),
        &defs,
    );
    world
        .schedule(
            world.tick(),
            PlayerId(0),
            Command::PlaceStructure {
                kind: Kind(defs.kind_of("construction_center").unwrap()),
                cell: Cell::new(48, 48),
                owner: PlayerId(0),
            },
        )
        .unwrap();
    world.step();
    (world, defs)
}

fn answer_for(world: &World, defs: &UnitDefs, role: &str, x: u32, y: u32) -> GhostAnswer {
    let query = GhostQuery {
        kind: Kind(defs.kind_of(role).unwrap()),
        cell: Cell::new(x, y),
        player: PlayerId(0),
    };
    let mut snap = SimSnapshot::capture(world);
    snap.answer(world, query);
    snap.ghost().cloned().unwrap()
}

fn quads(answer: &GhostAnswer) -> (usize, usize) {
    quads_in(answer, &frame())
}

fn quads_in(answer: &GhostAnswer, frame: &GhostFrame) -> (usize, usize) {
    let mut ui = Ui::new(UiConfig::default());
    ui.begin_frame(UiInput::default(), (W as u32, H as u32), 1.0);
    ui.end_frame();
    let mut layer = ui.world_layer();
    let cells = draw_ghost(&mut layer, frame, &flat, answer);
    (cells, layer.quad_count())
}

#[test]
fn a_cell_is_green_red_or_amber_by_its_own_verdict_and_the_placements() {
    // The placement stands: the free cells are green.
    assert_eq!(tint_of(CellVerdict::Free, true), Tint::Valid);
    // The placement is refused for another reason: the free cells are amber.
    assert_eq!(tint_of(CellVerdict::Free, false), Tint::Elsewhere);
    // A cell with its own problem is red, whatever the placement says.
    for v in [
        CellVerdict::OffMap,
        CellVerdict::BadTerrain,
        CellVerdict::Structure,
        CellVerdict::Unit,
    ] {
        assert_eq!(tint_of(v, true), Tint::Blocked, "{v:?}");
        assert_eq!(tint_of(v, false), Tint::Blocked, "{v:?}");
    }
    // The three tints are three colours.
    let colours = [VALID, BLOCKED, ELSEWHERE];
    assert!(colours[0] != colours[1] && colours[1] != colours[2] && colours[0] != colours[2]);
    assert!(OUTLINE_VALID != OUTLINE_REFUSED && EXIT_OPEN != EXIT_BLOCKED);
}

#[test]
fn every_refusal_has_its_words_and_a_prerequisite_is_named() {
    let name = |k: Kind| format!("kind{}", k.0);
    let all = [
        PlacementError::NoBuildingRules,
        PlacementError::UnknownKind,
        PlacementError::CannotBeBuilt,
        PlacementError::NoFaction,
        PlacementError::WrongFaction,
        PlacementError::MissingPrerequisite(Kind(7)),
        PlacementError::OffMap,
        PlacementError::BadTerrain,
        PlacementError::Occupied,
        PlacementError::UnitInTheWay,
        PlacementError::TooFarFromBase,
        PlacementError::ExitBlocked,
        PlacementError::EntityLimit,
    ];
    let texts: Vec<String> = all.iter().map(|&e| describe(e, &name)).collect();
    let mut distinct = texts.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), all.len(), "{texts:?}");
    assert!(texts.iter().all(|t| !t.is_empty()));
    assert!(describe(PlacementError::MissingPrerequisite(Kind(7)), &name).contains("kind7"));
    assert!(describe(PlacementError::TooFarFromBase, &name).contains("base"));
}

#[test]
fn the_ghost_is_the_commands_verdict() {
    // The answer the ghost paints is the one `BuildStructure` is run on: the same verdict
    // for the same question, cell by cell.
    let (mut world, defs) = world();
    let plant = Kind(defs.kind_of("power_plant").unwrap());
    for (x, y) in [(54, 48), (60, 48), (40, 48), (52, 40), (0, 0), (63, 63)] {
        let answer = answer_for(&world, &defs, "power_plant", x, y);
        assert_eq!(
            answer.preview.verdict,
            world.check_placement(plant, Cell::new(x, y), PlayerId(0)),
            "({x}, {y})"
        );
        let mut run = world.clone();
        run.schedule(
            run.tick(),
            PlayerId(0),
            Command::BuildStructure {
                kind: plant,
                cell: Cell::new(x, y),
            },
        )
        .unwrap();
        let report = run.step();
        assert_eq!(
            answer.preview.verdict.is_ok(),
            report.commands_executed == 1,
            "({x}, {y}): {:?}",
            answer.preview.verdict
        );
    }
    let _ = &mut world;
}

#[test]
fn a_valid_ghost_draws_every_cell_the_outline_and_no_exit() {
    let (world, defs) = world();
    // A power plant is 2 x 3 = 6 cells, 4 cells from the anchor's edge.
    let answer = answer_for(&world, &defs, "power_plant", 52, 48);
    assert_eq!(answer.preview.verdict, Ok(()));
    let (cells, total) = quads(&answer);
    assert_eq!(cells, 6);
    // 6 tinted cells + the outline (4 line segments).
    assert_eq!(total, 6 + 4);
}

#[test]
fn a_factory_ghost_also_draws_its_exit_cell() {
    let (mut world, defs) = world();
    // The factory needs a refinery, which needs a power plant: put them there as a scenario would.
    for (role, x, y) in [("power_plant", 40, 48), ("refinery", 40, 40)] {
        world
            .schedule(
                world.tick(),
                PlayerId(0),
                Command::PlaceStructure {
                    kind: Kind(defs.kind_of(role).unwrap()),
                    cell: Cell::new(x, y),
                    owner: PlayerId(0),
                },
            )
            .unwrap();
    }
    world.step();
    let answer = answer_for(&world, &defs, "vehicle_factory", 52, 48);
    assert_eq!(answer.preview.verdict, Ok(()));
    assert!(answer.preview.exit.is_some_and(|e| e.usable));
    let (cells, total) = quads(&answer);
    assert_eq!(cells, 12);
    // 12 cells + outline (4) + the exit cell (1 fill + 4 outline).
    assert_eq!(total, 12 + 4 + 1 + 4);
}

#[test]
fn cells_off_the_map_are_not_drawn_and_a_refused_ghost_still_shows_its_cells() {
    let (world, defs) = world();
    // Hanging off the east edge of the 64 x 64 map: 2 x 3, only one column on it.
    let answer = answer_for(&world, &defs, "power_plant", 63, 4);
    assert_eq!(answer.preview.verdict, Err(PlacementError::OffMap));
    let on_map = answer
        .preview
        .cells
        .iter()
        .filter(|c| c.1 != CellVerdict::OffMap)
        .count();
    assert_eq!(on_map, 3);
    let (cells, _) = quads_in(&answer, &frame_at((248.0, 24.0)));
    assert_eq!(cells, 3, "the three cells on the map");
}
