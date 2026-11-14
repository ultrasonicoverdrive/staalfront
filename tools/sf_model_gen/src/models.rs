//! The model recipes of set v5 (WP11.6a, style G "semi-realistic, bright", docs/art-style.md).
//!
//! Six models prove the style: the light tank of both factions, a Meridian harvester, a Veyran
//! power plant, a Meridian construction center and a Veyran defence tower, so both shape
//! languages show (Meridian rounded, Veyran angular with sloped plates). Sizes stay inside the
//! set v4 models they replace (pinned in `tests/models.rs`).
//!
//! Every recipe builds all three levels from one function of [`Lod`]: round parts lose segments,
//! bevels shrink and go, and details (wheels, cleats, seams, bolts, hatches) drop out, while the
//! node tree and the team panels stay in every level.

use crate::kit::{
    Lod, Mat, Mesh, Model, Node, P, bevel_box, bolt, chamfer_rect, closed_box, cuboid, extrude_z1,
    hatch, lathe, lathe_x, lathe_z, loft, ngon, plate, seam_side_z, seam_top,
};
use crate::palette::{Palette, slot::*};

/// The recipes of set v5, sorted by file name.
pub const MODELS: &[fn() -> Model] = &[
    meridian_construction_center,
    meridian_harvester,
    meridian_light_tank,
    veyran_defence_tower,
    veyran_light_tank,
    veyran_power_plant,
];

// ---- Shared parts ------------------------------------------------------------------------

/// Mirror a right-side (`+Z`) mesh to both sides.
fn both_sides(right: Mesh) -> Mesh {
    right.clone().with(right.mirrored_z())
}

/// One track run on the right side: a rubber band with sloped ends from `-hx` to `hx`, between
/// `z0` and `z1`, `h` high; cleats on its top run and road wheels on its outer face at LOD0, the
/// wheels with fewer segments at LOD1; a plain block at LOD2.
struct Track {
    hx: i32,
    z0: i32,
    z1: i32,
    h: i32,
    wheels: i32,
    wheel_r: i32,
}

impl Track {
    fn mesh(&self, lod: Lod) -> Mesh {
        let Track {
            hx,
            z0,
            z1,
            h,
            wheels,
            wheel_r,
        } = *self;
        if !lod.has(1) {
            return cuboid([-hx, 0, z0], [hx, h, z1], RUBBER);
        }
        let end = h * 2 / 3;
        let outline = [
            [-hx, h * 2 / 3],
            [-hx + end / 2, h / 8],
            [-hx + end, 0],
            [hx - end, 0],
            [hx - end / 2, h / 8],
            [hx, h * 2 / 3],
            [hx - end / 3, h],
            [-hx + end / 3, h],
        ];
        let mut m = extrude_z1(&outline, z0, z1, RUBBER);
        let count = if lod.has(0) { wheels } else { wheels.min(3) };
        let span = 2 * (hx - end) - 2 * wheel_r;
        for i in 0..count {
            let x = -(hx - end - wheel_r) + span * i / (count - 1).max(1);
            let wheel = lathe_z(
                &[(wheel_r, 0), (wheel_r, 50)],
                lod.segs(16),
                &[METAL],
                None,
                None,
            )
            .moved([x, wheel_r + 20, z1]);
            m.add(wheel);
            if lod.has(0) {
                m.add(
                    lathe_z(
                        &[(wheel_r / 3, 0), (wheel_r / 3, 30)],
                        4,
                        &[RUBBER],
                        None,
                        None,
                    )
                    .moved([x, wheel_r + 20, z1 + 50]),
                );
            }
        }
        if lod.has(0) {
            // Track links: cleats across the top run.
            let n = (2 * (hx - end / 3)) / 600;
            for i in 0..n {
                let x = -(hx - end / 3) + 300 + i * 600;
                m.add(cuboid(
                    [x - 70, h, z0 + 20],
                    [x + 70, h + 40, z1 - 20],
                    RUBBER,
                ));
            }
        }
        m
    }
}

/// A wedge outline (pointed nose): back at `-hx`, the sides run to `fx`, the nose to `fx + nose`.
fn wedge(hx: i32, fx: i32, nose: i32, hz: i32) -> Vec<[i32; 2]> {
    vec![
        [-hx, -hz],
        [fx, -hz],
        [fx + nose, -hz / 2],
        [fx + nose, hz / 2],
        [fx, hz],
        [-hx, hz],
    ]
}

/// Smoke and fire anchors of a structure. Vehicles have none yet: without them a hurt vehicle smokes by
/// the engine's rule, as every other vehicle does (`docs/art-style.md` section 7).
fn anchors(n: Node, smoke: P, fire: P) -> Node {
    n.child(Node::empty("fx_smoke_0", smoke))
        .child(Node::empty("fx_fire_0", fire))
}

// ---- Light tanks -------------------------------------------------------------------------

/// Meridian light tank: a rounded cast turret, a rounded hull with fenders, five road wheels.
pub fn meridian_light_tank() -> Model {
    let pal = Palette::shipped();
    Model::new("meridian_light_tank", pal.materials("meridian"), |lod| {
        let b = lod.bevel(1);
        let track = Track {
            hx: 2450,
            z0: 820,
            z1: 1370,
            h: 640,
            wheels: 5,
            wheel_r: 300,
        };
        let mut hull = both_sides(track.mesh(lod));
        // Rounded hull: a sloped lower front, chamfered corners all round.
        let c = |v: i32| v * b;
        hull.add(loft(
            &[
                (chamfer_rect(2100, 800, 100 * b), 300),
                (chamfer_rect(2400, 1200, c(260)), 700),
                (chamfer_rect(2350, 1200, c(260)), 900),
                (chamfer_rect(2100, 1000, c(340)), 1060),
            ],
            &[PAINT, PAINT, PAINT],
            Some(PAINT),
            None,
        ));
        // Fenders over the tracks, olive.
        hull.add(both_sides(bevel_box(
            [-2350, 640, 800],
            [2380, 700, 1420],
            lod.bevel(30),
            PAINT2,
            PAINT2,
        )));
        // Team: the engine deck and the hull side panels.
        hull.add(plate(
            [-2100, 1060, -900],
            [-700, 1095, 900],
            lod.bevel(40),
            TEAM,
        ));
        if lod.has(1) {
            hull.add(both_sides(closed_box(
                [-1700, 720, 1200],
                [1300, 880, 1230],
                TEAM,
            )));
        }
        if lod.has(0) {
            // Engine louvres, a driver's hatch, bolts on the glacis, tow hooks.
            for i in 0..5 {
                let x = -1950 + i * 230;
                hull.add(seam_top([x, 1095, -650], [x, 1095, 650], 50, 18, PAINT2));
            }
            hull.add(hatch([1500, 1060, -450], 260, 8, PAINT));
            for i in 0..4 {
                hull.add(bolt([1700, 1060, -600 + i * 400], 40, 25, 4, METAL));
            }
            hull.add(seam_top(
                [1900, 1060, -900],
                [1900, 1060, 900],
                40,
                12,
                PAINT,
            ));
            hull.add(both_sides(seam_side_z(-200, 720, 880, 1230, 40, 10, PAINT)));
            hull.add(both_sides(closed_box(
                [2300, 400, 500],
                [2480, 480, 700],
                METAL,
            )));
        }
        let mut turret = loft(
            &[
                (chamfer_rect(1150, 980, c(340).max(0)), 0),
                (chamfer_rect(1220, 1040, c(380)), 260),
                (chamfer_rect(1000, 860, c(330)), 600),
            ],
            &[PAINT, PAINT],
            Some(PAINT),
            None,
        )
        .moved([80, 0, 0]);
        turret.add(plate(
            [-880, 600, -780],
            [1000, 640, 780],
            lod.bevel(40),
            TEAM,
        ));
        turret.add(closed_box([1050, 120, -380], [1380, 500, 380], PAINT2));
        turret.add(lathe(&[(30, 0), (30, 900)], 4, &[METAL], None, None).moved([-800, 640, -560]));
        if lod.has(0) {
            turret.add(hatch([-250, 640, 330], 280, 8, PAINT2));
            turret.add(both_sides(closed_box(
                [-900, 250, 1040],
                [100, 480, 1150],
                PAINT2,
            )));
            for i in 0..4 {
                turret.add(bolt([1200, 500, -270 + i * 180], 35, 20, 4, METAL));
            }
        }
        let barrel = lathe_x(
            &if lod.has(1) {
                vec![(118, 0), (118, 1480), (165, 1520), (165, 1820)]
            } else {
                vec![(130, 0), (130, 1820)]
            },
            lod.segs(16),
            &[METAL],
            None,
            None,
        );
        let barrel =
            Node::part("barrel", [1150, 310, 0], barrel).child(Node::empty("muzzle", [1830, 0, 0]));
        let turret = Node::part("turret", [-420, 1060, 0], turret).child(barrel);
        let hull = Node::part("hull", [0, 0, 0], hull).child(turret);
        vec![hull]
    })
}

/// Veyran light tank: a low wedge turret, a pointed hull of sloped plates, armour skirts.
pub fn veyran_light_tank() -> Model {
    let pal = Palette::shipped();
    Model::new("veyran_light_tank", pal.materials("veyran"), |lod| {
        let track = Track {
            hx: 2400,
            z0: 820,
            z1: 1330,
            h: 620,
            wheels: 6,
            wheel_r: 260,
        };
        let mut hull = both_sides(track.mesh(lod));
        hull.add(loft(
            &[
                (wedge(2100, 1600, 400, 800), 300),
                (wedge(2350, 1800, 500, 1220), 680),
                (wedge(2200, 1450, 450, 900), 1000),
            ],
            &[PAINT, PAINT],
            Some(PAINT),
            None,
        ));
        // Armour skirts over the upper run, gunmetal.
        let skirt = [[-2400, 330], [2150, 330], [2420, 620], [-2380, 700]];
        if lod.has(1) {
            hull.add(both_sides(extrude_z1(&skirt, 1330, 1400, PAINT2)));
        }
        hull.add(plate(
            [-2150, 1000, -880],
            [-650, 1035, 880],
            lod.bevel(40),
            TEAM,
        ));
        if lod.has(1) {
            hull.add(both_sides(closed_box(
                [-1600, 420, 1400],
                [1200, 620, 1430],
                TEAM,
            )));
        }
        if lod.has(0) {
            for i in 0..4 {
                let z = -480 + i * 320;
                hull.add(seam_top([-1950, 1035, z], [-800, 1035, z], 40, 14, PAINT2));
            }
            hull.add(seam_top(
                [1300, 1000, -800],
                [1300, 1000, 800],
                40,
                14,
                PAINT2,
            ));
            hull.add(hatch([900, 1000, -420], 240, 8, PAINT2));
            for i in 0..5 {
                hull.add(bolt([-2000 + i * 900, 650, 1400], 35, 20, 4, METAL).toned(720));
            }
            // A crimson trim at the rear: the fixed faction mark, small and dark.
            hull.add(closed_box([-2230, 820, -600], [-2190, 900, 600], ACCENT));
        }
        let mut turret = loft(
            &[
                (wedge(1100, 600, 550, 960), 0),
                (wedge(900, 450, 480, 700), 470),
            ],
            &[PAINT],
            Some(PAINT),
            None,
        );
        turret.add(loft(
            &[
                (wedge(880, 430, 470, 680), 470),
                (wedge(880, 430, 470, 680), 505),
            ],
            &[TEAM],
            Some(TEAM),
            None,
        ));
        turret.add(lathe(&[(25, 0), (25, 800)], 4, &[METAL], None, None).moved([-850, 505, -500]));
        if lod.has(0) {
            turret.add(both_sides(closed_box(
                [-900, 180, 900],
                [-300, 220, 990],
                ACCENT,
            )));
            turret.add(cuboid([-650, 505, 150], [-350, 680, 450], PAINT2));
            turret.add(seam_top(
                [-600, 505, -400],
                [100, 505, -400],
                30,
                10,
                PAINT2,
            ));
        }
        let mut barrel = lathe_x(&[(110, 0), (110, 1700)], lod.segs(8), &[METAL], None, None);
        barrel.add(closed_box([1580, -150, -150], [1880, 150, 150], METAL));
        if lod.has(0) {
            barrel.add(closed_box([-50, -200, -230], [250, 200, 230], PAINT2));
        }
        let barrel =
            Node::part("barrel", [1150, 230, 0], barrel).child(Node::empty("muzzle", [1880, 0, 0]));
        let turret = Node::part("turret", [-500, 1000, 0], turret).child(barrel);
        let hull = Node::part("hull", [0, 0, 0], hull).child(turret);
        vec![hull]
    })
}

// ---- Harvester ---------------------------------------------------------------------------

/// Meridian harvester: a tracked chassis, a cab with windows at the front, an ore drum with team
/// bands, a scoop. Empty `turret` and `barrel` nodes (the brief: a vehicle keeps them).
pub fn meridian_harvester() -> Model {
    let pal = Palette::shipped();
    Model::new("meridian_harvester", pal.materials("meridian"), |lod| {
        let track = Track {
            hx: 3000,
            z0: 1000,
            z1: 1640,
            h: 720,
            wheels: 5,
            wheel_r: 320,
        };
        let mut hull = both_sides(track.mesh(lod));
        hull.add(bevel_box(
            [-3100, 600, -1550],
            [3050, 900, 1550],
            lod.bevel(80),
            PAINT2,
            PAINT2,
        ));
        // Cab.
        let b = lod.bevel(1);
        hull.add(
            loft(
                &[
                    (chamfer_rect(700, 1400, 200 * b), 900),
                    (chamfer_rect(700, 1400, 200 * b), 1700),
                    (chamfer_rect(560, 1300, 260 * b), 2250),
                ],
                &[PAINT, PAINT],
                Some(TEAM),
                None,
            )
            .moved([2280, 0, 0]),
        );
        hull.add(closed_box([2930, 1300, -1100], [2985, 1650, 1100], ACCENT));
        // Scoop at the front.
        hull.add(bevel_box(
            [2950, 80, -1600],
            [3200, 650, 1600],
            lod.bevel(60),
            METAL,
            METAL,
        ));
        // The ore drum lying along X, team bands.
        let r = 1250;
        let drum = if lod.has(1) {
            lathe_x(
                &[
                    (900, 0),
                    (r, 300),
                    (r, 700),
                    (r, 1300),
                    (r, 2300),
                    (r, 2900),
                    (r, 3300),
                    (900, 3600),
                ],
                lod.segs(16),
                &[PAINT2, PAINT2, TEAM, PAINT2, TEAM, PAINT2, PAINT2],
                Some(METAL),
                Some(METAL),
            )
        } else {
            lathe_x(
                &[(r, 0), (r, 700), (r, 1300), (r, 2300), (r, 2900), (r, 3600)],
                lod.segs(16),
                &[PAINT2, TEAM, PAINT2, TEAM, PAINT2],
                Some(PAINT2),
                Some(PAINT2),
            )
        };
        hull.add(drum.moved([-3050, 2150, 0]));
        hull.add(closed_box([-2700, 900, -900], [-2400, 1200, 900], METAL));
        hull.add(closed_box([0, 900, -900], [300, 1200, 900], METAL));
        if lod.has(0) {
            hull.add(hatch([2250, 2250, 0], 300, 8, PAINT));
            for i in 0..4 {
                let x = -2900 + i * 300;
                hull.add(seam_side_z(x, 650, 880, 1550, 50, 12, PAINT2));
                hull.add(seam_side_z(x, 650, 880, -1550, 50, 12, PAINT2));
            }
            hull.add(
                lathe(
                    &[(110, 0), (110, 1600), (140, 1650)],
                    8,
                    &[METAL],
                    None,
                    None,
                )
                .moved([1500, 900, 1200]),
            );
            for i in 0..4 {
                hull.add(bolt([3000, 650, -1200 + i * 800], 45, 25, 4, METAL));
            }
            hull.add(both_sides(closed_box(
                [2400, 1000, 1400],
                [2700, 1600, 1440],
                PAINT2,
            )));
        }
        let turret = Node::empty("turret", [0, 900, 0]).child(Node::empty("barrel", [0, 0, 0]));
        let hull = Node::part("hull", [0, 0, 0], hull).child(turret);
        vec![hull]
    })
}

// ---- Buildings ---------------------------------------------------------------------------

/// Veyran power plant: a stepped hall with sloped roof facets (team), two octagonal stacks with
/// a crimson glow band, pipes and vents.
pub fn veyran_power_plant() -> Model {
    let pal = Palette::shipped();
    Model::new("veyran_power_plant", pal.materials("veyran"), |lod| {
        let mut body = bevel_box(
            [-3700, 0, -5600],
            [3700, 400, 5600],
            lod.bevel(120),
            PAINT2,
            PAINT2,
        );
        body.add(
            loft(
                &[
                    (chamfer_rect(3200, 3800, lod.bevel(300)), 400),
                    (chamfer_rect(3200, 3800, lod.bevel(300)), 2800),
                    (chamfer_rect(2400, 3000, lod.bevel(300)), 3700),
                ],
                &[PAINT, TEAM],
                Some(PAINT),
                None,
            )
            .moved([0, 0, -1500]),
        );
        let stack = |r: i32| -> Mesh {
            let segs = 8;
            let rings = if lod.has(1) {
                vec![
                    (ngon(r, segs, true), 400),
                    (ngon(r * 4 / 5, segs, true), 5600),
                    (ngon(r * 4 / 5, segs, true), 5900),
                    (ngon(r * 4 / 5, segs, true), 6200),
                    (ngon(r * 9 / 10, segs, true), 6500),
                ]
            } else {
                vec![
                    (ngon(r, segs, true), 400),
                    (ngon(r * 4 / 5, segs, true), 6500),
                ]
            };
            let bands: Vec<Mat> = if lod.has(1) {
                vec![PAINT2, ACCENT, PAINT2, PAINT2]
            } else {
                vec![PAINT2]
            };
            loft(&rings, &bands, Some(METAL), None)
        };
        body.add(stack(900).moved([-1700, 0, 4100]));
        body.add(stack(900).moved([1700, 0, 4100]));
        body.add(plate(
            [-1700, 3700, -3800],
            [1700, 3760, 0],
            lod.bevel(40),
            TEAM,
        ));
        if lod.has(1) {
            // Pipes from the hall to the stacks.
            for x in [-1700, 1700] {
                body.add(
                    lathe(&[(220, 0), (220, 1600)], lod.segs(8), &[METAL], None, None)
                        .turned_y(0)
                        .moved([x, 1600, 2000])
                        .clone(),
                );
                body.add(closed_box(
                    [x - 250, 1400, 2300],
                    [x + 250, 1900, 3300],
                    METAL,
                ));
            }
            body.add(cuboid([-2600, 3700, 1000], [-1400, 4300, 1900], PAINT2));
            body.add(cuboid([1400, 3700, 1000], [2600, 4300, 1900], PAINT2));
        }
        if lod.has(0) {
            // Vents along both long sides, a door, rooftop fans, bolts.
            for i in 0..8 {
                let z = -5000 + i * 800;
                for x in [3200, -3200] {
                    let dx = if x > 0 { 20 } else { -20 };
                    body.add(closed_box(
                        [x.min(x + dx), 900, z],
                        [x.max(x + dx), 2200, z + 450],
                        PAINT2,
                    ));
                }
            }
            body.add(closed_box([-1000, 400, -5330], [1000, 2400, -5290], METAL));
            body.add(closed_box(
                [-1100, 2400, -5360],
                [1100, 2550, -5280],
                ACCENT,
            ));
            for (x, z) in [(-2000, 1450), (2000, 1450)] {
                body.add(
                    lathe(&[(420, 0), (420, 120)], 16, &[METAL], None, None).moved([x, 4300, z]),
                );
                body.add(hatch([x, 4420, z], 380, 16, PAINT2));
            }
            for i in 0..10 {
                body.add(bolt([-3500 + i * 780, 400, -5450], 50, 30, 4, METAL));
                body.add(bolt([-3500 + i * 780, 400, 5450], 50, 30, 4, METAL));
            }
            for i in 0..6 {
                let x = -1500 + i * 600;
                body.add(seam_top([x, 3760, -3700], [x, 3760, 700], 50, 12, PAINT));
            }
        }
        let body = Node::part("body", [0, 0, 0], body);
        vec![anchors(body, [-1700, 6500, 4100], [0, 3800, -1500])]
    })
}

/// Meridian construction center: a broad platform, a rounded main hall with a dome (team band),
/// two round tanks with team caps, a crane, a landing pad.
pub fn meridian_construction_center() -> Model {
    let pal = Palette::shipped();
    Model::new(
        "meridian_construction_center",
        pal.materials("meridian"),
        |lod| {
            let mut body = bevel_box(
                [-7400, 0, -7400],
                [7400, 500, 7400],
                lod.bevel(200),
                PAINT2,
                PAINT2,
            );
            body.add(
                loft(
                    &[
                        (chamfer_rect(5000, 4600, lod.bevel(900)), 500),
                        (chamfer_rect(5000, 4600, lod.bevel(900)), 3200),
                        (chamfer_rect(4800, 4400, lod.bevel(900)), 3500),
                    ],
                    &[PAINT, PAINT],
                    Some(TEAM),
                    None,
                )
                .moved([-1600, 0, -1500]),
            );
            // The dome.
            let dome = lathe(
                &[
                    (3000, 0),
                    (2850, 900),
                    (2400, 1700),
                    (1600, 2300),
                    (700, 2600),
                    (0, 2700),
                ],
                lod.segs(32),
                &[PAINT, PAINT, PAINT, PAINT, PAINT],
                None,
                None,
            );
            let ring = lathe(
                &[(3150, 0), (3150, 260), (3000, 300)],
                lod.segs(32),
                &[PAINT2],
                None,
                None,
            );
            body.add(dome.with(ring).moved([-1600, 3500, -1500]));
            // Two round tanks.
            for (x, z) in [(4600, 3800), (4600, 800)] {
                body.add(
                    lathe(
                        &[(1200, 0), (1200, 3000), (1000, 3400), (650, 3550)],
                        lod.segs(16),
                        &[PAINT2, PAINT2, TEAM],
                        Some(TEAM),
                        None,
                    )
                    .moved([x, 500, z]),
                );
            }
            // Landing pad.
            body.add(bevel_box(
                [2200, 500, -6800],
                [6800, 650, -2200],
                lod.bevel(80),
                METAL,
                METAL,
            ));
            // Crane: a mast and an arm (in every level: it sets the height).
            body.add(closed_box([-6600, 500, 4600], [-6000, 6500, 5200], PAINT));
            body.add(closed_box([-6800, 6500, 3000], [-1000, 7000, 5400], PAINT));
            if lod.has(1) {
                body.add(plate(
                    [3500, 650, -5500],
                    [5500, 680, -3500],
                    lod.bevel(40),
                    TEAM,
                ));
                body.add(closed_box([-1300, 4000, 4100], [-1150, 6500, 4250], METAL));
            }
            if lod.has(0) {
                // Doors, windows, vents, rails, bolts.
                body.add(closed_box([3380, 500, -3600], [3440, 2600, 600], METAL));
                for i in 0..7 {
                    let x = -6000 + i * 1300;
                    body.add(closed_box([x, 2000, -6130], [x + 800, 2700, -6080], ACCENT));
                    body.add(seam_side_z(x - 250, 600, 3150, -6100, 60, 15, PAINT));
                }
                for i in 0..8 {
                    let z = -5800 + i * 1200;
                    body.add(closed_box([-6630, 1000, z], [-6580, 2400, z + 600], PAINT2));
                }
                for i in 0..18 {
                    let t = -7000 + i * 800;
                    body.add(bolt([t, 500, -7250], 60, 35, 4, METAL));
                    body.add(bolt([t, 500, 7250], 60, 35, 4, METAL));
                }
                body.add(hatch([-4200, 3500, 1800], 600, 16, PAINT2));
                body.add(hatch([1300, 3500, 1800], 600, 16, PAINT2));
                for i in 0..5 {
                    let z = 3200 + i * 400;
                    body.add(seam_top([-6600, 7000, z], [-1000, 7000, z], 60, 20, METAL));
                }
                for (x, z) in [(4600, 3800), (4600, 800)] {
                    body.add(
                        lathe(&[(1230, 0), (1230, 120)], 16, &[METAL], None, None)
                            .moved([x, 2000, z]),
                    );
                }
                body.add(
                    lathe(&[(80, 0), (80, 700)], 8, &[METAL], None, None)
                        .moved([-1600, 6150, -1500]),
                );
            }
            let body = Node::part("body", [0, 0, 0], body);
            vec![anchors(body, [-1600, 6000, -1500], [4600, 4000, 3800])]
        },
    )
}

/// Veyran defence tower: a sloped octagonal bunker with a crimson slit, a sharp wedge gun head.
pub fn veyran_defence_tower() -> Model {
    let pal = Palette::shipped();
    Model::new("veyran_defence_tower", pal.materials("veyran"), |lod| {
        let b = lod.bevel(1);
        let k = i32::from(lod.has(1));
        let mut body = loft(
            &[
                (chamfer_rect(1850, 1850, 520 * k), 0),
                (chamfer_rect(1550, 1550, 440 * k), 1700),
                (chamfer_rect(1550, 1550, 440 * k), 1900),
                (chamfer_rect(1350, 1350, 380 * k), 2150),
            ],
            &[PAINT, PAINT2, TEAM],
            Some(PAINT2),
            None,
        );
        if lod.has(1) {
            body.add(both_sides(closed_box(
                [-700, 1100, 1600 - 40 * b],
                [700, 1250, 1660],
                ACCENT,
            )));
        }
        if lod.has(0) {
            for i in 0..4 {
                let x = -1300 + i * 860;
                body.add(seam_side_z(x, 200, 1600, 1690, 60, 16, PAINT2));
                body.add(seam_side_z(x, 200, 1600, -1690, 60, 16, PAINT2));
            }
            for i in 0..8 {
                body.add(bolt([-1400 + i * 400, 1900, 1450], 40, 25, 4, METAL));
                body.add(bolt([-1400 + i * 400, 1900, -1450], 40, 25, 4, METAL));
            }
            body.add(closed_box([-1890, 0, -500], [-1820, 1100, 500], METAL));
            // Armour fins on the corners, louvres on the back, a plinth, lamps on the ledge.
            for q in 0..4u8 {
                body.add(closed_box([1180, 0, 1180], [1420, 1500, 1420], PAINT2).turned_y(q));
                body.add(bolt([1450, 1900, 1450], 70, 60, 8, ACCENT).turned_y(q));
            }
            for i in 0..6 {
                let y = 300 + i * 180;
                body.add(closed_box([-1760, y, -1100], [-1700, y + 60, 1100], PAINT2));
            }
            body.add(bevel_box(
                [-1900, 0, -1880],
                [1900, 120, 1880],
                60,
                METAL,
                METAL,
            ));
            for i in 0..5 {
                let y = 300 + i * 300;
                body.add(closed_box([-1880, y, 560], [-1820, y + 40, 900], METAL));
            }
        }
        let mut turret = loft(
            &[
                (wedge(950, 450, 450, 900), 0),
                (wedge(800, 300, 400, 680), 650),
            ],
            &[PAINT],
            Some(PAINT),
            None,
        );
        turret.add(loft(
            &[
                (wedge(620, 180, 320, 500), 650),
                (wedge(620, 180, 320, 500), 690),
            ],
            &[TEAM],
            Some(TEAM),
            None,
        ));
        turret.add(lathe(&[(25, 0), (25, 700)], 4, &[METAL], None, None).moved([-800, 690, 420]));
        if lod.has(0) {
            turret.add(both_sides(closed_box(
                [-700, 250, 880],
                [200, 330, 930],
                ACCENT,
            )));
            turret.add(cuboid([-550, 690, -250], [-250, 900, 250], PAINT2));
        }
        let mut barrel = lathe_x(&[(120, 0), (120, 950)], lod.segs(8), &[METAL], None, None);
        barrel.add(closed_box([850, -160, -160], [1050, 160, 160], METAL));
        let barrel =
            Node::part("barrel", [850, 300, 0], barrel).child(Node::empty("muzzle", [1050, 0, 0]));
        let turret = Node::part("turret", [-100, 2150, 0], turret).child(barrel);
        let body = Node::part("body", [0, 0, 0], body).child(turret);
        vec![anchors(body, [0, 2900, 0], [500, 2200, 600])]
    })
}
