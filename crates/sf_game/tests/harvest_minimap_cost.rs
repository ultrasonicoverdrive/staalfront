//! What harvesting costs the picture of the ore (WP7.2), the gap WP7.1 left open: the ore the
//! harvesters take changes the fill level of cells, and every change the picture shows is an upload
//! of the minimap (`UiRenderer::set_minimap` makes a new texture and bind group each time).
//!
//! `harvesting_makes_few_uploads` counts, for a world with `N` harvesters over a few minutes of
//! game time, in how many ticks some cell crossed a level (each such tick is one upload in the
//! game) and how many cells changed. `measure_minimap_upload` (ignored; run it with `--nocapture`)
//! times one upload on the software adapter and prints the totals; the numbers are in
//! `docs/rules/economy.md` and `docs/progress.md`.

use std::path::PathBuf;
use std::time::Instant;

use sf_data::map::Map;
use sf_data::units::UnitDefs;
use sf_game::ore_view::OreView;
use sf_game::playfield::{make_world, with_buildings, with_harvest, with_movement};
use sf_game::sim_snapshot::SimSnapshot;
use sf_render::terrain_look::TerrainLooks;
use sf_render::{GpuOptions, Renderer};
use sf_sim::{Cell, Command, Kind, PlayerId, Tick, World, WorldConfig};
use sf_ui::{Minimap, UiRenderer};

const P0: PlayerId = PlayerId(0);

fn map() -> Map {
    Map::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps/playfield256"))
        .expect("the playfield map")
}

/// The playfield with a refinery at (96, 86) and `n` harvesters of player 0 round it.
fn world_with_harvesters(map: &Map, n: u32) -> World {
    let defs = UnitDefs::embedded();
    let cells = map.size_in_cells();
    let world = make_world(WorldConfig::new(1).with_map_size(cells.0, cells.1), map)
        .unwrap()
        .with_units(sf_bridge::unit_table(&defs).unwrap())
        .unwrap();
    let mut world = with_harvest(with_buildings(with_movement(world, &defs), &defs), &defs);
    let kind = |name: &str| Kind(defs.kind_of(name).unwrap());
    world
        .schedule(
            Tick(0),
            P0,
            Command::PlaceStructure {
                kind: kind("refinery"),
                cell: Cell::new(96, 86),
                owner: P0,
            },
        )
        .unwrap();
    for k in 0..n {
        // A lattice of spawn places south-east of the refinery on open grass.
        let (x, y) = (100 + (k % 10) * 2, 90 + (k / 10) * 2);
        world
            .schedule(
                Tick(0),
                P0,
                Command::SpawnUnit {
                    kind: kind("harvester"),
                    pos: sf_math::Vec2::new(
                        sf_math::Fx::from_int(x as i32) + sf_math::Fx::HALF,
                        sf_math::Fx::from_int(y as i32) + sf_math::Fx::HALF,
                    ),
                    owner: P0,
                },
            )
            .unwrap();
    }
    world
}

/// What the picture would do over `ticks` ticks: `(ticks with some level change, cells changed,
/// ore units taken off the map, the most ticks with a change in any one second (20 ticks))`.
fn count_changes(n: u32, ticks: u64) -> (u64, u64, u64, u64) {
    let map = map();
    let mut world = world_with_harvesters(&map, n);
    let mut view = OreView::new(&TerrainLooks::embedded());
    // The first snapshot draws every cell; it is not harvesting.
    world.step();
    let _ = view.changes(&SimSnapshot::capture(&world));
    let (mut upload_ticks, mut cells) = (0, 0);
    let mut seen = Vec::new();
    let mut spent = std::time::Duration::ZERO;
    for _ in 0..ticks {
        world.step();
        // The level comparison the presentation does with every new snapshot. The snapshot itself
        // is made outside the timer: here `capture` builds the whole of its shared tables each time,
        // which the game's sim thread does once.
        let snap = SimSnapshot::capture(&world);
        let started = Instant::now();
        let changes = view.changes(&snap);
        spent += started.elapsed();
        seen.push(!changes.is_empty());
        if !changes.is_empty() {
            upload_ticks += 1;
            cells += changes.len() as u64;
        }
    }
    let peak = seen
        .windows(20)
        .map(|w| w.iter().filter(|&&c| c).count() as u64)
        .max()
        .unwrap_or(0);
    // What the harvesters took off the map: what the refinery has been given and what they still
    // carry (the field also grows back, so the total of the cells says nothing).
    let carried: u64 = world
        .snapshot()
        .iter()
        .filter_map(|v| world.harvester_of(v.id))
        .map(|h| u64::from(h.load))
        .sum();
    let taken = world.ore_delivered(P0) + carried;
    println!(
        "    OreView::changes: {:.2} microseconds per tick ({n} harvesters)",
        spent.as_secs_f64() * 1e6 / ticks as f64
    );
    (upload_ticks, cells, taken, peak)
}

#[test]
fn harvesting_makes_few_uploads() {
    // Four harvesters for three minutes of game time (3,600 ticks).
    let (uploads, cells, taken, _) = count_changes(4, 3600);
    assert!(taken > 0, "the harvesters took ore");
    assert!(cells > 0 && uploads > 0, "some cell crossed a level");
    // A cell is 120 units and crosses a level (a third of the maximum, about 53 units) a few times
    // as it is emptied; a tick with a change is far from every tick.
    assert!(
        uploads * 10 < 3600,
        "{uploads} uploads in 3600 ticks, {cells} cells"
    );
}

/// Prints the numbers for the documents: the uploads and changed cells for 4 and 50 harvesters,
/// and the time of one upload on the software adapter.
#[test]
#[ignore]
fn measure_minimap_upload() {
    let map = map();
    for (n, ticks) in [(4, 3600u64), (50, 3600)] {
        let (uploads, cells, taken, peak) = count_changes(n, ticks);
        println!(
            "{n:3} harvesters, {ticks} ticks ({} s of game time): {uploads} ticks with a level change \
             (one minimap upload each, at most {peak} in one second), {cells} cells changed, {taken} \
             ore units taken",
            ticks / 20
        );
    }
    let r = Renderer::headless(960, 540, &GpuOptions::software())
        .unwrap_or_else(|e| panic!("NO GPU ADAPTER AVAILABLE: {e}"));
    println!("adapter: {}", r.adapter_summary());
    let minimap = Minimap::bake(&map, 256);
    let mut ur = UiRenderer::new(&r.gpu().device, r.color_format());
    // Warm up, then time a few hundred uploads.
    for _ in 0..20 {
        ur.set_minimap(&r.gpu().device, &r.gpu().queue, minimap.image());
    }
    let n = 400;
    let started = Instant::now();
    for _ in 0..n {
        ur.set_minimap(&r.gpu().device, &r.gpu().queue, minimap.image());
    }
    // Make the queue finish what it was given.
    let _ = r.gpu().device.poll(sf_render::wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    let per = started.elapsed().as_secs_f64() * 1e6 / f64::from(n);
    println!("one set_minimap (256 x 256 image): {per:.1} microseconds");
}
