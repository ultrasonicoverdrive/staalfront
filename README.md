# Staalfront

Staalfront is a real-time strategy game in Rust, built on its own thin engine
(wgpu + winit) with a fully deterministic, headless-testable simulation.
The simulation (`sf_sim`) never touches the GPU, floats, the clock or I/O.

The full plan, architecture and quality rules are in [PLAN.md](PLAN.md).
Project status: phase 0 (workspace and CI); there is no gameplay yet.

## Build and test

The toolchain is pinned in `rust-toolchain.toml` (rustup installs it on first use).

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/check-sim-determinism.sh   # rejects f32/f64/HashMap/HashSet in sf_sim
```

## Layout

`crates/` holds the libraries (`sf_math`, `sf_sim`, `sf_data`, `sf_ai`,
`sf_engine`, `sf_render`, `sf_ui`, `sf_audio`, `sf_cutscene`, `sf_net`) and the
game binary `sf_game`; `tools/` holds `sf_editor`, `sf_headless` and `sf_demo`.
See PLAN.md section 1.1.
