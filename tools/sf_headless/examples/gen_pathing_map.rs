//! The hand-drawn test map of the `pathing` scenario (WP4.2).
//!
//! `cargo run -p sf_headless --example gen_pathing_map -- <map_dir>` writes a
//! flat 96 x 64 cell map (384 x 256 m) made to show what the path finder does
//! (see `assets/scenarios/pathing.ron` and `docs/rules/pathfinding.md`):
//!
//! ```text
//!   x 0..43            x 44..49      x 50..95
//!   a serpentine maze  the river     the east bank: a lake with an island, and
//!   in the north-west  (water, with  a rock wall with two gaps in the south
//!   (rock and water    one bridge,
//!   walls), open       row 40)
//!   ground below
//! ```
//!
//! * the **maze**: four walls across a rock-bound room (x 2..37, y 2..26), rock,
//!   water, rock, water from the top, with a three-cell gap at alternating ends,
//!   and a one-cell door at (38, 25) out of the room. Walkers have to snake
//!   through it; a hover craft crosses the water walls;
//! * the **river** (x 44..49, every row) with one **bridge** in row 40;
//! * the **lake** (x 58..88, y 6..26) with an **island** in it (x 68..76, y 13..19)
//!   that nothing on foot or wheels can reach;
//! * the **wall** (x 70..71, y 27..63, from the lake to the edge of the map) with **gap 1** in rows 44..47 and **gap 2** in
//!   rows 56..59: a structure placed in front of the first makes the second the
//!   only way;
//! * a few rock blocks on the open ground.
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
        eprintln!("usage: gen_pathing_map <map_dir>");
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
    rect(44, 40, 49, 40, bridge);

    // The maze: a room with a door, and four walls with gaps at alternating ends.
    rect(1, 1, 38, 1, rock);
    rect(1, 27, 38, 27, rock);
    rect(1, 1, 1, 27, rock);
    rect(38, 1, 38, 27, rock);
    rect(38, 25, 38, 25, grass);
    rect(2, 7, 37, 7, rock);
    rect(34, 7, 36, 7, grass);
    rect(2, 12, 37, 12, water);
    rect(3, 12, 5, 12, grass);
    rect(2, 17, 37, 17, rock);
    rect(34, 17, 36, 17, grass);
    rect(2, 22, 37, 22, water);
    rect(3, 22, 5, 22, grass);

    // The lake and its island.
    rect(58, 6, 88, 26, water);
    rect(68, 13, 76, 19, grass);

    // The wall with two gaps.
    rect(70, 27, 71, H - 1, rock);
    rect(70, 44, 71, 47, grass);
    rect(70, 56, 71, 59, grass);

    // Rock blocks on the open ground, so that no run is a straight line.
    for (x0, y0, x1, y1) in [
        (14, 32, 17, 35),
        (22, 40, 23, 50),
        (10, 44, 13, 45),
        (28, 30, 31, 33),
        (30, 52, 33, 54),
        (56, 44, 58, 50),
        (62, 52, 64, 58),
        (80, 40, 84, 42),
        (60, 30, 62, 34),
    ] {
        rect(x0, y0, x1, y1, rock);
    }

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
