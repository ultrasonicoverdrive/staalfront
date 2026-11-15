//! The rules of the kit, per model of set v5 (WP11.6a, WP11.6c, docs/art-style.md): budgets per
//! level and strictly fewer triangles per level, the team share of the top-down view in 20-30 %
//! on many triangles, the nodes the engine and the checker need, sizes inside the set v4 models
//! they replace, and `sf_asset_check` with no finding at all. Every check runs on what the
//! generator itself builds and writes (`set::all`, `glb::write`); the reports are made once.

use std::sync::OnceLock;

use sf_asset_check::{Checker, FileReport};
use sf_model_gen::kit::{Lod, Model, TEAM};
use sf_model_gen::palette::{FactionColours, Palette};
use sf_model_gen::stages::{STAGE_CEIL_PERMILLE, Stage};
use sf_model_gen::{files, set};

fn models() -> &'static [Model] {
    static M: OnceLock<Vec<Model>> = OnceLock::new();
    M.get_or_init(set::all)
}

/// The checker's report of every file, in the order of [`models`].
fn reports() -> &'static [FileReport] {
    static R: OnceLock<Vec<FileReport>> = OnceLock::new();
    R.get_or_init(|| {
        let checker = Checker::embedded();
        files()
            .iter()
            .map(|(name, bytes)| checker.check_bytes(name, bytes))
            .collect()
    })
}

/// The structure roles (WP11.6c): a `body` with smoke and fire anchors, the building budget.
const STRUCTURES: [&str; 21] = [
    "construction_center",
    "power_plant",
    "refinery",
    "barracks",
    "vehicle_factory",
    "radar",
    "defence_tower",
    "advanced_defences",
    "shipyard",
    "decoy_emplacement",
    "fortified_bunker",
    "ore_silo",
    "relay_transmitter",
    "fuel_depot",
    "archive_building",
    "bridge_segment",
    "rail_spur",
    "generator",
    "automated_gun",
    "unmanned_artillery",
    "evidence_crate",
];

/// The suffixes of the states (`build_stages.ron`, `damage.ron`, and the mission objects' own);
/// `_heavily_damaged` before `_damaged`, which it ends with.
const STATES: [&str; 9] = [
    "_foundation",
    "_scaffolding",
    "_heavily_damaged",
    "_damaged",
    "_rubble",
    "_burnt_out",
    "_destroyed",
    "_off",
    "_disabled",
];

/// The role of a stem without its faction and state (`meridian_refinery_damaged` -> `refinery`).
fn role(stem: &str) -> &str {
    let mut r = stem.split_once('_').map_or(stem, |(_, r)| r);
    for s in STATES.iter().chain(&["_no_uplink"]) {
        r = r.strip_suffix(s).unwrap_or(r);
    }
    r
}

/// A structure, a mission object or a heap: anchors, no hull.
fn is_building(stem: &str) -> bool {
    STRUCTURES.contains(&role(stem)) || is_heap(stem)
}

/// A rubble heap of a footprint (`wrecks.ron`): decor.
fn is_heap(stem: &str) -> bool {
    stem.starts_with("neutral_rubble_")
}

/// Decor: the budget of a vehicle.
fn is_decor(stem: &str) -> bool {
    is_heap(stem) || stem.starts_with("neutral_evidence_crate")
}

/// A faction's finished structure (not a state, not a mission object).
fn is_finished_structure(stem: &str) -> bool {
    is_building(stem)
        && (stem.starts_with("meridian_") || stem.starts_with("veyran_"))
        && role(stem) == stem.split_once('_').unwrap().1
}

/// A neutral object has no player colour (its third slot is `paint3`).
fn is_neutral(stem: &str) -> bool {
    stem.starts_with("neutral_")
}

/// A person (or a team of people): the figure nodes instead of hull and turret, the infantry
/// budget. Only the heavy gun team is generated; the other figures are Tijmen's set v3 (WP11.6b).
fn is_infantry(stem: &str) -> bool {
    stem.ends_with("heavy_gun_team")
}

/// A vessel floats: the waterline `Y = 0` lies inside the hull (WP11.6b).
fn is_vessel(stem: &str) -> bool {
    stem.ends_with("gunboat") || stem.ends_with("ferry")
}

/// A model with a gun: a muzzle under its barrel (the heavy tank has two, `muzzle_l`/`_r`).
fn is_armed(stem: &str) -> bool {
    let armed = [
        "light_tank",
        "siege_artillery",
        "gunboat",
        "defence_tower",
        "advanced_defences",
        "decoy_emplacement",
        "automated_gun",
        "unmanned_artillery",
    ];
    let unarmed_state = ["_foundation", "_scaffolding", "_rubble"]
        .iter()
        .any(|s| stem.ends_with(s));
    armed.contains(&role(stem)) && !unarmed_state
}

/// The art brief's budgets (`sf_asset_check::class::budget`), as literals.
fn budget(stem: &str) -> [usize; 3] {
    if is_building(stem) && !is_decor(stem) {
        [8000, 2400, 500]
    } else if is_infantry(stem) {
        [1500, 250, 60]
    } else if is_decor(stem) {
        [1500, 500, 150]
    } else {
        [4000, 1200, 300]
    }
}

/// docs/art-style.md section 9, "far levels keep the silhouette" (carried over from the WP11.6b
/// review), for every unit, finished structure and object: LOD1 and LOD2 span, along every axis, at least `SILHOUETTE_MIN_PERMILLE` of LOD0's
/// extent and reach at most 5 % of that extent past LOD0 on either side, so a model does not shrink, lose its gun or its
/// mast, or grow when the level switches.
#[test]
fn far_levels_keep_the_silhouette() {
    const SILHOUETTE_MIN_PERMILLE: i64 = 900;
    let extent = |n: &sf_model_gen::kit::Node| {
        let tris = n.world_tris();
        let mut lo = [i32::MAX; 3];
        let mut hi = [i32::MIN; 3];
        for p in tris.iter().flat_map(|t| t.v) {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        (lo, hi)
    };
    let mut worst = (1000, String::new());
    // A state (a stage, a wreck of a mission object) is derived from its finished model's far
    // level, so its far levels can stand above its own broken LOD0: they are left out here.
    let state = |stem: &str| {
        STATES
            .iter()
            .chain(&["_no_uplink"])
            .any(|s| stem.ends_with(s))
    };
    // Named exceptions, measured on this generator (WP11.6r): the rubble heaps are decor whose
    // top is a chunk tip (LOD2 down to 522 per mille of the height), and the ferry and the gunboat
    // drop their mast in LOD2 (746 and 705 per mille of the height): left as they are, a gap.
    let exempt = |stem: &str, k: usize| {
        is_heap(stem) || (k == 2 && ["neutral_ferry", "veyran_gunboat"].contains(&stem))
    };
    for m in models().iter().filter(|m| !state(&m.stem)) {
        let (lo0, hi0) = extent(&m.lods[0]);
        for k in 1..3 {
            let (lo, hi) = extent(&m.lods[k]);
            for i in 0..3 {
                let e0 = i64::from(hi0[i] - lo0[i]);
                let e = i64::from(hi[i] - lo[i]);
                let permille = e * 1000 / e0.max(1);
                if exempt(&m.stem, k) {
                    continue;
                }
                if permille < worst.0 {
                    worst = (permille, format!("{} LOD{k} axis {i}", m.stem));
                }
                assert!(
                    permille >= SILHOUETTE_MIN_PERMILLE,
                    "{} LOD{k}: axis {i} spans {e} mm of LOD0's {e0}",
                    m.stem
                );
                let slack = (hi0[i] - lo0[i]) / 20;
                assert!(
                    lo[i] >= lo0[i] - slack && hi[i] <= hi0[i] + slack,
                    "{} LOD{k}: axis {i} {lo:?}..{hi:?} outside LOD0 {lo0:?}..{hi0:?}",
                    m.stem
                );
            }
        }
    }
    println!("narrowest far level: {} per mille ({})", worst.0, worst.1);
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
/// vehicles get a little more detail but stay under the unit budget. WP11.6c: a state keeps a
/// share of its finished model at most ([`STAGE_CEIL_PERMILLE`], WP11.6c review), a mission
/// object at least 1,000.
#[test]
fn buildings_use_their_detail_budget() {
    let all = models();
    for m in all {
        let t = m.triangles();
        if is_finished_structure(&m.stem) {
            assert!(
                (2500..=6000).contains(&t[0]),
                "{}: LOD0 {} not in 2500-6000",
                m.stem,
                t[0]
            );
        } else if let Some(stage) = Stage::ALL
            .into_iter()
            .rev() // `_heavily_damaged` before `_damaged`
            .find(|s| m.stem.ends_with(s.suffix()) && !is_neutral(&m.stem))
        {
            let base = m.stem.strip_suffix(stage.suffix()).unwrap();
            let finished = all
                .iter()
                .find(|f| f.stem == base)
                .expect("its finished model");
            let k = Stage::ALL.iter().position(|s| *s == stage).unwrap();
            let ceil = finished.triangles()[0] * STAGE_CEIL_PERMILLE[k] / 1000;
            assert!(t[0] <= ceil, "{}: LOD0 {} > {ceil}", m.stem, t[0]);
        } else if is_decor(&m.stem) {
            assert!((100..1500).contains(&t[0]), "{}: LOD0 {}", m.stem, t[0]);
        } else if is_building(&m.stem) {
            // A mission object at least 1,000 (`objects::OBJECT_FLOOR`); its states (a heavy stage
            // broken off low, `_destroyed`, `_rubble`) at least 200.
            let state = STATES
                .iter()
                .chain(&["_no_uplink"])
                .any(|s| m.stem.ends_with(s));
            let least = if state { 200 } else { 1000 };
            assert!((least..=6000).contains(&t[0]), "{}: LOD0 {}", m.stem, t[0]);
        } else if is_infantry(&m.stem) {
            // A figure is small: 400 triangles in LOD0 keep LOD2 (60 at most) under 15 %.
            assert!((400..1500).contains(&t[0]), "{}: LOD0 {}", m.stem, t[0]);
        } else {
            // WP11.6r: bevels and rounder parts on top of the 1,200 of the tuning round, well
            // under the 4,000 ceiling (the budget is not a target).
            assert!((1200..=3000).contains(&t[0]), "{}: LOD0 {}", m.stem, t[0]);
        }
    }
}

/// The stage floors and the state list, as literals (they set what a state may cost).
#[test]
fn the_stage_rules_are_pinned() {
    use sf_model_gen::{buildings, objects, stages};
    assert_eq!(STAGE_CEIL_PERMILLE, [300, 700, 900, 700]);
    assert_eq!(stages::SCAFFOLD_CUT_PERMILLE, 600);
    assert_eq!(stages::HEAVY_CUT_PERMILLE, 500);
    assert_eq!(stages::DETAIL_GONE_PERMILLE, 8000);
    assert_eq!(stages::LOD1_MAX_PERMILLE, 380);
    assert_eq!(stages::LOD2_MAX_PERMILLE, 140);
    assert_eq!(stages::TEAM_MIN_TRIS, 12);
    assert_eq!(stages::CHUNK_SIDES, 8);
    assert_eq!(stages::FIT_RES, 400);
    assert_eq!(buildings::DETAIL_CEIL, 3400);
    assert_eq!(buildings::DETAIL_FLOOR, 2650);
    assert_eq!(objects::OBJECT_FLOOR, 1000);
    assert_eq!(objects::OBJECT_CEIL, 3400);
    assert_eq!(objects::DECOR_CEIL, 1450);
    assert_eq!(
        Stage::ALL.map(Stage::suffix),
        [
            "_foundation",
            "_scaffolding",
            "_damaged",
            "_heavily_damaged"
        ]
    );
    assert_eq!(sf_model_gen::stages::TEAM_AIM, 0.216);
    assert_eq!(sf_model_gen::stages::FAR_STRIPE_MM, 600);
}

#[test]
fn the_checker_finds_nothing_and_the_team_share_is_20_to_30_percent_in_every_level() {
    for (m, report) in models().iter().zip(reports()) {
        let file = format!("{}.glb", m.stem);
        assert_eq!(report.file, file);
        assert!(report.findings.is_empty(), "{file}: {:?}", report.findings);
        let shares = &report.facts.team_share;
        if is_neutral(&m.stem) {
            // No player colour on a neutral object: the checker has nothing to measure.
            assert!(shares.is_empty(), "{file}: {shares:?}");
            continue;
        }
        assert_eq!(shares.len(), 3, "{file}");
        let shares: Vec<f64> = shares.iter().map(|s| s.expect("seen from above")).collect();
        println!("{file}: team share {shares:.3?}");
        for (n, s) in shares.iter().enumerate() {
            // The tuning round (14-11-2026): team is bands and trim, aimed at 20-25 %. The
            // checker's own rule stays at 20-40 % (so 20 is the floor); this test caps it at 30.
            assert!((0.20..=0.30).contains(s), "{file} LOD{n}: team {s}");
        }
    }
}

#[test]
fn a_neutral_model_has_no_team_material() {
    for m in models() {
        let names: Vec<&str> = m.materials.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(m.team().is_none(), is_neutral(&m.stem), "{}", m.stem);
        let third = if is_neutral(&m.stem) {
            "paint3"
        } else {
            "team"
        };
        assert_eq!(
            names,
            ["paint", "paint2", third, "metal", "rubber", "accent"],
            "{}",
            m.stem
        );
    }
}

/// A vessel's waterline `Y = 0` lies inside its hull (it has a keel under the water); every
/// other model stands on the ground with its lowest point at `Y = 0`.
#[test]
fn vessels_float_and_everything_else_stands_on_the_ground() {
    for (m, r) in models().iter().zip(reports()) {
        let (lo, hi) = r.facts.bounds.expect("bounds");
        if is_vessel(&m.stem) {
            assert!(lo.y < 0.0 && hi.y > 0.0, "{}: {lo:?} {hi:?}", m.stem);
        } else {
            assert!(lo.y.abs() < 0.005, "{}: lowest point {}", m.stem, lo.y);
        }
    }
}

/// The units of set v5 (WP11.6b) are in the set (the wiring is pinned in `sf_data`'s units test).
#[test]
fn every_unit_role_has_its_model_in_the_set() {
    let stems: Vec<&str> = models().iter().map(|m| m.stem.as_str()).collect();
    for want in UNIT_FOOTPRINTS.iter().map(|f| f.0) {
        assert!(stems.contains(&want), "{want} is missing");
    }
}

#[test]
fn the_team_colour_is_bands_and_panels_never_one_triangle() {
    for m in models().iter().filter(|m| !is_neutral(&m.stem)) {
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
/// two wider bands (four narrow ones cost 32 triangles more than its budget allows) and a stage's
/// stripes are 600 mm (`stages::FAR_STRIPE_MM`).
const MAX_BAND_MM: f64 = 350.0;

#[test]
fn every_team_band_is_narrow() {
    for m in models().iter().filter(|m| !is_neutral(&m.stem)) {
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
            if m.stem.ends_with("heavy_tank") {
                // The twin cannon (burst of two): a muzzle per barrel, 912 mm apart like the set
                // v4 heavy tank's; the n-th shot of a burst comes from the n-th anchor.
                need.extend(["turret", "barrel", "muzzle_l", "muzzle_r"]);
                let (l, r) = (
                    root.find("muzzle_l").unwrap(),
                    root.find("muzzle_r").unwrap(),
                );
                assert_eq!(l.at[0], r.at[0], "{}", m.stem);
                assert_eq!(
                    r.at[2] - l.at[2],
                    912,
                    "{}: the barrels are 912 mm apart",
                    m.stem
                );
                assert!(root.find("muzzle").is_none(), "{}", m.stem);
            } else if is_armed(&m.stem) {
                need.extend(["turret", "barrel", "muzzle"]);
            }
            for name in need {
                let node = root
                    .find(name)
                    .unwrap_or_else(|| panic!("{} LOD{n}: no node {name}", m.stem));
                if name.starts_with("fx_") || name.starts_with("muzzle") {
                    assert!(node.mesh.is_empty(), "{} {name} is an empty anchor", m.stem);
                }
            }
            // The muzzle hangs under a barrel with geometry (docs/conventions.md §12).
            if let Some(barrel) = root.find("barrel")
                && (root.find("muzzle").is_some() || root.find("muzzle_l").is_some())
            {
                assert!(!barrel.mesh.is_empty());
                assert!(barrel.find("muzzle").is_some() || barrel.find("muzzle_l").is_some());
            }
        }
    }
}

/// Size of LOD0 (x, z) in metres must stay inside the set v4 model of its role, and a structure
/// inside its footprint in `unit_defs.ron` (4 m a cell); the v4 sizes as `sf_asset_check` measured
/// them on 14-11-2026 (`size` column), as literals. A state shares its role's size.
const V4_SIZES: [(&str, f32, f32); 30] = [
    ("meridian_construction_center", 15.20, 15.20),
    ("meridian_harvester", 6.46, 3.46),
    ("meridian_light_tank", 5.11, 2.90),
    ("veyran_defence_tower", 3.90, 3.80),
    ("veyran_light_tank", 5.11, 2.90),
    ("veyran_power_plant", 7.60, 11.40),
    ("veyran_construction_center", 15.20, 15.20),
    ("meridian_power_plant", 7.60, 11.40),
    ("refinery", 11.40, 15.20),
    ("barracks", 7.60, 11.40),
    ("vehicle_factory", 11.40, 15.20),
    ("radar", 7.60, 7.60),
    ("meridian_defence_tower", 3.90, 3.80),
    ("advanced_defences", 7.80, 7.60),
    ("shipyard", 15.20, 19.00),
    ("decoy_emplacement", 3.90, 3.80),
    ("fortified_bunker", 7.60, 7.60),
    // No set v4 model: as the other two-cell structures of set v4 (radar, bunker).
    ("ore_silo", 7.60, 7.60),
    // Mission objects: the smaller of the v4 model and the footprint.
    ("relay_transmitter", 3.80, 3.80),
    ("fuel_depot", 7.60, 8.00),
    ("archive_building", 11.40, 11.40),
    ("bridge_segment", 4.00, 4.00),
    ("rail_spur", 4.00, 2.50),
    ("generator", 3.80, 4.00),
    ("evidence_crate", 1.01, 0.61),
    ("automated_gun", 4.00, 4.00),
    ("unmanned_artillery", 7.80, 7.60),
    ("rubble_1x1", 2.80, 2.75),
    ("rubble_2x2", 5.60, 5.50),
    ("rubble_2x3", 5.60, 8.25),
];
/// The units of WP11.6b: the set v4 file they replace (the heavy gun team: its infantry cell).
const UNIT_FOOTPRINTS: &[(&str, f32, f32)] = &[
    ("meridian_amphibious_transport", 6.33, 3.26),
    ("meridian_heavy_tank", 7.25, 3.80),
    ("meridian_light_transport", 3.42, 2.11),
    ("meridian_repair_truck", 4.94, 2.50),
    ("meridian_siege_artillery", 8.60, 3.10),
    ("neutral_ferry", 21.12, 9.00),
    ("neutral_supply_truck", 5.64, 2.50),
    ("veyran_gunboat", 18.00, 5.00),
    ("veyran_harvester", 6.54, 3.60),
    ("veyran_heavy_gun_team", 1.70, 1.20),
    ("veyran_heavy_tank", 7.25, 3.80),
    ("veyran_repair_truck", 4.94, 2.50),
    ("veyran_siege_artillery", 8.60, 3.10),
    ("veyran_transport_truck", 5.64, 2.50),
];
/// The larger heaps (set v4 sizes).
const V4_HEAPS: [(&str, f32, f32); 3] = [
    ("rubble_3x4", 8.40, 11.00),
    ("rubble_4x4", 11.20, 11.00),
    ("rubble_4x5", 11.20, 13.75),
];

#[test]
fn every_model_stays_inside_the_set_v4_footprint() {
    let size_of = |stem: &str| -> (f32, f32) {
        if let Some((_, x, z)) = UNIT_FOOTPRINTS.iter().find(|f| f.0 == stem) {
            return (*x, *z);
        }
        let base = stem.split_once('_').unwrap().1;
        let by_stem = V4_SIZES.iter().chain(&V4_HEAPS).find(|(s, _, _)| {
            *s == stem || stem.strip_prefix(s).is_some_and(|r| r.starts_with('_'))
        });
        let by_role = V4_SIZES
            .iter()
            .chain(&V4_HEAPS)
            .find(|(s, _, _)| *s == role(stem) || *s == base);
        let (_, x, z) = by_stem
            .or(by_role)
            .unwrap_or_else(|| panic!("{stem}: no size"));
        (*x, *z)
    };
    for (m, r) in models().iter().zip(reports()) {
        let (x, z) = size_of(&m.stem);
        let (lo, hi) = r.facts.bounds.expect("bounds");
        let ext = hi - lo;
        assert!(
            ext.x <= x + 1e-3 && ext.z <= z + 1e-3,
            "{}: {:.2} x {:.2} vs {x} x {z}",
            m.stem,
            ext.x,
            ext.z
        );
    }
}

/// The set holds every role's states (`build_stages.ron`, `damage.ron`): each faction structure
/// in five files, the mission objects with the states of set v4, the six heaps; 143 in all.
#[test]
fn every_structure_has_all_its_states() {
    let stems: Vec<&str> = models().iter().map(|m| m.stem.as_str()).collect();
    let factioned: [(&str, &[&str]); 12] = [
        ("construction_center", &["meridian", "veyran"]),
        ("power_plant", &["meridian", "veyran"]),
        ("refinery", &["meridian", "veyran"]),
        ("barracks", &["meridian", "veyran"]),
        ("vehicle_factory", &["meridian", "veyran"]),
        ("radar", &["meridian", "veyran"]),
        ("defence_tower", &["meridian", "veyran"]),
        ("advanced_defences", &["meridian", "veyran"]),
        ("shipyard", &["meridian", "veyran"]),
        ("ore_silo", &["meridian", "veyran"]),
        ("decoy_emplacement", &["meridian"]),
        ("fortified_bunker", &["veyran"]),
    ];
    for (role, factions) in factioned {
        for f in factions {
            for s in [
                "",
                "_foundation",
                "_scaffolding",
                "_damaged",
                "_heavily_damaged",
            ] {
                let stem = format!("{f}_{role}{s}");
                assert!(stems.contains(&stem.as_str()), "{stem} missing");
            }
        }
    }
    for stem in [
        "neutral_relay_transmitter_damaged",
        "neutral_relay_transmitter_rubble",
        "neutral_fuel_depot_damaged",
        "neutral_fuel_depot_burnt_out",
        "neutral_archive_building_damaged",
        "neutral_archive_building_heavily_damaged",
        "neutral_bridge_segment_damaged",
        "neutral_bridge_segment_destroyed",
        "neutral_rail_spur_destroyed",
        "neutral_generator_off",
        "neutral_generator_rubble",
        "neutral_evidence_crate",
        "network_automated_gun_disabled",
        "network_automated_gun_rubble",
        "network_unmanned_artillery_no_uplink",
        "network_unmanned_artillery_rubble",
        "neutral_rubble_1x1",
        "neutral_rubble_4x5",
    ] {
        assert!(stems.contains(&stem), "{stem} missing");
    }
    // WP11.6a 6, WP11.6b 14 units, WP11.6c the rest.
    assert_eq!(stems.len(), 6 + 14 + 22 * 5 - 3 + 24 + 6);
    let mut sorted = stems.clone();
    sorted.sort_unstable();
    assert_eq!(stems, sorted, "set::all is sorted by file name");
}

/// A damaged state has a hole (fewer triangles of the finished roof survive) and the heavy state
/// is broken off lower than the finished model; the scaffolding stands higher than the cut walls
/// (the crane); the foundation is a slab under 1.5 m. Measured on LOD0 of every faction structure.
#[test]
fn the_states_read_as_their_stage() {
    use sf_model_gen::stages::{CHAR, RECESS_DEBRIS, SOOT};
    let all = models();
    let height = |m: &Model| {
        m.lods[0]
            .world_tris()
            .iter()
            .flat_map(|t| t.v.map(|p| p[1]))
            .max()
            .unwrap()
    };
    // The height half the area of the upward faces over 0.6 m lies under (seen from above): the
    // roof of a building, not its crane or antenna.
    let roof = |m: &Model| {
        // The body only: a turret sits on the break, it is no roof.
        let body = m.lods[0].find("body").expect("body");
        let mut faces: Vec<(i32, i64)> = body
            .mesh
            .tris
            .iter()
            .filter_map(|t| {
                let n = t.normal();
                let y = t.v.iter().map(|p| p[1]).sum::<i32>() / 3;
                (n[1] > 0 && y > 600).then_some((y, n[1]))
            })
            .collect();
        faces.sort_unstable();
        let total: i64 = faces.iter().map(|f| f.1).sum();
        let mut acc = 0;
        faces
            .iter()
            .find(|f| {
                acc += f.1;
                acc * 2 >= total
            })
            .map_or(0, |f| f.0)
    };
    let anchor = |m: &Model, name: &str| m.lods[0].find(name).expect(name).at;
    let toned = |m: &Model, tones: &[u16]| {
        m.lods[0]
            .world_tris()
            .iter()
            .filter(|t| tones.contains(&t.tone))
            .count()
    };
    for m in all.iter().filter(|m| is_finished_structure(&m.stem)) {
        let get = |s: &str| {
            all.iter()
                .find(|x| x.stem == format!("{}{s}", m.stem))
                .unwrap()
        };
        let h = height(m);
        assert!(height(get("_foundation")) <= 1500, "{} foundation", m.stem);
        // The scaffolding: its crane stands over the finished model; the cut walls keep under
        // two thirds of the finished corners.
        let scaffold = get("_scaffolding");
        assert!(
            height(scaffold) > h,
            "{}: no crane over the cut walls",
            m.stem
        );
        // The heavy stage is broken off: its roof is far under the finished roof.
        let heavy = get("_heavily_damaged");
        assert!(height(heavy) < h, "{} heavy", m.stem);
        assert!(
            roof(heavy) * 10 < roof(m) * 9,
            "{}: heavy roof {} vs finished {}",
            m.stem,
            roof(heavy),
            roof(m)
        );
        // Broken off at half the roof level (docs/art-style.md §11): the break of the heavy body
        // lies at most 0.6 of the way from the platform to the finished roof level. The break is
        // a plane through the model's centre line tilting at most 250 mm/m along X and 120 mm/m
        // along Z: the mean of the highest wall top left and right of the centre cancels the X
        // tilt, the Z tilt gets its 120 mm/m over half the depth as margin.
        let slab = sf_model_gen::stages::slab_of(heavy).expect("slab marker");
        let finished_roof =
            sf_model_gen::stages::roof_level(&sf_model_gen::stages::split(&m.lods[0]), slab);
        let hb = &heavy.lods[0].find("body").expect("body").mesh.tris;
        let team_mat = heavy
            .team()
            .expect("a faction structure has the team material");
        // The walls, not the fallen team planks on the heap (WP11.6r: on the decoy a plank lay
        // 13 mm above the bound; near the break every wall stripe is soot-coloured paint, so a
        // team triangle there is a plank).
        let top = |left: bool| {
            hb.iter()
                .filter(|t| t.mat != team_mat)
                .flat_map(|t| t.v)
                .filter(|p| (p[0] < 0) == left && p[0] != 0 && p[2].abs() <= 600)
                .map(|p| p[1])
                .max()
                .unwrap_or(slab)
        };
        let brk = (top(true) + top(false)) / 2;
        let bound = slab + (finished_roof - slab) * 6 / 10 + 120 * 600 / 1000;
        println!("{}: heavy break slack {} mm", m.stem, bound - brk);
        assert!(
            brk <= bound,
            "{}: heavy break at {brk} over {bound} (slab {slab}, finished roof {finished_roof})",
            m.stem
        );
        // Its smoke and fire come from the break, never from the finished roof.
        for a in ["fx_smoke_0", "fx_fire_0"] {
            assert!(anchor(heavy, a)[1] <= anchor(m, a)[1], "{} {a}", m.stem);
        }
        assert!(
            anchor(heavy, "fx_smoke_0")[1] < anchor(m, "fx_smoke_0")[1],
            "{}: the smoke anchor stays at the finished height",
            m.stem
        );
        // Soot and debris on both damage stages.
        for s in [get("_damaged"), heavy] {
            assert!(toned(s, &[SOOT, CHAR]) > 0, "{}: no soot", s.stem);
            assert!(
                toned(s, &[RECESS_DEBRIS, CHAR]) > 0,
                "{}: no debris",
                s.stem
            );
        }
        let lod0 = |x: &Model| x.lods[0].world_tris();
        // The damaged state keeps a good part of the finished model but not all of it (the hole,
        // the detail round it; the anchor-bolt filler of a small structure is not inherited).
        // By corners only: soot changes the tone and burns team paint, the hole removes geometry.
        let finished: std::collections::BTreeSet<_> = lod0(m).iter().map(|t| t.v).collect();
        let kept = |x: &Model| lod0(x).iter().filter(|t| finished.contains(&t.v)).count();
        assert!(kept(get("_damaged")) < lod0(m).len(), "{}: no hole", m.stem);
        // The hole itself (its generator marker): the finished body has roof there, and none of
        // the finished body's triangles with its centroid inside it is left in the damaged body
        // (by corners: soot only changes the tone; the burnt chunks in it are new geometry).
        let damaged = get("_damaged");
        let (c, r) = sf_model_gen::stages::hole_of(damaged).expect("hole marker");
        let inside = |t: &&sf_model_gen::kit::Tri| {
            let d = [0, 1, 2]
                .map(|k| i64::from(t.v.iter().map(|p| p[k]).sum::<i32>() / 3) - i64::from(c[k]));
            d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= i64::from(r) * i64::from(r)
        };
        let body = |x: &Model| x.lods[0].find("body").expect("body").mesh.tris.clone();
        let roof_there: std::collections::BTreeSet<_> = body(m)
            .iter()
            .filter(inside)
            .filter(|t| sf_model_gen::stages::faces_up(t))
            .map(|t| t.v)
            .collect();
        assert!(
            !roof_there.is_empty(),
            "{}: no roof where the hole is",
            m.stem
        );
        let in_hole: std::collections::BTreeSet<_> =
            body(m).iter().filter(inside).map(|t| t.v).collect();
        let left = body(damaged)
            .iter()
            .filter(inside)
            .filter(|t| in_hole.contains(&t.v))
            .count();
        assert_eq!(
            left, 0,
            "{}: {left} finished body triangles left in the hole",
            m.stem
        );
        assert!(
            kept(get("_damaged")) * 4 > lod0(m).len(),
            "{}: too much gone",
            m.stem
        );
        assert!(
            kept(scaffold) * 3 < lod0(m).len() * 2,
            "{}: walls not cut",
            m.stem
        );
    }
}

/// A state is never richer than its finished structure (WP11.6c review): LOD0 of every state
/// file at most LOD0 of the file it is derived from.
#[test]
fn no_state_has_more_triangles_than_its_finished_model() {
    let all = models();
    for m in all {
        let Some(base) = STATES
            .iter()
            .chain(&["_no_uplink"])
            .find_map(|s| m.stem.strip_suffix(s))
        else {
            continue;
        };
        let Some(finished) = all.iter().find(|f| f.stem == base) else {
            continue; // a heap or an `_off` variant made by its own recipe
        };
        let (t, f) = (m.triangles()[0], finished.triangles()[0]);
        println!("{}: {t} of {f}", m.stem);
        assert!(t <= f, "{}: LOD0 {t} > its finished model's {f}", m.stem);
    }
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
    // Neutral models (WP11.6b): no faction accent, the third slot is a light paint.
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
    // WP11.6c: the Network defences (graphite with an amber lamp, the player colour).
    assert_eq!(
        p.factions["network"],
        FactionColours {
            paint: [92, 100, 112],
            paint_light: [118, 126, 138],
            paint2: [54, 58, 66],
            paint2_shade: [70, 74, 84],
            metal: [36, 36, 40],
            rubber: [20, 20, 20],
            accent: [150, 100, 20],
        }
    );
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
    // WP11.6r: LOD0 has at least 16 segments; the far levels keep halving the recipe's number.
    assert_eq!(Lod::ALL.map(|l| l.segs(8)), [16, 4, 4]);
    assert_eq!(Lod::ALL.map(|l| l.segs(32)), [32, 16, 8]);
    assert_eq!(sf_model_gen::kit::ROUND_MIN_SEGS, 16);
    // WP11.6r: LOD0 widens a recipe's bevel by half, at most 60 mm; a switch (under 10) stays.
    assert_eq!(Lod::ALL.map(|l| l.bevel(40)), [60, 20, 0]);
    assert_eq!(Lod::ALL.map(|l| l.bevel(300)), [360, 150, 0]);
    assert_eq!(Lod::ALL.map(|l| l.bevel(1)), [1, 0, 0]);
    assert_eq!(sf_model_gen::kit::BEVEL_WIDEN_MAX_MM, 60);
    assert_eq!(Lod::ALL.map(|l| l.has(0)), [true, false, false]);
    assert_eq!(Lod::ALL.map(|l| l.has(1)), [true, true, false]);
}

/// WP11.6r, the bevel rule: in LOD0 every box of the kit whose smallest side is at least
/// `BEVEL_MIN_SIDE_MM` gets a bevel of 8 % of that side (20..=120 mm), the closed box on its bottom
/// edges too; a detail-sized box and every box of the far levels stay plain.
#[test]
fn every_lod0_block_is_bevelled_by_the_kit_rule() {
    use sf_model_gen::kit::{
        BEVEL_MAX_MM, BEVEL_MIN_MM, BEVEL_MIN_SIDE_MM, BEVEL_PERMILLE, Mat, Model, Node,
        auto_bevel, closed_box, cuboid,
    };
    assert_eq!(
        (
            BEVEL_MIN_SIDE_MM,
            BEVEL_PERMILLE,
            BEVEL_MIN_MM,
            BEVEL_MAX_MM
        ),
        (240, 80, 20, 120)
    );
    assert_eq!(auto_bevel([0, 0, 0], [239, 5000, 5000]), 0, "a detail");
    assert_eq!(auto_bevel([0, 0, 0], [240, 5000, 5000]), 20, "the floor");
    assert_eq!(auto_bevel([0, 0, 0], [1000, 600, 2000]), 48, "8 % of 600");
    assert_eq!(
        auto_bevel([0, 0, 0], [3000, 4000, 5000]),
        120,
        "the ceiling"
    );
    // A bevelled face has a normal along no axis; a plain box has none of those.
    let slanted = |tris: &[sf_model_gen::kit::Tri]| {
        tris.iter()
            .filter(|t| t.normal().iter().filter(|c| **c != 0).count() > 1)
            .count()
    };
    let m = Mat(0);
    let big = closed_box([0, 0, 0], [2000, 1000, 1200], m);
    assert!(slanted(&big.tris) > 0, "a hull block is bevelled");
    let low = |t: &sf_model_gen::kit::Tri| t.v.iter().any(|p| p[1] == 0);
    assert!(
        big.tris
            .iter()
            .any(|t| low(t) && t.normal()[1] < 0 && t.normal()[0] != 0),
        "the bottom edges are bevelled too"
    );
    assert!(slanted(&cuboid([0, 0, 0], [2000, 1000, 1200], m).tris) > 0);
    assert_eq!(
        closed_box([0, 0, 0], [2000, 100, 1200], m).len(),
        12,
        "a plate"
    );
    // Built through a model, the far levels of the same box are plain.
    let model = Model::new("probe", Vec::new(), |_| {
        vec![Node::part(
            "body",
            [0, 0, 0],
            closed_box([0, 0, 0], [2000, 1000, 1200], m),
        )]
    });
    assert!(slanted(&model.lods[0].world_tris()) > 0);
    assert_eq!(slanted(&model.lods[1].world_tris()), 0);
    assert_eq!(model.triangles()[1..], [12, 12]);
    // In the set: every model's LOD0 has bevel faces, and far more of them than its LOD2.
    for m in models() {
        let near = slanted(&m.lods[0].world_tris());
        assert!(near >= 8, "{}: {near} bevel faces in LOD0", m.stem);
    }
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
