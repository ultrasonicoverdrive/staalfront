//! The fog of war on the minimap (WP8.4): shroud, fog and visible cells in their colours, the terrain
//! under it kept (and the ore painted under it), taken off again without a trace.

use sf_data::map::generate::generate_cells;
use sf_ui::{FOG, Minimap, MinimapFog, SHROUD, VISIBLE, fogged_color};

const LOOK: MinimapFog = MinimapFog {
    shroud: [4, 8, 12],
    fog_dim: 0.5,
};

fn px(m: &Minimap, x: u32, y: u32) -> [u8; 3] {
    let i = m.image();
    let at = ((y * i.width + x) * 4) as usize;
    [i.rgba[at], i.rgba[at + 1], i.rgba[at + 2]]
}

fn clean_px(m: &Minimap, x: u32, y: u32) -> [u8; 3] {
    let w = m.image().width;
    let at = ((y * w + x) * 4) as usize;
    [
        m.clean_rgba()[at],
        m.clean_rgba()[at + 1],
        m.clean_rgba()[at + 2],
    ]
}

/// 16 cells a side, 16 pixels a cell: the pixel `(cx * 16 + 5, cy * 16 + 5)` is inside cell `(cx, cy)`.
fn inside(cx: u32, cy: u32) -> (u32, u32) {
    (cx * 16 + 5, cy * 16 + 5)
}

/// Column 0..4 shroud, 4..8 fog, 8.. visible, on every row.
fn states() -> Vec<u8> {
    (0..16 * 16)
        .map(|i| match i % 16 {
            0..=3 => SHROUD,
            4..=7 => FOG,
            _ => VISIBLE,
        })
        .collect()
}

#[test]
fn each_state_has_its_colour_and_the_terrain_shows_through_fog_and_visible() {
    let map = generate_cells(16, 16, 1);
    let mut m = Minimap::bake(&map, 256);
    assert_eq!(m.image().rgba.len(), 256 * 256 * 4);
    let clean = m.image().rgba.clone();
    assert!(m.set_fog(&map, Some((&states(), LOOK))));
    for cy in 0..16 {
        for cx in 0..16 {
            let (x, y) = inside(cx, cy);
            let terrain = clean_px(&m, x, y);
            let want = match cx {
                0..=3 => LOOK.shroud,
                4..=7 => fogged_color(terrain, FOG, &LOOK),
                _ => terrain,
            };
            assert_eq!(px(&m, x, y), want, "cell {cx},{cy}");
        }
    }
    // The visible columns are bit for bit the bake; the alpha is untouched everywhere.
    for y in 0..256_u32 {
        for x in 128..256_u32 {
            let at = ((y * 256 + x) * 4) as usize;
            assert_eq!(m.image().rgba[at..at + 4], clean[at..at + 4], "{x},{y}");
        }
    }
    assert!(m.image().rgba.chunks(4).all(|p| p[3] == 255));
    // Fog is between the terrain and the shroud colour, strictly, for a bright terrain.
    let t = [200_u8, 200, 200];
    let f = fogged_color(t, FOG, &LOOK);
    assert!(f[0] < t[0] && f[0] > LOOK.shroud[0]);
}

#[test]
fn taking_the_fog_off_gives_the_bake_back_and_the_same_fog_twice_changes_nothing() {
    let map = generate_cells(16, 16, 1);
    let mut m = Minimap::bake(&map, 256);
    let clean = m.image().clone();
    assert!(m.set_fog(&map, Some((&states(), LOOK))));
    assert!(
        !m.set_fog(&map, Some((&states(), LOOK))),
        "the same fog again: no change, no upload"
    );
    assert!(m.set_fog(&map, None));
    assert_eq!(*m.image(), clean);
    assert!(!m.set_fog(&map, None), "no fog and no fog");
}

#[test]
fn fog_of_the_wrong_size_is_refused() {
    let map = generate_cells(16, 16, 1);
    let mut m = Minimap::bake(&map, 256);
    let before = m.image().clone();
    assert!(!m.set_fog(&map, Some((&[VISIBLE; 10], LOOK))));
    assert_eq!(*m.image(), before);
}

#[test]
fn ore_painted_into_a_shroud_cell_stays_hidden_and_shows_when_the_fog_goes() {
    let map = generate_cells(16, 16, 1);
    let mut m = Minimap::bake(&map, 256);
    assert!(m.set_fog(&map, Some((&states(), LOOK))));
    // Cell (1, 1) is shroud, (5, 1) fog, (10, 1) visible.
    let n = m.paint_cells(
        &map,
        &[
            (1, 1, [255, 0, 255]),
            (5, 1, [255, 0, 255]),
            (10, 1, [255, 0, 255]),
        ],
    );
    assert_eq!(
        n, 2,
        "the shroud cell shows the same, so only two cells changed"
    );
    let (sx, sy) = inside(1, 1);
    assert_eq!(px(&m, sx, sy), LOOK.shroud, "shroud hides the ore");
    let (fx, fy) = inside(5, 1);
    let ore = clean_px(&m, fx, fy);
    assert_eq!(px(&m, fx, fy), fogged_color(ore, FOG, &LOOK), "fog dims it");
    let (vx, vy) = inside(10, 1);
    assert_eq!(px(&m, vx, vy), clean_px(&m, vx, vy));
    assert!(px(&m, vx, vy)[1] == 0, "magenta: no green");
    // Taking the fog off shows the ore that was painted under the shroud.
    assert!(m.set_fog(&map, None));
    let shown = px(&m, sx, sy);
    assert!(
        shown[0] > 200 && shown[2] > 200 && shown[1] == 0,
        "{shown:?}"
    );
}
