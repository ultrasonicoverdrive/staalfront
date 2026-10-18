//! The test map of the `bridge_200` scenario (WP4.3).
//!
//! `cargo run -p sf_headless --example gen_bridge_map -- <map_dir>` writes a flat
//! 96 x 64 cell map (384 x 256 m): a river across the whole map with one bridge,
//! and a wall on the far bank with a gap that is not in line with the bridge, so
//! that 200 units sent to the far side of it have to cross the bridge and then
//! bend (see `assets/scenarios/bridge_200.ron` and `docs/rules/pathfinding.md`):
//!
//! ```text
//!   x 0..43          x 44..49       x 50..95
//!   the west bank:   the river      the east bank: rock outcrops either side
//!   open ground and  (water, every  of the bridge's mouth, then a wall (x 66..67,
//!   rock outcrops    row) with ONE  every row) with a gap in rows 36..40, and the
//!   funnelling to    bridge in      open ground behind it where the goal is
//!   the bridge       rows 31..33
//! ```
//!
//! Heights are zero and the materials follow the terrain types, so the files
//! are small and the same on every machine.

use std::path::PathBuf;

use sf_data::map::Map;
use sf_data::terrain::{TerrainLayer, TerrainTypes};

const W: u32 = 96;
const H: u32 = 64;

fn main() {
    let Some(dir) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: gen_bridge_map <map_dir>");
        std::process::exit(2);
    };
    let types = TerrainTypes::embedded();
    let id = |name: &str| types.id_of(name).expect("a shipped terrain type");
    let (grass, water, rock, bridge) = (id("grass"), id("water"), id("rock"), id("bridge"));
    let mut ids = vec![grass; (W * H) as usize];
    // Fills the inclusive rectangle (x0, y0) - (x1, y1).
    let mut rect = |x0: u32, y0: u32, x1: u32, y1: u32, terrain: u8| {
        for y in y0..=y1 {
            for x in x0..=x1 {
                ids[(y * W + x) as usize] = terrain;
            }
        }
    };

    // The river and its one bridge.
    rect(44, 0, 49, H - 1, water);
    rect(44, 31, 49, 33, bridge);

    // Rock outcrops funnelling toward the bridge on the west bank, and flanking
    // its mouth on the east bank.
    for (x0, y0, x1, y1) in [
        (32, 18, 36, 27),
        (32, 37, 36, 46),
        (38, 6, 41, 12),
        (38, 52, 41, 58),
        (52, 20, 56, 28),
        (52, 36, 56, 44),
    ] {
        rect(x0, y0, x1, y1, rock);
    }

    // The wall on the east bank with its gap, out of line with the bridge.
    rect(66, 0, 67, H - 1, rock);
    rect(66, 36, 67, 40, grass);

    // One material sample per metre; four per cell side.
    let (mw, mh) = (W * 4, H * 4);
    let material_of = |terrain: u8| match terrain {
        t if t == grass => 0,
        t if t == rock => 2,
        t if t == water => 3,
        t if t == bridge => 1,
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
