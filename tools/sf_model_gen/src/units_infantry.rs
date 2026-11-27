//! The trial rifleman of both factions in style G (WP11.6i, docs/art-style.md "Figures (WP11.6i
//! trial)"): one figure per faction before the rest of the infantry, so Tijmen can judge the
//! style first.
//!
//! The figure has realistic proportions (about 7.5 heads, 1.8 m to the top of the helmet) and its
//! detail is real geometry: helmet, webbing and pouches, boots, a backpack and a rifle. The rifle
//! and the helmet are about 20 % oversized, like the vehicle turrets, so they stay readable at the
//! far view. Meridian is rounded (turned limbs, a round helmet, sand and olive); Veyran is
//! angular (faceted limbs, an angular helmet, grey-blue and gunmetal, the small crimson accent).
//!
//! The six figure nodes (`torso`, `head`, `arm_l`, `arm_r`, `leg_l`, `leg_r`) sit directly under
//! the root like the set v3 figures, with the rifle in `arm_r` as in set v3. Unlike set v3, each
//! node's origin is at its **joint** ([`JOINTS`]): the legs at the hip, the arms at the shoulder,
//! the head at the neck and the torso at the waist, so the renderer's walk cycle can turn a limb
//! about its joint (`sf_render::walk`). Team colour: the helmet's top patch and band, a pad on each
//! shoulder and a chest panel.

use crate::buildings::F;
use crate::kit::{
    Lod, Mat, Mesh, Model, Node, chamfer_rect, closed_box, cuboid, lathe, lathe_x, loft, on_circle,
};
use crate::models::team_strip;
use crate::palette::{Palette, slot::*};

/// The joint of every figure node in the model's frame (mm): `(node, origin)`. The legs hinge at
/// the hip, the arms at the shoulder, the head at the neck, the torso at the waist. Pinned by
/// `tests/infantry.rs`; the renderer only animates a figure whose nodes sit at such joints.
pub const JOINTS: [(&str, [i32; 3]); 6] = [
    ("leg_l", [0, HIP_Y, -HIP_Z]),
    ("leg_r", [0, HIP_Y, HIP_Z]),
    ("torso", [0, WAIST_Y, 0]),
    ("head", [0, NECK_Y, 0]),
    ("arm_l", [0, SHOULDER_Y, -SHOULDER_Z]),
    ("arm_r", [0, SHOULDER_Y, SHOULDER_Z]),
];

/// Height of the hip joint above the ground (mm).
pub const HIP_Y: i32 = 900;
/// Half the distance between the hip joints.
pub const HIP_Z: i32 = 105;
/// The waist (the torso's origin).
pub const WAIST_Y: i32 = 960;
/// The neck (the head's origin).
pub const NECK_Y: i32 = 1450;
/// The shoulder joints.
pub const SHOULDER_Y: i32 = 1400;
/// Half the distance between the shoulder joints.
pub const SHOULDER_Z: i32 = 220;

/// The trial rifleman of Meridian.
pub fn meridian_rifleman() -> Model {
    rifleman(F::M)
}

/// The trial rifleman of Veyran.
pub fn veyran_rifleman() -> Model {
    rifleman(F::V)
}

/// A horizontal ring: an oval of half sizes `rx`, `rz` in `segs` steps (Meridian) or a chamfered
/// rectangle (Veyran, angular).
fn ring(f: F, rx: i32, rz: i32, segs: u32) -> Vec<[i32; 2]> {
    match f {
        F::M => (0..segs as i32)
            .map(|k| [on_circle(rx, k, segs)[0], on_circle(rz, k, segs)[1]])
            .collect(),
        F::V => chamfer_rect(rx, rz, rx.min(rz) / 3),
    }
}

/// A body of horizontal rings `(rx, rz, y)` from bottom to top, capped at both ends; turned
/// (smooth normals) for Meridian, faceted for Veyran.
fn body(f: F, rings: &[(i32, i32, i32)], segs: u32, mat: Mat) -> Mesh {
    let r: Vec<(Vec<[i32; 2]>, i32)> = rings
        .iter()
        .map(|&(rx, rz, y)| (ring(f, rx, rz, segs), y))
        .collect();
    let m = loft(&r, &vec![mat; r.len() - 1], Some(mat), Some(mat));
    match f {
        F::M => m.lathed(),
        F::V => m,
    }
}

/// One leg in its hip frame (the ground at `-HIP_Y`), the left one at `z < 0` mirrored by the
/// caller's node position only (the leg itself is symmetric about its own axis).
fn leg(f: F, lod: Lod) -> Mesh {
    let g = -HIP_Y;
    let (cloth, armour) = match f {
        F::M => (PAINT, PAINT2),
        F::V => (PAINT, PAINT2),
    };
    let mut m = Mesh::new();
    if lod.has(0) {
        // Thigh to ankle, its top tucked into the torso so a swing opens no gap.
        m.add(body(
            f,
            &[
                (52, 50, g + 110),
                (70, 64, g + 430),
                (88, 82, g + 720),
                (92, 86, 40),
            ],
            16,
            cloth,
        ));
        // Boot: sole and upper, the toe forward.
        m.add(closed_box([-75, g, -62], [165, g + 50, 62], RUBBER));
        m.add(closed_box([-70, g + 50, -58], [120, g + 150, 58], RUBBER));
        // Knee pad (Meridian) or a knee and shin plate (Veyran).
        match f {
            F::M => m.add(closed_box([55, g + 400, -45], [88, g + 520, 45], armour)),
            F::V => {
                m.add(closed_box([58, g + 410, -50], [92, g + 530, 50], armour));
                m.add(closed_box([52, g + 180, -38], [74, g + 400, 38], armour))
            }
        };
    } else if lod.has(1) {
        m.add(closed_box([-80, g + 120, -75], [85, 40, 75], cloth));
        m.add(closed_box([-75, g, -62], [165, g + 140, 62], RUBBER));
    } else {
        // Leg and boot in one block: the boot's toe keeps the footprint.
        m.add(cuboid([-80, g, -75], [140, 40, 75], cloth));
    }
    m
}

/// The torso in its waist frame: chest, belt, webbing, pouches, backpack and the chest panel.
fn torso(f: F, lod: Lod) -> Mesh {
    let top = SHOULDER_Y - WAIST_Y + 40; // the shoulder line
    let mut m = Mesh::new();
    let (jacket, kit) = match f {
        F::M => (PAINT2, METAL),
        F::V => (PAINT, PAINT2),
    };
    if lod.has(0) {
        let rings: &[(i32, i32, i32)] = match f {
            F::M => &[
                (112, 172, -120),
                (108, 165, 120),
                (128, 192, 300),
                (118, 196, top - 40),
                (70, 120, top),
            ],
            F::V => &[
                (110, 170, -120),
                (106, 162, 120),
                (130, 196, 300),
                (122, 200, top - 40),
                (75, 125, top),
            ],
        };
        m.add(body(f, rings, 16, jacket));
        // Belt and three pouches on the front.
        m.add(body(f, &[(122, 180, -60), (122, 180, 10)], 16, RUBBER));
        for i in 0..3 {
            let z = -110 + i * 110;
            m.add(closed_box([105, -70, z - 42], [165, 50, z + 42], kit));
        }
        // Webbing: two straps from the belt over the shoulders, front and back.
        for z in [-95, 95] {
            m.add(closed_box([100, 10, z - 18], [138, top - 30, z + 18], kit));
            m.add(closed_box(
                [-132, 10, z - 18],
                [-104, top - 30, z + 18],
                kit,
            ));
        }
        // Backpack with its flap and a rolled mat on top.
        m.add(closed_box([-255, 110, -150], [-112, 430, 150], kit));
        m.add(closed_box([-262, 380, -140], [-140, 440, 140], jacket));
        m.add(
            crate::kit::lathe_z(&[(55, 0), (55, 300)], 16, &[RUBBER], Some(RUBBER), None)
                .moved([-190, 495, -150]),
        );
        // The chest panel (team), and Veyran's chest plate with its crimson mark.
        m.add(closed_box([128, 210, -70], [146, 330, 70], TEAM));
        if f == F::V {
            m.add(closed_box([118, 120, -150], [140, 200, 150], PAINT2));
            m.add(closed_box([132, 350, -150], [150, 390, -100], ACCENT));
        }
    } else if lod.has(1) {
        m.add(closed_box([-120, -120, -190], [125, top, 190], jacket));
        m.add(closed_box([-255, 110, -150], [-118, 440, 150], kit));
        m.add(closed_box([125, 210, -70], [145, 330, 70], TEAM));
    } else {
        // Torso, backpack and the arms' width in one block (the arms are pads at LOD2).
        m.add(cuboid([-255, -120, -280], [140, top, 280], jacket));
    }
    m
}

/// The head in its neck frame: neck, face and the helmet (about 20 % oversized) with its team
/// band and top patch.
fn head(f: F, lod: Lod) -> Mesh {
    let mut m = Mesh::new();
    let face = match f {
        F::M => PAINT,
        F::V => METAL,
    };
    if lod.has(0) {
        m.add(lathe(&[(48, -20), (52, 70)], 16, &[face], None, None));
        match f {
            F::M => {
                m.add(lathe(
                    &[(60, 50), (86, 110), (96, 190), (92, 250)],
                    16,
                    &[face],
                    Some(face),
                    None,
                ));
                // Round helmet: rim, team band, dome, team patch on top; goggles in front.
                m.add(lathe(
                    &[
                        (156, 170),
                        (162, 196),
                        (162, 226),
                        (148, 290),
                        (100, 345),
                        (78, 356),
                        (0, 368),
                    ],
                    16,
                    &[PAINT2, TEAM, PAINT2, PAINT2, PAINT2, TEAM],
                    None,
                    Some(PAINT2),
                ));
                m.add(closed_box([150, 236, -80], [176, 272, 80], ACCENT));
            }
            F::V => {
                // A faceted face guard, then the angular helmet with a flat top.
                m.add(body(
                    f,
                    &[(78, 70, 50), (96, 88, 140), (92, 86, 250)],
                    16,
                    face,
                ));
                m.add(body(
                    f,
                    &[
                        (168, 150, 165),
                        (172, 156, 200),
                        (172, 156, 228),
                        (150, 138, 300),
                        (102, 96, 352),
                    ],
                    16,
                    PAINT2,
                ));
                // Team: a band round the helmet and the flat top patch.
                m.add(body(f, &[(174, 158, 200), (174, 158, 226)], 16, TEAM));
                m.add(loft(
                    &[
                        (chamfer_rect(78, 74, 24), 352),
                        (chamfer_rect(74, 70, 22), 362),
                    ],
                    &[TEAM],
                    Some(TEAM),
                    None,
                ));
                // The visor slit and the small crimson mark on the side.
                m.add(closed_box([160, 140, -70], [184, 168, 70], RUBBER));
                m.add(closed_box([-40, 240, 150], [40, 280, 162], ACCENT));
            }
        }
    } else if lod.has(1) {
        m.add(closed_box([-85, -20, -80], [95, 240, 80], face));
        m.add(lathe(
            &[(158, 170), (160, 226), (100, 345), (0, 368)],
            4,
            &[PAINT2, PAINT2, TEAM],
            None,
            None,
        ));
    } else {
        m.add(cuboid([-150, -20, -150], [150, 340, 150], PAINT2));
        m.add(team_strip([-85, 360, -85], [85, 362, 85], lod));
    }
    m
}

/// An arm in its shoulder frame, hanging down; `rifle` adds the forearm held forward and the
/// rifle (about 20 % oversized) in the hand. The shoulder pad on top is team colour.
fn arm(f: F, lod: Lod, rifle: bool) -> Mesh {
    let (sleeve, hand) = match f {
        F::M => (PAINT2, PAINT),
        F::V => (PAINT, RUBBER),
    };
    let mut m = Mesh::new();
    if lod.has(0) {
        let elbow = if rifle { -290 } else { -560 };
        m.add(body(
            f,
            &[(50, 48, elbow), (58, 56, -200), (64, 62, 40)],
            16,
            sleeve,
        ));
        if rifle {
            // Forearm forward, the hand round the grip.
            m.add(
                lathe_x(&[(48, -20), (42, 230)], 16, &[sleeve], Some(sleeve), None)
                    .moved([0, -290, 0]),
            );
            m.add(closed_box([220, -340, -40], [300, -250, 40], hand));
        } else {
            m.add(closed_box([-40, -700, -36], [50, -560, 36], hand));
        }
        m.add(team_strip([-70, 40, -60], [72, 62, 60], lod));
    } else if lod.has(1) {
        if rifle {
            m.add(closed_box([-60, -300, -58], [60, 40, 58], sleeve));
            m.add(closed_box([0, -340, -44], [300, -250, 44], sleeve));
        } else {
            m.add(closed_box([-60, -700, -58], [60, 40, 58], sleeve));
        }
        m.add(team_strip([-70, 40, -60], [72, 62, 60], lod));
    } else {
        // The arm is in the torso block; the pad is the arm.
        m.add(team_strip([-70, 40, -60], [70, 42, 60], lod));
    }
    if rifle {
        m.add(rifle_mesh(f, lod));
    }
    m
}

/// The rifle in the right arm's frame, along `+X` at the hand.
fn rifle_mesh(f: F, lod: Lod) -> Mesh {
    let (y, z) = (-300, -15);
    let stock = match f {
        F::M => RUBBER,
        F::V => PAINT2,
    };
    let mut m = Mesh::new();
    if lod.has(0) {
        m.add(closed_box(
            [-340, y - 60, z - 24],
            [-130, y + 30, z + 24],
            stock,
        ));
        m.add(closed_box(
            [-130, y - 40, z - 26],
            [190, y + 42, z + 26],
            METAL,
        ));
        m.add(closed_box(
            [40, y - 150, z - 18],
            [100, y - 40, z + 18],
            METAL,
        ));
        m.add(closed_box(
            [190, y - 30, z - 28],
            [430, y + 34, z + 28],
            stock,
        ));
        m.add(lathe_x(&[(16, 0), (16, 360)], 16, &[METAL], Some(METAL), None).moved([430, y, z]));
        m.add(closed_box(
            [-40, y + 42, z - 14],
            [110, y + 66, z + 14],
            METAL,
        ));
        m.add(closed_box(
            [700, y + 16, z - 6],
            [730, y + 56, z + 6],
            METAL,
        ));
    } else if lod.has(1) {
        m.add(closed_box(
            [-340, y - 60, z - 26],
            [190, y + 42, z + 26],
            METAL,
        ));
        m.add(closed_box(
            [190, y - 30, z - 28],
            [790, y + 34, z + 28],
            stock,
        ));
    } else {
        m.add(cuboid([-340, y - 60, z - 28], [790, y + 42, z + 28], METAL));
    }
    m
}

fn rifleman(f: F) -> Model {
    let pal = Palette::shipped();
    Model::new(
        &format!("{}_rifleman", f.name()),
        pal.materials(f.name()),
        |lod| {
            let at = |name: &str| {
                JOINTS
                    .iter()
                    .find(|j| j.0 == name)
                    .map(|j| j.1)
                    .expect("a joint")
            };
            vec![
                Node::part("leg_l", at("leg_l"), leg(f, lod)),
                Node::part("leg_r", at("leg_r"), leg(f, lod)),
                Node::part("torso", at("torso"), torso(f, lod)),
                Node::part("head", at("head"), head(f, lod)),
                Node::part("arm_l", at("arm_l"), arm(f, lod, false)),
                Node::part("arm_r", at("arm_r"), arm(f, lod, true)),
            ]
        },
    )
}
