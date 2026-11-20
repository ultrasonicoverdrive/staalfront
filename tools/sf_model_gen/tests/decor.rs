//! WP11.6d (docs/art-style.md section 12): the rules the decor and the wrecks add to the kit.
//! The per-model rules (budget, strictly fewer triangles per level, nodes, footprint, ground, the
//! checker with no finding) live in `models.rs`, which runs over every file of `set::all`.

use sf_model_gen::decor::{self, BURNT_STEEL, CHAR, ORE_NUGGETS, ORE_TONE, PIT, RUST, SOOT, blob};
use sf_model_gen::kit::{Lod, Mat, Model, at_level};
use sf_model_gen::palette::Palette;
use sf_model_gen::set;
use sf_model_gen::stages::bounds;

fn decor_models() -> Vec<Model> {
    decor::recipes().iter().map(|r| r()).collect()
}

#[test]
fn the_tones_of_a_burnt_wreck_are_pinned() {
    assert_eq!(
        (SOOT, BURNT_STEEL, CHAR, PIT, RUST, ORE_TONE),
        (200, 420, 380, 560, 650, 800)
    );
}

/// Rule 1 (blobs): the 16-, 8- and 4-sided levels of a rock-like solid share their extremes, so
/// the far levels keep the extent. Every compass point of the ring has the full radius whatever
/// the jitter of the points between.
#[test]
fn a_blob_keeps_its_extent_in_every_level() {
    for seed in 0..64u32 {
        for (rx, rz, h) in [(1850, 1450, 2300), (300, 250, 280), (900, 650, 1000)] {
            let ext = |lod: Lod| {
                at_level(lod, || {
                    let m = blob([0, 0, 0], rx, rz, h, seed, lod, [Mat(0), Mat(1)], Mat(0));
                    bounds(&m.tris)
                })
            };
            let (lo0, hi0) = ext(Lod(0));
            assert_eq!(
                (hi0[0], lo0[0], hi0[1], lo0[1]),
                (rx, -rx, h, 0),
                "seed {seed}"
            );
            assert_eq!((hi0[2], lo0[2]), (rz, -rz), "seed {seed}");
            for lod in [Lod(1), Lod(2)] {
                let (lo, hi) = ext(lod);
                assert_eq!((lo, hi), (lo0, hi0), "seed {seed} LOD{}", lod.0);
            }
        }
    }
}

/// Rule 2 (burnt wrecks): every slot of a wreck is held to the tone it is meant to have: the
/// paints (and the light paint) at most `SOOT`, the rust at most `RUST`, steel and debris at most
/// `BURNT_STEEL` (`CHAR` is darker still), rubber at most `PIT` (the dark pits need the light
/// left). And the baked colour is dark: no corner of any wreck (LOD0) is brighter than
/// `BAKED_LIMIT` in linear luminance, where the unburnt paint alone is 0.26 and the measured wrecks are 0.061 to 0.072.
const BAKED_LIMIT: f32 = 0.10;

#[test]
fn every_triangle_of_a_wreck_is_burnt() {
    let wrecks: Vec<Model> = decor_models()
        .into_iter()
        .filter(|m| m.stem.starts_with("neutral_wreck_"))
        .collect();
    assert_eq!(wrecks.len(), 4);
    // Slots: 0 paint, 1 paint2, 2 paint3, 3 metal, 4 rubber, 5 accent (rust).
    let ceiling = |slot: u8| match slot {
        0..=2 => SOOT,
        3 => BURNT_STEEL,
        4 => PIT,
        _ => RUST,
    };
    for m in &wrecks {
        for (n, lod) in m.lods.iter().enumerate() {
            let tris = lod.world_tris();
            for t in &tris {
                assert!(
                    t.tone <= ceiling(t.mat.0),
                    "{} LOD{n}: a triangle of slot {} at tone {} (ceiling {})",
                    m.stem,
                    t.mat.0,
                    t.tone,
                    ceiling(t.mat.0)
                );
            }
        }
        let world = m.lods[0].world_tris();
        let lin: Vec<[f32; 3]> = m
            .materials
            .iter()
            .map(|x| x.srgb.map(sf_model_gen::glb::linear))
            .collect();
        let baked = sf_model_gen::shade::bake_team(&world, &lin, None);
        let mut brightest = 0.0f32;
        for (i, t) in world.iter().enumerate() {
            let f = sf_model_gen::shade::factor(lin[usize::from(t.mat.0)]);
            for c in 0..3 {
                let b = baked[i * 3 + c];
                let l = 0.2126 * b[0] * f[0] + 0.7152 * b[1] * f[1] + 0.0722 * b[2] * f[2];
                brightest = brightest.max(l);
            }
        }
        println!("{}: brightest baked corner {brightest:.3}", m.stem);
        assert!(brightest <= BAKED_LIMIT, "{}: {brightest}", m.stem);
    }
}

/// The ore field grows from stage 1 to 4 in triangles and in width, inside the 3.1 m cell.
#[test]
fn the_ore_field_stages_grow() {
    let stages: Vec<Model> = (1..=4).map(decor::ore_field).collect();
    let mut last = (0, 0);
    for m in &stages {
        let (lo, hi) = bounds(&m.lods[0].world_tris());
        let w = hi[0] - lo[0];
        assert!(w <= 3100 && hi[2] - lo[2] <= 3000, "{}: {w}", m.stem);
        let t = m.triangles()[0];
        assert!(
            t > last.0 && w > last.1,
            "{}: {t} triangles, {w} mm",
            m.stem
        );
        last = (t, w);
    }
    // The nugget counts, as literals (the recipe reads this constant).
    assert_eq!(ORE_NUGGETS, [4, 6, 9, 13]);
}

/// The trees use the `flora` palette entry (dark olive greens, bark, soil), the rest the neutral
/// one; both have `paint3` as the third slot and no team material. The values are literals.
#[test]
fn the_flora_palette_has_the_pinned_values() {
    let p = Palette::shipped();
    let f = &p.factions["flora"];
    assert_eq!(
        (f.paint, f.paint_light, f.paint2, f.paint2_shade),
        ([86, 98, 70], [104, 116, 86], [60, 70, 48], [72, 82, 58])
    );
    assert_eq!(
        (f.metal, f.rubber, f.accent),
        ([62, 52, 42], [46, 44, 40], [118, 96, 58])
    );
    let names: Vec<String> = p.materials("flora").into_iter().map(|m| m.name).collect();
    assert_eq!(
        names,
        ["paint", "paint2", "paint3", "metal", "rubber", "accent"]
    );
    // Foliage is a dark olive, not a bright green: every channel of the foliage slots at most 120
    // and the green only a little over the red.
    for c in [f.paint, f.paint_light, f.paint2, f.paint2_shade] {
        assert!(c.iter().all(|&v| v <= 120), "{c:?}");
        assert!(i32::from(c[1]) - i32::from(c[0]) <= 16, "{c:?}");
    }
    for m in decor_models()
        .iter()
        .filter(|m| m.stem.starts_with("neutral_tree_"))
    {
        let tree: Vec<[u8; 3]> = m.materials.iter().map(|x| x.srgb).collect();
        let want: Vec<[u8; 3]> = p.materials("flora").iter().map(|x| x.srgb).collect();
        assert_eq!(tree, want, "{}", m.stem);
    }
    for m in decor_models()
        .iter()
        .filter(|m| !m.stem.starts_with("neutral_tree_"))
    {
        let got: Vec<[u8; 3]> = m.materials.iter().map(|x| x.srgb).collect();
        let want: Vec<[u8; 3]> = p.materials("neutral").iter().map(|x| x.srgb).collect();
        assert_eq!(got, want, "{}", m.stem);
    }
}

/// The set holds the 14 files of this WP (4 wrecks, 2 rocks, a fence, a cliff edge, 4 ore field
/// stages, 2 broadleaf trees), by name.
#[test]
fn the_set_holds_the_decor_and_the_wrecks() {
    let stems: Vec<String> = set::all().into_iter().map(|m| m.stem).collect();
    for want in [
        "neutral_wreck_light_vehicle",
        "neutral_wreck_medium_vehicle",
        "neutral_wreck_heavy_vehicle",
        "neutral_wreck_vessel",
        "neutral_rock_large",
        "neutral_rock_small",
        "neutral_fence",
        "neutral_cliff_edge",
        "neutral_ore_field_stage_01",
        "neutral_ore_field_stage_02",
        "neutral_ore_field_stage_03",
        "neutral_ore_field_stage_04",
        "neutral_tree_broadleaf",
        "neutral_tree_broadleaf_fallen",
    ] {
        assert!(stems.iter().any(|s| s == want), "{want} is missing");
    }
    assert_eq!(decor::recipes().len(), 14);
}
