//! The baked shade rules of the tuning round (14-11-2026, docs/art-style.md section 5), each on a
//! small solid with a known answer: the top-edge highlight, edge wear, the AO strength and grime.
//! Every check runs on [`shade::bake`] itself, the function the GLB writer uses.

use sf_model_gen::kit::{Mat, Mesh, Tri, bevel_box, cuboid, lathe};
use sf_model_gen::shade::{self, NOMINAL, bake, bake_team, factor, unit_normal};

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
    // WP11.6p: a few per cent, not the 16 % of the bright style: visible, and pinned from above.
    assert!(bevel > flat * 1.02, "highlight: {bevel} vs {flat}");
    assert!(
        bevel < flat * 1.08,
        "highlight too strong: {bevel} vs {flat}"
    );
}

/// WP11.6p: the constants of the muted bake, as literals (a changed value fails here).
#[test]
fn the_realistic_bake_constants_are_pinned() {
    assert_eq!(shade::HIGHLIGHT, 0.05);
    const { assert!(shade::HIGHLIGHT <= 0.08, "a few per cent at most") };
    assert_eq!(shade::WEAR, 0.20);
    assert_eq!(shade::WEAR_GREY_GAIN, 1.12);
    assert_eq!(shade::WEAR_GREY_LIFT, 0.01);
    assert_eq!(shade::WEAR_TINT, 0.5);
    assert_eq!(shade::GRIME, 0.20);
    assert_eq!(shade::GRIME_HEIGHT_MM, 1300.0);
    assert_eq!(shade::DUST, 0.28);
    assert_eq!(shade::DUST_SRGB, [118, 104, 82]);
    assert_eq!(sf_model_gen::kit::SEAM, 500);
    assert_eq!(sf_model_gen::kit::RECESS, 720);
}

/// The mean final colour of the top faces of a plate at height `y` (slot `mat`).
fn plate_top(y: i32, mat: u8, team: Option<u8>) -> [f32; 3] {
    let m = cuboid([-1000, y, -1000], [1000, y + 50, 1000], Mat(mat));
    let lin = [COLOUR, COLOUR];
    let baked = bake_team(&m.tris, &lin, team);
    let f = factor(COLOUR);
    let tops: Vec<[f32; 3]> = m
        .tris
        .iter()
        .enumerate()
        .filter(|(_, t)| unit_normal(t)[1] > 0.99)
        .flat_map(|(i, _)| (0..3).map(move |c| i * 3 + c))
        .map(|k| [baked[k][0] * f[0], baked[k][1] * f[1], baked[k][2] * f[2]])
        .collect();
    [0, 1, 2].map(|k| mean(tops.iter().map(|c| c[k])))
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// WP11.6p: the dust. Low faces are darker and closer to the dust colour than high faces of the
/// same material; a team triangle carries neither.
#[test]
fn dust_darkens_and_pulls_the_foot_towards_the_dust_colour_and_a_team_triangle_has_none() {
    let dust = sf_model_gen::shade::DUST_SRGB.map(sf_model_gen::glb::linear);
    let (foot, mid, high) = (
        plate_top(0, 0, None),
        plate_top(700, 0, None),
        plate_top(3000, 0, None),
    );
    println!("foot {foot:?} mid {mid:?} high {high:?}");
    assert!(
        lum(foot) < lum(mid) && lum(mid) < lum(high),
        "darker towards the ground"
    );
    // The saturated COLOUR is far from the earth colour; the foot is nearer to it than the top.
    let (df, dm, dh) = (dist(foot, dust), dist(mid, dust), dist(high, dust));
    println!("distance to the dust colour: foot {df:.3} mid {dm:.3} high {dh:.3}");
    assert!(
        df < dm && dm < dh,
        "closer to the dust colour at the ground"
    );
    // Team: the same plate in the team slot is the same colour at the ground and high up.
    assert_eq!(
        plate_top(0, 1, Some(1)),
        plate_top(3000, 1, Some(1)),
        "team: no grime, no dust"
    );
    // ... and the same two plates in a normal slot differ (the check above has teeth).
    assert_ne!(plate_top(0, 1, None), plate_top(3000, 1, None));
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
    let worn = bevel.iter().filter(|c| sat(**c) < flat_sat * 0.93).count();
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

/// Refinement round 2: the narrow segments of a turned surface (the Meridian dome) are facets of a
/// round body, not exposed bevels, so they get no edge wear (before, the dome showed light
/// streaks). The same dome profile as the construction center, high and alone.
#[test]
fn a_lathed_dome_gets_no_edge_wear() {
    let dome = lathe(
        &[
            (3000, 0),
            (2850, 900),
            (2400, 1700),
            (1600, 2300),
            (700, 2600),
            (0, 2700),
        ],
        32,
        &[Mat(0)],
        None,
        None,
    )
    .moved([0, 5000, 0]);
    assert!(dome.tris.iter().all(|t| t.lathed));
    let c = colours(&dome);
    let sats: Vec<f32> = c.iter().map(|(_, c)| sat(*c)).collect();
    let top = sats.iter().copied().fold(0.0, f32::max);
    let worn = sats.iter().filter(|s| **s < top * 0.93).count();
    assert_eq!(worn, 0, "{worn} of {} dome corners are worn", sats.len());
    // The same mesh without the mark does wear: the rule is what keeps the dome clean.
    let mut plain = dome.clone();
    for t in &mut plain.tris {
        t.lathed = false;
    }
    let sats: Vec<f32> = colours(&plain).iter().map(|(_, c)| sat(*c)).collect();
    assert!(
        sats.iter().any(|s| *s < top * 0.93),
        "the control mesh shows no wear"
    );
}

/// A straight drum of 16 segments, high and alone, closed on top (the cap meets the wall at 90°).
fn drum() -> Mesh {
    lathe(&[(1000, 0), (1000, 1200)], 16, &[Mat(0)], None, None).moved([0, 5000, 0])
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-5)
}

/// The corner normals of `m` at the point `p`.
fn normals_at(m: &Mesh, p: [i32; 3]) -> Vec<(Tri, [f32; 3])> {
    let n = shade::corner_normals(&m.tris);
    m.tris
        .iter()
        .enumerate()
        .flat_map(|(i, t)| (0..3).map(move |k| (i, *t, k)))
        .filter(|(_, t, k)| t.v[*k] == p)
        .map(|(i, t, k)| (t, n[i][k]))
        .collect()
}

/// WP11.6r: on a turned surface the normals of a shared corner agree (the wall is smooth, its
/// segments 22.5° apart), while the cap, 90° away, keeps its own normal; and a box edge stays hard
/// even when the box is marked as turned, because it is sharper than the crease angle.
#[test]
fn smooth_normals_agree_on_a_lathe_and_stay_hard_past_the_crease_angle() {
    assert_eq!(shade::CREASE_DEG, 40);
    assert_eq!(shade::CREASE_COS, 0.766_044);
    let m = drum();
    // A corner of the wall at the foot, shared by several wall triangles.
    let wall = normals_at(&m, [1000, 5000, 0]);
    assert!(wall.len() >= 2, "the corner is shared by the wall");
    assert!(wall.iter().all(|w| close(w.1, wall[0].1)), "{wall:?}");
    // The smooth normal points straight out at this corner (it lies on +X).
    assert!(close(wall[0].1, [1.0, 0.0, 0.0]), "{:?}", wall[0].1);
    // The top corner is shared by the cap: hard edge, two different normals.
    let top: Vec<[f32; 3]> = normals_at(&m, [1000, 6200, 0])
        .into_iter()
        .map(|x| x.1)
        .collect();
    assert!(top.iter().any(|v| close(*v, [0.0, 1.0, 0.0])), "{top:?}");
    assert!(top.iter().any(|v| close(*v, [1.0, 0.0, 0.0])), "{top:?}");
    // A box marked as turned: every corner keeps its face's own normal (90° > 40°).
    let boxed = cuboid([-500, 3000, -500], [500, 3200, 500], Mat(0)).lathed();
    let nb = shade::corner_normals(&boxed.tris);
    for (i, t) in boxed.tris.iter().enumerate() {
        for n in nb[i] {
            assert!(close(n, unit_normal(t)), "box edge smoothed");
        }
    }
    // A flat (not turned) solid is never smoothed, even where its faces meet at a shallow angle.
    let mut flat = drum();
    for t in &mut flat.tris {
        t.lathed = false;
    }
    let nf = shade::corner_normals(&flat.tris);
    for (i, t) in flat.tris.iter().enumerate() {
        assert!((0..3).all(|k| close(nf[i][k], unit_normal(t))));
    }
}

/// WP11.6r: no smooth (lathed) triangle carries facet variation: on a lone drum every wall corner
/// at the foot has the same colour; the same drum unmarked does vary from facet to facet.
#[test]
fn no_smooth_part_carries_facet_variation() {
    let spread = |m: &Mesh| {
        let c = colours(m);
        let side: Vec<f32> = c
            .iter()
            .zip(m.tris.iter().flat_map(|t| t.v))
            .filter(|((t, _), p)| unit_normal(t)[1].abs() < 0.5 && p[1] == 5000)
            .map(|((_, c), _)| lum(*c))
            .collect();
        let hi = side.iter().copied().fold(0.0, f32::max);
        let lo = side.iter().copied().fold(1.0, f32::min);
        hi / lo
    };
    let smooth = drum();
    assert!(spread(&smooth) <= 1.005, "smooth drum {}", spread(&smooth));
    let mut flat = drum();
    for t in &mut flat.tris {
        t.lathed = false;
    }
    assert!(spread(&flat) > 1.01, "flat drum {}", spread(&flat));
}

/// WP11.6p, the black-crush floor: no vertex of any shipped model, times its material factor, is
/// darker (luminance, linear) than [`CRUSH_FLOOR`] outside the team slot. Measured on the finished
/// set (the darkest baked corner is printed); AO, grime, the downward term and the seam tone
/// multiply, and on the near-black slots (rubber, metal) the floor is what keeps a track readable.
/// 0.003, not 0.004: the heavily damaged power plant has a charred metal triangle at 0.0033 (the
/// stage char tone, on purpose); the darkest corner is printed on every run.
const CRUSH_FLOOR: f32 = 0.003;

#[test]
fn no_baked_vertex_crushes_to_black() {
    let mut darkest = (f32::MAX, String::new());
    for m in sf_model_gen::set::all() {
        let lin: Vec<[f32; 3]> = m
            .materials
            .iter()
            .map(|x| x.srgb.map(sf_model_gen::glb::linear))
            .collect();
        let team = m
            .materials
            .iter()
            .position(|x| x.name == sf_model_gen::kit::TEAM)
            .map(|i| i as u8);
        let world = m.lods[0].world_tris();
        let baked = bake_team(&world, &lin, team);
        for (i, t) in world.iter().enumerate() {
            if team == Some(t.mat.0) {
                continue;
            }
            let f = factor(lin[usize::from(t.mat.0)]);
            for c in 0..3 {
                let b = baked[i * 3 + c];
                let l = lum([b[0] * f[0], b[1] * f[1], b[2] * f[2]]);
                if l < darkest.0 {
                    darkest = (l, format!("{} tri {i} material {}", m.stem, t.mat.0));
                }
            }
        }
    }
    println!("darkest baked corner: {:.5} ({})", darkest.0, darkest.1);
    assert!(darkest.0 >= CRUSH_FLOOR, "crushed to black: {darkest:?}");
}
