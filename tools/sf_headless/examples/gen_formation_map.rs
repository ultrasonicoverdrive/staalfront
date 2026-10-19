//! The test map of the `formation_move` scenario (WP4.6).
//!
//! `cargo run -p sf_headless --example gen_formation_map -- <map_dir>` writes a flat
//! 112 x 48 cell map (448 x 192 m): open ground, a wall of rock with one narrow
//! passage in it, and open ground behind it (see `assets/scenarios/formation_move.ron`
//! and `docs/rules/formations.md`):
//!
//! ```text
//!   x 0..39          x 40..47               x 48..111
//!   the west field   a wall of rock, every  the east field
//!   (open ground)    row, with a passage    (open ground)
//!                    in rows 22..24 (three
//!                    cells wide, eight long)
//! ```
//!
//! Heights are zero and the materials follow the terrain types, so the files
//! are small and the same on every machine.

use std::path::PathBuf;

use sf_data::map::Map;
use sf_data::terrain::{TerrainLayer, TerrainTypes};

const W: u32 = 112;
const H: u32 = 48;

fn main() {
    let Some(dir) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: gen_formation_map <map_dir>");
        std::process::exit(2);
    };
    let types = TerrainTypes::embedded();
    let id = |name: &str| types.id_of(name).expect("a shipped terrain type");
    let (grass, rock) = (id("grass"), id("rock"));
    let mut ids = vec![grass; (W * H) as usize];
    // Fills the inclusive rectangle (x0, y0) - (x1, y1).
    let mut rect = |x0: u32, y0: u32, x1: u32, y1: u32, terrain: u8| {
        for y in y0..=y1 {
            for x in x0..=x1 {
                ids[(y * W + x) as usize] = terrain;
            }
        }
    };

    // The wall and its passage.
    rect(40, 0, 47, H - 1, rock);
    rect(40, 22, 47, 24, grass);

    // One material sample per metre; four per cell side.
    let (mw, mh) = (W * 4, H * 4);
    let material_of = |terrain: u8| if terrain == rock { 2 } else { 0 };
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
