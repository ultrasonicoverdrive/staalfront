//! Decor and wrecks in style G (WP11.6d, docs/art-style.md section 12): the four wrecks, two
//! rocks, a fence, a cliff edge, the ore field stages and the broadleaf tree with its fallen
//! twin. Everything is neutral (no player colour): the palette entry `neutral`, trees `flora`.
//!
//! Shared shape rules (the new kit rules of this WP):
//! - **Blobs** ([`blob`]): a rock-like solid whose outline radius is jittered by a fixed hash of
//!   the *absolute angle*, except at the four compass points, which are always the full radius.
//!   So the 16-, 8- and 4-sided levels share their extremes and the far levels keep the extent.
//! - **A wreck is burnt**: the paint is toned down (`SOOT`), steel and rubber are darker still,
//!   rust (`accent`) patches sit on the sides, and every wreck has the nodes `hull`, `turret`,
//!   `barrel` (empty anchors `fx_smoke_0` / `fx_fire_0` under the hull) in every level.

use crate::kit::{
    Lod, Mat, Mesh, Model, Node, P, RECESS, SEAM, Tone, bevel_box, chamfer_rect, closed_box,
    cuboid, frame, lathe, lathe_x, lathe_z, loft, on_circle,
};
use crate::models::anchors;
use crate::palette::{Palette, slot::*};
use crate::stages::{chunk, fallen_plate, hash};

/// The tone of burnt paint (per mille of the baked colour; `kit::Tone`).
pub const SOOT: Tone = 200;
/// The tone of steel in a wreck.
pub const BURNT_STEEL: Tone = 420;
/// The tone of the soot on the ground round a wreck and on its steel debris.
pub const CHAR: Tone = 380;
/// The tone of rust on a wreck (rust-brown patches on the soot-dark hull).
pub const RUST: Tone = 650;
/// The tone of old fence wire and posts.
pub const OLD_WIRE: Tone = 430;
/// The tone of ore nuggets (a darker, warmer mineral than the rust-brown palette accent).
pub const ORE_TONE: Tone = 800;
/// Nuggets on the ore field stages 1 to 4 (pinned by `the_ore_field_stages_grow`).
pub const ORE_NUGGETS: [i32; 4] = [4, 6, 9, 13];
/// The tone of a dark pit (burnt-through deck, engine hole): rubber needs at least this much
/// light left, or the baked corner crushes to black (`no_baked_vertex_crushes_to_black`).
pub const PIT: Tone = 560;

/// The ring of a blob: `segs` points, radius 1000 at the compass points, between `lo` and 1000
/// per mille elsewhere, from a hash of the absolute angle (`k * 64 / segs`).
fn blob_ring(segs: u32, seed: u32, lo: i32) -> Vec<[i32; 2]> {
    (0..segs)
        .map(|k| {
            let a = k * 64 / segs;
            let j = if a.is_multiple_of(16) {
                1000
            } else {
                lo + hash(seed.wrapping_mul(131) ^ a) as i32 * (1000 - lo) / 1000
            };
            on_circle(j, k as i32, segs)
        })
        .collect()
}

/// A rock-like solid on the ground at `at` (on its base): `rx` and `rz` half widths, `h` high,
/// 16 / 8 / 4 sides (`Lod::segs`). LOD0 has a middle ring, so the side bulges; the top is a
/// small flat cap leaning a little. Bands: `mats` (low, high), the cap `top`.
#[allow(clippy::too_many_arguments)]
pub fn blob(
    at: P,
    rx: i32,
    rz: i32,
    h: i32,
    seed: u32,
    lod: Lod,
    mats: [Mat; 2],
    top: Mat,
) -> Mesh {
    let segs = lod.segs(16);
    let ring = blob_ring(segs, seed, 780);
    let scaled = |s: i32, dx: i32| -> Vec<[i32; 2]> {
        ring.iter()
            .map(|p| {
                [
                    p[0] * rx / 1000 * s / 1000 + dx,
                    p[1] * rz / 1000 * s / 1000,
                ]
            })
            .collect()
    };
    let lean = (hash(seed ^ 0x51) as i32 - 500) * rx / 3000;
    if lod.has(0) {
        loft(
            &[
                (scaled(1000, 0), 0),
                (scaled(960, 0), h * 45 / 100),
                (scaled(560, lean), h),
            ],
            &mats,
            Some(top),
            None,
        )
        .moved(at)
    } else {
        loft(
            &[(scaled(1000, 0), 0), (scaled(600, lean), h)],
            &[mats[1]],
            Some(top),
            None,
        )
        .moved(at)
    }
}

/// A decor model: one `body` node, no anchors.
fn decor(stem: &str, palette: &str, parts: impl Fn(Lod) -> Mesh) -> Model {
    let pal = Palette::shipped();
    Model::new(stem, pal.materials(palette), |lod| {
        vec![Node::part("body", [0, 0, 0], parts(lod))]
    })
}

// ---- Rocks -------------------------------------------------------------------------------------

/// `(x, z, rx, rz, h, seed)` of the boulders of a rock; the first is the main mass.
type Boulder = (i32, i32, i32, i32, i32, u32);

fn rock(stem: &str, boulders: &'static [Boulder], stones: u32, half: (i32, i32)) -> Model {
    decor(stem, "neutral", move |lod| {
        let mut m = Mesh::new();
        // The levels keep the boulders that set the extent: the first four at LOD1 and LOD2.
        let keep = if lod.has(0) {
            boulders.len()
        } else {
            boulders.len().min(4)
        };
        for (i, &(x, z, rx, rz, h, s)) in boulders.iter().take(keep).enumerate() {
            let mats = [[PAINT2, PAINT], [PAINT2, PAINT], [PAINT, PAINT3]][i % 3];
            m.add(blob([x, 0, z], rx, rz, h, s, lod, mats, PAINT));
        }
        if lod.has(0) {
            // Loose stones at the foot.
            for k in 0..stones {
                let s = 900 + k;
                let x = (hash(s) as i32 - 500) * half.0 / 560;
                let z = (hash(s ^ 7) as i32 - 500) * half.1 / 560;
                m.add(blob(
                    [x, 0, z],
                    170 + hash(s ^ 3) as i32 / 8,
                    150 + hash(s ^ 5) as i32 / 9,
                    190 + hash(s ^ 9) as i32 / 6,
                    s,
                    lod,
                    [PAINT2, PAINT],
                    PAINT,
                ));
            }
        }
        m
    })
}

const LARGE_ROCK: [Boulder; 9] = [
    (-200, 100, 1850, 1450, 2300, 11),
    (1450, -650, 1000, 900, 1300, 12),
    (-1650, -1100, 900, 950, 1100, 13),
    (700, 1250, 900, 800, 1000, 14),
    (-1800, 1150, 700, 650, 700, 15),
    (1650, 1000, 600, 600, 640, 16),
    (-300, -1450, 650, 600, 620, 17),
    (1950, -1600, 450, 400, 420, 18),
    (-1950, -100, 420, 400, 400, 19),
];

const SMALL_ROCK: [Boulder; 5] = [
    (0, 0, 700, 560, 760, 21),
    (560, -420, 380, 330, 400, 22),
    (-640, 380, 340, 330, 360, 23),
    (500, 520, 260, 240, 280, 24),
    (-500, -480, 240, 230, 240, 25),
];

pub fn rock_large() -> Model {
    rock("neutral_rock_large", &LARGE_ROCK, 10, (2300, 1800))
}

pub fn rock_small() -> Model {
    rock("neutral_rock_small", &SMALL_ROCK, 6, (1000, 600))
}

// ---- Fence and cliff edge -----------------------------------------------------------------------

/// A fence section 4 m long and 140 mm thick: three posts, two rails, wire bars; one bar bent
/// and the wire torn at the right end.
pub fn fence() -> Model {
    decor("neutral_fence", "neutral", |lod| {
        let mut m = Mesh::new();
        for x in [-1920, 0, 1920] {
            m.add(closed_box([x - 70, 0, -70], [x + 70, 1500, 70], PAINT2));
            if lod.has(0) {
                m.add(
                    closed_box([x - 75, 1500, -75], [x + 75, 1560, 75], METAL).toned(BURNT_STEEL),
                );
                m.add(closed_box([x - 75, 0, -75], [x + 75, 90, 75], PAINT).toned(RECESS));
            }
        }
        for y in [420, 1100] {
            m.add(closed_box([-1920, y, -40], [1920, y + 70, 40], METAL).toned(BURNT_STEEL));
        }
        if lod.has(1) {
            let n = if lod.has(0) { 22 } else { 6 };
            for k in 0..n {
                let x = -1750 + k * 3500 / (n - 1);
                // The bars of the torn end stand shorter.
                let top = if x > 1200 { 760 + (k % 3) * 120 } else { 1380 };
                m.add(closed_box([x - 20, 90, -20], [x + 20, top, 20], PAINT3).toned(OLD_WIRE));
            }
        }
        if lod.has(0) {
            // Diagonal bracing and a bent bar lying in the grass.
            m.add(closed_box([-1850, 430, 40], [-60, 480, 70], METAL).toned(CHAR));
            m.add(closed_box([60, 1060, 40], [1850, 1110, 70], METAL).toned(CHAR));
            m.add(fallen_plate([1100, 0, 0], 700, 120, 120, 0, METAL).toned(CHAR));
        }
        m
    })
}

/// A cliff edge 4 m wide, 2 m deep and up to 3 m high: layered strata stepping down towards
/// `+Z`, a cap of boulders on top and a talus heap at the foot.
pub fn cliff_edge() -> Model {
    decor("neutral_cliff_edge", "neutral", |lod| {
        let mut m = Mesh::new();
        // Strata: `(y0, y1, front z)`, each narrower than the one below.
        let layers: [(i32, i32, i32); 5] = [
            (0, 800, 950),
            (800, 1500, 700),
            (1500, 2000, 380),
            (2000, 2350, 80),
            (2350, 2600, -250),
        ];
        let upto = 5;
        for (k, &(y0, y1, zf)) in layers.iter().take(upto).enumerate() {
            let k = k as i32;
            let mat = [PAINT2, PAINT, PAINT2, PAINT, PAINT][k as usize];
            let hx = 2000 - 40 * k;
            let z0 = -1000;
            m.add(bevel_box(
                [-hx, y0, z0],
                [hx, y1, zf],
                lod.bevel(70),
                mat,
                mat,
            ));
            if lod.has(0) {
                // A seam between the strata and a crack down the face.
                m.add(
                    cuboid([-hx + 20, y1 - 10, zf], [hx - 20, y1 + 30, zf + 40], METAL).toned(SEAM),
                );
                let cx = -900 + 650 * k;
                m.add(
                    closed_box([cx, y0 + 60, zf], [cx + 50, y1 - 60, zf + 36], METAL).toned(CHAR),
                );
            }
        }
        // The cap and the talus: boulders sitting on and below the face.
        let caps: [(i32, i32, i32, i32, u32); 4] = [
            (-1300, -300, 560, 400, 31),
            (700, -330, 640, 400, 32),
            (1500, 250, 380, 360, 33),
            (-200, -500, 460, 400, 34),
        ];
        for &(x, z, r, h, s) in &caps {
            m.add(chunk([x, 2600, z], r, h, s, PAINT, lod));
        }
        for (k, &(x, z, r, h)) in [
            (-1400, 480, 400, 520),
            (-300, 520, 380, 420),
            (1100, 470, 440, 600),
        ]
        .iter()
        .enumerate()
        {
            m.add(blob(
                [x, 0, z],
                r,
                r * 7 / 10,
                h,
                40 + k as u32,
                lod,
                [PAINT2, PAINT],
                PAINT,
            ));
        }
        if lod.has(0) {
            for k in 0..16u32 {
                let x = (hash(60 + k) as i32 - 500) * 3400 / 1000;
                let z = 540 + (hash(70 + k) as i32 / 10) - 50;
                m.add(chunk(
                    [x.clamp(-1800, 1800), 0, z.min(700)],
                    160,
                    150,
                    80 + k,
                    PAINT2,
                    lod,
                ));
            }
        }
        m
    })
}

// ---- Ore field -------------------------------------------------------------------------------

/// One ore field stage (1 sparse, 4 full), inside the 3.1 x 3.0 m of the set v4 field: a low
/// mound of dark soil with nuggets of rust-brown ore, light crystal tips on the larger ones.
pub fn ore_field(stage: u32) -> Model {
    let stem = format!("neutral_ore_field_stage_{stage:02}");
    decor(&stem, "neutral", move |lod| {
        let mut m = Mesh::new();
        let r = 900 + stage as i32 * 150; // soil radius 1050..1500 along x
        let ring = blob_ring(lod.segs(16), 7 + stage, 800);
        let soil = |s: i32| -> Vec<[i32; 2]> {
            ring.iter()
                .map(|p| {
                    [
                        p[0] * r / 1000 * s / 1000,
                        p[1] * (r - 60) / 1000 * s / 1000,
                    ]
                })
                .collect()
        };
        m.add(
            loft(
                &[(soil(1000), 0), (soil(780), 120)],
                &[RUBBER],
                Some(RUBBER),
                None,
            )
            .toned(760),
        );
        let n = ORE_NUGGETS[(stage - 1) as usize];
        for i in 0..n {
            let s = 300 + stage * 40 + i as u32;
            let a = hash(s) as i32 * 64 / 1000;
            let d = (hash(s ^ 1) as i32 * 7 / 10 + 80) * (r - 350) / 1000;
            let [x, z] = on_circle(d, a, 64);
            let big = 230 + hash(s ^ 2) as i32 / 6 + stage as i32 * 25;
            m.add(
                blob(
                    [x, 60, z],
                    big,
                    big * 9 / 10,
                    big * 14 / 10,
                    s,
                    lod,
                    [ACCENT, ACCENT],
                    ACCENT,
                )
                .toned(ORE_TONE),
            );
        }
        m
    })
}

// ---- Trees ---------------------------------------------------------------------------------

/// A tree crown: three lumps of foliage over a trunk top, `r` the main lump's radius.
fn crown(at: P, r: i32, lod: Lod) -> Mesh {
    let mut m = Mesh::new();
    let lumps: [(i32, i32, i32, i32, u32); 4] = [
        (0, 0, 1000, 1500, 51),
        (r * 6 / 10, r / 4, 700, 1100, 52),
        (-r * 5 / 10, -r / 3, 720, 1150, 53),
        (r / 8, -r * 5 / 10, 600, 1000, 54),
    ];
    for (i, &(dx, dz, s, hh, seed)) in lumps.iter().enumerate() {
        if !lod.has(1) && i > 1 {
            continue;
        }
        let rr = r * s / 1000;
        let base_y = [0, 380, 300, 520][i];
        m.add(blob(
            [at[0] + dx, at[1] + base_y, at[2] + dz],
            rr,
            rr * 95 / 100,
            hh * r / 1500,
            seed,
            lod,
            [
                [PAINT2, PAINT],
                [PAINT, PAINT3],
                [PAINT2, PAINT3],
                [PAINT2, PAINT],
            ][i],
            PAINT3,
        ));
    }
    m
}

fn tree_materials() -> Vec<crate::kit::Material> {
    Palette::shipped().materials("flora")
}

/// The slots of the `flora` palette entry: paint = foliage, paint2 = shaded foliage, paint3 =
/// sunlit foliage, metal = bark, rubber = soil, accent = dry leaves.
pub fn broadleaf() -> Model {
    Model::new("neutral_tree_broadleaf", tree_materials(), |lod| {
        let mut m = lathe(
            &[(300, 0), (200, 500), (160, 3200), (130, 4100)],
            lod.segs(8),
            &[METAL],
            Some(METAL),
            None,
        );
        if lod.has(0) {
            // Root flare and two boughs into the crown.
            for k in 0..4 {
                let [x, z] = on_circle(330, k * 16 + 4, 64);
                m.add(closed_box([x - 70, 0, z - 70], [x + 70, 300, z + 70], METAL).toned(RECESS));
            }
            m.add(closed_box([-60, 3000, 40], [380, 3080, 120], METAL).toned(RECESS));
            m.add(closed_box([-340, 3300, -120], [60, 3380, -40], METAL).toned(RECESS));
        }
        m.add(crown([0, 2900, 0], 1900, lod));
        vec![Node::part("body", [0, 0, 0], m)]
    })
}

/// The fallen broadleaf: the trunk lies along `+X` with the root plate at its foot and the
/// crown crushed on the ground at the other end.
pub fn broadleaf_fallen() -> Model {
    Model::new("neutral_tree_broadleaf_fallen", tree_materials(), |lod| {
        let mut m = lathe_x(
            &[(0, -3200), (170, -3000), (230, 0), (90, 3300)],
            lod.segs(8),
            &[METAL],
            Some(METAL),
            None,
        )
        .moved([0, 240, 0]);
        // The root plate: an upright disc of earth and roots at the foot.
        m.add(
            lathe_x(
                &[(900, -3300), (900, -3150)],
                lod.segs(8),
                &[RUBBER],
                Some(METAL),
                Some(RUBBER),
            )
            .moved([0, 900, 0]),
        );
        if lod.has(0) {
            m.add(closed_box([-3400, 0, -500], [-3150, 70, 500], RUBBER).toned(RECESS));
        }
        // The crown lies along the trunk: four lumps from the middle to the tip, the trunk
        // running through them, each narrower than the one before.
        let lumps: [(i32, i32, i32, i32, u32); 4] = [
            (700, 150, 1050, 1300, 61),
            (1500, -200, 1000, 1250, 62),
            (2250, 150, 880, 1100, 63),
            (2950, -100, 700, 900, 64),
        ];
        for (i, &(x, z, r, h, seed)) in lumps.iter().enumerate() {
            if !lod.has(1) && i % 3 == 2 {
                continue;
            }
            m.add(blob(
                [x, 0, z],
                r,
                r * 95 / 100,
                h,
                seed,
                lod,
                [
                    [PAINT2, PAINT],
                    [PAINT, PAINT3],
                    [PAINT2, PAINT3],
                    [PAINT2, PAINT],
                ][i],
                PAINT3,
            ));
        }
        vec![Node::part("body", [0, 0, 0], m)]
    })
}

// ---- Wrecks ---------------------------------------------------------------------------------

/// What differs between the three vehicle wrecks.
struct Wreck {
    stem: &'static str,
    /// Half length and half width of the footprint, hull height over the running gear.
    hx: i32,
    hz: i32,
    hull_h: i32,
    wheeled: bool,
    /// Road wheels a side of a tracked wreck.
    wheels: i32,
    /// Turret radius, barrel length, barrel count (1 or 2).
    turret_r: i32,
    barrel: i32,
    barrels: i32,
    seed: u32,
}

fn track_side(w: &Wreck, lod: Lod, s: i32, short: bool) -> Mesh {
    let th = 640;
    let (lo, hi) = if s > 0 {
        (w.hz - 520, w.hz)
    } else {
        (-w.hz, -w.hz + 520)
    };
    let x1 = if short { w.hx * 30 / 100 } else { w.hx - 60 };
    let mut m = bevel_box(
        [-w.hx + 60, 0, lo],
        [x1, th, hi],
        lod.bevel(90),
        RUBBER,
        RUBBER,
    )
    .toned(BURNT_STEEL);
    if lod.has(0) {
        // Road wheels on the outer face; the broken side lost the ones ahead of its break.
        let z = if s > 0 { hi } else { lo - 40 };
        for k in 0..w.wheels {
            let x = -w.hx + 420 + k * (2 * w.hx - 840) / (w.wheels - 1);
            if x > x1 - 250 {
                continue;
            }
            m.add(
                lathe_z(&[(250, 0), (250, 40)], 16, &[METAL], Some(METAL), None)
                    .toned(BURNT_STEEL)
                    .moved([x, th / 2, z]),
            );
        }
    }
    m
}

fn vehicle_wreck(w: &'static Wreck) -> Model {
    let pal = Palette::shipped();
    Model::new(w.stem, pal.materials("neutral"), |lod| {
        let seed = w.seed;
        let mut hull = Mesh::new();
        let top;
        let flat = !lod.has(1);
        if w.wheeled {
            // A burnt truck: chassis, cab and bed, wheels (one gone).
            let r = 520;
            top = 900 + w.hull_h;
            hull.add(
                bevel_box(
                    [-w.hx + 150, 520, -w.hz + 380],
                    [w.hx - 150, 900, w.hz - 380],
                    lod.bevel(80),
                    METAL,
                    METAL,
                )
                .toned(BURNT_STEEL),
            );
            hull.add(
                bevel_box(
                    [w.hx * 25 / 100, 900, -w.hz + 380],
                    [w.hx - 150, top, w.hz - 380],
                    lod.bevel(110),
                    PAINT2,
                    PAINT2,
                )
                .toned(SOOT),
            );
            hull.add(
                bevel_box(
                    [-w.hx + 150, 900, -w.hz + 380],
                    [w.hx * 15 / 100, 900 + w.hull_h * 40 / 100, w.hz - 380],
                    lod.bevel(90),
                    PAINT,
                    PAINT,
                )
                .toned(SOOT),
            );
            let prof: &[(i32, i32)] = if flat {
                &[(520, 0), (520, 380)]
            } else {
                &[(460, 0), (520, 50), (520, 330), (460, 380)]
            };
            for (i, x) in [-w.hx * 55 / 100, 0, w.hx * 62 / 100].iter().enumerate() {
                for s in [-1, 1] {
                    if i == 1 && s == 1 {
                        continue; // lost wheel
                    }
                    let z0 = if s > 0 { w.hz - 380 } else { -w.hz };
                    hull.add(
                        lathe_z(prof, lod.segs(8), &[RUBBER], Some(RUBBER), Some(RUBBER))
                            .toned(BURNT_STEEL)
                            .moved([*x, r, z0]),
                    );
                }
            }
            if lod.has(0) {
                hull.add(
                    lathe(
                        &[(460, 0), (520, 50), (520, 330), (460, 380)],
                        16,
                        &[RUBBER],
                        Some(RUBBER),
                        Some(RUBBER),
                    )
                    .toned(BURNT_STEEL)
                    .moved([-w.hx * 15 / 100, 0, w.hz - 600]),
                );
            }
        } else {
            let th = 640;
            top = th + w.hull_h;
            hull.add(track_side(w, lod, -1, false));
            hull.add(track_side(w, lod, 1, true));
            let c = if flat { 0 } else { lod.bevel(110).max(60) };
            let ring = |hx: i32, hz: i32| chamfer_rect(hx, hz, c);
            hull.add(
                loft(
                    &[
                        (ring(w.hx - 160, w.hz - 560), th * 70 / 100),
                        (ring(w.hx - 60, w.hz - 480), th + w.hull_h * 60 / 100),
                        (ring(w.hx - 260, w.hz - 600), top),
                    ],
                    &[PAINT2, PAINT2],
                    Some(PAINT),
                    None,
                )
                .toned(SOOT),
            );
        }
        let (px, pz) = (-w.hx * 35 / 100, w.hz * 30 / 100);
        // The level of the engine deck: the cab roof of a truck, the hull top of a tank.
        let deck_y = if w.wheeled {
            900 + w.hull_h * 40 / 100
        } else {
            top
        };
        if lod.has(1) {
            // The engine deck burnt through: a dark pit with a frame round it.
            hull.add(
                cuboid(
                    [px - 520, deck_y - 10, pz - 420],
                    [px + 520, deck_y + 25, pz + 420],
                    RUBBER,
                )
                .toned(PIT),
            );
        }
        if lod.has(1) {
            // Rust-brown patches on the deck and charred strips along its edges.
            let patches: [(i32, i32, i32, i32); 3] = if w.wheeled {
                [
                    (w.hx * 55 / 100, -w.hz * 20 / 100, 700, 380),
                    (w.hx * 78 / 100, w.hz * 25 / 100, 520, 340),
                    (w.hx * 45 / 100, w.hz * 40 / 100, 420, 300),
                ]
            } else {
                [
                    (w.hx * 10 / 100, -w.hz * 30 / 100, 760, 420),
                    (-w.hx * 55 / 100, -w.hz * 10 / 100, 640, 300),
                    (w.hx * 55 / 100, w.hz * 20 / 100, 560, 360),
                ]
            };
            for (x, z, a, b) in patches {
                hull.add(
                    cuboid(
                        [x - a / 2, top, z - b / 2],
                        [x + a / 2, top + 25, z + b / 2],
                        ACCENT,
                    )
                    .toned(RUST),
                );
            }
        }
        if lod.has(0) {
            let edge = (w.hz - 700).min(w.hz * 6 / 10 + 200);
            for s in [-1, 1] {
                let (z0, z1) = (s * edge - 60, s * edge + 60);
                hull.add(
                    cuboid(
                        [
                            if w.wheeled {
                                w.hx * 30 / 100
                            } else {
                                -w.hx * 45 / 100
                            },
                            top,
                            z0.min(z1),
                        ],
                        [w.hx * 65 / 100, top + 30, z0.max(z1)],
                        METAL,
                    )
                    .toned(CHAR),
                );
            }
            let o = chamfer_rect(600, 500, 80);
            let i = chamfer_rect(520, 420, 60);
            hull.add(
                frame(&o, &i, deck_y, deck_y + 60, METAL)
                    .toned(BURNT_STEEL)
                    .moved([px, 0, pz]),
            );
            // Rust on the side and an open hatch.
            for (k, x) in [-w.hx / 2, w.hx / 6].iter().enumerate() {
                let zz = w.hz - 520 + k as i32 * 30;
                hull.add(
                    closed_box(
                        [*x - 360, top - 400, -zz - 30],
                        [*x + 360, top - 120, -zz + 6],
                        ACCENT,
                    )
                    .toned(RUST),
                );
            }
            hull.add(
                lathe(&[(210, 0), (210, 40), (180, 50)], 16, &[METAL], None, None)
                    .toned(CHAR)
                    .moved([w.hx * 30 / 100, top, -w.hz / 4]),
            );
            // Bent plates and chunks lying beside the wreck, inside the footprint.
            for k in 0..4 {
                let s = seed * 17 + k;
                let x = (hash(s) as i32 - 500) * (w.hx - 700) / 520;
                let z = if k % 2 == 0 { w.hz - 260 } else { -w.hz + 260 };
                hull.add(
                    fallen_plate([x, 0, z], 620, 420, 220, (k % 2 * 2) as u8, METAL).toned(CHAR),
                );
            }
            for k in 0..3 {
                let s = seed * 29 + k;
                let x = ((hash(s) as i32 - 500) * 2 * (w.hx - 400) / 1000)
                    .clamp(-w.hx + 300, w.hx - 300);
                hull.add(
                    chunk(
                        [x, 0, -w.hz + 220 + (k as i32 % 2) * 40],
                        190,
                        160,
                        s,
                        PAINT2,
                        lod,
                    )
                    .toned(SOOT),
                );
            }
        }
        // The turret: thrown back by the blast, resting on the hull off the middle.
        let t_at = [-w.hx * 10 / 100, deck_y, w.hz * 22 / 100];
        let tr = w.turret_r;
        let tring = |s: i32| {
            let c = if flat { 0 } else { (tr * s / 1000) / 3 };
            chamfer_rect(tr * 130 / 100 * s / 1000, tr * s / 1000, c)
        };
        let mut turret = loft(
            &[
                (tring(1000), 0),
                (tring(930), tr * 28 / 100),
                (tring(560), tr * 44 / 100),
            ],
            &[PAINT2, PAINT2],
            Some(PAINT),
            None,
        )
        .toned(SOOT);
        if lod.has(0) {
            turret.add(
                closed_box(
                    [-tr * 90 / 100, tr * 10 / 100, tr * 93 / 100 - 10],
                    [tr * 60 / 100, tr * 26 / 100, tr * 93 / 100 + 30],
                    ACCENT,
                )
                .toned(SOOT),
            );
        }
        let mut barrel_mesh = Mesh::new();
        let bl = w.barrel;
        for b in 0..w.barrels {
            let dz = if w.barrels == 1 { 0 } else { (b * 2 - 1) * 200 };
            barrel_mesh.add(
                lathe_x(
                    &[(70, 0), (70, bl * 75 / 100), (95, bl)],
                    lod.segs(8),
                    &[METAL],
                    Some(METAL),
                    None,
                )
                .toned(BURNT_STEEL)
                .moved([0, 0, dz]),
            );
        }
        let barrel = Node::part("barrel", [tr * 130 / 100, tr * 26 / 100, 0], barrel_mesh);
        let turret_node = Node::part("turret", t_at, turret).child(barrel);
        let hull_node = Node::part("hull", [0, 0, 0], hull).child(turret_node);
        vec![anchors(hull_node, [px, deck_y + 300, pz], [px, deck_y, pz])]
    })
}

static LIGHT: Wreck = Wreck {
    stem: "neutral_wreck_light_vehicle",
    hx: 2500,
    hz: 1400,
    hull_h: 640,
    wheeled: false,
    wheels: 4,
    turret_r: 780,
    barrel: 1000,
    barrels: 1,
    seed: 3,
};
static MEDIUM: Wreck = Wreck {
    stem: "neutral_wreck_medium_vehicle",
    hx: 3150,
    hz: 1620,
    hull_h: 900,
    wheeled: true,
    wheels: 0,
    turret_r: 720,
    barrel: 1000,
    barrels: 1,
    seed: 5,
};
static HEAVY: Wreck = Wreck {
    stem: "neutral_wreck_heavy_vehicle",
    hx: 3600,
    hz: 1850,
    hull_h: 860,
    wheeled: false,
    wheels: 5,
    turret_r: 1000,
    barrel: 1400,
    barrels: 2,
    seed: 7,
};

pub fn wreck_light_vehicle() -> Model {
    vehicle_wreck(&LIGHT)
}

pub fn wreck_medium_vehicle() -> Model {
    vehicle_wreck(&MEDIUM)
}

pub fn wreck_heavy_vehicle() -> Model {
    vehicle_wreck(&HEAVY)
}

/// A sunk gunboat: the hull listing with the stern under water (the waterline is `Y = 0`, 400 mm
/// above the keel at the stern), a torn deck, a leaning bridge, the gun mount hanging askew and
/// the mast lying across the deck.
pub fn wreck_vessel() -> Model {
    let pal = Palette::shipped();
    Model::new("neutral_wreck_vessel", pal.materials("neutral"), |lod| {
        let hz = 2250;
        // The plan of the hull at one level: a square stern, parallel sides, and a bow that
        // narrows to a point (nine corners, the same at every level, convex).
        let plan = |w: i32, xs: i32, xb: i32| -> Vec<[i32; 2]> {
            vec![
                [xs, -w],
                [1500, -w],
                [5200, -w * 80 / 100],
                [8000, -w * 30 / 100],
                [xb, 0],
                [8000, w * 30 / 100],
                [5200, w * 80 / 100],
                [1500, w],
                [xs, w],
            ]
        };
        let deck = 800;
        let mut hull = loft(
            &[
                (plan(hz * 65 / 100, -8200, 8300), -400),
                (plan(hz, -8600, 8900), 300),
                (plan(hz * 95 / 100, -8600, 8700), deck),
            ],
            &[PAINT2, PAINT2],
            Some(PAINT),
            None,
        )
        .toned(SOOT);
        // The bridge, leaning forward on the deck (every level: it sets the height).
        hull.add(
            bevel_box(
                [2200, deck + 400, -1500],
                [4900, deck + 1950, 1500],
                lod.bevel(140),
                PAINT,
                PAINT,
            )
            .toned(SOOT),
        );
        hull.add(cuboid([2300, deck + 1950, -1400], [4800, deck + 2010, 1400], METAL).toned(CHAR));
        if lod.has(1) {
            // The torn deck: a dark hole with ribs standing in it.
            hull.add(cuboid([-3600, deck - 10, -1300], [900, deck + 25, 1300], RUBBER).toned(PIT));
            if lod.has(0) {
                for k in 0..6 {
                    let x = -3300 + k * 700;
                    hull.add(
                        closed_box(
                            [x, deck, -1300],
                            [x + 90, deck + 480 - (k % 3) * 160, -1220],
                            METAL,
                        )
                        .toned(CHAR),
                    );
                    hull.add(
                        closed_box(
                            [x, deck, 1220],
                            [x + 90, deck + 380 - (k % 2) * 220, 1300],
                            METAL,
                        )
                        .toned(CHAR),
                    );
                }
                for k in 0..7 {
                    let x = -8000 + k * 2300;
                    hull.add(
                        closed_box([x, 300, hz - 60], [x + 520, 520, hz + 20], ACCENT).toned(RUST),
                    );
                    hull.add(
                        closed_box([x + 300, 560, -hz - 40], [x + 900, 680, -hz + 40], METAL)
                            .toned(CHAR),
                    );
                }
            }
            if lod.has(0) {
                for k in 0..4 {
                    let z = -1100 + k * 700;
                    hull.add(
                        closed_box([4900, deck + 1000, z], [4960, deck + 1600, z + 380], METAL)
                            .toned(CHAR),
                    );
                }
                // The mast lying across the deck and rails.
                hull.add(
                    lathe_x(
                        &[(80, -2900), (110, 2900)],
                        16,
                        &[METAL],
                        Some(METAL),
                        Some(METAL),
                    )
                    .toned(BURNT_STEEL)
                    .moved([-6200, deck + 140, -900]),
                );
                for k in 0..9 {
                    let x = -8200 + k * 1500;
                    hull.add(
                        closed_box([x, deck, -2130], [x + 50, deck + 700, -2080], METAL)
                            .toned(BURNT_STEEL),
                    );
                }
            }
        }
        // Gun mount: a turret hanging off the foredeck.
        let mut turret = loft(
            &[
                (chamfer_rect(800, 650, 220), 0),
                (chamfer_rect(700, 580, 200), 420),
                (chamfer_rect(420, 360, 120), 640),
            ],
            &[PAINT2, PAINT2],
            Some(PAINT),
            None,
        )
        .toned(SOOT);
        if lod.has(0) {
            turret.add(closed_box([-500, 120, 640], [300, 380, 690], ACCENT).toned(RUST));
        }
        let barrel = Node::part(
            "barrel",
            [800, 330, 0],
            lathe_x(
                &[(90, 0), (90, 1500), (120, 1700)],
                lod.segs(8),
                &[METAL],
                Some(METAL),
                None,
            )
            .toned(BURNT_STEEL),
        );
        let turret_node = Node::part("turret", [5600, deck, -500], turret).child(barrel);
        let mut hull_node = Node::part("hull", [0, 0, 0], hull).child(turret_node);
        hull_node = anchors(hull_node, [-1500, deck + 400, 0], [-1500, deck + 100, 400]);
        vec![hull_node]
    })
}

/// Every decor and wreck recipe of this WP, in no particular order (`set::all` sorts).
pub fn recipes() -> Vec<fn() -> Model> {
    vec![
        wreck_light_vehicle,
        wreck_medium_vehicle,
        wreck_heavy_vehicle,
        wreck_vessel,
        rock_large,
        rock_small,
        fence,
        cliff_edge,
        || ore_field(1),
        || ore_field(2),
        || ore_field(3),
        || ore_field(4),
        broadleaf,
        broadleaf_fallen,
    ]
}
