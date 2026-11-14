//! The unit recipes of set v5 (WP11.6b, style G, docs/art-style.md): every vehicle and vessel
//! role of both factions, the neutral supply truck and ferry, and the Veyran heavy gun team.
//! Same rules as [`crate::models`]: one function of [`Lod`] builds all three levels, round parts
//! lose segments, details drop out, the node tree and the team bands stay in every level.
//!
//! Meridian is rounded (chamfered boxes, domes), Veyran angular (wedges, sloped plates); both
//! builders take a [`Style`] so a role of both factions shares its recipe and differs in outline.

use crate::kit::{
    Model, Node, bevel_box, chamfer_rect, closed_box, extrude_z1, hatch, lathe, lathe_x, loft,
    plate, seam_top, shifted,
};
use crate::models::{Track, both_sides, team_frame, team_strip, wedge};
use crate::palette::{Palette, slot::*};

/// The shape language of a faction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// Meridian: chamfered boxes and rounded forms.
    Round,
    /// Veyran: wedges with a pointed nose and sloped plates.
    Angular,
}

impl Style {
    pub(crate) fn faction(self) -> &'static str {
        match self {
            Style::Round => "meridian",
            Style::Angular => "veyran",
        }
    }
}

/// A convex hull outline from `-back` to `front` (nose included) and `+-hz` wide: a chamfered
/// rectangle with corners cut by `c` for [`Style::Round`], a wedge with its nose a sixth of the
/// length for [`Style::Angular`] (6 corners, `c` unused).
pub(crate) fn ring(style: Style, back: i32, front: i32, hz: i32, c: i32) -> Vec<[i32; 2]> {
    match style {
        Style::Round => shifted(
            &chamfer_rect((front + back) / 2, hz, c),
            (front - back) / 2,
            0,
        ),
        Style::Angular => {
            let nose = (front + back) / 6;
            wedge(back, front - nose, nose, hz)
        }
    }
}

// ---- Heavy tank --------------------------------------------------------------------------

/// Heavy tank: seven road wheels a side, a broad hull with skirts, a large turret with a long
/// gun. Meridian rounded with a cast turret, Veyran a wedge with sloped plates and a muzzle brake.
pub fn heavy_tank(style: Style) -> Model {
    let pal = Palette::shipped();
    let stem = format!("{}_heavy_tank", style.faction());
    let round = style == Style::Round;
    Model::new(&stem, pal.materials(style.faction()), |lod| {
        let track = Track {
            hx: 3000,
            z0: 1160,
            z1: 1810,
            h: 820,
            wheels: 5,
            wheel_r: 400,
        };
        let c = |v: i32| lod.bevel(v);
        let mut hull = both_sides(track.mesh(lod));
        hull.add(loft(
            &[
                (ring(style, 2700, 2800, 900, c(120)), 350),
                (ring(style, 2900, 3000, 1250, c(300)), 800),
                (ring(style, 2850, 2900, 1250, c(300)), 1100),
                (ring(style, 2500, 2500, 1050, c(420)), 1300),
            ],
            &[PAINT, PAINT, PAINT],
            Some(PAINT),
            None,
        ));
        // Fenders over the tracks, in the second paint; skirts on the upper run.
        hull.add(both_sides(bevel_box(
            [-2900, 820, 1150],
            [2950, 880, 1900],
            lod.bevel(30),
            PAINT2,
            PAINT2,
        )));
        if lod.has(1) {
            let skirt = [[-2900, 380], [2700, 380], [2950, 820], [-2900, 820]];
            hull.add(both_sides(extrude_z1(&skirt, 1810, 1850, PAINT2)));
        }
        // The engine deck; team: a stripe across the rear, one along each fender, panels on
        // the skirts and two narrow cross bands in front of the turret.
        if lod.has(1) {
            hull.add(plate(
                [-2300, 1300, -1000],
                [-1000, 1335, 1000],
                lod.bevel(40),
                PAINT2,
            ));
        }
        hull.add(team_strip([-2500, 1300, -1000], [-2200, 1335, 1000], lod));
        hull.add(team_strip([1500, 1300, -950], [1800, 1318, 950], lod));
        hull.add(team_strip([2000, 1300, -900], [2300, 1318, 900], lod));
        hull.add(both_sides(team_strip(
            [-2600, 880, 1510],
            [2400, 898, 1810],
            lod,
        )));
        if lod.has(0) {
            hull.add(both_sides(closed_box(
                [-2200, 450, 1850],
                [1900, 780, 1860],
                TEAM,
            )));
        }
        if lod.has(0) {
            for i in 0..4 {
                let x = -2200 + i * 230;
                hull.add(seam_top([x, 1335, -800], [x, 1335, 800], 50, 18, PAINT2));
            }
            hull.add(hatch([1800, 1300, -600], 280, 8, PAINT));
            hull.add(both_sides(closed_box(
                [3000, 500, 700],
                [3200, 600, 950],
                METAL,
            )));
            // Headlights, a spare track on the glacis, an exhaust pair at the rear.
            hull.add(both_sides(closed_box(
                [2780, 1000, 900],
                [2900, 1110, 1080],
                ACCENT,
            )));
            for z in [-500, 500] {
                hull.add(
                    lathe_x(&[(100, 0), (100, 260)], 8, &[METAL], Some(RUBBER), None)
                        .moved([-3150, 1000, z]),
                );
            }
            if !round {
                hull.add(closed_box([-2950, 900, -700], [-2900, 1000, 700], ACCENT));
            }
        }
        let t = |v: i32| lod.bevel(v);
        let mut turret = if round {
            loft(
                &[
                    (chamfer_rect(1350, 1150, t(400)), 0),
                    (chamfer_rect(1450, 1220, t(440)), 320),
                    (chamfer_rect(1180, 960, t(380)), 760),
                ],
                &[PAINT, PAINT],
                Some(PAINT),
                None,
            )
            .moved([120, 0, 0])
        } else {
            loft(
                &[
                    (wedge(1350, 700, 700, 1100), 0),
                    (wedge(1100, 560, 640, 820), 520),
                ],
                &[PAINT],
                Some(PAINT),
                None,
            )
        };
        let (roof_y, outer, inner) = if round {
            (
                760,
                chamfer_rect(1170, 950, t(375)),
                chamfer_rect(900, 680, t(240)),
            )
        } else {
            (520, wedge(1080, 540, 620, 800), wedge(830, 400, 450, 560))
        };
        // Team: a band round the roof edge, not the whole roof.
        let band = team_frame(&outer, &inner, roof_y - 10, roof_y + 40, lod, PAINT);
        turret.add(if round { band.moved([120, 0, 0]) } else { band });
        if lod.has(1) {
            turret.add(closed_box([1250, 140, -480], [1620, 560, 480], PAINT2));
        }
        turret.add(
            lathe(&[(30, 0), (30, 1000)], 4, &[METAL], None, None).moved([-900, roof_y, -600]),
        );
        if lod.has(0) {
            turret.add(hatch([-300, roof_y, 380], 270, 8, PAINT2));
            turret.add(hatch([-300, roof_y, -380], 230, 8, PAINT));
            for i in 0..3 {
                turret.add(both_sides(
                    lathe_x(&[(55, 0), (55, 230)], 4, &[METAL], Some(RUBBER), None).moved([
                        600,
                        380,
                        1160 - i * 150,
                    ]),
                ));
            }
            turret.add(both_sides(closed_box(
                [-1100, 260, 1130],
                [100, 560, 1240],
                PAINT2,
            )));
            turret.add(closed_box([-1500, 200, -700], [-1300, 600, 700], PAINT2));
            if !round {
                turret.add(both_sides(closed_box(
                    [-900, 180, 1100],
                    [-300, 220, 1190],
                    ACCENT,
                )));
            }
        }
        let barrel_mesh = {
            let mut b = lathe_x(
                &if lod.has(1) {
                    vec![
                        (150, 0),
                        (150, 1500),
                        (200, 1560),
                        (200, 1950),
                        (160, 2010),
                        (160, 2500),
                    ]
                } else {
                    vec![(160, 0), (160, 2500)]
                },
                lod.segs(8),
                &[METAL],
                None,
                None,
            );
            if !round && lod.has(1) {
                b.add(closed_box([2300, -190, -190], [2650, 190, 190], METAL));
            }
            if lod.has(0) {
                b.add(closed_box([-50, -230, -300], [300, 230, 300], PAINT2));
            }
            b
        };
        let muzzle = if round { 2500 } else { 2650 };
        let barrel = Node::part("barrel", [1250, 330, 0], barrel_mesh)
            .child(Node::empty("muzzle", [muzzle, 0, 0]));
        let turret = Node::part("turret", [-250, 1300, 0], turret).child(barrel);
        vec![Node::part("hull", [0, 0, 0], hull).child(turret)]
    })
}

pub fn meridian_heavy_tank() -> Model {
    heavy_tank(Style::Round)
}

pub fn veyran_heavy_tank() -> Model {
    heavy_tank(Style::Angular)
}
