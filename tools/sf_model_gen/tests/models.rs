//! The rules of the kit, per model of set v5 (WP11.6a, WP11.6c, docs/art-style.md): budgets per
//! level and strictly fewer triangles per level, the team share of the top-down view in 20-30 %
//! on many triangles, the nodes the engine and the checker need, sizes inside the set v4 models
//! they replace, and `sf_asset_check` with no finding at all. Every check runs on what the
//! generator itself builds and writes (`set::all`, `glb::write`); the reports are made once.

use std::sync::OnceLock;

use sf_asset_check::{Checker, FileReport};
use sf_model_gen::kit::{Lod, Model, TEAM};
use sf_model_gen::palette::{FactionColours, Palette};
use sf_model_gen::stages::{STAGE_FLOOR_PERMILLE, Stage};
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

/// The suffixes of the states (`build_stages.ron`, `damage.ron`, and the mission objects' own).
const STATES: [&str; 9] = [
    "_foundation",
    "_scaffolding",
    "_damaged",
    "_heavily_damaged",
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

/// A model with a gun: a muzzle under its barrel.
fn is_armed(stem: &str) -> bool {
    let armed = [
        "light_tank",
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
        [5000, 1500, 300]
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
/// vehicles get a little more detail but stay under the unit budget. WP11.6c: a state keeps a
/// share of its finished model ([`STAGE_FLOOR_PERMILLE`]), a mission object at least 1,000.
#[test]
fn buildings_use_their_detail_budget() {
    let all = models();
    for m in all {
        let t = m.triangles();
        if is_finished_structure(&m.stem) {
            assert!(
                (2500..=3500).contains(&t[0]),
                "{}: LOD0 {} not in 2500-3500",
                m.stem,
                t[0]
            );
        } else if let Some(stage) = Stage::ALL
            .into_iter()
            .find(|s| m.stem.ends_with(s.suffix()) && !is_neutral(&m.stem))
        {
            let base = m.stem.strip_suffix(stage.suffix()).unwrap();
            let finished = all
                .iter()
                .find(|f| f.stem == base)
                .expect("its finished model");
            let k = Stage::ALL.iter().position(|s| *s == stage).unwrap();
            let floor = finished.triangles()[0] * STAGE_FLOOR_PERMILLE[k] / 1000;
            assert!(t[0] >= floor, "{}: LOD0 {} < {floor}", m.stem, t[0]);
        } else if is_decor(&m.stem) {
            assert!((100..1500).contains(&t[0]), "{}: LOD0 {}", m.stem, t[0]);
        } else if is_building(&m.stem) {
            assert!((1000..=3500).contains(&t[0]), "{}: LOD0 {}", m.stem, t[0]);
        } else {
            assert!((1200..1500).contains(&t[0]), "{}: LOD0 {}", m.stem, t[0]);
        }
    }
}

/// The stage floors and the state list, as literals (they set what a state may cost).
#[test]
fn the_stage_rules_are_pinned() {
    assert_eq!(STAGE_FLOOR_PERMILLE, [300, 700, 850, 700]);
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
        assert_eq!(m.team().is_none(), is_neutral(&m.stem), "{}", m.stem);
        if is_neutral(&m.stem) {
            assert_eq!(m.materials[2].name, "paint3", "{}", m.stem);
        }
    }
}

#[test]
fn the_team_colour_is_bands_and_panels_never_one_triangle() {
    for m in models().iter().filter(|m| !is_neutral(&m.stem)) {
        let team = m.team().expect("a team material");
        assert_eq!(m.materials[team.0 as usize].name, TEAM);
        for (n, level) in m.lods.iter().enumerate() {
            let count = level.world_tris().iter().filter(|t| t.mat == team).count();
            assert!(count >= 12, "{} LOD{n}: {count} team triangles", m.stem);
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
            } else {
                // Vehicles smoke by the engine's rule (no anchors), like every other vehicle.
                assert!(root.find("fx_smoke_0").is_none(), "{}", m.stem);
                need.extend(["hull", "turret", "barrel"]);
            }
            if is_armed(&m.stem) {
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
    // No set v4 model: the footprint (2 x 2 cells).
    ("ore_silo", 8.00, 8.00),
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
/// The larger heaps (set v4 sizes).
const V4_HEAPS: [(&str, f32, f32); 3] = [
    ("rubble_3x4", 8.40, 11.00),
    ("rubble_4x4", 11.20, 11.00),
    ("rubble_4x5", 11.20, 13.75),
];

#[test]
fn every_model_stays_inside_the_set_v4_footprint() {
    let size_of = |stem: &str| -> (f32, f32) {
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
    assert_eq!(stems.len(), 6 + 22 * 5 - 3 + 24 + 6);
    let mut sorted = stems.clone();
    sorted.sort_unstable();
    assert_eq!(stems, sorted, "set::all is sorted by file name");
}

/// A damaged state has a hole (fewer triangles of the finished roof survive) and the heavy state
/// is broken off lower than the finished model; the scaffolding stands higher than the cut walls
/// (the crane); the foundation is a slab under 1.5 m. Measured on LOD0 of every faction structure.
#[test]
fn the_states_read_as_their_stage() {
    let all = models();
    let height = |m: &Model| {
        m.lods[0]
            .world_tris()
            .iter()
            .flat_map(|t| t.v.map(|p| p[1]))
            .max()
            .unwrap()
    };
    for m in all.iter().filter(|m| is_finished_structure(&m.stem)) {
        let get = |s: &str| {
            all.iter()
                .find(|x| x.stem == format!("{}{s}", m.stem))
                .unwrap()
        };
        let h = height(m);
        assert!(height(get("_foundation")) <= 1500, "{} foundation", m.stem);
        assert!(height(get("_heavily_damaged")) < h, "{} heavy", m.stem);
        let lod0 = |x: &Model| x.lods[0].world_tris();
        // The damaged state keeps most of the finished model but not all of it (the hole).
        let kept = lod0(get("_damaged"))
            .iter()
            .filter(|t| lod0(m).contains(t))
            .count();
        assert!(kept < lod0(m).len(), "{}: no hole", m.stem);
        assert!(kept * 10 > lod0(m).len() * 7, "{}: too much gone", m.stem);
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
