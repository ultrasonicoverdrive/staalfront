//! The hand-drawn test map of the `terrain_costs` scenario (WP4.1).
//!
//! `cargo run -p sf_data --example gen_costs_map -- <map_dir>` writes a flat
//! 40 x 20 cell map (160 x 80 m) whose terrain layer is a set of straight lanes,
//! one terrain per lane, so that units crossing each of them show what a
//! movement class does on it (see `assets/scenarios/terrain_costs.ron`):
//!
//! ```text
//! row  0..19   x 0..19      x 20..23   x 24..39
//! all          grass        water      grass        (a river, four cells wide)
//! 4            road         water      grass        (a road lane)
//! 6            ore          water      grass        (an ore lane)
//! 8            grass        BRIDGE     grass        (the river bridged)
//! 12, 16       grass        water      grass, ROCK in x 30..31
//! ```
//!
//! Heights are zero and the materials follow the terrain types, so the files
//! are small and the same on every machine.

use std::path::PathBuf;

use sf_data::map::Map;
use sf_data::terrain::{TerrainLayer, TerrainTypes};

const W: u32 = 40;
const H: u32 = 20;

fn main() {
    let Some(dir) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: gen_costs_map <map_dir>");
        std::process::exit(2);
    };
    let types = TerrainTypes::embedded();
    let id = |name: &str| types.id_of(name).expect("a shipped terrain type");
    let (grass, road, water, rock, ore, bridge) = (
        id("grass"),
        id("road"),
        id("water"),
        id("rock"),
        id("ore"),
        id("bridge"),
    );
    let mut ids = vec![grass; (W * H) as usize];
    let mut set = |x: u32, y: u32, terrain: u8| ids[(y * W + x) as usize] = terrain;
    for y in 0..H {
        for x in 20..=23 {
            set(x, y, water);
        }
    }
    for x in 0..=19 {
        set(x, 4, road);
        set(x, 6, ore);
    }
    for x in 20..=23 {
        set(x, 8, bridge);
    }
    for y in [12, 16] {
        for x in 30..=31 {
            set(x, y, rock);
        }
    }

    // One material sample per metre; four per cell side.
    let (mw, mh) = (W * 4, H * 4);
    let material_of = |terrain: u8| match terrain {
        t if t == grass => 0,
        t if t == road || t == bridge => 1,
        t if t == rock => 2,
        t if t == water => 3,
        _ => 4,
    };
    let cells: Vec<u8> = (0..mh)
        .flat_map(|y| (0..mw).map(move |x| (x, y)))
        .map(|(x, y)| material_of(ids[((y / 4) * W + x / 4) as usize]))
        .collect();
    let layer = TerrainLayer::new(W, H, ids).expect("W x H ids");
    let map = Map::from_parts(
        mw,
        mh,
        8.0,
        ["grass", "dirt", "rock", "sand", "dark_ground"]
            .map(String::from)
            .to_vec(),
        vec![0; ((mw + 1) * (mh + 1)) as usize],
        cells,
    )
    .expect("a valid flat map")
    .with_terrain(layer, &types)
    .expect("a valid layer");
    map.save(&dir).expect("write the map");
    println!("wrote the {W}x{H} cell map to {}", dir.display());
}
