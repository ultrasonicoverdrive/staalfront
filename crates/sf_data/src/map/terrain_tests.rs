//! Tests of the terrain layer of a map (WP2.1): loading, validation, file name
//! safety and the generator's layer.

use std::path::PathBuf;

use super::generate::{generate, generate_cells, terrain_for};
use super::tests::{ESCAPING_NAMES, cells_png, header, height_png};
use super::*;
use crate::terrain::{TerrainLayer, TerrainType, TerrainTypes};

/// A header for a 4 x 3 m map (a 1 x 1 cell playfield) with a terrain file.
fn header_with_terrain(name: &str) -> String {
    format!(
        "(width: 4, height: 3, height_scale: 4.0, materials: [\"a\", \"b\"], \
         heightmap: \"height.png\", material_map: \"materials.png\", terrain_map: Some({name:?}))"
    )
}

fn terrain_png(w: u32, h: u32, id: u8) -> Vec<u8> {
    let data = vec![id; (w * h) as usize];
    encode_gray("terrain.png", &data, png::BitDepth::Eight, (w, h)).unwrap()
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sf_data_terrain_{label}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// --- loading and saving ---

#[test]
fn parse_with_reads_the_layer() {
    // 8 x 8 m = 2 x 2 cells.
    let text = "(width: 8, height: 8, height_scale: 4.0, materials: [\"a\"], \
                heightmap: \"h.png\", material_map: \"m.png\", terrain_map: Some(\"t.png\"))";
    let t = encode_gray("t.png", &[0, 2, 5, 1], png::BitDepth::Eight, (2, 2)).unwrap();
    let m = Map::parse_with(
        text,
        &height_png(9, 9),
        &cells_png(8, 8, 0),
        Some(&t),
        &TerrainTypes::embedded(),
    )
    .unwrap();
    let layer = m.terrain().unwrap();
    assert_eq!((layer.width(), layer.height()), (2, 2));
    assert_eq!(layer.get(1, 0), Some(2));
    assert_eq!(layer.get(0, 1), Some(5));
    assert_eq!(layer.get(1, 1), Some(1));
}

#[test]
fn a_map_without_a_layer_loads_as_before_and_writes_no_terrain_line() {
    let m = Map::parse(&header(4, 3), &height_png(5, 4), &cells_png(4, 3, 1)).unwrap();
    assert!(m.terrain().is_none());
    let dir = temp_dir("none");
    m.save(&dir).unwrap();
    let text = std::fs::read_to_string(dir.join("map.ron")).unwrap();
    assert!(!text.contains("terrain"), "{text}");
    assert!(!dir.join("terrain.png").exists());
    let back = Map::load(&dir).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(back, m);
}

#[test]
fn a_layer_survives_save_and_load() {
    // 22 x 14 m = 6 x 4 cells (a side that is not a whole number of cells).
    let map = generate(22, 14, 3);
    assert_eq!(map.size_in_cells(), (6, 4));
    let dir = temp_dir("roundtrip");
    map.save(&dir).unwrap();
    let text = std::fs::read_to_string(dir.join("map.ron")).unwrap();
    assert!(
        text.contains("terrain_map: Some(\"terrain.png\")"),
        "{text}"
    );
    let back = Map::load(&dir).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(back, map);
    assert_eq!(back.terrain().unwrap().ids().len(), 24);
}

#[test]
fn save_terrain_layer_changes_only_the_layer_and_the_header() {
    let dir = temp_dir("layer_only");
    let bare =
        Map::from_parts(8, 8, 2.0, vec!["a".into()], (0..81).collect(), vec![0; 64]).unwrap();
    bare.save(&dir).unwrap();
    let height = std::fs::read(dir.join("height.png")).unwrap();
    let materials = std::fs::read(dir.join("materials.png")).unwrap();
    let layer = TerrainLayer::new(2, 2, vec![0, 1, 2, 3]).unwrap();
    let with = bare
        .clone()
        .with_terrain(layer, &TerrainTypes::embedded())
        .unwrap();
    with.save_terrain_layer(&dir).unwrap();
    // Twice is the same as once.
    with.save_terrain_layer(&dir).unwrap();
    assert_eq!(std::fs::read(dir.join("height.png")).unwrap(), height);
    assert_eq!(std::fs::read(dir.join("materials.png")).unwrap(), materials);
    let back = Map::load(&dir).unwrap();
    // A map without a layer has nothing to write.
    bare.save_terrain_layer(&dir).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(back, with);
}

// --- validation ---

#[test]
fn a_layer_of_the_wrong_size_is_refused_with_the_expected_size() {
    // 4 x 3 m is 1 x 1 cell; a 2 x 1 image does not fit.
    let err = Map::parse_with(
        &header_with_terrain("t.png"),
        &height_png(5, 4),
        &cells_png(4, 3, 0),
        Some(&terrain_png(2, 1, 0)),
        &TerrainTypes::embedded(),
    )
    .unwrap_err();
    let text = err.to_string();
    assert!(matches!(err, MapError::Image { .. }), "{text}");
    assert!(
        text.contains("t.png") && text.contains("1x1") && text.contains("2x1"),
        "{text}"
    );
    // The same through the in-memory route.
    let map = Map::parse(&header(8, 8), &height_png(9, 9), &cells_png(8, 8, 0)).unwrap();
    let layer = TerrainLayer::new(3, 2, vec![0; 6]).unwrap();
    let err = map
        .with_terrain(layer, &TerrainTypes::embedded())
        .unwrap_err();
    assert!(
        matches!(err, MapError::Image { .. }) && err.to_string().contains("2x2"),
        "{err}"
    );
}

#[test]
fn a_cell_with_an_unknown_terrain_id_is_refused_with_its_position() {
    let text = "(width: 8, height: 8, height_scale: 4.0, materials: [\"a\"], \
                heightmap: \"h.png\", material_map: \"m.png\", terrain_map: Some(\"t.png\"))";
    let t = encode_gray("t.png", &[0, 1, 2, 9], png::BitDepth::Eight, (2, 2)).unwrap();
    let err = Map::parse_with(
        text,
        &height_png(9, 9),
        &cells_png(8, 8, 0),
        Some(&t),
        &TerrainTypes::embedded(),
    )
    .unwrap_err();
    assert!(
        matches!(
            err,
            MapError::TerrainId {
                cell: (1, 1),
                id: 9
            }
        ),
        "{err:?}"
    );
    assert!(err.to_string().contains("(1, 1)") && err.to_string().contains("id 9"));
    // `Map::parse` against a header without a layer never sees it; the same
    // ids are fine with a table that defines id 9.
    let mut types = TerrainTypes::embedded().iter().cloned().collect::<Vec<_>>();
    types.push(TerrainType {
        id: 9,
        name: "lava".into(),
        buildable: false,
        harvestable: false,
        minimap_colour: [200, 40, 20],
    });
    let custom = TerrainTypes::new(types).unwrap();
    let ok = Map::parse_with(
        text,
        &height_png(9, 9),
        &cells_png(8, 8, 0),
        Some(&t),
        &custom,
    )
    .unwrap();
    assert_eq!(ok.terrain().unwrap().get(1, 1), Some(9));
}

#[test]
fn a_layer_must_be_an_8_bit_grayscale_png() {
    let sixteen = encode_gray("t.png", &[0; 2], png::BitDepth::Sixteen, (1, 1)).unwrap();
    let err = Map::parse_with(
        &header_with_terrain("t.png"),
        &height_png(5, 4),
        &cells_png(4, 3, 0),
        Some(&sixteen),
        &TerrainTypes::embedded(),
    )
    .unwrap_err();
    assert!(
        matches!(err, MapError::Image { .. }) && err.to_string().contains("8-bit"),
        "{err}"
    );
    let err = Map::parse_with(
        &header_with_terrain("t.png"),
        &height_png(5, 4),
        &cells_png(4, 3, 0),
        Some(b"not a png"),
        &TerrainTypes::embedded(),
    )
    .unwrap_err();
    assert!(matches!(err, MapError::Png { .. }), "{err}");
}

/// A 2 x 2 PNG in the given colour type and bit depth, with a palette when
/// `indexed`. `rows` are the packed sample bytes of the whole image (two rows
/// of one byte each, since two samples of at most 4 bits fit in one byte).
fn small_png(color: png::ColorType, depth: png::BitDepth, rows: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 2, 2);
    encoder.set_color(color);
    encoder.set_depth(depth);
    if color == png::ColorType::Indexed {
        // Six grey entries, one per terrain id; the palette makes the file a
        // picture a paint program would happily save, which is the trap.
        let palette: Vec<u8> = (0..6u8).flat_map(|i| [i * 40, i * 40, i * 40]).collect();
        encoder.set_palette(palette);
    }
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(rows).unwrap();
    writer.finish().unwrap();
    out
}

/// A layer a paint program saved with a palette (indexed colour) or with fewer
/// than 8 bits is refused, not misread: the sample value would be a palette
/// index or a packed pair of cells, both of which look like valid terrain ids.
#[test]
fn a_paletted_or_4_bit_layer_is_refused_not_misread() {
    // 2 x 2 pixels at 4 bits: one byte per row (two samples of 4 bits each).
    let four_bit = small_png(
        png::ColorType::Grayscale,
        png::BitDepth::Four,
        &[0x01, 0x23],
    );
    // 2 x 2 pixels at 8 bits palette index: two bytes per row.
    let paletted = small_png(png::ColorType::Indexed, png::BitDepth::Eight, &[0, 1, 2, 3]);
    // Same palette picture at 4 bits, the form most paint programs write.
    let paletted_4 = small_png(png::ColorType::Indexed, png::BitDepth::Four, &[0x01, 0x23]);
    // The same four cells as a plain 8-bit grayscale PNG load: the refusals
    // below are about the file kind, not the size or the ids.
    let plain = encode_gray("t.png", &[0, 1, 2, 3], png::BitDepth::Eight, (2, 2)).unwrap();
    assert!(parse_two_by_two(&plain).is_ok());
    for (what, bytes, expected) in [
        ("4-bit grayscale", four_bit, "got 4-bit"),
        ("paletted 8-bit", paletted, "Indexed"),
        ("paletted 4-bit", paletted_4, "Indexed"),
    ] {
        let err = parse_two_by_two(&bytes).unwrap_err();
        let text = err.to_string();
        assert!(
            matches!(err, MapError::Image { .. })
                && text.contains("8-bit grayscale PNG")
                && text.contains(expected)
                && text.contains("t.png"),
            "{what}: {text}"
        );
    }
}

/// An 8 x 8 m map (2 x 2 cells) with `terrain` as its `t.png`.
fn parse_two_by_two(terrain: &[u8]) -> Result<Map, MapError> {
    let text = "(width: 8, height: 8, height_scale: 4.0, materials: [\"a\"], \
                heightmap: \"h.png\", material_map: \"m.png\", terrain_map: Some(\"t.png\"))";
    Map::parse_with(
        text,
        &height_png(9, 9),
        &cells_png(8, 8, 0),
        Some(terrain),
        &TerrainTypes::embedded(),
    )
}

#[test]
fn the_header_and_the_given_bytes_must_agree_about_the_layer() {
    // Named but not given: `parse` cannot read files.
    let err = Map::parse(
        &header_with_terrain("t.png"),
        &height_png(5, 4),
        &cells_png(4, 3, 0),
    )
    .unwrap_err();
    assert!(matches!(err, MapError::Header(_)), "{err}");
    assert!(err.to_string().contains("t.png"), "{err}");
    // Given but not named.
    let err = Map::parse_with(
        &header(4, 3),
        &height_png(5, 4),
        &cells_png(4, 3, 0),
        Some(&terrain_png(1, 1, 0)),
        &TerrainTypes::embedded(),
    )
    .unwrap_err();
    assert!(matches!(err, MapError::Header(_)), "{err}");
}

#[test]
fn load_reads_the_layer_and_checks_it_against_the_shipped_table() {
    let dir = temp_dir("load");
    std::fs::write(dir.join("height.png"), height_png(5, 4)).unwrap();
    std::fs::write(dir.join("materials.png"), cells_png(4, 3, 0)).unwrap();
    std::fs::write(dir.join("map.ron"), header_with_terrain("t.png")).unwrap();
    // Missing file: an I/O error that names it.
    let err = Map::load(&dir).unwrap_err();
    assert!(
        matches!(err, MapError::Io { .. }) && err.to_string().contains("t.png"),
        "{err}"
    );
    std::fs::write(dir.join("t.png"), terrain_png(1, 1, 4)).unwrap();
    let ok = Map::load(&dir).unwrap();
    assert_eq!(ok.terrain().unwrap().get(0, 0), Some(4));
    std::fs::write(dir.join("t.png"), terrain_png(1, 1, 200)).unwrap();
    let err = Map::load(&dir).unwrap_err();
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        matches!(err, MapError::TerrainId { id: 200, .. }),
        "{err:?}"
    );
}

// --- file name safety: the same rules as the other two names ---

#[test]
fn the_terrain_file_name_follows_the_same_safety_rules() {
    for (name, word) in ESCAPING_NAMES {
        let err = Map::parse_with(
            &header_with_terrain(name),
            &height_png(5, 4),
            &cells_png(4, 3, 0),
            Some(&terrain_png(1, 1, 0)),
            &TerrainTypes::embedded(),
        )
        .unwrap_err();
        let message = err.to_string();
        assert!(matches!(err, MapError::Header(_)), "{name}: {message}");
        assert!(
            message.contains("terrain_map") && message.contains(word) && message.contains(name),
            "{name:?}: {message}"
        );
    }
    for name in ["", "CON", "nul.png", "sub/aux", "a\0b.png"] {
        let err = Map::parse_with(
            &header_with_terrain(name),
            &height_png(5, 4),
            &cells_png(4, 3, 0),
            Some(&terrain_png(1, 1, 0)),
            &TerrainTypes::embedded(),
        )
        .unwrap_err();
        assert!(
            matches!(err, MapError::Header(_)) && err.to_string().contains("terrain_map"),
            "{name:?}: {err}"
        );
    }
    // Plain and sub-directory names are fine.
    for name in ["terrain.png", "sub/terrain.png", "./t.png"] {
        let ok = Map::parse_with(
            &header_with_terrain(name),
            &height_png(5, 4),
            &cells_png(4, 3, 0),
            Some(&terrain_png(1, 1, 0)),
            &TerrainTypes::embedded(),
        );
        assert!(ok.is_ok(), "{name}: {:?}", ok.err());
    }
}

/// `load` must refuse an escaping terrain name before it reads the file.
#[test]
fn load_refuses_an_escaping_terrain_name_before_reading_it() {
    let base = temp_dir("escape");
    let dir = base.join("map");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(base.join("outside.png"), terrain_png(1, 1, 0)).unwrap();
    std::fs::write(dir.join("height.png"), height_png(5, 4)).unwrap();
    std::fs::write(dir.join("materials.png"), cells_png(4, 3, 0)).unwrap();
    for name in ["../outside.png", "C:\\x", "\\\\server\\x", "CON"] {
        std::fs::write(dir.join("map.ron"), header_with_terrain(name)).unwrap();
        let err = Map::load(&dir).unwrap_err();
        assert!(matches!(err, MapError::Header(_)), "{name}: {err}");
    }
    std::fs::remove_dir_all(&base).ok();
}

/// A terrain layer that is a symlink leading out of the map directory is
/// refused, like the other files.
#[cfg(unix)]
#[test]
fn load_refuses_a_terrain_symlink_that_leads_outside() {
    use std::os::unix::fs::symlink;
    let base = temp_dir("link");
    let dir = base.join("map");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(base.join("secret.png"), terrain_png(1, 1, 0)).unwrap();
    std::fs::write(dir.join("height.png"), height_png(5, 4)).unwrap();
    std::fs::write(dir.join("materials.png"), cells_png(4, 3, 0)).unwrap();
    symlink(base.join("secret.png"), dir.join("t.png")).unwrap();
    std::fs::write(dir.join("map.ron"), header_with_terrain("t.png")).unwrap();
    let err = Map::load(&dir).unwrap_err();
    std::fs::remove_dir_all(&base).ok();
    assert!(
        matches!(err, MapError::Header(_)) && err.to_string().contains("outside the map"),
        "{err}"
    );
}

// --- committed maps ---

#[test]
fn the_committed_maps_carry_their_terrain_layer() {
    let maps = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps");
    let test64 = Map::load(&maps.join("test64")).unwrap();
    let layer = test64.terrain().expect("test64 has a terrain layer");
    assert_eq!((layer.width(), layer.height()), (16, 16));
    // The layer is what the generator derives from the committed materials.
    assert_eq!(layer, &terrain_for(&test64, 1));
    let h = layer.histogram();
    assert!(h[0] > 0 && h[1] > 0, "grass and road: {:?}", &h[..6]);

    // The playfield of the headless scenario: the generator's layer for the
    // 256 x 256 cell map of seed 1, on a flat map.
    let field = Map::load(&maps.join("playfield256")).unwrap();
    assert_eq!(field.size_in_cells(), (256, 256));
    assert_eq!(
        field.terrain().unwrap(),
        generate_cells(256, 256, 1).terrain().unwrap()
    );
}

// --- the generator's layer ---

/// Whether at least half of the samples of gameplay cell `(cx, cz)` have the
/// named material (the rule the generator uses).
fn mostly(map: &Map, name: &str, cx: u32, cz: u32) -> bool {
    let index = map.materials().iter().position(|m| m == name).unwrap() as u8;
    let (mut n, mut hits) = (0_u32, 0_u32);
    for z in cz * 4..((cz + 1) * 4).min(map.height()) {
        for x in cx * 4..((cx + 1) * 4).min(map.width()) {
            n += 1;
            hits += u32::from(map.material_at(i64::from(x), i64::from(z)) == index);
        }
    }
    hits > 0 && hits * 2 >= n
}

fn ids() -> (u8, u8, u8, u8, u8, u8) {
    let t = TerrainTypes::embedded();
    let id = |n: &str| t.id_of(n).unwrap();
    (
        id("grass"),
        id("road"),
        id("water"),
        id("rock"),
        id("ore"),
        id("bridge"),
    )
}

#[test]
fn the_generated_layer_is_deterministic_and_depends_on_the_seed() {
    let a = generate_cells(32, 24, 4);
    assert_eq!(a.terrain(), generate_cells(32, 24, 4).terrain());
    // Another seed moves the river, the hills and the ore.
    assert_ne!(a.terrain(), generate_cells(32, 24, 5).terrain());
    let (cw, ch) = a.size_in_cells();
    assert_eq!((cw, ch), (32, 24));
    assert_eq!(
        (a.terrain().unwrap().width(), a.terrain().unwrap().height()),
        (32, 24)
    );
}

#[test]
fn the_generated_layer_has_every_type_and_only_known_ids() {
    let (grass, road, water, rock, ore, bridge) = ids();
    for (cells, seed) in [(64, 1), (64, 7), (256, 1), (128, 3)] {
        let map = generate_cells(cells, cells, seed);
        let h = map.terrain().unwrap().histogram();
        for (name, id) in [
            ("grass", grass),
            ("road", road),
            ("water", water),
            ("rock", rock),
            ("ore", ore),
            ("bridge", bridge),
        ] {
            assert!(
                h[usize::from(id)] > 0,
                "{cells} cells seed {seed}: no {name}"
            );
        }
        let known: u32 = (0..6).map(|i| h[i]).sum();
        assert_eq!(known, cells * cells, "{cells} seed {seed}: unknown ids");
        // Grass is the bulk; water and ore are a minority.
        assert!(h[usize::from(grass)] > cells * cells / 2);
        assert!(h[usize::from(water)] < cells * cells / 8);
    }
}

#[test]
fn water_and_rock_follow_the_materials_and_ore_sits_on_grass() {
    let (_, _, water, rock, ore, bridge) = ids();
    let map = generate_cells(64, 64, 1);
    let layer = map.terrain().unwrap();
    for cz in 0..64 {
        for cx in 0..64 {
            let id = layer.get(cx, cz).unwrap();
            let is_water = mostly(&map, "sand", cx, cz);
            let is_rock = !is_water && mostly(&map, "rock", cx, cz);
            if id == water || id == bridge {
                assert!(is_water, "({cx}, {cz}) is {id} but the river is not drawn");
            }
            if id == rock {
                assert!(is_rock, "({cx}, {cz}) is rock but the rock material is not");
            }
            if id == ore {
                assert!(
                    !is_water && !is_rock,
                    "ore at ({cx}, {cz}) on water or rock"
                );
            }
            // The river is water unless a road crosses it.
            if is_water {
                assert!(id == water || id == bridge, "({cx}, {cz}) is {id}");
            }
        }
    }
}

#[test]
fn the_road_crosses_the_map_and_bridges_the_river() {
    let (_, road, water, _, _, bridge) = ids();
    for (cells, seed) in [(64, 1), (64, 7), (256, 1), (256, 5), (40, 9)] {
        let map = generate_cells(cells, cells, seed);
        let layer = map.terrain().unwrap();
        let is_road =
            |x: u32, z: u32| matches!(layer.get(x, z), Some(i) if i == road || i == bridge);
        // Joined by sides from the west edge to the east edge.
        for x in 0..cells {
            assert!(
                (0..cells).any(|z| is_road(x, z)),
                "{cells} seed {seed}: no road in column {x}"
            );
        }
        for x in 0..cells - 1 {
            assert!(
                (0..cells).any(|z| is_road(x, z) && is_road(x + 1, z)),
                "{cells} seed {seed}: the road breaks between columns {x} and {}",
                x + 1
            );
        }
        // It crosses the river, so it has a bridge, and no road cell lies on
        // water (the bridge is what a road on water is).
        let h = layer.histogram();
        assert!(h[usize::from(bridge)] > 0, "{cells} seed {seed}: no bridge");
        for cz in 0..cells {
            for cx in 0..cells {
                if layer.get(cx, cz) == Some(road) {
                    assert!(
                        !mostly(&map, "sand", cx, cz),
                        "{cells} seed {seed}: road on water at ({cx}, {cz})"
                    );
                }
            }
        }
        // The water that is left is in the river's cells.
        assert!(h[usize::from(water)] > 0);
    }
}

#[test]
fn a_tiny_map_has_a_layer_and_no_road() {
    let (_, road, _, _, _, bridge) = ids();
    // 8 x 8 m is 2 x 2 cells: too small for the road, but not for a layer.
    let map = generate(8, 8, 1);
    let layer = map.terrain().unwrap();
    assert_eq!((layer.width(), layer.height()), (2, 2));
    let h = layer.histogram();
    assert_eq!(h[usize::from(road)] + h[usize::from(bridge)], 0);
    // A 1 x 1 m map is one partial cell.
    let one = generate(1, 1, 1);
    assert_eq!(one.terrain().unwrap().ids().len(), 1);
}

#[test]
fn a_map_without_the_generators_materials_has_neither_water_nor_rock() {
    // The layer reads the materials by name: another palette has no river.
    let bare = Map::from_parts(
        16,
        16,
        1.0,
        vec!["x".into(), "y".into()],
        vec![0; 17 * 17],
        vec![1; 256],
    )
    .unwrap();
    let (_, _, water, rock, _, bridge) = ids();
    let layer = terrain_for(&bare, 1);
    let h = layer.histogram();
    assert_eq!(
        h[usize::from(water)] + h[usize::from(rock)] + h[usize::from(bridge)],
        0
    );
}
