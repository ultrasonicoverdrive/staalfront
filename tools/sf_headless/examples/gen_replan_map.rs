//! The test map of the `replan` scenario (WP4.7).
//!
//! `cargo run -p sf_headless --example gen_replan_map -- <map_dir>` writes a flat 96 x 64
//! cell map (384 x 256 m) of six rock-walled rooms of 32 x 32 cells, two sectors a side
//! (see `assets/scenarios/replan.ron` and `docs/rules/pathfinding.md`, "Replanning"):
//!
//! ```text
//!   R1 (0,0)            R2 (32,0)           R3 (64,0)
//!   two corridors, a    two corridors, a    one corridor, a unit
//!   structure is built  unit on hold stands on hold stands in it:
//!   in the upper one    in the upper one    no way round
//!
//!   R4 (0,32)           R5 (32,32)          R6 (64,32)
//!   a wall with a gap   open ground: the    open ground: a group
//!   below: a target     target of an        goes to a point that
//!   walks away round it attack dies         is built over
//! ```
//!
//! In R1 to R3 a left and a right chamber (columns 1 to 6 and 25 to 30 of the room, rows 1
//! to 30) are joined by a corridor one cell wide along row 8 (columns 7 to 24) and, in R1
//! and R2, a second one along row 24. Heights are zero and the materials follow the
//! terrain types, so the files are small and the same on every machine.

use std::path::PathBuf;

use sf_data::map::Map;
use sf_data::terrain::{TerrainLayer, TerrainTypes};

const W: u32 = 96;
const H: u32 = 64;
const ROOM: u32 = 32;

fn main() {
    let Some(dir) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: gen_replan_map <map_dir>");
        std::process::exit(2);
    };
    let types = TerrainTypes::embedded();
    let id = |name: &str| types.id_of(name).expect("a shipped terrain type");
    let (grass, rock) = (id("grass"), id("rock"));
    // Everything is rock until a room is cut out of it.
    let mut ids = vec![rock; (W * H) as usize];
    let mut rect = |x0: u32, y0: u32, x1: u32, y1: u32, terrain: u8| {
        for y in y0..=y1 {
            for x in x0..=x1 {
                ids[(y * W + x) as usize] = terrain;
            }
        }
    };

    // R1, R2, R3: two chambers and the corridors between them.
    for (room, lower) in [(0, true), (1, true), (2, false)] {
        let ox = room * ROOM;
        rect(ox + 1, 1, ox + 6, 30, grass);
        rect(ox + 25, 1, ox + 30, 30, grass);
        rect(ox + 7, 8, ox + 24, 8, grass);
        if lower {
            rect(ox + 7, 24, ox + 24, 24, grass);
        }
    }
    // R4: an open room with a wall across it and a gap at the bottom.
    rect(1, ROOM + 1, 30, ROOM + 30, grass);
    rect(15, ROOM + 1, 15, ROOM + 20, rock);
    // R5, R6: open rooms.
    rect(ROOM + 1, ROOM + 1, ROOM + 30, ROOM + 30, grass);
    rect(2 * ROOM + 1, ROOM + 1, 2 * ROOM + 30, ROOM + 30, grass);

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
