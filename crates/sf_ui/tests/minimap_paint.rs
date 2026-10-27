//! Repainting single minimap cells (WP7.1: the ore a cell still holds).

use sf_data::map::generate::generate_cells;
use sf_ui::Minimap;

fn px(m: &Minimap, x: u32, y: u32) -> [u8; 3] {
    let i = m.image();
    let at = ((y * i.width + x) * 4) as usize;
    [i.rgba[at], i.rgba[at + 1], i.rgba[at + 2]]
}

#[test]
fn a_painted_cell_changes_its_pixels_and_no_others() {
    let map = generate_cells(16, 16, 1);
    // 256 pixels for 16 cells: 16 pixels per cell side.
    let mut m = Minimap::bake(&map, 256);
    let before = m.image().clone();
    let n = m.paint_cells(&map, &[(5, 7, [255, 0, 255])]);
    assert_eq!(n, 1);
    let mut changed = 0;
    for y in 0..256_u32 {
        for x in 0..256_u32 {
            let i = ((y * 256 + x) * 4) as usize;
            let was = before.rgba[i..i + 4] != m.image().rgba[i..i + 4];
            let inside = (5..6).contains(&(x / 16)) && (7..8).contains(&(y / 16));
            assert_eq!(was, inside, "pixel ({x}, {y})");
            changed += usize::from(was);
        }
    }
    assert_eq!(changed, 16 * 16);
    // The colour is the one asked for under the relief shade (at most 18 percent either way,
    // and the green channel stays zero).
    let c = px(&m, 5 * 16 + 3, 7 * 16 + 3);
    assert!(c[0] >= 208 && c[2] >= 208 && c[1] == 0, "{c:?}");
    // Painting the same colour again changes nothing.
    assert_eq!(m.paint_cells(&map, &[(5, 7, [255, 0, 255])]), 0);
    // Cells off the map are skipped.
    assert_eq!(
        m.paint_cells(&map, &[(16, 0, [1, 2, 3]), (0, 99, [1, 2, 3])]),
        0
    );
}

/// On a map larger than the image (sampled down) every cell still has pixels of its own or is
/// skipped without touching a neighbour: painting all cells leaves no stale pixel.
#[test]
fn painting_every_cell_of_a_downsampled_map_recolours_the_whole_image() {
    let map = generate_cells(64, 64, 1);
    let mut m = Minimap::bake(&map, 40);
    let all: Vec<(u32, u32, [u8; 3])> = (0..64)
        .flat_map(|y| (0..64).map(move |x| (x, y, [200, 200, 200])))
        .collect();
    m.paint_cells(&map, &all);
    let i = m.image();
    // Under the relief shade (0.82 to 1.18) every pixel is a grey near 200.
    for p in i.rgba.chunks(4) {
        assert!(
            p[0] == p[1] && p[1] == p[2] && (164..=236).contains(&p[0]),
            "{p:?}"
        );
    }
}
