//! Write a procedural map to a directory.
//!
//! `cargo run -p sf_data --example gen_map -- <out_dir> [width] [height] [seed] [flat]`
//! (defaults 64 x 64 m, seed 1; the size is in metres, so 1024 x 1024 is the
//! 256 x 256 cell playfield). The committed test map `assets/maps/test64` was
//! made with this command by the earlier float version of the generator;
//! the generator is bit-exact now and gives a slightly different map, which is
//! why the committed files are not regenerated (the terrain goldens use them).
//! Its terrain layer was added afterwards with the `gen_terrain` example.
//!
//! With the word `flat` the written map keeps the generated terrain layer but
//! has no relief (height scale 0) and one material: that is the playfield the
//! headless scenario `terrain_256` loads (`assets/maps/playfield256`). The
//! simulation reads only the terrain layer, and a flat 1,025 x 1,025 height
//! image is a few kilobytes where the generated relief is 1.7 MB.

use std::path::PathBuf;

use sf_data::map::Map;
use sf_data::terrain::TerrainTypes;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next() else {
        eprintln!("usage: gen_map <out_dir> [width] [height] [seed] [flat]");
        std::process::exit(2);
    };
    let mut num = |default: u32| args.next().map_or(default, |a| a.parse().expect("number"));
    let (width, height, seed) = (num(64), num(64), num(1));
    let flat = args.next().is_some_and(|a| a == "flat");
    let mut map = sf_data::map::generate::generate(width, height, seed);
    if flat {
        let layer = map.terrain().expect("generated maps have terrain").clone();
        map = Map::from_parts(
            width,
            height,
            0.0,
            vec!["grass".to_string()],
            vec![0; (width as usize + 1) * (height as usize + 1)],
            vec![0; width as usize * height as usize],
        )
        .expect("a flat map is valid")
        .with_terrain(layer, &TerrainTypes::embedded())
        .expect("the generated layer fits");
    }
    let dir = PathBuf::from(dir);
    map.save(&dir).expect("write map");
    println!(
        "wrote {width}x{height} map (seed {seed}{}) to {}",
        if flat { ", flat" } else { "" },
        dir.display()
    );
}
