//! The baked shade rules of the tuning round (14-11-2026, docs/art-style.md section 5), each on a
//! small solid with a known answer: the top-edge highlight, edge wear, the AO strength and grime.
//! Every check runs on [`shade::bake`] itself, the function the GLB writer uses.

use sf_model_gen::kit::{Mat, Mesh, Tri, bevel_box, cuboid};
use sf_model_gen::shade::{self, NOMINAL, bake, factor, unit_normal};

/// A saturated colour (linear), so desaturation shows.
const COLOUR: [f32; 3] = [0.7, 0.45, 0.08];

/// The final linear colour of every corner: vertex colour times the stored material factor.
fn colours(mesh: &Mesh) -> Vec<(Tri, [f32; 3])> {
    let baked = bake(&mesh.tris, &[COLOUR]);
    let f = factor(COLOUR);
    mesh.tris
        .iter()
        .enumerate()
        .flat_map(|(i, t)| (0..3).map(move |c| (i, *t, c)))
        .map(|(i, t, c)| {
            let v = baked[i * 3 + c];
            (t, [v[0] * f[0], v[1] * f[1], v[2] * f[2]])
        })
        .collect()
}

fn lum(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// Saturation as (max - min) / max.
fn sat(c: [f32; 3]) -> f32 {
    let hi = c.iter().copied().fold(0.0, f32::max);
    let lo = c.iter().copied().fold(1.0, f32::min);
    (hi - lo) / hi
}

/// A bevelled box standing high (no grime), alone (no occlusion).
fn bevelled() -> Mesh {
    bevel_box(
        [-1000, 3000, -1000],
        [1000, 4000, 1000],
        200,
        Mat(0),
        Mat(0),
    )
}

fn mean(v: impl Iterator<Item = f32>) -> f32 {
    let v: Vec<f32> = v.collect();
    v.iter().sum::<f32>() / v.len() as f32
}

#[test]
fn a_plain_open_face_is_exactly_the_palette_colour() {
    // A flat roof high above the ground, nothing over it: NOMINAL times the factor.
    let m = cuboid([-1000, 3000, -1000], [1000, 3200, 1000], Mat(0));
    let baked = bake(&m.tris, &[COLOUR]);
    let top = m
        .tris
        .iter()
        .position(|t| unit_normal(t)[1] > 0.99)
        .expect("a top");
    let f = factor(COLOUR);
    for c in 0..3 {
        let got = baked[top * 3][c] * f[c];
        assert!(
            (got - COLOUR[c]).abs() < 0.06 * COLOUR[c] + 0.004,
            "{got} vs {COLOUR:?}"
        );
    }
    const { assert!(NOMINAL < 1.0, "head room for the highlight") };
}

#[test]
fn the_top_edge_bevels_are_brighter_than_the_flat_top() {
    let c = colours(&bevelled());
    let flat = mean(
        c.iter()
            .filter(|(t, _)| unit_normal(t)[1] > 0.99)
            .map(|(_, c)| lum(*c)),
    );
    let bevel = mean(
        c.iter()
            .filter(|(t, _)| {
                let n = unit_normal(t);
                n[1] > shade::HIGHLIGHT_MIN_UP && n[1] < shade::HIGHLIGHT_MAX_UP
            })
            .map(|(_, c)| lum(*c)),
    );
    println!("flat top {flat:.4}, top bevels {bevel:.4}");
    assert!(bevel > flat * 1.08, "highlight: {bevel} vs {flat}");
}

#[test]
fn exposed_bevels_show_lighter_desaturated_wear() {
    let c = colours(&bevelled());
    let flat: Vec<[f32; 3]> = c
        .iter()
        .filter(|(t, _)| unit_normal(t)[1] > 0.99)
        .map(|(_, c)| *c)
        .collect();
    let bevel: Vec<[f32; 3]> = c
        .iter()
        .filter(|(t, _)| {
            unit_normal(t)
                .iter()
                .all(|v| v.abs() < shade::HIGHLIGHT_MAX_UP)
        })
        .map(|(_, c)| *c)
        .collect();
    let flat_sat = mean(flat.iter().map(|c| sat(*c)));
    let worn = bevel.iter().filter(|c| sat(**c) < flat_sat * 0.85).count();
    println!(
        "flat saturation {flat_sat:.3}, worn corners {worn} of {}",
        bevel.len()
    );
    // About two thirds of the bevel triangles are worn (a fixed hash), never all of them.
    assert!(worn * 3 >= bevel.len(), "wear: {worn} of {}", bevel.len());
    assert!(
        worn < bevel.len(),
        "wear on every bevel is a uniform tint, not wear"
    );
}

#[test]
fn a_crease_is_clearly_darker_and_grime_darkens_the_foot() {
    // A wall standing between two slabs: the slab corners against the wall get the AO.
    let mut m = cuboid([-3000, 2000, -3000], [-200, 2100, 3000], Mat(0));
    m.add(cuboid([200, 2000, -3000], [3000, 2100, 3000], Mat(0)));
    m.add(cuboid([-200, 2000, -3000], [200, 3600, 3000], Mat(0)));
    let baked = bake(&m.tris, &[COLOUR]);
    let corner = |pick: &dyn Fn([i32; 3]) -> bool| -> f32 {
        let f = factor(COLOUR);
        let mut v = Vec::new();
        for (i, t) in m.tris.iter().enumerate() {
            if unit_normal(t)[1] < 0.99 {
                continue;
            }
            for (c, p) in t.v.iter().enumerate() {
                if pick(*p) {
                    let b = baked[i * 3 + c];
                    v.push(lum([b[0] * f[0], b[1] * f[1], b[2] * f[2]]));
                }
            }
        }
        mean(v.into_iter())
    };
    let crease = corner(&|p| p[1] == 2100 && p[0].abs() == 200);
    let open = corner(&|p| p[1] == 2100 && p[0].abs() == 3000);
    println!("open {open:.4}, crease {crease:.4}");
    // Twice the visibility of the first round: at least 35 % darker in the crease.
    assert!(crease < open * 0.65, "AO: {crease} vs {open}");
    // Grime: the same plate at the ground is at least 30 % darker than high up.
    let low = colours(&cuboid([-1000, 0, -1000], [1000, 50, 1000], Mat(0)));
    let high = colours(&cuboid([-1000, 3000, -1000], [1000, 3050, 1000], Mat(0)));
    let top = |v: &[(Tri, [f32; 3])]| {
        mean(
            v.iter()
                .filter(|(t, _)| unit_normal(t)[1] > 0.99)
                .map(|(_, c)| lum(*c)),
        )
    };
    println!("ground {:.4}, high {:.4}", top(&low), top(&high));
    assert!(top(&low) < top(&high) * 0.7, "grime");
}
