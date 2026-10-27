//! Placement (WP6.2): where a player may build, through the public API and a
//! hand-made unit and building table, so the rules are tested apart from the
//! shipped data (the bridge tests hold the shipped data against the same rules).
//!
//! Map 40 x 30. Player 0 is faction bit 1 with its anchor on (12, 12) (cells
//! 12 to 15), player 1 is faction bit 2 with its anchor on (28, 12), player 2
//! has no faction.

use sf_math::{Fx, Vec2};
use sf_sim::{
    BuildingFunction, BuildingStats, BuildingTable, Cell, CellVerdict, Command, ConfigError,
    Footprint, Kind, PlacementError, PlayerId, Side, StepReport, Terrain, TerrainDef, TerrainFlags,
    TerrainId, Tick, UnitStats, UnitTable, World, WorldConfig, exit_cell, gap, per_tick,
};

const MAP: (u32, u32) = (40, 30);
const MERIDIAN: u8 = 1;
const VEYRAN: u8 = 2;

const GRASS: u8 = 0;
const ROAD: u8 = 1;
const WATER: u8 = 2;
const ROCK: u8 = 3;

const ANCHOR: Kind = Kind(1); // 4 x 4, build radius 8, cannot be built
const PLANT: Kind = Kind(2); // 2 x 3
const FACTORY: Kind = Kind(3); // 3 x 4, vehicles, exit +x, needs a plant
const DECOY: Kind = Kind(4); // 1 x 1, Meridian only
const BUNKER: Kind = Kind(5); // 1 x 1, Veyran only
const TOWER: Kind = Kind(6); // 1 x 1, needs a plant
const MISSION: Kind = Kind(7); // 1 x 1, cannot be built
const SHIPYARD: Kind = Kind(8); // 3 x 3, vessels, exit -y
const RIFLEMAN: Kind = Kind(20);
const NEEDS_TWO_A: Kind = Kind(30); // 1 x 1, needs a decoy and a plant, listed in that order
const NEEDS_TWO_B: Kind = Kind(31); // the same, listed the other way round

fn units() -> UnitTable {
    let structure = |kind: Kind, hp, w, h| UnitStats {
        kind,
        hp,
        movement_class: None,
        speed_per_tick: Fx::ZERO,
        turn_per_tick: 0,
        size: Fx::ZERO,
        footprint: Some(Footprint {
            width: w,
            height: h,
        }),
        weapon_range: Fx::ZERO,
        turret_per_tick: 0,
    };
    UnitTable::new(vec![
        structure(ANCHOR, 1500, 4, 4),
        structure(PLANT, 600, 2, 3),
        structure(FACTORY, 1000, 3, 4),
        structure(DECOY, 150, 1, 1),
        structure(BUNKER, 1200, 1, 1),
        structure(TOWER, 600, 1, 1),
        structure(MISSION, 0, 1, 1),
        structure(SHIPYARD, 900, 3, 3),
        structure(NEEDS_TWO_A, 100, 1, 1),
        structure(NEEDS_TWO_B, 100, 1, 1),
        UnitStats {
            kind: RIFLEMAN,
            hp: 50,
            movement_class: Some(7),
            speed_per_tick: per_tick(Fx::from_int(1)),
            turn_per_tick: 16,
            size: Fx::from_ratio(4, 10),
            footprint: None,
            weapon_range: Fx::from_int(4),
            turret_per_tick: 0,
        },
    ])
    .unwrap()
}

#[allow(clippy::too_many_arguments)]
fn entry(
    kind: Kind,
    (w, h): (u8, u8),
    function: BuildingFunction,
    exit_side: Option<Side>,
    cost: Option<u32>,
    factions: u8,
    requires: &[Kind],
    build_radius: u16,
) -> BuildingStats {
    BuildingStats {
        kind,
        footprint: Footprint {
            width: w,
            height: h,
        },
        function,
        exit_side,
        build_ticks: if cost.is_some() { 100 } else { 0 },
        cost,
        power: 0,
        factions,
        requires: requires.to_vec(),
        build_radius,
    }
}

fn buildings(radius: u16) -> BuildingTable {
    let both = MERIDIAN | VEYRAN;
    BuildingTable::new(vec![
        entry(
            ANCHOR,
            (4, 4),
            BuildingFunction::Anchor,
            None,
            None,
            both,
            &[],
            radius,
        ),
        entry(
            PLANT,
            (2, 3),
            BuildingFunction::PowerPlant,
            None,
            Some(300),
            both,
            &[],
            0,
        ),
        entry(
            FACTORY,
            (3, 4),
            BuildingFunction::VehicleFactory,
            Some(Side::PosX),
            Some(1500),
            both,
            &[PLANT],
            0,
        ),
        entry(
            DECOY,
            (1, 1),
            BuildingFunction::Decoy,
            None,
            Some(150),
            MERIDIAN,
            &[],
            0,
        ),
        entry(
            BUNKER,
            (1, 1),
            BuildingFunction::Defence,
            None,
            Some(600),
            VEYRAN,
            &[],
            0,
        ),
        entry(
            TOWER,
            (1, 1),
            BuildingFunction::Defence,
            None,
            Some(500),
            both,
            &[PLANT],
            0,
        ),
        entry(
            MISSION,
            (1, 1),
            BuildingFunction::None,
            None,
            None,
            4,
            &[],
            0,
        ),
        entry(
            NEEDS_TWO_A,
            (1, 1),
            BuildingFunction::None,
            None,
            Some(10),
            both,
            &[DECOY, PLANT],
            0,
        ),
        entry(
            NEEDS_TWO_B,
            (1, 1),
            BuildingFunction::None,
            None,
            Some(10),
            both,
            &[PLANT, DECOY],
            0,
        ),
        entry(
            SHIPYARD,
            (3, 3),
            BuildingFunction::VesselFactory,
            Some(Side::NegY),
            Some(1000),
            both,
            &[],
            0,
        ),
    ])
    .unwrap()
}

fn terrain(specials: &[(u32, u32, u8)]) -> Terrain {
    let flags = |buildable, ground| TerrainFlags {
        buildable,
        harvestable: false,
        ground,
    };
    let defs = [
        TerrainDef {
            id: TerrainId(GRASS),
            flags: flags(true, true),
        },
        TerrainDef {
            id: TerrainId(ROAD),
            flags: flags(false, true),
        },
        TerrainDef {
            id: TerrainId(WATER),
            flags: flags(false, false),
        },
        TerrainDef {
            id: TerrainId(ROCK),
            flags: flags(false, false),
        },
    ];
    let mut ids = vec![GRASS; (MAP.0 * MAP.1) as usize];
    for &(x, y, id) in specials {
        ids[(y * MAP.0 + x) as usize] = id;
    }
    Terrain::from_layer(MAP.0, MAP.1, &ids, &defs).unwrap()
}

fn config() -> WorldConfig {
    WorldConfig::new(5).with_map_size(MAP.0, MAP.1)
}

fn bare(specials: &[(u32, u32, u8)], radius: u16) -> World {
    World::with_terrain(config(), terrain(specials))
        .unwrap()
        .with_units(units())
        .unwrap()
        .with_buildings(buildings(radius))
        .unwrap()
        .with_player_factions(&[(PlayerId(0), MERIDIAN), (PlayerId(1), VEYRAN)])
        .unwrap()
}

/// Two bases: the anchors are placed the way a scenario places them, with
/// `PlaceStructure`, and one tick has run.
fn rig(specials: &[(u32, u32, u8)]) -> World {
    let mut w = bare(specials, 8);
    place(&mut w, 0, ANCHOR, 12, 12);
    place(&mut w, 1, ANCHOR, 28, 12);
    w
}

fn place(w: &mut World, owner: u8, kind: Kind, x: u32, y: u32) {
    let tick = w.tick();
    w.schedule(
        tick,
        PlayerId(owner),
        Command::PlaceStructure {
            kind,
            cell: Cell::new(x, y),
            owner: PlayerId(owner),
        },
    )
    .unwrap();
    let r = w.step();
    assert_eq!(r.commands_executed, 1, "scenario placement of {kind:?}");
}

fn build(w: &mut World, player: u8, kind: Kind, x: u32, y: u32) -> StepReport {
    let tick = w.tick();
    w.schedule(
        tick,
        PlayerId(player),
        Command::BuildStructure {
            kind,
            cell: Cell::new(x, y),
        },
    )
    .unwrap();
    w.step()
}

fn stands(w: &World, x: u32, y: u32) -> bool {
    w.occupant_at(Cell::new(x, y)).is_some()
}

fn check(w: &World, player: u8, kind: Kind, x: u32, y: u32) -> Result<(), PlacementError> {
    w.check_placement(kind, Cell::new(x, y), PlayerId(player))
}

fn rifleman(w: &mut World, x: i32, y: i32) {
    let tick = w.tick();
    w.schedule(
        tick,
        PlayerId(0),
        Command::SpawnUnit {
            kind: RIFLEMAN,
            pos: Vec2::from_ints(x, y) + Vec2::new(Fx::HALF, Fx::HALF),
            owner: PlayerId(0),
        },
    )
    .unwrap();
    assert_eq!(w.step().commands_executed, 1);
}

// --- a build runs, for the player who sent it ---

#[test]
fn a_valid_build_stands_and_belongs_to_the_player_who_sent_it() {
    let mut w = rig(&[]);
    let report = build(&mut w, 0, PLANT, 18, 12);
    assert_eq!(
        (report.commands_executed, report.placements_rejected),
        (1, 0)
    );
    let id = w.occupant_at(Cell::new(18, 12)).unwrap();
    assert_eq!(w.get(id).unwrap().owner, PlayerId(0));
    assert_eq!(w.hp_of(id), Some(600));
    // Every cell of the 2 x 3 footprint is taken.
    for (x, y) in [(18, 12), (19, 12), (18, 14), (19, 14)] {
        assert!(stands(&w, x, y), "({x}, {y})");
    }
    assert!(!stands(&w, 20, 12));
    assert!(w.placement_rejections().is_empty());

    // The other player's build is its own, not player 0's: the owner is whoever sent the command.
    let report = build(&mut w, 1, BUNKER, 33, 12);
    assert_eq!(report.commands_executed, 1);
    let id = w.occupant_at(Cell::new(33, 12)).unwrap();
    assert_eq!(w.get(id).unwrap().owner, PlayerId(1));
    // And it is player 1's base that counts: player 0 cannot build there.
    assert_eq!(
        check(&w, 0, DECOY, 33, 13),
        Err(PlacementError::TooFarFromBase)
    );
}

#[test]
fn place_structure_stays_the_unchecked_placement_of_scenario_setup() {
    // A mission structure nobody can build, an enemy decoy of the wrong
    // faction, a tower without its power plant, far from any base, on rock:
    // none of the rules apply to `PlaceStructure` (F1-M05 pre-places enemy
    // decoys, the sandbox places every role of every side).
    let mut w = rig(&[(2, 25, ROCK)]);
    place(&mut w, 1, DECOY, 3, 3);
    place(&mut w, 0, MISSION, 36, 27);
    place(&mut w, 0, TOWER, 20, 27);
    place(&mut w, 2, BUNKER, 5, 20);
    assert!(stands(&w, 3, 3) && stands(&w, 36, 27) && stands(&w, 20, 27) && stands(&w, 5, 20));
    // Terrain and overlap are still refused, as they always were.
    let tick = w.tick();
    w.schedule(
        tick,
        PlayerId(0),
        Command::PlaceStructure {
            kind: DECOY,
            cell: Cell::new(2, 25),
            owner: PlayerId(0),
        },
    )
    .unwrap();
    let r = w.step();
    assert_eq!((r.commands_executed, r.commands_ignored), (0, 1));
    // A refused PlaceStructure is not a reported rejection: it is not a build.
    assert_eq!(r.placements_rejected, 0);
}

// --- the footprint ---

#[test]
fn every_cell_of_the_footprint_must_be_buildable_terrain() {
    // Rock, water and a road (not buildable) next to the anchor.
    for (terrain, name) in [(ROCK, "rock"), (WATER, "water"), (ROAD, "road")] {
        let mut w = rig(&[(19, 13, terrain)]);
        // A 2 x 3 plant on (18, 12) covers (19, 13) with its second column.
        assert_eq!(
            check(&w, 0, PLANT, 18, 12),
            Err(PlacementError::BadTerrain),
            "{name}"
        );
        let r = build(&mut w, 0, PLANT, 18, 12);
        assert_eq!(
            (
                r.commands_executed,
                r.commands_ignored,
                r.placements_rejected
            ),
            (0, 1, 1),
            "{name}"
        );
        assert!(!stands(&w, 18, 12), "{name}: nothing is placed in part");
        // One cell beside it is fine.
        assert_eq!(check(&w, 0, PLANT, 20, 12), Ok(()), "{name}");
    }
}

#[test]
fn a_footprint_that_leaves_the_map_is_refused() {
    let mut w = bare(&[], 8);
    // An anchor near the east edge (34..38); the map is 40 wide and 30 high.
    // A plant is 2 wide and 3 high.
    place(&mut w, 0, ANCHOR, 34, 12);
    assert_eq!(check(&w, 0, PLANT, 38, 12), Ok(()));
    assert_eq!(check(&w, 0, PLANT, 39, 12), Err(PlacementError::OffMap));
    assert_eq!(check(&w, 0, PLANT, 39, 20), Err(PlacementError::OffMap));
    // The bottom edge: rows 27 to 29 fit, 28 to 30 do not.
    assert_eq!(check(&w, 0, PLANT, 36, 20), Ok(()));
    place(&mut w, 0, ANCHOR, 34, 20);
    assert_eq!(check(&w, 0, PLANT, 36, 27), Ok(()));
    assert_eq!(check(&w, 0, PLANT, 36, 28), Err(PlacementError::OffMap));
    // A cell far outside (the arithmetic must not wrap).
    assert_eq!(
        check(&w, 0, PLANT, u32::MAX, u32::MAX),
        Err(PlacementError::OffMap)
    );
    let p = w.preview_placement(PLANT, Cell::new(38, 12), PlayerId(0));
    assert_eq!(p.cells.len(), 6);
    assert!(p.cells.iter().all(|c| c.1 == CellVerdict::Free));
    // The plant at (39, 12) has a column off the map, and only that column.
    let q = w.preview_placement(PLANT, Cell::new(39, 12), PlayerId(0));
    assert_eq!(q.cells[0], (Cell::new(39, 12), CellVerdict::Free));
    assert_eq!(q.cells[1], (Cell::new(40, 12), CellVerdict::OffMap));
}

#[test]
fn a_structure_in_the_way_blocks_even_with_one_cell() {
    let mut w = rig(&[]);
    build(&mut w, 0, PLANT, 18, 12);
    // Overlapping at a corner, in the middle, and on the same cells.
    for (x, y) in [(17, 11), (18, 12), (19, 14), (19, 13)] {
        assert_eq!(
            check(&w, 0, PLANT, x, y),
            Err(PlacementError::Occupied),
            "({x}, {y})"
        );
    }
    // Next to it is free.
    assert_eq!(check(&w, 0, PLANT, 20, 12), Ok(()));
    assert_eq!(check(&w, 0, PLANT, 16, 12), Ok(()));
    // Another player's structure is in the way too.
    assert_eq!(check(&w, 0, DECOY, 28, 12), Err(PlacementError::Occupied));
}

#[test]
fn two_builds_in_one_tick_see_each_other() {
    let mut w = rig(&[]);
    let tick = w.tick();
    for _ in 0..2 {
        w.schedule(
            tick,
            PlayerId(0),
            Command::BuildStructure {
                kind: PLANT,
                cell: Cell::new(18, 12),
            },
        )
        .unwrap();
    }
    let r = w.step();
    assert_eq!(
        (
            r.commands_executed,
            r.commands_ignored,
            r.placements_rejected
        ),
        (1, 1, 1)
    );
    let rejected = w.placement_rejections()[0];
    assert_eq!(rejected.reason, PlacementError::Occupied);
    assert_eq!((rejected.player, rejected.kind), (PlayerId(0), PLANT));
    assert_eq!((rejected.cell, rejected.tick), (Cell::new(18, 12), tick));
}

#[test]
fn a_unit_on_the_footprint_is_in_the_way_and_one_beside_it_is_not() {
    let mut w = rig(&[]);
    rifleman(&mut w, 19, 14); // the last cell of a plant on (18, 12)
    rifleman(&mut w, 17, 12); // beside it
    assert_eq!(
        check(&w, 0, PLANT, 18, 12),
        Err(PlacementError::UnitInTheWay)
    );
    let p = w.preview_placement(PLANT, Cell::new(18, 12), PlayerId(0));
    let units: Vec<Cell> = p
        .cells
        .iter()
        .filter(|(_, v)| *v == CellVerdict::Unit)
        .map(|(c, _)| *c)
        .collect();
    assert_eq!(units, vec![Cell::new(19, 14)]);
    let r = build(&mut w, 0, PLANT, 18, 12);
    assert_eq!(r.placements_rejected, 1);
    assert_eq!(
        w.placement_rejections()[0].reason,
        PlacementError::UnitInTheWay
    );
    // Two cells further east the unit is not on the footprint.
    assert_eq!(
        check(&w, 0, PLANT, 19, 12),
        Err(PlacementError::UnitInTheWay)
    );
    assert_eq!(check(&w, 0, PLANT, 20, 12), Ok(()));
}

// --- proximity ---

#[test]
fn the_gap_between_two_footprints_is_the_chessboard_distance() {
    // Anchor 12..16 x 12..16 against 1 x 1 cells.
    let anchor = (12, 12, 4, 4);
    assert_eq!(gap(anchor, (16, 12, 1, 1)), 0); // touching
    assert_eq!(gap(anchor, (17, 12, 1, 1)), 1);
    assert_eq!(gap(anchor, (24, 12, 1, 1)), 8);
    assert_eq!(gap(anchor, (11, 12, 1, 1)), 0);
    assert_eq!(gap(anchor, (10, 12, 1, 1)), 1);
    assert_eq!(gap(anchor, (3, 12, 1, 1)), 8);
    // Diagonal: the larger of the two.
    assert_eq!(gap(anchor, (24, 24, 1, 1)), 8);
    assert_eq!(gap(anchor, (24, 26, 1, 1)), 10);
    assert_eq!(gap(anchor, (14, 14, 1, 1)), 0); // overlapping
    // A wide new footprint counts from its near edge.
    assert_eq!(gap(anchor, (4, 12, 8, 1)), 0);
    assert_eq!(gap(anchor, (2, 12, 8, 1)), 2);
}

#[test]
fn the_build_radius_reaches_exactly_eight_cells_from_every_side() {
    let w = rig(&[]);
    // East: the plant (2 wide) may start 8 cells beyond the anchor's edge (x 16).
    assert_eq!(check(&w, 0, PLANT, 24, 12), Ok(()));
    assert_eq!(
        check(&w, 0, PLANT, 25, 12),
        Err(PlacementError::TooFarFromBase)
    );
    // West: the plant must end within 8 cells of x = 12: it covers 2 cells, so x >= 2.
    assert_eq!(check(&w, 0, PLANT, 2, 12), Ok(()));
    assert_eq!(
        check(&w, 0, PLANT, 1, 12),
        Err(PlacementError::TooFarFromBase)
    );
    // South (anchor ends at y 16) and north (the plant is 3 tall, so y >= 1).
    assert_eq!(check(&w, 0, PLANT, 12, 24), Ok(()));
    assert_eq!(
        check(&w, 0, PLANT, 12, 25),
        Err(PlacementError::TooFarFromBase)
    );
    assert_eq!(check(&w, 0, PLANT, 12, 1), Ok(()));
    assert_eq!(
        check(&w, 0, PLANT, 12, 0),
        Err(PlacementError::TooFarFromBase)
    );
    // The corner: both directions at the limit are fine (a square, not a circle).
    assert_eq!(check(&w, 0, DECOY, 24, 24), Ok(()));
    assert_eq!(
        check(&w, 0, DECOY, 25, 24),
        Err(PlacementError::TooFarFromBase)
    );
}

#[test]
fn only_a_standing_anchor_of_the_same_player_counts() {
    let mut w = rig(&[]);
    // Player 0 cannot build beside player 1's anchor (28, 12), 12 cells away.
    assert_eq!(
        check(&w, 0, DECOY, 33, 12),
        Err(PlacementError::TooFarFromBase)
    );
    assert_eq!(check(&w, 1, BUNKER, 33, 12), Ok(()));
    // A player with no anchor has no base.
    assert_eq!(
        check(&w, 0, DECOY, 2, 2),
        Err(PlacementError::TooFarFromBase)
    );
    // A second anchor extends the reach.
    place(&mut w, 0, ANCHOR, 2, 20);
    assert_eq!(check(&w, 0, DECOY, 3, 11), Ok(()));
    // The radius is the table's number: with 3 the reach shrinks.
    let mut narrow = bare(&[], 3);
    place(&mut narrow, 0, ANCHOR, 12, 12);
    assert_eq!(check(&narrow, 0, DECOY, 19, 12), Ok(()));
    assert_eq!(
        check(&narrow, 0, DECOY, 20, 12),
        Err(PlacementError::TooFarFromBase)
    );
}

#[test]
fn a_structure_that_is_not_an_anchor_gives_no_reach_and_a_removed_anchor_none() {
    let mut w = rig(&[]);
    // A plant at the limit does not extend the base.
    build(&mut w, 0, PLANT, 24, 12);
    assert_eq!(
        check(&w, 0, DECOY, 33, 12),
        Err(PlacementError::TooFarFromBase)
    );
    // Without a base at all there is nothing to build near.
    let mut empty = bare(&[], 8);
    assert_eq!(
        check(&empty, 0, DECOY, 12, 12),
        Err(PlacementError::TooFarFromBase)
    );
    place(&mut empty, 0, ANCHOR, 12, 12);
    assert_eq!(check(&empty, 0, DECOY, 18, 12), Ok(()));
    // Taking the anchor away takes the base with it.
    let anchor = empty.occupant_at(Cell::new(12, 12)).unwrap();
    let tick = empty.tick();
    empty
        .schedule(tick, PlayerId(0), Command::Despawn { id: anchor })
        .unwrap();
    empty.step();
    assert_eq!(
        check(&empty, 0, DECOY, 18, 12),
        Err(PlacementError::TooFarFromBase)
    );
}

// --- prerequisites and factions ---

#[test]
fn the_prerequisites_must_stand_for_the_player() {
    let mut w = rig(&[]);
    assert_eq!(
        check(&w, 0, TOWER, 18, 12),
        Err(PlacementError::MissingPrerequisite(PLANT))
    );
    let r = build(&mut w, 0, TOWER, 18, 12);
    assert_eq!(r.placements_rejected, 1);
    assert_eq!(
        w.placement_rejections()[0].reason,
        PlacementError::MissingPrerequisite(PLANT)
    );
    // The other player's plant does not count.
    place(&mut w, 1, PLANT, 24, 18);
    assert_eq!(
        check(&w, 0, TOWER, 18, 12),
        Err(PlacementError::MissingPrerequisite(PLANT))
    );
    assert_eq!(check(&w, 1, TOWER, 24, 16), Ok(()));
    // Its own plant does.
    build(&mut w, 0, PLANT, 6, 12);
    assert_eq!(check(&w, 0, TOWER, 18, 12), Ok(()));
    let r = build(&mut w, 0, TOWER, 18, 12);
    assert_eq!(r.commands_executed, 1);
}

#[test]
fn the_missing_prerequisite_named_does_not_depend_on_the_order_of_the_list() {
    // Two prerequisites missing: the lowest kind is named, whichever way round the data lists them.
    let mut w = rig(&[]);
    for kind in [NEEDS_TWO_A, NEEDS_TWO_B] {
        assert_eq!(
            check(&w, 0, kind, 18, 12),
            Err(PlacementError::MissingPrerequisite(PLANT)),
            "{kind:?}"
        );
    }
    // With the plant standing the other one is named.
    build(&mut w, 0, PLANT, 6, 12);
    for kind in [NEEDS_TWO_A, NEEDS_TWO_B] {
        assert_eq!(
            check(&w, 0, kind, 18, 12),
            Err(PlacementError::MissingPrerequisite(DECOY)),
            "{kind:?}"
        );
    }
}

#[test]
fn the_last_row_and_column_of_the_map_are_inside_it() {
    // The plant is 2 x 3: the last column pair is x 38 and 39, the last rows are 27 to 29.
    let mut w = bare(&[], 8);
    place(&mut w, 0, ANCHOR, 34, 24);
    assert_eq!(check(&w, 0, PLANT, 38, 27), Ok(()));
    assert_eq!(check(&w, 0, PLANT, 39, 27), Err(PlacementError::OffMap));
    assert_eq!(check(&w, 0, PLANT, 38, 28), Err(PlacementError::OffMap));
}

#[test]
fn a_prerequisite_whose_structure_is_gone_no_longer_counts() {
    let mut w = rig(&[]);
    build(&mut w, 0, PLANT, 6, 12);
    assert_eq!(check(&w, 0, TOWER, 18, 12), Ok(()));
    let plant = w.occupant_at(Cell::new(6, 12)).unwrap();
    let tick = w.tick();
    w.schedule(tick, PlayerId(0), Command::Despawn { id: plant })
        .unwrap();
    w.step();
    assert_eq!(
        check(&w, 0, TOWER, 18, 12),
        Err(PlacementError::MissingPrerequisite(PLANT))
    );
}

#[test]
fn the_factions_of_a_structure_restrict_who_builds_it() {
    let mut w = rig(&[]);
    // Meridian-only decoy: player 0 yes, player 1 no (next to its own anchor).
    assert_eq!(check(&w, 0, DECOY, 18, 12), Ok(()));
    assert_eq!(
        check(&w, 1, DECOY, 33, 12),
        Err(PlacementError::WrongFaction)
    );
    // Veyran-only bunker the other way round.
    assert_eq!(check(&w, 1, BUNKER, 33, 12), Ok(()));
    assert_eq!(
        check(&w, 0, BUNKER, 18, 12),
        Err(PlacementError::WrongFaction)
    );
    // Shared structures: both.
    assert_eq!(check(&w, 0, PLANT, 18, 12), Ok(()));
    assert_eq!(check(&w, 1, PLANT, 33, 12), Ok(()));
    // A player with no faction builds nothing.
    place(&mut w, 2, ANCHOR, 2, 2);
    assert_eq!(check(&w, 2, PLANT, 8, 2), Err(PlacementError::NoFaction));
    let r = build(&mut w, 1, DECOY, 33, 12);
    assert_eq!(r.placements_rejected, 1);
    assert_eq!(
        w.placement_rejections()[0].reason,
        PlacementError::WrongFaction
    );
    assert!(!stands(&w, 33, 12));
}

#[test]
fn the_anchor_and_the_mission_structures_cannot_be_built() {
    let w = rig(&[]);
    assert_eq!(
        check(&w, 0, ANCHOR, 20, 12),
        Err(PlacementError::CannotBeBuilt)
    );
    assert_eq!(
        check(&w, 0, MISSION, 18, 12),
        Err(PlacementError::CannotBeBuilt)
    );
    // Not a structure at all, and a kind nobody defined.
    assert_eq!(
        check(&w, 0, RIFLEMAN, 18, 12),
        Err(PlacementError::UnknownKind)
    );
    assert_eq!(
        check(&w, 0, Kind(99), 18, 12),
        Err(PlacementError::UnknownKind)
    );
}

#[test]
fn a_world_without_building_rules_builds_nothing() {
    let mut w = World::with_terrain(config(), terrain(&[]))
        .unwrap()
        .with_units(units())
        .unwrap();
    assert_eq!(
        check(&w, 0, PLANT, 5, 5),
        Err(PlacementError::NoBuildingRules)
    );
    let r = build(&mut w, 0, PLANT, 5, 5);
    assert_eq!((r.commands_ignored, r.placements_rejected), (1, 1));
}

#[test]
fn player_factions_are_checked_when_they_are_given() {
    let base = || {
        World::with_terrain(config(), terrain(&[]))
            .unwrap()
            .with_units(units())
            .unwrap()
    };
    let err = |list: &[(PlayerId, u8)]| base().with_player_factions(list).unwrap_err();
    // Not exactly one bit.
    assert_eq!(
        err(&[(PlayerId(0), 0)]),
        ConfigError::PlayerFaction(PlayerId(0))
    );
    assert_eq!(
        err(&[(PlayerId(3), 3)]),
        ConfigError::PlayerFaction(PlayerId(3))
    );
    // A player twice.
    assert_eq!(
        err(&[(PlayerId(1), 1), (PlayerId(1), 2)]),
        ConfigError::PlayerFaction(PlayerId(1))
    );
    // Too late.
    let mut started = base();
    started.step();
    assert_eq!(
        started.with_player_factions(&[]).unwrap_err(),
        ConfigError::PlayersAfterStart
    );
    // The order of the list does not matter.
    let a = base()
        .with_player_factions(&[(PlayerId(1), 2), (PlayerId(0), 1)])
        .unwrap();
    assert_eq!(a.player_factions(), &[(PlayerId(0), 1), (PlayerId(1), 2)]);
    assert_eq!(a.faction_of(PlayerId(1)), Some(2));
    assert_eq!(a.faction_of(PlayerId(5)), None);
}

// --- the exit of a factory ---

#[test]
fn the_exit_cell_is_next_to_the_middle_of_the_exit_side() {
    let fp = Footprint {
        width: 3,
        height: 4,
    };
    let at = Cell::new(10, 10);
    assert_eq!(exit_cell(at, fp, Side::PosX), (13, 12));
    assert_eq!(exit_cell(at, fp, Side::NegX), (9, 12));
    assert_eq!(exit_cell(at, fp, Side::PosY), (11, 14));
    assert_eq!(exit_cell(at, fp, Side::NegY), (11, 9));
    // On the edge of the map the cell is outside it: negative.
    assert_eq!(exit_cell(Cell::new(0, 0), fp, Side::NegX), (-1, 2));
}

#[test]
fn a_factory_needs_a_free_passable_exit_cell() {
    // The factory (3 x 4) on (18, 12) covers x 18..21 and its exit is (21, 14).
    let free = rig(&[]);
    let mut w = free.clone();
    build(&mut w, 0, PLANT, 6, 12);
    let p = w.preview_placement(FACTORY, Cell::new(18, 12), PlayerId(0));
    assert_eq!(p.verdict, Ok(()));
    assert_eq!(
        p.exit.map(|e| (e.cell, e.usable)),
        Some((Cell::new(21, 14), true))
    );

    // Water on the exit cell: units cannot come out.
    // Rock too. Neither is the factory's own footprint, which is fine.
    for terrain in [WATER, ROCK] {
        let mut blocked = rig(&[(21, 14, terrain)]);
        build(&mut blocked, 0, PLANT, 6, 12);
        assert_eq!(
            check(&blocked, 0, FACTORY, 18, 12),
            Err(PlacementError::ExitBlocked)
        );
        let r = build(&mut blocked, 0, FACTORY, 18, 12);
        assert_eq!(r.placements_rejected, 1);
        assert!(!stands(&blocked, 18, 12));
    }
    // A road is passable although it is not buildable: the exit may be on it.
    let mut road = rig(&[(21, 14, ROAD)]);
    build(&mut road, 0, PLANT, 6, 12);
    assert_eq!(check(&road, 0, FACTORY, 18, 12), Ok(()));

    // A structure on the exit cell.
    let mut walled = rig(&[]);
    build(&mut walled, 0, PLANT, 6, 12);
    build(&mut walled, 0, DECOY, 21, 14);
    assert_eq!(
        check(&walled, 0, FACTORY, 18, 12),
        Err(PlacementError::ExitBlocked)
    );
    // The cell beside the exit does not matter: a structure on (21, 13) leaves it free.
    let mut beside = rig(&[]);
    build(&mut beside, 0, PLANT, 6, 12);
    build(&mut beside, 0, DECOY, 21, 13);
    assert_eq!(check(&beside, 0, FACTORY, 18, 12), Ok(()));

    // The exit off the map.
    let mut edge = bare(&[], 8);
    place(&mut edge, 0, ANCHOR, 30, 12);
    place(&mut edge, 0, PLANT, 26, 12);
    // The factory is 3 wide: on (37, 12) it fits (columns 37 to 39) but its exit
    // would be column 40.
    assert_eq!(
        check(&edge, 0, FACTORY, 38, 12),
        Err(PlacementError::OffMap)
    );
    assert_eq!(
        check(&edge, 0, FACTORY, 37, 12),
        Err(PlacementError::ExitBlocked)
    );
    assert_eq!(
        edge.preview_placement(FACTORY, Cell::new(37, 12), PlayerId(0))
            .exit,
        None
    );
    // On (36, 12) the exit is column 39, the last one.
    assert_eq!(check(&edge, 0, FACTORY, 36, 12), Ok(()));

    // A unit standing on the exit does not block it: units move away.
    let mut crowded = rig(&[]);
    build(&mut crowded, 0, PLANT, 6, 12);
    rifleman(&mut crowded, 21, 14);
    assert_eq!(check(&crowded, 0, FACTORY, 18, 12), Ok(()));
}

#[test]
fn a_building_that_is_not_a_factory_has_no_exit_rule() {
    let w = rig(&[]);
    let p = w.preview_placement(PLANT, Cell::new(18, 12), PlayerId(0));
    assert_eq!(p.exit, None);
    assert_eq!(p.verdict, Ok(()));
}

#[test]
fn a_vessel_factory_needs_its_exit_free_but_not_ground() {
    // The shipyard (3 x 3) on (18, 12) has its exit on -y: (19, 11). Water there
    // is where a vessel comes out; a structure there is not.
    let on_water = rig(&[(19, 11, WATER)]);
    assert_eq!(check(&on_water, 0, SHIPYARD, 18, 12), Ok(()));
    let mut blocked = rig(&[]);
    build(&mut blocked, 0, DECOY, 19, 11);
    assert_eq!(
        check(&blocked, 0, SHIPYARD, 18, 12),
        Err(PlacementError::ExitBlocked)
    );
}

// --- one rule for the sim and the picture ---

#[test]
fn the_preview_and_the_command_never_disagree() {
    // For every anchor cell of a window around the base, for several kinds and
    // players: the verdict of the preview says whether the build stands.
    let mut base = rig(&[(19, 13, ROCK), (22, 20, WATER), (21, 14, ROAD)]);
    build(&mut base, 0, PLANT, 6, 12);
    rifleman(&mut base, 24, 13);
    let mut agree = 0;
    let mut stood = 0;
    for kind in [PLANT, FACTORY, DECOY, BUNKER, TOWER, SHIPYARD, MISSION] {
        for player in [0, 1, 2] {
            for y in (8..22).step_by(3) {
                for x in (4..30).step_by(2) {
                    let preview = base.preview_placement(kind, Cell::new(x, y), PlayerId(player));
                    let mut w = base.clone();
                    let r = build(&mut w, player, kind, x, y);
                    assert_eq!(
                        preview.verdict.is_ok(),
                        r.commands_executed == 1,
                        "{kind:?} p{player} ({x}, {y}): {:?}",
                        preview.verdict
                    );
                    assert_eq!(
                        preview.verdict.err(),
                        w.placement_rejections().first().map(|j| j.reason),
                        "{kind:?} p{player} ({x}, {y})"
                    );
                    agree += 1;
                    stood += u32::from(stands(&w, x, y));
                }
            }
        }
    }
    // The window has both outcomes in quantity, so neither side is vacuous.
    assert!(agree > 500, "{agree} placements compared");
    assert!(stood > 20 && stood < agree - 20, "{stood} of {agree} stood");
}

#[test]
fn the_preview_marks_each_cell() {
    let mut w = rig(&[(19, 13, ROCK), (18, 14, WATER)]);
    rifleman(&mut w, 19, 12);
    build(&mut w, 0, DECOY, 18, 13);
    // A plant on (18, 12): (18,12) free, (19,12) unit, (18,13) structure,
    // (19,13) rock, (18,14) water, (19,14) free.
    let p = w.preview_placement(PLANT, Cell::new(18, 12), PlayerId(0));
    let verdicts: Vec<CellVerdict> = p.cells.iter().map(|c| c.1).collect();
    assert_eq!(
        verdicts,
        vec![
            CellVerdict::Free,
            CellVerdict::Unit,
            CellVerdict::Structure,
            CellVerdict::BadTerrain,
            CellVerdict::BadTerrain,
            CellVerdict::Free,
        ]
    );
    // The reason is the worst one: terrain before structure before unit.
    assert_eq!(p.verdict, Err(PlacementError::BadTerrain));
    // A kind that cannot be built still shows its cells.
    let m = w.preview_placement(MISSION, Cell::new(18, 12), PlayerId(0));
    assert_eq!(m.cells.len(), 1);
    assert_eq!(m.verdict, Err(PlacementError::CannotBeBuilt));
    // An unknown kind has none.
    let u = w.preview_placement(Kind(99), Cell::new(18, 12), PlayerId(0));
    assert!(u.cells.is_empty() && u.footprint.is_none());
}

// --- the checksum and determinism ---

#[test]
fn rejections_are_replaced_by_every_step_and_the_run_is_deterministic() {
    let run = || {
        let mut w = rig(&[]);
        let mut sums = Vec::new();
        for step in 0..6u32 {
            let tick = w.tick();
            // Alternate a good build and one of the other faction's bunker.
            let (kind, x) = if step % 2 == 0 {
                (PLANT, 18)
            } else {
                (BUNKER, 20)
            };
            w.schedule(
                tick,
                PlayerId(0),
                Command::BuildStructure {
                    kind,
                    cell: Cell::new(x, 20 - 2 * step),
                },
            )
            .unwrap();
            let r = w.step();
            sums.push((r.checksum, r.placements_rejected));
            assert_eq!(
                w.placement_rejections().len(),
                r.placements_rejected as usize
            );
        }
        sums
    };
    let (a, b) = (run(), run());
    assert_eq!(a, b);
    // The odd steps (a Veyran bunker for a Meridian player) are the rejected ones.
    assert_eq!(
        a.iter().map(|s| s.1).collect::<Vec<_>>(),
        vec![0, 1, 0, 1, 0, 1]
    );
}

#[test]
fn the_factions_and_the_build_radius_are_in_the_checksum() {
    let plain = |factions: Option<&[(PlayerId, u8)]>, radius: u16| {
        let w = World::with_terrain(config(), terrain(&[]))
            .unwrap()
            .with_units(units())
            .unwrap()
            .with_buildings(buildings(radius))
            .unwrap();
        match factions {
            Some(list) => w.with_player_factions(list).unwrap(),
            None => w,
        }
    };
    let own = [(PlayerId(0), MERIDIAN), (PlayerId(1), VEYRAN)];
    let swapped = [(PlayerId(0), MERIDIAN), (PlayerId(1), MERIDIAN)];
    let base = plain(Some(&own), 8).checksum();
    assert_ne!(base, plain(Some(&swapped), 8).checksum());
    assert_ne!(base, plain(Some(&own), 9).checksum());
    assert_ne!(base, plain(None, 8).checksum());
    assert_eq!(base, plain(Some(&[own[1], own[0]]), 8).checksum());
    // A pending build is state: it is hashed like every pending command.
    let pending = |kind, x, y| {
        let mut w = plain(Some(&own), 8);
        w.schedule(
            Tick(3),
            PlayerId(0),
            Command::BuildStructure {
                kind,
                cell: Cell::new(x, y),
            },
        )
        .unwrap();
        w.checksum()
    };
    assert_ne!(base, pending(PLANT, 1, 1));
    // Each field of the pending build on its own: kind, x and y.
    assert_ne!(pending(PLANT, 1, 1), pending(BUNKER, 1, 1));
    assert_ne!(pending(PLANT, 1, 1), pending(PLANT, 2, 1));
    assert_ne!(pending(PLANT, 1, 1), pending(PLANT, 1, 2));
}
