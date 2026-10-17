//! Give an existing map a terrain layer (WP2.1) without touching its other files.
//!
//! `cargo run -p sf_data --example gen_terrain -- <map_dir> [seed]` (default
//! seed 1) reads the map in `map_dir`, derives its terrain layer with
//! `sf_data::map::generate::terrain_for` (water where the riverbed material is,
//! rock where the rock material is, ore fields, a road and its bridge), then
//! writes `terrain.png` and sets `terrain_map` in `map.ron`. `height.png` and
//! `materials.png` stay byte for byte as they were, which is how the committed
//! `assets/maps/test64` (made by the earlier float generator) got its layer.

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next() else {
        eprintln!("usage: gen_terrain <map_dir> [seed]");
        std::process::exit(2);
    };
    let seed = args.next().map_or(1, |a| a.parse().expect("seed"));
    let dir = PathBuf::from(dir);
    let map = sf_data::map::Map::load(&dir).expect("read the map");
    let layer = sf_data::map::generate::terrain_for(&map, seed);
    let map = map
        .with_terrain(layer, &sf_data::terrain::TerrainTypes::embedded())
        .expect("a valid layer");
    map.save_terrain_layer(&dir).expect("write the layer");
    println!(
        "wrote terrain.png ({}x{} cells, seed {seed}) to {}",
        map.size_in_cells().0,
        map.size_in_cells().1,
        dir.display()
    );
}
