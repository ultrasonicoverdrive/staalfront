//! WP6.2: placement against the shipped data. The rules themselves are tested
//! with a hand-made table in `sf_sim/tests/placement.rs`; here every number
//! they read is held against `assets/config/unit_defs.ron` and
//! `terrain_types.ron`, and both factions build the whole shipped tech tree
//! with `Command::BuildStructure` and nothing else.

use sf_bridge::{building_table, terrain_defs, unit_table};
use sf_data::movement::MovementCosts;
use sf_data::terrain::TerrainTypes;
use sf_data::units::{Faction, UnitDefs};
use sf_sim::{Cell, Command, Kind, PlacementError, PlayerId, Terrain, World, WorldConfig};

const SIDE: u32 = 96;
const MERIDIAN: PlayerId = PlayerId(0);
const VEYRAN: PlayerId = PlayerId(1);

fn defs() -> UnitDefs {
    UnitDefs::embedded()
}

fn kind(defs: &UnitDefs, id: &str) -> Kind {
    Kind(defs.kind_of(id).unwrap())
}

/// Shipped terrain: `layer` is the ids of the map, `SIDE * SIDE` of them.
fn world_on(defs: &UnitDefs, layer: &[u8]) -> World {
    let types = TerrainTypes::embedded();
    let costs = MovementCosts::embedded();
    let terrain = Terrain::from_layer(SIDE, SIDE, layer, &terrain_defs(&types, &costs)).unwrap();
    World::with_terrain(WorldConfig::new(3).with_map_size(SIDE, SIDE), terrain)
        .unwrap()
        .with_units(unit_table(defs).unwrap())
        .unwrap()
        .with_buildings(building_table(defs).unwrap())
        .unwrap()
        .with_player_factions(&[
            (MERIDIAN, Faction::Meridian.bit()),
            (VEYRAN, Faction::Veyran.bit()),
        ])
        .unwrap()
}

fn grass(defs: &UnitDefs) -> World {
    let types = TerrainTypes::embedded();
    world_on(
        defs,
        &vec![types.id_of("grass").unwrap(); (SIDE * SIDE) as usize],
    )
}

fn send(w: &mut World, player: PlayerId, command: Command) -> sf_sim::StepReport {
    let tick = w.tick();
    w.schedule(tick, player, command).unwrap();
    w.step()
}

fn place(w: &mut World, defs: &UnitDefs, owner: PlayerId, id: &str, x: u32, y: u32) {
    let r = send(
        w,
        owner,
        Command::PlaceStructure {
            kind: kind(defs, id),
            cell: Cell::new(x, y),
            owner,
        },
    );
    assert_eq!(r.commands_executed, 1, "{id} placed at ({x}, {y})");
}

fn check(w: &World, player: PlayerId, k: Kind, x: u32, y: u32) -> Result<(), PlacementError> {
    w.check_placement(k, Cell::new(x, y), player)
}

/// The first cell of a window around `(cx, cy)`, row by row, where `player`
/// may build `k` (by the sim's own rule), if any.
fn spot(w: &World, player: PlayerId, k: Kind, cx: u32, cy: u32) -> Option<(u32, u32)> {
    (cy - 14..cy + 14)
        .flat_map(|y| (cx - 14..cx + 14).map(move |x| (x, y)))
        .find(|&(x, y)| check(w, player, k, x, y).is_ok())
}

// --- the numbers placement reads, held against the data ---

#[test]
fn the_build_radius_and_the_faction_bits_come_from_the_data() {
    let defs = defs();
    let table = building_table(&defs).unwrap();
    // The one anchor has radius 8 (`flags: (build_radius: 8)`); nothing else has one.
    for b in table.iter() {
        let expected = if b.kind == kind(&defs, "construction_center") {
            8
        } else {
            0
        };
        assert_eq!(b.build_radius, expected, "kind {}", b.kind.0);
    }
    assert_eq!(
        defs.sim_building(kind(&defs, "construction_center").0)
            .unwrap()
            .build_radius
            .floor_int(),
        8
    );
    // The bits are the data's, and the shipped masks are what the docs say.
    assert_eq!(
        (
            Faction::Meridian.bit(),
            Faction::Veyran.bit(),
            Faction::Neutral.bit()
        ),
        (1, 2, 4)
    );
    let mask = |id: &str| table.get(kind(&defs, id)).unwrap().factions;
    assert_eq!(mask("decoy_emplacement"), 1, "Meridian only");
    assert_eq!(mask("fortified_bunker"), 2, "Veyran only");
    assert_eq!(mask("power_plant"), 3);
    assert_eq!(
        mask("relay_transmitter"),
        4,
        "mission structures are Neutral"
    );
}

#[test]
fn only_grass_is_built_on_in_the_shipped_terrain() {
    let defs = defs();
    let types = TerrainTypes::embedded();
    let mut buildable = Vec::new();
    for t in types.iter() {
        // A map of this one type with the anchor's ground (a patch of grass) under it.
        let grass_id = types.id_of("grass").unwrap();
        let mut layer = vec![grass_id; (SIDE * SIDE) as usize];
        layer[(40 * SIDE + 50) as usize] = t.id;
        let mut w = world_on(&defs, &layer);
        place(&mut w, &defs, MERIDIAN, "construction_center", 40, 36);
        let verdict = check(&w, MERIDIAN, kind(&defs, "power_plant"), 50, 40);
        if t.buildable {
            assert_eq!(verdict, Ok(()), "{}", t.name);
            buildable.push(t.name.clone());
        } else {
            assert_eq!(verdict, Err(PlacementError::BadTerrain), "{}", t.name);
        }
    }
    // Water and bridges are not built on, nor rock, road or ore.
    assert_eq!(buildable, vec!["grass"]);
}

// --- the shipped tech tree, built by both factions ---

const TREE: [&str; 11] = [
    "power_plant",
    "refinery",
    "barracks",
    "vehicle_factory",
    "radar",
    "defence_tower",
    "advanced_defences",
    "shipyard",
    // The two faction structures last; each side builds the one that is its own.
    "decoy_emplacement",
    "fortified_bunker",
    "",
];

fn build_the_tree(player: PlayerId, faction: Faction) {
    let defs = defs();
    let mut w = grass(&defs);
    let anchor = (40, 40);
    place(
        &mut w,
        &defs,
        player,
        "construction_center",
        anchor.0,
        anchor.1,
    );
    // Another player's base elsewhere does not matter.
    place(
        &mut w,
        &defs,
        if player == MERIDIAN { VEYRAN } else { MERIDIAN },
        "construction_center",
        8,
        8,
    );
    let table = building_table(&defs).unwrap();
    let mut built: Vec<&str> = vec!["construction_center"];
    // Round after round, the structures the rules allow right now: a
    // structure whose prerequisites stand is built, one that lacks one is
    // refused for exactly that.
    let mut left: Vec<&str> = TREE.iter().copied().filter(|s| !s.is_empty()).collect();
    for round in 0..8 {
        let mut next = Vec::new();
        for id in left.clone() {
            let k = kind(&defs, id);
            let b = table.get(k).unwrap();
            let mine = b.factions & faction.bit() != 0;
            let missing = b
                .requires
                .iter()
                .find(|&&r| !built.iter().any(|&bid| kind(&defs, bid) == r));
            if !mine {
                // The other faction's structure: refused for that, near the base.
                let any = (30..50)
                    .flat_map(|y| (30..50).map(move |x| (x, y)))
                    .map(|(x, y)| check(&w, player, k, x, y))
                    .collect::<Vec<_>>();
                assert!(
                    any.iter()
                        .all(|r| matches!(r, Err(PlacementError::WrongFaction))),
                    "{id} for {faction:?}: {:?}",
                    any.iter()
                        .find(|r| !matches!(r, Err(PlacementError::WrongFaction)))
                );
                continue;
            }
            match missing {
                Some(&m) => {
                    let probe = check(&w, player, k, anchor.0 + 6, anchor.1);
                    assert_eq!(
                        probe,
                        Err(PlacementError::MissingPrerequisite(m)),
                        "round {round}: {id}"
                    );
                    next.push(id);
                }
                None => {
                    let (x, y) = spot(&w, player, k, anchor.0, anchor.1)
                        .unwrap_or_else(|| panic!("round {round}: room for {id}"));
                    let r = send(
                        &mut w,
                        player,
                        Command::BuildStructure {
                            kind: k,
                            cell: Cell::new(x, y),
                        },
                    );
                    assert_eq!(r.commands_executed, 1, "round {round}: {id} at ({x}, {y})");
                    let at = w.occupant_at(Cell::new(x, y)).unwrap();
                    assert_eq!(w.get(at).unwrap().owner, player);
                    // WP6.3: it goes up first; it counts as built (for the next round) once it
                    // has run its build time from the data, not before.
                    assert!(!w.stands(at), "{id} stands the moment it is placed");
                    assert_eq!(w.construction_of(at).map(|c| c.total), Some(b.build_ticks));
                    for _ in 0..b.build_ticks {
                        assert!(!w.stands(at), "{id} stands before its build time");
                        w.step();
                    }
                    assert!(w.stands(at), "{id} does not stand after {} ticks", b.build_ticks);
                    built.push(id);
                }
            }
        }
        left = next;
        if left.is_empty() {
            break;
        }
    }
    assert!(left.is_empty(), "{faction:?} could not build {left:?}");
    // Everything the faction may build stands: the shared tree and its own faction structure.
    // The anchor, the eight shared structures and the faction's own one (decoy or bunker).
    assert_eq!(built.len(), 10, "{built:?}");
}

#[test]
fn meridian_builds_the_whole_shipped_tree_with_build_commands() {
    build_the_tree(MERIDIAN, Faction::Meridian);
}

#[test]
fn veyran_builds_the_whole_shipped_tree_with_build_commands() {
    build_the_tree(VEYRAN, Faction::Veyran);
}

#[test]
fn from_the_anchor_alone_only_the_power_plant_can_be_built() {
    let defs = defs();
    let mut w = grass(&defs);
    place(&mut w, &defs, MERIDIAN, "construction_center", 40, 40);
    let table = building_table(&defs).unwrap();
    let mut open = Vec::new();
    for b in table
        .iter()
        .filter(|b| b.cost.is_some() && b.factions & 1 != 0)
    {
        if check(&w, MERIDIAN, b.kind, 46, 40).is_ok() {
            open.push(b.kind);
        }
    }
    assert_eq!(open, vec![kind(&defs, "power_plant")]);
    // The anchor and the mission structures are never offered.
    for id in [
        "construction_center",
        "relay_transmitter",
        "bridge_segment",
        "rail_spur",
    ] {
        assert_eq!(
            check(&w, MERIDIAN, kind(&defs, id), 46, 40),
            Err(PlacementError::CannotBeBuilt),
            "{id}"
        );
    }
}

// --- the mission F1-M05 ---

#[test]
fn f1_m05_the_player_builds_the_decoy_and_the_enemys_preplaced_decoys_stand() {
    // F1-M05: the player (Meridian) starts with construction_center, power_plant,
    // barracks and vehicle_factory; the decoy requires the barracks. Enemy 1 (Veyran) has
    // 8 decoys PLACED as mission objects, although the decoy is Meridian-only to build.
    let defs = defs();
    let mut w = grass(&defs);
    for (id, x, y) in [
        ("construction_center", 40, 40),
        ("power_plant", 48, 40),
        ("barracks", 48, 44),
        ("vehicle_factory", 40, 46),
    ] {
        place(&mut w, &defs, MERIDIAN, id, x, y);
    }
    for n in 0..8 {
        place(&mut w, &defs, VEYRAN, "decoy_emplacement", 10 + 3 * n, 70);
    }
    // The decoys of the enemy are there, and are the enemy's.
    for n in 0..8 {
        let id = w.occupant_at(Cell::new(10 + 3 * n, 70)).unwrap();
        assert_eq!(w.get(id).unwrap().owner, VEYRAN);
    }
    // The player can build a decoy at once...
    let decoy = kind(&defs, "decoy_emplacement");
    let (x, y) = spot(&w, MERIDIAN, decoy, 40, 40).expect("a spot for the decoy");
    let r = send(
        &mut w,
        MERIDIAN,
        Command::BuildStructure {
            kind: decoy,
            cell: Cell::new(x, y),
        },
    );
    assert_eq!((r.commands_executed, r.placements_rejected), (1, 0));
    // ...and the Veyran player cannot build one, next to its own base or anywhere.
    place(&mut w, &defs, VEYRAN, "construction_center", 40, 20);
    place(&mut w, &defs, VEYRAN, "power_plant", 48, 20);
    place(&mut w, &defs, VEYRAN, "barracks", 48, 24);
    assert_eq!(
        check(&w, VEYRAN, decoy, 46, 20),
        Err(PlacementError::WrongFaction)
    );
    let r = send(
        &mut w,
        VEYRAN,
        Command::BuildStructure {
            kind: decoy,
            cell: Cell::new(46, 20),
        },
    );
    assert_eq!((r.commands_ignored, r.placements_rejected), (1, 1));
    assert_eq!(
        w.placement_rejections()[0].reason,
        PlacementError::WrongFaction
    );
}

// --- the checksum sees the shipped numbers ---

#[test]
fn the_shipped_building_digest_did_not_move_and_placement_has_its_own() {
    let defs = defs();
    let table = building_table(&defs).unwrap();
    // WP6.1's pin (see tests/buildings.rs): the build radius is not in it.
    assert_eq!(table.digest(), 0x53ae_b53a_b441_57d1);
    assert_ne!(table.placement_digest(), 0);
    assert_ne!(table.placement_digest(), table.digest());
}
