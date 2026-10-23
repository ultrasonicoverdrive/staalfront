use sf_data::map::Map;
use sf_data::terrain::{TerrainLayer, TerrainType, TerrainTypes};

use super::*;

fn shipped() -> (TerrainTypes, TerrainLooks) {
    (TerrainTypes::embedded(), TerrainLooks::embedded())
}

/// A flat map of `cw` x `ch` gameplay cells with the given type ids, and a
/// material map that deliberately says "sand" everywhere.
fn map_with(ids: Vec<u8>, cw: u32, ch: u32, types: &TerrainTypes) -> Map {
    let (w, h) = (cw * 4, ch * 4);
    let map = Map::from_parts(
        w,
        h,
        0.0,
        vec!["grass".into(), "sand".into()],
        vec![0; ((w + 1) * (h + 1)) as usize],
        vec![1; (w * h) as usize],
    )
    .expect("map");
    map.with_terrain(TerrainLayer::new(cw, ch, ids).expect("layer"), types)
        .expect("terrain")
}

fn mean_color(texture: &[u8]) -> [f32; 3] {
    let n = (texture.len() / 4) as f32;
    let mut sum = [0.0_f32; 3];
    for p in texture.chunks(4) {
        for c in 0..3 {
            sum[c] += f32::from(p[c]);
        }
    }
    sum.map(|s| s / n)
}

#[test]
fn every_shipped_type_has_a_look_and_every_look_a_type() {
    let (types, looks) = shipped();
    for t in types.iter() {
        assert!(looks.get(&t.name).is_some(), "no look for type {}", t.name);
    }
    for l in &looks.looks {
        assert!(
            types.id_of(&l.terrain).is_some(),
            "look for unknown type {}",
            l.terrain
        );
    }
    assert!(looks.check_names(&types).is_ok());
}

#[test]
fn looks_tile_differ_and_read_apart() {
    let (types, looks) = shipped();
    let textures: Vec<(String, Vec<u8>)> = types
        .iter()
        .map(|t| (t.name.clone(), looks.look_for(&t.name).texture()))
        .collect();
    let n = TEXTURE_SIZE as usize;
    for (name, t) in &textures {
        assert_eq!(t.len(), n * n * 4, "{name}");
        assert!(t.chunks(4).all(|p| p[3] == 255), "{name} is not opaque");
        let distinct: std::collections::BTreeSet<_> =
            t.chunks(4).map(|p| (p[0], p[1], p[2])).collect();
        assert!(distinct.len() > 30, "{name} is flat");
        // Tileable: the step across each wrap is like the typical step
        // (planks and crests have hard steps inside, so allow a generous
        // margin; a seam would be as large as the whole contrast).
        let px = |x: usize, y: usize| i32::from(t[(y * n + x) * 4 + 1]);
        // Mean step between neighbouring pixels, and the step across the wrap.
        let step_x = (0..n)
            .flat_map(|y| (1..n).map(move |x| (x, y)))
            .map(|(x, y)| (px(x, y) - px(x - 1, y)).abs())
            .sum::<i32>() as f32
            / (n * (n - 1)) as f32;
        let step_y = (1..n)
            .flat_map(|y| (0..n).map(move |x| (x, y)))
            .map(|(x, y)| (px(x, y) - px(x, y - 1)).abs())
            .sum::<i32>() as f32
            / (n * (n - 1)) as f32;
        let wrap_x = (0..n).map(|y| (px(0, y) - px(n - 1, y)).abs()).sum::<i32>() as f32 / n as f32;
        let wrap_y = (0..n).map(|x| (px(x, 0) - px(x, n - 1)).abs()).sum::<i32>() as f32 / n as f32;
        assert!(
            wrap_x <= step_x * 2.0 + 3.0,
            "{name}: seam in x ({wrap_x} vs {step_x})"
        );
        assert!(
            wrap_y <= step_y * 2.0 + 3.0,
            "{name}: seam in y ({wrap_y} vs {step_y})"
        );
    }
    // Readable apart: the mean colours of any two types are clearly different.
    for (i, (a, ta)) in textures.iter().enumerate() {
        for (b, tb) in &textures[..i] {
            let (ma, mb) = (mean_color(ta), mean_color(tb));
            let dist =
                ((ma[0] - mb[0]).powi(2) + (ma[1] - mb[1]).powi(2) + (ma[2] - mb[2]).powi(2))
                    .sqrt();
            assert!(dist > 40.0, "{a} and {b} look alike ({dist:.0})");
        }
    }
}

#[test]
fn the_looks_say_what_the_types_are() {
    let (_, looks) = shipped();
    let mean = |n: &str| mean_color(&looks.look_for(n).texture());
    let (grass, road, water, rock, ore, bridge) = (
        mean("grass"),
        mean("road"),
        mean("water"),
        mean("rock"),
        mean("ore"),
        mean("bridge"),
    );
    assert!(grass[1] > grass[0] && grass[1] > grass[2], "grass is green");
    assert!(
        water[2] > water[0] + 40.0 && water[2] > water[1],
        "water is blue"
    );
    assert!(ore[0] > ore[2] + 30.0, "ore is warm");
    assert!(bridge[0] > bridge[2] + 30.0, "a bridge is wood");
    let lum = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    // Packed earth: a light tan, lighter and much warmer than the grey rock.
    assert!(lum(road) > lum(rock), "the road is lighter than the rock");
    assert!(
        road[0] > road[2] + 50.0,
        "the road is warm, the rock is grey"
    );
    assert!(rock[0] < rock[2] + 20.0, "the rock is grey");
    // Water and the bridge deck are the level ones, only water is a liquid.
    assert!(looks.look_for("water").flat && looks.look_for("water").liquid);
    assert!(looks.look_for("bridge").flat && !looks.look_for("bridge").liquid);
    assert!(!looks.look_for("grass").flat && !looks.look_for("rock").flat);
}

#[test]
fn a_map_with_terrain_is_drawn_from_its_types_not_its_material_map() {
    let (types, looks) = shipped();
    // 3 x 2 cells: grass, water, bridge / road, rock, ore. The material map
    // says sand everywhere and must not matter.
    let map = map_with(vec![0, 2, 5, 1, 3, 4], 3, 2, &types);
    let s = terrain_surface(&map, &types, &looks);
    assert_eq!(s.size, (3, 2));
    assert_eq!(s.layers.len(), types.len());
    assert_eq!(s.index.len(), 3 * 2 * 2);
    assert_eq!(s.index_meters, 4.0);
    // Layers are in id order: layer = position in the table.
    let texel = |i: usize| (s.index[2 * i], s.index[2 * i + 1]);
    assert_eq!(texel(0), (0, 0), "grass");
    assert_eq!(texel(1), (2, STYLE_FLAT | STYLE_LIQUID), "water");
    assert_eq!(texel(2), (5, STYLE_FLAT), "bridge");
    assert_eq!(texel(3), (1, 0), "road");
    assert_eq!(texel(4), (3, 0), "rock");
    assert_eq!(texel(5), (4, 0), "ore");
    // Each layer is the look of the type at that position.
    for (i, t) in types.iter().enumerate() {
        assert_eq!(s.layers[i], looks.look_for(&t.name).texture(), "{}", t.name);
    }
    // The ramp is a half-cell wide in total (2 m of 4 m), the wobble 0.7 m.
    assert!((s.ramp - 0.25).abs() < 1e-6);
    assert!((s.wobble - 0.175).abs() < 1e-6);
}

#[test]
fn a_new_type_is_drawn_before_anybody_chose_its_colours() {
    let mut list: Vec<TerrainType> = TerrainTypes::embedded().iter().cloned().collect();
    // A copy of a shipped type under a new id and name, not a struct literal:
    // a new field of TerrainType (the minimap colour of WP2.3) cannot break it.
    let mut lava: TerrainType = list[0].clone();
    lava.id = 9;
    lava.name = "lava".into();
    list.push(lava);
    let types = TerrainTypes::new(list).unwrap();
    let looks = TerrainLooks::embedded();
    assert!(looks.get("lava").is_none());
    let map = map_with(vec![9, 0, 0, 0], 2, 2, &types);
    let s = terrain_surface(&map, &types, &looks);
    assert_eq!(s.layers.len(), 7);
    // id 9 sits at position 6 of the table.
    assert_eq!(s.index[0], 6);
    let lava = looks.look_for("lava");
    assert_eq!(lava, looks.look_for("lava"), "the derived colour is stable");
    assert_ne!(s.layers[6], s.layers[0]);
}

#[test]
fn a_map_without_terrain_keeps_its_material_map() {
    let (types, looks) = shipped();
    let map = Map::from_parts(
        8,
        4,
        0.0,
        vec!["grass".into(), "rock".into()],
        vec![0; 9 * 5],
        (0..32).map(|i| (i % 2) as u8).collect(),
    )
    .unwrap();
    let s = terrain_surface(&map, &types, &looks);
    assert_eq!(s.size, (8, 4), "one texel per 1 m sample");
    assert_eq!(s.index_meters, 1.0);
    assert_eq!(s.layers.len(), 2);
    assert_eq!(s.layers[1], material_texture("rock"));
    assert_eq!(
        &s.index[..6],
        &[0, 0, 1, 0, 0, 0],
        "material of each sample"
    );
    assert_eq!(s.ramp, 0.5, "full ramp, like WP1.7");
    assert_eq!(s.wobble, 0.0);
    assert_eq!(s.shore_amount, 0.0);
}

#[test]
fn a_looks_file_with_a_problem_is_refused() {
    let edge = "edge: (ramp_m: 2.0, wobble_m: 0.5, shore: (1.0, 1.0, 1.0), shore_amount: 0.4)";
    let ok = |looks: &str| TerrainLooks::from_ron(&format!("({edge}, looks: [{looks}])"));
    let grass = "(terrain: \"grass\", color: (1, 2, 3), contrast: 0.2, pattern: Noise)";
    assert!(ok(grass).is_ok());
    assert!(ok("").is_ok(), "no entries: every type gets a derived look");
    assert!(
        ok(&format!("{grass}, {grass}"))
            .unwrap_err()
            .0
            .contains("twice")
    );
    assert!(
        ok("(terrain: \"a\", color: (1, 2, 3), contrast: 2.0, pattern: Noise)")
            .unwrap_err()
            .0
            .contains("contrast")
    );
    assert!(
        ok("(terrain: \"a\", color: (1, 2, 3), contrast: 0.1, pattern: Planks(count: 0))")
            .unwrap_err()
            .0
            .contains("planks")
    );
    assert!(
        TerrainLooks::from_ron(
            "(edge: (ramp_m: 9.0, wobble_m: 0.5, shore: (1.0, 1.0, 1.0), shore_amount: 0.4), looks: [])"
        )
        .unwrap_err()
        .0
        .contains("ramp_m")
    );
    // An unknown field is an error, not ignored.
    assert!(
        ok("(terrain: \"a\", color: (1, 2, 3), contrast: 0.1, pattern: Noise, shiny: true)")
            .is_err()
    );
    assert!(TerrainLooks::from_ron("(looks: [)").is_err());
    assert!(
        TerrainLooks::from_ron(
            "(edge: (ramp_m: 2.0, wobble_m: 0.5, shore: (1.0, 3.0, 1.0), shore_amount: 0.4), looks: [])"
        )
        .unwrap_err()
        .0
        .contains("shore")
    );
    // A misspelt type name parses, and is refused against the table.
    let (types, _) = shipped();
    let misspelt =
        ok("(terrain: \"gras\", color: (1, 2, 3), contrast: 0.2, pattern: Noise)").unwrap();
    assert!(misspelt.check_names(&types).unwrap_err().0.contains("gras"));
    assert!(ok(grass).unwrap().check_names(&types).is_ok());
}
