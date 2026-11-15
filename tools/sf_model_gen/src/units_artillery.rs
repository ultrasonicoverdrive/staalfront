//! Siege artillery of both factions (WP11.6b, docs/art-style.md).

use crate::kit::{
    Model, Node, bevel_box, bolt, chamfer_rect, closed_box, extrude_z1, hatch, lathe_x, loft,
    seam_top,
};
use crate::models::{Track, both_sides, team_frame, team_strip, wedge};
use crate::palette::{Palette, slot::*};
use crate::units::{Style, ring};

/// Siege artillery: a tracked chassis carrying a tall armoured casemate at the rear and a very
/// long gun, with a recoil sleeve and a muzzle brake; spades at the rear. Meridian rounded,
/// Veyran angular with sloped plates.
pub fn siege_artillery(style: Style) -> Model {
    let pal = Palette::shipped();
    let stem = format!("{}_siege_artillery", style.faction());
    let round = style == Style::Round;
    Model::new(&stem, pal.materials(style.faction()), |lod| {
        let track = Track {
            hx: 2750,
            z0: 920,
            z1: 1460,
            h: 740,
            wheels: 5,
            wheel_r: 330,
        };
        let c = |v: i32| lod.bevel(v);
        let mut hull = both_sides(track.mesh(lod));
        hull.add(loft(
            &[
                (ring(style, 2500, 2600, 760, c(120)), 330),
                (ring(style, 2700, 2700, 1100, c(260)), 760),
                (ring(style, 2650, 2650, 1100, c(260)), 980),
                (ring(style, 2300, 2300, 940, c(360)), 1140),
            ],
            &[PAINT, PAINT, PAINT],
            Some(PAINT),
            None,
        ));
        hull.add(both_sides(bevel_box(
            [-2650, 740, 900],
            [2700, 800, 1500],
            lod.bevel(30),
            PAINT2,
            PAINT2,
        )));
        if lod.has(1) {
            hull.add(both_sides(extrude_z1(
                &[[-2650, 360], [2400, 360], [2700, 740], [-2650, 740]],
                1460,
                1500,
                PAINT2,
            )));
        }
        // Team: two cross stripes on the front deck, one along each fender.
        hull.add(team_strip([1500, 1140, -900], [1800, 1158, 900], lod));
        if lod.has(1) {
            hull.add(team_strip([1900, 1140, -850], [2200, 1158, 850], lod));
        }
        // The fender stripes are a little narrower in the far levels, where the model is smaller.
        let fender = if lod.has(0) { 1180 } else { 1260 };
        hull.add(both_sides(team_strip(
            [-2500, 800, fender],
            [2400, 818, 1480],
            lod,
        )));
        if lod.has(0) {
            hull.add(both_sides(closed_box(
                [-2200, 420, 1500],
                [1800, 700, 1520],
                TEAM,
            )));
            for i in 0..4 {
                let x = 1000 + i * 240;
                hull.add(seam_top([x, 1158, -700], [x, 1158, 700], 50, 16, PAINT2));
            }
            hull.add(hatch([2000, 1140, 0], 260, 8, PAINT));
            // Spades at the rear, a tool box on each fender, headlights, exhausts.
            hull.add(closed_box([-3050, 0, -1000], [-2650, 260, -700], METAL));
            hull.add(closed_box([-3050, 0, 700], [-2650, 260, 1000], METAL));
            hull.add(both_sides(closed_box(
                [-1500, 800, 1250],
                [-300, 1000, 1450],
                PAINT2,
            )));
            hull.add(both_sides(closed_box(
                [2380, 880, 760],
                [2520, 990, 940],
                ACCENT,
            )));
            for z in [-450, 450] {
                hull.add(
                    lathe_x(&[(90, 0), (90, 240)], 8, &[METAL], Some(RUBBER), None)
                        .moved([-2800, 900, z]),
                );
            }
        }
        // The casemate: a tall box at the rear carrying the gun.
        let t = |v: i32| lod.bevel(v);
        let mut turret = if round {
            loft(
                &[
                    (chamfer_rect(1300, 1050, t(420)), 0),
                    (chamfer_rect(1300, 1050, t(420)), 700),
                    (chamfer_rect(1100, 850, t(360)), 1050),
                ],
                &[PAINT, PAINT],
                Some(PAINT),
                None,
            )
        } else {
            loft(
                &[
                    (wedge(1300, 600, 700, 1050), 0),
                    (wedge(1300, 600, 700, 1050), 600),
                    (wedge(1050, 480, 600, 750), 1050),
                ],
                &[PAINT, PAINT],
                Some(PAINT),
                None,
            )
        };
        let (outer, inner) = if round {
            (
                chamfer_rect(1090, 840, t(355)),
                chamfer_rect(830, 580, t(230)),
            )
        } else {
            (wedge(1040, 470, 590, 740), wedge(780, 340, 440, 500))
        };
        turret.add(team_frame(&outer, &inner, 1040, 1090, lod, PAINT));
        if lod.has(1) {
            // The gun mount at the front of the casemate.
            turret.add(closed_box([1250, 380, -420], [1700, 900, 420], PAINT2));
        }
        if lod.has(0) {
            turret.add(hatch([-400, 1090, 400], 250, 8, PAINT2));
            turret.add(hatch([-400, 1090, -400], 220, 8, PAINT));
            for i in 0..4 {
                turret.add(bolt([1000, 1090, -270 + i * 180], 35, 20, 4, METAL));
            }
            turret.add(both_sides(closed_box(
                [-900, 300, 1040],
                [200, 800, 1130],
                PAINT2,
            )));
            if !round {
                turret.add(both_sides(closed_box(
                    [-800, 200, 1040],
                    [-300, 240, 1100],
                    ACCENT,
                )));
            }
        }
        // The long gun: a recoil sleeve, the barrel, a muzzle brake.
        let barrel_mesh = {
            let mut b = lathe_x(
                &if lod.has(1) {
                    vec![
                        (200, 0),
                        (200, 900),
                        (130, 940),
                        (130, 4600),
                        (180, 4650),
                        (180, 5200),
                        (140, 5260),
                        (140, 5500),
                    ]
                } else {
                    vec![(180, 0), (180, 5500)]
                },
                lod.segs(8),
                &[METAL],
                None,
                None,
            );
            if lod.has(0) {
                b.add(closed_box([5100, -150, -220], [5500, 150, 220], METAL));
            }
            b
        };
        let barrel = Node::part("barrel", [1200, 640, 0], barrel_mesh)
            .child(Node::empty("muzzle", [5500, 0, 0]));
        let turret = Node::part("turret", [-1300, 1140, 0], turret).child(barrel);
        vec![Node::part("hull", [0, 0, 0], hull).child(turret)]
    })
}

pub fn meridian_siege_artillery() -> Model {
    siege_artillery(Style::Round)
}

pub fn veyran_siege_artillery() -> Model {
    siege_artillery(Style::Angular)
}
