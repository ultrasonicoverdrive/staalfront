//! Two variants of the first match's map for the AI's base tests (WP9.1b-1, `docs/rules/ai.md`).
//!
//! `cargo run -p sf_headless --example gen_ai_maps -- assets/maps` writes, next to `m1`:
//!
//! ```text
//!   m1_noore   the map of gen_match_map with every ore cell turned to grass: no income at all
//!   m1_boxed   the same map with the ground round the south-west construction centre (x 12..15,
//!              y 68..71) turned to rock in x 4..27, y 56..83, except a pocket x 5..17, y 66..73 that
//!              holds the centre: 2 x 3 structures fit west of it, a refinery (3 x 4 and its apron)
//!              fits nowhere
//! ```
//!
//! Integer arithmetic only, like gen_match_map, so the files are the same on every machine.

use std::path::PathBuf;

use sf_data::map::Map;
use sf_data::terrain::{TerrainLayer, TerrainTypes};

#[allow(dead_code)]
#[path = "gen_match_map.rs"]
mod gen_match_map;

const W: u32 = 96;
const H: u32 = 96;

/// The rock box of `m1_boxed` and the pocket left open in it, `(x0, y0, x1, y1)` inclusive.
pub const BOX: (u32, u32, u32, u32) = (4, 56, 27, 83);
/// The open pocket round the construction centre.
pub const POCKET: (u32, u32, u32, u32) = (5, 66, 17, 73);

fn with_ids(ids: Vec<u8>) -> Map {
    let types = TerrainTypes::embedded();
    let (mw, mh) = (W * 4, H * 4);
    let layer = TerrainLayer::new(W, H, ids).expect("W x H ids");
    Map::from_parts(
        mw,
        mh,
        0.0,
        vec!["grass".to_string()],
        vec![0; ((mw + 1) * (mh + 1)) as usize],
        vec![0; (mw * mh) as usize],
    )
    .expect("a valid flat map")
    .with_terrain(layer, &types)
    .expect("a valid layer")
}

fn m1_ids() -> Vec<u8> {
    gen_match_map::draw()
        .terrain()
        .expect("m1 has a terrain layer")
        .ids()
        .to_vec()
}

/// `m1` without ore.
pub fn draw_noore() -> Map {
    let types = TerrainTypes::embedded();
    let (grass, ore) = (
        types.id_of("grass").expect("grass"),
        types.id_of("ore").expect("ore"),
    );
    let ids = m1_ids()
        .into_iter()
        .map(|t| if t == ore { grass } else { t })
        .collect();
    with_ids(ids)
}

/// `m1` with the south-west base boxed in by rock.
pub fn draw_boxed() -> Map {
    let rock = TerrainTypes::embedded().id_of("rock").expect("rock");
    let mut ids = m1_ids();
    let inside = |(x0, y0, x1, y1): (u32, u32, u32, u32), x: u32, y: u32| {
        (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
    };
    for y in 0..H {
        for x in 0..W {
            if inside(BOX, x, y) && !inside(POCKET, x, y) {
                ids[(y * W + x) as usize] = rock;
            }
        }
    }
    with_ids(ids)
}

fn main() {
    let Some(dir) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: gen_ai_maps <maps_dir>");
        std::process::exit(2);
    };
    for (name, map) in [("m1_noore", draw_noore()), ("m1_boxed", draw_boxed())] {
        map.save(&dir.join(name)).expect("write the map");
        println!("wrote {}", dir.join(name).display());
    }
}
