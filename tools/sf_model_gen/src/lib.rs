//! `sf_model_gen` (WP11.6a): our own model generator. Geometry is authored in integer millimetres
//! with a fixed sine table, so the bytes are identical on every platform; the outputs are committed
//! in `assets/set_v5` and a test checks that a fresh run writes the same bytes.
//!
//! - [`kit`]: solids (bevelled boxes, lofts, extrusions, lathes), details (plates, panel lines,
//!   bolts, hatches), the node tree and the level-of-detail knob [`kit::Lod`].
//! - [`shade`]: baked per-vertex shading (ambient occlusion, grime toward the ground, facet
//!   variation), written as `COLOR_0`, because the renderer has vertex colour only.
//! - [`glb`]: the GLB writer (one scene, roots `<stem>_LOD0..2`, one primitive per material).
//! - [`models`]: the model recipes of set v5.
//!
//! Floating point appears only in basic operations (`+ - * /` and `sqrt`), which IEEE 754 rounds
//! the same way everywhere; no `sin`, `cos`, `pow` or fused multiply-add is used.

pub mod glb;
pub mod kit;
pub mod models;
pub mod palette;
pub mod shade;

/// Every file of set v5: `(file name, bytes)`, in the order of [`models::MODELS`].
pub fn files() -> Vec<(String, Vec<u8>)> {
    models::MODELS
        .iter()
        .map(|build| {
            let model = build();
            (format!("{}.glb", model.stem), glb::write(&model))
        })
        .collect()
}
