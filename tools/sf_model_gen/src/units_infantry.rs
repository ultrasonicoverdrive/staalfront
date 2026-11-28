//! The trial rifleman of both factions in style G (WP11.6i, docs/art-style.md section 13): one
//! figure per faction before the rest of the infantry, so Tijmen can judge the style first.
//!
//! Round 2 (Tijmen 28-11-2026, "a real step towards realistic little soldiers"): anatomy with real
//! cross-sections (tapered thighs, calves and arms on 16-sided rings in LOD0 and 8- or 6-sided in
//! LOD1, never a square post), shoulders wider than the hips, a neck, a head with a jaw and a nose
//! under the helmet, hands, boots with a sole and a toe, a jacket with a hem, a belt, pockets,
//! pouches and a pack. Meridian is rounded in every level (oval rings, a round helmet with a brim,
//! soft shoulders, rolled sleeves); Veyran is angular in every level (faceted rings, a faceted
//! helmet with a visor line, hard shoulder plates, gloves). The colours are the figures' own
//! palette entries (`meridian_figure`, `veyran_figure` in `model_palette.ron`), lighter than the
//! vehicles' so the figures read sand/olive and cool grey-blue in the game.
//!
//! **Nodes.** Eight figure nodes directly under the root, each with its origin at its joint
//! ([`JOINTS`]): `torso` (waist), `head` (neck), `arm_l`/`arm_r` (shoulders, the rifle in `arm_r`
//! as in set v3), `leg_l`/`leg_r` (the thighs, at the hips; the names the checker requires) and
//! `shin_l`/`shin_r` (at the knees, with the boot). The shins are not children of the thighs: every
//! figure part stays directly under the root (the loader's figure rule and the checker's flat
//! figure tree stay as they are), and the renderer poses a shin from its thigh's angle
//! (`sf_render::model::pose_parts`). Team colour: the helmet cover and band, an armband on each
//! upper arm, a pad or plate on each shoulder and a chest panel.

use crate::buildings::F;
use crate::kit::{
    Lod, Mat, Mesh, Model, Node, chamfer_rect, closed_box, cuboid, lathe, lathe_x, loft, on_circle,
};
use crate::models::team_strip;
use crate::palette::{Palette, slot::*};

/// Skin: the figures use the accent slot for it (`model_palette.ron`, `*_figure`).
const SKIN: Mat = ACCENT;

/// The joint of every figure node in the model's frame (mm). Pinned by `tests/infantry.rs`; the
/// renderer only walks a figure whose eight nodes sit at such joints (`glb::figure_joints`).
pub const JOINTS: [(&str, [i32; 3]); 8] = [
    ("leg_l", [0, HIP_Y, -HIP_Z]),
    ("leg_r", [0, HIP_Y, HIP_Z]),
    ("shin_l", [0, KNEE_Y, -HIP_Z]),
    ("shin_r", [0, KNEE_Y, HIP_Z]),
    ("torso", [0, WAIST_Y, 0]),
    ("head", [0, NECK_Y, 0]),
    ("arm_l", [0, SHOULDER_Y, -SHOULDER_Z]),
    ("arm_r", [0, SHOULDER_Y, SHOULDER_Z]),
];

/// Height of the hip joints above the ground (mm).
pub const HIP_Y: i32 = 920;
/// Half the distance between the hip joints.
pub const HIP_Z: i32 = 100;
/// The knees (the shins' origins).
pub const KNEE_Y: i32 = 495;
/// The waist (the torso's origin).
pub const WAIST_Y: i32 = 980;
/// The neck (the head's origin).
pub const NECK_Y: i32 = 1490;
/// The shoulder joints.
pub const SHOULDER_Y: i32 = 1420;
/// Half the distance between the shoulder joints.
pub const SHOULDER_Z: i32 = 215;

/// The trial rifleman of Meridian.
pub fn meridian_rifleman() -> Model {
    rifleman(F::M)
}

/// The trial rifleman of Veyran.
pub fn veyran_rifleman() -> Model {
    rifleman(F::V)
}

/// A horizontal ring of half sizes `rx` (front-back) and `rz` (sideways): Meridian an oval of 16
/// (LOD0) or 8 (LOD1) points, Veyran a facetted octagon (LOD0) or hexagon (LOD1).
fn ring(f: F, lod: Lod, rx: i32, rz: i32) -> Vec<[i32; 2]> {
    match (f, lod.has(0)) {
        (F::M, near) => {
            let segs = if near { 16 } else { 8 };
            (0..segs as i32)
                .map(|k| [on_circle(rx, k, segs)[0], on_circle(rz, k, segs)[1]])
                .collect()
        }
        (F::V, true) => chamfer_rect(rx, rz, rx.min(rz) * 2 / 5),
        (F::V, false) => vec![
            [rx, -rz / 2],
            [rx, rz / 2],
            [0, rz],
            [-rx, rz / 2],
            [-rx, -rz / 2],
            [0, -rz],
        ],
    }
}

/// A body of horizontal rings `(rx, rz, y)` from bottom to top, capped at both ends; turned
/// (smooth normals) for Meridian, faceted for Veyran.
fn body(f: F, lod: Lod, rings: &[(i32, i32, i32)], mat: Mat) -> Mesh {
    let r: Vec<(Vec<[i32; 2]>, i32)> = rings
        .iter()
        .map(|&(rx, rz, y)| (ring(f, lod, rx, rz), y))
        .collect();
    let m = loft(&r, &vec![mat; r.len() - 1], Some(mat), Some(mat));
    match f {
        F::M => m.lathed(),
        F::V => m,
    }
}

/// LOD0 keeps every ring; LOD1 the first and the last (a tapered section), and the middle one
/// too for a body of six rings or more (the torso).
fn rings_for(lod: Lod, all: &[(i32, i32, i32)]) -> Vec<(i32, i32, i32)> {
    if lod.has(0) {
        all.to_vec()
    } else if all.len() >= 6 {
        vec![all[0], all[all.len() / 2], all[all.len() - 1]]
    } else {
        vec![all[0], all[all.len() - 1]]
    }
}

/// A thigh in its hip frame: from inside the hip down to the knee.
fn thigh(f: F, lod: Lod) -> Mesh {
    let knee = KNEE_Y - HIP_Y;
    if !lod.has(1) {
        return cuboid([-80, knee - 10, -72], [80, 60, 72], PAINT);
    }
    let rings = [
        (60, 56, knee - 20),
        (68, 64, knee + 70),
        (84, 80, knee + 230),
        (94, 88, -40),
        (96, 90, 60),
    ];
    let mut m = body(f, lod, &rings_for(lod, &rings), PAINT);
    if lod.has(0) {
        // A cargo pocket on the outer side of the thigh.
        m.add(closed_box([-40, -260, 70], [50, -130, 98], PAINT));
    }
    m
}

/// A shin in its knee frame: knee, calf, ankle and the boot on the ground (`-KNEE_Y`).
fn shin(f: F, lod: Lod) -> Mesh {
    let g = -KNEE_Y;
    if !lod.has(1) {
        let mut m = cuboid([-62, g + 90, -56], [62, 20, 56], PAINT);
        m.add(cuboid([-75, g, -58], [165, g + 90, 58], RUBBER));
        return m;
    }
    let rings = [
        (44, 42, g + 150),
        (48, 46, g + 230),
        (64, 58, g + 330),
        (60, 56, -60),
        (62, 58, 30),
    ];
    let mut m = body(f, lod, &rings_for(lod, &rings), PAINT);
    // Boot: upper round the ankle, sole with a toe in front.
    let upper = [(58, 54, g + 40), (52, 48, g + 120), (50, 47, g + 170)];
    m.add(body(f, lod, &rings_for(lod, &upper), RUBBER).moved([10, 0, 0]));
    m.add(closed_box([-72, g, -56], [128, g + 45, 56], RUBBER));
    if lod.has(0) {
        m.add(body(f, lod, &[(42, 50, g + 10), (40, 48, g + 70)], RUBBER).moved([128, 0, 0]));
        match f {
            F::M => m.add(closed_box([40, -40, -50], [72, 70, 50], PAINT2)),
            F::V => {
                m.add(closed_box([44, -50, -54], [80, 80, 54], PAINT2));
                m.add(closed_box([46, g + 200, -36], [70, -70, 36], PAINT2))
            }
        };
    }
    m
}

/// The torso in its waist frame: hips, waist, chest and shoulders, jacket hem, belt, pockets,
/// webbing, pouches, the pack and the chest panel.
fn torso(f: F, lod: Lod) -> Mesh {
    let top = SHOULDER_Y - WAIST_Y + 40;
    let mut m = Mesh::new();
    if !lod.has(1) {
        m.add(cuboid([-150, -110, -205], [140, top, 205], PAINT));
        m.add(cuboid([-270, 120, -150], [-150, 440, 150], PAINT2));
        m.add(team_strip([136, 230, -60], [138, 330, 60], lod));
        return m;
    }
    let rings: &[(i32, i32, i32)] = &[
        (120, 165, -120),
        (114, 158, 40),
        (126, 186, 220),
        (130, 204, 360),
        (112, 212, top - 30),
        (64, 96, top),
    ];
    m.add(body(f, lod, &rings_for(lod, rings), PAINT));
    // Jacket hem and belt.
    if lod.has(0) {
        m.add(body(f, lod, &[(126, 171, -120), (124, 168, -60)], PAINT));
    }
    m.add(body(f, lod, &[(122, 166, -20), (122, 166, 40)], RUBBER));
    // The pack, rounded (Meridian) or boxy (Veyran), with its flap.
    let pack = [
        (62, 140, 120),
        (72, 150, 200),
        (72, 150, 380),
        (56, 132, 440),
    ];
    m.add(body(f, lod, &rings_for(lod, &pack), PAINT2).moved([-200, 0, 0]));
    // Team: the chest panel.
    m.add(closed_box([124, 230, -64], [142, 330, 64], TEAM));
    if lod.has(0) {
        m.add(body(f, lod, &[(80, 150, 420), (70, 136, 460)], PAINT2).moved([-200, 0, 0]));
        // Chest pockets with flaps.
        for z in [-120, 120] {
            m.add(closed_box([118, 140, z - 45], [140, 220, z + 45], PAINT));
            m.add(closed_box([120, 214, z - 48], [144, 232, z + 48], PAINT2));
        }
        // Pouches on the belt, webbing straps front and back.
        for z in [-120, 0, 120] {
            m.add(closed_box([110, -60, z - 40], [168, 50, z + 40], PAINT2));
        }
        for z in [-90, 90] {
            m.add(closed_box(
                [108, 40, z - 18],
                [138, top - 30, z + 18],
                PAINT2,
            ));
            m.add(closed_box(
                [-134, 40, z - 18],
                [-110, top - 30, z + 18],
                PAINT2,
            ));
        }
        // A rolled mat on the pack.
        m.add(
            crate::kit::lathe_z(&[(52, 0), (52, 280)], 16, &[RUBBER], Some(RUBBER), None)
                .moved([-200, 492, -140]),
        );
    }
    m
}

/// The head in its neck frame: neck, head with jaw and nose, and the helmet with its team cover
/// and band (about 20 % oversized).
fn head(f: F, lod: Lod) -> Mesh {
    let mut m = Mesh::new();
    if !lod.has(1) {
        m.add(cuboid([-80, -20, -76], [96, 230, 76], SKIN));
        match f {
            F::M => m.add(lathe(
                &[(176, 186), (150, 214), (0, 362)],
                4,
                &[PAINT2],
                None,
                None,
            )),
            F::V => m.add(cuboid([-160, 190, -150], [165, 352, 150], PAINT2)),
        };
        m.add(team_strip([-80, 364, -80], [80, 366, 80], lod));
        return m;
    }
    m.add(body(f, lod, &[(48, 46, -30), (50, 48, 70)], SKIN));
    // The head: chin and jaw, cheeks, crown (under the helmet).
    let face = [
        (56, 50, 40),
        (84, 70, 100),
        (96, 80, 170),
        (94, 82, 230),
        (76, 70, 280),
    ];
    m.add(body(f, lod, &rings_for(lod, &face), SKIN).moved([8, 0, 0]));
    if lod.has(0) {
        m.add(closed_box([96, 140, -14], [122, 196, 14], SKIN));
    }
    match f {
        F::M => {
            // Round helmet: a brim, the team band, the dome and the team cover on top.
            m.add(lathe(
                &[
                    (178, 190),
                    (176, 202),
                    (152, 208),
                    (154, 228),
                    (152, 262),
                    (130, 312),
                    (92, 346),
                    (0, 366),
                ],
                if lod.has(0) { 16 } else { 8 },
                &[PAINT2, PAINT2, PAINT2, TEAM, PAINT2, PAINT2, TEAM],
                None,
                Some(PAINT2),
            ));
        }
        F::V => {
            // Faceted helmet: rim, team band, sloped facets, a flat team top and a visor line.
            m.add(body(
                f,
                lod,
                &rings_for(
                    lod,
                    &[
                        (156, 146, 190),
                        (166, 154, 214),
                        (164, 152, 290),
                        (136, 128, 330),
                        (96, 92, 356),
                    ],
                ),
                PAINT2,
            ));
            m.add(body(f, lod, &[(169, 157, 226), (169, 157, 262)], TEAM));
            m.add(loft(
                &[(ring(f, lod, 96, 92), 356), (ring(f, lod, 82, 78), 368)],
                &[TEAM],
                Some(TEAM),
                None,
            ));
            m.add(closed_box([150, 168, -96], [176, 186, 96], RUBBER));
        }
    }
    m
}

/// An arm in its shoulder frame, hanging down; `rifle` bends the forearm forward and puts the
/// rifle (about 20 % oversized) in the hand. Team: an armband and a pad (Meridian) or plate
/// (Veyran) on the shoulder.
fn arm(f: F, lod: Lod, rifle: bool) -> Mesh {
    let (sleeve, forearm, hand) = match f {
        // Meridian rolls its sleeves: the forearm is skin.
        F::M => (PAINT, SKIN, SKIN),
        F::V => (PAINT, PAINT, RUBBER),
    };
    let mut m = Mesh::new();
    let elbow = -300;
    if !lod.has(1) {
        if rifle {
            m.add(cuboid([-56, elbow, -52], [56, 40, 52], sleeve));
        } else {
            m.add(cuboid([-56, -650, -52], [56, 40, 52], sleeve));
        }
        m.add(team_strip([-62, 80, -58], [62, 82, 58], lod));
    } else {
        let upper = [
            (48, 46, elbow - 10),
            (54, 52, -150),
            (60, 58, -40),
            (62, 60, 30),
        ];
        m.add(body(f, lod, &rings_for(lod, &upper), sleeve));
        // Team armband round the upper arm.
        m.add(body(f, lod, &[(57, 55, -170), (57, 55, -110)], TEAM));
        // The shoulder: a soft cap with a team pad (Meridian) or a hard team plate (Veyran).
        match f {
            F::M => {
                m.add(lathe(
                    &[(62, 30), (50, 62), (0, 80)],
                    if lod.has(0) { 16 } else { 8 },
                    &[sleeve],
                    None,
                    None,
                ));
                m.add(team_strip([-52, 70, -46], [52, 86, 46], lod));
            }
            F::V => {
                m.add(closed_box([-80, 40, -66], [80, 84, 78], PAINT2));
                m.add(team_strip([-74, 84, -60], [74, 96, 72], lod));
            }
        }
        if rifle {
            m.add(
                lathe_x(
                    &[(46, -20), (40, 230)],
                    if lod.has(0) { 16 } else { 8 },
                    &[forearm],
                    Some(forearm),
                    None,
                )
                .moved([0, elbow, 0]),
            );
            m.add(closed_box(
                [220, elbow - 40, -38],
                [300, elbow + 36, 38],
                hand,
            ));
        } else {
            let fore = [(36, 34, -560), (42, 40, -470), (47, 45, elbow - 20)];
            m.add(body(f, lod, &rings_for(lod, &fore), forearm));
            m.add(closed_box([-34, -660, -30], [44, -555, 30], hand));
            if f == F::M && lod.has(0) {
                // The rolled cuff.
                m.add(body(
                    f,
                    lod,
                    &[(52, 50, elbow - 30), (52, 50, elbow + 20)],
                    sleeve,
                ));
            }
        }
    }
    if rifle {
        m.add(rifle_mesh(f, lod));
    }
    m
}

/// The rifle in the right arm's frame, along `+X` at the hand.
fn rifle_mesh(f: F, lod: Lod) -> Mesh {
    let (y, z) = (-300, -18);
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
    let entry = format!("{}_figure", f.name());
    Model::new(
        &format!("{}_rifleman", f.name()),
        pal.materials(&entry),
        |lod| {
            let at = |name: &str| {
                JOINTS
                    .iter()
                    .find(|j| j.0 == name)
                    .map(|j| j.1)
                    .expect("a joint")
            };
            vec![
                Node::part("leg_l", at("leg_l"), thigh(f, lod)),
                Node::part("leg_r", at("leg_r"), thigh(f, lod)),
                Node::part("shin_l", at("shin_l"), shin(f, lod)),
                Node::part("shin_r", at("shin_r"), shin(f, lod)),
                Node::part("torso", at("torso"), torso(f, lod)),
                Node::part("head", at("head"), head(f, lod)),
                Node::part("arm_l", at("arm_l"), arm(f, lod, false)),
                Node::part("arm_r", at("arm_r"), arm(f, lod, true)),
            ]
        },
    )
}
