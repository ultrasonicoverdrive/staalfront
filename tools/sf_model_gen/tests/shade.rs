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
    #[allow(clippy::assertions_on_constants)]
    {
        assert!(shade::HIGHLIGHT <= 0.08, "a few per cent at most");
    }
    assert_eq!(shade::WEAR, 0.20);
    assert_eq!(shade::WEAR_GREY_GAIN, 1.12);
    assert_eq!(shade::WEAR_GREY_LIFT, 0.01);
    assert_eq!(shade::WEAR_TINT, 0.5);
    assert_eq!(shade::GRIME, 0.20);
    assert_eq!(shade::GRIME_HEIGHT_MM, 1300.0);
    assert_eq!(shade::DUST, 0.28);
    assert_eq!(shade::FACTOR_FLOOR, 0.15);
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
    // The grime alone only darkens (the hue stays); the dust moves the colour towards the earth
    // colour, so the blue share of the (very orange) test colour grows towards the ground.
    let blue = |c: [f32; 3]| c[2] / c[0];
    assert!(
        blue(foot) > blue(high) * 1.15 && blue(mid) < blue(foot) && blue(mid) > blue(high),
        "dust: blue share foot {} mid {} high {}",
        blue(foot),
        blue(mid),
        blue(high)
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
/// darker (luminance, linear) than [`CRUSH_FLOOR`] outside the team slot. AO, grime, the downward
/// term and the seam tone multiply, and on the near-black slots (rubber, metal) the floor is what
/// keeps a track readable. The one named exception is the stages' fire damage: a triangle toned
/// `stages::CHAR` or `stages::SOOT` is burnt black on purpose (the darkest of them, 0.0033, is on
/// the heavily damaged Meridian power plant).
const CRUSH_FLOOR: f32 = 0.004;

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
            if team == Some(t.mat.0)
                || t.tone == sf_model_gen::stages::CHAR
                || t.tone == sf_model_gen::stages::SOOT
            {
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

/// Every vertex colour channel that `bake` clamped at 1.0 (the value the clamp leaves), counted
/// over the triangles of the given material slots; a clamped channel has lost its hue and its
/// occlusion (WP11.6p review: the dust on rubber and metal).
fn clamped(tris: &[Tri], lin: &[[f32; 3]], team: Option<u8>, slots: &[u8]) -> usize {
    let baked = bake_team(tris, lin, team);
    tris.iter()
        .enumerate()
        .filter(|(_, t)| slots.contains(&t.mat.0) && team != Some(t.mat.0))
        .flat_map(|(i, _)| (0..3).map(move |c| i * 3 + c))
        .filter(|&k| baked[k].iter().any(|&q| q >= 0.9999))
        .count()
}

/// The shipped models' linear material colours and team slot.
fn linear_of(m: &sf_model_gen::kit::Model) -> (Vec<[f32; 3]>, Option<u8>) {
    let lin = m
        .materials
        .iter()
        .map(|x| x.srgb.map(sf_model_gen::glb::linear))
        .collect();
    let team = m
        .materials
        .iter()
        .position(|x| x.name == sf_model_gen::kit::TEAM)
        .map(|i| i as u8);
    (lin, team)
}

/// The dark slots (metal 3, rubber 4) have little head room in the vertex colour (the material
/// factor is a grey of the brightest channel over `NOMINAL`); the dust, which lifts them towards a
/// brighter earth colour, must not overflow it. Before the factor floor, 5-25 % of the vertices of
/// a tank's metal and rubber clamped to flat grey.
#[test]
fn the_dust_never_clamps_a_vertex_of_the_dark_slots() {
    let (mut dark, mut all) = (0, 0);
    for m in sf_model_gen::set::all() {
        let (lin, team) = linear_of(&m);
        let world = m.lods[0].world_tris();
        let d = clamped(&world, &lin, team, &[3, 4]);
        let a = clamped(&world, &lin, team, &[0, 1, 2, 3, 4, 5]);
        if d > 0 || a > 0 {
            println!("{}: {d} dark-slot and {a} clamped vertices in all", m.stem);
        }
        dark += d;
        all += a;
    }
    println!("clamped vertices over the set: {dark} in the dark slots, {all} in all");
    assert_eq!(dark, 0, "dark slots clamp");
}

/// The same on a plate in each shipped dark colour: low faces move towards the dust hue (the blue
/// share over the red share grows towards the ground), with no channel at the clamp.
#[test]
fn dust_lifts_rubber_and_metal_towards_the_dust_hue_without_clamping() {
    let p = sf_model_gen::palette::Palette::shipped();
    for faction in ["meridian", "veyran"] {
        let f = &p.factions[faction];
        for (name, srgb) in [("rubber", f.rubber), ("metal", f.metal)] {
            let lin = [srgb.map(sf_model_gen::glb::linear)];
            let fac = factor(lin[0]);
            let vertex = |y: i32| {
                let m = cuboid([-1000, y, -1000], [1000, y + 50, 1000], Mat(0));
                let b = bake(&m.tris, &lin);
                let i = m
                    .tris
                    .iter()
                    .position(|t| unit_normal(t)[1] > 0.99)
                    .unwrap();
                b[i * 3]
            };
            let (foot, high) = (vertex(0), vertex(3000));
            println!("{faction} {name}: foot {foot:?} high {high:?}");
            assert!(foot.iter().all(|&q| q < 0.9999), "{faction} {name} clamps");
            // Dark, near-neutral materials get warmer: red over blue grows towards the ground.
            let warm = |q: [f32; 3]| (q[0] * fac[0]) / (q[2] * fac[2]);
            assert!(
                warm(foot) > warm(high) * 1.10,
                "{faction} {name}: red over blue {} at the foot, {} high",
                warm(foot),
                warm(high)
            );
        }
    }
}

/// A panel line is half the plain shade (tone 500; it was 560): measured on the same plate plain
/// and as a seam, through the bake.
#[test]
fn a_panel_line_is_half_as_bright_as_the_plain_face() {
    let top = |m: &Mesh| {
        let b = bake(&m.tris, &[COLOUR]);
        let i = m
            .tris
            .iter()
            .position(|t| unit_normal(t)[1] > 0.99)
            .unwrap();
        lum(b[i * 3])
    };
    let plain = cuboid([-1000, 3000, -1000], [1000, 3050, 1000], Mat(0));
    let seam = plain.clone().toned(sf_model_gen::kit::SEAM);
    let r = top(&seam) / top(&plain);
    println!("panel line / plain = {r:.3}");
    assert!((0.48..=0.52).contains(&r), "panel line is {r} of plain");
}

/// Worn paint, not bright metal: the brightest corner of a worn bevel stays within 13 % of the
/// flat top's luminance. (The bright style, wear 0.35 towards 1.25x grey, went well beyond.)
#[test]
fn worn_edges_stay_close_to_the_paint() {
    let c = colours(&bevelled());
    let flat = mean(
        c.iter()
            .filter(|(t, _)| unit_normal(t)[1] > 0.99)
            .map(|(_, c)| lum(*c)),
    );
    let brightest = c
        .iter()
        .filter(|(t, _)| {
            unit_normal(t)
                .iter()
                .all(|v| v.abs() < shade::HIGHLIGHT_MAX_UP)
        })
        .map(|(_, c)| lum(*c))
        .fold(0.0, f32::max);
    println!(
        "brightest bevel corner / flat top = {:.3}",
        brightest / flat
    );
    assert!(brightest < flat * 1.13, "{brightest} vs {flat}");
}

/// Measured on the shipped tanks: dE 22.7, dL 12.2 (and 24.1 / 15.2 on the pixels of the M1 shot).
const READ_DE: f64 = 20.0;
const READ_DL: f64 = 10.0;

/// sRGB 0..1 to CIE L*a*b* (D65), f64: a test helper.
fn lab_of(srgb: [f64; 3]) -> [f64; 3] {
    let lin = |s: f64| {
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (lin(srgb[0]), lin(srgb[1]), lin(srgb[2]));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f64| {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// The area-weighted mean sRGB of the visible hull paint (slots paint and paint2, faces looking
/// up) of a model's LOD0 as the renderer shows it: the baked vertex colour times the material
/// factor, lit by the default light of `sf_render::scene::Lighting` (direction (0.5, -0.8, 0.4),
/// colour (0.85, 0.82, 0.75), ambient (0.22, 0.25, 0.30); copied here, the generator does not
/// depend on the renderer).
fn lit_hull(stem: &str) -> [f64; 3] {
    let m = sf_model_gen::set::all()
        .into_iter()
        .find(|m| m.stem == stem)
        .unwrap_or_else(|| panic!("no model {stem}"));
    let (lin, team) = linear_of(&m);
    let world = m.lods[0].world_tris();
    let baked = bake_team(&world, &lin, team);
    let d = {
        let l = (0.5f64 * 0.5 + 0.8 * 0.8 + 0.4 * 0.4).sqrt();
        [-0.5 / l, 0.8 / l, -0.4 / l]
    };
    let (col, amb) = ([0.85f64, 0.82, 0.75], [0.22f64, 0.25, 0.30]);
    let (mut sum, mut area) = ([0.0f64; 3], 0.0f64);
    for (i, t) in world.iter().enumerate() {
        let n = unit_normal(t);
        if t.mat.0 > 1 || n[1] < 0.3 {
            continue;
        }
        let f = factor(lin[usize::from(t.mat.0)]);
        let facing =
            (f64::from(n[0]) * d[0] + f64::from(n[1]) * d[1] + f64::from(n[2]) * d[2]).max(0.0);
        let a = {
            let n = t.normal();
            ((n[0] as f64).powi(2) + (n[1] as f64).powi(2) + (n[2] as f64).powi(2)).sqrt()
        };
        for k in 0..3 {
            let v: f64 = (0..3)
                .map(|c| f64::from(baked[i * 3 + c][k] * f[k]))
                .sum::<f64>()
                / 3.0;
            sum[k] += a * v * (amb[k] + col[k] * facing);
        }
        area += a;
    }
    sum.map(|x| {
        let l = (x / area).clamp(0.0, 1.0);
        if l <= 0.003_130_8 {
            12.92 * l
        } else {
            1.055 * l.powf(1.0 / 2.4) - 0.055
        }
    })
}

/// WP11.6p (review): the two factions read apart as rendered. The palette test cannot see grime,
/// dust, AO or the light, so this bakes the shipped light tanks of both factions and compares the
/// lit mean colour of the visible hull paint. The M1 shot at zoom 0.12 (two tanks close up,
/// `docs/screenshots/wp11.6p/after_m1_zoom_0.12.png`) measured dE 24.1 and dL 15.2 on its pixels.
#[test]
fn the_factions_read_apart_as_baked_and_lit() {
    let m = lit_hull("meridian_light_tank");
    let v = lit_hull("veyran_light_tank");
    let (lm, lv) = (lab_of(m), lab_of(v));
    let de = ((lm[0] - lv[0]).powi(2) + (lm[1] - lv[1]).powi(2) + (lm[2] - lv[2]).powi(2)).sqrt();
    let dl = lm[0] - lv[0];
    println!("meridian {m:?} veyran {v:?}: dE {de:.1}, dL {dl:.1}");
    // Meridian leans warm, Veyran cool.
    assert!(m[0] > m[2] && v[2] > v[0], "warm vs cool: {m:?} {v:?}");
    assert!(de >= READ_DE, "dE {de}");
    assert!(dl >= READ_DL, "dL {dl}");
}
