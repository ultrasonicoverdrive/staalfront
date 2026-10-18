use super::*;

use std::path::PathBuf;

fn assets_icons() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons")
}

/// 64 x 64 RGBA from a function of the texel.
fn image(f: impl Fn(usize, usize) -> [u8; 4]) -> Vec<u8> {
    (0..ICON_SIDE * ICON_SIDE)
        .flat_map(|t| f(t % ICON_SIDE, t / ICON_SIDE))
        .collect()
}

fn png_bytes(side: u32, pixels: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut e = png::Encoder::new(&mut bytes, side, side);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut w = e.write_header().unwrap();
        w.write_image_data(pixels).unwrap();
        w.finish().unwrap();
    }
    bytes
}

const WHITE: [u8; 4] = [255, 255, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];
const CLEAR: [u8; 4] = [255, 255, 255, 0];

fn square(lo: usize, hi: usize) -> Vec<u8> {
    image(|x, y| {
        if (lo..hi).contains(&x) && (lo..hi).contains(&y) {
            WHITE
        } else {
            CLEAR
        }
    })
}

fn at(layer: &[u8], x: usize, y: usize, field: usize) -> f32 {
    decode_distance(layer[(y * ICON_SIDE + x) * 2 + field])
}

// ---- the distance transform ----------------------------------------------

/// The transform against the definition, on a mask with no pattern to it.
#[test]
fn the_distance_transform_is_exact() {
    let m = 23;
    // A fixed pseudo-random mask (no rand dependency): an LCG.
    let mut s = 12345_u32;
    let mut next = || {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        s >> 16
    };
    for density in [3, 20, 90] {
        let mask: Vec<bool> = (0..m * m).map(|_| next() % 100 < density).collect();
        if !mask.iter().any(|&b| b) {
            continue;
        }
        let fast = distance_to(&mask, m);
        for y in 0..m {
            for x in 0..m {
                let brute = (0..m * m)
                    .filter(|&t| mask[t])
                    .map(|t| {
                        let (dx, dy) = ((t % m) as f32 - x as f32, (t / m) as f32 - y as f32);
                        (dx * dx + dy * dy).sqrt()
                    })
                    .fold(f32::MAX, f32::min);
                assert!(
                    (fast[y * m + x] - brute).abs() < 1e-4,
                    "density {density} at {x},{y}: {} vs {brute}",
                    fast[y * m + x]
                );
            }
        }
    }
    // No feature at all: far, not a number that breaks the encoding.
    assert!(distance_to(&[false; 9], 3).iter().all(|&d| d >= 1000.0));
}

#[test]
fn a_square_has_the_distance_to_its_edge_in_both_directions() {
    let layer = sdf_layer(&square(16, 48)).unwrap();
    // The square covers texels 16 to 47: its left edge is the line x = 16 in
    // texel units, and a texel centre is at x + 0.5.
    for (x, y, want) in [
        (16, 31, -0.5), // the first inside texel on the left side
        (15, 31, 0.5),  // the first outside texel
        (31, 31, -15.5),
        (10, 31, 5.5),
        (31, 3, 12.5),
        (0, 0, SDF_SPREAD), // far outside: the spread, not more
    ] {
        let got = at(&layer, x, y, 0);
        // One byte step is 32 / 255 texels.
        assert!((got - want).abs() < 0.13, "{x},{y}: {got}, want {want}");
    }
    // No sign: the second field is outside everywhere.
    assert!((0..ICON_SIDE * ICON_SIDE).all(|t| at(&layer, t % 64, t / 64, 1) > 1.0));
}

#[test]
fn a_black_sign_and_a_cut_out_sign_are_the_same_sign() {
    let blob = |x: usize, y: usize| (28..36).contains(&x) && (28..36).contains(&y);
    let black = image(|x, y| match (x, y) {
        _ if blob(x, y) => BLACK,
        (16..48, 16..48) => WHITE,
        _ => CLEAR,
    });
    let cut = image(|x, y| match (x, y) {
        _ if blob(x, y) => CLEAR,
        (16..48, 16..48) => WHITE,
        _ => CLEAR,
    });
    let (a, b) = (sdf_layer(&black).unwrap(), sdf_layer(&cut).unwrap());
    assert_eq!(a, b, "the engine reads both the same way");
    // The sign is inside, and the silhouette includes the hole.
    assert!(at(&a, 31, 31, 1) < -3.0);
    assert!(at(&a, 31, 31, 0) < -10.0);
    assert!(at(&a, 20, 20, 1) > 3.0, "outside the sign");
}

#[test]
fn a_cut_out_that_reaches_the_border_is_outside_the_shape() {
    // A channel from the left edge of the image into the square (row 31,
    // texels 0 to 29) is background, not a sign.
    let img = image(|x, y| {
        let in_square = (16..48).contains(&x) && (16..48).contains(&y);
        if in_square && !(y == 31 && x < 30) {
            WHITE
        } else {
            CLEAR
        }
    });
    let layer = sdf_layer(&img).unwrap();
    assert!(
        at(&layer, 5, 31, 0) > 0.0,
        "the notch outside the square is outside"
    );
    assert!(
        at(&layer, 20, 31, 0) > 0.0,
        "and so is the channel inside it"
    );
    assert!(at(&layer, 20, 31, 1) > 0.0, "it is not a sign either");
}

#[test]
fn the_border_of_the_image_counts_as_outside() {
    // A fully opaque image: the silhouette fills the box and its edge is the
    // border of the image, half a texel beyond the outermost texel centres.
    let layer = sdf_layer(&image(|_, _| WHITE)).unwrap();
    assert!((at(&layer, 0, 31, 0) + 0.5).abs() < 0.13);
    assert!((at(&layer, 31, 31, 0) - decode_distance(encode_distance(-SDF_SPREAD))).abs() < 0.13);
}

#[test]
fn distances_round_trip_through_a_byte() {
    for d in [-20.0_f32, -16.0, -3.3, -0.5, 0.0, 0.5, 7.77, 16.0, 40.0] {
        let back = decode_distance(encode_distance(d));
        assert!(
            (back - d.clamp(-SDF_SPREAD, SDF_SPREAD)).abs() <= SDF_SPREAD / 255.0 + 1e-4,
            "{d} -> {back}"
        );
    }
    // The edge is the middle of the range.
    assert!(decode_distance(encode_distance(0.0)).abs() < 0.07);
    assert!(sdf_layer(&[0; 12]).is_none(), "a wrong size is refused");
}

// ---- loading ---------------------------------------------------------------

/// The shipped set: every base shape and every role of the art brief, no
/// warnings, names that share a file share a layer.
#[test]
fn the_shipped_icons_load_into_an_atlas() {
    let atlas = IconAtlas::load(&assets_icons());
    assert!(atlas.warnings().is_empty(), "{:?}", atlas.warnings());
    // 4 plain shapes + 37 role pictures; the decoy shares the defence tower's and
    // the civilian the plain circle (the aliases of icons.ron).
    assert_eq!(atlas.len(), 4 + 37);
    assert_eq!(atlas.lookup("civilian"), atlas.lookup("base_circle"));
    assert!(!atlas.is_empty());
    for (i, base) in BASE_NAMES.iter().enumerate() {
        assert_eq!(atlas.lookup(base), Some(IconId(i as u16)), "{base}");
    }
    for role in [
        "rifleman",
        "light_tank",
        "gunboat",
        "construction_center",
        "archive_building",
        "decoy_emplacement",
    ] {
        assert!(atlas.lookup(role).is_some(), "{role}");
    }
    assert_eq!(
        atlas.lookup("decoy_emplacement"),
        atlas.lookup("defence_tower"),
        "the decoy has the tower's picture"
    );
    assert_ne!(atlas.lookup("rifleman"), atlas.lookup("engineer"));
    // Every layer is a real picture: an inside, an outside, the right size.
    for (name, id) in atlas.names() {
        let layer = atlas.layer(id).unwrap();
        assert_eq!(layer.len(), LAYER_BYTES, "{name}");
        let inside = layer.chunks_exact(2).filter(|t| t[0] > 128).count();
        assert!(
            (1000..3600).contains(&inside),
            "{name}: {inside} texels inside"
        );
        assert!(
            layer.chunks_exact(2).any(|t| t[0] < 128),
            "{name}: nothing outside"
        );
    }
    // Signs exist on the role icons and not on the plain shapes.
    let signed = |name: &str| {
        let layer = atlas.layer(atlas.lookup(name).unwrap()).unwrap();
        layer.chunks_exact(2).filter(|t| t[1] > 128).count()
    };
    assert_eq!(signed("base_square"), 0);
    assert_eq!(signed("civilian"), 0, "the civilian is the empty circle");
    assert!(signed("harvester") > 30);
    assert!(signed("power_plant") > 30);
    assert!(atlas.layer(IconId(4000)).is_none());
}

/// The four plain shapes in the binary are the files on disk.
#[test]
fn the_embedded_base_shapes_are_the_committed_files() {
    let from_disk = IconAtlas::load(&assets_icons());
    let embedded = IconAtlas::base();
    assert_eq!(embedded.len(), 4);
    assert!(embedded.warnings().is_empty());
    for i in 0..4 {
        assert_eq!(
            embedded.layer(IconId(i)),
            from_disk.layer(IconId(i)),
            "{}",
            BASE_NAMES[usize::from(i)]
        );
    }
    // And they are the shapes their names say: the circle is round, the
    // triangle has its apex up, the half-circle its deck up.
    let inside =
        |id: u16, x: usize, y: usize| at(embedded.layer(IconId(id)).unwrap(), x, y, 0) < 0.0;
    assert!(
        inside(0, 8, 8) == inside(0, 55, 55) && inside(0, 8, 8),
        "square corners"
    );
    assert!(!inside(1, 8, 8), "circle corner is outside");
    assert!(inside(1, 32, 32));
    assert!(
        inside(2, 32, 12) && !inside(2, 6, 12),
        "triangle: narrow at the top"
    );
    assert!(inside(2, 10, 50), "and wide at the bottom");
    assert!(
        inside(3, 8, 20) && !inside(3, 8, 42) && inside(3, 32, 42),
        "half-circle: wide at the deck, narrow in the hull below"
    );
    assert!(!inside(3, 32, 8), "and nothing above the deck");
}

/// A role without an icon, or with one the atlas does not have, gets its base
/// shape; a missing or broken file is a warning, never a failure.
#[test]
fn a_missing_icon_falls_back_to_the_base_shape() {
    let atlas = IconAtlas::load(&assets_icons());
    for shape in IconShape::ALL {
        assert_eq!(atlas.resolve(None, shape), IconId::from(shape));
        assert_eq!(
            atlas.resolve(Some("no_such_icon"), shape),
            IconId::from(shape)
        );
        assert_eq!(atlas.resolve(Some(""), shape), IconId::from(shape));
    }
    let tank = atlas.lookup("light_tank").unwrap();
    assert_eq!(atlas.resolve(Some("light_tank"), IconShape::Triangle), tank);
    assert!(tank.0 >= 4, "a role icon is not one of the plain shapes");
    // The name wins over the fallback shape even if the shapes disagree.
    assert_eq!(atlas.resolve(Some("light_tank"), IconShape::Circle), tank);
}

fn entries(list: &[(&str, &str)]) -> BTreeMap<String, String> {
    list.iter()
        .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
        .collect()
}

#[test]
fn broken_icon_files_are_warnings_and_leave_the_name_unknown() {
    let good = png_bytes(64, &square(16, 48));
    let small = png_bytes(32, &vec![255; 32 * 32 * 4]);
    let empty = png_bytes(64, &image(|_, _| CLEAR));
    let read = |file: &str| -> std::io::Result<Vec<u8>> {
        match file {
            "good.png" => Ok(good.clone()),
            "small.png" => Ok(small.clone()),
            "empty.png" => Ok(empty.clone()),
            "junk.png" => Ok(b"not a png".to_vec()),
            _ => Err(not_found()),
        }
    };
    let atlas = IconAtlas::from_entries(
        &entries(&[
            ("good", "good.png"),
            ("alias", "good.png"),
            ("small", "small.png"),
            ("empty", "empty.png"),
            ("junk", "junk.png"),
            ("gone", "gone.png"),
            ("Bad Name", "good.png"),
            ("escape", "../good.png"),
        ]),
        read,
        Vec::new(),
    );
    assert!(atlas.lookup("good").is_some());
    assert_eq!(
        atlas.lookup("good"),
        atlas.lookup("alias"),
        "one file, one layer"
    );
    for missing in ["small", "empty", "junk", "gone", "Bad Name", "escape"] {
        assert!(atlas.lookup(missing).is_none(), "{missing}");
        assert_eq!(
            atlas.resolve(Some(missing), IconShape::Square),
            IconId(0),
            "{missing} is drawn as its base shape"
        );
    }
    // 4 base shapes and the one good picture.
    assert_eq!(atlas.len(), 5);
    let warnings = atlas.warnings().join("\n");
    for needle in [
        "32x32",
        "empty",
        "junk.png",
        "gone.png",
        "Bad Name",
        "plain file name",
    ] {
        assert!(warnings.contains(needle), "{needle} in {warnings}");
    }
}

#[test]
fn an_icon_file_can_replace_a_base_shape() {
    let own = png_bytes(64, &square(8, 56));
    let atlas = IconAtlas::from_entries(
        &entries(&[("base_square", "mine.png")]),
        |_| Ok(own.clone()),
        Vec::new(),
    );
    assert!(atlas.warnings().is_empty());
    assert_eq!(atlas.len(), 4);
    let layer = atlas.layer(IconId(0)).unwrap();
    // A square from 8 to 56: the texel at 10 is inside, the stock square (16 to 48) ends there.
    assert!(at(layer, 10, 31, 0) < 0.0);
    assert_ne!(Some(layer), IconAtlas::base().layer(IconId(0)));
}

#[test]
fn a_missing_directory_gives_the_plain_shapes_and_a_warning() {
    let atlas = IconAtlas::load(&assets_icons().join("no/such/dir"));
    assert_eq!(atlas.len(), 4);
    assert_eq!(atlas.warnings().len(), 1, "{:?}", atlas.warnings());
    assert!(atlas.warnings()[0].contains("icon directory"));
    assert_eq!(
        atlas.resolve(Some("rifleman"), IconShape::Circle),
        IconId::from(IconShape::Circle)
    );
}

#[test]
fn the_atlas_stops_at_the_layer_limit() {
    let good = png_bytes(64, &square(16, 48));
    let mut list = BTreeMap::new();
    // Different files so that none shares a layer.
    for i in 0..(MAX_LAYERS + 5) {
        list.insert(format!("icon_{i:03}"), format!("f{i}.png"));
    }
    let atlas = IconAtlas::from_entries(&list, |_| Ok(good.clone()), Vec::new());
    assert_eq!(atlas.len(), MAX_LAYERS);
    assert!(atlas.warnings().iter().any(|w| w.contains("at most")));
    assert!(atlas.lookup("icon_000").is_some());
    assert!(
        atlas
            .lookup(&format!("icon_{:03}", MAX_LAYERS + 4))
            .is_none()
    );
}

/// The shader reads the fields with the constants of this module; WGSL has no
/// includes, so the numbers are written twice and kept equal here.
#[test]
fn the_shader_uses_the_same_atlas_constants() {
    let shader = include_str!("../../../../assets/shaders/icon.wgsl");
    assert!(shader.contains(&format!("const ATLAS_SIDE: f32 = {ICON_SIDE}.0;")));
    assert!(shader.contains(&format!("const SDF_SPREAD: f32 = {SDF_SPREAD:.1};")));
    assert!(shader.contains(&format!(
        "const LAYER_SHIFT: u32 = {}u;",
        crate::icons::ICON_LAYER_SHIFT
    )));
    assert!(shader.contains(&format!(
        "const HIGHLIGHT_BIT: u32 = 0x{:x}u;",
        crate::icons::ICON_HIGHLIGHT
    )));
    assert!(shader.contains(&format!(
        "const SELECTED_BIT: u32 = 0x{:x}u;",
        crate::icons::ICON_SELECTED
    )));
}

#[test]
fn names_are_checked() {
    assert!(valid_name("light_tank"));
    assert!(valid_name("a1"));
    for bad in [
        "",
        "Light",
        "light-tank",
        "a b",
        "tank.png",
        "../x",
        &"x".repeat(49),
    ] {
        assert!(!valid_name(bad), "{bad:?}");
    }
}
