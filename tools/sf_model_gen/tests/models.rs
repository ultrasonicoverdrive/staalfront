//! The rules of the kit, per model of set v5 (WP11.6a, docs/art-style.md): budgets per level and
//! strictly fewer triangles per level, the team share of the top-down view in 20-30 % on many
//! triangles, the nodes the engine and the checker need, sizes inside the set v4 models they
//! replace, and `sf_asset_check` with no finding at all. Every check runs on what the generator
//! itself builds and writes (`models::MODELS`, `glb::write`).

use sf_asset_check::Checker;
use sf_model_gen::glb;
use sf_model_gen::kit::{Lod, Model, TEAM};
use sf_model_gen::models::MODELS;
use sf_model_gen::palette::{FactionColours, Palette};

fn models() -> Vec<Model> {
    MODELS.iter().map(|b| b()).collect()
}

fn is_building(stem: &str) -> bool {
    ["construction_center", "power_plant", "defence_tower"]
        .iter()
        .any(|b| stem.ends_with(b))
}

/// A person (or a team of people): the figure nodes instead of hull and turret, the infantry
/// budget. Only the heavy gun team is generated; the other figures are Tijmen's set v3.
fn is_infantry(stem: &str) -> bool {
    stem.ends_with("heavy_gun_team")
}

/// A neutral object has no player colour: no `team` material (the third slot is `paint3`).
fn is_neutral(stem: &str) -> bool {
    stem.starts_with("neutral_")
}

/// A vessel floats: the waterline `Y = 0` lies inside the hull.
fn is_vessel(stem: &str) -> bool {
    stem.ends_with("gunboat") || stem.ends_with("ferry")
}

/// Vehicles with a gun: they need a muzzle under the barrel.
fn is_armed(stem: &str) -> bool {
    ["light_tank", "heavy_tank", "siege_artillery", "gunboat"]
        .iter()
        .any(|r| stem.ends_with(r))
}

/// The art brief's budgets (`sf_asset_check::class::budget`), as literals.
fn budget(stem: &str) -> [usize; 3] {
    if is_building(stem) {
        [5000, 1500, 300]
    } else if is_infantry(stem) {
        [1500, 250, 60]
    } else {
        [1500, 500, 150]
    }
}

#[test]
fn every_level_is_inside_its_budget_and_really_reduced() {
    for m in models() {
        let t = m.triangles();
        let b = budget(&m.stem);
        println!("{}: {t:?}", m.stem);
        for n in 0..3 {
            assert!(t[n] <= b[n], "{} LOD{n}: {} > {}", m.stem, t[n], b[n]);
        }
        assert!(
            t[0] > t[1] && t[1] > t[2],
            "{}: LODs not reduced {t:?}",
            m.stem
        );
        // Really reduced, not a token: LOD1 at most 40 % and LOD2 at most 15 % of LOD0.
        assert!(t[1] * 10 <= t[0] * 4, "{}: LOD1 {t:?}", m.stem);
        assert!(t[2] * 100 <= t[0] * 15, "{}: LOD2 {t:?}", m.stem);
    }
}

/// The tuning round (14-11-2026): buildings spend their budget on real detail (vents, pipes,
/// railings, roof units, window rows, door frames, bolted panels), 2,500-3,500 triangles in LOD0;
/// vehicles get a little more detail but stay under the unit budget.
#[test]
fn buildings_use_their_detail_budget() {
    for m in models() {
        let t = m.triangles();
        if is_building(&m.stem) {
            assert!(
                (2500..=3500).contains(&t[0]),
                "{}: LOD0 {} not in 2500-3500",
                m.stem,
                t[0]
            );
        } else if is_infantry(&m.stem) {
            // A figure is small: 400 triangles in LOD0 keep LOD2 (60 at most) under 15 %.
            assert!((400..1500).contains(&t[0]), "{}: LOD0 {}", m.stem, t[0]);
        } else {
            assert!((1200..1500).contains(&t[0]), "{}: LOD0 {}", m.stem, t[0]);
        }
    }
}

#[test]
fn the_checker_finds_nothing_and_the_team_share_is_20_to_30_percent_in_every_level() {
    let checker = Checker::embedded();
    for m in models() {
        let file = format!("{}.glb", m.stem);
        let report = checker.check_bytes(&file, &glb::write(&m));
        let shares = &report.facts.team_share;
        if is_neutral(&m.stem) {
            // No player colour on a neutral object: the checker has nothing to measure.
            assert!(report.findings.is_empty(), "{file}: {:?}", report.findings);
            assert!(shares.is_empty(), "{file}: {shares:?}");
            continue;
        }
        assert_eq!(shares.len(), 3, "{file}");
        let shares: Vec<f64> = shares.iter().map(|s| s.expect("seen from above")).collect();
        println!("{file}: team share {shares:.3?}");
        assert!(report.findings.is_empty(), "{file}: {:?}", report.findings);
        for (n, s) in shares.iter().enumerate() {
            // The tuning round (14-11-2026): team is bands and trim, aimed at 20-25 %. The
            // checker's own rule stays at 20-40 % (so 20 is the floor); this test caps it at 30.
            assert!((0.20..=0.30).contains(s), "{file} LOD{n}: team {s}");
        }
    }
}

#[test]
fn the_team_colour_is_bands_and_panels_never_one_triangle() {
    for m in models() {
        if is_neutral(&m.stem) {
            continue;
        }
        let team = m.team().expect("a team material");
        assert_eq!(m.materials[team.0 as usize].name, TEAM);
        for (n, level) in m.lods.iter().enumerate() {
            let count = level.world_tris().iter().filter(|t| t.mat == team).count();
            // A figure's far level has 60 triangles in all: four team triangles there (a vest).
            let least = if is_infantry(&m.stem) && n == 2 {
                4
            } else {
                12
            };
            assert!(count >= least, "{} LOD{n}: {count} team triangles", m.stem);
        }
    }
}

/// Refinement round 2 (14-11-2026): the team colour reads as markings, not as a frame, so every
/// band is narrow. Every team triangle that faces up (seen from the game camera) is at most
/// [`MAX_BAND_MM`] across: its height over its longest edge (a strip of width `w` split into two
/// triangles gives at most `w`). The thick roof frames of the tuning round (850 mm) fail this.
/// LOD0 and LOD1 only: LOD2 is the far view on 150-300 triangles, where the harvester's drum keeps
/// two wider bands (four narrow ones cost 32 triangles more than its budget allows).
const MAX_BAND_MM: f64 = 350.0;

#[test]
fn every_team_band_is_narrow() {
    for m in models() {
        if is_neutral(&m.stem) {
            continue;
        }
        let team = m.team().expect("a team material");
        for (n, level) in m.lods.iter().enumerate().take(2) {
            for t in level.world_tris().iter().filter(|t| t.mat == team) {
                let p = t.v.map(|v| v.map(f64::from));
                let e = |a: usize, b: usize| [0, 1, 2].map(|k| p[b][k] - p[a][k]);
                let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                let (ab, ac) = (e(0, 1), e(0, 2));
                let x = [
                    ab[1] * ac[2] - ab[2] * ac[1],
                    ab[2] * ac[0] - ab[0] * ac[2],
                    ab[0] * ac[1] - ab[1] * ac[0],
                ];
                let area2 = len(x);
                if area2 == 0.0 || x[1] / area2 < 0.7 {
                    continue; // a wall or a downward face: not seen from above
                }
                let longest = len(ab).max(len(ac)).max(len(e(1, 2)));
                let width = area2 / longest;
                assert!(
                    width <= MAX_BAND_MM,
                    "{} LOD{n}: a team band {width:.0} mm wide at {:?}",
                    m.stem,
                    t.v
                );
            }
        }
    }
}

#[test]
fn every_level_has_the_nodes_of_its_role() {
    for m in models() {
        for (n, root) in m.lods.iter().enumerate() {
            assert_eq!(root.name, format!("{}_LOD{n}", m.stem));
            let mut need = vec![];
            if is_building(&m.stem) {
                need.extend(["body", "fx_smoke_0", "fx_fire_0"]);
            } else if is_infantry(&m.stem) {
                // The six figure nodes of the checker (`sf_asset_check::check::FIGURE_NODES`).
                assert!(root.find("fx_smoke_0").is_none(), "{}", m.stem);
                need.extend(["torso", "head", "arm_l", "arm_r", "leg_l", "leg_r"]);
            } else {
                // Vehicles smoke by the engine's rule (no anchors), like every other vehicle.
                assert!(root.find("fx_smoke_0").is_none(), "{}", m.stem);
                need.extend(["hull", "turret", "barrel"]);
            }
            if is_armed(&m.stem) || m.stem.ends_with("defence_tower") {
                need.extend(["turret", "barrel", "muzzle"]);
            }
            for name in need {
                let node = root
                    .find(name)
                    .unwrap_or_else(|| panic!("{} LOD{n}: no node {name}", m.stem));
                if name.starts_with("fx_") || name == "muzzle" {
                    assert!(node.mesh.is_empty(), "{} {name} is an empty anchor", m.stem);
                }
            }
            // The muzzle hangs under a barrel with geometry (docs/conventions.md §12).
            if let Some(barrel) = root.find("barrel")
                && root.find("muzzle").is_some()
            {
                assert!(!barrel.mesh.is_empty() && barrel.find("muzzle").is_some());
            }
        }
    }
}

/// Size of LOD0 (x, z) in metres must stay inside the model it replaces: the set v4 file, as
/// `sf_asset_check` measured it on 14-11-2026 (`size` column), as literals. The heavy gun team has
/// no v4 file; its bound is the infantry cell it stands in (about 1.7 x 1.2 m, pinned).
const FOOTPRINTS: &[(&str, f32, f32)] = &[
    ("meridian_amphibious_transport", 6.33, 3.26),
    ("meridian_construction_center", 15.20, 15.20),
    ("meridian_harvester", 6.46, 3.46),
    ("meridian_heavy_tank", 7.25, 3.80),
    ("meridian_light_tank", 5.11, 2.90),
    ("meridian_light_transport", 3.42, 2.11),
    ("meridian_repair_truck", 4.94, 2.50),
    ("meridian_siege_artillery", 8.60, 3.10),
    ("neutral_ferry", 21.12, 9.00),
    ("neutral_supply_truck", 5.64, 2.50),
    ("veyran_defence_tower", 3.90, 3.80),
    ("veyran_gunboat", 18.00, 5.00),
    ("veyran_harvester", 6.54, 3.60),
    ("veyran_heavy_gun_team", 1.70, 1.20),
    ("veyran_heavy_tank", 7.25, 3.80),
    ("veyran_light_tank", 5.11, 2.90),
    ("veyran_power_plant", 7.60, 11.40),
    ("veyran_repair_truck", 4.94, 2.50),
    ("veyran_siege_artillery", 8.60, 3.10),
    ("veyran_transport_truck", 5.64, 2.50),
];

#[test]
fn every_model_stays_inside_the_footprint_it_replaces() {
    let checker = Checker::embedded();
    let all = models();
    for m in &all {
        let (_, x, z) = *FOOTPRINTS
            .iter()
            .find(|f| f.0 == m.stem)
            .unwrap_or_else(|| panic!("{} has no footprint", m.stem));
        let r = checker.check_bytes(&format!("{}.glb", m.stem), &glb::write(m));
        let (lo, hi) = r.facts.bounds.expect("bounds");
        let ext = hi - lo;
        println!("{}: {:.2} x {:.2} (limit {x} x {z})", m.stem, ext.x, ext.z);
        assert!(ext.x <= x && ext.z <= z, "{}: {ext:?} vs {x} x {z}", m.stem);
    }
}

/// A vessel's waterline `Y = 0` lies inside its hull (it has a keel under the water); every
/// other model stands on the ground with its lowest point at `Y = 0`.
#[test]
fn vessels_float_and_everything_else_stands_on_the_ground() {
    let checker = Checker::embedded();
    for m in models() {
        let r = checker.check_bytes(&format!("{}.glb", m.stem), &glb::write(&m));
        let (lo, hi) = r.facts.bounds.expect("bounds");
        if is_vessel(&m.stem) {
            assert!(lo.y < 0.0 && hi.y > 0.0, "{}: {lo:?} {hi:?}", m.stem);
        } else {
            assert!(lo.y.abs() < 0.005, "{}: lowest point {}", m.stem, lo.y);
        }
    }
}

/// A neutral model carries `paint3` where a faction model carries `team`, and no triangle of it
/// uses that slot as a player colour.
#[test]
fn a_neutral_model_has_no_team_material() {
    for m in models() {
        let names: Vec<&str> = m.materials.iter().map(|x| x.name.as_str()).collect();
        if is_neutral(&m.stem) {
            assert_eq!(
                names,
                ["paint", "paint2", "paint3", "metal", "rubber", "accent"],
                "{}",
                m.stem
            );
            assert!(m.team().is_none(), "{}", m.stem);
        } else {
            assert_eq!(
                names,
                ["paint", "paint2", "team", "metal", "rubber", "accent"],
                "{}",
                m.stem
            );
        }
    }
}

/// The units of set v5 (WP11.6b): the files, in the order of `MODELS`, are the ones the roles of
/// `unit_defs.ron` name (the wiring itself is pinned in `sf_data`'s units test).
#[test]
fn every_unit_role_has_its_model_in_the_set() {
    let stems: Vec<String> = models().iter().map(|m| m.stem.clone()).collect();
    for want in [
        "meridian_amphibious_transport",
        "meridian_heavy_tank",
        "meridian_light_transport",
        "meridian_repair_truck",
        "meridian_siege_artillery",
        "neutral_ferry",
        "neutral_supply_truck",
        "veyran_gunboat",
        "veyran_harvester",
        "veyran_heavy_gun_team",
        "veyran_heavy_tank",
        "veyran_repair_truck",
        "veyran_siege_artillery",
        "veyran_transport_truck",
    ] {
        assert!(stems.iter().any(|s| s == want), "{want} is missing");
    }
    let mut sorted = stems.clone();
    sorted.sort();
    assert_eq!(stems, sorted, "MODELS is sorted by file name");
}

#[test]
fn the_shipped_palette_has_the_style_g_values() {
    let p = Palette::shipped();
    assert_eq!(
        p.factions["meridian"],
        FactionColours {
            paint: [220, 180, 80],
            paint_light: [232, 200, 104],
            paint2: [122, 122, 64],
            paint2_shade: [80, 80, 60],
            metal: [40, 40, 20],
            rubber: [20, 20, 20],
            accent: [70, 96, 110],
        }
    );
    assert_eq!(
        p.factions["veyran"],
        FactionColours {
            paint: [120, 148, 178],
            paint_light: [150, 176, 204],
            paint2: [74, 78, 104],
            paint2_shade: [98, 102, 128],
            metal: [40, 40, 40],
            rubber: [20, 20, 20],
            accent: [160, 32, 48],
        }
    );
    // Neutral models (the supply truck, the ferry): no accent of a faction, the third slot is paint.
    assert_eq!(
        p.factions["neutral"],
        FactionColours {
            paint: [190, 176, 140],
            paint_light: [206, 196, 164],
            paint2: [96, 108, 92],
            paint2_shade: [70, 80, 70],
            metal: [44, 44, 40],
            rubber: [20, 20, 20],
            accent: [150, 70, 40],
        }
    );
    let neutral: Vec<(String, [u8; 3])> = p
        .materials("neutral")
        .into_iter()
        .map(|m| (m.name, m.srgb))
        .collect();
    assert_eq!(neutral[2], ("paint3".to_owned(), [206, 196, 164]));
    assert_eq!(p.team_base, [235, 235, 235]);
    // The material slots, in their fixed order (a later texture step keys on them).
    let names: Vec<String> = p.materials("veyran").into_iter().map(|m| m.name).collect();
    assert_eq!(
        names,
        ["paint", "paint2", "team", "metal", "rubber", "accent"]
    );
    // The Veyran accent stays darker than a red player colour could read (docs/art-style.md):
    // its brightest channel is at most 160.
    assert!(p.factions["veyran"].accent.iter().all(|&c| c <= 160));
    assert!(Palette::parse("(factions: {}, team_base: (1, 2, 3))").is_err());
}

#[test]
fn the_lod_knob_halves_segments_and_drops_bevels() {
    assert_eq!(
        Lod::ALL.map(|l| l.segs(16)),
        [16, 8, 4],
        "segments per level"
    );
    assert_eq!(Lod::ALL.map(|l| l.segs(8)), [8, 4, 4]);
    assert_eq!(Lod::ALL.map(|l| l.bevel(40)), [40, 20, 0]);
    assert_eq!(Lod::ALL.map(|l| l.has(0)), [true, false, false]);
    assert_eq!(Lod::ALL.map(|l| l.has(1)), [true, true, false]);
}

#[test]
fn every_solid_of_the_kit_faces_outward_and_has_no_zero_area_triangle() {
    use sf_model_gen::kit::{Mat, bevel_box, dot, extrude_z1, lathe, lathe_x, lathe_z, loft, ngon};
    let m = Mat(0);
    // Convex solids round the origin (height middle at y = 500): every triangle's normal points
    // away from the middle.
    let solids = [
        (
            "bevel box",
            bevel_box([-1000, 0, -600], [1000, 1000, 600], 120, m, m),
        ),
        (
            "plain box",
            bevel_box([-1000, 0, -600], [1000, 1000, 600], 0, m, m),
        ),
        (
            "lathe",
            lathe(
                &[(800, 0), (800, 600), (500, 1000)],
                16,
                &[m],
                None,
                Some(m),
            ),
        ),
        (
            "lathe x",
            lathe_x(&[(400, -500), (400, 500)], 8, &[m], None, Some(m)).moved([0, 500, 0]),
        ),
        (
            "lathe z",
            lathe_z(&[(400, -500), (400, 500)], 8, &[m], None, Some(m)).moved([0, 500, 0]),
        ),
        (
            "extrude",
            extrude_z1(
                &[[-900, 0], [900, 0], [700, 1000], [-700, 1000]],
                -400,
                400,
                m,
            ),
        ),
        (
            "loft",
            loft(
                &[(ngon(900, 8, true), 0), (ngon(600, 8, true), 1000)],
                &[m],
                Some(m),
                Some(m),
            ),
        ),
    ];
    for (name, mesh) in solids {
        assert!(!mesh.is_empty(), "{name}");
        for t in &mesh.tris {
            let n = t.normal();
            assert_ne!(n, [0, 0, 0], "{name}: zero-area triangle");
            let c = [0, 1, 2].map(|i| t.v.iter().map(|p| i64::from(p[i])).sum::<i64>());
            assert!(
                dot(n, [c[0], c[1] - 1500, c[2]]) > 0,
                "{name}: {:?} faces inward",
                t.v
            );
        }
    }
}
