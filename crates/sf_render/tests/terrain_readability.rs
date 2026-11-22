//! WP11.6e: units stay readable against every terrain type, measured on the shipped data.
//!
//! The unit side is the shipped glb models loaded by the engine's own loader (`glb::load_glb_file`:
//! the baked vertex colour times the material colour, in linear light), the visible body (every
//! non-team triangle that looks up, area-weighted, every level of detail: zoom 0.30 and 0.12 show
//! different levels) lit with the engine's default light (`scene::Lighting::default`, the same
//! `ambient + light * max(n.l, 0)` as `scene.wgsl`). The terrain side is the shipped texture of
//! each look (`TerrainLook::texture`, every ore level as well) lit as level ground the way
//! `terrain.wgsl` does. Colours are compared as CIE L*a*b* distance (dE76) of the colours that
//! reach the screen.
//!
//! Two things are held, per unit model, per level of detail and per terrain look:
//!
//! * the mean body colour is at least `MIN_MEAN_DE` from the mean terrain colour, and
//! * the share of the body (by area) that lies within `CLOSE_DE` of the terrain mean is at most
//!   `MAX_CLOSE_SHARE`: a hull that is two thirds sand and one third olive is allowed to have its
//!   olive parts close to olive grass, but not to be mostly olive-on-olive.
//!
//! The team stripes are held against the terrain as well (the four player colours of `units.ron`
//! as the engine draws them: full strength, the same lighting as a top face).

use std::path::PathBuf;

use sf_render::glb::{self, AxisConvention, LoadedModel};
use sf_render::scene::Lighting;
use sf_render::terrain_look::TerrainLooks;

/// Mean body colour against mean terrain colour (dE76 of the rendered colours): 12 is where two
/// flat patches stop reading as two shades of one colour and read as two colours (the just-noticeable
/// difference is about 2.3; the 11.6p test holds the two factions apart by 20 and the neutral models
/// by 12). The numbers measured on the shipped data are in the PR body and docs/art-style.md.
const MIN_MEAN_DE: f64 = 12.0;
/// A body triangle this close to the terrain mean counts as "close" (the same 12).
const CLOSE_DE: f64 = 12.0;
/// The most body area that may be close to the terrain: half. A hull with an olive trim may have
/// part of it near olive grass, never most of it.
const MAX_CLOSE_SHARE: f64 = 0.5;
/// The 5th-percentile texel of the terrain texture against the mean body colour.
const MIN_TEXEL_DE: f64 = 8.0;
/// A team stripe against the terrain mean: the stripes are saturated and bright, the terrain muted.
const MIN_STRIPE_DE: f64 = 30.0;
/// A unit paint (as a top face renders it) against a ground colour (as rendered).
const MIN_PAINT_DE: f64 = 12.0;
/// The grass mean against each of the two reference colours.
const REFERENCE_DE: f64 = 8.0;

fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn load(dir: &str, file: &str) -> LoadedModel {
    glb::load_glb_file(&assets().join(dir).join(file), &AxisConvention::ENGINE)
        .unwrap_or_else(|e| panic!("{dir}/{file}: {e}"))
}

/// Meridian and Veyran units that stand on the ground (set v5 vehicles and vessels, set v3 infantry).
const UNITS: [(&str, &str); 25] = [
    ("set_v5", "meridian_amphibious_transport.glb"),
    ("set_v5", "meridian_harvester.glb"),
    ("set_v5", "meridian_heavy_tank.glb"),
    ("set_v5", "meridian_light_tank.glb"),
    ("set_v5", "meridian_light_transport.glb"),
    ("set_v5", "meridian_repair_truck.glb"),
    ("set_v5", "meridian_siege_artillery.glb"),
    ("set_v5", "veyran_gunboat.glb"),
    ("set_v5", "veyran_harvester.glb"),
    ("set_v5", "veyran_heavy_gun_team.glb"),
    ("set_v5", "veyran_heavy_tank.glb"),
    ("set_v5", "veyran_light_tank.glb"),
    ("set_v5", "veyran_repair_truck.glb"),
    ("set_v5", "veyran_siege_artillery.glb"),
    ("set_v5", "veyran_transport_truck.glb"),
    ("set_v3", "meridian_engineer.glb"),
    ("set_v3", "meridian_rifleman.glb"),
    ("set_v3", "meridian_rocket_trooper.glb"),
    ("set_v3", "meridian_scout.glb"),
    ("set_v3", "meridian_sniper.glb"),
    ("set_v3", "veyran_combat_engineer.glb"),
    ("set_v3", "veyran_demolition_team.glb"),
    ("set_v3", "veyran_engineer.glb"),
    ("set_v3", "veyran_medic.glb"),
    ("set_v3", "veyran_rifleman.glb"),
];

fn lin_to_srgb(l: f64) -> f64 {
    let l = l.clamp(0.0, 1.0);
    if l <= 0.003_130_8 {
        12.92 * l
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

fn srgb_to_lin(s: f64) -> f64 {
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// sRGB 0..1 to CIE L*a*b* (D65).
fn lab(srgb: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = srgb.map(srgb_to_lin);
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f64| {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn de(a: [f64; 3], b: [f64; 3]) -> f64 {
    let (la, lb) = (lab(a), lab(b));
    ((la[0] - lb[0]).powi(2) + (la[1] - lb[1]).powi(2) + (la[2] - lb[2]).powi(2)).sqrt()
}

/// The engine's light as the shaders use it: `ambient + colour * max(n . to_light, 0)`.
fn lit_factor(normal: [f64; 3]) -> [f64; 3] {
    let l = Lighting::default();
    let d = (-l.direction).normalize();
    let facing =
        (normal[0] * f64::from(d.x) + normal[1] * f64::from(d.y) + normal[2] * f64::from(d.z))
            .max(0.0);
    [
        f64::from(l.ambient.x) + f64::from(l.color.x) * facing,
        f64::from(l.ambient.y) + f64::from(l.color.y) * facing,
        f64::from(l.ambient.z) + f64::from(l.color.z) * facing,
    ]
}

/// Linear colour times light, to sRGB 0..1.
fn shown(linear: [f64; 3], normal: [f64; 3]) -> [f64; 3] {
    let f = lit_factor(normal);
    [
        lin_to_srgb(linear[0] * f[0]),
        lin_to_srgb(linear[1] * f[1]),
        lin_to_srgb(linear[2] * f[2]),
    ]
}

/// One visible body triangle: rendered colour (sRGB 0..1) and area.
struct Face {
    colour: [f64; 3],
    area: f64,
}

/// The visible body of one level of a model: non-team triangles that look up, lit as rendered.
fn body(model: &LoadedModel, level: usize) -> Vec<Face> {
    let mut out = Vec::new();
    for part in &model.levels[level].parts {
        let m = &part.mesh;
        for t in m.indices.chunks(3) {
            let v = [
                &m.vertices[t[0] as usize],
                &m.vertices[t[1] as usize],
                &m.vertices[t[2] as usize],
            ];
            // The team material is the player colour (held separately).
            if v.iter().any(|v| v.color[3] > 0.0) {
                continue;
            }
            let p = |i: usize| v[i].position.map(f64::from);
            let (a, b, c) = (p(0), p(1), p(2));
            let (e1, e2) = (
                [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
            );
            let cross = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let len = (cross[0].powi(2) + cross[1].powi(2) + cross[2].powi(2)).sqrt();
            if len < 1e-12 {
                continue;
            }
            // The part frames of a tank only turn about Y: the up axis of a part is the world's.
            let n = v[0].normal.map(f64::from);
            if n[1] < 0.3 {
                continue;
            }
            let lin: [f64; 3] = std::array::from_fn(|k| {
                f64::from(v[0].color[k] + v[1].color[k] + v[2].color[k]) / 3.0
            });
            out.push(Face {
                colour: shown(lin, n),
                area: 0.5 * len,
            });
        }
    }
    out
}

fn mean_of(faces: &[Face]) -> [f64; 3] {
    let total: f64 = faces.iter().map(|f| f.area).sum();
    std::array::from_fn(|k| faces.iter().map(|f| f.colour[k] * f.area).sum::<f64>() / total)
}

/// The terrain colours as rendered: every texel of the texture of a look, lit as level ground.
fn ground(texture: &[u8]) -> Vec<[f64; 3]> {
    texture
        .chunks(4)
        .map(|p| {
            let lin = [p[0], p[1], p[2]].map(|c| srgb_to_lin(f64::from(c) / 255.0));
            shown(lin, [0.0, 1.0, 0.0])
        })
        .collect()
}

fn mean_px(px: &[[f64; 3]]) -> [f64; 3] {
    std::array::from_fn(|k| px.iter().map(|p| p[k]).sum::<f64>() / px.len() as f64)
}

/// Every terrain texture of the shipped looks: each type, and each fill level of the ore.
fn terrains() -> Vec<(String, Vec<[f64; 3]>)> {
    let looks = TerrainLooks::embedded();
    let mut out = Vec::new();
    for name in ["grass", "road", "water", "rock", "ore", "bridge"] {
        out.push((name.to_owned(), ground(&looks.look_for(name).texture())));
    }
    let ore = looks.look_for("ore");
    for (i, level) in looks.resource().unwrap().levels.iter().enumerate() {
        out.push((format!("ore{i}"), ground(&ore.level_texture(level))));
    }
    out
}

/// Every terrain texture with its name, as the shipped looks draw them.
fn names_of(terrains: &[(String, Vec<[f64; 3]>)]) -> Vec<&str> {
    terrains.iter().map(|(n, _)| n.as_str()).collect()
}

/// Both factions, every unit model, every level of detail, against every terrain look (and every
/// fill level of the ore): the mean body colour stands clear of the terrain, and not most of the
/// body is close to it. The olive of Meridian on the olive grass is the case this guards.
#[test]
fn every_unit_reads_against_every_terrain_as_rendered() {
    let terrains = terrains();
    assert_eq!(
        names_of(&terrains),
        [
            "grass", "road", "water", "rock", "ore", "bridge", "ore0", "ore1", "ore2", "ore3"
        ]
    );
    let mut checked = 0;
    for (dir, file) in UNITS {
        let model = load(dir, file);
        assert_eq!(model.levels.len(), 3, "{file}: three levels of detail");
        for level in 0..model.levels.len() {
            let faces = body(&model, level);
            assert!(!faces.is_empty(), "{file} L{level}: no visible body");
            let total: f64 = faces.iter().map(|f| f.area).sum();
            let mean = mean_of(&faces);
            for (name, px) in &terrains {
                let tm = mean_px(px);
                let d = de(mean, tm);
                assert!(
                    d >= MIN_MEAN_DE,
                    "{file} L{level} on {name}: mean body {:?} is only dE {d:.1} from the terrain {:?}",
                    mean.map(|c| (c * 255.0).round()),
                    tm.map(|c| (c * 255.0).round())
                );
                let close: f64 = faces
                    .iter()
                    .filter(|f| de(f.colour, tm) < CLOSE_DE)
                    .map(|f| f.area)
                    .sum::<f64>()
                    / total;
                assert!(
                    close <= MAX_CLOSE_SHARE,
                    "{file} L{level} on {name}: {:.0} % of the body is within dE {CLOSE_DE} of the terrain",
                    close * 100.0
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 25 * 3 * 10);
}

/// The pixel-level worst case too: no unit's mean body colour lies inside the spread of the
/// terrain texture itself (the 5th percentile of the dE to every texel stays over
/// `MIN_TEXEL_DE`), so a noisy texel does not swallow a hull.
#[test]
fn no_unit_hides_in_the_texture_noise() {
    let terrains = terrains();
    for (dir, file) in UNITS {
        let model = load(dir, file);
        for level in 0..model.levels.len() {
            let mean = mean_of(&body(&model, level));
            for (name, px) in &terrains {
                let mut d: Vec<f64> = px.iter().map(|p| de(mean, *p)).collect();
                d.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let p5 = d[d.len() / 20];
                assert!(
                    p5 >= MIN_TEXEL_DE,
                    "{file} L{level} on {name}: 5th percentile texel dE {p5:.1}"
                );
            }
        }
    }
}

/// The player colours as the engine draws a team stripe (full strength, lit like a top face), the
/// standard four of `units.ron` and the colour-blind set, against every terrain.
#[test]
fn the_team_stripes_read_against_every_terrain() {
    let units = std::fs::read_to_string(assets().join("config/units.ron")).unwrap();
    let cvd = std::fs::read_to_string(assets().join("config/palette_colour_blind.ron")).unwrap();
    let mut colours: Vec<[u8; 3]> = Vec::new();
    for text in [&units, &cvd] {
        for t in text.split("srgb: (").skip(1).take(4) {
            let v: Vec<u8> = t
                .split(')')
                .next()
                .unwrap()
                .split(',')
                .map(|x| x.trim().parse().unwrap())
                .collect();
            colours.push([v[0], v[1], v[2]]);
        }
    }
    assert_eq!(
        colours.len(),
        8,
        "four standard and four colour-blind colours"
    );
    for pc in colours {
        let stripe = render_srgb8(pc);
        for (name, px) in terrains() {
            let tm = mean_px(&px);
            let d = de(stripe, tm);
            assert!(
                d >= MIN_STRIPE_DE,
                "player colour {pc:?} on {name}: dE {d:.1}"
            );
        }
    }
}

/// What the palette file says: the stored slot colours of the factions.
#[derive(serde::Deserialize)]
struct PaletteFile {
    factions: std::collections::BTreeMap<String, Slots>,
}

/// The paint slots of one faction (the other slots are not read).
#[derive(serde::Deserialize)]
struct Slots {
    paint: [u8; 3],
    paint_light: [u8; 3],
    paint2: [u8; 3],
    paint2_shade: [u8; 3],
}

/// No terrain colour (the ground, every ore level) is a unit paint: the paints of the model
/// palette, as a top face renders them, stay `MIN_PAINT_DE` from every ground colour as rendered.
/// Meridian's olive (`paint2`) is the closest relative of the grass.
#[test]
fn no_terrain_colour_is_a_unit_paint() {
    let text = std::fs::read_to_string(assets().join("config/model_palette.ron")).unwrap();
    let palette: PaletteFile = ron::from_str(&text).unwrap();
    let looks = TerrainLooks::embedded();
    let mut ground: Vec<(String, [f64; 3])> = Vec::new();
    for name in ["grass", "road", "water", "rock", "ore", "bridge"] {
        let l = looks.look_for(name);
        ground.push((name.to_owned(), render_srgb8(l.color)));
    }
    for (i, level) in looks.resource().unwrap().levels.iter().enumerate() {
        ground.push((format!("ore level {i}"), render_srgb8(level.color)));
    }
    let mut closest = (1e9_f64, String::new());
    // The two factions of the game. The neutral and Network entries are left out on purpose: the
    // neutral supply truck's grey-green paint was already as near the rock before this WP (dE 6
    // against the old rock too) and is a gap listed in the PR, not a rule this test can hold.
    for (faction, slots) in palette
        .factions
        .iter()
        .filter(|(f, _)| ["meridian", "veyran"].contains(&f.as_str()))
    {
        let named = [
            ("paint", slots.paint),
            ("paint_light", slots.paint_light),
            ("paint2", slots.paint2),
            ("paint2_shade", slots.paint2_shade),
        ];
        for (slot, colour) in named {
            let paint = render_srgb8(colour);
            for (name, g) in &ground {
                let d = de(paint, *g);
                if d < closest.0 {
                    closest = (d, format!("{faction} {slot} on {name}"));
                }
                assert!(
                    d >= MIN_PAINT_DE,
                    "{faction} {slot} {colour:?} is dE {d:.1} from the {name} colour"
                );
            }
        }
    }
    println!("closest paint to a ground colour: {closest:?}");
}

/// The grass is the dark olive of the approved reference shot (`#4F511B` and `#484E17`, used here
/// as numbers only): its mean as rendered is within `REFERENCE_DE` of both, so it is neither the
/// bright yellow-green of the old look nor so dark that it reads black.
#[test]
fn the_grass_is_the_dark_olive_of_the_reference() {
    let terrains = terrains();
    let grass = mean_px(&terrains[0].1);
    for reference in [[79u8, 81, 27], [72, 78, 23]] {
        let r = reference.map(|c| f64::from(c) / 255.0);
        let d = de(grass, r);
        assert!(
            d <= REFERENCE_DE,
            "grass {:?} is dE {d:.1} from the reference {reference:?}",
            grass.map(|c| (c * 255.0).round())
        );
    }
    // Not black: the lightness stays well above the darkest units.
    assert!(lab(grass)[0] >= 25.0, "grass reads black: {:?}", lab(grass));
}

/// Nothing neon: no terrain colour has a channel above 235, and none a saturation (HSV) above
/// 0.75 (the olive reference itself is 0.66).
#[test]
fn nothing_in_the_terrain_is_neon() {
    let looks = TerrainLooks::embedded();
    let mut colours: Vec<(String, [u8; 3])> = Vec::new();
    for name in ["grass", "road", "water", "rock", "ore", "bridge"] {
        let l = looks.look_for(name);
        colours.push((name.to_owned(), l.color));
        if let Some(m) = l.minimap {
            colours.push((format!("{name} minimap"), m));
        }
        if let sf_render::terrain_look::Pattern::Flecks { color } = l.pattern {
            colours.push(("ore flecks".to_owned(), color));
        }
    }
    for (i, level) in looks.resource().unwrap().levels.iter().enumerate() {
        colours.push((format!("ore level {i}"), level.color));
        colours.push((format!("ore level {i} minimap"), level.minimap));
    }
    for (name, c) in colours {
        let (max, min) = (*c.iter().max().unwrap(), *c.iter().min().unwrap());
        assert!(max <= 235, "{name} {c:?} is too bright");
        let sat = f64::from(max - min) / f64::from(max);
        assert!(sat <= 0.75, "{name} {c:?} is too saturated ({sat:.2})");
    }
}

/// An sRGB byte colour as a lit top face (level ground) shows it, sRGB 0..1.
fn render_srgb8(c: [u8; 3]) -> [f64; 3] {
    shown(
        c.map(|v| srgb_to_lin(f64::from(v) / 255.0)),
        [0.0, 1.0, 0.0],
    )
}
