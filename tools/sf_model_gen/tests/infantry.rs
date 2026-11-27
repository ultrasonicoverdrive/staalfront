//! The trial riflemen of WP11.6i (`units_infantry`): per figure the triangles per level, the team
//! share, the six figure nodes at their joints with the rifle in `arm_r`, the height of the set v3
//! rifleman, and what the engine's loader makes of them (a figure that walks) next to the set v3
//! figures (one body that does not).

use std::path::PathBuf;

use sf_asset_check::Checker;
use sf_model_gen::kit::{Model, Node};
use sf_model_gen::units_infantry::{
    HIP_Y, JOINTS, NECK_Y, SHOULDER_Y, WAIST_Y, meridian_rifleman, veyran_rifleman,
};
use sf_render::glb::{self, AxisConvention};
use sf_render::model::{Joint, Limb};

fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn figures() -> [(Model, [usize; 3]); 2] {
    // Pinned triangle counts per level (the infantry budget is 1500 / 250 / 60, LOD0 >= 400).
    [
        (meridian_rifleman(), [1452, 196, 56]),
        (veyran_rifleman(), [1046, 196, 56]),
    ]
}

#[test]
fn the_triangles_per_level_are_pinned_and_inside_the_infantry_budget() {
    for (m, want) in figures() {
        let t = m.triangles();
        assert_eq!(t, want, "{}", m.stem);
        assert!(
            t[0] >= 400 && t[0] <= 1500 && t[1] <= 250 && t[2] <= 60,
            "{t:?}"
        );
        assert!(t[0] > t[1] && t[1] > t[2], "{}: not reduced {t:?}", m.stem);
    }
}

#[test]
fn the_team_share_is_20_to_40_percent_in_every_level() {
    let checker = Checker::embedded();
    for (m, _) in figures() {
        let bytes = sf_model_gen::glb::write(&m);
        let r = checker.check_bytes(&format!("{}.glb", m.stem), &bytes);
        assert!(r.findings.is_empty(), "{}: {:?}", m.stem, r.findings);
        let shares = &r.facts.team_share;
        assert_eq!(shares.len(), 3);
        for (n, s) in shares.iter().enumerate() {
            let s = s.expect("seen from above");
            println!("{} LOD{n}: team {:.1} %", m.stem, s * 100.0);
            assert!((0.20..=0.40).contains(&s), "{} LOD{n}: team {s}", m.stem);
        }
    }
}

/// The six figure nodes are the root's children with their origins at the joints ([`JOINTS`]),
/// none empty in any level, and the rifle is in `arm_r` like set v3: its muzzle 0.7 m or more
/// ahead of the shoulder, its receiver metal.
#[test]
fn the_figure_nodes_sit_at_their_joints_and_the_rifle_is_in_the_right_arm() {
    assert_eq!(
        JOINTS,
        [
            ("leg_l", [0, 900, -105]),
            ("leg_r", [0, 900, 105]),
            ("torso", [0, 960, 0]),
            ("head", [0, 1450, 0]),
            ("arm_l", [0, 1400, -220]),
            ("arm_r", [0, 1400, 220]),
        ]
    );
    const { assert!(HIP_Y < WAIST_Y && WAIST_Y < SHOULDER_Y && SHOULDER_Y < NECK_Y) };
    for (m, _) in figures() {
        let metal = sf_model_gen::palette::slot::METAL;
        for root in &m.lods {
            let names: Vec<&str> = root.children.iter().map(|c| c.name.as_str()).collect();
            assert_eq!(
                names,
                ["leg_l", "leg_r", "torso", "head", "arm_l", "arm_r"],
                "{}",
                root.name
            );
            for (name, at) in JOINTS {
                let n: &Node = root.find(name).unwrap();
                assert_eq!(n.at, at, "{} {name}", root.name);
                assert!(!n.mesh.is_empty() && n.children.is_empty(), "{name}");
            }
            let arm_r = root.find("arm_r").unwrap();
            let reach = arm_r.mesh.tris.iter().flat_map(|t| t.v).map(|p| p[0]).max();
            assert!(
                reach.unwrap() >= 700,
                "{}: the rifle reaches {reach:?}",
                root.name
            );
            assert!(
                arm_r.mesh.tris.iter().any(|t| t.mat == metal),
                "{}: no metal in the rifle",
                root.name
            );
            for other in ["arm_l", "torso", "head"] {
                let n = root.find(other).unwrap();
                let x = n.mesh.tris.iter().flat_map(|t| t.v).map(|p| p[0]).max();
                assert!(x.unwrap() < 400, "{} {other}: no rifle there", root.name);
            }
        }
    }
}

/// The new figure is as tall as the set v3 rifleman it replaces, within 5 %.
#[test]
fn the_height_is_within_5_percent_of_the_set_v3_rifleman() {
    let checker = Checker::embedded();
    for (m, _) in figures() {
        let file = format!("{}.glb", m.stem);
        let v3 = std::fs::read(assets().join("set_v3").join(&file)).unwrap();
        let (_, hi3) = checker.check_bytes(&file, &v3).facts.bounds.unwrap();
        let ours = checker.check_bytes(&file, &sf_model_gen::glb::write(&m));
        let (lo, hi) = ours.facts.bounds.unwrap();
        assert!(lo.y.abs() < 0.005);
        let ratio = hi.y / hi3.y;
        println!("{}: {:.3} m against {:.3} m (set v3)", m.stem, hi.y, hi3.y);
        assert!((0.95..=1.05).contains(&ratio), "{}: {ratio}", m.stem);
    }
}

/// The engine's loader: the new figures keep their six nodes as parts with a figure joint each
/// (they walk), the set v3 riflemen are one body with no joint (they do not).
#[test]
fn the_engine_walks_the_new_figures_and_not_the_set_v3_ones() {
    for (m, _) in figures() {
        let loaded = glb::load_glb(&sf_model_gen::glb::write(&m), &AxisConvention::ENGINE).unwrap();
        for level in &loaded.levels {
            let joints: Vec<Joint> = level.parts.iter().map(|p| p.joint).collect();
            assert_eq!(
                joints,
                [
                    Limb::LegL,
                    Limb::LegR,
                    Limb::Torso,
                    Limb::Head,
                    Limb::ArmL,
                    Limb::ArmR
                ]
                .map(Joint::Figure),
                "{}",
                m.stem
            );
            let hip = level.parts[0].rest.w_axis;
            assert!(
                (hip.y - 0.9).abs() < 1e-6 && (hip.z + 0.105).abs() < 1e-6,
                "{hip}"
            );
        }
        let set = glb::model_set(&[loaded], &[60.0, 30.0]).unwrap();
        assert!(set.model(0).walks(), "{}", m.stem);
    }
    for file in ["meridian_rifleman.glb", "veyran_rifleman.glb"] {
        let v3 = glb::load_glb_file(&assets().join("set_v3").join(file), &AxisConvention::ENGINE)
            .unwrap();
        for level in &v3.levels {
            assert!(
                level.parts.iter().all(|p| p.joint == Joint::Fixed),
                "set v3 {file}"
            );
            assert_eq!(level.parts.len(), 1, "set v3 {file}: one body");
        }
        let set = glb::model_set(&[v3], &[60.0, 30.0]).unwrap();
        assert!(!set.model(0).walks(), "set v3 {file}");
    }
}
