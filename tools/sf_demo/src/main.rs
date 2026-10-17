//! Engine demo and benchmark (phase 1, WP1.14, milestone M0).
//!
//! The same shell as `sf_game` (the library does everything, see its docs)
//! with the demo's defaults: a 256 x 256 cell map (1,024 m), 5,000 wandering
//! units in three placeholder shapes and four colours plus the glTF models of
//! the test pack, its buildings standing in a base at the centre, minimap, F1
//! panels, a sound on clicks. `--bench` flies the fixed camera path and
//! writes `bench.json`.

fn main() {
    std::process::exit(sf_game::run(sf_game::Preset::Demo));
}
