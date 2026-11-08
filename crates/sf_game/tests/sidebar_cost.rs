//! WP11.1: what the sidebar costs per frame on the CPU. The sidebar adds no work to the simulation (it
//! reads the snapshot); this measures the two things it does on the main thread each frame:
//!
//! * `Situation::read` + `sidebar::model`: one pass over the entities of the snapshot to find the
//!   standing structures, the factories and the structures going up, and the items of the tab;
//! * `sidebar_view::frame`: building the quads of the column on the UI (money, power bar, four tabs,
//!   a full tab of items).
//!
//! The timing test is `#[ignore]` (a timing in the CI container means nothing, `docs/progress.md`
//! says so for every frame number); run it with
//! `cargo test --release -p sf_game --test sidebar_cost -- --ignored --nocapture`.
//! The plain test checks what does not depend on the machine: the number of quads is bounded and
//! does not grow with the number of entities in the world.

use std::time::Instant;

use sf_data::money_look::MoneyLook;
use sf_data::sidebar_look::SidebarLook;
use sf_data::units::{Faction, UnitDefs};
use sf_game::playfield::{with_buildings, with_harvest, with_money, with_production, with_storage};
use sf_game::sidebar::{self, Catalog, Situation, Tab};
use sf_game::sidebar_view::{self, SidebarState};
use sf_game::sim_snapshot::SimSnapshot;
use sf_math::{Fx, Vec2};
use sf_sim::{Cell, Command, Kind, PlayerId, Tick, World, WorldConfig};
use sf_ui::{Ui, UiConfig, UiInput};

const ME: PlayerId = PlayerId(0);

/// A world with a base of Meridian (every structure of the sidebar's first tab standing) and
/// `crowd` riflemen spread over the map.
fn world(defs: &UnitDefs, crowd: u32) -> World {
    let world = World::new(WorldConfig::new(3).with_map_size(256, 256))
        .unwrap()
        .with_units(sf_bridge::unit_table(defs).unwrap())
        .unwrap();
    let world = with_storage(with_harvest(with_buildings(world, defs), defs), defs);
    let mut world = with_production(with_money(world), defs);
    let kind = |id: &str| Kind(defs.kind_of(id).unwrap());
    let base = [
        ("construction_center", (20, 20)),
        ("power_plant", (30, 20)),
        ("refinery", (30, 26)),
        ("barracks", (36, 20)),
        ("vehicle_factory", (36, 26)),
        ("radar", (44, 26)),
        ("defence_tower", (44, 20)),
        ("shipyard", (52, 26)),
    ];
    for (id, cell) in base {
        world
            .schedule(
                Tick(0),
                ME,
                Command::PlaceStructure {
                    kind: kind(id),
                    cell: Cell::new(cell.0, cell.1),
                    owner: ME,
                },
            )
            .unwrap();
    }
    for i in 0..crowd {
        world
            .schedule(
                Tick(0),
                ME,
                Command::SpawnUnit {
                    kind: kind("rifleman"),
                    owner: PlayerId((i % 2) as u8),
                    pos: Vec2::new(
                        Fx::from_int(80 + (i % 170) as i32) + Fx::HALF,
                        Fx::from_int(80 + ((i / 170) % 170) as i32) + Fx::HALF,
                    ),
                },
            )
            .unwrap();
    }
    world.step();
    world
}

struct Run {
    snap: SimSnapshot,
    catalog: Catalog,
    look: SidebarLook,
    money: MoneyLook,
}

fn run(crowd: u32) -> Run {
    let defs = UnitDefs::embedded();
    let look = SidebarLook::embedded();
    let catalog = Catalog::build(&defs, Faction::Meridian, &look);
    let snap = SimSnapshot::capture(&world(&defs, crowd));
    Run {
        snap,
        catalog,
        look,
        money: MoneyLook::embedded(),
    }
}

impl Run {
    fn model(&self) -> sidebar::SidebarModel {
        let sit = Situation::read(&self.snap, ME, &self.catalog, &[]);
        sidebar::model(&self.catalog, &sit, Tab::Structures, None, None, &self.look)
    }

    /// One frame on `ui` (the HUD keeps its `Ui` between frames, so the timing does too); the quads.
    fn frame(&self, ui: &mut Ui, model: &sidebar::SidebarModel) -> usize {
        let input = UiInput::default();
        ui.begin_frame(input, (1920, 1080), 1.0);
        let setup = sidebar_view::Setup {
            look: &self.look,
            money: &self.money,
            edge_band_px: 4.0,
            minimap: true,
        };
        sidebar_view::frame(ui, &input, &mut SidebarState::default(), model, &setup);
        ui.end_frame();
        ui.draw_list().quad_count()
    }
}

#[test]
fn the_number_of_quads_is_bounded_and_does_not_grow_with_the_world() {
    let small = run(0);
    let big = run(5000);
    assert_eq!(big.snap.len(), small.snap.len() + 5000);
    let (a, b) = (small.model(), big.model());
    assert_eq!(a, b, "the crowd changes nothing the sidebar shows");
    assert_eq!(a.items.len(), 9, "all nine structures can be built");
    let (qa, qb) = (
        small.frame(&mut Ui::new(UiConfig::default()), &a),
        big.frame(&mut Ui::new(UiConfig::default()), &b),
    );
    assert_eq!(qa, qb);
    // A panel, the money block with its text, the power bar frame with its text, four tabs with
    // their text and nine items with a picture, a label, a price and a bar: a few hundred quads.
    assert!(qa < 500, "{qa} quads");
    assert!(qa > 100, "{qa} quads: the text is in it");
}

#[test]
#[ignore = "a timing: run with --release -- --ignored --nocapture"]
fn the_cost_of_the_sidebar_per_frame() {
    const FRAMES: u32 = 4000;
    for crowd in [0u32, 5000] {
        let r = run(crowd);
        let entities = r.snap.len();
        let model = r.model();
        let mut ui = Ui::new(UiConfig::default());
        for round in 0..3 {
            let mut times = Vec::with_capacity(FRAMES as usize);
            for _ in 0..FRAMES {
                let t = Instant::now();
                std::hint::black_box(r.model());
                times.push(t.elapsed().as_nanos() as u64);
            }
            times.sort_unstable();
            let model_us = (
                times.iter().sum::<u64>() as f64 / f64::from(FRAMES) / 1000.0,
                times[times.len() * 99 / 100] as f64 / 1000.0,
            );
            let mut times = Vec::with_capacity(FRAMES as usize);
            for _ in 0..FRAMES {
                let t = Instant::now();
                std::hint::black_box(r.frame(&mut ui, &model));
                times.push(t.elapsed().as_nanos() as u64);
            }
            times.sort_unstable();
            let ui_us = (
                times.iter().sum::<u64>() as f64 / f64::from(FRAMES) / 1000.0,
                times[times.len() * 99 / 100] as f64 / 1000.0,
            );
            println!(
                "entities {entities:5}  round {round}: model mean {:.1} us p99 {:.1} us | \
                 ui build mean {:.1} us p99 {:.1} us ({} quads)",
                model_us.0,
                model_us.1,
                ui_us.0,
                ui_us.1,
                r.frame(&mut ui, &model)
            );
        }
    }
}
