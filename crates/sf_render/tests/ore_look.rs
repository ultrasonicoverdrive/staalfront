//! The fill levels of the ore look (WP7.1): which level a cell shows, that the textures and the
//! minimap colours of the levels fit together, and that a bad `resource` block is refused.

use sf_data::terrain::TerrainTypes;
use sf_render::terrain_look::{LookError, TerrainLooks};

fn looks() -> TerrainLooks {
    TerrainLooks::embedded()
}

fn mean(texture: &[u8]) -> [f32; 3] {
    let n = (texture.len() / 4) as f32;
    let mut sum = [0.0_f32; 3];
    for p in texture.chunks(4) {
        for c in 0..3 {
            sum[c] += f32::from(p[c]);
        }
    }
    sum.map(|s| s / n)
}

/// The shipped levels, written out: changing `terrain_look.ron` means changing this on purpose.
#[test]
fn the_shipped_levels_are_pinned() {
    let looks = looks();
    let r = looks.resource().unwrap();
    assert_eq!(r.terrain, "ore");
    let got: Vec<(u8, [u8; 3], u8, [u8; 3])> = r
        .levels
        .iter()
        .map(|l| (l.up_to_percent, l.color, l.flecks_percent, l.minimap))
        .collect();
    assert_eq!(
        got,
        vec![
            (0, [84, 64, 44], 0, [84, 64, 44]),
            (33, [90, 63, 38], 30, [104, 76, 40]),
            (66, [94, 65, 36], 65, [122, 88, 40]),
            (100, [98, 66, 34], 100, [138, 100, 39]),
        ]
    );
}

/// Empty is level 0 and nothing else is; a non-empty cell never shows empty; the boundaries are
/// inclusive (the fill is `amount * 100 / max`, rounded down); full is the last level.
#[test]
fn a_cell_shows_the_first_level_that_holds_its_fill() {
    let looks = looks();
    let r = looks.resource().unwrap();
    let level = |amount| r.level_of(amount, 160);
    assert_eq!(level(0), 0);
    assert_eq!(level(1), 1, "one unit is not empty");
    assert_eq!(level(54), 1, "33 percent (33.75 rounded down) is level 1");
    assert_eq!(level(55), 2, "34 percent");
    assert_eq!(level(106), 2, "66 percent (66.25 rounded down)");
    assert_eq!(level(107), 2, "66 percent (66.9 rounded down)");
    assert_eq!(level(108), 3, "67 percent");
}

#[test]
fn the_top_levels_hold_a_full_cell() {
    let looks = looks();
    let r = looks.resource().unwrap();
    assert_eq!(r.level_of(160, 160), 3);
    assert_eq!(r.level_of(120, 160), 3, "the start amount is 75 percent");
    // A maximum of zero cannot divide by zero.
    assert_eq!(r.level_of(5, 0), 3);
}

/// Each level's minimap colour is near the mean of its texture (as for the types), and the
/// levels get lighter and more gold as the cell fills.
#[test]
fn level_textures_agree_with_their_minimap_colours_and_rise() {
    let looks = looks();
    let look = looks.look_for("ore");
    let r = looks.resource().unwrap();
    let mut last_red = 0.0_f32;
    for (i, level) in r.levels.iter().enumerate() {
        let tex = look.level_texture(level);
        let m = mean(&tex);
        let d = (0..3)
            .map(|c| (m[c] - f32::from(level.minimap[c])).powi(2))
            .sum::<f32>()
            .sqrt();
        assert!(
            d < 40.0,
            "level {i}: minimap {:?} vs mean {m:?} ({d})",
            level.minimap
        );
        assert!(
            m[0] > last_red,
            "level {i} is not lighter than the one below"
        );
        last_red = m[0];
    }
    // The full level is the look itself.
    assert_eq!(
        look.level_texture(r.levels.last().unwrap()),
        look.texture(),
        "the full level draws as the plain ore look"
    );
}

/// An empty cell has no flecks at all: its texture is the ground colour with the look's
/// variation only, so it holds far fewer distinct bright pixels than a full one.
#[test]
fn an_empty_cell_has_no_flecks() {
    let looks = looks();
    let look = looks.look_for("ore");
    let r = looks.resource().unwrap();
    let bright = |tex: &[u8]| tex.chunks(4).filter(|p| p[0] > 200 && p[1] > 150).count();
    let empty = bright(&look.level_texture(&r.levels[0]));
    let full = bright(&look.level_texture(&r.levels[3]));
    assert_eq!(empty, 0);
    assert!(full > 200, "a full cell shows flecks ({full})");
}

#[test]
fn the_surface_has_the_level_layers_after_the_types() {
    let types = TerrainTypes::embedded();
    let looks = looks();
    let map = sf_data::map::generate::generate_cells(64, 64, 1);
    let surface = sf_render::terrain_look::terrain_surface(&map, &types, &looks);
    let res = surface.resource.expect("resource layers");
    assert_eq!(res.type_id, types.id_of("ore").unwrap());
    assert_eq!(usize::from(res.first_layer), types.len());
    assert_eq!(surface.layers.len(), types.len() + 4);
    // The level layers are the level textures.
    let look = looks.look_for("ore");
    let r = looks.resource().unwrap();
    for (i, level) in r.levels.iter().enumerate() {
        assert_eq!(
            surface.layers[usize::from(res.first_layer) + i],
            look.level_texture(level)
        );
    }
}

#[test]
fn a_bad_resource_block_is_refused() {
    let base = |res: &str| {
        format!(
            "(edge: (ramp_m: 2.0, wobble_m: 0.7, shore: (0.8, 0.9, 0.9), shore_amount: 0.4),
              resource: {res},
              looks: [(terrain: \"ore\", color: (1, 2, 3), contrast: 0.2,
                       pattern: Flecks(color: (200, 200, 0)))])"
        )
    };
    let lvl = |up, flecks| {
        format!(
            "(up_to_percent: {up}, color: (1, 2, 3), flecks_percent: {flecks}, minimap: (1, 2, 3))"
        )
    };
    let block = |levels: &[String]| format!("(terrain: \"ore\", levels: [{}])", levels.join(","));
    let ok = block(&[lvl(0, 0), lvl(100, 100)]);
    assert!(TerrainLooks::from_ron(&base(&ok)).is_ok());
    for (what, bad) in [
        ("empty", block(&[])),
        ("no empty level", block(&[lvl(10, 0), lvl(100, 100)])),
        ("not ending at 100", block(&[lvl(0, 0), lvl(90, 100)])),
        (
            "not rising",
            block(&[lvl(0, 0), lvl(50, 50), lvl(50, 70), lvl(100, 100)]),
        ),
        ("flecks over 100", block(&[lvl(0, 0), lvl(100, 101)])),
    ] {
        let e: Result<TerrainLooks, LookError> = TerrainLooks::from_ron(&base(&bad));
        assert!(e.is_err(), "{what} should be refused");
    }
    // A resource whose type has no Flecks look.
    let text = base(&ok).replace("Flecks(color: (200, 200, 0))", "Noise");
    assert!(TerrainLooks::from_ron(&text).is_err());
    // And a resource that names no terrain type.
    let looks = TerrainLooks::from_ron(
        &base(&ok).replace("terrain: \"ore\", levels", "terrain: \"nope\", levels"),
    );
    // Without flecks look for "nope" it is already refused at parse; with the check against
    // the table it is refused too.
    assert!(looks.is_err());
}
