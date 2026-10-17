use std::path::PathBuf;

use super::generate::{generate, generate_cells};
use super::*;

pub(super) fn header(w: u32, h: u32) -> String {
    format!(
        "(width: {w}, height: {h}, height_scale: 4.0, materials: [\"a\", \"b\"], \
         heightmap: \"height.png\", material_map: \"materials.png\")"
    )
}

pub(super) fn height_png(w: u32, h: u32) -> Vec<u8> {
    let data: Vec<u8> = (0..w * h)
        .flat_map(|i| ((i * 97) as u16).to_be_bytes())
        .collect();
    encode_gray("height.png", &data, png::BitDepth::Sixteen, (w, h)).unwrap()
}

pub(super) fn cells_png(w: u32, h: u32, value: u8) -> Vec<u8> {
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
    let text = err.to_string();
    assert!(
        text.contains("must be a 16-bit grayscale PNG, got 8-bit Grayscale"),
        "{text}"
    );
    // The materials need 8 bits; 16 is refused the other way round.
    let sixteen = height_png(4, 3);
    let err = Map::parse(&header(4, 3), &height_png(5, 4), &sixteen).unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("materials.png")
            && text.contains("must be a 8-bit grayscale PNG, got 16-bit"),
        "{text}"
    );
    // The Debug names of png::BitDepth never reach the message.
    assert!(
        !text.contains("Eight") && !text.contains("Sixteen"),
        "{text}"
    );
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

// --- file names in map.ron ---

/// A header whose two file names are replaced.
pub(super) fn header_with_names(heightmap: &str, material_map: &str) -> String {
    format!(
        "(width: 4, height: 3, height_scale: 4.0, materials: [\"a\", \"b\"], \
         heightmap: {heightmap:?}, material_map: {material_map:?})"
    )
}

/// Names that point outside the map directory, on any OS, with a word of the
/// reason the message must give.
pub(super) const ESCAPING_NAMES: [(&str, &str); 14] = [
    ("/etc/passwd", "absolute"),
    ("\\windows\\system32\\x", "absolute"),
    ("C:\\x", "drive"),
    ("C:/x", "drive"),
    ("d:x.png", "drive"),
    ("c:\\windows\\win.ini", "drive"),
    ("\\\\server\\share\\x", "UNC"),
    ("//server/share/x", "UNC"),
    ("../height.png", ".."),
    ("sub/../../height.png", ".."),
    ("..\\height.png", ".."),
    ("sub\\..\\..\\x", ".."),
    ("height.png:stream", "':'"),
    ("a\\b.png", "backslash"),
];

#[test]
fn parse_refuses_file_names_that_escape_the_map_directory() {
    for (name, word) in ESCAPING_NAMES {
        for (field, text) in [
            ("heightmap", header_with_names(name, "materials.png")),
            ("material_map", header_with_names("height.png", name)),
        ] {
            let err = Map::parse(&text, &height_png(5, 4), &cells_png(4, 3, 0)).unwrap_err();
            let message = err.to_string();
            assert!(matches!(err, MapError::Header(_)), "{name}: {message}");
            assert!(
                message.contains(field) && message.contains(word) && message.contains(name),
                "{field} {name:?}: {message}"
            );
        }
    }
    for (name, why) in [("", "empty"), ("a\0b.png", "NUL")] {
        let text = header_with_names(name, "materials.png");
        let err = Map::parse(&text, &height_png(5, 4), &cells_png(4, 3, 0)).unwrap_err();
        assert!(err.to_string().contains(why), "{name:?}: {err}");
    }
}

#[test]
fn parse_accepts_plain_and_sub_directory_names() {
    for name in [
        "height.png",
        "sub/height.png",
        "./height.png",
        "my..map.png",
    ] {
        let text = header_with_names(name, "materials.png");
        let m = Map::parse(&text, &height_png(5, 4), &cells_png(4, 3, 0));
        assert!(m.is_ok(), "{name}: {:?}", m.err());
    }
}

/// `load` must refuse before it reads: the file the name points at exists,
/// and the error must be the name check, not the content of that file.
#[test]
fn load_refuses_an_escaping_name_before_reading_the_file() {
    let base = std::env::temp_dir().join(format!("sf_data_map_names_{}", std::process::id()));
    let dir = base.join("map");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(base.join("outside.png"), height_png(5, 4)).unwrap();
    std::fs::write(dir.join("materials.png"), cells_png(4, 3, 0)).unwrap();
    let absolute = base.join("outside.png").display().to_string();
    for name in [
        "../outside.png",
        absolute.as_str(),
        "C:\\x",
        "\\\\server\\x",
    ] {
        std::fs::write(
            dir.join("map.ron"),
            header_with_names(name, "materials.png"),
        )
        .unwrap();
        let err = Map::load(&dir).unwrap_err();
        assert!(
            matches!(err, MapError::Header(_)),
            "{name}: {err} (it must not have been read)"
        );
    }
    // The same files with a name inside the directory load.
    std::fs::write(dir.join("height.png"), height_png(5, 4)).unwrap();
    std::fs::write(
        dir.join("map.ron"),
        header_with_names("height.png", "materials.png"),
    )
    .unwrap();
    let loaded = Map::load(&dir);
    std::fs::remove_dir_all(&base).ok();
    assert!(loaded.is_ok(), "{:?}", loaded.err());
}

// --- Windows device names and symlinks ---

/// `CON` and friends are refused on every OS: a map made for Windows must not
/// be able to open a device there, and the same map must fail the same way
/// everywhere.
#[test]
fn parse_refuses_windows_device_names_on_every_os() {
    let refused = [
        "CON",
        "con",
        "Nul",
        "PRN.png",
        "aux.txt",
        "NUL.tar.gz",
        "COM1",
        "com9.png",
        "LPT1",
        "lpt9.png",
        "sub/CON",
        "sub/nul.png",
        "con/height.png",
        "CON.",
        "CON .png",
        "Com5.height.png",
    ];
    for name in refused {
        for (field, text) in [
            ("heightmap", header_with_names(name, "materials.png")),
            ("material_map", header_with_names("height.png", name)),
        ] {
            let err = Map::parse(&text, &height_png(5, 4), &cells_png(4, 3, 0)).unwrap_err();
            let message = err.to_string();
            assert!(matches!(err, MapError::Header(_)), "{name}: {message}");
            assert!(
                message.contains(field) && message.contains("device name"),
                "{field} {name:?}: {message}"
            );
        }
    }
    // Names that only look similar are fine.
    for name in [
        "CONSOLE.png",
        "icon.png",
        "NULL.png",
        "COM0.png",
        "COM10.png",
        "LPT.png",
        "COMX",
        "my.CON",
        "xCON",
    ] {
        let text = header_with_names(name, "materials.png");
        let m = Map::parse(&text, &height_png(5, 4), &cells_png(4, 3, 0));
        assert!(m.is_ok(), "{name}: {:?}", m.err());
    }
}

#[test]
fn load_refuses_a_device_name_without_opening_it() {
    // `CON` would block on a Windows console read; here the name check must
    // fire before any file system access.
    let dir = std::env::temp_dir().join(format!("sf_data_map_con_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("materials.png"), cells_png(4, 3, 0)).unwrap();
    std::fs::write(
        dir.join("map.ron"),
        header_with_names("CON", "materials.png"),
    )
    .unwrap();
    let err = Map::load(&dir).unwrap_err();
    std::fs::remove_dir_all(&dir).ok();
    assert!(matches!(err, MapError::Header(_)), "{err}");
}

/// A symlink inside the map directory that points outside is refused, for a
/// file, for a directory in the middle of the name, and for `map.ron` itself.
#[cfg(unix)]
#[test]
fn load_refuses_a_symlink_that_leads_outside_the_map_directory() {
    use std::os::unix::fs::symlink;
    let base = std::env::temp_dir().join(format!("sf_data_map_links_{}", std::process::id()));
    let dir = base.join("map");
    let outside = base.join("outside");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("height.png"), height_png(5, 4)).unwrap();
    std::fs::write(dir.join("materials.png"), cells_png(4, 3, 0)).unwrap();
    symlink(outside.join("height.png"), dir.join("link.png")).unwrap();
    symlink(&outside, dir.join("linked_dir")).unwrap();

    for name in ["link.png", "linked_dir/height.png"] {
        let header = header_with_names(name, "materials.png");
        std::fs::write(dir.join("map.ron"), header).unwrap();
        let err = Map::load(&dir).unwrap_err();
        assert!(matches!(err, MapError::Header(_)), "{name}: {err}");
        assert!(
            err.to_string().contains("outside the map directory"),
            "{name}: {err}"
        );
    }

    // map.ron itself as a link to a header outside.
    let header = header_with_names("height.png", "materials.png");
    std::fs::write(outside.join("map.ron"), header).unwrap();
    std::fs::remove_file(dir.join("map.ron")).unwrap();
    symlink(outside.join("map.ron"), dir.join("map.ron")).unwrap();
    let err = Map::load(&dir).unwrap_err();
    assert!(
        err.to_string().contains("outside the map directory"),
        "{err}"
    );

    // A symlink that stays inside the directory is fine.
    std::fs::remove_file(dir.join("map.ron")).unwrap();
    std::fs::write(dir.join("real_height.png"), height_png(5, 4)).unwrap();
    symlink(dir.join("real_height.png"), dir.join("inner.png")).unwrap();
    let header = header_with_names("inner.png", "materials.png");
    std::fs::write(dir.join("map.ron"), header).unwrap();
    let loaded = Map::load(&dir);
    std::fs::remove_dir_all(&base).ok();
    assert!(loaded.is_ok(), "{:?}", loaded.err());
}
