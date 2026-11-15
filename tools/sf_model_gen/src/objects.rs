//! The mission objects, the Network defences and the rubble heaps in style G (WP11.6c,
//! docs/art-style.md section 11).
//!
//! The world's objects are neutral: their third material slot is the light paint `paint3`
//! (palette `neutral`, as the neutral units of WP11.6b), never the player colour. The two automated Network defences carry the
//! player colour like a faction's structures (palette `network`). The rubble of a destroyed
//! structure is one heap per footprint, shared by all factions (`wrecks.ron`).

use crate::buildings::{Built, F, antenna, dish, hall, pipe_x, roof_unit, tank, turret};
use crate::kit::{
    Lod, Mesh, Model, Node, RECESS, SEAM, bevel_box, closed_box, cuboid, lathe, lathe_x, loft, ngon,
};
use crate::models::{anchors, team_frame, team_strip};
use crate::palette::{Palette, slot::*};
use crate::stages::{chunk, fallen_plate, fit_team, hash, pad_model};

/// The light marking paint of a neutral model (`paint3`, the slot of `team`).
pub const TRIM: crate::kit::Mat = PAINT3;

/// Build an object from `recipe` with the palette `pal_name`: the team share is fitted only when
/// the palette has the player colour (Network); a neutral model keeps its candidates in paint.
pub fn object(
    stem: &str,
    pal_name: &str,
    floor: usize,
    ceil: usize,
    recipe: impl Fn(Lod) -> Built,
) -> Model {
    let pal = Palette::shipped();
    let teamed = pal_name != "neutral";
    let pad = recipe(Lod(0)).pad;
    let mut m = Model::new(stem, pal.materials(pal_name), |lod| {
        let mut b = recipe(lod);
        let turret_world = b.turret.as_ref().map(Node::world_tris).unwrap_or_default();
        if teamed {
            fit_team(&mut b.body, &turret_world, b.cands, stem);
        } else {
            // A neutral model's candidates are its light markings (`paint3`): all of them stay.
            for c in b.cands {
                b.body.add(c);
            }
        }
        let mut body = Node::part("body", [0, 0, 0], b.body);
        if let Some(t) = b.turret {
            body = body.child(t);
        }
        vec![anchors(body, b.smoke, b.fire)]
    });
    pad_model(&mut m, floor, ceil, pad);
    m
}

/// The least LOD0 triangles of a mission object (they are structures: building budget).
pub const OBJECT_FLOOR: usize = 1000;
/// No object pads past this; decor (the crate, the heaps) stays under its 1500.
pub const OBJECT_CEIL: usize = 3400;
pub const DECOR_CEIL: usize = 1450;

#[allow(clippy::too_many_arguments)]
fn built(
    body: Mesh,
    cands: Vec<Mesh>,
    turret: Option<Node>,
    smoke: [i32; 3],
    fire: [i32; 3],
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

/// A plain concrete base with light marking stripes along two edges (neutral) or team candidates.
fn base(lod: Lod, hx: i32, hz: i32, h: i32) -> (Mesh, Vec<Mesh>) {
    let m = bevel_box([-hx, 0, -hz], [hx, h, hz], lod.bevel(80), PAINT, PAINT);
    let mut c = Vec::new();
    for s in [-1, 1] {
        let z = s * (hz - 220) - if s > 0 { 240 } else { 0 };
        c.push(team_strip(
            [-hx + 250, h, z],
            [hx - 250, h + 15, z + 240],
            lod,
        ));
        let x = s * (hx - 220) - if s > 0 { 240 } else { 0 };
        c.push(team_strip(
            [x, h, -hz + 500],
            [x + 240, h + 15, hz - 500],
            lod,
        ));
    }
    (m, c)
}

/// Relay transmitter (2 x 2): a lattice mast with dishes and a hut at its foot.
pub fn relay_transmitter() -> Model {
    object(
        "neutral_relay_transmitter",
        "neutral",
        OBJECT_FLOOR,
        OBJECT_CEIL,
        |lod| {
            let (mut m, _) = base(lod, 1850, 1850, 200);
            let c = Vec::new();
            let sections = if lod.has(1) { 6 } else { 2 };
            let height = 18000;
            for s in 0..sections {
                let y0 = 200 + height * s / sections;
                let y1 = 200 + height * (s + 1) / sections;
                let w0 = 1300 - 900 * s / sections;
                let w1 = 1300 - 900 * (s + 1) / sections;
                let w = (w0 + w1) / 2;
                for (sx, sz) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
                    m.add(closed_box(
                        [sx * w - 60, y0, sz * w - 60],
                        [sx * w + 60, y1, sz * w + 60],
                        METAL,
                    ));
                }
                let band = if s % 2 == 0 { TRIM } else { PAINT };
                m.add(cuboid(
                    [-w1 - 80, y1 - 120, -w1 - 80],
                    [w1 + 80, y1, w1 + 80],
                    band,
                ));
                if lod.has(0) {
                    for k in 0..2 {
                        let y = y0 + (y1 - y0) * (k + 1) / 3;
                        for (a, b) in [((-w, -w), (w, -w)), ((-w, w), (w, w))] {
                            m.add(closed_box(
                                [a.0, y, a.1 - 30],
                                [b.0, y + 60, b.1 + 30],
                                METAL,
                            ));
                        }
                        for (a, b) in [((-w, -w), (-w, w)), ((w, -w), (w, w))] {
                            m.add(closed_box(
                                [a.0 - 30, y, a.1],
                                [b.0 + 30, y + 60, b.1],
                                METAL,
                            ));
                        }
                    }
                }
            }
            let top = 200 + height;
            m.add(antenna(lod, 0, top, 0, 4000));
            for (k, h) in [(0u8, top - 3000), (2u8, top - 6000)] {
                if lod.has(1) {
                    m.add(
                        lathe_x(
                            &[(900, 0), (500, 300), (100, 450)],
                            lod.segs(16),
                            &[PAINT, PAINT],
                            Some(METAL),
                            None,
                        )
                        .moved([500, h, 0])
                        .turned_y(k),
                    );
                }
            }
            let (hut, _) = hall(F::M, lod, -900, 900, 800, 800, 200, 2200);
            m.add(hut.recoloured(TEAM, PAINT2));
            if lod.has(0) {
                for k in 0..24 {
                    m.add(
                        cuboid(
                            [1100 - 40, 200 + k * 700, -200],
                            [1100, 240 + k * 700, 200],
                            METAL,
                        )
                        .moved([0, 0, 0]),
                    );
                }
            }
            built(
                m,
                c,
                None,
                [0, top - 2000, 0],
                [-900, 2400, 900],
                (1850, 1850, 200),
            )
        },
    )
}

/// Fuel depot (2 x 2): three lying tanks on cradles, a pump house and pipes.
pub fn fuel_depot() -> Model {
    object(
        "neutral_fuel_depot",
        "neutral",
        OBJECT_FLOOR,
        OBJECT_CEIL,
        |lod| {
            let (mut m, c) = base(lod, 3700, 3700, 200);
            for z in [-2400, 0, 2400] {
                m.add(
                    lathe_x(
                        &[
                            (0, -3000),
                            (700, -2800),
                            (1000, -2400),
                            (1000, 2000),
                            (700, 2400),
                            (0, 2600),
                        ],
                        lod.segs(16),
                        &[PAINT, PAINT, PAINT, PAINT, PAINT],
                        None,
                        None,
                    )
                    .moved([-400, 1400, z]),
                );
                m.add(
                    lathe_x(
                        &[(1020, -400), (1020, 0)],
                        lod.segs(16),
                        &[TRIM],
                        None,
                        None,
                    )
                    .moved([-400, 1400, z]),
                );
                if lod.has(1) {
                    for x in [-2400, 1600] {
                        m.add(closed_box(
                            [x - 200, 200, z - 800],
                            [x + 200, 700, z + 800],
                            PAINT2,
                        ));
                    }
                }
                if lod.has(0) {
                    m.add(
                        lathe(&[(150, 0), (150, 300)], 8, &[METAL], Some(METAL), None)
                            .moved([0, 2380, z]),
                    );
                    m.add(pipe_x(lod, [2000, 700, z], 1300, 110));
                }
            }
            let (h, _) = hall(F::M, lod, 2900, -2600, 650, 900, 200, 2000);
            m.add(h.recoloured(TEAM, PAINT2));
            built(
                m,
                c,
                None,
                [-400, 2600, 0],
                [2900, 2200, -2600],
                (3700, 3700, 200),
            )
        },
    )
}

/// Archive building (3 x 3): a two-storey office block with window rows and roof units.
pub fn archive_building() -> Model {
    object(
        "neutral_archive_building",
        "neutral",
        OBJECT_FLOOR,
        OBJECT_CEIL,
        |lod| {
            let (mut m, c) = base(lod, 5600, 5600, 200);
            let (h, top) = hall(F::M, lod, 0, 0, 5000, 4600, 200, 3000);
            m.add(h.recoloured(TEAM, PAINT2));
            let (h2, top2) = hall(F::V, lod, -800, 0, 3200, 3400, top, 1900);
            m.add(h2.recoloured(TEAM, PAINT2));
            for (x, z) in [(3000, -3200), (3000, 3200)] {
                m.add(roof_unit(F::M, lod, x, top, z, 1000));
            }
            m.add(antenna(lod, -2500, top2, 2000, 1800));
            built(
                m,
                c,
                None,
                [0, top2 + 300, 0],
                [3000, top, 0],
                (5600, 5600, 200),
            )
        },
    )
}

/// Bridge segment (1 x 1): a deck on two girders and a pier, railings.
pub fn bridge_segment() -> Model {
    object(
        "neutral_bridge_segment",
        "neutral",
        OBJECT_FLOOR,
        OBJECT_CEIL,
        |lod| {
            let mut m = Mesh::new();
            m.add(bevel_box(
                [-1950, 950, -1700],
                [1950, 1250, 1700],
                lod.bevel(40),
                PAINT,
                PAINT,
            ));
            for z in [-1200, 1200] {
                m.add(closed_box(
                    [-1950, 500, z - 200],
                    [1950, 950, z + 200],
                    METAL,
                ));
            }
            m.add(lathe(
                &[(700, 0), (600, 500)],
                lod.segs(16),
                &[PAINT2],
                Some(PAINT2),
                None,
            ));
            m.add(closed_box([-500, 0, -1600], [500, 500, 1600], PAINT2));
            let mut c = Vec::new();
            for z in [-1500, 1300] {
                c.push(team_strip([-1900, 1250, z], [1900, 1265, z + 200], lod));
            }
            if !lod.has(0) {
                for z in [-1650, 1650] {
                    m.add(cuboid([-1950, 1250, z - 30], [1950, 2150, z + 30], METAL));
                }
            }
            if lod.has(0) {
                for z in [-1650, 1650] {
                    m.add(crate::kit::railing(
                        [-1900, 1250, z],
                        [1900, 1250, z],
                        900,
                        6,
                        METAL,
                    ));
                }
                for k in 0..8 {
                    let x = -1800 + k * 500;
                    for z in [-1200, 1200] {
                        m.add(
                            closed_box([x, 520, z - 210], [x + 60, 930, z + 210], METAL)
                                .toned(RECESS),
                        );
                    }
                    m.add(cuboid([x, 1250, -1000], [x + 40, 1260, 1000], PAINT).toned(SEAM));
                }
            }
            built(m, c, None, [0, 1500, 0], [0, 1300, 900], (1900, 1550, 1250))
        },
    )
}

/// Rail spur (1 x 1): track on sleepers and ballast with a buffer stop.
pub fn rail_spur() -> Model {
    object(
        "neutral_rail_spur",
        "neutral",
        OBJECT_FLOOR,
        OBJECT_CEIL,
        |lod| {
            let mut m = loft(
                &[
                    (crate::kit::chamfer_rect(1950, 1250, 0), 0),
                    (crate::kit::chamfer_rect(1950, 950, 0), 250),
                ],
                &[PAINT2],
                Some(PAINT2),
                None,
            );
            let n = if lod.has(0) {
                7
            } else if lod.has(1) {
                4
            } else {
                2
            };
            for k in 0..n {
                let x = -1700 + 3400 * k / (n - 1).max(1);
                m.add(cuboid([x - 120, 250, -1100], [x + 120, 350, 1100], RUBBER).toned(860));
            }
            for z in [-720, 720] {
                m.add(closed_box([-1950, 350, z - 40], [1950, 500, z + 40], METAL));
            }
            // The buffer stop at +X.
            m.add(closed_box([1400, 350, -900], [1700, 1000, 900], PAINT));
            let c = vec![
                team_strip([1400, 1000, -900], [1700, 1015, -700], lod),
                team_strip([1400, 1000, 700], [1700, 1015, 900], lod),
            ];
            if lod.has(0) {
                for z in [-720, 720] {
                    m.add(
                        lathe_x(&[(160, 0), (160, 200)], 8, &[METAL], Some(RUBBER), None)
                            .moved([1200, 750, z]),
                    );
                    for k in 0..n {
                        let x = -1700 + 3400 * k / (n - 1).max(1);
                        m.add(
                            cuboid([x - 60, 350, z - 90], [x + 60, 380, z + 90], METAL)
                                .toned(RECESS),
                        );
                    }
                }
            }
            built(m, c, None, [0, 800, 0], [1500, 1000, 0], (1950, 1000, 250))
        },
    )
}

/// Generator (1 x 1): a skid-mounted generator with a radiator, an exhaust and cable drums.
/// `on`: the lamps lit (accent) or dark (the `_off` state).
pub fn generator(on: bool) -> Model {
    let stem = if on {
        "neutral_generator"
    } else {
        "neutral_generator_off"
    };
    object(stem, "neutral", OBJECT_FLOOR, OBJECT_CEIL, move |lod| {
        let (mut m, c) = base(lod, 1850, 1850, 150);
        m.add(bevel_box(
            [-1500, 150, -1000],
            [1300, 1900, 1000],
            lod.bevel(120),
            PAINT2,
            PAINT2,
        ));
        m.add(closed_box([1300, 300, -900], [1600, 1700, 900], METAL));
        m.add(
            lathe(
                &[(180, 0), (150, 1500)],
                lod.segs(8),
                &[METAL],
                Some(RUBBER),
                None,
            )
            .moved([-900, 1900, 500]),
        );
        let lamp = if on { ACCENT } else { RUBBER };
        m.add(cuboid([-1500, 1900, -1000], [1300, 1915, -760], TRIM));
        m.add(cuboid([-1500, 1900, 760], [1300, 1915, 1000], TRIM));
        if lod.has(1) {
            m.add(closed_box([0, 1900, -300], [600, 2150, 300], lamp));
        }
        if lod.has(0) {
            for k in 0..8 {
                let y = 450 + k * 150;
                m.add(closed_box([1600, y, -850], [1650, y + 60, 850], PAINT2).toned(RECESS));
            }
            for k in 0..5 {
                let x = -1300 + k * 500;
                m.add(closed_box([x, 500, 1000], [x + 300, 1500, 1040], PAINT).toned(SEAM));
                m.add(closed_box([x, 500, -1040], [x + 300, 1500, -1000], PAINT).toned(SEAM));
            }
            m.add(
                lathe_x(
                    &[(450, 0), (450, 500)],
                    16,
                    &[RUBBER],
                    Some(METAL),
                    Some(METAL),
                )
                .moved([-1700, 600, -1400])
                .moved([0, 0, 0]),
            );
        }
        built(
            m,
            c,
            None,
            [-900, 3400, 500],
            [0, 1900, 0],
            (1850, 1850, 150),
        )
    })
}

/// Evidence crate: a small crate with banded corners.
pub fn evidence_crate() -> Model {
    object("neutral_evidence_crate", "neutral", 0, DECOR_CEIL, |lod| {
        let mut m = bevel_box(
            [-480, 0, -280],
            [480, 560, 280],
            lod.bevel(30),
            PAINT2,
            PAINT2,
        );
        if lod.has(0) {
            for x in [-300, 300] {
                m.add(closed_box([x - 40, 0, -300], [x + 40, 580, 300], METAL));
            }
        }
        if lod.has(0) {
            m.add(cuboid([-200, 560, -150], [200, 570, 150], PAINT).toned(900));
            for (x, z) in [(-470, -270), (470, -270), (-470, 270), (470, 270)] {
                m.add(closed_box(
                    [x - 30, 0, z - 30],
                    [x + 30, 580, z + 30],
                    METAL,
                ));
            }
            for x in [-420, 420] {
                m.add(closed_box([x - 60, 300, -310], [x + 60, 360, -290], METAL).toned(RECESS));
            }
        }
        let c = vec![cuboid([-440, 560, -40], [-320, 565, 40], TRIM)];
        built(m, c, None, [0, 600, 0], [0, 560, 0], (0, 0, 0))
    })
}

/// Network automated gun (1 x 1): an armoured plinth with a turret. `working` false: the
/// `_disabled` state (lamps dark, the barrel lowered, scorched).
pub fn automated_gun(working: bool) -> Model {
    let stem = if working {
        "network_automated_gun"
    } else {
        "network_automated_gun_disabled"
    };
    object(stem, "network", OBJECT_FLOOR, OBJECT_CEIL, move |lod| {
        let mut m = loft(
            &[
                (ngon(1800, 8, true), 0),
                (ngon(1600, 8, true), 900),
                (ngon(1250, 8, true), 1500),
            ],
            &[PAINT2, PAINT],
            Some(PAINT2),
            None,
        );
        m.add(team_frame(
            &ngon(1250, lod.segs(8), true),
            &ngon(1030, lod.segs(8), true),
            1500,
            1530,
            lod,
            PAINT2,
        ));
        let mut c = Vec::new();
        for k in 0..4u8 {
            c.push(team_strip([1350, 900, -300], [1550, 912, 300], lod).turned_y(k));
        }
        c.push(team_frame(
            &ngon(1640, 8, true),
            &ngon(1400, 8, true),
            900,
            930,
            lod,
            PAINT2,
        ));
        c.push(team_frame(
            &ngon(1880, 8, true),
            &ngon(1640, 8, true),
            0,
            40,
            lod,
            PAINT2,
        ));
        {}
        let lamp = if working { ACCENT } else { RUBBER };
        if lod.has(1) {
            for k in 0..4u8 {
                m.add(closed_box([1500, 600, -150], [1700, 750, 150], lamp).turned_y(k));
            }
        }
        if lod.has(0) {
            for k in 0..16 {
                let [x, z] = crate::kit::on_circle(1500, k, 16);
                m.add(crate::kit::bolt([x, 1200, z], 40, 25, 4, METAL));
                m.add(crate::kit::bolt(
                    [x * 112 / 100, 450, z * 112 / 100],
                    40,
                    25,
                    4,
                    METAL,
                ));
            }
        }
        let mut t = turret(F::V, lod, [0, 1530, 0], 800, 1300, false);
        if !working {
            t = t.clone();
            if let Some(b) = t.children.first_mut() {
                b.at[1] -= 150;
            }
            m = m.toned(700);
        }
        built(
            m,
            c,
            Some(t),
            [0, 2300, 0],
            [500, 1500, 500],
            (900, 900, 1530),
        )
    })
}

/// Network unmanned artillery (2 x 2): a platform with a long gun and an uplink dish. `uplink`
/// false: the `_no_uplink` state (the dish gone, its mast broken, lamps dark).
pub fn unmanned_artillery(uplink: bool) -> Model {
    let stem = if uplink {
        "network_unmanned_artillery"
    } else {
        "network_unmanned_artillery_no_uplink"
    };
    object(stem, "network", OBJECT_FLOOR, OBJECT_CEIL, move |lod| {
        let (mut m, c) = crate::buildings::slab(F::V, lod, 3700, 3700, 350);
        m.add(loft(
            &[
                (ngon(2300, lod.segs(8), true), 350),
                (ngon(1900, lod.segs(8), true), 1100),
            ],
            &[PAINT],
            Some(PAINT2),
            None,
        ));
        m.add(team_frame(
            &ngon(1900, lod.segs(8), true),
            &ngon(1680, lod.segs(8), true),
            1100,
            1130,
            lod,
            PAINT2,
        ));
        if uplink {
            m.add(dish(F::V, lod, -2500, 350, 2500, 1000, 1800));
        } else {
            m.add(
                lathe(
                    &[(180, 0), (180, 900)],
                    lod.segs(8),
                    &[METAL],
                    Some(RUBBER),
                    None,
                )
                .moved([-2500, 350, 2500]),
            );
        }
        m.add(tank(F::V, lod, -2600, -2500, 350, 700, 1300));
        let lamp = if uplink { ACCENT } else { RUBBER };
        if lod.has(1) {
            m.add(closed_box([2200, 350, -400], [2600, 700, 400], lamp));
        }
        if lod.has(0) {
            for k in 0..16 {
                let [x, z] = crate::kit::on_circle(2100, k, 16);
                m.add(crate::kit::bolt([x, 720, z], 40, 25, 4, METAL));
            }
            m.add(pipe_x(lod, [-2000, 700, -2500], 1200, 120));
        }
        let t = turret(F::V, lod, [0, 1130, 0], 1100, 2500, false);
        built(
            m,
            c,
            Some(t),
            [0, 2200, 0],
            [-2600, 1700, -2500],
            (3700, 3700, 350),
        )
    })
}

/// A rubble heap of a `w` x `h` structure (cells of 4 m): chunks of masonry and plate on a dusty
/// spread, bent plates and rebar, no team.
pub fn rubble(w: i32, h: i32) -> Model {
    let stem = format!("neutral_rubble_{w}x{h}");
    let seed = (w * 31 + h) as u32;
    // Inside the set v4 heaps (2.8 m a cell less a little).
    let (hx, hz) = (w * 1380, h * 1360);
    object(&stem, "neutral", 0, DECOR_CEIL, move |lod| {
        let mut m = loft(
            &[
                (
                    crate::kit::chamfer_rect(hx, hz, lod.bevel(hx.min(hz) / 3)),
                    0,
                ),
                (
                    crate::kit::chamfer_rect(
                        hx * 85 / 100,
                        hz * 85 / 100,
                        lod.bevel(hx.min(hz) / 3),
                    ),
                    120,
                ),
            ],
            &[PAINT],
            Some(PAINT),
            None,
        )
        .toned(820);
        let step = if lod.has(0) {
            1100
        } else if lod.has(1) {
            2000
        } else {
            3600
        };
        let cap = if lod.has(0) {
            5
        } else if lod.has(1) {
            3
        } else {
            2
        };
        let (nx, nz) = ((2 * hx / step).clamp(1, cap), (2 * hz / step).clamp(1, cap));
        for i in 0..nx {
            for k in 0..nz {
                let s = seed.wrapping_mul(97).wrapping_add((i * nz + k) as u32);
                let x =
                    -hx + (2 * hx) * (2 * i + 1) / (2 * nx) + (hash(s) as i32 - 500) * step / 3000;
                let z = -hz
                    + (2 * hz) * (2 * k + 1) / (2 * nz)
                    + (hash(s ^ 9) as i32 - 500) * step / 3000;
                let r = step * (55 + hash(s ^ 3) as i32 / 25) / 100;
                let hh = 350 + hash(s ^ 5) as i32 * 5 / 10;
                let mat = [PAINT, PAINT2, PAINT][(s % 3) as usize];
                let r = r.min(hx * 8 / 10).min(hz * 8 / 10);
                let m2 = r * 12 / 10;
                let (x, z) = (x.clamp(-hx + m2, hx - m2), z.clamp(-hz + m2, hz - m2));
                m.add(chunk([x, 60, z], r, hh, s, mat, lod));
            }
        }
        if lod.has(1) {
            for k in 0..(w * h).min(6) {
                let s = seed.wrapping_add(k as u32);
                let x = (hash(s * 7) as i32 - 500) * hx / 800;
                let z = (hash(s * 11) as i32 - 500) * hz / 800;
                let x = x.clamp(-hx + 1000, hx - 1000);
                let z = z.clamp(-hz + 1000, hz - 1000);
                m.add(fallen_plate([x, 100, z], 900, 600, 380, (k % 4) as u8, METAL).toned(700));
            }
        }
        if lod.has(0) {
            for k in 0..(w * h * 3).min(18) {
                let s = seed.wrapping_mul(5).wrapping_add(k as u32);
                let x = ((hash(s) as i32 - 500) * hx / 700).clamp(-hx + 200, hx - 200);
                let z = ((hash(s ^ 1) as i32 - 500) * hz / 700).clamp(-hz + 200, hz - 200);
                m.add(
                    closed_box([x - 15, 100, z - 15], [x + 15, 750, z + 15], METAL).toned(RECESS),
                );
            }
        }
        built(m, Vec::new(), None, [0, 500, 0], [0, 400, 0], (hx, hz, 120))
    })
}
