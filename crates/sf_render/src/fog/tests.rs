use super::*;

fn words(w: u32, h: u32, cells: &[(u32, u32)]) -> Vec<u64> {
    let mut v = vec![0u64; (w * h).div_ceil(64) as usize];
    for &(x, y) in cells {
        let i = (y * w + x) as usize;
        v[i / 64] |= 1 << (i % 64);
    }
    v
}

fn at(t: &[u8], w: u32, x: u32, y: u32) -> (u8, u8) {
    let i = ((y * w + x) * 2) as usize;
    (t[i], t[i + 1])
}

#[test]
fn without_softness_the_texels_are_the_bits() {
    let (w, h) = (6, 4);
    let visible = words(w, h, &[(1, 1)]);
    let explored = words(w, h, &[(1, 1), (2, 1), (5, 3)]);
    let t = texels(w, h, &visible, &explored, 0);
    assert_eq!(t.len(), 6 * 4 * 2);
    assert_eq!(at(&t, w, 1, 1), (255, 255), "visible is also explored");
    assert_eq!(at(&t, w, 2, 1), (255, 0), "fog: explored, not visible");
    assert_eq!(at(&t, w, 5, 3), (255, 0));
    assert_eq!(at(&t, w, 0, 0), (0, 0), "shroud");
    assert_eq!(at(&t, w, 3, 1), (0, 0));
}

#[test]
fn red_is_explored_and_green_is_visible_not_the_other_way() {
    let (w, h) = (4, 4);
    // Only explored: red on, green off. A swap of the two channels shows here.
    let t = texels(w, h, &words(w, h, &[]), &words(w, h, &[(2, 2)]), 0);
    assert_eq!(at(&t, w, 2, 2), (255, 0));
    // Only visible (cannot happen in the sim, but the mapping must not mix them up).
    let t = texels(w, h, &words(w, h, &[(1, 1)]), &words(w, h, &[]), 0);
    assert_eq!(at(&t, w, 1, 1), (0, 255));
}

#[test]
fn the_row_stride_is_the_width() {
    // A cell at (x, y) must land at texel y * width + x, not x * height + y.
    let (w, h) = (5, 3);
    let t = texels(w, h, &words(w, h, &[(4, 0)]), &words(w, h, &[(4, 0)]), 0);
    assert_eq!(at(&t, w, 4, 0), (255, 255));
    assert_eq!(at(&t, w, 0, 2), (0, 0));
    assert_eq!(t.iter().filter(|&&b| b == 255).count(), 2);
}

#[test]
fn softness_spreads_an_edge_over_cells_and_keeps_the_sides() {
    let (w, h) = (16, 3);
    let left: Vec<(u32, u32)> = (0..8).flat_map(|x| (0..3).map(move |y| (x, y))).collect();
    let bits = words(w, h, &left);
    let hard = texels(w, h, &bits, &bits, 0);
    let soft = texels(w, h, &bits, &bits, 2);
    // The hard edge jumps from 255 to 0 between x = 7 and x = 8.
    assert_eq!((at(&hard, w, 7, 1).1, at(&hard, w, 8, 1).1), (255, 0));
    // The soft one passes through values in between, falling monotonically.
    let row: Vec<u8> = (0..16).map(|x| at(&soft, w, x, 1).1).collect();
    assert!(row.windows(2).all(|p| p[0] >= p[1]), "{row:?}");
    assert!(
        row.iter().filter(|&&v| v > 0 && v < 255).count() >= 3,
        "{row:?}"
    );
    // Far from the edge nothing changed.
    assert_eq!(row[0], 255);
    assert_eq!(row[15], 0);
}

#[test]
fn a_blurred_visible_cell_is_never_brighter_than_explored() {
    let (w, h) = (12, 9);
    let visible = words(w, h, &[(3, 3), (4, 3), (8, 6)]);
    let explored = words(
        w,
        h,
        &[(3, 3), (4, 3), (8, 6), (2, 3), (5, 3), (7, 6), (9, 6)],
    );
    for s in 0..=4 {
        let t = texels(w, h, &visible, &explored, s);
        for y in 0..h {
            for x in 0..w {
                let (r, g) = at(&t, w, x, y);
                assert!(g <= r, "softness {s} at {x},{y}: explored {r} visible {g}");
            }
        }
    }
}

#[test]
fn short_bitsets_read_as_clear() {
    let t = texels(8, 8, &[], &[], 2);
    assert!(t.iter().all(|&b| b == 0));
    assert_eq!(t.len(), 8 * 8 * 2);
}

#[test]
fn srgb_to_linear_has_its_fixed_points() {
    assert_eq!(srgb_to_linear(0), 0.0);
    assert!((srgb_to_linear(255) - 1.0).abs() < 1e-6);
    assert!((srgb_to_linear(128) - 0.2158).abs() < 1e-3);
}
