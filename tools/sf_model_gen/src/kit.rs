//! The modelling kit: integer-millimetre solids, details, the node tree and the LOD knob.
//!
//! Every solid is built from convex faces whose outward side is given explicitly (a vector the
//! face normal must agree with), so winding is never a matter of luck: a face that comes out
//! with zero area is dropped (no degenerate triangles), and a face whose outward side cannot be
//! decided panics while generating, never in the game.
//!
//! Axes are the engine's (docs/conventions.md §12): metres in the file, millimetres here, `+Y` up,
//! the front along `+X`, the right side `+Z`, the ground at `Y = 0`.

/// A point in millimetres.
pub type P = [i32; 3];

/// Index into [`Model::materials`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Mat(pub u8);

/// Per-mille shade multiplier of a triangle: 1000 is plain, lower is darker (panel lines, the
/// inside of a hatch, bolt heads). It is folded into the baked vertex colour ([`crate::shade`]).
pub type Tone = u16;

/// The plain tone.
pub const PLAIN: Tone = 1000;
/// The tone of a panel line or seam.
pub const SEAM: Tone = 560;
/// The tone of a bolt head or a recessed hatch.
pub const RECESS: Tone = 720;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tri {
    /// Corners, counter-clockwise seen from outside (the glTF front face).
    pub v: [P; 3],
    pub mat: Mat,
    pub tone: Tone,
}

impl Tri {
    /// The (unnormalised) outward normal, `(b - a) x (c - a)`.
    pub fn normal(&self) -> [i64; 3] {
        cross(sub(self.v[1], self.v[0]), sub(self.v[2], self.v[0]))
    }
}

pub fn sub(a: P, b: P) -> [i64; 3] {
    [
        i64::from(a[0]) - i64::from(b[0]),
        i64::from(a[1]) - i64::from(b[1]),
        i64::from(a[2]) - i64::from(b[2]),
    ]
}

pub fn cross(a: [i64; 3], b: [i64; 3]) -> [i64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub fn dot(a: [i64; 3], b: [i64; 3]) -> i128 {
    a.iter()
        .zip(&b)
        .map(|(&p, &q)| i128::from(p) * i128::from(q))
        .sum()
}

/// A triangle soup in millimetres.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mesh {
    pub tris: Vec<Tri>,
}

impl Mesh {
    pub fn new() -> Self {
        Self::default()
    }

    /// A convex polygon as a fan, oriented so its normal agrees with `out`. Zero-area triangles
    /// are dropped.
    ///
    /// # Panics
    /// When a triangle with area is perpendicular to `out` (the recipe gave no usable outside).
    pub fn face(&mut self, pts: &[P], mat: Mat, out: [i64; 3]) {
        for i in 1..pts.len().saturating_sub(1) {
            let mut t = Tri {
                v: [pts[0], pts[i], pts[i + 1]],
                mat,
                tone: PLAIN,
            };
            let n = t.normal();
            if n == [0, 0, 0] {
                continue;
            }
            let d = dot(n, out);
            assert!(d != 0, "face {pts:?}: outward side {out:?} is ambiguous");
            if d < 0 {
                t.v.swap(1, 2);
            }
            self.tris.push(t);
        }
    }

    pub fn add(&mut self, other: Mesh) -> &mut Self {
        self.tris.extend(other.tris);
        self
    }

    pub fn with(mut self, other: Mesh) -> Self {
        self.tris.extend(other.tris);
        self
    }

    pub fn moved(mut self, d: P) -> Self {
        for t in &mut self.tris {
            for v in &mut t.v {
                *v = [v[0] + d[0], v[1] + d[1], v[2] + d[2]];
            }
        }
        self
    }

    /// Mirror in the `Z = 0` plane (the left side from the right), winding kept outward.
    pub fn mirrored_z(mut self) -> Self {
        for t in &mut self.tris {
            for v in &mut t.v {
                v[2] = -v[2];
            }
            t.v.swap(1, 2);
        }
        self
    }

    /// Turn by `quarters` × 90° about `+Y` (front `+X` towards `+Z` is negative here: a proper
    /// rotation `x' = z, z' = -x` per quarter).
    pub fn turned_y(mut self, quarters: u8) -> Self {
        for _ in 0..quarters % 4 {
            for t in &mut self.tris {
                for v in &mut t.v {
                    *v = [v[2], v[1], -v[0]];
                }
            }
        }
        self
    }

    /// Set the tone of every triangle.
    pub fn toned(mut self, tone: Tone) -> Self {
        for t in &mut self.tris {
            t.tone = tone;
        }
        self
    }

    /// Change every triangle of material `from` to `to`.
    pub fn recoloured(mut self, from: Mat, to: Mat) -> Self {
        for t in self.tris.iter_mut().filter(|t| t.mat == from) {
            t.mat = to;
        }
        self
    }

    pub fn len(&self) -> usize {
        self.tris.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    fn map(mut self, f: impl Fn(P) -> P) -> Self {
        for t in &mut self.tris {
            for v in &mut t.v {
                *v = f(*v);
            }
        }
        self
    }
}

// ---- Fixed trigonometry -------------------------------------------------------------------

/// `sin(2π k / 64)` × 16384, k = 0..=16 (a quarter turn), rounded.
const SIN64: [i32; 17] = [
    0, 1606, 3196, 4756, 6270, 7723, 9102, 10394, 11585, 12665, 13623, 14449, 15137, 15679, 16069,
    16305, 16384,
];
/// The scale of [`SIN64`].
pub const ONE: i32 = 16384;

/// `sin(2π k / 64)` × [`ONE`] for any integer `k`.
pub fn sin64(k: i32) -> i32 {
    let k = k.rem_euclid(64);
    match k {
        0..=16 => SIN64[k as usize],
        17..=32 => SIN64[(32 - k) as usize],
        33..=48 => -SIN64[(k - 32) as usize],
        _ => -SIN64[(64 - k) as usize],
    }
}

/// `cos(2π k / 64)` × [`ONE`].
pub fn cos64(k: i32) -> i32 {
    sin64(k + 16)
}

/// `r × v / ONE`, rounded half away from zero.
pub fn scale(r: i32, v: i32) -> i32 {
    let p = i64::from(r) * i64::from(v);
    let q = (p.abs() + i64::from(ONE / 2)) / i64::from(ONE);
    (if p < 0 { -q } else { q }) as i32
}

/// The point at radius `r` and step `k` of a circle of `segs` steps, `(cos, sin)`.
///
/// # Panics
/// When `segs` does not divide 64.
pub fn on_circle(r: i32, k: i32, segs: u32) -> [i32; 2] {
    assert!(
        segs >= 3 && 64 % segs == 0,
        "{segs} segments: must divide 64"
    );
    let step = 64 / segs as i32;
    [scale(r, cos64(k * step)), scale(r, sin64(k * step))]
}

// ---- Outlines in a plane ------------------------------------------------------------------

/// A rectangle of half sizes `hx`, `hz` round the origin with its corners cut by `c` (8 points;
/// 4 when `c` is 0), counter-clockwise from `+X`.
pub fn chamfer_rect(hx: i32, hz: i32, c: i32) -> Vec<[i32; 2]> {
    if c <= 0 {
        return vec![[hx, -hz], [hx, hz], [-hx, hz], [-hx, -hz]];
    }
    vec![
        [hx, -hz + c],
        [hx, hz - c],
        [hx - c, hz],
        [-hx + c, hz],
        [-hx, hz - c],
        [-hx, -hz + c],
        [-hx + c, -hz],
        [hx - c, -hz],
    ]
}

/// A regular polygon of radius `r` with `segs` corners (half a step turned when `offset`).
pub fn ngon(r: i32, segs: u32, offset: bool) -> Vec<[i32; 2]> {
    let fine = segs * 2;
    (0..segs as i32)
        .map(|k| on_circle(r, 2 * k + i32::from(offset), fine))
        .collect()
}

/// Move a 2D outline.
pub fn shifted(pts: &[[i32; 2]], dx: i32, dz: i32) -> Vec<[i32; 2]> {
    pts.iter().map(|p| [p[0] + dx, p[1] + dz]).collect()
}

fn centroid2(pts: &[[i32; 2]]) -> [i64; 2] {
    let n = pts.len() as i64;
    let sx: i64 = pts.iter().map(|p| i64::from(p[0])).sum();
    let sz: i64 = pts.iter().map(|p| i64::from(p[1])).sum();
    [sx * 4 / n, sz * 4 / n]
}

// ---- Solids ------------------------------------------------------------------------------

/// A loft of horizontal convex rings (outline in `XZ`, height `Y`), all with the same number of
/// points: band `j` (ring `j` to `j + 1`) gets `bands[j]`, the top ring a cap of `top`, the
/// bottom ring a cap of `bottom`. Bottoms are left open by default: they face the ground.
pub fn loft(
    rings: &[(Vec<[i32; 2]>, i32)],
    bands: &[Mat],
    top: Option<Mat>,
    bottom: Option<Mat>,
) -> Mesh {
    assert!(rings.len() >= 2 && bands.len() + 1 == rings.len());
    let rings = &fix_ring_sizes(rings.to_vec());
    let n = rings[0].0.len();
    assert!(rings.iter().all(|r| r.0.len() == n), "rings differ in size");
    let mut m = Mesh::new();
    for (j, mat) in bands.iter().enumerate() {
        let (a, ya) = &rings[j];
        let (b, yb) = &rings[j + 1];
        let ca = centroid2(a);
        let cb = centroid2(b);
        // The axis point at mid height, times 4 like the centroids.
        let axis = [
            (ca[0] + cb[0]) / 2,
            i64::from(ya + yb) * 2,
            (ca[1] + cb[1]) / 2,
        ];
        for k in 0..n {
            let k1 = (k + 1) % n;
            let quad = [
                [a[k][0], *ya, a[k][1]],
                [a[k1][0], *ya, a[k1][1]],
                [b[k1][0], *yb, b[k1][1]],
                [b[k][0], *yb, b[k][1]],
            ];
            let c: [i64; 3] = [0, 1, 2].map(|i| quad.iter().map(|p| i64::from(p[i])).sum::<i64>());
            let out = [c[0] - axis[0], c[1] - axis[1], c[2] - axis[2]];
            m.face(&quad, *mat, out);
        }
    }
    let cap = |m: &mut Mesh, (ring, y): &(Vec<[i32; 2]>, i32), mat: Mat, up: i64| {
        let pts: Vec<P> = ring.iter().map(|p| [p[0], *y, p[1]]).collect();
        m.face(&pts, mat, [0, up, 0]);
    };
    if let Some(mat) = top {
        cap(&mut m, &rings[rings.len() - 1], mat, 1);
    }
    if let Some(mat) = bottom {
        cap(&mut m, &rings[0], mat, -1);
    }
    m
}

/// A box from `lo` to `hi` with its vertical and top edges bevelled by `b` (0: a plain box).
/// Open at the bottom.
pub fn bevel_box(lo: P, hi: P, b: i32, side: Mat, top: Mat) -> Mesh {
    let hx = (hi[0] - lo[0]) / 2;
    let hz = (hi[2] - lo[2]) / 2;
    let (cx, cz) = (lo[0] + hx, lo[2] + hz);
    let b = b.min(hx / 2).min(hz / 2).min((hi[1] - lo[1]) / 2).max(0);
    let rings = vec![
        (chamfer_rect(hx, hz, b), lo[1]),
        (chamfer_rect(hx, hz, b), hi[1] - b),
        (chamfer_rect(hx - b, hz - b, b / 2), hi[1]),
    ];
    loft(&rings, &[side, side], Some(top), None).moved([cx, 0, cz])
}

/// A plain box, open at the bottom (10 triangles).
pub fn cuboid(lo: P, hi: P, mat: Mat) -> Mesh {
    bevel_box(lo, hi, 0, mat, mat)
}

/// A closed plain box (12 triangles), for parts seen from below.
pub fn closed_box(lo: P, hi: P, mat: Mat) -> Mesh {
    let hx = (hi[0] - lo[0]) / 2;
    let hz = (hi[2] - lo[2]) / 2;
    let r = chamfer_rect(hx, hz, 0);
    loft(
        &[(r.clone(), lo[1]), (r, hi[1])],
        &[mat],
        Some(mat),
        Some(mat),
    )
    .moved([lo[0] + hx, 0, lo[2] + hz])
}

/// Rings of a bevel box all get the point count of the largest (a chamfer of 0 next to one of
/// more gives 4 and 8 points): a 4-point ring is padded by doubling its corners.
fn fix_ring_sizes(rings: Vec<(Vec<[i32; 2]>, i32)>) -> Vec<(Vec<[i32; 2]>, i32)> {
    let n = rings.iter().map(|r| r.0.len()).max().unwrap_or(0);
    rings
        .into_iter()
        .map(|(pts, y)| {
            if pts.len() == n || pts.len() != 4 || n != 8 {
                (pts, y)
            } else {
                // Same order as chamfer_rect: +X side first.
                let [a, b, c, d] = [pts[0], pts[1], pts[2], pts[3]];
                (vec![a, b, b, c, c, d, d, a], y)
            }
        })
        .collect()
}

/// A convex outline in the `XY` plane (side view) pushed out from `z0` to `z1`; both ends capped.
/// `sides[i]` is the material of the face along edge `i` (from point `i` to `i + 1`).
pub fn extrude_z(outline: &[[i32; 2]], z0: i32, z1: i32, sides: &[Mat], cap: Mat) -> Mesh {
    assert_eq!(outline.len(), sides.len());
    let n = outline.len();
    let c = centroid2(outline);
    let mut m = Mesh::new();
    for i in 0..n {
        let (p, q) = (outline[i], outline[(i + 1) % n]);
        let quad = [
            [p[0], p[1], z0],
            [q[0], q[1], z0],
            [q[0], q[1], z1],
            [p[0], p[1], z1],
        ];
        let mid = [i64::from(p[0] + q[0]) * 2, i64::from(p[1] + q[1]) * 2];
        m.face(&quad, sides[i], [mid[0] - c[0], mid[1] - c[1], 0]);
    }
    let end = |z: i32| -> Vec<P> { outline.iter().map(|p| [p[0], p[1], z]).collect() };
    m.face(&end(z0), cap, [0, 0, -1]);
    m.face(&end(z1), cap, [0, 0, 1]);
    m
}

/// One material for every side of [`extrude_z`].
pub fn extrude_z1(outline: &[[i32; 2]], z0: i32, z1: i32, mat: Mat) -> Mesh {
    extrude_z(outline, z0, z1, &vec![mat; outline.len()], mat)
}

/// A lathe about `+Y`: the profile `(radius, height)` from bottom to top, turned in `segs` steps
/// (a divisor of 64). Band `j` gets `bands[j]` (the last material repeats); a profile that ends
/// above the axis gets a flat cap of `cap` (or the last band). The bottom stays open unless
/// `base` is given.
pub fn lathe(
    profile: &[(i32, i32)],
    segs: u32,
    bands: &[Mat],
    cap: Option<Mat>,
    base: Option<Mat>,
) -> Mesh {
    assert!(!bands.is_empty() && profile.len() >= 2);
    let mut m = Mesh::new();
    let ring = |r: i32, h: i32| -> Vec<P> {
        (0..segs as i32)
            .map(|k| {
                let [x, z] = on_circle(r, k, segs);
                [x, h, z]
            })
            .collect()
    };
    for j in 0..profile.len() - 1 {
        let (r0, h0) = profile[j];
        let (r1, h1) = profile[j + 1];
        let mat = bands[j.min(bands.len() - 1)];
        let a = ring(r0, h0);
        let b = ring(r1, h1);
        let (dr, dy) = (i64::from(r1 - r0), i64::from(h1 - h0));
        let rmid = i64::from(r0 + r1);
        for k in 0..segs as usize {
            let k1 = (k + 1) % segs as usize;
            let quad = [a[k], a[k1], b[k1], b[k]];
            let cx: i64 = quad.iter().map(|p| i64::from(p[0])).sum();
            let cz: i64 = quad.iter().map(|p| i64::from(p[2])).sum();
            // Outward in the profile plane is (dy, -dr); cx, cz carry the radial direction
            // (|(cx, cz)| is about 2 rmid, as is the scale on the vertical part).
            m.face(&quad, mat, [dy * cx, -dr * rmid * 2, dy * cz]);
        }
    }
    let (rt, ht) = profile[profile.len() - 1];
    if rt > 0 {
        m.face(
            &ring(rt, ht),
            cap.unwrap_or(bands[bands.len() - 1]),
            [0, 1, 0],
        );
    }
    if let Some(mat) = base {
        let (rb, hb) = profile[0];
        if rb > 0 {
            m.face(&ring(rb, hb), mat, [0, -1, 0]);
        }
    }
    m
}

/// [`lathe`] with its axis along `+X` (barrels, exhausts lying down): height becomes `+X`.
pub fn lathe_x(
    profile: &[(i32, i32)],
    segs: u32,
    bands: &[Mat],
    cap: Option<Mat>,
    base: Option<Mat>,
) -> Mesh {
    lathe(profile, segs, bands, cap, base).map(|[x, y, z]| [y, -x, z])
}

/// [`lathe`] with its axis along `+Z` (wheels): height becomes `+Z`.
pub fn lathe_z(
    profile: &[(i32, i32)],
    segs: u32,
    bands: &[Mat],
    cap: Option<Mat>,
    base: Option<Mat>,
) -> Mesh {
    lathe(profile, segs, bands, cap, base).map(|[x, y, z]| [x, -z, y])
}

// ---- Details -----------------------------------------------------------------------------

/// A raised plate from `lo` to `hi` with a small bevel (armour plates, roof panels).
/// The bevel is `b` (pass [`Lod::bevel`]), at most half the thickness.
pub fn plate(lo: P, hi: P, b: i32, mat: Mat) -> Mesh {
    let t = hi[1] - lo[1];
    bevel_box(lo, hi, b.min(t / 2), mat, mat)
}

/// A panel line: a thin dark strip of real geometry, standing `raise` mm proud, from `a` to `b`
/// along one horizontal axis, `width` mm wide, lying on the plane `Y = a[1]`.
pub fn seam_top(a: P, b: P, width: i32, raise: i32, mat: Mat) -> Mesh {
    let (lo, hi) = if a[0] == b[0] {
        (
            [a[0] - width / 2, a[1], a[2].min(b[2])],
            [a[0] + width / 2, a[1] + raise, a[2].max(b[2])],
        )
    } else {
        (
            [a[0].min(b[0]), a[1], a[2] - width / 2],
            [a[0].max(b[0]), a[1] + raise, a[2] + width / 2],
        )
    };
    cuboid(lo, hi, mat).toned(SEAM)
}

/// A vertical panel line on a side face at `Z = z` (outside towards `+Z` when `z > 0`), from
/// height `y0` to `y1` at `x`.
pub fn seam_side_z(x: i32, y0: i32, y1: i32, z: i32, width: i32, raise: i32, mat: Mat) -> Mesh {
    let (z0, z1) = if z >= 0 {
        (z, z + raise)
    } else {
        (z - raise, z)
    };
    closed_box([x - width / 2, y0, z0], [x + width / 2, y1, z1], mat).toned(SEAM)
}

/// A bolt head (a short cylinder) standing on the plane `Y = at[1]`.
pub fn bolt(at: P, r: i32, h: i32, segs: u32, mat: Mat) -> Mesh {
    lathe(&[(r, 0), (r, h)], segs, &[mat], None, None)
        .toned(RECESS)
        .moved(at)
}

/// A round hatch on the plane `Y = at[1]`: a raised rim and a darker lid set into it.
pub fn hatch(at: P, r: i32, segs: u32, mat: Mat) -> Mesh {
    let rim = lathe(&[(r, 0), (r, 50), (r - 40, 60)], segs, &[mat], None, None);
    let lid = lathe(&[(r - 40, 30), (r - 40, 40)], segs, &[mat], None, None).toned(RECESS);
    rim.with(lid).moved(at)
}

// ---- Levels of detail ---------------------------------------------------------------------

/// Which level is being built: 0 is the full model, 1 and 2 are reduced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lod(pub u8);

impl Lod {
    pub const ALL: [Lod; 3] = [Lod(0), Lod(1), Lod(2)];

    /// Segments of a round part: `full`, then halved per level, at least 4 (a power of two).
    pub fn segs(self, full: u32) -> u32 {
        assert!(full.is_power_of_two() && full <= 64);
        (full >> self.0).max(4)
    }

    /// A bevel width: `full`, half of it at LOD1, none at LOD2.
    pub fn bevel(self, full: i32) -> i32 {
        match self.0 {
            0 => full,
            1 => full / 2,
            _ => 0,
        }
    }

    /// Whether details of level `upto` (0: only the full model) are built at this level.
    pub fn has(self, upto: u8) -> bool {
        self.0 <= upto
    }
}

// ---- Nodes and models --------------------------------------------------------------------

/// A node of the tree: a named part with its mesh (in its own frame), placed at `at` in its
/// parent's frame, or an empty anchor (`muzzle`, `fx_smoke_0`, ...).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub at: P,
    pub mesh: Mesh,
    pub children: Vec<Node>,
}

impl Node {
    pub fn part(name: &str, at: P, mesh: Mesh) -> Self {
        Self {
            name: name.to_owned(),
            at,
            mesh,
            children: Vec::new(),
        }
    }

    /// An empty node: an anchor or an empty joint.
    pub fn empty(name: &str, at: P) -> Self {
        Self::part(name, at, Mesh::new())
    }

    pub fn child(mut self, c: Node) -> Self {
        self.children.push(c);
        self
    }

    /// Triangles in this node and below.
    pub fn triangles(&self) -> usize {
        self.mesh.len() + self.children.iter().map(Node::triangles).sum::<usize>()
    }

    /// Find a node by name, depth first; with the name of its parent.
    pub fn find(&self, name: &str) -> Option<&Node> {
        if self.name == name {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(name))
    }

    /// Every triangle in model space (the translations summed), with its material and tone.
    pub fn world_tris(&self) -> Vec<Tri> {
        let mut out = Vec::new();
        self.collect(&[0, 0, 0], &mut out);
        out
    }

    fn collect(&self, origin: &P, out: &mut Vec<Tri>) {
        let o = [
            origin[0] + self.at[0],
            origin[1] + self.at[1],
            origin[2] + self.at[2],
        ];
        out.extend(self.mesh.clone().moved(o).tris);
        for c in &self.children {
            c.collect(&o, out);
        }
    }
}

/// A material: its name (`team` is the player colour) and its sRGB colour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Material {
    pub name: String,
    pub srgb: [u8; 3],
}

/// The name the engine reads as the player colour.
pub const TEAM: &str = "team";

/// A whole model: file stem, materials and the three level roots (`<stem>_LOD0..2`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub stem: String,
    pub materials: Vec<Material>,
    pub lods: Vec<Node>,
}

impl Model {
    /// Build a model: `parts(lod)` gives the children of each level root.
    pub fn new(stem: &str, materials: Vec<Material>, parts: impl Fn(Lod) -> Vec<Node>) -> Self {
        let lods = Lod::ALL
            .iter()
            .map(|&lod| Node {
                name: format!("{stem}_LOD{}", lod.0),
                at: [0, 0, 0],
                mesh: Mesh::new(),
                children: parts(lod),
            })
            .collect();
        Self {
            stem: stem.to_owned(),
            materials,
            lods,
        }
    }

    /// Triangles per level.
    pub fn triangles(&self) -> Vec<usize> {
        self.lods.iter().map(Node::triangles).collect()
    }

    /// The index of the team material, if there is one.
    pub fn team(&self) -> Option<Mat> {
        self.materials
            .iter()
            .position(|m| m.name == TEAM)
            .map(|i| Mat(i as u8))
    }
}
