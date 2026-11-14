//! The structures of both factions in style G (WP11.6c, docs/art-style.md section 10).
//!
//! One recipe per role, written once for both factions: the parts below ([`slab`], [`hall`],
//! [`tank`], [`stack`], ...) carry the shape language (Meridian rounded: big chamfers, round
//! tanks and domes; Veyran angular: sloped walls, octagonal stacks, wedges), so a role reads the
//! same in both factions and still belongs to its side. The team colour is narrow bands (roof
//! rings, platform edge stripes, flanges) switched on by [`fit_team`] until the share is reached.
//! The finished models of WP11.6a (the Meridian construction center, the Veyran power plant and
//! the Veyran defence tower) stay as they are; their states come from [`crate::stages`] like all.

use crate::kit::{
    Lod, Mat, Mesh, Model, Node, P, RECESS, SEAM, bevel_box, bolt, chamfer_rect, closed_box,
    cuboid, extrude_z1, hatch, lathe, lathe_x, lathe_z, loft, ngon, railing,
};
use crate::models::{anchors, insets, team_frame, team_rings, team_strip, wedge};
use crate::palette::{Palette, slot::*};
use crate::stages::{fit_team, pad_model};

/// The faction of a recipe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum F {
    M,
    V,
}

impl F {
    pub fn name(self) -> &'static str {
        match self {
            F::M => "meridian",
            F::V => "veyran",
        }
    }
}

/// What a recipe builds for one level: the body, the team candidates ([`fit_team`]), a turret
/// subtree for a defence, the two anchors and the platform the detail pad goes on.
pub struct Built {
    pub body: Mesh,
    pub cands: Vec<Mesh>,
    pub turret: Option<Node>,
    pub smoke: P,
    pub fire: P,
    /// `(hx, hz, y)` of the platform top: [`structure`] pads LOD0 with bolt rows along its edges
    /// up to [`DETAIL_FLOOR`] triangles.
    pub pad: (i32, i32, i32),
}

/// A finished structure's LOD0 has at least this many triangles (docs/art-style.md section 6:
/// 2,500-3,500); short recipes get bolt rows along their platform edges.
pub const DETAIL_FLOOR: usize = 2650;
/// Bolt rows stop before this many triangles.
pub const DETAIL_CEIL: usize = 3400;

/// Build a structure of `f` from `recipe`: pad the detail of LOD0, fit the team share, put the
/// node tree together (`body` -> `turret` -> ..., the anchors under the body).
pub fn structure(stem: &str, f: F, recipe: impl Fn(Lod) -> Built) -> Model {
    let pal = Palette::shipped();
    let pad = recipe(Lod(0)).pad;
    let mut m = Model::new(stem, pal.materials(f.name()), |lod| {
        let mut b = recipe(lod);
        let turret_tris = b.turret.as_ref().map(Node::world_tris).unwrap_or_default();
        fit_team(&mut b.body, &turret_tris, b.cands, stem);
        let mut body = Node::part("body", [0, 0, 0], b.body);
        if let Some(t) = b.turret {
            body = body.child(t);
        }
        vec![anchors(body, b.smoke, b.fire)]
    });
    pad_model(&mut m, DETAIL_FLOOR, DETAIL_CEIL, pad);
    m
}

// ---- Parts -------------------------------------------------------------------------------------

/// The platform: `hx` x `hz` half sizes, `h` high (Meridian a rounded slab, Veyran a slab with a
/// sloped skirt), and the team candidates: two stripes along every edge, 260 mm wide.
pub fn slab(f: F, lod: Lod, hx: i32, hz: i32, h: i32) -> (Mesh, Vec<Mesh>) {
    let m = match f {
        F::M => bevel_box([-hx, 0, -hz], [hx, h, hz], lod.bevel(180), PAINT2, PAINT2),
        F::V => loft(
            &[
                (chamfer_rect(hx, hz, lod.bevel(60)), 0),
                (chamfer_rect(hx - 160, hz - 160, lod.bevel(60)), h),
            ],
            &[PAINT2],
            Some(PAINT2),
            None,
        ),
    };
    let mut c = Vec::new();
    for ring in 0..4 {
        let e = 230 + if f == F::V { 160 } else { 0 } + ring * 560;
        if hx - e < 900 || hz - e < 900 {
            break;
        }
        for s in [-1, 1] {
            let z = s * (hz - e) - if s > 0 { 260 } else { 0 };
            c.push(team_strip(
                [-hx + e + 300, h, z],
                [-150, h + 18, z + 260],
                lod,
            ));
            c.push(team_strip(
                [150, h, z],
                [hx - e - 300, h + 18, z + 260],
                lod,
            ));
            let x = s * (hx - e) - if s > 0 { 260 } else { 0 };
            c.push(team_strip(
                [x, h, -hz + e + 600],
                [x + 260, h + 18, -150],
                lod,
            ));
            c.push(team_strip(
                [x, h, 150],
                [x + 260, h + 18, hz - e - 600],
                lod,
            ));
        }
    }
    (m, c)
}

/// A wall box on a wall facing along X (`out` = +1 for `+X`) at `x`, from `depth0` to `depth1` mm
/// out of it, `z0..z1`, `y0..y1`.
fn wx(x: i32, out: i32, d0: i32, d1: i32, z0: i32, z1: i32, y0: i32, y1: i32, mat: Mat) -> Mesh {
    let (a, b) = (x + out * d0, x + out * d1);
    closed_box([a.min(b), y0, z0], [a.max(b), y1, z1], mat)
}

/// The same on a wall facing along Z.
fn wz(z: i32, out: i32, d0: i32, d1: i32, x0: i32, x1: i32, y0: i32, y1: i32, mat: Mat) -> Mesh {
    let (a, b) = (z + out * d0, z + out * d1);
    closed_box([x0, y0, a.min(b)], [x1, y1, a.max(b)], mat)
}

/// A hall: `hx` x `hz` round `(cx, cz)`, from `y0` up `h`. Meridian: upright walls with big
/// chamfered corners and a rounded roof edge; Veyran: a dark skirt, upright walls to 55 % and
/// sloped walls above to 85 % of its size. Two team bands on the roof. LOD0 adds the facade:
/// window rows with frames along the long walls, panel lines and louvred vents along the short
/// ones. Returns the mesh and the roof height.
pub fn hall(f: F, lod: Lod, cx: i32, cz: i32, hx: i32, hz: i32, y0: i32, h: i32) -> (Mesh, i32) {
    let top = y0 + h;
    let (mut m, wall_top, roof) = match f {
        F::M => {
            let c = lod.bevel((hx.min(hz) / 4).min(1400));
            let c2 = if c > 0 { (c - 120).max(1) } else { 0 };
            let m = loft(
                &[
                    (chamfer_rect(hx, hz, c), y0),
                    (chamfer_rect(hx, hz, c), top - 260),
                    (chamfer_rect(hx - 200, hz - 200, c2), top),
                ],
                &[PAINT, PAINT],
                None,
                None,
            );
            (m, top - 260, (hx - 200, hz - 200, c2))
        }
        F::V => {
            let c = lod.bevel(260);
            let mid = y0 + h * 55 / 100;
            let (tx, tz) = (hx * 85 / 100, hz * 85 / 100);
            let m = loft(
                &[
                    (chamfer_rect(hx, hz, c), y0),
                    (chamfer_rect(hx, hz, c), y0 + 650),
                    (chamfer_rect(hx, hz, c), mid),
                    (chamfer_rect(tx, tz, c), top),
                ],
                &[PAINT2, PAINT, PAINT],
                None,
                None,
            );
            (m, mid, (tx, tz, c))
        }
    };
    let (rx, rz, rc) = roof;
    m.add(team_rings(
        &insets(rx, rz, rc, &[240, 300, 240]),
        top,
        top + 40,
        lod,
        PAINT,
    ));
    if lod.has(0) {
        let c = match f {
            F::M => (hx.min(hz) / 4).min(1400),
            F::V => 260,
        };
        // Windows along the long walls (the Z walls when hx >= hz).
        let wy0 = (y0 + 900).min(wall_top - 900);
        let wy1 = (wy0 + 700).min(wall_top - 150);
        let span = hx - c - 300;
        let n = ((2 * span) / 1500).max(1);
        for s in [-1, 1] {
            let z = s * hz;
            for i in 0..n {
                let x = -span + (2 * span) * (2 * i + 1) / (2 * n);
                let (fw, gw) = (520, 440);
                m.add(wz(z, s, 0, 40, x - fw, x + fw, wy0 - 60, wy1 + 60, PAINT2));
                match f {
                    F::M => {
                        m.add(wz(z, s, 40, 55, x - gw, x + gw, wy0, wy1, ACCENT));
                        m.add(
                            wz(z, s, 0, 25, x - fw, x + fw, y0 + 250, wy0 - 160, PAINT).toned(900),
                        );
                    }
                    F::V => {
                        m.add(
                            wz(z, s, 40, 55, x - gw, x + gw, wy1 - 260, wy1, RUBBER).toned(RECESS),
                        );
                        m.add(
                            wz(z, s, 0, 30, x - 20, x + 20, y0 + 650, wy0 - 60, PAINT2).toned(SEAM),
                        );
                    }
                }
                if i + 1 < n {
                    let xs = -span + (2 * span) * (i + 1) / n;
                    m.add(
                        wz(
                            z,
                            s,
                            0,
                            14,
                            xs - 25,
                            xs + 25,
                            y0 + 100,
                            wall_top - 80,
                            PAINT,
                        )
                        .toned(SEAM),
                    );
                }
            }
        }
        // Panel lines and vents on the short walls.
        let spz = hz - c - 300;
        let nz = ((2 * spz) / 1300).max(1);
        for s in [-1, 1] {
            let x = s * hx;
            for i in 0..nz {
                let z = -spz + (2 * spz) * (2 * i + 1) / (2 * nz);
                if i % 2 == 0 {
                    m.add(wx(x, s, 0, 30, z - 320, z + 320, wy0, wy0 + 600, PAINT2));
                    for k in 0..2 {
                        let y = wy0 + 120 + k * 240;
                        m.add(wx(x, s, 30, 70, z - 280, z + 280, y, y + 60, METAL).toned(RECESS));
                    }
                } else {
                    m.add(
                        wx(x, s, 0, 14, z - 25, z + 25, y0 + 100, wall_top - 80, PAINT).toned(SEAM),
                    );
                }
            }
        }
    }
    (m.moved([cx, 0, cz]), top)
}

/// A roof unit at `(x, y, z)`: a box with a fan (Meridian round, Veyran under a grille), `w` mm
/// across. LOD1 the box only, LOD2 nothing.
pub fn roof_unit(f: F, lod: Lod, x: i32, y: i32, z: i32, w: i32) -> Mesh {
    let mut m = Mesh::new();
    if !lod.has(1) {
        return m;
    }
    let h = w / 2;
    m.add(match f {
        F::M => bevel_box(
            [-w / 2, 0, -w / 2],
            [w / 2, h, w / 2],
            lod.bevel(80),
            PAINT2,
            PAINT2,
        ),
        F::V => cuboid([-w / 2, 0, -w / 2], [w / 2, h, w / 2], PAINT2),
    });
    if lod.has(0) {
        match f {
            F::M => {
                m.add(
                    lathe(
                        &[(w * 4 / 10, 0), (w * 4 / 10, 80)],
                        16,
                        &[METAL],
                        None,
                        None,
                    )
                    .moved([0, h, 0]),
                );
                m.add(hatch([0, h + 80, 0], w * 36 / 100, 16, PAINT2));
            }
            F::V => {
                for k in 0..4 {
                    let t = -w / 2 + 80 + k * (w - 160) / 4;
                    m.add(
                        cuboid([t, h, -w / 2 + 60], [t + 60, h + 40, w / 2 - 60], METAL)
                            .toned(SEAM),
                    );
                }
                m.add(
                    cuboid(
                        [-w / 2 + 40, h, -w / 2 + 40],
                        [w / 2 - 40, h + 10, w / 2 - 40],
                        RUBBER,
                    )
                    .toned(RECESS),
                );
            }
        }
    }
    m.moved([x, y, z])
}

/// A tank of radius `r`, `h` high on `y0` at `(x, z)`: Meridian a round tank with a team shoulder
/// and a domed top, Veyran an octagonal tank with a team flange on its top. LOD0 adds a rivet
/// ring and a ladder (towards `+X`).
pub fn tank(f: F, lod: Lod, x: i32, z: i32, y0: i32, r: i32, h: i32) -> Mesh {
    let mut m = match f {
        F::M => {
            let prof = [
                (r, 0),
                (r, h - 450),
                (r * 85 / 100, h - 100),
                (r * 55 / 100, h),
                (r * 25 / 100, h + 60),
            ];
            let n = if lod.has(1) { 5 } else { 4 };
            lathe(
                &prof[..n],
                lod.segs(16),
                &[PAINT2, TEAM, PAINT2, PAINT2][..n - 1],
                Some(PAINT2),
                None,
            )
        }
        F::V => {
            let top = ngon(r * 8 / 10, lod.segs(8), true);
            let mut m = loft(
                &[
                    (ngon(r, lod.segs(8), true), 0),
                    (ngon(r, lod.segs(8), true), h - 500),
                    (top.clone(), h),
                ],
                &[PAINT2, PAINT],
                None,
                None,
            );
            m.add(team_frame(
                &top,
                &ngon(r * 8 / 10 - 250, lod.segs(8), true),
                h,
                h + 30,
                lod,
                PAINT,
            ));
            m
        }
    };
    if lod.has(0) {
        m.add(lathe(&[(r + 30, 0), (r + 30, 100)], 16, &[METAL], None, None).moved([0, h / 3, 0]));
        let lx = r - 20;
        for side in [-170, 170] {
            m.add(closed_box(
                [lx, 0, side - 25],
                [lx + 50, h - 300, side + 25],
                METAL,
            ));
        }
        for k in 0..(h / 450) {
            let y = 350 + k * 420;
            m.add(cuboid([lx, y, -170], [lx + 40, y + 35, 170], METAL));
        }
    }
    m.moved([x, y0, z])
}

/// A stack of radius `r`, `h` high on `y0`: Meridian round with a lip, Veyran octagonal with an
/// accent glow band (`accent`); both with a team flange at the mouth.
pub fn stack(f: F, lod: Lod, x: i32, z: i32, y0: i32, r: i32, h: i32, accent: bool) -> Mesh {
    let mut m = match f {
        F::M => lathe(
            &[
                (r, 0),
                (r * 85 / 100, h * 6 / 10),
                (r * 80 / 100, h - 300),
                (r * 88 / 100, h),
            ],
            lod.segs(16),
            &[PAINT, PAINT, PAINT2],
            Some(METAL),
            None,
        ),
        F::V => {
            let rings = vec![
                (ngon(r, lod.segs(8), true), 0),
                (ngon(r * 4 / 5, lod.segs(8), true), h - 700),
                (ngon(r * 4 / 5, lod.segs(8), true), h - 400),
                (ngon(r * 9 / 10, lod.segs(8), true), h),
            ];
            let mid = if accent && lod.has(1) { ACCENT } else { PAINT2 };
            loft(&rings, &[PAINT2, mid, PAINT2], Some(METAL), None)
        }
    };
    let rr = match f {
        F::M => r * 88 / 100,
        F::V => r * 9 / 10,
    };
    let segs = match f {
        F::M => lod.segs(16),
        F::V => 8,
    };
    m.add(team_frame(
        &ngon(rr + 60, segs, true),
        &ngon(rr - 200, segs, true),
        h,
        h + 50,
        lod,
        METAL,
    ));
    if lod.has(0) {
        for k in 0..3 {
            m.add(
                lathe(&[(r + 20, 0), (r + 20, 80)], segs, &[METAL], None, None).moved([
                    0,
                    600 + k * h / 4,
                    0,
                ]),
            );
        }
    }
    m.moved([x, y0, z])
}

/// A pipe along X from `at`, `len` long, radius `r`, with flanges every 1.5 m at LOD0. LOD2 none.
pub fn pipe_x(lod: Lod, at: P, len: i32, r: i32) -> Mesh {
    let mut m = Mesh::new();
    if !lod.has(1) {
        return m;
    }
    m.add(lathe_x(
        &[(r, 0), (r, len)],
        lod.segs(8),
        &[METAL],
        None,
        None,
    ));
    if lod.has(0) {
        let mut t = 300;
        while t < len - 100 {
            m.add(lathe_x(
                &[(r + 50, t), (r + 50, t + 70)],
                8,
                &[METAL],
                Some(METAL),
                Some(METAL),
            ));
            t += 1500;
        }
    }
    m.moved(at)
}

/// The same along Z.
pub fn pipe_z(lod: Lod, at: P, len: i32, r: i32) -> Mesh {
    let mut m = Mesh::new();
    if !lod.has(1) {
        return m;
    }
    m.add(lathe_z(
        &[(r, 0), (r, len)],
        lod.segs(8),
        &[METAL],
        None,
        None,
    ));
    if lod.has(0) {
        let mut t = 300;
        while t < len - 100 {
            m.add(lathe_z(
                &[(r + 50, t), (r + 50, t + 70)],
                8,
                &[METAL],
                Some(METAL),
                Some(METAL),
            ));
            t += 1500;
        }
    }
    m.moved(at)
}

/// A large door in a wall facing `+X` at `x`, `w` wide round `z`, `h` high from `y0`: a frame, a
/// roller door with slats (LOD0), Meridian a rounded lintel, Veyran a sloped hood. LOD2 a dark
/// panel only.
pub fn door(f: F, lod: Lod, x: i32, z: i32, y0: i32, w: i32, h: i32) -> Mesh {
    let mut m = Mesh::new();
    if !lod.has(1) {
        return m;
    }
    m.add(wx(x, 1, 0, 30, z - w / 2, z + w / 2, y0, y0 + h, METAL).toned(860));
    for s in [-1, 1] {
        let zp = z + s * (w / 2 + 90);
        m.add(wx(x, 1, 0, 110, zp - 90, zp + 90, y0, y0 + h + 160, PAINT2));
    }
    match f {
        F::M => m.add(
            lathe_z(
                &[(160, -w / 2 - 180), (160, w / 2 + 180)],
                lod.segs(8),
                &[PAINT2],
                Some(PAINT2),
                Some(PAINT2),
            )
            .moved([x + 60, y0 + h + 160, z]),
        ),
        F::V => m.add(extrude_z1(
            &[
                [x, y0 + h],
                [x + 420, y0 + h + 60],
                [x + 420, y0 + h + 160],
                [x, y0 + h + 360],
            ],
            z - w / 2 - 180,
            z + w / 2 + 180,
            PAINT2,
        )),
    };
    if lod.has(0) {
        let mut y = y0 + 250;
        while y < y0 + h - 100 {
            m.add(
                wx(
                    x,
                    1,
                    30,
                    45,
                    z - w / 2 + 40,
                    z + w / 2 - 40,
                    y,
                    y + 40,
                    METAL,
                )
                .toned(SEAM),
            );
            y += 300;
        }
        // Hazard posts at the foot of the frame.
        for s in [-1, 1] {
            m.add(
                lathe(&[(110, 0), (110, 900)], 8, &[PAINT], Some(METAL), None).moved([
                    x + 350,
                    y0,
                    z + s * (w / 2 + 300),
                ]),
            );
        }
    }
    m
}

/// A railing round a rectangle on `y` (LOD0).
pub fn railing_rect(lod: Lod, x0: i32, z0: i32, x1: i32, z1: i32, y: i32) -> Mesh {
    let mut m = Mesh::new();
    if !lod.has(0) {
        return m;
    }
    let p = |a: i32, b: i32| ((b - a).abs() / 1200).max(2);
    m.add(railing([x0, y, z0], [x1, y, z0], 600, p(x0, x1), METAL));
    m.add(railing([x0, y, z1], [x1, y, z1], 600, p(x0, x1), METAL));
    m.add(railing([x0, y, z0], [x0, y, z1], 600, p(z0, z1), METAL));
    m.add(railing([x1, y, z0], [x1, y, z1], 600, p(z0, z1), METAL));
    m
}

/// Team candidates: narrow stripes (260 mm) across a flat area from `x0..x1`, every `step` mm
/// along Z between `z0` and `z1`, lying on `y` (lane or setting-out markings).
pub fn stripes(x0: i32, x1: i32, z0: i32, z1: i32, y: i32, step: i32, lod: Lod) -> Vec<Mesh> {
    let mut c = Vec::new();
    let mut z = z0;
    while z + 260 <= z1 {
        c.push(team_strip([x0, y, z], [x1, y + 16, z + 260], lod));
        z += step;
    }
    c
}

/// An antenna mast `h` high on `(x, y, z)` (every level: it sets the height).
pub fn antenna(lod: Lod, x: i32, y: i32, z: i32, h: i32) -> Mesh {
    let mut m = lathe(&[(60, 0), (35, h)], 4, &[METAL], None, None);
    if lod.has(0) {
        m.add(lathe(&[(120, 0), (120, 60)], 8, &[METAL], Some(METAL), None).moved([0, h / 2, 0]));
    }
    m.moved([x, y, z])
}

/// A dish of radius `r` facing up on a mast at `(x, y, z)`, `mast` high: an outer shell and an
/// inner face (seen from above), a feed on struts. Meridian round, Veyran octagonal.
pub fn dish(f: F, lod: Lod, x: i32, y: i32, z: i32, r: i32, mast: i32) -> Mesh {
    let segs = match f {
        F::M => lod.segs(16),
        F::V => 8,
    };
    let d = r * 35 / 100;
    let mut m = lathe(
        &[(r * 18 / 100, 0), (r * 18 / 100, mast)],
        lod.segs(8),
        &[METAL],
        None,
        None,
    );
    let prof = [
        (r * 15 / 100, mast),
        (r * 60 / 100, mast + d * 35 / 100),
        (r, mast + d),
    ];
    m.add(lathe(&prof, segs, &[PAINT, PAINT], None, None));
    // The inner face: the same profile from the rim down, so it faces up and in.
    let inner = [
        (r - 30, mast + d - 10),
        (r * 60 / 100 - 20, mast + d * 35 / 100 + 20),
        (r * 15 / 100, mast + 30),
    ];
    m.add(lathe(&inner, segs, &[PAINT2, PAINT2], Some(METAL), None));
    m.add(lathe(
        &[(r + 40, mast + d - 40), (r + 40, mast + d + 30)],
        segs,
        &[TEAM],
        None,
        None,
    ));
    if lod.has(1) {
        m.add(lathe(
            &[
                (90, mast + 30),
                (70, mast + d + r * 3 / 10),
                (140, mast + d + r * 3 / 10 + 60),
            ],
            lod.segs(8),
            &[METAL],
            Some(if f == F::V { ACCENT } else { METAL }),
            None,
        ));
    }
    m.moved([x, y, z])
}

/// A defence turret: Meridian a rounded cast head with a dome, Veyran a wedge; a barrel of
/// `len` (and `twin` for two) with a muzzle. Team: a band round the head's roof. Placed at
/// `at` on the body.
pub fn turret(f: F, lod: Lod, at: P, size: i32, len: i32, twin: bool) -> Node {
    let s = size;
    let mut t = match f {
        F::M => {
            let mut t = lathe(
                &[
                    (s, 0),
                    (s, s * 45 / 100),
                    (s * 88 / 100, s * 65 / 100),
                    (s * 70 / 100, s * 72 / 100),
                ],
                lod.segs(16),
                &[PAINT, PAINT, PAINT],
                None,
                None,
            );
            t.add(team_frame(
                &ngon(s * 70 / 100, lod.segs(16), true),
                &ngon(s * 70 / 100 - 220, lod.segs(16), true),
                s * 72 / 100,
                s * 72 / 100 + 30,
                lod,
                PAINT,
            ));
            if lod.has(1) {
                t.add(
                    lathe(
                        &[
                            (s * 30 / 100, 0),
                            (s * 22 / 100, s * 12 / 100),
                            (0, s * 16 / 100),
                        ],
                        lod.segs(16),
                        &[PAINT2],
                        None,
                        None,
                    )
                    .moved([-s * 25 / 100, s * 72 / 100 + 30, 0]),
                );
            }
            t
        }
        F::V => {
            let hz = s * 9 / 10;
            let h = s * 66 / 100;
            let mut t = loft(
                &[
                    (wedge(s, s / 2, s / 2, hz), 0),
                    (
                        wedge(s * 85 / 100, s * 32 / 100, s * 42 / 100, hz * 75 / 100),
                        h,
                    ),
                ],
                &[PAINT],
                Some(PAINT),
                None,
            );
            t.add(team_frame(
                &wedge(s * 83 / 100, s * 31 / 100, s * 41 / 100, hz * 73 / 100),
                &wedge(s * 60 / 100, s * 18 / 100, s * 30 / 100, hz * 50 / 100),
                h,
                h + 40,
                lod,
                PAINT,
            ));
            t
        }
    };
    if lod.has(0) {
        t.add(hatch(
            [-s * 30 / 100, s * 72 / 100 + 30, s * 30 / 100],
            s * 18 / 100,
            8,
            PAINT2,
        ));
        for i in 0..8 {
            let [x, z] = crate::kit::on_circle(s * 92 / 100, i, 8);
            t.add(bolt([x, s * 40 / 100, z], 40, 25, 4, METAL).moved([0, 0, 0]));
        }
    }
    // The whip antenna (every level: it sets the height).
    t.add(lathe(&[(25, 0), (25, s)], 4, &[METAL], None, None).moved([
        -s * 60 / 100,
        s * 60 / 100,
        -s * 40 / 100,
    ]));
    let r = (s * 13 / 100).max(90);
    let mut barrel = Mesh::new();
    let offs: &[i32] = if twin {
        &[-r * 15 / 10, r * 15 / 10]
    } else {
        &[0]
    };
    for &dz in offs {
        barrel
            .add(lathe_x(&[(r, 0), (r, len)], lod.segs(8), &[METAL], None, None).moved([0, 0, dz]));
        barrel.add(closed_box(
            [len - 200, -r * 13 / 10, dz - r * 13 / 10],
            [len, r * 13 / 10, dz + r * 13 / 10],
            METAL,
        ));
        if lod.has(0) {
            barrel.add(
                lathe_x(
                    &[(r * 14 / 10, 0), (r * 14 / 10, 300)],
                    8,
                    &[PAINT2],
                    None,
                    Some(PAINT2),
                )
                .moved([0, 0, dz]),
            );
        }
    }
    let barrel = Node::part("barrel", [s * 70 / 100, s * 38 / 100, 0], barrel)
        .child(Node::empty("muzzle", [len, 0, 0]));
    Node::part("turret", at, t).child(barrel)
}

/// Steps of a ladder on a wall facing `+X` at `x`, round `z`, from `y0` to `y1` (LOD0).
pub fn ladder(lod: Lod, x: i32, z: i32, y0: i32, y1: i32) -> Mesh {
    let mut m = Mesh::new();
    if !lod.has(0) {
        return m;
    }
    for s in [-200, 200] {
        m.add(closed_box(
            [x, y0, z + s - 25],
            [x + 50, y1, z + s + 25],
            METAL,
        ));
    }
    let mut y = y0 + 300;
    while y < y1 - 100 {
        m.add(cuboid([x, y, z - 200], [x + 40, y + 35, z + 200], METAL));
        y += 320;
    }
    m
}

/// A landing or loading pad at `(x, y, z)`: a plate and two team rings.
pub fn pad(f: F, lod: Lod, x: i32, y: i32, z: i32, hx: i32, hz: i32) -> Mesh {
    let c = match f {
        F::M => lod.bevel(hx.min(hz) / 3),
        F::V => lod.bevel(120),
    };
    let mut m = bevel_box([-hx, 0, -hz], [hx, 150, hz], lod.bevel(60), METAL, METAL);
    m.add(team_rings(
        &insets(hx - 250, hz - 250, c, &[260, 280, 260]),
        150,
        180,
        lod,
        METAL,
    ));
    m.moved([x, y, z])
}

/// Sandbags or armour blocks in a ring of `n` blocks of radius `r` on `y` (LOD1+). Meridian:
/// rounded bags, Veyran: wedge blocks.
pub fn bags(f: F, lod: Lod, x: i32, z: i32, y: i32, r: i32, n: i32, skip: i32) -> Mesh {
    let mut m = Mesh::new();
    if !lod.has(1) {
        return m;
    }
    for k in 0..n {
        if k == skip {
            continue;
        }
        let [px, pz] = crate::kit::on_circle(r, k * 64 / n, 64);
        let b = match f {
            F::M => bevel_box(
                [-280, 0, -280],
                [280, 380, 280],
                lod.bevel(110),
                PAINT2,
                PAINT2,
            ),
            F::V => loft(
                &[
                    (chamfer_rect(300, 300, 0), 0),
                    (chamfer_rect(220, 220, 0), 420),
                ],
                &[PAINT2],
                Some(PAINT2),
                None,
            ),
        };
        m.add(b.moved([px, 0, pz]));
    }
    m.moved([x, y, z])
}

fn built(
    body: Mesh,
    cands: Vec<Mesh>,
    turret: Option<Node>,
    smoke: P,
    fire: P,
    pad: (i32, i32, i32),
) -> Built {
    Built {
        body,
        cands,
        turret,
        smoke,
        fire,
        pad,
    }
}

// ---- The roles -----------------------------------------------------------------------------------

/// Construction center (4 x 4): a hall with a command tower, two tanks, a landing pad and a
/// crane (Veyran; the Meridian one is the WP11.6a model).
pub fn construction_center(f: F) -> Model {
    structure(&format!("{}_construction_center", f.name()), f, |lod| {
        let (sh, (hx, hz)) = (450, (7400, 7400));
        let (mut m, mut c) = slab(f, lod, hx, hz, sh);
        let (h, top) = hall(f, lod, -1500, -1500, 4600, 4300, sh, 3400);
        m.add(h);
        // The command tower on the hall.
        m.add(
            match f {
                F::M => lathe(
                    &[(1500, 0), (1500, 1400), (1100, 1800), (0, 1900)],
                    lod.segs(16),
                    &[PAINT, PAINT, PAINT],
                    None,
                    None,
                ),
                F::V => loft(
                    &[
                        (ngon(1600, lod.segs(8), true), 0),
                        (ngon(1600, lod.segs(8), true), 1200),
                        (ngon(1100, lod.segs(8), true), 1900),
                    ],
                    &[PAINT2, PAINT],
                    Some(PAINT),
                    None,
                ),
            }
            .moved([-2300, top, -2300]),
        );
        m.add(antenna(lod, -2300, top + 1900, -2300, 2600));
        for (x, z) in [(4700, 4000), (4700, 1000)] {
            m.add(tank(f, lod, x, z, sh, 1150, 3000));
        }
        m.add(pipe_x(lod, [3100, 1700, 4000], 500, 180));
        m.add(pipe_x(lod, [3100, 1700, 1000], 500, 180));
        m.add(pad(f, lod, 4400, sh, -4400, 2300, 2300));
        // The crane (all levels).
        m.add(closed_box([-6700, sh, 4500], [-6100, 6800, 5100], PAINT2));
        m.add(closed_box([-6900, 6800, 3200], [-900, 7250, 5300], PAINT));
        c.push(team_strip([-6900, 7250, 3200], [-900, 7268, 3460], lod));
        c.push(team_strip([-6900, 7250, 5040], [-900, 7268, 5300], lod));
        if lod.has(0) {
            for k in 0..7 {
                let y = 1100 + k * 800;
                m.add(closed_box([-6750, y, 4450], [-6050, y + 80, 5150], METAL));
            }
            m.add(closed_box([-1250, 4000, 4150], [-1210, 6800, 4190], METAL));
            m.add(closed_box([-1450, 3600, 3950], [-1000, 4000, 4400], PAINT2));
        }
        m.add(door(f, lod, 3100, -1500, sh, 3200, 2400));
        for (x, z) in [(-4500, -4400), (1000, -4400), (-4500, 1100)] {
            m.add(roof_unit(f, lod, x, top, z, 1000));
        }
        m.add(railing_rect(lod, -7200, -7200, 7200, 7200, sh));
        m.add(ladder(lod, 5850, 4000, sh, sh + 2800));
        built(
            m,
            c,
            None,
            [-2300, top + 1900, -2300],
            [4700, 3500, 4000],
            (hx, hz, sh),
        )
    })
}

/// Power plant (2 x 3): a hall with two cooling stacks behind it (Meridian; the Veyran one is the
/// WP11.6a model).
pub fn power_plant(f: F) -> Model {
    structure(&format!("{}_power_plant", f.name()), f, |lod| {
        let (sh, hx, hz) = (400, 3700, 5600);
        let (mut m, c) = slab(f, lod, hx, hz, sh);
        let (h, top) = hall(f, lod, 0, -1700, 3100, 3500, sh, 3100);
        m.add(h);
        for x in [-1600, 1600] {
            m.add(stack(f, lod, x, 3700, sh, 1050, 5600, true));
            m.add(pipe_z(lod, [x, 1700, 1800], 900, 200));
        }
        m.add(door(f, lod, 3100, -1700, sh, 1800, 2200));
        for z in [-3200, -500] {
            m.add(roof_unit(f, lod, -1500, top, z, 1000));
        }
        if lod.has(1) {
            // The transformer yard on the front left.
            for x in [1500, 2500] {
                m.add(bevel_box(
                    [x - 400, sh, -5300],
                    [x + 400, sh + 900, -4400],
                    lod.bevel(60),
                    PAINT2,
                    PAINT2,
                ));
            }
        }
        if lod.has(0) {
            for x in [1500, 2500] {
                for k in 0..3 {
                    m.add(
                        lathe(
                            &[(60, 0), (60, 300), (110, 320), (110, 360)],
                            8,
                            &[ACCENT, METAL],
                            Some(METAL),
                            None,
                        )
                        .moved([x - 250 + k * 250, sh + 900, -4850]),
                    );
                }
            }
            m.add(railing_rect(lod, -1400, -4500, 1400, 1000, top));
        }
        built(
            m,
            c,
            None,
            [-1600, sh + 5650, 3700],
            [0, top, -1700],
            (hx, hz, sh),
        )
    })
}

/// Refinery (3 x 4): a processing hall, two ore silos with a conveyor, a docking bay for the
/// harvester on the `+X` side and a stack.
pub fn refinery(f: F) -> Model {
    structure(&format!("{}_refinery", f.name()), f, |lod| {
        let (sh, hx, hz) = (400, 5600, 7500);
        let (mut m, mut c) = slab(f, lod, hx, hz, sh);
        let (h, top) = hall(f, lod, -1800, 2400, 3600, 4600, sh, 3600);
        m.add(h);
        for z in [-3200, -6000] {
            m.add(tank(f, lod, -2800, z, sh, 1300, 3400));
        }
        m.add(stack(f, lod, -4300, 5700, top, 520, 2200, true));
        // The conveyor from the bay to the silos: an inclined run of boxes.
        for k in 0..if lod.has(1) { 6 } else { 0 } {
            let x = 1000 - k * 520 * if lod.has(1) { 1 } else { 3 };
            let y = sh + 600 + k * 380 * if lod.has(1) { 1 } else { 3 };
            m.add(closed_box(
                [x - 300, y, -4800],
                [x + 300, y + 200, -4400],
                PAINT2,
            ));
        }
        // The docking bay: a pad with team rings and a hopper.
        m.add(pad(f, lod, 3600, sh, -4300, 1800, 2900));
        m.add(
            match f {
                F::M => lathe(
                    &[(900, 0), (1100, 1300), (1100, 1600)],
                    lod.segs(16),
                    &[PAINT, PAINT],
                    Some(RUBBER),
                    None,
                ),
                F::V => loft(
                    &[
                        (ngon(800, lod.segs(8), true), 0),
                        (ngon(1100, lod.segs(8), true), 1300),
                        (ngon(1100, lod.segs(8), true), 1600),
                    ],
                    &[PAINT, PAINT],
                    Some(RUBBER),
                    None,
                ),
            }
            .moved([1500, sh, -4600]),
        );
        c.push(team_strip([2200, sh, 300], [5200, sh + 18, 560], lod));
        c.push(team_strip([2200, sh, 1300], [5200, sh + 18, 1560], lod));
        m.add(door(f, lod, 1800, 2400, sh, 2400, 2400));
        c.extend(stripes(2200, 5300, 200, 7000, sh, 700, lod));
        for z in [600, 4200] {
            m.add(roof_unit(f, lod, -2800, top, z, 1100));
        }
        m.add(pipe_z(lod, [-1300, 2000, -4500], 2300, 160));
        m.add(railing_rect(lod, -5400, -7300, 5400, 7300, sh));
        built(
            m,
            c,
            None,
            [-4300, top + 2250, 5700],
            [-1800, top, 2400],
            (hx, hz, sh),
        )
    })
}

/// Barracks (2 x 3): a long low hall with a door to `+X`, a flag mast and a sandbag ring.
pub fn barracks(f: F) -> Model {
    structure(&format!("{}_barracks", f.name()), f, |lod| {
        let (sh, hx, hz) = (350, 3700, 5600);
        let (mut m, mut c) = slab(f, lod, hx, hz, sh);
        let (h, top) = hall(f, lod, -600, 800, 2700, 4400, sh, 2800);
        m.add(h);
        m.add(door(f, lod, 2100, 800, sh, 1400, 2000));
        c.extend(stripes(2300, 3500, -3000, 5200, sh, 700, lod));
        m.add(antenna(lod, 2900, sh, -4600, 4800));
        if lod.has(1) {
            // The flag on the mast is a team panel facing the camera's side.
            m.add(closed_box(
                [2930, sh + 3900, -4620],
                [3800, sh + 4500, -4580],
                TEAM,
            ));
        }
        m.add(bags(f, lod, 1800, -4300, sh, 1300, 8, 0));
        for z in [-2000, 1000, 3800] {
            m.add(roof_unit(f, lod, -1200, top, z, 900));
        }
        m.add(railing_rect(lod, -3500, -5400, 3500, -3800, sh));
        built(
            m,
            c,
            None,
            [-1200, top + 400, 1000],
            [-600, top, 3000],
            (hx, hz, sh),
        )
    })
}

/// Vehicle factory (3 x 4): a tall hall with a big door to `+X`, roof lights, a gantry on the
/// roof and a side annex.
pub fn vehicle_factory(f: F) -> Model {
    structure(&format!("{}_vehicle_factory", f.name()), f, |lod| {
        let (sh, hx, hz) = (400, 5600, 7500);
        let (mut m, mut c) = slab(f, lod, hx, hz, sh);
        let (h, top) = hall(f, lod, -700, -500, 4300, 5600, sh, 4300);
        m.add(h);
        let (a, atop) = hall(f, lod, -2800, 6000, 2200, 1200, sh, 2400);
        m.add(a);
        m.add(door(f, lod, 3600, -500, sh, 4200, 3200));
        c.extend(stripes(3900, 5300, -7000, 7000, sh, 700, lod));
        // Roof lights: Meridian rounded ridges, Veyran saw teeth.
        let ridges = if lod.has(1) { 3 } else { 1 };
        for k in 0..ridges {
            let z = -3600
                + k * 3000 * if lod.has(1) { 1 } else { 0 }
                + if lod.has(1) { 0 } else { 2900 };
            m.add(match f {
                F::M => lathe_x(
                    &[(500, -2500), (500, 1500)],
                    lod.segs(8),
                    &[ACCENT],
                    Some(PAINT2),
                    Some(PAINT2),
                )
                .moved([-700, top, z]),
                F::V => extrude_z1(
                    &[[-3200, top], [800, top], [800, top + 900]],
                    z - 500,
                    z + 500,
                    PAINT2,
                ),
            });
        }
        // The gantry over the door (all levels; it sets the height).
        m.add(closed_box(
            [2700, sh, -5900],
            [3200, top + 1200, -5400],
            PAINT2,
        ));
        m.add(closed_box(
            [2700, sh, 4400],
            [3200, top + 1200, 4900],
            PAINT2,
        ));
        m.add(closed_box(
            [2600, top + 1200, -5900],
            [3300, top + 1600, 4900],
            PAINT,
        ));
        m.add(roof_unit(f, lod, -3300, atop, 6000, 900));
        m.add(railing_rect(lod, -5400, -7300, 5400, -6500, sh));
        if lod.has(0) {
            m.add(closed_box(
                [2850, top + 1000, -800],
                [3050, top + 1200, -200],
                METAL,
            ));
            m.add(ladder(lod, 3200, -5650, sh, top + 1200));
        }
        built(
            m,
            c,
            None,
            [-700, top + 300, -500],
            [-2800, atop, 6000],
            (hx, hz, sh),
        )
    })
}

/// Radar (2 x 2): an operations hut and a large dish on a mast.
pub fn radar(f: F) -> Model {
    structure(&format!("{}_radar", f.name()), f, |lod| {
        let (sh, hx, hz) = (300, 3700, 3700);
        let (mut m, c) = slab(f, lod, hx, hz, sh);
        let (h, top) = hall(f, lod, -1500, -1500, 1800, 1700, sh, 2400);
        m.add(h);
        m.add(dish(f, lod, 1300, sh, 1300, 2300, 2600));
        m.add(antenna(lod, -2600, top, -2400, 2000));
        m.add(roof_unit(f, lod, -1500, top, -1100, 800));
        m.add(railing_rect(lod, -3500, -3500, 3500, 3500, sh));
        built(
            m,
            c,
            None,
            [-1500, top + 300, -1500],
            [1300, 3200, 1300],
            (hx, hz, sh),
        )
    })
}

/// Defence tower (1 x 1): a bunker with a gun head (Meridian; the Veyran one is the WP11.6a model).
pub fn defence_tower(f: F) -> Model {
    structure(&format!("{}_defence_tower", f.name()), f, |lod| {
        let mut m = lathe(
            &[
                (1800, 0),
                (1750, 300),
                (1500, 1600),
                (1350, 1900),
                (1100, 2050),
            ],
            lod.segs(16),
            &[PAINT2, PAINT, PAINT, PAINT],
            Some(PAINT2),
            None,
        );
        m.add(team_frame(
            &ngon(1100, lod.segs(16), true),
            &ngon(880, lod.segs(16), true),
            2050,
            2080,
            lod,
            PAINT2,
        ));
        let mut c = Vec::new();
        let segs = lod.segs(16);
        c.push(team_frame(
            &ngon(1770, segs, true),
            &ngon(1530, segs, true),
            300,
            330,
            lod,
            PAINT2,
        ));
        c.push(team_frame(
            &ngon(1660, segs, true),
            &ngon(1420, segs, true),
            950,
            980,
            lod,
            PAINT2,
        ));
        for k in 0..4u8 {
            c.push(team_strip([1450, 300, -400], [1700, 318, 400], lod).turned_y(k));
        }
        if lod.has(0) {
            for k in 0..16 {
                let [x, z] = crate::kit::on_circle(1600, k, 16);
                m.add(bolt([x, 1000, z], 40, 20, 4, METAL).moved([0, 0, 0]));
                m.add(bolt([x * 104 / 100, 320, z * 104 / 100], 40, 20, 4, METAL));
            }
            for k in 0..4u8 {
                m.add(
                    closed_box([1500, 600, -500], [1620, 820, 500], RUBBER)
                        .toned(RECESS)
                        .turned_y(k),
                );
            }
            m.add(ladder(lod, -1700, 0, 0, 1900).turned_y(2));
        }
        let t = turret(f, lod, [0, 2080, 0], 900, 1200, false);
        built(
            m,
            c,
            Some(t),
            [0, 2600, 0],
            [600, 1600, 600],
            (1700, 1700, 300),
        )
    })
}

/// Advanced defences (2 x 2): an armoured emplacement with a twin-gun turret, an ammunition
/// bunker and a ring of bags or armour blocks.
pub fn advanced_defences(f: F) -> Model {
    structure(&format!("{}_advanced_defences", f.name()), f, |lod| {
        let (sh, hx, hz) = (300, 3700, 3700);
        let (mut m, c) = slab(f, lod, hx, hz, sh);
        m.add(
            match f {
                F::M => lathe(
                    &[(2400, 0), (2300, 600), (1900, 1300), (1600, 1450)],
                    lod.segs(16),
                    &[PAINT, PAINT, PAINT],
                    Some(PAINT2),
                    None,
                ),
                F::V => loft(
                    &[
                        (ngon(2500, lod.segs(8), true), 0),
                        (ngon(2300, lod.segs(8), true), 700),
                        (ngon(1800, lod.segs(8), true), 1450),
                    ],
                    &[PAINT2, PAINT],
                    Some(PAINT2),
                    None,
                ),
            }
            .moved([0, sh, 0]),
        );
        let y = sh + 1450;
        m.add(team_frame(
            &ngon(1600, if f == F::M { lod.segs(16) } else { 8 }, true),
            &ngon(1380, if f == F::M { lod.segs(16) } else { 8 }, true),
            y,
            y + 30,
            lod,
            PAINT2,
        ));
        m.add(bags(f, lod, 0, 0, sh, 3000, 12, 3));
        let (b, _) = hall(f, lod, -2400, 2500, 900, 800, sh, 1500);
        m.add(b);
        if lod.has(0) {
            for k in 0..12 {
                let [x, z] = crate::kit::on_circle(2150, k, 16);
                m.add(bolt([x, sh + 900, z], 45, 25, 4, METAL));
            }
        }
        let t = turret(f, lod, [0, y + 30, 0], 1250, 2100, true);
        built(
            m,
            c,
            Some(t),
            [0, y + 900, 0],
            [-2400, sh + 1500, 2500],
            (hx, hz, sh),
        )
    })
}

/// Shipyard (4 x 5): a workshop hall, a slipway with a hull under construction, a gantry crane
/// over it and bollards along the quay.
pub fn shipyard(f: F) -> Model {
    structure(&format!("{}_shipyard", f.name()), f, |lod| {
        let (sh, hx, hz) = (500, 7400, 9300);
        let (mut m, mut c) = slab(f, lod, hx, hz, sh);
        let (h, top) = hall(f, lod, -4300, -5000, 2800, 3800, sh, 3400);
        m.add(h);
        // The slipway: a dark basin plate with a hull on blocks.
        m.add(cuboid([-800, sh, -2500], [6800, sh + 20, 8500], RUBBER).toned(800));
        c.extend(stripes(-600, 6600, -2300, 8300, sh + 20, 1400, lod));
        c.extend(stripes(-7000, -1600, -800, 3000, sh, 700, lod));
        let hull = match f {
            F::M => lathe_x(
                &[(0, 0), (1200, 1200), (1300, 5200), (900, 6600)],
                lod.segs(8),
                &[PAINT2, PAINT2, PAINT2],
                Some(PAINT2),
                None,
            ),
            F::V => lathe_x(
                &[(0, 0), (1200, 1400), (1200, 5400), (700, 6600)],
                4,
                &[PAINT2, PAINT2, PAINT2],
                Some(PAINT2),
                None,
            ),
        };
        m.add(clip_up(hull).moved([0, sh + 600, 3000]));
        // The gantry crane over the slipway (all levels).
        for x in [-600, 6600] {
            m.add(closed_box(
                [x - 250, sh, -2300],
                [x + 250, sh + 6500, -1800],
                PAINT2,
            ));
            m.add(closed_box(
                [x - 250, sh, 7800],
                [x + 250, sh + 6500, 8300],
                PAINT2,
            ));
            m.add(closed_box(
                [x - 250, sh + 6500, -2300],
                [x + 250, sh + 6900, 8300],
                PAINT,
            ));
            c.push(team_strip(
                [x - 250, sh + 6900, -2200],
                [x + 10, sh + 6918, 8200],
                lod,
            ));
        }
        m.add(closed_box(
            [-850, sh + 6000, 2600],
            [6850, sh + 6500, 3400],
            PAINT,
        ));
        if lod.has(1) {
            m.add(closed_box(
                [2600, sh + 5300, 2700],
                [3400, sh + 6000, 3300],
                METAL,
            ));
            for k in 0..4 {
                m.add(
                    closed_box(
                        [-500, sh, 200 + k * 1800],
                        [6500, sh + 250, 500 + k * 1800],
                        METAL,
                    )
                    .toned(RECESS),
                );
            }
        }
        if lod.has(0) {
            for k in 0..8 {
                let z = -1500 + k * 1300;
                m.add(
                    lathe(
                        &[(170, 0), (170, 400), (230, 460), (230, 520)],
                        8,
                        &[METAL],
                        Some(METAL),
                        None,
                    )
                    .moved([7000, sh, z]),
                );
            }
            m.add(
                cuboid(
                    [3000, sh + 6900, -2200],
                    [3060, sh + 6000 + 900, 8200],
                    METAL,
                )
                .moved([0, 0, 0]),
            );
        }
        m.add(tank(f, lod, -5000, 4500, sh, 1300, 2800));
        if lod.has(1) {
            m.add(tank(f, lod, -5000, 7600, sh, 1100, 2400));
        }
        m.add(door(f, lod, -1500, -5000, sh, 2600, 2400));
        for z in [-7000, -3000] {
            m.add(roof_unit(f, lod, -4300, top, z, 1000));
        }
        if lod.has(0) {
            m.add(railing(
                [-7200, sh, -9100],
                [-1000, sh, -9100],
                600,
                6,
                METAL,
            ));
        }
        built(
            m,
            c,
            None,
            [-4300, top + 500, -5000],
            [3000, sh + 2000, 3000],
            (hx, hz, sh),
        )
    })
}

/// A hull lying along X: keep only its upper half (a slipway hull shows its deck and flanks).
fn clip_up(m: Mesh) -> Mesh {
    crate::stages::clip(
        &m,
        crate::stages::Plane {
            n: [0, -1, 0],
            d: 0,
        },
    )
}

/// Decoy emplacement (Meridian, 1 x 1): a dummy gun under camouflage, sandbags round it.
pub fn decoy_emplacement(f: F) -> Model {
    structure(&format!("{}_decoy_emplacement", f.name()), f, |lod| {
        let (sh, hx, hz) = (200, 1800, 1800);
        let (mut m, mut c) = slab(f, lod, hx, hz, sh);
        m.add(bags(f, lod, 0, 0, sh, 1350, 10, 0));
        c.push(team_frame(
            &ngon(1060, lod.segs(16), true),
            &ngon(820, lod.segs(16), true),
            sh,
            sh + 20,
            lod,
            PAINT2,
        ));
        // The dummy: a wooden frame and a tube.
        m.add(
            match f {
                F::M => lathe(
                    &[(800, 0), (780, 500), (500, 800)],
                    lod.segs(16),
                    &[PAINT2, PAINT2],
                    Some(PAINT2),
                    None,
                ),
                F::V => loft(
                    &[
                        (ngon(800, lod.segs(8), true), 0),
                        (ngon(520, lod.segs(8), true), 800),
                    ],
                    &[PAINT2],
                    Some(PAINT2),
                    None,
                ),
            }
            .moved([0, sh, 0]),
        );
        m.add(team_frame(
            &ngon(520, if f == F::M { lod.segs(16) } else { 8 }, true),
            &ngon(320, if f == F::M { lod.segs(16) } else { 8 }, true),
            sh + 800,
            sh + 830,
            lod,
            PAINT2,
        ));
        // A camouflage net on posts (the posts in every level: they set the height).
        for (x, z) in [(-1200, -1200), (1200, -1200), (-1200, 1200), (1200, 1200)] {
            m.add(cuboid(
                [x - 40, sh, z - 40],
                [x + 40, sh + 1500, z + 40],
                METAL,
            ));
        }
        if lod.has(0) {
            for k in 0..6 {
                let z = -1200 + k * 480;
                c.push(cuboid(
                    [-1200, sh + 1500, z],
                    [1200, sh + 1520, z + 200],
                    TEAM,
                ));
            }
        }
        let t = turret(f, lod, [0, sh + 830, 0], 600, 1000, false);
        built(
            m,
            c,
            Some(t),
            [0, sh + 1400, 0],
            [700, sh + 600, 700],
            (hx, hz, sh),
        )
    })
}

/// Fortified bunker (Veyran, 2 x 2): a low sloped bunker with firing slits and armoured doors.
pub fn fortified_bunker(f: F) -> Model {
    structure(&format!("{}_fortified_bunker", f.name()), f, |lod| {
        let (sh, hx, hz) = (250, 3700, 3700);
        let (mut m, mut c) = slab(f, lod, hx, hz, sh);
        let (h, top) = hall(f, lod, 0, 0, 3000, 3000, sh, 2600);
        c.extend(stripes(-1300, 1300, -1300, 1300, top + 40, 650, lod));
        m.add(h);
        if lod.has(1) {
            for k in 0..4u8 {
                m.add(
                    closed_box([3000, sh + 1200, -1400], [3080, sh + 1400, 1400], RUBBER)
                        .toned(RECESS)
                        .turned_y(k),
                );
                m.add(
                    closed_box([3000, sh + 1150, -1500], [3120, sh + 1180, 1500], ACCENT)
                        .turned_y(k),
                );
            }
        }
        m.add(roof_unit(f, lod, -1000, top, -1000, 900));
        m.add(antenna(lod, 1500, top, -1500, 1600));
        if lod.has(0) {
            for k in 0..4u8 {
                for i in 0..6 {
                    m.add(
                        extrude_z1(
                            &[[3000, sh], [3250, sh], [3250, sh + 260], [3000, sh + 480]],
                            -2600 + i * 900,
                            -2200 + i * 900,
                            PAINT2,
                        )
                        .turned_y(k),
                    );
                }
            }
        }
        built(
            m,
            c,
            None,
            [-1000, top + 300, -1000],
            [1000, top, 1000],
            (hx, hz, sh),
        )
    })
}

/// Ore silo (2 x 2): two silos joined by a bridge with a conveyor feed.
pub fn ore_silo(f: F) -> Model {
    structure(&format!("{}_ore_silo", f.name()), f, |lod| {
        let (sh, hx, hz) = (300, 3700, 3700);
        let (mut m, c) = slab(f, lod, hx, hz, sh);
        for z in [-1600, 1600] {
            m.add(tank(f, lod, -800, z, sh, 1500, 4200));
        }
        m.add(closed_box(
            [-1200, sh + 3600, -700],
            [-400, sh + 4000, 700],
            PAINT2,
        ));
        for k in 0..if lod.has(1) { 5 } else { 1 } {
            let x = 900 + k * 500;
            let y = sh + 3000 - k * 600;
            m.add(closed_box(
                [x - 300, y, -300],
                [x + 300, y + 180, 300],
                PAINT2,
            ));
        }
        m.add(pad(f, lod, 2400, sh, 0, 1100, 2000));
        if lod.has(0) {
            m.add(railing_rect(lod, -1200, -700, -400, 700, sh + 4000));
            for z in [-1600, 1600] {
                m.add(hatch([-800, sh + 4300, z], 300, 8, PAINT2));
            }
        }
        built(
            m,
            c,
            None,
            [-800, sh + 4300, -1600],
            [-800, sh + 2000, 1600],
            (hx, hz, sh),
        )
    })
}
