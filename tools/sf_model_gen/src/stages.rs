//! The states of a structure (WP11.6c, docs/art-style.md section 11): the build stages
//! (`_foundation`, `_scaffolding`) and the damage stages (`_damaged`, `_heavily_damaged`) are made
//! from the finished model by fixed rules, so every structure gets all of them in the same style:
//!
//! - **foundation:** a poured slab of the finished model's platform with footing beams, rebar,
//!   formwork boards and survey stakes; no building yet;
//! - **scaffolding:** the finished model cut off at about half its height ([`clip`]), a deck over the
//!   open walls, and a scaffold of poles, ledgers and planks round it with a crane on one corner;
//! - **damaged:** the finished model with a hole blown in its roof (the triangles round one point
//!   removed), soot round the hole and debris on the ground;
//! - **heavily damaged:** the finished model broken off along a slanted plane, rubble inside the
//!   open shell, two holes, soot and more debris; a turret sinks onto the break.
//!
//! Every stage keeps the team colour at 20-30 % of the top-down view with the same narrow
//! markings as the finished models: [`fit_team`] switches fixed candidate stripes (toe boards on
//! the scaffold, stripes on footing beams, fallen band pieces among the debris) to `team` one by
//! one, in a fixed order, until the share and the triangle count are reached. Integer geometry
//! throughout; the share uses basic float operations only, like the checker's raster.

// The geometry helpers take their sizes as plain arguments, as the kit does.
#![allow(clippy::too_many_arguments)]

use crate::kit::{
    Lod, Mat, Mesh, Model, Node, P, Tri, bevel_box, closed_box, cuboid, dot, extrude_z1, loft, sub,
};
use crate::palette::slot::*;

/// The tone of soot (darker than a recess).
pub const SOOT: crate::kit::Tone = 420;
/// The tone of the burnt rim of a hole.
pub const CHAR: crate::kit::Tone = 300;
/// The team share every stage aims at (the floor; the checker's is 20 %, the tests cap at 30 %).
pub const TEAM_AIM: f64 = 0.216;
/// At least this many team triangles per level (the rule of the finished models).
pub const TEAM_MIN_TRIS: usize = 12;

/// Which state of a structure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Foundation,
    Scaffolding,
    Damaged,
    Heavy,
}

impl Stage {
    pub const ALL: [Stage; 4] = [
        Stage::Foundation,
        Stage::Scaffolding,
        Stage::Damaged,
        Stage::Heavy,
    ];

    /// The suffix of the stage's file (`build_stages.ron`, `damage.ron`).
    pub fn suffix(self) -> &'static str {
        match self {
            Stage::Foundation => "_foundation",
            Stage::Scaffolding => "_scaffolding",
            Stage::Damaged => "_damaged",
            Stage::Heavy => "_heavily_damaged",
        }
    }
}

/// 0..=999 from a fixed integer hash (the same mixer as the shade's facet variation).
pub fn hash(i: u32) -> u32 {
    let mut x = i.wrapping_mul(0x9e37_79b9) ^ 0x5bd1_e995;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    x % 1000
}

// ---- The top-down share ----------------------------------------------------------------------

/// The share of the top-down view that is `team`, as `sf_asset_check`'s raster measures it (the
/// highest triangle per pixel), on a coarser grid of `res` pixels along the longer side.
pub fn team_share(tris: &[Tri], res: usize) -> f64 {
    if tris.is_empty() {
        return 0.0;
    }
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for t in tris {
        for p in &t.v {
            let q = [f64::from(p[0]), f64::from(p[2])];
            for k in 0..2 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
    }
    let longest = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    if longest <= 0.0 {
        return 0.0;
    }
    let pixel = longest / res as f64;
    let w = ((hi[0] - lo[0]) / pixel).ceil() as usize + 1;
    let d = ((hi[1] - lo[1]) / pixel).ceil() as usize + 1;
    let mut top = vec![f64::NEG_INFINITY; w * d];
    let mut team = vec![false; w * d];
    for t in tris {
        let c =
            t.v.map(|p| [f64::from(p[0]), f64::from(p[2]), f64::from(p[1])]);
        let (a, b, cc) = (c[0], c[1], c[2]);
        let area = (b[0] - a[0]) * (cc[1] - a[1]) - (b[1] - a[1]) * (cc[0] - a[0]);
        if area.abs() < 1e-9 {
            continue;
        }
        let x0 = a[0].min(b[0]).min(cc[0]);
        let x1 = a[0].max(b[0]).max(cc[0]);
        let z0 = a[1].min(b[1]).min(cc[1]);
        let z1 = a[1].max(b[1]).max(cc[1]);
        let px0 = ((x0 - lo[0]) / pixel - 0.5).floor().max(0.0) as usize;
        let px1 = (((x1 - lo[0]) / pixel - 0.5).ceil().max(0.0) as usize).min(w - 1);
        let pz0 = ((z0 - lo[1]) / pixel - 0.5).floor().max(0.0) as usize;
        let pz1 = (((z1 - lo[1]) / pixel - 0.5).ceil().max(0.0) as usize).min(d - 1);
        for pz in pz0..=pz1 {
            for px in px0..=px1 {
                let x = lo[0] + (px as f64 + 0.5) * pixel;
                let z = lo[1] + (pz as f64 + 0.5) * pixel;
                let w0 = ((b[0] - x) * (cc[1] - z) - (b[1] - z) * (cc[0] - x)) / area;
                let w1 = ((cc[0] - x) * (a[1] - z) - (cc[1] - z) * (a[0] - x)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < -1e-9 || w1 < -1e-9 || w2 < -1e-9 {
                    continue;
                }
                let y = w0 * a[2] + w1 * b[2] + w2 * cc[2];
                let at = pz * w + px;
                if y > top[at] {
                    top[at] = y;
                    team[at] = t.mat == TEAM;
                }
            }
        }
    }
    let covered = top.iter().filter(|y| **y > f64::NEG_INFINITY).count();
    let t = top
        .iter()
        .zip(&team)
        .filter(|(y, t)| **y > f64::NEG_INFINITY && **t)
        .count();
    if covered == 0 {
        0.0
    } else {
        t as f64 / covered as f64
    }
}

/// The resolution of [`fit_team`]'s raster (the checker uses 512; 160 is within a percent here).
pub const FIT_RES: usize = 400;

/// Add `cands` (team stripes) to `body` one by one, in order, until the team share of `fixed` +
/// `body` + the added ones reaches [`TEAM_AIM`] and the level has at least [`TEAM_MIN_TRIS`]
/// team triangles; the rest are left out. `fixed` is the rest of the level (a turret) in model
/// space.
///
/// # Panics
/// When every candidate is added and the share is still short (the recipe gives too few).
pub fn fit_team(body: &mut Mesh, fixed: &[Tri], cands: Vec<Mesh>, stem: &str) {
    let is_team = |t: &&Tri| t.mat == TEAM;
    let mut all: Vec<Tri> = fixed.to_vec();
    all.extend(body.tris.iter().copied());
    let mut count = all.iter().filter(is_team).count();
    let start = body.len();
    let mut cands = cands.into_iter();
    loop {
        let share = team_share(&all, FIT_RES);
        if share >= TEAM_AIM && count >= TEAM_MIN_TRIS {
            if std::env::var_os("SF_FIT_DEBUG").is_some() {
                eprintln!(
                    "FIT {stem}: body {start} -> {} share {share:.3}",
                    body.len()
                );
            }
            break;
        }
        let Some(c) = cands.next() else {
            if std::env::var_os("SF_FIT_SOFT").is_some() {
                eprintln!("SHORT {stem}: {share:.3} {count} body {}", body.len());
                break;
            }
            panic!("{stem}: team share {share:.3} with {count} triangles after every candidate");
        };
        count += c.tris.iter().filter(is_team).count();
        all.extend(c.tris.iter().copied());
        body.add(c);
    }
}

/// [`fit_team`] for a model with the player colour; a neutral model (its third slot is the fixed
/// `paint3`) gets none of the candidates.
pub fn fit(teamed: bool, body: &mut Mesh, fixed: &[Tri], cands: Vec<Mesh>, stem: &str) {
    if teamed {
        fit_team(body, fixed, cands, stem);
    }
}

/// LOD1 has at most this share (per mille) of LOD0's triangles after [`pad_model`] (the rule is
/// 400; a little under it).
pub const LOD1_MAX_PERMILLE: usize = 380;
/// LOD2 likewise (the rule is 150).
pub const LOD2_MAX_PERMILLE: usize = 140;

/// Give LOD0 of `m` real detail until it has at least `floor` triangles and the reduced levels
/// keep their share ([`LOD1_MAX_PERMILLE`], [`LOD2_MAX_PERMILLE`]): rows of bolts along the
/// edges of the platform `(hx, hz, y)` (anchor bolts on a slab, from the edge inwards), never past
/// `ceil`. A model whose levels already meet it is left alone.
pub fn pad_model(m: &mut Model, floor: usize, ceil: usize, pad: (i32, i32, i32)) {
    let t = m.triangles();
    let need = floor
        .max(t[1] * 1000 / LOD1_MAX_PERMILLE + 1)
        .max(t[2] * 1000 / LOD2_MAX_PERMILLE + 1)
        .min(ceil);
    let mut have = t[0];
    if have >= need {
        return;
    }
    let (hx, hz, y) = pad;
    let Some(body) = m.lods[0].children.iter_mut().find(|n| n.name == "body") else {
        return;
    };
    let mut row = 0;
    while have < need && row < 12 {
        let inset = 110 + row * 160;
        let (ax, az) = (hx - inset, hz - inset);
        if ax < 300 || az < 300 {
            break;
        }
        let step = 420;
        let mut spots = Vec::new();
        let mut x = -ax;
        while x <= ax {
            spots.push([x, -az]);
            spots.push([x, az]);
            x += step;
        }
        let mut z = -az + step;
        while z < az {
            spots.push([-ax, z]);
            spots.push([ax, z]);
            z += step;
        }
        for [x, z] in spots {
            if have >= need || have + 10 > ceil {
                break;
            }
            let b = crate::kit::bolt([x, y, z], 45, 30, 4, PAINT2);
            have += b.len();
            body.mesh.add(b);
        }
        row += 1;
    }
}

// ---- Geometry of the stages --------------------------------------------------------------------

/// A plane `n . p <= d` keeps what is below it.
#[derive(Clone, Copy, Debug)]
pub struct Plane {
    pub n: [i64; 3],
    pub d: i64,
}

impl Plane {
    /// The horizontal plane `y = h`.
    pub fn level(h: i32) -> Self {
        Plane {
            n: [0, 1, 0],
            d: i64::from(h),
        }
    }

    /// A slanted plane through `(0, h, 0)` rising `rx` mm per metre along X and `rz` along Z.
    pub fn slant(h: i32, rx: i64, rz: i64) -> Self {
        Plane {
            n: [-rx, 1000, -rz],
            d: i64::from(h) * 1000,
        }
    }

    fn side(&self, p: P) -> i64 {
        dot(self.n, [i64::from(p[0]), i64::from(p[1]), i64::from(p[2])]) as i64 - self.d
    }

    /// The height of the plane above `(x, z)`.
    pub fn height_at(&self, x: i32, z: i32) -> i32 {
        ((self.d - self.n[0] * i64::from(x) - self.n[2] * i64::from(z)) / self.n[1]) as i32
    }
}

/// Cut `mesh` with `plane`: what lies below stays, triangles across it are clipped (the cut
/// polygon fanned out again, every piece keeping the triangle's winding, material and tone).
pub fn clip(mesh: &Mesh, plane: Plane) -> Mesh {
    let mut out = Mesh::new();
    for t in &mesh.tris {
        let s = t.v.map(|p| plane.side(p));
        if s.iter().all(|&x| x <= 0) {
            out.tris.push(*t);
            continue;
        }
        if s.iter().all(|&x| x > 0) {
            continue;
        }
        let mut poly: Vec<P> = Vec::new();
        for i in 0..3 {
            let j = (i + 1) % 3;
            let (a, b) = (t.v[i], t.v[j]);
            if s[i] <= 0 {
                poly.push(a);
            }
            if (s[i] <= 0) != (s[j] <= 0) {
                let den = i128::from(s[i] - s[j]);
                let num = i128::from(s[i]);
                let at = |k: usize| -> i32 {
                    (i128::from(a[k]) + (i128::from(b[k]) - i128::from(a[k])) * num / den) as i32
                };
                poly.push([at(0), at(1), at(2)]);
            }
        }
        let n0 = t.normal();
        for k in 1..poly.len().saturating_sub(1) {
            let mut piece = *t;
            piece.v = [poly[0], poly[k], poly[k + 1]];
            let n = piece.normal();
            if dot(n, n) >= 1_000_000 && dot(n, n0) > 0 {
                out.tris.push(piece);
            }
        }
    }
    out
}

/// The bounds of `tris` (`lo`, `hi`).
pub fn bounds(tris: &[Tri]) -> (P, P) {
    let mut lo = [i32::MAX; 3];
    let mut hi = [i32::MIN; 3];
    for t in tris {
        for p in &t.v {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
    }
    (lo, hi)
}

/// The sides of a rubble [`chunk`].
pub const CHUNK_SIDES: u32 = 8;

/// A rough chunk of masonry or plate: an irregular block of [`CHUNK_SIDES`] sides, narrower on
/// top, at `at` (on its base), `r` across and `h` high; `seed` fixes its shape. 8 x 2 + 6 = 22
/// triangles; at LOD2 a plain block of 10.
pub fn chunk(at: P, r: i32, h: i32, seed: u32, mat: Mat, lod: Lod) -> Mesh {
    if !lod.has(1) {
        let q = r * 8 / 10;
        return cuboid([-q, 0, -q], [q, h, q], mat).moved(at);
    }
    let ring = |scale: i32, salt: u32| -> Vec<[i32; 2]> {
        (0..CHUNK_SIDES)
            .map(|k| {
                let j = 750 + hash(seed.wrapping_mul(31) ^ (k * 7 + salt)) / 4; // 750..999
                let rr = r * scale / 1000 * j as i32 / 1000;
                crate::kit::on_circle(rr, k as i32, CHUNK_SIDES)
            })
            .collect()
    };
    let lean = (hash(seed ^ 0x77) as i32 - 500) * r / 2500;
    let top: Vec<[i32; 2]> = ring(560, 3).iter().map(|p| [p[0] + lean, p[1]]).collect();
    loft(&[(ring(1000, 1), 0), (top, h)], &[mat], Some(mat), None).moved(at)
}

/// A bent plate lying askew on the ground, `len` long along X, tilted by `rise` over its length:
/// a slanted slab (12 triangles), turned by `q` quarters.
pub fn fallen_plate(at: P, len: i32, w: i32, rise: i32, q: u8, mat: Mat) -> Mesh {
    let t = 60;
    extrude_z1(
        &[[0, 0], [len, rise], [len, rise + t], [0, t]],
        -w / 2,
        w / 2,
        mat,
    )
    .turned_y(q)
    .moved(at)
}

/// Debris round a footprint: `n` chunks spread on a ring between the inner box `(hx, hz)` and
/// the outer box `(ox, oz)`, sizes from `r` down; fixed by `seed`.
pub fn debris(
    n: u32,
    hx: i32,
    hz: i32,
    ox: i32,
    oz: i32,
    r: i32,
    seed: u32,
    mats: &[Mat],
    lod: Lod,
) -> Mesh {
    let mut m = Mesh::new();
    for i in 0..n {
        let h = hash(seed * 1013 + i);
        let side = i % 4;
        let along = |half: i32| (h as i32 * 2 - 1000) * half / 1000;
        let gap = |inner: i32, outer: i32| {
            inner + (hash(seed * 7 + i * 13) as i32) * (outer - r - inner).max(1) / 1000
        };
        let (x, z) = match side {
            0 => (gap(hx, ox), along(oz - r)),
            1 => (-gap(hx, ox), along(oz - r)),
            2 => (along(ox - r), gap(hz, oz)),
            _ => (along(ox - r), -gap(hz, oz)),
        };
        let rr = r * (500 + hash(seed + i * 3) as i32 / 2) / 1000;
        let mat = mats[(i as usize) % mats.len()];
        m.add(chunk([x, 0, z], rr, rr * 3 / 4, seed + i, mat, lod).toned(RECESS_DEBRIS));
    }
    m
}

/// The tone of debris (dusty, a little darker than the plain surface).
pub const RECESS_DEBRIS: crate::kit::Tone = 780;

/// Darken (tone) the triangles of `mesh` whose centroid lies within `r` of `c`; team triangles
/// there lose their paint (become `burnt`, sooted).
pub fn soot(mesh: &mut Mesh, c: P, r: i32, tone: crate::kit::Tone, burnt: Mat) {
    let r2 = i64::from(r) * i64::from(r);
    for t in &mut mesh.tris {
        let d = sub(centroid(t), c);
        if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= r2 {
            if t.mat == TEAM {
                t.mat = burnt;
            }
            t.tone = t.tone.min(tone);
        }
    }
}

/// Remove the triangles whose centroid lies within `r` of `c` (a hole).
pub fn hole(mesh: &mut Mesh, c: P, r: i32) {
    let r2 = i64::from(r) * i64::from(r);
    mesh.tris.retain(|t| {
        let d = sub(centroid(t), c);
        d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > r2
    });
}

fn centroid(t: &Tri) -> P {
    [0, 1, 2]
        .map(|k| ((i64::from(t.v[0][k]) + i64::from(t.v[1][k]) + i64::from(t.v[2][k])) / 3) as i32)
}

/// Whether the triangle faces up (for the share and the hole picks).
pub fn faces_up(t: &Tri) -> bool {
    let n = t.normal();
    n[1] > 0 && n[1] * n[1] * 2 > n[0] * n[0] + n[2] * n[2]
}

// ---- The node tree of a level ---------------------------------------------------------------

/// A level taken apart: the body's triangles in model space (every node except the turret
/// subtree and the anchors), the turret subtree with its model-space origin, and the anchors.
pub struct Parts {
    pub body: Mesh,
    pub turret: Option<(Node, P)>,
    pub anchors: Vec<(String, P)>,
}

/// Take a level root apart (see [`Parts`]).
pub fn split(root: &Node) -> Parts {
    let mut parts = Parts {
        body: Mesh::new(),
        turret: None,
        anchors: Vec::new(),
    };
    fn walk(n: &Node, o: P, parts: &mut Parts) {
        let at = [o[0] + n.at[0], o[1] + n.at[1], o[2] + n.at[2]];
        if n.name == "turret" {
            parts.turret = Some((n.clone(), at));
            return;
        }
        if n.name.starts_with("fx_") {
            parts.anchors.push((n.name.clone(), at));
            return;
        }
        parts.body.add(n.mesh.clone().moved(at));
        for c in &n.children {
            walk(c, at, parts);
        }
    }
    for c in &root.children {
        walk(c, root.at, &mut parts);
    }
    parts
}

/// Put a level together again: `body` at the origin, the turret (if any) at its model-space
/// origin, the anchors under the body.
pub fn join(body: Mesh, turret: Option<(Node, P)>, anchors: &[(String, P)]) -> Vec<Node> {
    let mut b = Node::part("body", [0, 0, 0], body);
    if let Some((mut t, at)) = turret {
        t.at = at;
        b = b.child(t);
    }
    for (name, at) in anchors {
        b = b.child(Node::empty(name, *at));
    }
    vec![b]
}

/// The world triangles of a turret subtree placed at `at`.
fn turret_tris(t: &Option<(Node, P)>) -> Vec<Tri> {
    match t {
        Some((n, at)) => {
            let mut n = n.clone();
            n.at = *at;
            n.world_tris()
        }
        None => Vec::new(),
    }
}

/// What a stage needs to know of the finished structure: the platform (its half sizes and top),
/// measured on LOD0 of the finished model.
#[derive(Clone, Copy, Debug)]
pub struct Footprint {
    /// Half sizes of the whole model (X, Z).
    pub hx: i32,
    pub hz: i32,
    /// The top of the finished model.
    pub top: i32,
    /// The middle of the footprint (X, Z) (a model may sit a little off the origin).
    pub cx: i32,
    pub cz: i32,
}

/// The footprint of a finished model, from its LOD0.
pub fn footprint(m: &Model) -> Footprint {
    let (lo, hi) = bounds(&m.lods[0].world_tris());
    Footprint {
        hx: (hi[0] - lo[0]) / 2,
        hz: (hi[2] - lo[2]) / 2,
        top: hi[1],
        cx: (hi[0] + lo[0]) / 2,
        cz: (hi[2] + lo[2]) / 2,
    }
}

/// Build the stage of a finished model. `slab` is the height of its platform (the scaffolding and
/// the debris stand on it). `alt` is the paint team candidates fall back to.
pub fn derive(built: &Model, stage: Stage, slab: i32) -> Model {
    derive_as(built, stage, slab, stage.suffix(), 500)
}

/// [`derive`] with its own file suffix and, for [`Stage::Heavy`], the height of the break in
/// thousandths of the body (a mission object's `_rubble`, `_burnt_out` or `_destroyed` state is a
/// heavy stage broken off lower).
pub fn derive_as(built: &Model, stage: Stage, slab: i32, suffix: &str, cut: i32) -> Model {
    let f = footprint(built);
    let stem = format!("{}{}", built.stem, suffix);
    let teamed = built.team().is_some();
    let seed = built
        .stem
        .bytes()
        .fold(7u32, |h, b| h.wrapping_mul(31) ^ u32::from(b))
        % 100_000; // small, so the recipes may multiply it without overflow
    let lods: Vec<Node> = Lod::ALL
        .iter()
        .map(|&lod| {
            // A stage's LOD1 starts from the finished model's far level too (WP11.6c review: so its
            // reduced levels keep their share without padding LOD0).
            let source = if lod.has(0) { 0 } else { 2 };
            let mut parts = split(&built.lods[source]);
            strip_filler(&mut parts.body, slab);
            let children = match stage {
                Stage::Foundation => foundation(&f, slab, &parts, lod, seed, &stem, teamed),
                Stage::Scaffolding => scaffolding(&f, slab, parts, lod, seed, &stem, teamed),
                Stage::Damaged => damaged(&f, slab, parts, lod, seed, &stem, teamed),
                Stage::Heavy => heavy(&f, slab, parts, lod, seed, &stem, teamed, cut),
            };
            Node {
                name: format!("{stem}_LOD{}", lod.0),
                at: [0, 0, 0],
                mesh: Mesh::new(),
                children,
            }
        })
        .collect();
    let mut m = Model {
        stem,
        materials: built.materials.clone(),
        lods,
    };
    // A stage keeps only what its derived geometry gives: no detail floor. Only where a reduced
    // level would exceed its share of LOD0 does LOD0 get anchor bolts, never past the stage's
    // ceiling (a share of the finished model's LOD0, docs/art-style.md §11).
    let k = Stage::ALL
        .iter()
        .position(|s| *s == stage)
        .expect("a stage");
    let ceil = built.triangles()[0] * STAGE_CEIL_PERMILLE[k] / 1000;
    pad_model(&mut m, 0, ceil, (f.hx - 60, f.hz - 60, slab));
    m
}

/// The most LOD0 triangles of a stage, in thousandths of its finished model's LOD0: foundation,
/// scaffolding, damaged, heavily damaged (WP11.6c review: a stage is never richer than the
/// finished structure).
pub const STAGE_CEIL_PERMILLE: [usize; 4] = [300, 700, 900, 700];

/// How far round a damage hole the small detail is gone, in thousandths of the hole's radius.
pub const DETAIL_GONE_PERMILLE: i32 = 8000;

/// Remove the small detail of `m` (toned triangles: panel lines, bolts, recesses, and the glass of
/// the accent slot when it is toned or a window), everywhere or within `r` of `c`. Team triangles
/// stay.
fn strip_detail(m: &mut Mesh, near: Option<(P, i32)>) {
    m.tris.retain(|t| {
        let detail = t.mat != TEAM && t.tone < crate::kit::PLAIN;
        let inside = near.is_none_or(|(c, r)| {
            let d = sub(centroid(t), c);
            d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= i64::from(r) * i64::from(r)
        });
        !(detail && inside)
    });
}

/// Take the anchor-bolt filler of [`pad_model`] (slab-coloured bolt heads standing on the
/// platform top `slab`) out of a finished body before a stage is derived from it: a stage does not
/// inherit the finished model's padding.
fn strip_filler(body: &mut Mesh, slab: i32) {
    body.tris.retain(|t| {
        let filler = t.mat == PAINT2
            && t.tone == crate::kit::RECESS
            && t.v.iter().all(|p| p[1] >= slab && p[1] <= slab + 30);
        !filler
    });
}

/// Anchors moved onto the slab (a stage under construction smokes from the ground).
fn grounded(anchors: &[(String, P)], y: i32) -> Vec<(String, P)> {
    anchors
        .iter()
        .map(|(n, p)| (n.clone(), [p[0], p[1].min(y), p[2]]))
        .collect()
}

/// The foundation: a slab, footing beams, rebar, formwork, stakes.
fn foundation(
    f: &Footprint,
    slab: i32,
    parts: &Parts,
    lod: Lod,
    seed: u32,
    stem: &str,
    teamed: bool,
) -> Vec<Node> {
    let (hx, hz) = (f.hx - 100, f.hz - 100);
    let y = slab.clamp(200, 450);
    let mut m = bevel_box([-hx, 0, -hz], [hx, y, hz], lod.bevel(60), PAINT2, PAINT2);
    // Formwork boards round the slab, standing a little proud.
    let b = 90;
    for s in [-1, 1] {
        m.add(
            cuboid(
                [-hx - b, 0, s * hz - b],
                [hx + b, y + 120, s * hz + b],
                PAINT,
            )
            .toned(860),
        );
        m.add(
            cuboid(
                [s * hx - b, 0, -hz + b],
                [s * hx + b, y + 120, hz - b],
                PAINT,
            )
            .toned(860),
        );
    }
    // Footing beams in a grid (300 mm wide, team candidates), rebar stubs on the crossings.
    let step = 2400;
    let nx = ((2 * hx) / step).max(1);
    let nz = ((2 * hz) / step).max(1);
    let mut cands = Vec::new();
    let beam_h = 18;
    for i in 1..nx.max(2) {
        let x = -hx + 2 * hx * i / nx.max(2);
        cands.push(strip(
            [x - 150, y, -hz + 300],
            [x + 150, y + beam_h, hz - 300],
            lod,
        ));
    }
    for k in 1..nz.max(2) {
        let z = -hz + 2 * hz * k / nz.max(2);
        cands.push(strip(
            [-hx + 300, y, z - 150],
            [hx - 300, y + beam_h, z + 150],
            lod,
        ));
    }
    // Survey stripes along the slab edge (inside the boards).
    for s in [-1, 1] {
        cands.push(strip(
            [-hx + 200, y, s * (hz - 330) - 130],
            [hx - 200, y + 18, s * (hz - 330) + 130],
            lod,
        ));
        cands.push(strip(
            [s * (hx - 330) - 130, y, -hz + 520],
            [s * (hx - 330) + 130, y + 18, hz - 520],
            lod,
        ));
    }
    // Interleave: beams and edge stripes alternate so the markings spread evenly.
    let cands = interleave(cands);
    if lod.has(0) {
        for i in 0..=nx.max(2) {
            for k in 0..=nz.max(2) {
                let x = -hx + 400 + (2 * hx - 800) * i / nx.max(2);
                let z = -hz + 400 + (2 * hz - 800) * k / nz.max(2);
                for (dx, dz) in [(0, 0)].into_iter().filter(|_| (i + k) % 2 == 0) {
                    m.add(cuboid(
                        [x + dx - 20, y, z + dz - 20],
                        [x + dx + 20, y + 620, z + dz + 20],
                        METAL,
                    ));
                }
            }
        }
        // Pallets of blocks and a stack of boards.
        for (i, (px, pz)) in [(hx - 1300, hz - 1300), (-hx + 1300, -hz + 1300)]
            .iter()
            .enumerate()
        {
            m.add(
                closed_box(
                    [px - 600, y, pz - 500],
                    [px + 600, y + 120, pz + 500],
                    RUBBER,
                )
                .toned(900),
            );
            for k in 0..3 {
                let x0 = px - 550 + k * 380;
                m.add(
                    bevel_box(
                        [x0, y + 120, pz - 450],
                        [x0 + 340, y + 520, pz + 450],
                        20,
                        PAINT,
                        PAINT,
                    )
                    .toned(900 - 40 * i as u16),
                );
            }
        }
    }
    {
        // Survey stakes on the corners.
        for (sx, sz) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
            let (x, z) = (sx * (hx + 20), sz * (hz + 20));
            m.add(cuboid(
                [x - 40, 0, z - 40],
                [x + 40, y + 900, z + 40],
                METAL,
            ));
        }
    }
    let fixed = turret_tris(&None);
    fit(teamed, &mut m, &fixed, cands, stem);
    let _ = (seed, parts.turret.is_some());
    join(m, None, &grounded(&parts.anchors, y + 200))
}

/// Order candidates from both ends towards the middle (spread markings).
fn interleave(v: Vec<Mesh>) -> Vec<Mesh> {
    let mut out = Vec::with_capacity(v.len());
    let (mut i, mut j) = (0usize, v.len());
    while i < j {
        out.push(v[i].clone());
        i += 1;
        if i < j {
            j -= 1;
            out.push(v[j].clone());
        }
    }
    out
}

/// The body of `parts` without the platform: its bounds above `slab + 100`.
fn hull_bounds(parts: &Parts, slab: i32) -> (P, P) {
    let above: Vec<Tri> = parts
        .body
        .tris
        .iter()
        .filter(|t| t.v.iter().all(|p| p[1] > slab + 100))
        .copied()
        .collect();
    if above.is_empty() {
        bounds(&parts.body.tris)
    } else {
        bounds(&above)
    }
}

/// The roof level of a body: the height that half of its top-down area (above the platform) lies
/// under, weighted by each upward face's area seen from above. A crane or an antenna sets the
/// bounds, not the roof.
pub fn roof_level(parts: &Parts, slab: i32) -> i32 {
    let mut faces: Vec<(i32, i64)> = parts
        .body
        .tris
        .iter()
        .filter(|t| t.v.iter().all(|p| p[1] > slab + 100))
        .filter_map(|t| {
            let n = t.normal();
            (n[1] > 0).then(|| (centroid(t)[1], n[1]))
        })
        .collect();
    if faces.is_empty() {
        return bounds(&parts.body.tris).1[1];
    }
    faces.sort();
    let total: i64 = faces.iter().map(|f| f.1).sum();
    let mut acc = 0;
    for (y, a) in &faces {
        acc += a;
        if acc * 2 >= total {
            return *y;
        }
    }
    faces[faces.len() - 1].0
}

/// The scaffolding: the finished body cut at about half its height, a deck, a scaffold round it.
fn scaffolding(
    f: &Footprint,
    slab: i32,
    parts: Parts,
    lod: Lod,
    seed: u32,
    stem: &str,
    teamed: bool,
) -> Vec<Node> {
    let cut = slab + (roof_level(&parts, slab) - slab) * 60 / 100;
    let _ = f.top;
    let mut m = clip(&parts.body, Plane::level(cut));
    // Bare walls: the facade detail (panel lines, bolts, louvres, glass) comes last.
    strip_detail(&mut m, None);
    // Nor are the fittings in yet (pipes, railings, ladders: the steel slot).
    m.tris.retain(|t| t.mat != METAL);
    // The team of the finished model under the cut stays; its roof bands are gone. The paint of
    // the cut walls is bare: soot-free but a little darker (primer).
    let (lo, hi) = hull_bounds(&parts, slab);
    let (x0, x1, z0, z1) = (lo[0], hi[0], lo[2], hi[2]);
    let (ix, iz) = ((x1 - x0) * 12 / 100, (z1 - z0) * 12 / 100);
    m.add(
        cuboid(
            [x0 + ix, cut - 160, z0 + iz],
            [x1 - ix, cut - 100, z1 - iz],
            PAINT,
        )
        .toned(820),
    );
    // Setting-out stripes on the deck (team candidates, added after the scaffold's own).
    let mut deck = Vec::new();
    let mut z = z0 + iz + 300;
    while z + 260 < z1 - iz - 200 {
        deck.push(strip(
            [x0 + ix + 200, cut - 100, z],
            [x1 - ix - 200, cut - 84, z + 260],
            lod,
        ));
        z += 900;
    }
    // Scaffold: poles round the hull bounds, 500 mm out, inside the footprint.
    let (px0, px1) = (
        (x0 - 500).max(-f.hx + f.cx + 150),
        (x1 + 500).min(f.hx + f.cx - 150),
    );
    let (pz0, pz1) = (
        (z0 - 500).max(-f.hz + f.cz + 150),
        (z1 + 500).min(f.hz + f.cz - 150),
    );
    let top = cut + 1100;
    let spacing = if lod.has(0) {
        3400
    } else if lod.has(1) {
        5000
    } else {
        1_000_000
    };
    let mut poles: Vec<(i32, i32)> = Vec::new();
    let nx = ((px1 - px0) / spacing).max(1);
    let nz = ((pz1 - pz0) / spacing).max(1);
    for i in 0..=nx {
        let x = px0 + (px1 - px0) * i / nx;
        poles.push((x, pz0));
        poles.push((x, pz1));
    }
    for k in 1..nz {
        let z = pz0 + (pz1 - pz0) * k / nz;
        poles.push((px0, z));
        poles.push((px1, z));
    }
    for (x, z) in poles.iter().filter(|_| lod.has(1)) {
        m.add(cuboid([x - 30, slab, z - 30], [x + 30, top, z + 30], METAL));
    }
    // Ledgers and planks on two lifts; the toe boards are the team candidates.
    let lifts: Vec<i32> = if lod.has(0) {
        vec![slab + (cut - slab) / 2, cut]
    } else if lod.has(1) {
        vec![cut]
    } else {
        Vec::new()
    };
    let mut cands = Vec::new();
    for &ly in &lifts {
        for (a0, a1, z, out) in [(px0, px1, pz0, -1), (px0, px1, pz1, 1)] {
            m.add(cuboid([a0, ly - 60, z - 30], [a1, ly, z + 30], METAL));
            let zi = z - out * 380;
            let (za, zb) = (z.min(zi), z.max(zi));
            m.add(cuboid([a0 + 40, ly, za], [a1 - 40, ly + 50, zb], PAINT).toned(840));
            let (ta, tb) = (z - out * 260, z - out * 40);
            cands.push(strip(
                [a0 + 40, ly + 50, ta.min(tb)],
                [a1 - 40, ly + 70, ta.max(tb)],
                lod,
            ));
        }
        for (b0, b1, x, out) in [(pz0, pz1, px0, -1), (pz0, pz1, px1, 1)] {
            m.add(cuboid([x - 30, ly - 60, b0], [x + 30, ly, b1], METAL));
            let xi = x - out * 380;
            let (xa, xb) = (x.min(xi), x.max(xi));
            m.add(cuboid([xa, ly, b0 + 420], [xb, ly + 50, b1 - 420], PAINT).toned(840));
            let (ta, tb) = (x - out * 260, x - out * 40);
            cands.push(strip(
                [ta.min(tb), ly + 50, b0 + 420],
                [ta.max(tb), ly + 70, b1 - 420],
                lod,
            ));
        }
    }
    // The crane on the +X+Z corner (all levels: it sets the height).
    let (cx, cz) = (px1 - 300, pz1 - 300);
    let mast_top = f.top + 1500;
    let w = if lod.has(1) { 220 } else { 260 };
    m.add(boxy(
        lod,
        [cx - w, slab, cz - w],
        [cx + w, mast_top, cz + w],
        PAINT2,
    ));
    let jib = (px1 - px0) * 6 / 10;
    if lod.has(1) {
        m.add(boxy(
            lod,
            [cx - jib, mast_top - 300, cz - 110],
            [cx + 250, mast_top, cz + 110],
            PAINT,
        ));
    }
    if lod.has(1) {
        cands.push(cuboid(
            [cx - jib + 200, mast_top, cz - 110],
            [cx - 200, mast_top + 20, cz + 110],
            TEAM,
        ));
    }
    if lod.has(1) {
        m.add(closed_box(
            [cx - 700, mast_top - 900, cz - 300],
            [cx - 260, mast_top - 300, cz + 300],
            METAL,
        ));
        m.add(cuboid(
            [cx - jib + 400, mast_top - 2300, cz - 10],
            [cx - jib + 420, mast_top - 300, cz + 10],
            METAL,
        ));
    }
    if lod.has(0) {
        for k in 0..((mast_top - slab) / 1200) {
            let y = slab + k * 600;
            m.add(cuboid(
                [cx - w - 20, y, cz - w - 20],
                [cx + w + 20, y + 40, cz + w + 20],
                METAL,
            ));
        }
    }
    let mut cands = interleave(cands);
    cands.extend(edge_cands(f, slab, lod));
    cands.extend(interleave(deck));
    let fixed = turret_tris(&None);
    fit(teamed, &mut m, &fixed, cands, stem);
    let _ = seed;
    join(m, None, &grounded(&parts.anchors, cut))
}

/// A closed box, at LOD2 open at the bottom (two triangles fewer; the far view never sees it).
fn boxy(lod: Lod, lo: P, hi: P, mat: Mat) -> Mesh {
    if lod.has(1) {
        closed_box(lo, hi, mat)
    } else {
        cuboid(lo, hi, mat)
    }
}

/// The hole of a damage stage: on the roof, towards a corner fixed by `seed`.
fn hole_at(parts: &Parts, slab: i32, seed: u32, k: u32) -> P {
    let (lo, hi) = hull_bounds(parts, slab);
    let fx = 250 + hash(seed ^ (k * 101)) as i32 / 2; // 250..749 of the way across
    let fz = 250 + hash(seed ^ (k * 211 + 5)) as i32 / 2;
    let x = lo[0] + (hi[0] - lo[0]) * fx / 1000;
    let z = lo[2] + (hi[2] - lo[2]) * fz / 1000;
    // The highest upward face near (x, z) sets the height, up to a metre over the roof level (an
    // antenna or a crane arm is no roof).
    let roof = roof_level(parts, slab) + 1000;
    let y = parts
        .body
        .tris
        .iter()
        .filter(|t| faces_up(t))
        .map(centroid)
        .filter(|c| (c[0] - x).abs() < 1200 && (c[2] - z).abs() < 1200 && c[1] <= roof)
        .max_by_key(|c| c[1]);
    // The hole is centred on that face (so it always takes something); without one, on the roof.
    y.unwrap_or([x, roof - 1000, z])
}

/// Team candidates on the platform of every stage: narrow stripes (260 mm) in three rings along
/// the platform edge, two pieces per edge and ring, inside the footprint, on `y`.
fn edge_cands(f: &Footprint, y: i32, lod: Lod) -> Vec<Mesh> {
    let mut c = Vec::new();
    for inset in (0..12).map(|k| 300 + k * 550) {
        let (ax, az) = (f.hx - inset, f.hz - inset);
        if ax < 600 || az < 600 {
            continue;
        }
        for s in [-1, 1] {
            let z = s * az - if s > 0 { 260 } else { 0 };
            let x = s * ax - if s > 0 { 260 } else { 0 };
            for (a, b) in [(-ax + 300, -150), (150, ax - 300)] {
                c.push(strip([a, y, z], [b, y + 16, z + 260], lod));
            }
            for (a, b) in [(-az + 300, -150), (150, az - 300)] {
                c.push(strip([x, y, a], [x + 260, y + 16, b], lod));
            }
        }
    }
    c.into_iter().map(|m| m.moved([f.cx, 0, f.cz])).collect()
}

/// A team stripe: its top only, 16-20 mm over the surface (walls that thin are never seen and cost
/// eight triangles each; WP11.6c review). At LOD2, the far view, a stripe is [`FAR_STRIPE_MM`] wide (the narrow-band
/// rule holds for LOD0 and LOD1), so fewer of them reach the share on its 300 triangles.
fn strip(lo: P, hi: P, lod: Lod) -> Mesh {
    let (mut lo, mut hi) = (lo, hi);
    if !lod.has(1) {
        let k = if hi[0] - lo[0] < hi[2] - lo[2] { 0 } else { 2 };
        let mid = (lo[k] + hi[k]) / 2;
        // Never wider than a tenth of its length (a short stripe on a small slab stays narrow).
        let len = (hi[2 - k] - lo[2 - k]).abs();
        let w = FAR_STRIPE_MM.min(len / 10).max(hi[k] - lo[k]);
        lo[k] = mid - w / 2;
        hi[k] = mid + w / 2;
    }
    cuboid([lo[0], hi[1], lo[2]], hi, TEAM)
}

/// The width of a stage's team stripe at LOD2.
pub const FAR_STRIPE_MM: i32 = 600;

/// Debris and the team candidates among it: fallen band pieces (narrow strips lying on the ground).
fn debris_cands(
    f: &Footprint,
    slab: i32,
    n: u32,
    seed: u32,
    lod: Lod,
    hull: (P, P),
) -> (Mesh, Vec<Mesh>) {
    // Debris lies on the free platform round the hull, never on the body (a small structure that
    // fills its footprint gets none).
    let (lo, hi) = hull;
    let hx = (f.hx * 70 / 100).max((hi[0] - lo[0]) / 2 + 200);
    let hz = (f.hz * 70 / 100).max((hi[2] - lo[2]) / 2 + 200);
    let (ox, oz) = (f.hx - 200, f.hz - 200);
    let n = if hx > ox - 400 && hz > oz - 400 { 0 } else { n };
    let mut m = debris(
        n,
        hx.min(ox - 400),
        hz.min(oz - 400),
        ox,
        oz,
        700,
        seed,
        &[PAINT, PAINT2, METAL],
        lod,
    );
    m = m.moved([f.cx, slab, f.cz]);
    let mut cands = Vec::new();
    // Fallen band pieces on the platform edge: short narrow strips along the edges.
    let pieces = if lod.has(1) { 20 } else { 10 };
    for i in 0..pieces {
        let h = hash(seed * 3 + i);
        let len = 900 + h as i32;
        let along_x = i % 2 == 0;
        let s = if i % 4 < 2 { 1 } else { -1 };
        let (x, z) = if along_x {
            ((h as i32 - 500) * (f.hx - len) / 600, s * (f.hz - 420))
        } else {
            (s * (f.hx - 420), (h as i32 - 500) * (f.hz - len) / 600)
        };
        let (x, z) = (
            x.clamp(-f.hx + len / 2 + 50, f.hx - len / 2 - 50),
            z.clamp(-f.hz + len / 2 + 50, f.hz - len / 2 - 50),
        );
        let lo = if along_x {
            [x - len / 2, slab, z - 140]
        } else {
            [x - 140, slab, z - len / 2]
        };
        let hi = if along_x {
            [x + len / 2, slab + 20, z + 140]
        } else {
            [x + 140, slab + 20, z + len / 2]
        };
        cands.push(cuboid([lo[0], hi[1], lo[2]], hi, TEAM).moved([f.cx, 0, f.cz]));
    }
    (m, cands)
}

/// Damaged: one hole in the roof, soot round it, debris.
fn damaged(
    f: &Footprint,
    slab: i32,
    parts: Parts,
    lod: Lod,
    seed: u32,
    stem: &str,
    teamed: bool,
) -> Vec<Node> {
    let r = (f.hx.min(f.hz) * 22 / 100).clamp(500, 1800);
    // Of four places on the roof, the one that takes the least team colour with it (a hole in a
    // band would leave too little of it on a small structure).
    let team_near = |c: P| {
        let r2 = i64::from(r * 3 / 2).pow(2);
        parts
            .body
            .tris
            .iter()
            .filter(|t| t.mat == TEAM)
            .filter(|t| {
                let d = sub(centroid(t), c);
                d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= r2
            })
            .count()
    };
    let c = (1..=4)
        .map(|k| hole_at(&parts, slab, seed, k))
        .min_by_key(|c| {
            // A place where the hole takes nothing is no hole at all: last choice.
            let r2 = i64::from(r) * i64::from(r);
            let takes = parts.body.tris.iter().any(|t| {
                let d = sub(centroid(t), *c);
                d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= r2
            });
            (!takes, team_near(*c))
        })
        .expect("four places");
    let mut m = parts.body.clone();
    hole(&mut m, c, r);
    // The blast took the small detail off the walls round the hole.
    strip_detail(&mut m, Some((c, r * DETAIL_GONE_PERMILLE / 1000)));
    soot(&mut m, c, r * 2, SOOT, TEAM);
    soot(&mut m, c, r * 15 / 10, CHAR, TEAM);
    soot(&mut m, c, r * 11 / 10, CHAR, PAINT2);
    // The burnt rim: chunks round the hole on the roof.
    // On a structure without a platform (a bridge deck) the chunks lie in the hole itself.
    let rim = if slab == 0 || f.hx.min(f.hz) < 3000 {
        // A small structure: two at LOD0, one at LOD1 (so LOD1 stays richer than LOD2).
        u32::from(lod.has(1)) + u32::from(lod.has(0))
    } else if lod.has(0) {
        4
    } else if lod.has(1) {
        2
    } else {
        0
    };
    for i in 0..rim {
        let [x, z] = crate::kit::on_circle(if slab == 0 { r / 4 } else { r }, i as i32, 8);
        m.add(
            chunk(
                [c[0] + x, c[1] - 120, c[2] + z],
                320,
                260,
                seed + i,
                PAINT2,
                lod,
            )
            .toned(CHAR),
        );
    }
    let n = if slab == 0 {
        0 // no platform to lie on (a bridge deck)
    } else if lod.has(0) {
        6
    } else if lod.has(1) {
        2
    } else {
        0
    };
    let (d, mut cands) = debris_cands(f, slab, n, seed, lod, hull_bounds(&parts, slab));
    cands.extend(edge_cands(f, slab, lod));
    m.add(d);
    let fixed = turret_tris(&parts.turret);
    fit(teamed, &mut m, &fixed, cands, stem);
    join(m, parts.turret, &parts.anchors)
}

/// Heavily damaged: broken off along a slanted plane, rubble inside, two holes, soot, debris.
fn heavy(
    f: &Footprint,
    slab: i32,
    mut parts: Parts,
    lod: Lod,
    seed: u32,
    stem: &str,
    teamed: bool,
    cut: i32,
) -> Vec<Node> {
    let (lo, hi) = hull_bounds(&parts, slab);
    let h = roof_level(&parts, slab) - slab;
    let tilt = if hash(seed).is_multiple_of(2) { 1 } else { -1 };
    let rise = (i64::from(h) * 300 / i64::from((hi[0] - lo[0]).max(1000))).min(250) * tilt;
    let plane = Plane::slant(
        slab + h * cut / 1000,
        rise,
        (i64::from(h) * 120 / i64::from((hi[2] - lo[2]).max(1000))).min(120),
    );
    let c1 = hole_at(&parts, slab, seed, 2);
    let mut m = clip(&parts.body, plane);
    // Burnt out: no small detail survives.
    strip_detail(&mut m, None);
    let r = (f.hx.min(f.hz) * 20 / 100).clamp(450, 1600);
    hole(&mut m, [c1[0], plane.height_at(c1[0], c1[2]), c1[2]], r);
    // Soot over the break, deep at the top.
    let mid = [(lo[0] + hi[0]) / 2, 0, (lo[2] + hi[2]) / 2];
    for t in &mut m.tris {
        let y = t.v.iter().map(|p| p[1]).max().unwrap_or(0);
        let edge = plane.height_at(t.v[0][0], t.v[0][2]);
        if y > edge - 900 {
            if t.mat == TEAM && y > edge - 250 {
                t.mat = PAINT2;
            }
            t.tone = t.tone.min(if y > edge - 400 { CHAR } else { SOOT });
        }
    }
    // Rubble inside the shell: a heap of chunks up to just under the break.
    let mut tops = Vec::new();
    let (ix, iz) = (
        (hi[0] - lo[0]) / 2 * 70 / 100,
        (hi[2] - lo[2]) / 2 * 70 / 100,
    );
    let grid = if lod.has(0) && ix.min(iz) >= 1500 {
        3
    } else if lod.has(1) {
        2
    } else {
        i32::from(cut >= 300) // a low heap has no room for one at the far level
    };
    let floor = slab.max(0);
    m.add(
        cuboid(
            [mid[0] - ix, floor, mid[2] - iz],
            [mid[0] + ix, floor + 8, mid[2] + iz],
            RUBBER,
        )
        .toned(CHAR),
    );
    for i in 0..grid {
        for k in 0..grid {
            let cell = (i * grid + k) as u32;
            let jx = (hash(seed ^ (cell * 17 + 1)) as i32 - 500) * ix / (3 * grid * 500);
            let jz = (hash(seed ^ (cell * 17 + 2)) as i32 - 500) * iz / (3 * grid * 500);
            let x = mid[0] - ix + (2 * ix) * (2 * i + 1) / (2 * grid) + jx;
            let z = mid[2] - iz + (2 * iz) * (2 * k + 1) / (2 * grid) + jz;
            let hh = ((plane.height_at(x, z) - floor)
                * (25 + hash(seed + (i * 7 + k) as u32) as i32 / 30)
                / 100)
                .max(300);
            let rr = (ix.min(iz) * 3 / (2 * grid)).clamp(400, 2400);
            let mat = [PAINT, PAINT2, METAL][((i + k) % 3) as usize];
            m.add(
                chunk(
                    [x, floor, z],
                    rr,
                    hh,
                    seed + 50 + (i * grid + k) as u32,
                    mat,
                    lod,
                )
                .toned(SOOT),
            );
            // A piece of roof band fallen on the heap (a team candidate): two narrow planks.
            let planks = [(-rr / 3, 0u8), (0, 2u8), (rr / 3, 0u8)];
            for &(dz, q) in &planks[..if lod.has(1) { 3 } else { 1 }] {
                let len = rr * 8 / 10;
                let start = if q == 0 { x - len / 2 } else { x + len / 2 };
                tops.push(fallen_plate(
                    [start, floor + hh * 9 / 10, z + dz],
                    len,
                    280,
                    180,
                    q,
                    TEAM,
                ));
            }
        }
    }
    let n = if lod.has(0) {
        6
    } else if lod.has(1) {
        2
    } else {
        0
    };
    let (d, mut cands) = debris_cands(f, slab, n, seed ^ 0x99, lod, (lo, hi));
    cands.extend(edge_cands(f, slab, lod));
    // Fallen roof bands on the rubble floor, between the chunks.
    let mut fl = Vec::new();
    let mut z = mid[2] - iz + 200;
    while z + 260 < mid[2] + iz {
        fl.push(strip(
            [mid[0] - ix + 200, floor + 8, z],
            [mid[0] + ix - 200, floor + 24, z + 260],
            lod,
        ));
        z += 700;
    }
    cands.extend(interleave(fl));
    cands.extend(interleave(tops));
    m.add(d);
    if lod.has(0) {
        for i in 0..2u32 {
            let s = if i % 2 == 0 { 1 } else { -1 };
            if ix >= 1800 && iz >= 1800 {
                let len = ix.min(iz) / 3;
                m.add(
                    fallen_plate(
                        [mid[0] + s * ix / 3, floor, mid[2] - s * iz / 2],
                        len,
                        700,
                        500,
                        i as u8,
                        PAINT2,
                    )
                    .toned(SOOT),
                );
            }
        }
    }
    // A heap broken off low (`_rubble`, `_destroyed`, below 30 % of the roof) has lost its turret.
    if cut < 300 {
        parts.turret = None;
    }
    // A turret sinks onto the break.
    if let Some((_, at)) = &mut parts.turret {
        at[1] = at[1].min(plane.height_at(at[0], at[2]).max(slab + 200));
    }
    let anchors: Vec<(String, P)> = parts
        .anchors
        .iter()
        .map(|(n, p)| {
            (
                n.clone(),
                [p[0], p[1].min(plane.height_at(p[0], p[2])), p[2]],
            )
        })
        .collect();
    let fixed = turret_tris(&parts.turret);
    fit(teamed, &mut m, &fixed, cands, stem);
    join(m, parts.turret, &anchors)
}
