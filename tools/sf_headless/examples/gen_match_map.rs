//! The hand-drawn map of the first match (WP7.6, `assets/matches/m1.ron`).
//!
//! `cargo run -p sf_headless --example gen_match_map -- assets/maps/m1` writes a flat 96 x 96 cell map
//! (384 x 384 m, one material, no relief) whose layout is written out in this file, so the committed
//! files can always be made again. The map is the data of the match, so the match file and its tests
//! name cells of it:
//!
//! ```text
//!   north-east  the enemy base: open ground x 62..93, y 6..40, with two ore fields
//!               (a disc of radius 3 round (64, 28) and one round (88, 10))
//!   south-west  the player's base: open ground x 4..33, y 56..90, with two ore fields
//!               (round (27, 66) and round (10, 86))
//!   middle      a lake (a disc of radius 9 round (46, 44)) that nothing on foot crosses; two ways
//!               round it, north-west of it between the lake and a rock ridge (x 34..37, y 20..36), and
//!               south-east of it between the lake and a second ridge (x 56..59, y 54..68); a neutral
//!               ore field on each way (round (44, 14) and round (50, 78))
//!   rock        a few blocks in the open ground, so that no march is a straight line
//! ```
//!
//! Everything is integer arithmetic, so the files are the same on every machine.

use std::path::PathBuf;

use sf_data::map::Map;
use sf_data::terrain::{TerrainLayer, TerrainTypes};

const W: u32 = 96;
const H: u32 = 96;

/// The two bases keep their ground free of rock and water: (x0, y0, x1, y1), inclusive.
const BASES: [(u32, u32, u32, u32); 2] = [(4, 56, 33, 90), (62, 6, 93, 40)];

fn main() {
    let Some(dir) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: gen_match_map <map_dir>");
        std::process::exit(2);
    };
    let types = TerrainTypes::embedded();
    let id = |name: &str| types.id_of(name).expect("a shipped terrain type");
    let (grass, water, rock, ore) = (id("grass"), id("water"), id("rock"), id("ore"));
    let mut ids = vec![grass; (W * H) as usize];
    let in_base = |x: u32, y: u32| {
        BASES
            .iter()
            .any(|&(x0, y0, x1, y1)| (x0..=x1).contains(&x) && (y0..=y1).contains(&y))
    };

    // A disc of `radius` cells round (cx, cy); `keep_bases` leaves the bases alone.
    let mut disc = |cx: i32, cy: i32, radius: i32, terrain: u8, keep_bases: bool| {
        for y in 0..H as i32 {
            for x in 0..W as i32 {
                let (dx, dy) = (x - cx, y - cy);
                if dx * dx + dy * dy <= radius * radius && !(keep_bases && in_base(x as u32, y as u32))
                {
                    ids[(y as u32 * W + x as u32) as usize] = terrain;
                }
            }
        }
    };
    // The lake, and its soft shore: a second, smaller disc is not needed, the edge is the disc.
    disc(46, 44, 9, water, true);
    // The ore: two fields at each base and one on each way round the lake.
    for (cx, cy) in [
        (27, 66),
        (10, 86),
        (64, 28),
        (88, 10),
        (44, 14),
        (50, 78),
    ] {
        disc(cx, cy, 3, ore, false);
    }
    let mut rect = |x0: u32, y0: u32, x1: u32, y1: u32, terrain: u8| {
        for y in y0..=y1 {
            for x in x0..=x1 {
                if !in_base(x, y) {
                    ids[(y * W + x) as usize] = terrain;
                }
            }
        }
    };
    // The two ridges that narrow the ways round the lake, and a few blocks of rock.
    rect(34, 20, 37, 36, rock);
    rect(56, 54, 59, 68, rock);
    for (x0, y0, x1, y1) in [
        (20, 40, 23, 43),
        (14, 50, 16, 53),
        (66, 44, 69, 47),
        (74, 52, 77, 55),
        (48, 24, 51, 27),
        (42, 62, 45, 65),
        (30, 8, 33, 11),
        (84, 70, 87, 73),
    ] {
        rect(x0, y0, x1, y1, rock);
    }

    // One material sample per metre, four per cell side: all grass.
    let (mw, mh) = (W * 4, H * 4);
    let layer = TerrainLayer::new(W, H, ids).expect("W x H ids");
    let map = Map::from_parts(
        mw,
        mh,
        0.0,
        vec!["grass".to_string()],
        vec![0; ((mw + 1) * (mh + 1)) as usize],
        vec![0; (mw * mh) as usize],
    )
    .expect("a valid flat map")
    .with_terrain(layer, &types)
    .expect("a valid layer");
    map.save(&dir).expect("write the map");
    println!("wrote the {W}x{H} cell map to {}", dir.display());
}
