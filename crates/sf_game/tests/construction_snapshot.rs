//! WP6.3: the presentation reads the construction state from the snapshot the sim thread publishes,
//! and picks the stage model from it. The state is the simulation's; nothing here counts ticks.

use std::path::PathBuf;
use std::time::Duration;

use sf_data::units::UnitDefs;
use sf_game::looks::Looks;
use sf_game::playfield::with_buildings;
use sf_game::population::Population;
use sf_game::roster::Roster;
use sf_game::sim_thread::{SimConfig, SimHandle};
use sf_render::Style;
use sf_sim::{Cell, Command, Kind, PlayerId, Tick, World, WorldConfig};

const ME: PlayerId = PlayerId(0);

fn kind(defs: &UnitDefs, id: &str) -> Kind {
    Kind(defs.kind_of(id).unwrap())
}

fn world(defs: &UnitDefs) -> World {
    let mut world = with_buildings(
        World::new(WorldConfig::new(9).with_map_size(64, 64))
            .unwrap()
            .with_units(sf_bridge::unit_table(defs).unwrap())
            .unwrap(),
        defs,
    );
    world
        .schedule(
            Tick(0),
            ME,
            Command::PlaceStructure {
                kind: kind(defs, "construction_center"),
                cell: Cell::new(20, 20),
                owner: ME,
            },
        )
        .unwrap();
    world.step();
    world
}

#[test]
fn the_snapshot_carries_the_construction_and_the_stage_follows_it() {
    let defs = UnitDefs::embedded();
    let plant = kind(&defs, "power_plant");
    let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let atlas = sf_render::IconAtlas::load(&assets.join("icons"));
    let base = Style::embedded().scaled(3.0);
    // Boxes for every role: the stage chosen is what is tested here, not the art.
    let roster = Roster::build(&defs, &base, &atlas, &assets, "", false).unwrap();
    let looks = Looks::Roster {
        roster: Box::new(roster),
        crowd: Population::new(None),
    };
    let mut sim = SimHandle::spawn(world(&defs), SimConfig::manual()).unwrap();
    sim.send_at(
        Tick(1),
        ME,
        Command::BuildStructure {
            kind: plant,
            cell: Cell::new(26, 20),
        },
    );
    sim.advance(1);
    let snap = sim.wait_for_tick(Tick(2), Duration::from_secs(5)).unwrap();
    let view = *snap
        .views()
        .iter()
        .find(|v| v.kind == plant)
        .expect("the plant is in the world");
    // Placed: nothing is built yet, and the picture is the first stage.
    let c = snap.construction_of(view.id).expect("under construction");
    assert_eq!((c.left, c.total), (300, 300));
    assert_eq!(snap.constructions().len(), 1);
    let finished = looks.shape_at(&view, None);
    let foundation = looks.shape_at(&view, Some(c.permille()));
    assert_ne!(foundation, finished, "a stage is not the finished look");
    assert_eq!(
        looks.shape_of(&view),
        finished,
        "shape_of is the finished look"
    );

    // Part way: the snapshot's progress picks the scaffolding past the data's threshold (400).
    sim.advance(150);
    let snap = sim
        .wait_for_tick(Tick(152), Duration::from_secs(5))
        .unwrap();
    let c = snap.construction_of(view.id).unwrap();
    assert_eq!((c.left, c.permille()), (150, 500));
    let scaffolding = looks.shape_at(&view, Some(c.permille()));
    assert_ne!(scaffolding, foundation);
    assert_ne!(scaffolding, finished);

    // One tick short of the end it is still the scaffolding; the tick after, the snapshot says it
    // is complete and the finished look is the one drawn.
    sim.advance(149);
    let snap = sim
        .wait_for_tick(Tick(301), Duration::from_secs(5))
        .unwrap();
    let c = snap.construction_of(view.id).unwrap();
    assert_eq!((c.left, c.permille()), (1, 996));
    assert_eq!(looks.shape_at(&view, Some(c.permille())), scaffolding);
    sim.advance(1);
    let snap = sim
        .wait_for_tick(Tick(302), Duration::from_secs(5))
        .unwrap();
    assert!(snap.construction_of(view.id).is_none());
    assert!(snap.constructions().is_empty());
    sim.stop();
}
