use super::*;
use crate::playfield::with_buildings;
use crate::sim_thread::SimConfig;
use sf_sim::{World, WorldConfig};

fn defs() -> UnitDefs {
    UnitDefs::embedded()
}

fn ids(mode: &BuildMode) -> Vec<&str> {
    mode.choices().iter().map(|c| c.id.as_str()).collect()
}

#[test]
fn each_faction_is_offered_the_shipped_structures_it_may_build() {
    let defs = defs();
    let meridian = BuildMode::for_faction(&defs, Faction::Meridian);
    let veyran = BuildMode::for_faction(&defs, Faction::Veyran);
    let shared = [
        "power_plant",
        "refinery",
        "barracks",
        "vehicle_factory",
        "radar",
        "defence_tower",
        "advanced_defences",
        "shipyard",
    ];
    for id in shared {
        assert!(ids(&meridian).contains(&id), "meridian {id}");
        assert!(ids(&veyran).contains(&id), "veyran {id}");
    }
    assert!(ids(&meridian).contains(&"decoy_emplacement"));
    assert!(!ids(&meridian).contains(&"fortified_bunker"));
    assert!(ids(&veyran).contains(&"fortified_bunker"));
    assert!(!ids(&veyran).contains(&"decoy_emplacement"));
    // The base anchor and the mission structures have no price and are never offered.
    for id in ["construction_center", "relay_transmitter", "bridge_segment"] {
        assert!(!ids(&meridian).contains(&id), "{id}");
        assert!(!ids(&veyran).contains(&id), "{id}");
    }
    // Nine each, in kind order.
    assert_eq!((meridian.choices().len(), veyran.choices().len()), (9, 9));
    let kinds: Vec<u16> = meridian.choices().iter().map(|c| c.kind.0).collect();
    assert!(kinds.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn b_cycles_through_the_choices_and_back_to_none() {
    let mut mode = BuildMode::for_faction(&defs(), Faction::Meridian);
    assert!(!mode.is_active());
    let mut seen = Vec::new();
    for _ in 0..mode.choices().len() {
        mode.cycle();
        assert!(mode.is_active());
        seen.push(mode.chosen().unwrap().id.clone());
    }
    assert_eq!(seen.len(), 9);
    let mut distinct = seen.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), 9);
    mode.cycle();
    assert!(!mode.is_active());
    mode.cycle();
    assert_eq!(mode.chosen().unwrap().id, seen[0]);
    mode.cancel();
    assert!(!mode.is_active());
}

#[test]
fn a_structure_is_chosen_by_name_and_only_if_the_faction_builds_it() {
    let mut mode = BuildMode::for_faction(&defs(), Faction::Veyran);
    assert!(mode.choose("fortified_bunker"));
    assert_eq!(mode.chosen().unwrap().id, "fortified_bunker");
    assert!(!mode.choose("decoy_emplacement"));
    assert!(!mode.choose("no_such_role"));
    // A refused choice leaves the old one.
    assert_eq!(mode.chosen().unwrap().id, "fortified_bunker");
}

#[test]
fn the_question_and_the_click_follow_the_cursor_cell_or_the_pinned_one() {
    let defs = defs();
    let mut mode = BuildMode::for_faction(&defs, Faction::Meridian);
    let me = PlayerId(0);
    // Nothing chosen: nothing is asked and a click sends nothing.
    assert_eq!(mode.query(me, Some(Cell::new(3, 4))), None);
    assert_eq!(mode.click(Some(Cell::new(3, 4))), None);
    mode.choose("barracks");
    let kind = Kind(defs.kind_of("barracks").unwrap());
    // The barracks is 2 x 3: the cursor is on its middle cell, so the anchor is one column
    // left and one row up.
    let anchor = Cell::new(2, 3);
    assert_eq!(
        mode.query(me, Some(Cell::new(3, 4))),
        Some(GhostQuery {
            kind,
            cell: anchor,
            player: me
        })
    );
    assert_eq!(
        mode.click(Some(Cell::new(3, 4))),
        Some(Command::BuildStructure { kind, cell: anchor })
    );
    // The cursor off the ground: no question, no command.
    assert_eq!(mode.query(me, None), None);
    assert_eq!(mode.click(None), None);
    // A pinned cell stands in for the cursor, and is the anchor itself.
    mode.pin(Cell::new(9, 9));
    assert_eq!(mode.query(me, None).unwrap().cell, Cell::new(9, 9));
    assert_eq!(
        mode.query(me, Some(Cell::new(3, 4))).unwrap().cell,
        Cell::new(9, 9)
    );
    assert_eq!(
        mode.click(None),
        Some(Command::BuildStructure {
            kind,
            cell: Cell::new(9, 9)
        })
    );
}

#[test]
fn the_ghost_is_centred_on_the_cursor_and_kept_on_the_map() {
    let defs = defs();
    let mut mode = BuildMode::for_faction(&defs, Faction::Meridian);
    let at = |mode: &BuildMode, x, y| mode.target(Some(Cell::new(x, y))).unwrap();
    // A 1 x 1 structure sits on the cursor cell.
    mode.choose("defence_tower");
    assert_eq!(at(&mode, 10, 10), Cell::new(10, 10));
    // A 3 x 4 one (vehicle factory) is centred: one left, two up.
    mode.choose("vehicle_factory");
    assert_eq!(at(&mode, 10, 10), Cell::new(9, 8));
    // At the north-west corner it does not go below zero.
    assert_eq!(at(&mode, 0, 1), Cell::new(0, 0));
}

#[test]
fn the_sim_thread_is_asked_only_when_the_question_changes() {
    let defs = defs();
    let world = with_buildings(
        World::new(WorldConfig::new(1).with_map_size(32, 32))
            .unwrap()
            .with_units(sf_bridge::unit_table(&defs).unwrap())
            .unwrap(),
        &defs,
    );
    let sim = SimHandle::spawn(world, SimConfig::manual()).unwrap();
    let mut mode = BuildMode::for_faction(&defs, Faction::Meridian);
    mode.choose("power_plant");
    let q = mode.query(PlayerId(0), Some(Cell::new(5, 5)));
    assert!(mode.ask(&sim, q));
    assert!(!mode.ask(&sim, q), "the same question is not sent twice");
    let q2 = mode.query(PlayerId(0), Some(Cell::new(6, 5)));
    assert!(mode.ask(&sim, q2));
    assert!(
        mode.ask(&sim, None),
        "putting the structure away is a change"
    );
    assert!(!mode.ask(&sim, None));
}
