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
    frame, hatch, lathe, lathe_x, lathe_z, loft, ngon, plate, railing, seam_side_z, seam_top,

};
use crate::kit::RECESS;
use crate::palette::{Palette, slot::*};

/// The tone of a track shoe (a little darker than the band it sits on).
const SHOE: crate::kit::Tone = 820;

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
            // A road wheel with a rubber tyre and a capped face, the hub on top of it.
            let wheel = lathe_z(
                &[(wheel_r, 0), (wheel_r, 50)],
                lod.segs(8),
                &[RUBBER],
                Some(METAL),
                None,
            )
            .moved([x, wheel_r + 20, z1]);
            m.add(wheel);
            if lod.has(0) {
                m.add(
                    lathe_z(
                        &[(wheel_r / 3, 0), (wheel_r / 3, 30)],
                        4,
                        &[METAL],
                        Some(METAL),
                        None,
                    )
                    .toned(RECESS)
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
            // Track shoes: the segments of the lower run, seen on the outer face.
            let shoes = (2 * (hx - end)) / 400;
            for i in 0..shoes {
                let x = -(hx - end) + 60 + i * 400;
                // One raised face each (two triangles): the shoe reads, the budget stays.
                let z = z1 + 8;
                let mut shoe = Mesh::new();
                shoe.face(
                    &[[x, 10, z], [x + 280, 10, z], [x + 280, 150, z], [x, 150, z]],
                    RUBBER,
                    [0, 0, 1],
                );
                m.add(shoe.toned(SHOE));
            }
        }
        m
    }
}

/// A team stripe lying on a surface from `lo` to `hi`: a thin box, at LOD2 only its top face (two
/// triangles; a zero-height box loses its walls in [`Mesh::face`]).
fn team_strip(lo: P, hi: P, lod: Lod) -> Mesh {
    if lod.has(1) {
        cuboid(lo, hi, TEAM)
    } else {
        cuboid([lo[0], hi[1], lo[2]], hi, TEAM)
    }
}

/// A team band round a roof edge ([`frame`]); at LOD2 only its top (zero-height walls drop out).
fn team_frame(outer: &[[i32; 2]], inner: &[[i32; 2]], y0: i32, y1: i32, lod: Lod) -> Mesh {
    frame(outer, inner, if lod.has(1) { y0 } else { y1 }, y1, TEAM)
}

/// A bolt head on a face that looks along `-Z`, its base at `at`.
fn bolt_z(at: P, r: i32, h: i32) -> Mesh {
    lathe_z(&[(r, 0), (r, h)], 4, &[METAL], Some(METAL), None)
        .toned(RECESS)
        .mirrored_z()
        .moved(at)
}

/// A vertical panel line on a face that looks along `+X`, at `x`, from `y0` to `y1` at `z`.
fn seam_side_x(x: i32, y0: i32, y1: i32, z: i32, width: i32, raise: i32, mat: Mat) -> Mesh {
    closed_box([x, y0, z - width / 2], [x + raise, y1, z + width / 2], mat).toned(crate::kit::SEAM)
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
        // The engine deck, olive; team: a stripe across its rear edge, a stripe along each
        // fender and the hull side panels (bands and trim, docs/art-style.md section 3).
        if lod.has(1) {
            hull.add(plate(
                [-1800, 1060, -900],
                [-700, 1095, 900],
                lod.bevel(40),
                PAINT2,
            ));
        }
        hull.add(team_strip([-2100, 1060, -900], [-1820, 1095, 900], lod));
        hull.add(both_sides(team_strip(
            [-2200, 700, 1250],
            [1700, 718, 1400],
            lod,
        )));
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
            // Headlights, a shovel and a crowbar on the left fender, an exhaust at the rear.
            hull.add(both_sides(closed_box(
                [2250, 900, 760],
                [2360, 1010, 920],
                ACCENT,
            )));
            hull.add(closed_box([-900, 700, -1380], [300, 740, -1300], METAL));
            hull.add(closed_box([300, 700, -1400], [600, 735, -1280], METAL));
            hull.add(closed_box([-1700, 700, -1180], [-200, 730, -1140], PAINT2));
            hull.add(
                lathe_x(&[(90, 0), (90, 250)], 8, &[METAL], Some(RUBBER), None)
                    .moved([-2550, 850, 500]),
            );
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
        // Team: a band round the roof edge, not the whole roof.
        turret.add(
            team_frame(
                &chamfer_rect(990, 850, c(325)),
                &chamfer_rect(770, 630, c(220)),
                590,
                640,
                lod,
            )
            .moved([80, 0, 0]),
        );
        turret.add(closed_box([1050, 120, -380], [1380, 500, 380], PAINT2));
        turret.add(lathe(&[(30, 0), (30, 900)], 4, &[METAL], None, None).moved([-800, 640, -560]));
        if lod.has(0) {
            turret.add(hatch([-250, 600, 330], 260, 8, PAINT2));
            // Smoke dischargers, three a side, angled forward.
            for i in 0..3 {
                turret.add(both_sides(
                    lathe_x(&[(55, 0), (55, 220)], 4, &[METAL], Some(RUBBER), None)
                        .moved([600, 380, 1050 - i * 140]),
                ));
            }
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
            lod.segs(8),
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
        // The engine deck in gunmetal; team: a stripe across its rear edge, a stripe along each
        // side of the hull top and panels on the skirts.
        if lod.has(1) {
            hull.add(plate(
                [-1680, 1000, -880],
                [-650, 1035, 880],
                lod.bevel(40),
                PAINT2,
            ));
        }
        hull.add(team_strip([-2150, 1000, -880], [-1700, 1035, 880], lod));
        hull.add(both_sides(team_strip(
            [-50, 1000, 580],
            [1450, 1018, 880],
            lod,
        )));
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
            // A tool box and a tow cable on the deck, exhausts at the rear, front lamps.
            hull.add(closed_box([-1500, 1035, -560], [-900, 1180, -260], PAINT2));
            hull.add(closed_box([-1500, 1035, 300], [-800, 1075, 380], METAL));
            for z in [-400, 400] {
                hull.add(closed_box([-2300, 780, z - 90], [-2150, 900, z + 90], METAL));
            }
            hull.add(both_sides(closed_box(
                [2050, 860, 600],
                [2200, 940, 760],
                METAL,
            )));
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
        // The roof plate, and team as a band round its edge.
        turret.add(loft(
            &[
                (wedge(880, 430, 470, 680), 470),
                (wedge(880, 430, 470, 680), 490),
            ],
            &[PAINT],
            Some(PAINT),
            None,
        ));
        turret.add(team_frame(
            &wedge(880, 430, 470, 680),
            &wedge(640, 300, 330, 440),
            490,
            505,
            lod,
        ));
        turret.add(lathe(&[(25, 0), (25, 800)], 4, &[METAL], None, None).moved([-850, 505, -500]));
        if lod.has(0) {
            turret.add(both_sides(closed_box(
                [-900, 180, 900],
                [-300, 220, 990],
                ACCENT,
            )));
            turret.add(cuboid([-650, 505, 150], [-350, 680, 450], PAINT2));
            turret.add(hatch([-150, 490, -180], 190, 8, PAINT2));
            for i in 0..2 {
                turret.add(both_sides(
                    lathe_x(&[(55, 0), (55, 220)], 4, &[METAL], Some(RUBBER), None)
                        .moved([150, 300, 860 - i * 130]),
                ));
            }
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
                Some(PAINT),
                None,
            )
            .moved([2280, 0, 0]),
        );
        // Team: a band round the cab roof edge.
        hull.add(
            team_frame(
                &chamfer_rect(560, 1300, 255 * b),
                &chamfer_rect(380, 1100, 170 * b),
                2240,
                2280,
                lod,
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
                    (r, 1350),
                    (r, 2250),
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
                &[(r, 0), (r, 700), (r, 1350), (r, 2250), (r, 2900), (r, 3600)],
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
            // A ladder up the drum side, grab rails on the cab, a beacon, drum straps' bolts.
            for k in 0..5 {
                hull.add(closed_box(
                    [1450 - k * 10, 950 + k * 260, 1560],
                    [1650 - k * 10, 990 + k * 260, 1600],
                    METAL,
                ));
            }
            hull.add(both_sides(closed_box(
                [1700, 1000, 1500],
                [1740, 2200, 1540],
                METAL,
            )));
            hull.add(railing([1850, 2280, -1000], [2650, 2280, -1000], 220, 3, METAL));
            hull.add(
                lathe(&[(110, 0), (110, 160), (60, 220)], 8, &[ACCENT], None, None)
                    .moved([1950, 2280, 700]),
            );
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

/// Veyran power plant: a stepped hall with sloped roof facets, two octagonal stacks with a
/// crimson glow band and a walkway, pipes, vents, windows and roof units. Team: a band round the
/// flat roof's edge and a flange on each stack.
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
                &[PAINT, PAINT],
                Some(PAINT2),
                None,
            )
            .moved([0, 0, -1500]),
        );
        // Team: the roof edge.
        body.add(
            team_frame(
                &chamfer_rect(2400, 3000, lod.bevel(300)),
                &chamfer_rect(1550, 2150, lod.bevel(200)),
                3700,
                3750,
                lod,
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
            let mut m = loft(&rings, &bands, Some(METAL), None);
            // Team: a flange round the stack's mouth.
            m.add(team_frame(
                &ngon(r * 9 / 10 + 60, segs, true),
                &ngon(r * 9 / 10 - 260, segs, true),
                6500,
                6560,
                lod,
            ));
            m
        };
        for x in [-1700, 1700] {
            body.add(stack(900).moved([x, 0, 4100]));
        }
        if lod.has(1) {
            // Pipes from the hall to the stacks, the roof units.
            for x in [-1700, 1700] {
                body.add(
                    lathe(&[(220, 0), (220, 1600)], lod.segs(8), &[METAL], None, None)
                        .moved([x, 1600, 2000]),
                );
                body.add(closed_box(
                    [x - 250, 1400, 2300],
                    [x + 250, 1900, 3300],
                    METAL,
                ));
            }
            for x in [-900, 900] {
                body.add(cuboid([x - 550, 3750, -3300], [x + 550, 4250, -2200], PAINT2));
            }
        }
        if lod.has(0) {
            // Vents along both long sides, louvred.
            for i in 0..8 {
                let z = -5000 + i * 800;
                for x in [3200, -3200] {
                    let dx = if x > 0 { 20 } else { -20 };
                    body.add(closed_box(
                        [x.min(x + dx), 900, z],
                        [x.max(x + dx), 2000, z + 450],
                        PAINT2,
                    ));
                    for k in 0..2 {
                        let y = 1150 + k * 450;
                        let d = if x > 0 { 70 } else { -70 };
                        body.add(
                            cuboid(
                                [x.min(x + d), y, z + 30],
                                [x.max(x + d), y + 60, z + 420],
                                METAL,
                            )
                            .toned(RECESS),
                        );
                    }
                }
            }
            // A pipe run along each long wall with flanges, and two downpipes.
            for x in [3330, -3330] {
                body.add(
                    lathe_z(&[(110, 0), (110, 7200)], 8, &[METAL], None, None)
                        .moved([x, 2450, -5100]),
                );
                for k in 0..5 {
                    body.add(
                        lathe_z(&[(160, 0), (160, 80)], 8, &[METAL], Some(METAL), None)
                            .moved([x, 2450, -4800 + k * 1600]),
                    );
                }
                body.add(lathe(&[(90, 0), (90, 2050)], 8, &[METAL], None, None).moved([x, 400, 1900]));
            }
            // The front: a door with a frame and a crimson lintel, a window row, bolted panels.
            body.add(closed_box([-1000, 400, -5330], [1000, 2400, -5290], METAL));
            for x in [-1150, 1000] {
                body.add(closed_box([x, 400, -5360], [x + 150, 2400, -5280], PAINT2));
            }
            body.add(closed_box(
                [-1100, 2400, -5360],
                [1100, 2550, -5280],
                ACCENT,
            ));
            for i in 0..6 {
                let x = -2900 + i * 1000 + if i >= 3 { 300 } else { -300 };
                if (-1300..=1300).contains(&x) {
                    continue;
                }
                body.add(closed_box([x - 60, 1900, -5345], [x + 460, 2450, -5290], PAINT2));
                body.add(closed_box([x, 1960, -5365], [x + 400, 2390, -5340], RUBBER).toned(RECESS));
            }
            for x in [-2700, 1500] {
                body.add(plate([x, 600, -5340], [x + 1200, 1600, -5300], 15, PAINT2));
                for (dx, dy) in [(80, 80), (1120, 80), (80, 920), (1120, 920)] {
                    body.add(
                        lathe_z(&[(40, 0), (40, 25)], 4, &[METAL], Some(METAL), None)
                            .mirrored_z()
                            .moved([x + dx, 600 + dy, -5340]),
                    );
                }
            }
            // The roof: fans on the roof units, a grille row, panel lines, a railing round the
            // inner roof and bolts along the platform edge.
            for x in [-900, 900] {
                body.add(
                    lathe(&[(420, 0), (420, 120)], 16, &[METAL], None, None)
                        .moved([x, 4250, -2750]),
                );
                body.add(hatch([x, 4370, -2750], 380, 16, PAINT2));
            }
            for i in 0..4 {
                let x = -1350 + i * 700;
                body.add(closed_box([x, 3700, -900], [x + 500, 3900, -300], PAINT2));
                body.add(seam_top([x + 40, 3900, -600], [x + 460, 3900, -600], 40, 12, METAL));
            }
            for i in 0..5 {
                let x = -1200 + i * 600;
                body.add(seam_top([x, 3700, -3700], [x, 3700, -3400], 50, 12, PAINT));
                body.add(seam_top([x, 3700, 200], [x, 3700, 700], 50, 12, PAINT));
            }
            let (rx, rz0, rz1) = (1500, -3600, 600);
            body.add(railing([-rx, 3700, rz0], [rx, 3700, rz0], 450, 6, METAL));
            body.add(railing([-rx, 3700, rz1], [rx, 3700, rz1], 450, 6, METAL));
            body.add(railing([-rx, 3700, rz0], [-rx, 3700, rz1], 450, 7, METAL));
            body.add(railing([rx, 3700, rz0], [rx, 3700, rz1], 450, 7, METAL));
            for i in 0..10 {
                body.add(bolt([-3500 + i * 780, 400, -5450], 50, 30, 4, METAL));
                body.add(bolt([-3500 + i * 780, 400, 5450], 50, 30, 4, METAL));
            }
            // The stacks: a walkway ring with a ladder up to it, rivet bands.
            for x in [-1700, 1700] {
                body.add(
                    frame(&ngon(1150, 8, true), &ngon(740, 8, true), 3900, 3970, PAINT2)
                        .moved([x, 0, 4100]),
                );
                let lz = 4100 + 820;
                for side in [-200, 200] {
                    body.add(closed_box([x + side - 25, 400, lz], [x + side + 25, 3970, lz + 50], METAL));
                }
                for k in 0..10 {
                    let y = 700 + k * 320;
                    body.add(cuboid([x - 200, y, lz], [x + 200, y + 35, lz + 40], METAL));
                }
                for k in 0..8 {
                    body.add(
                        bolt([0, 0, 0], 35, 20, 4, METAL)
                            .moved([x + 680 * (k % 2 * 2 - 1), 5580, 4100 + (k / 2 - 2) * 220 + 110]),
                    );
                }
            }
            // A transformer at the rear with insulators.
            for x in [-3000, 2300] {
                body.add(bevel_box([x, 400, 5000], [x + 700, 1300, 5500], 40, PAINT2, PAINT2));
                for k in 0..3 {
                    body.add(
                        lathe(&[(60, 0), (60, 300), (110, 320), (110, 360)], 8, &[ACCENT, METAL], Some(METAL), None)
                            .moved([x + 130 + k * 220, 1300, 5250]),
                    );
                }
            }
        }
        let body = Node::part("body", [0, 0, 0], body);
        vec![anchors(body, [-1700, 6560, 4100], [0, 3800, -1500])]
    })
}
/// Meridian construction center: a broad platform with railings, a rounded main hall with a dome,
/// two round tanks joined by pipes and a catwalk, a crane, a landing pad. Team: a band round the
/// hall roof edge, the tank shoulders and the pad marking.
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
                    Some(PAINT),
                    None,
                )
                .moved([-1600, 0, -1500]),
            );
            // Team: the hall roof edge.
            body.add(
                team_frame(
                    &chamfer_rect(4800, 4400, lod.bevel(900)),
                    &chamfer_rect(3500, 3100, lod.bevel(600)),
                    3500,
                    3550,
                    lod,
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
            // Two round tanks, team on the shoulder.
            for (x, z) in [(4600, 3800), (4600, 800)] {
                body.add(
                    lathe(
                        &[(1200, 0), (1200, 3000), (1000, 3400), (650, 3550)],
                        lod.segs(16),
                        &[PAINT2, TEAM, PAINT2],
                        Some(PAINT2),
                        None,
                    )
                    .moved([x, 500, z]),
                );
            }
            // Landing pad with a team marking.
            body.add(bevel_box(
                [2200, 500, -6800],
                [6800, 650, -2200],
                lod.bevel(80),
                METAL,
                METAL,
            ));
            body.add(
                team_frame(
                    &chamfer_rect(1500, 1500, lod.bevel(400)),
                    &chamfer_rect(1000, 1000, lod.bevel(250)),
                    650,
                    680,
                    lod,
                )
                .moved([4500, 0, -4500]),
            );
            // Crane: a mast and an arm (in every level: it sets the height).
            body.add(closed_box([-6600, 500, 4600], [-6000, 6500, 5200], PAINT));
            body.add(closed_box([-6800, 6500, 3000], [-1000, 7000, 5400], PAINT));
            if lod.has(1) {
                body.add(closed_box([-1300, 4000, 4100], [-1150, 6500, 4250], METAL));
                // Pipes between the tanks and the hall.
                for z in [3800, 800] {
                    body.add(
                        lathe_x(&[(200, 0), (200, 1200)], lod.segs(8), &[METAL], None, None)
                            .moved([3400, 1600, z]),
                    );
                }
            }
            if lod.has(0) {
                // Doors with frames, windows, vents, rails, bolts.
                body.add(closed_box([3380, 500, -3600], [3440, 2600, 600], METAL));
                for z in [-3750, 600] {
                    body.add(closed_box([3370, 500, z], [3460, 2700, z + 150], PAINT2));
                }
                body.add(closed_box([3370, 2600, -3750], [3460, 2750, 750], PAINT2));
                for i in 0..6 {
                    body.add(seam_side_x(3440, 600, 2550, -3400 + i * 650, 40, 10, METAL));
                }
                for i in 0..7 {
                    let x = -6000 + i * 1300;
                    body.add(closed_box([x - 60, 1940, -6140], [x + 860, 2760, -6100], PAINT2));
                    body.add(closed_box([x, 2000, -6160], [x + 800, 2700, -6130], ACCENT));
                    body.add(seam_side_z(x - 250, 600, 3150, -6100, 60, 15, PAINT));
                    // A bolted panel under each window.
                    body.add(plate([x, 900, -6130], [x + 800, 1700, -6100], 15, PAINT));
                    body.add(bolt_z([x + 400, 1640, -6130], 35, 20));
                }
                for i in 0..8 {
                    let z = -5800 + i * 1200;
                    body.add(closed_box([-6630, 1000, z], [-6580, 2400, z + 600], PAINT2));
                    for k in 0..2 {
                        body.add(
                            cuboid([-6680, 1150 + k * 450, z + 40], [-6630, 1210 + k * 450, z + 560], METAL)
                                .toned(RECESS),
                        );
                    }
                }
                for i in 0..9 {
                    let t = -6800 + i * 1600;
                    body.add(bolt([t, 500, -7250], 60, 35, 4, METAL));
                    body.add(bolt([t, 500, 7250], 60, 35, 4, METAL));
                }
                // Railings along the open platform edges.
                body.add(railing([-7200, 500, -7200], [1800, 500, -7200], 700, 7, METAL));
                body.add(railing([7200, 500, -1700], [7200, 500, 7200], 700, 7, METAL));
                body.add(railing([-3000, 500, 7200], [3000, 500, 7200], 700, 5, METAL));
                // Roof units in the corners of the hall roof, between the band and the dome.
                for (x, z) in [(-4500, -4100), (1300, -4100), (-4500, 1100), (1300, 1100)] {
                    body.add(bevel_box([x - 500, 3500, z - 500], [x + 500, 4000, z + 500], 60, PAINT2, PAINT2));
                    body.add(
                        lathe(&[(320, 0), (320, 60)], 8, &[METAL], Some(METAL), None)
                            .toned(RECESS)
                            .moved([x, 4000, z]),
                    );
                }
                for i in 0..5 {
                    let z = 3200 + i * 400;
                    body.add(seam_top([-6600, 7000, z], [-1000, 7000, z], 60, 20, METAL));
                }
                // Crane: cross braces on the mast, a cable and a hook block.
                for k in 0..7 {
                    let y = 1100 + k * 800;
                    body.add(closed_box([-6650, y, 4550], [-5950, y + 80, 5250], METAL));
                }
                body.add(closed_box([-1240, 3600, 4150], [-1210, 4000, 4200], METAL));
                body.add(closed_box([-1400, 3300, 4000], [-1050, 3600, 4350], ACCENT));
                // Tanks: a rivet ring, a ladder each and a catwalk between them.
                for (x, z) in [(4600, 3800), (4600, 800)] {
                    body.add(
                        lathe(&[(1230, 0), (1230, 120)], 16, &[METAL], None, None)
                            .moved([x, 2000, z]),
                    );
                    let lx = x + 1210;
                    for side in [-180, 180] {
                        body.add(closed_box([lx, 500, z + side - 25], [lx + 50, 3500, z + side + 25], METAL));
                    }
                    for k in 0..6 {
                        let y = 900 + k * 420;
                        body.add(cuboid([lx, y, z - 180], [lx + 40, y + 35, z + 180], METAL));
                    }
                }
                body.add(closed_box([4300, 3500, 1700], [4900, 3560, 2900], METAL));
                body.add(railing([4300, 3560, 1700], [4300, 3560, 2900], 500, 3, METAL));
                body.add(railing([4900, 3560, 1700], [4900, 3560, 2900], 500, 3, METAL));
                body.add(
                    lathe(&[(80, 0), (80, 700)], 8, &[METAL], None, None)
                        .moved([-1600, 6150, -1500]),
                );
                // Pad lamps.
                for (x, z) in [(2400, -6600), (6600, -6600), (2400, -2400), (6600, -2400)] {
                    body.add(lathe(&[(90, 0), (90, 80), (50, 140)], 4, &[ACCENT], None, None).moved([x, 650, z]));
                }
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
            // Armour blocks along the foot of every side, bolt rows round the band, a railing
            // round the roof, a cable conduit and two whip antennas.
            for q in 0..4u8 {
                for i in 0..4 {
                    let z = -1000 + i * 560;
                    body.add(
                        extrude_z1(
                            &[[1700, 120], [1890, 120], [1890, 360], [1760, 480]],
                            z,
                            z + 420,
                            PAINT2,
                        )
                        .turned_y(q),
                    );
                }
                for i in 0..14 {
                    body.add(
                        lathe_x(&[(30, 0), (30, 20)], 4, &[METAL], Some(METAL), None)
                            .toned(RECESS)
                            .moved([1550, 1800, -1300 + i * 200])
                            .turned_y(q),
                    );
                }
                for i in 0..6 {
                    body.add(
                        closed_box([1360, 500 + i * 180, -900], [1700, 540 + i * 180, -860], PAINT2)
                            .toned(crate::kit::SEAM)
                            .turned_y(q),
                    );
                }
            }
            let e = 1150;
            body.add(railing([-e, 2150, -e], [e, 2150, -e], 450, 6, METAL));
            body.add(railing([-e, 2150, e], [e, 2150, e], 450, 6, METAL));
            body.add(railing([-e, 2150, -e], [-e, 2150, e], 450, 6, METAL));
            body.add(railing([e, 2150, -e], [e, 2150, e], 450, 6, METAL));
            body.add(
                lathe(&[(70, 0), (70, 1900)], 8, &[METAL], None, None).moved([-1700, 0, -1100]),
            );
            for z in [-1050, 1050] {
                body.add(lathe(&[(20, 0), (20, 1400)], 4, &[METAL], None, None).moved([-1050, 2150, z]));
            }
            // A ladder up the back.
            for side in [980, 1420] {
                body.add(closed_box([-1950, 0, side - 25], [-1900, 2150, side + 25], METAL));
            }
            for k in 0..7 {
                let y = 250 + k * 280;
                body.add(cuboid([-1950, y, 980], [-1910, y + 35, 1420], METAL));
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
        // Team: a band round the gun head's roof.
        turret.add(team_frame(
            &wedge(780, 290, 390, 660),
            &wedge(560, 160, 280, 440),
            650,
            690,
            lod,
        ));
        turret.add(lathe(&[(25, 0), (25, 700)], 4, &[METAL], None, None).moved([-800, 690, 420]));
        if lod.has(0) {
            turret.add(both_sides(closed_box(
                [-700, 250, 880],
                [200, 330, 930],
                ACCENT,
            )));
            turret.add(cuboid([-550, 650, -250], [-250, 900, 250], PAINT2));
            // A searchlight, vision blocks, bolts along the cheeks, a roof hatch.
            turret.add(
                lathe_x(&[(130, 0), (150, 220)], 8, &[PAINT2], Some(ACCENT), None)
                    .moved([-450, 900, 0]),
            );
            for z in [-160, 0, 160] {
                turret.add(closed_box([520, 560, z - 50], [600, 620, z + 50], RUBBER).toned(RECESS));
            }
            for i in 0..6 {
                turret.add(both_sides(
                    lathe_z(&[(30, 0), (30, 20)], 4, &[METAL], Some(METAL), None)
                        .toned(RECESS)
                        .moved([-800 + i * 220, 400, 860]),
                ));
            }
            turret.add(hatch([-100, 650, -200], 170, 8, PAINT2));
            turret.add(both_sides(closed_box(
                [-850, 60, 900],
                [450, 160, 960],
                PAINT2,
            )));
        }
        let mut barrel = lathe_x(&[(120, 0), (120, 950)], lod.segs(8), &[METAL], None, None);
        barrel.add(closed_box([850, -160, -160], [1050, 160, 160], METAL));
        if lod.has(0) {
            // A sleeve at the root and the slots of the muzzle brake.
            barrel.add(lathe_x(&[(165, 0), (165, 260)], 8, &[PAINT2], None, Some(PAINT2)));
            for z in [-170, 170] {
                barrel.add(closed_box([890, -80, z - 15], [1010, 80, z + 15], RUBBER).toned(RECESS));
            }
        }
        let barrel =
            Node::part("barrel", [850, 300, 0], barrel).child(Node::empty("muzzle", [1050, 0, 0]));
        let turret = Node::part("turret", [-100, 2150, 0], turret).child(barrel);
        let body = Node::part("body", [0, 0, 0], body).child(turret);
        vec![anchors(body, [0, 2900, 0], [500, 2200, 600])]
    })
}
