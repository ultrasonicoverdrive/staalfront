//! The check behind the colour-blind safe set of player colours (WP11.5a, `docs/rules/menus.md`).
//!
//! The method: each colour is turned into linear light, run through the colour-vision-deficiency matrix
//! of Machado, Oliveira and Fernandes (2009, "A Physiologically-based Model for Simulation of Color
//! Vision Deficiency", severity 1.0) for protanopia, deuteranopia and tritanopia, and turned into CIELAB
//! (D65). The distance of two players' colours is CIE76 delta E, the plain distance in Lab. The
//! smallest distance between any two of the four players, for normal vision and for each deficiency,
//! has to be at least [`LEAST`] (about the point at which two colours that are not side by side are told
//! apart reliably). This is a screening of the colours, not a clinical test: the matrices model full
//! dichromacy, which is the hardest case of each kind, and a person with a milder deficiency sees more.
//!
//! The standard set of `units.ron` is measured the same way. It is not safe for protanopia (blue and
//! magenta come together), which is why the second set exists; the test states that too, so a change to
//! the standard colours that made the option pointless would show.

use sf_game::palette::Palette;
use sf_render::Style;

/// The least distance, in CIE76 delta E, between two players' colours.
const LEAST: f64 = 20.0;

type Matrix = [[f64; 3]; 3];

const IDENTITY: Matrix = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
const PROTANOPIA: Matrix = [
    [0.152286, 1.052583, -0.204868],
    [0.114503, 0.786281, 0.099216],
    [-0.003882, -0.048116, 1.051998],
];
const DEUTERANOPIA: Matrix = [
    [0.367322, 0.860646, -0.227968],
    [0.280085, 0.672501, 0.047413],
    [-0.011820, 0.042940, 0.968881],
];
const TRITANOPIA: Matrix = [
    [1.255528, -0.076749, -0.178779],
    [-0.078411, 0.930809, 0.147602],
    [0.004733, 0.691367, 0.303900],
];

const VISIONS: [(&str, Matrix); 4] = [
    ("normal", IDENTITY),
    ("protanopia", PROTANOPIA),
    ("deuteranopia", DEUTERANOPIA),
    ("tritanopia", TRITANOPIA),
];

fn linear(byte: u8) -> f64 {
    let c = f64::from(byte) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn simulate(srgb: [u8; 3], m: &Matrix) -> [f64; 3] {
    let l = srgb.map(linear);
    [0, 1, 2].map(|i| (0..3).map(|j| m[i][j] * l[j]).sum::<f64>())
}

fn lab(rgb: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = rgb;
    let x = 0.4124564 * r + 0.3575761 * g + 0.1804375 * b;
    let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
    let z = 0.0193339 * r + 0.1191920 * g + 0.9503041 * b;
    let f = |t: f64| {
        if t > 216.0 / 24389.0 {
            t.cbrt()
        } else {
            (24389.0 / 27.0 * t + 16.0) / 116.0
        }
    };
    let (fx, fy, fz) = (f(x / 0.95047), f(y), f(z / 1.08883));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn delta_e(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The smallest distance between two of the colours as `vision` sees them, and which two.
fn closest(colours: &[[u8; 3]], vision: &Matrix) -> (f64, usize, usize) {
    let seen: Vec<[f64; 3]> = colours.iter().map(|&c| lab(simulate(c, vision))).collect();
    let mut least = (f64::MAX, 0, 0);
    for i in 0..seen.len() {
        for j in i + 1..seen.len() {
            let d = delta_e(seen[i], seen[j]);
            if d < least.0 {
                least = (d, i, j);
            }
        }
    }
    least
}

fn colour_blind_set() -> Vec<[u8; 3]> {
    Palette::embedded()
        .players
        .iter()
        .map(|c| [c.srgb.0, c.srgb.1, c.srgb.2])
        .collect()
}

fn standard_set() -> Vec<[u8; 3]> {
    let style = Style::embedded();
    (0..4).map(|p| style.srgb(p)).collect()
}

/// The matrices are typed from a paper: a typo would make every number below meaningless. Each row of
/// a deficiency matrix sums to 1, so that grey stays grey, and a grey is left where it is.
#[test]
fn the_vision_matrices_keep_grey_grey() {
    for (name, m) in VISIONS {
        for row in m {
            assert!(
                (row.iter().sum::<f64>() - 1.0).abs() < 1e-3,
                "{name}: {row:?}"
            );
        }
        let grey = simulate([128, 128, 128], &m);
        assert!(
            (grey[0] - grey[1]).abs() < 1e-3 && (grey[1] - grey[2]).abs() < 1e-3,
            "{name}: {grey:?}"
        );
        assert!((grey[0] - linear(128)).abs() < 1e-3, "{name}");
    }
    // And the distance is a distance: black and white are 100 apart in lightness.
    let d = delta_e(
        lab(simulate([0, 0, 0], &IDENTITY)),
        lab(simulate([255, 255, 255], &IDENTITY)),
    );
    assert!((d - 100.0).abs() < 0.5, "{d}");
}

#[test]
fn the_colour_blind_set_stays_apart_for_every_common_colour_vision() {
    let set = colour_blind_set();
    assert_eq!(set.len(), 4);
    for (name, m) in VISIONS {
        let (d, i, j) = closest(&set, &m);
        println!("colour-blind set, {name}: closest pair {i}-{j} at delta E {d:.1}");
        assert!(
            d >= LEAST,
            "{name}: players {i} and {j} are only {d:.1} apart"
        );
    }
}

#[test]
fn the_standard_set_is_not_safe_for_protanopia_which_is_why_there_is_a_second() {
    let standard = standard_set();
    let ours = colour_blind_set();
    let (std_p, i, j) = closest(&standard, &PROTANOPIA);
    let (our_p, _, _) = closest(&ours, &PROTANOPIA);
    println!(
        "standard set, protanopia: closest pair {i}-{j} at delta E {std_p:.1}; ours {our_p:.1}"
    );
    assert!(
        std_p < LEAST,
        "the standard colours are fine already: {std_p:.1}"
    );
    // Blue (1) and magenta (3) are the pair that comes together: both look blue to a protanope.
    assert_eq!((i, j), (1, 3));
    assert!(our_p > 1.5 * std_p, "{our_p:.1} against {std_p:.1}");
    // For normal vision the standard set is the wider of the two, as it was drawn.
    let (std_n, _, _) = closest(&standard, &IDENTITY);
    assert!(std_n >= LEAST);
}

#[test]
fn the_second_set_has_the_four_players_of_the_first_in_the_same_order() {
    let alt = Palette::embedded();
    assert_eq!(alt.players.len(), Style::embedded().color_count());
    let names: Vec<_> = alt.players.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["vermilion", "blue", "yellow", "reddish purple"]);
    assert_eq!(
        colour_blind_set(),
        [[213, 94, 0], [0, 114, 178], [240, 228, 66], [204, 121, 167]]
    );
    // Their dots on the minimap are their colours.
    let dots: Vec<(u8, u8, u8)> = alt.minimap.clone();
    let colours: Vec<(u8, u8, u8)> = alt.players.iter().map(|c| c.srgb).collect();
    assert_eq!(dots, colours);
}
