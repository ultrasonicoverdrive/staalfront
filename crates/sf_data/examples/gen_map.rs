//! Write a procedural map to a directory.
//!
//! `cargo run -p sf_data --example gen_map -- <out_dir> [width] [height] [seed]`
//! (defaults 64 x 64, seed 1). The committed test map `assets/maps/test64` was
//! made with this command by the earlier float version of the generator;
//! the generator is bit-exact now and gives a slightly different map, which is
//! why the committed files are not regenerated (the terrain goldens use them).

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next() else {
        eprintln!("usage: gen_map <out_dir> [width] [height] [seed]");
        std::process::exit(2);
    };
    let mut num = |default: u32| args.next().map_or(default, |a| a.parse().expect("number"));
    let (width, height, seed) = (num(64), num(64), num(1));
    let map = sf_data::map::generate::generate(width, height, seed);
    let dir = PathBuf::from(dir);
    map.save(&dir).expect("write map");
    println!(
        "wrote {width}x{height} map (seed {seed}) to {}",
        dir.display()
    );
}
