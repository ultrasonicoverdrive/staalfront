//! The Meridian light transport and amphibious transport, and the Veyran harvester (WP11.6b,
//! docs/art-style.md).

use crate::kit::{
    Model, Node, bolt, chamfer_rect, closed_box, cuboid, extrude_z1, hatch, lathe, lathe_x,
    lathe_z, loft, railing, seam_side_z, seam_top,
};
use crate::models::{Track, both_sides, team_frame, team_strip, wedge};
use crate::palette::{Palette, slot::*};
use crate::units_wheeled::{arches, lamps, soft, wheels};

/// Meridian light transport: a small open four-wheeler with a rounded hood, a bench tub with a
/// roll bar and a windscreen frame. Team: stripes along the hood, a band round the tub rim.
pub fn meridian_light_transport() -> Model {
    let pal = Palette::shipped();
    Model::new(
        "meridian_light_transport",
        pal.materials("meridian"),
        |lod| {
            let c = |v: i32| lod.bevel(v);
            let (r, w, z_out) = (360, 300, 985);
            let xs = [-1000, 1000];
            let mut hull = wheels(lod, &xs, r, w, z_out);
            hull.add(arches(lod, &xs, r, w, z_out));
            // The frame, the hood in front, the tub behind it.
            hull.add(soft(
                lod,
                [-1650, 420, -760],
                [1650, 560, 760],
                c(30),
                PAINT2,
                PAINT2,
            ));
            hull.add(
                loft(
                    &[
                        (chamfer_rect(650, 800, c(220)), 560),
                        (chamfer_rect(650, 800, c(220)), 860),
                        (chamfer_rect(580, 700, c(260)), 1040),
                    ],
                    &[PAINT, PAINT],
                    Some(PAINT),
                    None,
                )
                .moved([1020, 0, 0]),
            );
            let tub = loft(
                &[
                    (chamfer_rect(960, 800, c(120)), 560),
                    (chamfer_rect(960, 800, c(120)), 1000),
                ],
                &[PAINT],
                Some(PAINT2),
                None,
            )
            .moved([-690, 0, 0]);
            hull.add(tub);
            // Team: a narrow band round the tub rim, two stripes along the hood.
            let rim = team_frame(
                &chamfer_rect(960, 800, c(120)),
                &chamfer_rect(740, 580, c(60)),
                990,
                1030,
                lod,
                PAINT2,
            );
            hull.add(rim.moved([-690, 0, 0]));
            for z in [-420, 220] {
                hull.add(team_strip([500, 1040, z], [1600, 1058, z + 200], lod));
            }
            if lod.has(1) {
                // Seats with backrests, the windscreen frame with glass, a roll bar behind them.
                hull.add(closed_box([-300, 1000, -640], [100, 1280, -80], PAINT));
                hull.add(closed_box([-300, 1000, 80], [100, 1280, 640], PAINT));
                hull.add(closed_box([-1300, 1030, -600], [-200, 1260, -520], PAINT2));
                hull.add(closed_box([440, 1040, -700], [500, 1450, 700], ACCENT));
                hull.add(both_sides(closed_box(
                    [420, 1040, 680],
                    [520, 1500, 740],
                    METAL,
                )));
            }
            hull.add(closed_box([-420, 1280, -780], [-360, 1700, 780], METAL));
            if lod.has(0) {
                hull.add(lamps(1560, 700, 520));
                for i in 0..4 {
                    hull.add(seam_top(
                        [800 + i * 220, 1040, -450],
                        [800 + i * 220, 1040, 450],
                        40,
                        14,
                        PAINT2,
                    ));
                }
                hull.add(hatch([1000, 1040, 0], 180, 8, PAINT));
                // A grille on the nose, jerry cans on the tub sides, tow hooks, rivets on the hood.
                for i in 0..5 {
                    hull.add(closed_box(
                        [1600, 640 + i * 70, -420],
                        [1650, 690 + i * 70, 420],
                        METAL,
                    ));
                }
                for i in 0..2 {
                    hull.add(both_sides(closed_box(
                        [-1400 + i * 420, 700, 800],
                        [-1100 + i * 420, 940, 880],
                        PAINT2,
                    )));
                }
                hull.add(both_sides(closed_box(
                    [1620, 430, 520],
                    [1700, 500, 640],
                    METAL,
                )));
                hull.add(both_sides(closed_box(
                    [-1650, 430, 520],
                    [-1700, 500, 640],
                    METAL,
                )));
                for i in 0..5 {
                    hull.add(bolt([700 + i * 160, 1040, -520], 30, 18, 4, METAL));
                    hull.add(bolt([700 + i * 160, 1040, 520], 30, 18, 4, METAL));
                }
                for i in 0..4 {
                    hull.add(bolt([-1500, 1000, -420 + i * 280], 35, 20, 4, METAL));
                }
                hull.add(both_sides(seam_side_z(-690, 600, 980, 800, 40, 10, PAINT2)));
                hull.add(both_sides(closed_box(
                    [-1700, 700, 540],
                    [-1640, 800, 700],
                    ACCENT,
                )));
                // A spare wheel on the rear panel and an antenna.
                hull.add(
                    lathe_z(&[(300, 0), (300, 140)], 8, &[RUBBER], Some(METAL), None)
                        .moved([-1000, 1000, -820]),
                );
            }
            let turret =
                Node::empty("turret", [0, 1000, 0]).child(Node::empty("barrel", [0, 0, 0]));
            vec![Node::part("hull", [0, 0, 0], hull).child(turret)]
        },
    )
}

/// Meridian amphibious transport: a boat hull with a sloped bow on six big wheels, a cab at the
/// front and an open hold behind it with a rail; team stripes across the hold cover and a band
/// round the cab roof.
pub fn meridian_amphibious_transport() -> Model {
    let pal = Palette::shipped();
    Model::new(
        "meridian_amphibious_transport",
        pal.materials("meridian"),
        |lod| {
            let c = |v: i32| lod.bevel(v);
            let (r, w, z_out) = (520, 450, 1560);
            let xs = [-1900, 0, 1900];
            let mut hull = wheels(lod, &xs, r, w, z_out);
            hull.add(arches(lod, &xs, r, w, z_out));
            // The hull: a side profile with a sloped bow, pushed out across the width, with a
            // bow ramp and a sloped stern; a wider deck rim on top.
            let side = [
                [-3100, 560],
                [2600, 560],
                [3150, 1000],
                [3150, 1500],
                [-3100, 1500],
            ];
            hull.add(extrude_z1(&side, -1250, 1250, PAINT));
            hull.add(soft(
                lod,
                [-3100, 1500, -1300],
                [3150, 1580, 1300],
                c(30),
                PAINT2,
                PAINT2,
            ));
            // The cab at the front: windows all round, a roof band in team colour.
            let cab = loft(
                &[
                    (chamfer_rect(650, 1000, c(260)), 1580),
                    (chamfer_rect(650, 1000, c(260)), 2150),
                    (chamfer_rect(580, 920, c(300)), 2330),
                ],
                &[PAINT, PAINT],
                Some(PAINT),
                None,
            )
            .moved([2200, 0, 0]);
            hull.add(cab);
            hull.add(
                team_frame(
                    &chamfer_rect(570, 910, c(295)),
                    &chamfer_rect(330, 660, c(180)),
                    2320,
                    2360,
                    lod,
                    PAINT,
                )
                .moved([2200, 0, 0]),
            );
            if lod.has(1) {
                hull.add(both_sides(closed_box(
                    [1900, 1800, 1000],
                    [2700, 2200, 1025],
                    ACCENT,
                )));
                hull.add(closed_box([2850, 1800, -800], [2875, 2200, 800], ACCENT));
            }
            // The hold behind the cab: a cover plate with team stripes across it.
            hull.add(soft(
                lod,
                [-2900, 1580, -1050],
                [1400, 1640, 1050],
                c(30),
                PAINT2,
                PAINT2,
            ));
            for i in 0..5 {
                let x = -2800 + i * 650;
                hull.add(team_strip([x, 1640, -1000], [x + 300, 1658, 1000], lod));
            }
            if lod.has(0) {
                hull.add(railing(
                    [-2900, 1640, -1050],
                    [1400, 1640, -1050],
                    420,
                    8,
                    METAL,
                ));
                hull.add(railing(
                    [-2900, 1640, 1050],
                    [1400, 1640, 1050],
                    420,
                    8,
                    METAL,
                ));
                for i in 0..5 {
                    let x = -2500 + i * 800;
                    hull.add(seam_top([x, 1640, -900], [x, 1640, 900], 40, 14, PAINT2));
                }
                hull.add(hatch([700, 1640, 0], 280, 8, PAINT));
                // Propeller guard and rudder at the stern, a bow light, hull bolts.
                hull.add(
                    lathe_x(&[(260, 0), (260, 300)], 8, &[METAL], Some(RUBBER), None)
                        .moved([-3160, 760, 500]),
                );
                hull.add(
                    lathe_x(&[(260, 0), (260, 300)], 8, &[METAL], Some(RUBBER), None)
                        .moved([-3160, 760, -500]),
                );
                hull.add(closed_box([3150, 1200, -150], [3160, 1450, 150], ACCENT));
                hull.add(both_sides(seam_side_z(
                    -1500, 600, 1480, 1250, 40, 10, PAINT2,
                )));
                hull.add(both_sides(seam_side_z(
                    500, 600, 1480, 1250, 40, 10, PAINT2,
                )));
            }
            let turret =
                Node::empty("turret", [0, 1580, 0]).child(Node::empty("barrel", [0, 0, 0]));
            vec![Node::part("hull", [0, 0, 0], hull).child(turret)]
        },
    )
}

/// Veyran harvester: a tracked chassis with a sharp cab at the front and an eight-sided ore drum
/// with four narrow team bands and crimson trim.
pub fn veyran_harvester() -> Model {
    let pal = Palette::shipped();
    Model::new("veyran_harvester", pal.materials("veyran"), |lod| {
        let track = Track {
            hx: 3000,
            z0: 1000,
            z1: 1640,
            h: 720,
            wheels: 5,
            wheel_r: 320,
        };
        let mut hull = both_sides(track.mesh(lod));
        hull.add(loft(
            &[
                (wedge(3100, 2400, 700, 1450), 600),
                (wedge(3100, 2400, 700, 1550), 900),
            ],
            &[PAINT2],
            Some(PAINT2),
            None,
        ));
        // The cab: a wedge with a sloped windscreen and a roof band in team colour.
        hull.add(
            loft(
                &[
                    (wedge(700, 450, 450, 1250), 900),
                    (wedge(700, 450, 450, 1250), 1700),
                    (wedge(560, 250, 320, 1100), 2250),
                ],
                &[PAINT, PAINT],
                Some(PAINT),
                None,
            )
            .moved([2280, 0, 0]),
        );
        hull.add(
            team_frame(
                &wedge(550, 240, 310, 1090),
                &wedge(380, 120, 150, 880),
                2240,
                2280,
                lod,
                PAINT,
            )
            .moved([2280, 0, 0]),
        );
        hull.add(closed_box([2930, 1300, -1100], [2985, 1650, 1100], ACCENT));
        hull.add(closed_box([3000, 80, -1550], [3250, 650, 1550], METAL));
        // The ore drum, eight-sided, lying along X with four narrow team bands.
        let r = 1250;
        let segs = if lod.has(1) { 8 } else { 4 };
        let drum = if lod.has(1) {
            lathe_x(
                &[
                    (900, 0),
                    (r, 300),
                    (r, 600),
                    (r, 900),
                    (r, 1250),
                    (r, 1550),
                    (r, 2050),
                    (r, 2350),
                    (r, 2700),
                    (r, 3000),
                    (r, 3300),
                    (900, 3600),
                ],
                segs,
                &[
                    PAINT2, PAINT2, TEAM, PAINT2, TEAM, PAINT2, TEAM, PAINT2, TEAM, PAINT2, PAINT2,
                ],
                Some(METAL),
                Some(METAL),
            )
        } else {
            lathe_x(
                &[(r, 0), (r, 600), (r, 1250), (r, 2350), (r, 3000), (r, 3600)],
                segs,
                &[PAINT2, TEAM, PAINT2, TEAM, PAINT2],
                Some(PAINT2),
                Some(PAINT2),
            )
        };
        hull.add(drum.moved([-3050, 2150, 0]));
        if lod.has(1) {
            hull.add(both_sides(team_strip(
                [-3000, 900, 1270],
                [-1300, 918, 1550],
                lod,
            )));
        }
        if lod.has(1) {
            hull.add(closed_box([-2700, 900, -900], [-2400, 1200, 900], METAL));
            hull.add(closed_box([0, 900, -900], [300, 1200, 900], METAL));
        }
        if lod.has(0) {
            hull.add(hatch([2250, 2280, 0], 300, 8, PAINT));
            hull.add(closed_box([-3100, 1100, -300], [-3060, 1300, 300], ACCENT));
            hull.add(both_sides(closed_box(
                [1700, 1000, 1500],
                [1740, 2200, 1540],
                METAL,
            )));
            hull.add(railing(
                [1850, 2280, -1000],
                [2650, 2280, -1000],
                220,
                3,
                METAL,
            ));
            hull.add(
                lathe(&[(110, 0), (110, 160), (60, 220)], 8, &[ACCENT], None, None)
                    .moved([1950, 2280, 700]),
            );
            for i in 0..2 {
                let x = -2900 + i * 600;
                hull.add(seam_side_z(x, 650, 880, 1550, 50, 12, PAINT2));
                hull.add(seam_side_z(x, 650, 880, -1550, 50, 12, PAINT2));
            }
            for i in 0..4 {
                hull.add(seam_top(
                    [-2600 + i * 700, 2150 + r, -120],
                    [-2600 + i * 700, 2150 + r, 120],
                    60,
                    20,
                    METAL,
                ));
            }
            hull.add(both_sides(closed_box(
                [2400, 1000, 1400],
                [2700, 1600, 1440],
                PAINT2,
            )));
            for i in 0..3 {
                hull.add(cuboid(
                    [2000 + i * 330, 900, -1200],
                    [2150 + i * 330, 960, -1100],
                    METAL,
                ));
            }
        }
        let turret = Node::empty("turret", [0, 900, 0]).child(Node::empty("barrel", [0, 0, 0]));
        vec![Node::part("hull", [0, 0, 0], hull).child(turret)]
    })
}
