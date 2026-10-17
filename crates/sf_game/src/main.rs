//! The development viewer: the whole shell is in the `sf_game` library (see
//! its docs); this binary only picks the dev defaults.

fn main() {
    std::process::exit(sf_game::run(sf_game::Preset::Game));
}
