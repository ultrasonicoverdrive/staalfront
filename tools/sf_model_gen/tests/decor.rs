//! WP11.6d (docs/art-style.md section 12): the rules the decor and the wrecks add to the kit.
//! The per-model rules (budget, strictly fewer triangles per level, nodes, footprint, ground, the
//! checker with no finding) live in `models.rs`, which runs over every file of `set::all`.

use sf_model_gen::decor::{self, BURNT_STEEL, CHAR, PIT, SOOT, blob};
use sf_model_gen::kit::{Lod, Mat, Model, at_level};
use sf_model_gen::palette::Palette;
use sf_model_gen::set;
use sf_model_gen::stages::bounds;

fn decor_models() -> Vec<Model> {
    decor::recipes().iter().map(|r| r()).collect()
}

#[test]
fn the_tones_of_a_burnt_wreck_are_pinned() {
    assert_eq!((SOOT, BURNT_STEEL, CHAR, PIT), (430, 420, 380, 560));
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

/// Rule 2 (burnt wrecks): no paint or rust triangle of a wreck is brighter than `SOOT` per mille
/// of its baked colour, and nothing is brighter than `PIT` (the dark pits, which are rubber and
/// need the light left); steel and debris are `BURNT_STEEL` / `CHAR`.
#[test]
fn every_triangle_of_a_wreck_is_burnt() {
    let wrecks: Vec<Model> = decor_models()
        .into_iter()
        .filter(|m| m.stem.starts_with("neutral_wreck_"))
        .collect();
    assert_eq!(wrecks.len(), 4);
    for m in &wrecks {
        for (n, lod) in m.lods.iter().enumerate() {
            let tris = lod.world_tris();
            // Slots 0 and 1 are the paints, 5 the rust.
            let paint = |t: &&sf_model_gen::kit::Tri| matches!(t.mat.0, 0 | 1 | 5);
            let loud = tris.iter().filter(paint).filter(|t| t.tone > SOOT).count();
            assert_eq!(
                loud, 0,
                "{} LOD{n}: {loud} paint triangles above the soot tone",
                m.stem
            );
            let bright = tris.iter().filter(|t| t.tone > PIT).count();
            assert_eq!(bright, 0, "{} LOD{n}: {bright} triangles unburnt", m.stem);
        }
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
