//! The placeholder mouse cursors (WP3.5): 32 x 32 PNGs, drawn like the icons
//! from signed distance functions with nothing but `+ - * /` and `sqrt`, so the
//! output is the same on every machine. `sf_ui` draws them as software
//! cursors; `sf_game` picks one per cursor context
//! (`crates/sf_game/src/cursor.rs`, `docs/rules/cursor.md`).
//!
//! # The format (for artists)
//! * `assets/ui/cursors/<name>.png`, **32 x 32**, 8-bit RGBA, straight alpha.
//!   The picture is **coloured** (unlike the icons, which are tinted): it is
//!   drawn as it is.
//! * **A dark outline of about 1.7 texels** goes all the way round the
//!   silhouette (the generator adds it; an artist's cursor needs one too), so
//!   the cursor reads on grass, sand, grey rock, snow and water alike. The
//!   test `every_cursor_has_a_dark_outline_all_round` checks it.
//! * Keep the picture about 26 texels across at most, centred, so the outline
//!   and the antialiasing stay inside the image.
//! * `cursors.ron` gives the **hotspot** of each cursor in texels from the
//!   top-left of the image: the pixel that is the mouse position (the tip of
//!   the pointer, the middle of a crosshair), and `size_px`, the logical size
//!   the 32 texels are drawn at (times the UI scale).
//! * Coordinates below are texels from the centre of the image, y up.

use std::collections::BTreeMap;
use std::f64::consts::FRAC_1_SQRT_2;

use super::{Sdf, bar, circle, coverage, inter, obox, polygon, rect, ring, sub, union};

/// Side of a cursor image in texels.
pub const SIDE: usize = 32;
/// Logical pixels the image is drawn at when the UI scale is 1: 1:1, so the
/// texels are exact at scale 1 and only blurred a little at fractional scales.
pub const SIZE_PX: u32 = 32;
/// Width of the dark outline in texels.
const OUTLINE: f64 = 1.7;
/// The colour of the outline.
const DARK: [u8; 3] = [18, 18, 22];

const WHITE: [u8; 3] = [248, 248, 248];
const GREEN: [u8; 3] = [92, 232, 120];
const RED: [u8; 3] = [240, 70, 60];
const ORANGE: [u8; 3] = [255, 140, 50];
const BLUE: [u8; 3] = [96, 156, 255];
const PALE_BLUE: [u8; 3] = [200, 226, 255];
const CYAN: [u8; 3] = [120, 208, 255];
const YELLOW: [u8; 3] = [255, 222, 80];
const GOLD: [u8; 3] = [236, 192, 64];
const PURPLE: [u8; 3] = [196, 130, 244];

/// What a cursor is drawn from: coloured shapes painted in order, with the
/// dark outline round their union.
struct Art {
    layers: Vec<(Sdf, [u8; 3])>,
    /// The hotspot in texels from the top-left of the image.
    hotspot: (u32, u32),
}

/// One cursor of the set.
#[derive(Clone, Copy, Debug)]
pub struct CursorSpec {
    /// The file stem and the key in `cursors.ron`; `CursorKind::name` in
    /// `sf_game` is the same string.
    pub name: &'static str,
}

/// Every cursor of the set, in the order of `CursorKind` in `sf_game`.
pub const NAMES: [&str; 24] = [
    "select",
    "inspect",
    "move",
    "queue",
    "attack",
    "attack_move",
    "guard",
    "no_go",
    "scroll_n",
    "scroll_ne",
    "scroll_e",
    "scroll_se",
    "scroll_s",
    "scroll_sw",
    "scroll_w",
    "scroll_nw",
    "capture",
    "repair",
    "enter",
    "deploy",
    "harvest",
    "infiltrate",
    "sell",
    "place",
];

/// The specs of the whole set.
pub fn cursor_specs() -> Vec<CursorSpec> {
    NAMES.iter().map(|&name| CursorSpec { name }).collect()
}

/// A hotspot at the middle of the image.
const MIDDLE: (u32, u32) = (SIDE as u32 / 2, SIDE as u32 / 2);

/// Hotspot of a point `(x, y)` in centre coordinates (y up).
fn at(x: f64, y: f64) -> (u32, u32) {
    (
        (SIDE as f64 / 2.0 + x) as u32,
        (SIDE as f64 / 2.0 - y) as u32,
    )
}

/// A polygon given in a frame turned so that `(ux, uy)` is its +x axis.
fn turned(points: &[(f64, f64)], ux: f64, uy: f64) -> Sdf {
    let pts: Vec<(f64, f64)> = points
        .iter()
        .map(|&(a, b)| (a * ux - b * uy, a * uy + b * ux))
        .collect();
    polygon(&pts)
}

/// The eight directions, N first and clockwise, as unit vectors.
const DIRECTIONS: [(&str, f64, f64); 8] = [
    ("scroll_n", 0.0, 1.0),
    ("scroll_ne", FRAC_1_SQRT_2, FRAC_1_SQRT_2),
    ("scroll_e", 1.0, 0.0),
    ("scroll_se", FRAC_1_SQRT_2, -FRAC_1_SQRT_2),
    ("scroll_s", 0.0, -1.0),
    ("scroll_sw", -FRAC_1_SQRT_2, -FRAC_1_SQRT_2),
    ("scroll_w", -1.0, 0.0),
    ("scroll_nw", -FRAC_1_SQRT_2, FRAC_1_SQRT_2),
];

/// An arrow along `(ux, uy)` from `from` to the tip at `tip` (distances along
/// the direction from the middle): a shaft and a triangular head.
fn arrow(from: f64, tip: f64, shaft: f64, head: f64, head_len: f64, ux: f64, uy: f64) -> Vec<Sdf> {
    let base = tip - head_len;
    vec![
        bar(from * ux, from * uy, base * ux, base * uy, shaft),
        turned(&[(tip, 0.0), (base, -head), (base, head)], ux, uy),
    ]
}

fn art(name: &str) -> Art {
    let one = |sdf: Sdf, color: [u8; 3]| vec![(sdf, color)];
    if let Some(&(_, ux, uy)) = DIRECTIONS.iter().find(|d| d.0 == name) {
        // A scroll arrow: it points the way the screen scrolls.
        let parts = arrow(-6.0, 13.0, 3.2, 8.5, 11.0, ux, uy);
        return Art {
            layers: vec![(union(parts), WHITE)],
            hotspot: MIDDLE,
        };
    }
    match name {
        // A pointer with its tip at the hotspot.
        "select" => Art {
            layers: one(
                polygon(&[
                    (-9.0, 13.0),
                    (-9.0, -8.0),
                    (-4.5, -4.5),
                    (-1.5, -11.5),
                    (1.8, -10.2),
                    (-1.0, -3.4),
                    (5.0, -3.4),
                ]),
                WHITE,
            ),
            hotspot: at(-9.0, 13.0),
        },
        // A lens with a handle: look at it.
        "inspect" => Art {
            layers: vec![
                (bar(4.0, -3.0, 10.5, -9.5, 2.4), CYAN),
                (ring(-2.0, 3.0, 9.0, 5.6), CYAN),
            ],
            hotspot: at(-2.0, 3.0),
        },
        // Four arrows out of the middle: go there.
        "move" => Art {
            layers: vec![(union(cross_arrows(13.0)), GREEN)],
            hotspot: MIDDLE,
        },
        // The same, with a plus in the corner: go there next.
        "queue" => Art {
            layers: vec![
                (union(cross_arrows(12.0)), GREEN),
                (plus(9.5, -9.5, 3.4, 1.5), YELLOW),
            ],
            hotspot: MIDDLE,
        },
        // A crosshair.
        "attack" => Art {
            layers: vec![
                (ring(0.0, 0.0, 9.0, 6.4), RED),
                (circle(0.0, 0.0, 1.9), RED),
                (
                    union(vec![
                        bar(0.0, 5.0, 0.0, 13.0, 1.6),
                        bar(0.0, -5.0, 0.0, -13.0, 1.6),
                        bar(5.0, 0.0, 13.0, 0.0, 1.6),
                        bar(-5.0, 0.0, -13.0, 0.0, 1.6),
                    ]),
                    RED,
                ),
            ],
            hotspot: MIDDLE,
        },
        // A crosshair that leaves to the right: go there and fight.
        "attack_move" => Art {
            layers: vec![
                (ring(-3.0, 0.0, 8.5, 6.0), ORANGE),
                (circle(-3.0, 0.0, 1.7), ORANGE),
                (
                    union(vec![
                        bar(-3.0, 4.5, -3.0, 12.0, 1.5),
                        bar(-3.0, -4.5, -3.0, -12.0, 1.5),
                        bar(-4.5, 0.0, -13.0, 0.0, 1.5),
                    ]),
                    ORANGE,
                ),
                (
                    union(vec![
                        turned(&[(13.0, 0.0), (6.5, -5.8), (6.5, 5.8)], 1.0, 0.0),
                        turned(&[(8.0, 0.0), (3.0, -4.2), (3.0, 4.2)], 1.0, 0.0),
                    ]),
                    RED,
                ),
            ],
            hotspot: at(-3.0, 0.0),
        },
        // A shield.
        "guard" => Art {
            layers: vec![
                (
                    polygon(&[
                        (-10.0, 12.0),
                        (10.0, 12.0),
                        (10.0, 1.0),
                        (0.0, -13.0),
                        (-10.0, 1.0),
                    ]),
                    BLUE,
                ),
                (
                    polygon(&[
                        (-5.5, 8.0),
                        (5.5, 8.0),
                        (5.5, 2.0),
                        (0.0, -7.0),
                        (-5.5, 2.0),
                    ]),
                    PALE_BLUE,
                ),
            ],
            hotspot: MIDDLE,
        },
        // A forbidden sign.
        "no_go" => Art {
            layers: vec![
                (ring(0.0, 0.0, 12.0, 8.4), RED),
                (bar(-6.5, 6.5, 6.5, -6.5, 2.2), RED),
            ],
            hotspot: MIDDLE,
        },
        // ---- placeholders: later work packages give them a meaning ----
        // A flag on a pole: capture.
        "capture" => Art {
            layers: vec![
                (bar(-7.0, -12.0, -7.0, 12.0, 1.6), WHITE),
                (polygon(&[(-6.0, 12.0), (11.0, 6.0), (-6.0, 0.0)]), GOLD),
            ],
            hotspot: MIDDLE,
        },
        // A spanner: repair.
        "repair" => Art {
            layers: vec![
                (bar(-10.0, -10.0, 3.5, 3.5, 2.6), GOLD),
                (ring(6.5, 6.5, 6.0, 3.2), GOLD),
            ],
            hotspot: MIDDLE,
        },
        // A door with an arrow into it: enter.
        "enter" => Art {
            layers: vec![
                (
                    sub(rect(5.0, 0.0, 7.5, 11.0), rect(5.0, 0.0, 4.8, 8.4)),
                    PALE_BLUE,
                ),
                (union(arrow(-13.0, 3.0, 1.9, 5.5, 6.0, 1.0, 0.0)), GREEN),
            ],
            hotspot: MIDDLE,
        },
        // A box with arrows out of it: deploy.
        "deploy" => Art {
            layers: vec![
                (
                    sub(rect(0.0, 0.0, 6.5, 6.5), rect(0.0, 0.0, 3.6, 3.6)),
                    PALE_BLUE,
                ),
                (
                    union(vec![
                        turned(&[(13.0, 0.0), (8.5, -4.0), (8.5, 4.0)], 0.0, 1.0),
                        turned(&[(13.0, 0.0), (8.5, -4.0), (8.5, 4.0)], 0.0, -1.0),
                        turned(&[(13.0, 0.0), (8.5, -4.0), (8.5, 4.0)], 1.0, 0.0),
                        turned(&[(13.0, 0.0), (8.5, -4.0), (8.5, 4.0)], -1.0, 0.0),
                    ]),
                    GREEN,
                ),
            ],
            hotspot: MIDDLE,
        },
        // A drop: harvest.
        "harvest" => Art {
            layers: vec![(
                union(vec![
                    circle(0.0, -3.5, 8.0),
                    polygon(&[(0.0, 13.0), (-7.2, -1.5), (7.2, -1.5)]),
                ]),
                GOLD,
            )],
            hotspot: MIDDLE,
        },
        // An eye: infiltrate.
        "infiltrate" => Art {
            layers: vec![
                (
                    sub(
                        inter(circle(0.0, -9.0, 16.0), circle(0.0, 9.0, 16.0)),
                        circle(0.0, 0.0, 4.6),
                    ),
                    PURPLE,
                ),
                (circle(0.0, 0.0, 2.2), PURPLE),
            ],
            hotspot: MIDDLE,
        },
        // A coin: sell.
        "sell" => Art {
            layers: vec![
                (circle(0.0, 0.0, 11.5), GOLD),
                (ring(0.0, 0.0, 8.0, 6.2), [150, 112, 20]),
                (bar(0.0, -5.0, 0.0, 5.0, 1.6), [150, 112, 20]),
            ],
            hotspot: MIDDLE,
        },
        // The four corners of a footprint: place.
        "place" => Art {
            layers: vec![(
                union(vec![
                    bar(-12.0, 11.0, -5.0, 11.0, 1.6),
                    bar(-11.0, 12.0, -11.0, 5.0, 1.6),
                    bar(12.0, 11.0, 5.0, 11.0, 1.6),
                    bar(11.0, 12.0, 11.0, 5.0, 1.6),
                    bar(-12.0, -11.0, -5.0, -11.0, 1.6),
                    bar(-11.0, -12.0, -11.0, -5.0, 1.6),
                    bar(12.0, -11.0, 5.0, -11.0, 1.6),
                    bar(11.0, -12.0, 11.0, -5.0, 1.6),
                ]),
                GREEN,
            )],
            hotspot: MIDDLE,
        },
        other => panic!("no art for the cursor {other}"),
    }
}

/// Four arrows from the middle along the axes, tips `reach` out.
fn cross_arrows(reach: f64) -> Vec<Sdf> {
    let mut parts = Vec::new();
    for (ux, uy) in [(0.0, 1.0), (1.0, 0.0), (0.0, -1.0), (-1.0, 0.0)] {
        parts.extend(arrow(0.0, reach, 1.7, 5.2, 6.0, ux, uy));
    }
    parts
}

/// A plus sign centred at `(cx, cy)`, arms `len` long and `half` thick.
fn plus(cx: f64, cy: f64, len: f64, half: f64) -> Sdf {
    union(vec![
        obox(cx, cy, len, half, 1.0, 0.0),
        obox(cx, cy, len, half, 0.0, 1.0),
    ])
}

/// The hotspot of a cursor in texels from the top-left.
pub fn hotspot(name: &str) -> (u32, u32) {
    art(name).hotspot
}

/// The RGBA pixels of a cursor, 32 x 32, straight (not premultiplied) alpha.
pub fn render(spec: CursorSpec) -> Vec<u8> {
    let art = art(spec.name);
    let mut out = Vec::with_capacity(SIDE * SIDE * 4);
    for j in 0..SIDE {
        for i in 0..SIDE {
            let x = i as f64 + 0.5 - SIDE as f64 / 2.0;
            let y = SIDE as f64 / 2.0 - (j as f64 + 0.5);
            let near = art
                .layers
                .iter()
                .map(|(sdf, _)| sdf(x, y))
                .fold(f64::MAX, f64::min);
            // Paint the outline, then every layer over it.
            let (mut r, mut g, mut b) =
                (f64::from(DARK[0]), f64::from(DARK[1]), f64::from(DARK[2]));
            let mut a = coverage(near - OUTLINE);
            for (sdf, color) in &art.layers {
                let f = coverage(sdf(x, y));
                if f <= 0.0 {
                    continue;
                }
                let total = f + a * (1.0 - f);
                let mix = |old: f64, new: u8| (f64::from(new) * f + old * a * (1.0 - f)) / total;
                (r, g, b) = (mix(r, color[0]), mix(g, color[1]), mix(b, color[2]));
                a = total;
            }
            out.extend_from_slice(&[
                r.round() as u8,
                g.round() as u8,
                b.round() as u8,
                (a * 255.0).round() as u8,
            ]);
        }
    }
    out
}

/// The text of `cursors.ron`.
pub fn manifest() -> String {
    let mut map: BTreeMap<&str, (u32, u32)> = BTreeMap::new();
    for spec in cursor_specs() {
        map.insert(spec.name, hotspot(spec.name));
    }
    let mut text = format!(
        "// The software cursors of the game (WP3.5), written by tools/sf_icon_gen\n\
         // (`cargo run -p sf_icon_gen -- --cursors`) together with the PNGs.\n\
         // size_px: logical pixels a 32 x 32 image is drawn at (times the UI scale).\n\
         // hotspot: the pixel of the image that is the mouse position, in texels\n\
         // from the top-left. See docs/rules/cursor.md.\n\
         (\n    size_px: {SIZE_PX},\n    cursors: {{\n"
    );
    for (name, (x, y)) in &map {
        text.push_str(&format!(
            "        \"{name}\": (file: \"{name}.png\", hotspot: ({x}, {y})),\n"
        ));
    }
    text.push_str("    },\n)\n");
    text
}

/// Every file of the set as `(file name, bytes)`: the PNGs and `cursors.ron`.
pub fn files() -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = cursor_specs()
        .into_iter()
        .map(|spec| {
            (
                format!("{}.png", spec.name),
                super::encode_png_sized(&render(spec), SIDE as u32),
            )
        })
        .collect();
    out.push(("cursors.ron".to_string(), manifest().into_bytes()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn assets_cursors() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/ui/cursors")
    }

    fn alpha(px: &[u8], x: usize, y: usize) -> u8 {
        px[(y * SIDE + x) * 4 + 3]
    }

    #[test]
    fn the_set_has_the_cursors_of_the_table_and_each_name_once() {
        let mut names = NAMES.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 24);
        for (name, ..) in DIRECTIONS {
            assert!(NAMES.contains(&name), "{name}");
        }
    }

    #[test]
    fn rendering_is_deterministic_and_the_size_is_right() {
        for spec in cursor_specs() {
            let px = render(spec);
            assert_eq!(px.len(), SIDE * SIDE * 4);
            assert!(px == render(spec), "{} is not deterministic", spec.name);
        }
    }

    #[test]
    fn every_cursor_has_a_dark_outline_all_round() {
        for spec in cursor_specs() {
            let px = render(spec);
            let at = |x: usize, y: usize| &px[(y * SIDE + x) * 4..(y * SIDE + x) * 4 + 4];
            let (mut opaque, mut edge) = (0, 0);
            for y in 1..SIDE - 1 {
                for x in 1..SIDE - 1 {
                    if alpha(&px, x, y) < 128 {
                        continue;
                    }
                    opaque += 1;
                    // A pixel with an empty neighbour (of eight) is on the silhouette:
                    // it must be the dark outline, not the colour, so the
                    // cursor reads on any ground.
                    let bare = [
                        (x - 1, y),
                        (x + 1, y),
                        (x, y - 1),
                        (x, y + 1),
                        (x - 1, y - 1),
                        (x + 1, y - 1),
                        (x - 1, y + 1),
                        (x + 1, y + 1),
                    ]
                    .iter()
                    .any(|&(nx, ny)| alpha(&px, nx, ny) == 0);
                    if bare {
                        edge += 1;
                        let p = at(x, y);
                        let sum = u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]);
                        assert!(
                            sum < 3 * 90,
                            "{}: the edge pixel ({x}, {y}) is {p:?}, not dark",
                            spec.name
                        );
                    }
                }
            }
            assert!(opaque > 150, "{}: only {opaque} opaque texels", spec.name);
            assert!(edge > 20, "{}: only {edge} edge texels", spec.name);
        }
    }

    #[test]
    fn nothing_touches_the_border_of_the_image_and_the_hotspot_is_inside() {
        for spec in cursor_specs() {
            let px = render(spec);
            for i in 0..SIDE {
                for (x, y) in [(i, 0), (i, SIDE - 1), (0, i), (SIDE - 1, i)] {
                    assert!(
                        alpha(&px, x, y) < 32,
                        "{}: the picture reaches the border at ({x}, {y})",
                        spec.name
                    );
                }
            }
            let (hx, hy) = hotspot(spec.name);
            assert!(
                (hx as usize) < SIDE && (hy as usize) < SIDE,
                "{}: hotspot {hx},{hy}",
                spec.name
            );
        }
    }

    #[test]
    fn the_pointer_hotspot_is_its_tip_and_the_others_are_in_the_picture() {
        // The tip of the pointer is opaque and is the hotspot.
        let (x, y) = hotspot("select");
        assert!(
            alpha(
                &render(CursorSpec { name: "select" }),
                x as usize,
                y as usize + 1
            ) > 200
        );
        // The crosshairs are centred on theirs.
        for name in ["attack", "move", "no_go", "guard"] {
            assert_eq!(hotspot(name), MIDDLE, "{name}");
        }
    }

    #[test]
    fn the_cursors_differ_from_each_other() {
        let all: Vec<_> = cursor_specs().into_iter().map(render).collect();
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate().skip(i + 1) {
                assert!(a != b, "{} and {} are the same picture", NAMES[i], NAMES[j]);
            }
        }
    }

    #[test]
    fn the_committed_files_are_what_the_generator_writes() {
        let dir = assets_cursors();
        for (name, bytes) in files() {
            let on_disk = std::fs::read(dir.join(&name)).unwrap_or_else(|e| {
                panic!("{name}: {e}; run `cargo run -p sf_icon_gen -- --cursors`")
            });
            if name.ends_with(".png") {
                // (Not assert_eq!: a failure would print every pixel.)
                assert!(
                    super::super::decode_png_sized(&on_disk, SIDE as u32).unwrap()
                        == super::super::decode_png_sized(&bytes, SIDE as u32).unwrap(),
                    "{name} is out of date; run `cargo run -p sf_icon_gen -- --cursors`"
                );
            } else {
                assert!(
                    super::super::same_text(&on_disk, &bytes),
                    "{name} is out of date"
                );
            }
        }
    }
}
