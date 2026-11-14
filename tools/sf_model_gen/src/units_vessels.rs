//! The Veyran gunboat, the neutral ferry and the Veyran heavy gun team (WP11.6b,
//! docs/art-style.md). A vessel's lowest point is the waterline, `Y = 0`.

use crate::kit::{
    Lod, Mesh, Model, Node, bolt, chamfer_rect, closed_box, cuboid, extrude_z1, hatch, lathe,
    lathe_x, lathe_z, loft, railing, seam_top,
};
use crate::models::{both_sides, team_frame, team_strip, wedge};
use crate::palette::{Palette, slot::*};

/// Bollards along a line of `x` at `z`: short round posts with a flared head.
fn bollards(lod: Lod, xs: &[i32], y: i32, z: i32) -> Mesh {
    let mut m = Mesh::new();
    if lod.has(0) {
        for &x in xs {
            m.add(
                lathe(&[(90, 0), (90, 230), (130, 290)], 4, &[METAL], Some(METAL), None)
                    .moved([x, y, z]),
            );
        }
    }
    m
}

/// Veyran gunboat: a long wedge hull with a pointed bow, a stepped bridge with a slit window and
/// a mast, an eight-sided funnel with a crimson glow band, a forward turret with a long gun and
/// an aft mount. Team: narrow stripes across the decks and a band round the bridge roof.
pub fn veyran_gunboat() -> Model {
    let pal = Palette::shipped();
    Model::new("veyran_gunboat", pal.materials("veyran"), |lod| {
        let mut hull = loft(
            &[
                (wedge(8200, 5000, 3600, 1000), -450),
                (wedge(8800, 5400, 3500, 1500), 0),
                (wedge(9000, 5600, 3400, 2300), 900),
                (wedge(9000, 5600, 3400, 2450), 1400),
            ],
            &[PAINT2, PAINT, PAINT],
            Some(PAINT2),
            None,
        );
        let deck = 1400;
        // Team stripes across the decks: aft, between funnel and bridge is superstructure,
        // fore. Each is a narrow band, 300 mm wide.
        let stripes: &[i32] = &[
            -8700, -8100, -5700, -5100, -4650, 8300, 1500, 2100, 2700, 5300, 5900, 6500, 7100, 7700,
        ];
        for &x in stripes {
            if x > 7500 && !lod.has(1) {
                continue; // the far view keeps the stripes the eye can count
            }
            // The deck narrows towards the bow from x = 5600 (2450 to 1225 over 3400 mm).
            let edge = 2450 - (x + 300 - 5600).max(0) * 1225 / 3400;
            let hz = edge - 200;
            hull.add(team_strip([x, deck, -hz], [x + 300, deck + 18, hz], lod));
        }
        // The bridge: two steps, a sloped front, slit windows in crimson.
        hull.add(loft(
            &if lod.has(1) {
                vec![
                    (wedge(1700, 800, 800, 1500), deck),
                    (wedge(1700, 800, 800, 1500), deck + 700),
                    (wedge(1250, 520, 650, 1050), deck + 1100),
                ]
            } else {
                vec![
                    (wedge(1700, 800, 800, 1500), deck),
                    (wedge(1250, 520, 650, 1050), deck + 1100),
                ]
            },
            if lod.has(1) { &[PAINT, PAINT][..] } else { &[PAINT][..] },
            Some(PAINT),
            None,
        ).moved([-300, 0, 0]));
        let roof_y = deck + 1100;
        hull.add(
            team_frame(
                &wedge(1240, 510, 640, 1040),
                &wedge(960, 340, 380, 760),
                roof_y - 10,
                roof_y + 40,
                lod,
                PAINT,
            )
            .moved([-300, 0, 0]),
        );
        // The funnel: octagonal, with a crimson glow band and a team flange.
        let seg = if lod.has(1) { 8 } else { 4 };
        let funnel: Vec<(i32, i32)> = if lod.has(1) {
            vec![(620, 0), (560, 700), (600, 760), (600, 840), (520, 900), (520, 1150)]
        } else {
            vec![(620, 0), (520, 1150)]
        };
        hull.add(
            lathe(&funnel, seg, &[PAINT2, PAINT2, ACCENT, PAINT2, PAINT2], Some(METAL), None)
                .moved([-3700, deck, 0]),
        );
        if lod.has(1) {
            hull.add(both_sides(closed_box(
                [-1300, deck + 750, 1450],
                [100, deck + 900, 1480],
                ACCENT,
            )));
            // The mast with a radar bar.
            hull.add(lathe(&[(40, 0), (40, 1300)], 4, &[METAL], None, None).moved([-700, roof_y, 0]));
            hull.add(closed_box([-900, roof_y + 1100, -500], [-500, roof_y + 1200, 500], METAL));
        }
        // The aft mount: a low gun house with a short fixed barrel.
        if lod.has(1) {
        hull.add(loft(
            &[
                (wedge(600, 300, 300, 650), deck),
                (wedge(500, 250, 250, 480), deck + 450),
            ],
            &[PAINT],
            Some(PAINT),
            None,
        ).moved([-6800, 0, 0]));
            hull.add(lathe_x(&[(70, 0), (70, 900)], 4, &[METAL], Some(METAL), None).moved([-6200, deck + 250, 0]));
        }
        if lod.has(0) {
            // Rails along both sides, bollards, hatches, vents and tie-down bars.
            for (x0, x1) in [(-8600, -5200), (2000, 6600)] {
                for sign in [-1, 1] {
                    let z = sign * 2380;
                    hull.add(railing([x0, deck, z], [x1, deck, z], 500, 9, METAL));
                }
            }
            hull.add(bollards(lod, &[-8800, -6000, 1600, 5400], deck, 2250));
            hull.add(bollards(lod, &[-8800, -6000, 1600, 5400], deck, -2250));
            hull.add(hatch([-4600, deck, 900], 300, 8, PAINT2));
            hull.add(hatch([-4600, deck, -900], 300, 8, PAINT2));
            hull.add(hatch([1000, deck, 800], 280, 8, PAINT2));
            hull.add(hatch([7600, deck, 0], 260, 8, PAINT2));
            // A crimson trim band along the waterline at the bow.
            hull.add(closed_box([6400, 120, -500], [8900, 220, 500], ACCENT));
            // The anchor and a lifeboat on the foredeck.
            hull.add(closed_box([7000, deck, -300], [7500, deck + 240, 300], METAL));
            hull.add(closed_box([800, deck, -1700], [2000, deck + 500, -900], PAINT2));
        }
        // The forward turret and gun.
        let mut turret = loft(
            &[
                (wedge(1000, 500, 600, 800), 0),
                (wedge(850, 400, 500, 600), 550),
            ],
            &[PAINT],
            Some(PAINT),
            None,
        );
        turret.add(team_frame(
            &wedge(840, 395, 495, 595),
            &wedge(620, 250, 330, 380),
            540,
            590,
            lod,
            PAINT,
        ));
        if lod.has(0) {
            turret.add(hatch([-300, 550, 0], 200, 8, PAINT2));
            turret.add(both_sides(closed_box([-500, 200, 800], [300, 360, 860], ACCENT)));
        }
        let mut barrel_mesh = lathe_x(
            &if lod.has(1) {
                vec![(130, 0), (130, 1900), (170, 1950), (170, 2300), (130, 2350), (130, 2700)]
            } else {
                vec![(140, 0), (140, 2700)]
            },
            lod.segs(8),
            &[METAL],
            None,
            None,
        );
        if lod.has(1) {
            barrel_mesh.add(closed_box([2400, -170, -170], [2700, 170, 170], METAL));
        }
        let barrel = Node::part("barrel", [800, 300, 0], barrel_mesh)
            .child(Node::empty("muzzle", [2700, 0, 0]));
        let turret = Node::part("turret", [4000, deck, 0], turret).child(barrel);
        vec![Node::part("hull", [0, 0, 0], hull).child(turret)]
    })
}

/// The neutral ferry: a flat barge with ramps at both ends, lane marks, rails, bollards and a
/// wheelhouse on one side. No player colour; the third paint slot marks the lanes.
pub fn neutral_ferry() -> Model {
    let pal = Palette::shipped();
    Model::new("neutral_ferry", pal.materials("neutral"), |lod| {
        let c = |v: i32| lod.bevel(v);
        let deck = 1100;
        let mut hull = loft(
            &[
                (chamfer_rect(9600, 3600, c(500)), -350),
                (chamfer_rect(10000, 4000, c(500)), 0),
                (chamfer_rect(10500, 4450, c(600)), 700),
                (chamfer_rect(10500, 4450, c(600)), deck),
            ],
            &[PAINT2, PAINT2, PAINT2],
            Some(PAINT),
            None,
        );
        // Ramps at both ends, sloping down to the water.
        for sign in [-1, 1] {
            let ramp = [
                [sign * 10000, deck],
                [sign * 10560, deck - 400],
                [sign * 10560, deck - 330],
                [sign * 10000, deck + 60],
            ];
            let mut pts = ramp.to_vec();
            if sign < 0 {
                pts.reverse();
            }
            hull.add(extrude_z1(&pts, -3000, 3000, PAINT2));
        }
        // Lane marks along the deck and stop bars at the ramps, in the third paint.
        for z in [-1800, 0, 1800] {
            hull.add(cuboid([-9300, deck, z - 90], [9300, deck + 14, z + 90], PAINT3));
        }
        for x in [-9500, 9300] {
            hull.add(cuboid([x, deck, -3300], [x + 200, deck + 14, 3300], PAINT3));
        }
        // The wheelhouse on the +Z side: a box with windows all round, a roof overhang, a stack.
        hull.add(loft(
            &[
                (chamfer_rect(1700, 1000, c(200)), deck),
                (chamfer_rect(1700, 1000, c(200)), deck + 1500),
            ],
            &[PAINT],
            Some(PAINT2),
            None,
        ).moved([0, 0, 3100]));
        if lod.has(1) {
            hull.add(cuboid([-1850, deck + 1500, 2000], [1850, deck + 1600, 4150], PAINT2));
            for sign in [-1, 1] {
                hull.add(closed_box([-1300, deck + 700, 4100 + (sign + 1) * 0], [1300, deck + 1300, 4125], ACCENT));
            }
            hull.add(closed_box([-1300, deck + 700, 2075], [1300, deck + 1300, 2100], ACCENT));
            hull.add(lathe(&[(220, 0), (220, 900)], 8, &[METAL], Some(RUBBER), None).moved([800, deck + 1600, 3400]));
        }
        if lod.has(0) {
            // Rails along both long sides (the wheelhouse side is open at the house), bollards,
            // chain posts at the ramps, a winch drum, life rings and crates.
            for sign in [-1, 1] {
                let z = sign * 4300;
                hull.add(railing([-9600, deck, z], [-2200, deck, z], 800, 8, METAL));
                hull.add(railing([2200, deck, z], [9600, deck, z], 800, 8, METAL));
            }
            let xs: Vec<i32> = (0..8).map(|i| -9000 + i * 2600).collect();
            hull.add(bollards(lod, &xs, deck, 4200));
            hull.add(bollards(lod, &xs, deck, -4200));
            hull.add(lathe(&[(300, 0), (300, 700), (200, 760)], 8, &[METAL, METAL], Some(METAL), None).moved([-8400, deck, 2800]));
            hull.add(lathe_z(&[(300, 0), (300, 60)], 8, &[PAINT3], Some(PAINT3), None).moved([200, deck + 700, 2060]));
            for i in 0..3 {
                hull.add(closed_box([5000 + i * 700, deck, 2400], [5600 + i * 700, deck + 600, 3000], PAINT));
            }
            for i in 0..6 {
                hull.add(seam_top([-6000 + i * 300, deck, -3800], [-6000 + i * 300, deck, -3300], 60, 20, METAL));
            }
            hull.add(hatch([-4800, deck, -2400], 340, 8, PAINT2));
            hull.add(hatch([4800, deck, -2400], 340, 8, PAINT2));
            for i in 0..4 {
                hull.add(bolt([-9800, deck, -1500 + i * 1000], 50, 25, 4, METAL));
                hull.add(bolt([9800, deck, -1500 + i * 1000], 50, 25, 4, METAL));
            }
        }
        let turret = Node::empty("turret", [0, deck, 0]).child(Node::empty("barrel", [0, 0, 0]));
        vec![Node::part("hull", [0, 0, 0], hull).child(turret)]
    })
}

/// Veyran heavy gun team: a gunner in a helmet behind a short heavy gun on a carriage with a
/// shield and two wheels. The six figure nodes (torso, head, arms, legs) sit under the root like
/// the set v3 figures, with the gun as a seventh part. Team: the yoke of the vest and bands on
/// the shield and the trail.
pub fn veyran_heavy_gun_team() -> Model {
    let pal = Palette::shipped();
    Model::new("veyran_heavy_gun_team", pal.materials("veyran"), |lod| {
        let near = lod.has(1);
        let full = lod.has(0);
        // The gunner stands at x = -300, facing +X.
        let mut leg_l = Mesh::new();
        let mut leg_r = Mesh::new();
        if near {
            leg_l.add(closed_box([-440, 0, 70], [-190, 860, 250], PAINT2));
            leg_r.add(closed_box([-440, 0, -250], [-190, 860, -70], PAINT2));
            if full {
                leg_l.add(closed_box([-390, 0, 40], [-120, 120, 270], METAL));
                leg_r.add(closed_box([-390, 0, -270], [-120, 120, -40], METAL));
            }
        } else {
            leg_l.add(cuboid([-440, 0, -250], [-190, 860, 250], PAINT2));
        }
        let mut torso = Mesh::new();
        torso.add(if near {
            closed_box([-500, 860, -290], [-150, 1460, 290], PAINT)
        } else {
            cuboid([-500, 860, -290], [-150, 1460, 290], PAINT)
        });
        // Team: the vest yoke across the shoulders, two bands on the back.
        if near {
            torso.add(team_strip([-470, 1460, -280], [-200, 1478, 280], lod));
        } else {
            torso.add(team_strip([-500, 1460, -290], [-150, 1478, 290], lod));
        }
        if near {
            torso.add(team_strip([-470, 1460, -150], [-420, 1478, 150], lod));
        }
        if full {
            for i in 0..3 {
                torso.add(bolt([-150, 1000 + i * 150, -150], 30, 20, 4, METAL).turned_y(1));
            }
        }
        let mut head = Mesh::new();
        if near {
            head.add(lathe(
                &[(120, 0), (150, 90), (140, 220), (60, 290)],
                lod.segs(8),
                &[PAINT2, PAINT2, PAINT2],
                Some(PAINT2),
                None,
            ).moved([-320, 1460, 0]));
            head.add(closed_box([-190, 1560, -90], [-170, 1620, 90], ACCENT));
        } else {
            head.add(cuboid([-420, 1460, -130], [-220, 1730, 130], PAINT2));
        }
        let mut arm_l = Mesh::new();
        let mut arm_r = Mesh::new();
        if near {
            arm_l.add(closed_box([-350, 1180, 290], [250, 1300, 410], PAINT));
            arm_r.add(closed_box([-350, 1180, -410], [250, 1300, -290], PAINT));
        }
        // The gun: carriage, wheels, shield, barrel.
        let mut gun = Mesh::new();
        if near {
            gun.add(closed_box([150, 450, -300], [700, 700, 300], PAINT2));
            gun.add(both_sides(
                lathe_z(&[(300, 0), (300, 90)], lod.segs(16), &[RUBBER], Some(METAL), None).moved([300, 300, 440]),
            ));
            gun.add(closed_box([520, 700, -520], [580, 1220, 520], PAINT));
            gun.add(lathe_x(&[(95, 0), (95, 500)], lod.segs(8), &[METAL], Some(METAL), None).moved([580, 950, 0]));
            gun.add(closed_box([-80, 330, -80], [160, 400, 80], METAL));
        } else {
            gun.add(cuboid([150, 450, -520], [700, 1220, 520], PAINT));
            gun.add(cuboid([700, 900, -90], [1080, 1000, 90], METAL));
        }
        // Team on the shield top and on the trail, narrow bands.
        gun.add(team_strip([520, 1220, -520], [580, 1238, 520], lod));
        if near {
            gun.add(team_strip([260, 700, -270], [430, 718, 270], lod));
        }
        if full {
            // Pouches on the belt, an ammunition box beside the gun, a spade on the trail, an
            // elevation wheel and a breech block.
            for i in 0..4 {
                torso.add(closed_box([-520, 900, -200 + i * 110], [-460, 1020, -120 + i * 110], PAINT2));
            }
            gun.add(closed_box([-120, 0, 330], [250, 260, 560], PAINT2));
            gun.add(closed_box([-420, 0, -200], [-380, 140, 200], METAL));
            gun.add(lathe_z(&[(110, 0), (110, 40)], 8, &[METAL], Some(METAL), None).moved([300, 560, 310]));
            gun.add(closed_box([420, 800, -150], [520, 1060, 150], METAL));
            gun.add(closed_box([150, 700, -330], [700, 760, -300], METAL));
            gun.add(closed_box([150, 700, 300], [700, 760, 330], METAL));
            gun.add(closed_box([-400, 120, -60], [150, 200, 60], METAL));
            for i in 0..4 {
                gun.add(bolt([540, 1220, -300 + i * 200], 25, 15, 4, METAL));
            }
            gun.add(closed_box([540, 1000, -520], [570, 1160, 520], ACCENT).moved([0, 0, 0]));
        }
        vec![
            Node::part("leg_l", [0, 0, 0], leg_l),
            Node::part("leg_r", [0, 0, 0], leg_r),
            Node::part("torso", [0, 0, 0], torso),
            Node::part("head", [0, 0, 0], head),
            Node::part("arm_l", [0, 0, 0], arm_l),
            Node::part("arm_r", [0, 0, 0], arm_r),
            Node::part("gun", [0, 0, 0], gun),
        ]
    })
}
