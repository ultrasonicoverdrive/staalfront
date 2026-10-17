use std::path::PathBuf;

use super::generate::{generate, generate_cells};
use super::*;

fn header(w: u32, h: u32) -> String {
    format!(
        "(width: {w}, height: {h}, height_scale: 4.0, materials: [\"a\", \"b\"], \
         heightmap: \"height.png\", material_map: \"materials.png\")"
    )
}

fn height_png(w: u32, h: u32) -> Vec<u8> {
    let data: Vec<u8> = (0..w * h)
        .flat_map(|i| ((i * 97) as u16).to_be_bytes())
        .collect();
    encode_gray("height.png", &data, png::BitDepth::Sixteen, (w, h)).unwrap()
}

fn cells_png(w: u32, h: u32, value: u8) -> Vec<u8> {
    let data = vec![value; (w * h) as usize];
    encode_gray("materials.png", &data, png::BitDepth::Eight, (w, h)).unwrap()
}

#[test]
fn parses_a_valid_map() {
    let m = Map::parse(&header(4, 3), &height_png(5, 4), &cells_png(4, 3, 1)).unwrap();
    assert_eq!((m.width(), m.height()), (4, 3));
    assert_eq!(m.heights().len(), 20);
    assert_eq!(m.cells().len(), 12);
    assert_eq!(m.materials(), ["a", "b"]);
    assert_eq!(m.raw_height(1, 0), 97);
    assert_eq!(m.material_at(3, 2), 1);
    // Out-of-range lookups clamp instead of panicking.
    assert_eq!(m.raw_height(-5, 99), m.raw_height(0, 3));
}

#[test]
fn rejects_a_heightmap_of_the_wrong_size() {
    // A w x h heightmap instead of (w+1) x (h+1).
    let err = Map::parse(&header(4, 3), &height_png(4, 3), &cells_png(4, 3, 0)).unwrap_err();
    let text = err.to_string();
    assert!(matches!(err, MapError::Image { .. }), "{text}");
    assert!(
        text.contains("height.png") && text.contains("5x4"),
        "{text}"
    );
}

#[test]
fn rejects_a_material_map_of_the_wrong_size() {
    let err = Map::parse(&header(4, 3), &height_png(5, 4), &cells_png(3, 3, 0)).unwrap_err();
    assert!(err.to_string().contains("materials.png"), "{err}");
}

#[test]
fn rejects_a_material_index_out_of_range() {
    let err = Map::parse(&header(4, 3), &height_png(5, 4), &cells_png(4, 3, 2)).unwrap_err();
    assert!(
        matches!(
            err,
            MapError::MaterialIndex {
                cell: (0, 0),
                index: 2,
                materials: 2
            }
        ),
        "{err}"
    );
    assert!(err.to_string().contains("material 2"), "{err}");
}

#[test]
fn rejects_a_wrong_bit_depth() {
    let eight = encode_gray("h", &[0; 20], png::BitDepth::Eight, (5, 4)).unwrap();
    let err = Map::parse(&header(4, 3), &eight, &cells_png(4, 3, 0)).unwrap_err();
    assert!(err.to_string().contains("Sixteen"), "{err}");
}

#[test]
fn rejects_bad_headers() {
    let cases = [
        (header(0, 3), "width and height"),
        (header(2000, 3), "width and height"),
        (header(4, 3).replace("4.0", "-1.0"), "height_scale"),
        (
            header(4, 3).replace("[\"a\", \"b\"]", "[]"),
            "material list",
        ),
        (header(4, 3).replace("\"b\"", "\"a\""), "twice"),
        (
            header(4, 3).replace("(width", "(depth: 1, width"),
            "map.ron",
        ),
        ("(width: 4".to_string(), "map.ron"),
    ];
    for (text, needle) in cases {
        let err = Map::parse(&text, &height_png(5, 4), &cells_png(4, 3, 0)).unwrap_err();
        assert!(err.to_string().contains(needle), "{text}: {err}");
    }
}

#[test]
fn rejects_garbage_png_and_missing_files() {
    let err = Map::parse(&header(4, 3), b"not a png", &cells_png(4, 3, 0)).unwrap_err();
    assert!(matches!(err, MapError::Png { .. }), "{err}");
    let err = Map::load(&PathBuf::from("/definitely/not/a/map")).unwrap_err();
    assert!(matches!(err, MapError::Io { .. }), "{err}");
}

#[test]
fn from_parts_checks_lengths() {
    let names = vec!["a".to_string()];
    assert!(Map::from_parts(2, 2, 1.0, names.clone(), vec![0; 8], vec![0; 4]).is_err());
    assert!(Map::from_parts(2, 2, 1.0, names.clone(), vec![0; 9], vec![0; 3]).is_err());
    assert!(Map::from_parts(2, 2, 1.0, names, vec![0; 9], vec![0; 4]).is_ok());
}

#[test]
fn save_and_load_round_trip() {
    let map = generate(40, 24, 7);
    let dir = std::env::temp_dir().join(format!("sf_data_map_{}", std::process::id()));
    map.save(&dir).unwrap();
    let back = Map::load(&dir).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(map, back);
}

#[test]
fn height_at_interpolates_bilinearly() {
    let heights = vec![0, 65535, 0, 65535, 0, 0, 0, 0, 0];
    let m = Map::from_parts(2, 2, 10.0, vec!["a".into()], heights, vec![0; 4]).unwrap();
    assert!((m.height_at(1.0, 0.0) - 10.0).abs() < 1e-4);
    assert!((m.height_at(0.5, 0.0) - 5.0).abs() < 1e-4);
    assert!((m.height_at(0.0, 0.0)).abs() < 1e-4);
    assert!((m.height_at(1.0, 0.5) - 5.0).abs() < 1e-4);
}

#[test]
fn generator_is_deterministic_and_varied() {
    let a = generate(128, 96, 3);
    assert_eq!(a, generate(128, 96, 3));
    assert_ne!(a, generate(128, 96, 4));
    let mut used = [0usize; 5];
    for &c in a.cells() {
        used[usize::from(c)] += 1;
    }
    assert!(
        used.iter().all(|&n| n > 0),
        "every material appears: {used:?}"
    );
    let (lo, hi) = a
        .heights()
        .iter()
        .fold((u16::MAX, 0), |(lo, hi), &h| (lo.min(h), hi.max(h)));
    assert!(
        f32::from(hi - lo) / 65535.0 * a.height_scale() > 3.0,
        "relief {lo}..{hi}"
    );
}

#[test]
fn committed_test_map_loads() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps/test64");
    let m = Map::load(&dir).unwrap();
    assert_eq!((m.width(), m.height()), (64, 64));
    let mut seen = [false; 8];
    for &c in m.cells() {
        seen[usize::from(c)] = true;
    }
    assert!(seen.iter().filter(|&&s| s).count() >= 4, "{seen:?}");
}

/// Guards against a flat-looking landscape: on small and large maps a good
/// part of the terrain must be visibly sloped.
#[test]
fn generated_maps_have_real_slopes() {
    for side in [64_u32, 256] {
        let m = generate(side, side, 1);
        let mut slopes: Vec<f32> = Vec::new();
        for z in 0..i64::from(side) {
            for x in 0..i64::from(side) {
                let dx = m.vertex_height(x + 1, z) - m.vertex_height(x, z);
                let dz = m.vertex_height(x, z + 1) - m.vertex_height(x, z);
                slopes.push(dx.hypot(dz));
            }
        }
        slopes.sort_by(f32::total_cmp);
        let p90 = slopes[slopes.len() * 9 / 10];
        assert!(p90 > 0.25, "{side}x{side}: 90th percentile slope {p90}");
    }
}

#[test]
fn a_cell_is_four_terrain_cells_per_side() {
    // B12: 256 x 256 gameplay cells are a 1,024 m map with 4 samples a side.
    let map = generate_cells(16, 8, 1);
    assert_eq!((map.width(), map.height()), (64, 32));
    assert_eq!(map.size_in_cells(), (16, 8));
    assert_eq!(map.heights().len(), 65 * 33);
    // A side that is not a whole number of cells counts the partial cell.
    assert_eq!(generate(65, 63, 1).size_in_cells(), (17, 16));
    // The generator is the same one: cells are only a unit conversion.
    assert_eq!(generate_cells(16, 8, 1), generate(64, 32, 1));
}
