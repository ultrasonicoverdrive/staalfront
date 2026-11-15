//! The wheeled units of set v5 (WP11.6b, docs/art-style.md): the repair truck of both factions,
//! the Veyran transport truck, the neutral supply truck, the Meridian light transport and the
//! Meridian amphibious transport.

use crate::kit::{
    Lod, Mat, Mesh, Model, Node, P, bevel_box, bolt, chamfer_rect, closed_box, cuboid, hatch,
    lathe, lathe_x, lathe_z, loft, seam_side_z, seam_top,
};
use crate::models::{both_sides, team_frame, team_strip};
use crate::palette::{Palette, slot::*};
use crate::units::{Style, ring};

/// Road wheels on both sides at `xs`: tyres with a rubber tread and a metal rim face, the outer
/// face at `z_out`, `w` wide, radius `r`, sitting on the ground. LOD0 has a tread profile and
/// hubs, LOD1 plain wheels with eight segments, LOD2 one block a side.
pub(crate) fn wheels(lod: Lod, xs: &[i32], r: i32, w: i32, z_out: i32) -> Mesh {
    let mut right = Mesh::new();
    if !lod.has(1) {
        let (a, b) = (xs[0], xs[xs.len() - 1]);
        right.add(cuboid(
            [a - r * 7 / 10, 0, z_out - w],
            [b + r * 7 / 10, 2 * r, z_out],
            RUBBER,
        ));
    } else {
        for &x in xs {
            let profile = if lod.has(0) {
                vec![(r, 0), (r, w - 70), (r - 80, w)]
            } else {
                vec![(r, 0), (r, w)]
            };
            right.add(
                lathe_z(&profile, lod.segs(16), &[RUBBER], Some(METAL), None).moved([
                    x,
                    r,
                    z_out - w,
                ]),
            );
            if lod.has(0) {
                right.add(
                    lathe_z(
                        &[(r * 40 / 100, 0), (r * 40 / 100, 70)],
                        8,
                        &[METAL],
                        Some(METAL),
                        None,
                    )
                    .moved([x, r, z_out]),
                );
            }
        }
    }
    both_sides(right)
}

/// A bevelled box at full detail, a plain box (10 triangles instead of about 40) in the far levels.
pub(crate) fn soft(lod: Lod, lo: P, hi: P, b: i32, side: Mat, top: Mat) -> Mesh {
    if lod.has(0) {
        bevel_box(lo, hi, b, side, top)
    } else {
        cuboid(lo, hi, side)
    }
}

/// Mudguards over the wheels, in the second paint (not at LOD2).
pub(crate) fn arches(lod: Lod, xs: &[i32], r: i32, w: i32, z_out: i32) -> Mesh {
    let mut m = Mesh::new();
    if lod.has(1) {
        for &x in xs {
            m.add(both_sides(soft(
                lod,
                [x - r - 40, 2 * r + 10, z_out - w - 40],
                [x + r + 40, 2 * r + 60, z_out + 20],
                lod.bevel(30),
                PAINT2,
                PAINT2,
            )));
        }
    }
    m
}

/// Headlights on both sides of a front at `x`, `y`, `z`: glass on Meridian, a small crimson lamp
/// on Veyran (the `accent` slot).
pub(crate) fn lamps(x: i32, y: i32, z: i32) -> Mesh {
    both_sides(closed_box(
        [x, y, z - 90],
        [x + 110, y + 110, z + 90],
        ACCENT,
    ))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Repair,
    Transport,
    Supply,
}

struct Truck {
    stem: &'static str,
    faction: &'static str,
    style: Style,
    kind: Kind,
    /// The ends of the chassis, `x` back and front.
    back: i32,
    front: i32,
    /// The outer face of the wheels, their radius and width, and the axles.
    z_out: i32,
    r: i32,
    w: i32,
    axles: &'static [i32],
    /// The cab: its centre `x`, half length, half width and roof height.
    cab_x: i32,
    cab_hl: i32,
    cab_hz: i32,
    roof: i32,
}

fn truck(t: &'static Truck) -> Model {
    let pal = Palette::shipped();
    let neutral = t.faction == "neutral";
    Model::new(t.stem, pal.materials(t.faction), |lod| {
        let c = |v: i32| lod.bevel(v);
        let hz = t.z_out - t.w - 40;
        let mut hull = wheels(lod, t.axles, t.r, t.w, t.z_out);
        hull.add(arches(lod, t.axles, t.r, t.w, t.z_out));
        // The chassis: a deep frame between the wheels, in the second paint.
        hull.add(soft(
            lod,
            [t.back, 600, -hz],
            [t.front, 1000, hz],
            c(40),
            PAINT2,
            PAINT2,
        ));
        // The cab: a tapering block with side windows, a roof band in team colour.
        let deck = 1000;
        let cab_ring = |hl_back: i32, hl_front: i32, half_z: i32, cc: i32| {
            ring(t.style, hl_back, hl_front, half_z, cc)
        };
        let mut cab = loft(
            &[
                (cab_ring(t.cab_hl, t.cab_hl, t.cab_hz, c(260)), deck),
                (cab_ring(t.cab_hl, t.cab_hl, t.cab_hz, c(260)), deck + 650),
                (
                    cab_ring(t.cab_hl - 40, t.cab_hl - 220, t.cab_hz - 90, c(330)),
                    t.roof,
                ),
            ],
            &[PAINT, PAINT],
            Some(PAINT),
            None,
        );
        let band_out = cab_ring(t.cab_hl - 50, t.cab_hl - 230, t.cab_hz - 100, c(325));
        let band_in = cab_ring(t.cab_hl - 300, t.cab_hl - 480, t.cab_hz - 330, c(210));
        if neutral {
            cab.add(team_frame_neutral(&band_out, &band_in, t.roof, lod));
        } else {
            cab.add(team_frame(
                &band_out,
                &band_in,
                t.roof - 10,
                t.roof + 40,
                lod,
                PAINT,
            ));
        }
        hull.add(cab.moved([t.cab_x, 0, 0]));
        if lod.has(1) {
            // Side windows, a windscreen band and the lamps.
            hull.add(both_sides(closed_box(
                [t.cab_x - t.cab_hl / 2, deck + 700, t.cab_hz],
                [t.cab_x + t.cab_hl / 3, t.roof - 150, t.cab_hz + 25],
                ACCENT,
            )));
        }
        if lod.has(0) {
            hull.add(lamps(t.front - 120, deck + 150, hz / 2 + 100));
            // Exhaust stack behind the cab, mirror stalk, a step under the door.
            hull.add(
                lathe(&[(70, 0), (70, 1300)], 8, &[METAL], Some(RUBBER), None).moved([
                    t.cab_x - t.cab_hl - 150,
                    deck,
                    hz - 100,
                ]),
            );
        }
        // The body behind the cab.
        match t.kind {
            Kind::Repair => repair_body(t, lod, &mut hull),
            Kind::Transport => transport_body(t, lod, &mut hull),
            Kind::Supply => supply_body(t, lod, &mut hull),
        }
        let turret = Node::empty("turret", [0, deck, 0]).child(Node::empty("barrel", [0, 0, 0]));
        vec![Node::part("hull", [0, 0, 0], hull).child(turret)]
    })
}

/// A neutral roof band: the same shape as [`team_frame`], in the third paint slot.
fn team_frame_neutral(outer: &[[i32; 2]], inner: &[[i32; 2]], roof: i32, lod: Lod) -> Mesh {
    let mut m = crate::kit::frame(
        outer,
        inner,
        if lod.has(1) { roof - 10 } else { roof },
        roof + 40,
        PAINT3,
    );
    let top: Vec<[i32; 3]> = inner.iter().map(|p| [p[0], roof + 40, p[1]]).collect();
    m.face(&top, PAINT, [0, 1, 0]);
    m
}

fn repair_body(t: &Truck, lod: Lod, hull: &mut Mesh) {
    let deck = 1000;
    let hz = t.z_out - t.w - 40;
    let bx0 = t.back + 30;
    let bx1 = t.cab_x - t.cab_hl - 220;
    // The bed plate, tool lockers on both sides with shutter lines, a crane base in the middle.
    hull.add(soft(
        lod,
        [bx0, deck, -hz - 100],
        [bx1, deck + 60, hz + 100],
        lod.bevel(30),
        PAINT2,
        PAINT2,
    ));
    hull.add(both_sides(soft(
        lod,
        [bx0 + 20, deck + 60, hz - 360],
        [bx1 - 40, deck + 740, hz + 90],
        lod.bevel(50),
        PAINT,
        PAINT,
    )));
    let mid = (bx0 + bx1) / 2;
    hull.add(soft(
        lod,
        [mid - 600, deck + 60, -hz + 360],
        [mid + 500, deck + 520, hz - 360],
        lod.bevel(40),
        PAINT2,
        PAINT2,
    ));
    // Team: stripes along the locker tops and across the crane base.
    hull.add(both_sides(team_strip(
        [bx0 + 40, deck + 740, hz - 160],
        [bx1 - 60, deck + 758, hz + 40],
        lod,
    )));
    // The crane: a pedestal, a boom lying forward over the cab, a hook.
    let boom_y = t.roof + 150;
    if lod.has(1) {
        hull.add(
            lathe(
                &[(330, 0), (330, 300), (240, 360)],
                lod.segs(8),
                &[PAINT2],
                Some(METAL),
                None,
            )
            .moved([mid, deck + 520, 0]),
        );
        hull.add(closed_box(
            [mid - 150, deck + 880, -170],
            [mid + 150, boom_y, 170],
            PAINT2,
        ));
    }
    hull.add(closed_box(
        [mid - 150, boom_y, -150],
        [t.front - 300, boom_y + 200, 150],
        PAINT,
    ));
    hull.add(team_strip(
        [mid + 100, boom_y + 200, -80],
        [t.front - 400, boom_y + 218, 80],
        lod,
    ));
    if lod.has(0) {
        hull.add(
            lathe_x(&[(70, 0), (70, 1500)], 8, &[METAL], Some(METAL), None).moved([
                mid + 100,
                boom_y - 150,
                -230,
            ]),
        );
        hull.add(
            lathe(&[(40, 0), (40, 650)], 4, &[METAL], None, None).moved([
                t.front - 350,
                boom_y - 650,
                0,
            ]),
        );
        hull.add(closed_box(
            [t.front - 420, boom_y - 800, -70],
            [t.front - 280, boom_y - 650, 70],
            METAL,
        ));
        // Locker doors: shutter lines and handles; bolts on the bed plate; a rear light bar.
        for i in 0..4 {
            let x = bx0 + 300 + i * ((bx1 - bx0 - 500) / 4);
            hull.add(both_sides(seam_side_z(
                x,
                deck + 120,
                deck + 700,
                hz + 90,
                40,
                10,
                PAINT2,
            )));
        }
        hull.add(closed_box(
            [t.back, deck + 200, -600],
            [t.back + 40, deck + 320, 600],
            ACCENT,
        ));
    }
}

fn transport_body(t: &Truck, lod: Lod, hull: &mut Mesh) {
    let deck = 1000;
    let hz = t.z_out - t.w - 20;
    let bx0 = t.back + 30;
    let bx1 = t.cab_x - t.cab_hl - 180;
    let c = |v: i32| lod.bevel(v);
    // The cargo box: sloped sides under a sloped roof (Veyran: plates), a rear door frame.
    let mid = (bx0 + bx1) / 2;
    let hl = (bx1 - bx0) / 2;
    let top = deck + 1500;
    let mut body = loft(
        &[
            (chamfer_rect(hl, hz, c(80)), deck + 60),
            (chamfer_rect(hl, hz, c(80)), top - 360),
            (chamfer_rect(hl - 80, hz - 260, c(300)), top),
        ],
        &[PAINT, PAINT],
        Some(PAINT),
        None,
    )
    .moved([mid, 0, 0]);
    // Team: three narrow bands across the roof and one on each shoulder.
    for i in 0..3 {
        let x = mid - hl + 500 + i * ((2 * hl - 1000) / 2);
        body.add(team_strip(
            [x - 150, top, -(hz - 300)],
            [x + 150, top + 20, hz - 300],
            lod,
        ));
    }
    body.add(both_sides(team_strip(
        [mid - hl + 150, top, hz - 560],
        [mid + hl - 150, top + 20, hz - 300],
        lod,
    )));
    // Roof ribs, vents, tie-downs; a crimson door slit on the faction mark of Veyran.
    if lod.has(0) {
        for i in 0..6 {
            let x = mid - hl + 300 + i * ((2 * hl - 600) / 5);
            body.add(both_sides(seam_side_z(
                x,
                deck + 200,
                top - 400,
                hz,
                40,
                12,
                PAINT2,
            )));
            body.add(seam_top(
                [x, top, -hz + 330],
                [x, top, hz - 330],
                50,
                14,
                PAINT2,
            ));
        }
        body.add(hatch([mid, top, 0], 240, 8, PAINT2));
        body.add(closed_box(
            [mid - hl, deck + 400, -400],
            [mid - hl + 40, deck + 1200, 400],
            if t.style == Style::Angular {
                ACCENT
            } else {
                PAINT2
            },
        ));
        for i in 0..4 {
            body.add(bolt([mid - hl + 300 + i * 500, top, 0], 40, 22, 4, METAL));
        }
    }
    hull.add(body);
}

fn supply_body(t: &Truck, lod: Lod, hull: &mut Mesh) {
    let deck = 1000;
    let hz = t.z_out - t.w - 20;
    let bx0 = t.back + 30;
    let bx1 = t.cab_x - t.cab_hl - 180;
    // A flat bed with low sides, crates and barrels, a lashed tarp; no player colour: the second
    // light paint stands in for the stripes.
    hull.add(soft(
        lod,
        [bx0, deck, -hz],
        [bx1, deck + 80, hz],
        lod.bevel(30),
        PAINT2,
        PAINT2,
    ));
    hull.add(both_sides(soft(
        lod,
        [bx0, deck + 80, hz - 60],
        [bx1, deck + 450, hz],
        lod.bevel(20),
        PAINT,
        PAINT,
    )));
    hull.add(soft(
        lod,
        [bx0, deck + 80, -hz],
        [bx0 + 60, deck + 450, hz],
        lod.bevel(20),
        PAINT,
        PAINT,
    ));
    let len = bx1 - bx0 - 200;
    let n = 3;
    // Far away the load is one block with a stripe across it.
    let near = if lod.has(1) { n } else { 0 };
    if !lod.has(1) {
        hull.add(cuboid(
            [bx0 + 140, deck + 80, -hz + 120],
            [bx1 - 100, deck + 800, hz - 120],
            PAINT2,
        ));
        hull.add(cuboid(
            [bx0 + 300, deck + 800, -hz + 200],
            [bx0 + 600, deck + 818, hz - 200],
            PAINT3,
        ));
    }
    for i in 0..near {
        let x0 = bx0 + 140 + i * (len / n);
        let x1 = x0 + len / n - 90;
        hull.add(cuboid(
            [x0, deck + 80, -hz + 120],
            [x1, deck + 900, -40],
            PAINT2,
        ));
        hull.add(cuboid(
            [x0, deck + 80, 40],
            [x1, deck + 700 + (i % 2) * 250, hz - 120],
            PAINT,
        ));
        // A stripe across each stack of crates in the third paint (not a team colour).
        hull.add(cuboid(
            [x0 + 60, deck + 900, -hz + 200],
            [x0 + 360, deck + 918, -120],
            PAINT3,
        ));
        if lod.has(0) {
            hull.add(seam_top(
                [x0 + (x1 - x0) / 2, deck + 700 + (i % 2) * 250, 40],
                [x0 + (x1 - x0) / 2, deck + 700 + (i % 2) * 250, hz - 120],
                40,
                14,
                PAINT2,
            ));
        }
    }
    if lod.has(1) {
        for i in 0..3 {
            hull.add(
                lathe(
                    &[(260, 0), (260, 700), (230, 740)],
                    lod.segs(8),
                    &[PAINT2, PAINT2],
                    Some(METAL),
                    None,
                )
                .moved([bx0 + 400 + i * 560, deck + 80, 0]),
            );
        }
        hull.add(cuboid(
            [bx1 - 700, deck + 80, -hz + 100],
            [bx1 - 60, deck + 600, hz - 100],
            PAINT3,
        ));
    }
    if lod.has(0) {
        for i in 0..5 {
            hull.add(bolt(
                [bx0 + 200 + i * 450, deck + 80, hz - 30],
                35,
                20,
                4,
                METAL,
            ));
        }
        hull.add(both_sides(closed_box(
            [bx0 + 300, deck + 450, hz - 80],
            [bx0 + 340, deck + 700, hz - 40],
            METAL,
        )));
    }
}

const REPAIR: Truck = Truck {
    stem: "",
    faction: "",
    style: Style::Round,
    kind: Kind::Repair,
    back: -2400,
    front: 2400,
    z_out: 1170,
    r: 480,
    w: 380,
    axles: &[-1750, -650, 1500],
    cab_x: 1650,
    cab_hl: 750,
    cab_hz: 950,
    roof: 2150,
};

static MERIDIAN_REPAIR: Truck = Truck {
    stem: "meridian_repair_truck",
    faction: "meridian",
    ..REPAIR
};
static VEYRAN_REPAIR: Truck = Truck {
    stem: "veyran_repair_truck",
    faction: "veyran",
    style: Style::Angular,
    ..REPAIR
};
static VEYRAN_TRANSPORT: Truck = Truck {
    stem: "veyran_transport_truck",
    faction: "veyran",
    style: Style::Angular,
    kind: Kind::Transport,
    back: -2820,
    front: 2820,
    z_out: 1170,
    r: 500,
    w: 380,
    axles: &[-2000, -900, 1750],
    cab_x: 1950,
    cab_hl: 800,
    cab_hz: 950,
    roof: 2200,
};
static NEUTRAL_SUPPLY: Truck = Truck {
    stem: "neutral_supply_truck",
    faction: "neutral",
    style: Style::Round,
    kind: Kind::Supply,
    back: -2820,
    front: 2820,
    z_out: 1170,
    r: 500,
    w: 380,
    axles: &[-2000, -900, 1750],
    cab_x: 1950,
    cab_hl: 800,
    cab_hz: 950,
    roof: 2200,
};

pub fn meridian_repair_truck() -> Model {
    truck(&MERIDIAN_REPAIR)
}

pub fn veyran_repair_truck() -> Model {
    truck(&VEYRAN_REPAIR)
}

pub fn veyran_transport_truck() -> Model {
    truck(&VEYRAN_TRANSPORT)
}

pub fn neutral_supply_truck() -> Model {
    truck(&NEUTRAL_SUPPLY)
}
