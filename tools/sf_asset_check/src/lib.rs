//! Checks delivered `.glb` models against the art brief (PLAN.md section 10,
//! `docs/conventions.md` section 12).
//!
//! ```text
//! cargo run -p sf_asset_check -- assets/set_v3
//! cargo run -p sf_asset_check -- --format markdown assets/set_v3/veyran_scout.glb
//! ```
//!
//! # What is checked, per file
//!
//! | rule id | what | severity |
//! |---|---|---|
//! | `file-name`, `class`, `faction` | `<faction>_<role>.glb`; the role gives the budget class (`unit_defs.ron` category, else `wreck_*`, `tree_*`, `rock_*`) | fail, fail, warn |
//! | `glb`, `engine-load` | a valid glTF 2.0 binary that `sf_render::glb::load_glb` accepts with front +X, up +Y, 1 m | fail |
//! | `lod-roots` | roots `<id>_LOD0`, `_LOD1`, `_LOD2` named after the file | fail (extra LODs warn) |
//! | `lod-structure`, `lod-origin` | the same node tree and the same root transform in every LOD | fail |
//! | `lod-size` | LOD1/LOD2 bounds against LOD0: more than 10 % of the longest side apart | warn |
//! | `budget`, `lod-triangles` | triangles per LOD within the class budget; fewer per level | fail, warn |
//! | `materials`, `textures` | at most 6 materials; base colour only (no textures; emissive warns) | fail |
//! | `team`, `team-share` | `team` present for faction models, absent for neutral ones; 20 to 40 % of the top-down view of LOD0 (LOD1/2: warn) | fail |
//! | `ground` | lowest point `Y = 0` within 5 cm, warn to 30 cm, then fail; a vessel: the waterline is inside the hull | warn / fail |
//! | `orientation` | vehicles and wrecks longer along X than Z; infantry higher than wide | fail |
//! | `nodes`, `fx-nodes` | infantry `torso head arm_l arm_r leg_l leg_r` (`_2` for two figures); vehicles and wrecks `hull` -> `turret` -> `barrel`; wrecks empty `fx_smoke_*` and `fx_fire_*` | fail |
//! | `origin`, `footprint` | the origin under the middle of the footprint; an ore field fits one 4 m cell | warn |
//!
//! The geometry (triangles, bounds, team flag, top-down view) comes from the
//! engine's own loader, so the check sees what the game draws. The top-down
//! share rasterises the LOD0 triangles on the ground plane and takes the
//! topmost triangle per pixel ([`raster`]).
//!
//! Budgets (LOD0/LOD1/LOD2): infantry 1500/250/60 (raised from 800 by the
//! orchestrator, see `assets/set_v3/README.md`), two-figure units twice that,
//! vehicle, decor and wreck 1500/500/150, building 5000/1500/300.

pub mod check;
pub mod class;
pub mod cli;
pub mod doc;
pub mod geom;
pub mod raster;
pub mod report;

pub use check::{Checker, FileReport, Finding, Severity, Status};
