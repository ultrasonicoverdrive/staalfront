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
//! - [`cutout`]: the restyled cutscene parts (two tones, a soft shade, a thin outline).
//!
//! Floating point appears only in basic operations (`+ - * /` and `sqrt`), which IEEE 754 rounds
//! the same way everywhere; no `sin`, `cos`, `pow` or fused multiply-add is used.

pub mod cutout;
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

/// The scene whose rigs are restyled, relative to `assets/`.
pub const CUTOUT_SCENE: &str = "scenes/f1_m01_briefing.ron";
/// Where the restyled parts go, relative to `assets/`: one folder per character. The shipped
/// scene is not switched over to them (that is the Phase 12 work package).
pub const CUTOUT_DIR: &str = "scenes/restyle_v5";

/// The restyled cutscene parts: `(path under assets/, PNG bytes)`, per character in scene order,
/// per part in rig order, then its mouth shapes (`<part>_<letter>.png`).
///
/// # Panics
/// When the shipped scene does not load.
pub fn cutout_files() -> Vec<(String, Vec<u8>)> {
    use sf_cutscene::Art;
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let scene = sf_cutscene::load_file(&assets.join(CUTOUT_SCENE), None)
        .expect("the shipped briefing loads")
        .scene;
    let pal = palette::Palette::shipped();
    let m = &pal.factions["meridian"];
    let swatches = [
        m.paint,
        m.paint_light,
        m.paint2,
        m.paint2_shade,
        m.metal,
        m.accent,
    ];
    let shape = |art: &Art| -> Option<(cutout::Shape, [u8; 3])> {
        match art {
            Art::Image(_) => None,
            Art::Rect { w, h, color } => Some((
                cutout::Shape::Rect { w: *w, h: *h },
                [color.r, color.g, color.b],
            )),
            Art::RoundRect {
                w,
                h,
                radius,
                color,
            } => Some((
                cutout::Shape::RoundRect {
                    w: *w,
                    h: *h,
                    radius: *radius,
                },
                [color.r, color.g, color.b],
            )),
            Art::Circle { radius, color } => Some((
                cutout::Shape::Circle { radius: *radius },
                [color.r, color.g, color.b],
            )),
        }
    };
    let mut out = Vec::new();
    let mut add = |path: String, art: &Art| {
        if let Some((s, c)) = shape(art) {
            let (w, h, px) = cutout::draw(s, cutout::snap(c, &swatches));
            out.push((path, cutout::png(w, h, &px)));
        }
    };
    for ch in &scene.characters {
        for part in &ch.parts {
            add(
                format!("{CUTOUT_DIR}/{}/{}.png", ch.id, part.name),
                &part.art,
            );
            for (shape_id, art) in &part.mouth_shapes {
                add(
                    format!(
                        "{CUTOUT_DIR}/{}/{}_{}.png",
                        ch.id,
                        part.name,
                        shape_id.letter()
                    ),
                    art,
                );
            }
        }
    }
    out
}
