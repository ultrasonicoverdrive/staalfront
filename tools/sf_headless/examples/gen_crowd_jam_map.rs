//! The test map of the `crowd_jam` scenario (WP4.4).
//!
//! `cargo run -p sf_headless --example gen_crowd_jam_map -- <map_dir>` writes a flat
//! 64 x 40 cell map (256 x 160 m) of two rooms joined by a gap, and a corridor in
//! the east room (see `assets/scenarios/crowd_jam.ron` and
//! `docs/rules/collision.md`):
//!
//! ```text
//!   x 0..26        x 27..34            x 35..63
//!   the west room  a wall of rock,     the east room, with a corridor
//!   (open ground)  every row, with a   (x 40..58, rows 30..34, five cells
//!                  gap in rows 18..20  wide) between two walls of rock
//!                  (three cells wide,
//!                  eight long)
//! ```
//!
//! Heights are zero and the materials follow the terrain types, so the files
//! are small and the same on every machine.

use std::path::PathBuf;

use sf_data::map::Map;
use sf_data::terrain::{TerrainLayer, TerrainTypes};

const W: u32 = 64;
const H: u32 = 40;

fn main() {
    let Some(dir) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: gen_crowd_jam_map <map_dir>");
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

    // The wall between the rooms and its gap.
    rect(27, 0, 34, H - 1, rock);
    rect(27, 18, 34, 20, grass);

    // The corridor in the east room: two walls with five rows between them.
    rect(40, 27, 58, 29, rock);
    rect(40, 35, 58, 37, rock);

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
