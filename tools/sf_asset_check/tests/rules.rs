//! One test per rule family, each on a tiny `.glb` built in `common`.

mod common;

use common::{
    Model, Part, add_second_figure, decor, infantry, patch_json, scene_per_lod, tank, wreck,
};
use sf_asset_check::class::Class;
use sf_asset_check::{Checker, FileReport, Status};

fn check(file: &str, m: &Model) -> FileReport {
    Checker::embedded().check_bytes(file, &m.bytes())
}

fn assert_clean(r: &FileReport) {
    assert!(r.findings.is_empty(), "{:#?}", r.findings);
    assert_eq!(r.status(), Status::Pass);
}

/// The messages of the findings of one rule, for `contains` checks.
fn messages(r: &FileReport, rule: &str) -> String {
    r.findings
        .iter()
        .filter(|f| f.rule == rule)
        .map(|f| f.message.as_str())
        .collect::<Vec<_>>()
        .join(" | ")
}

// ---------------------------------------------------------------- the good cases

#[test]
fn a_correct_infantry_model_has_no_findings() {
    let r = check(
        "meridian_rifleman.glb",
        &infantry("meridian_rifleman", 0.30),
    );
    assert_clean(&r);
    assert_eq!(r.facts.triangles, [72, 72, 24]);
    assert_eq!(r.facts.budget, Some([1500, 250, 60]));
    assert_eq!(r.facts.materials, 2);
    for share in &r.facts.team_share {
        assert!((share.unwrap() - 0.30).abs() < 0.02, "{share:?}");
    }
}

#[test]
fn a_correct_wreck_decor_and_tank_have_no_findings() {
    assert_clean(&check(
        "neutral_wreck_medium_vehicle.glb",
        &wreck("neutral_wreck_medium_vehicle"),
    ));
    assert_clean(&check("neutral_tree_pine.glb", &decor("neutral_tree_pine")));
    assert_clean(&check(
        "veyran_light_tank.glb",
        &tank("veyran_light_tank", 0.30),
    ));
}

// ---------------------------------------------------------------- names and classes

#[test]
fn a_bad_file_name_is_a_failure() {
    let m = infantry("meridian_rifleman", 0.30);
    for bad in ["rifleman.glb", "Meridian_rifleman.glb", "meridian_.glb"] {
        let r = check(bad, &m);
        assert!(r.failed("file-name"), "{bad}: {:#?}", r.findings);
    }
}

#[test]
fn lod_roots_must_be_named_after_the_file() {
    let r = check("meridian_scout.glb", &infantry("meridian_rifleman", 0.30));
    assert!(r.failed("lod-roots"));
    assert!(messages(&r, "lod-roots").contains("meridian_scout_LOD<n>"));
}

#[test]
fn an_unknown_role_cannot_get_a_budget_class() {
    let r = check(
        "meridian_spaceship.glb",
        &infantry("meridian_spaceship", 0.30),
    );
    assert!(r.failed("class"));
    assert_eq!(r.facts.class, None);
    // Nothing else is judged without a class, and nothing else is wrong.
    assert_eq!(r.findings.len(), 1, "{:#?}", r.findings);
}

#[test]
fn fences_cliff_edges_and_rubble_heaps_are_decor_with_the_decor_budget() {
    for stem in [
        "neutral_fence",
        "neutral_cliff_edge",
        "neutral_rubble_1x1",
        "neutral_rubble_4x5",
    ] {
        let r = check(&format!("{stem}.glb"), &decor(stem));
        assert_clean(&r);
        assert_eq!(r.facts.class, Some(Class::Decor), "{stem}");
        assert_eq!(r.facts.budget, Some([1500, 500, 150]), "{stem}");
    }
    // Over the decor budget still fails (LOD2 may have 150 triangles).
    let mut m = decor("neutral_rubble_2x2");
    m.part_mut(2, "trunk").reps = 13;
    let r = check("neutral_rubble_2x2.glb", &m);
    assert!(r.failed("budget"), "{:#?}", r.findings);
    // A fence is neutral: under a faction prefix it is only a remark.
    let r = check("veyran_fence.glb", &decor("veyran_fence"));
    assert!(!r.failed("class") && r.has("faction"), "{:#?}", r.findings);
    // Names that only start like a decor role are still unknown.
    for stem in ["neutral_fencing", "neutral_cliff", "neutral_rubbles"] {
        let r = check(&format!("{stem}.glb"), &decor(stem));
        assert!(r.failed("class"), "{stem}: {:#?}", r.findings);
    }
}

#[test]
fn a_role_for_the_other_faction_is_a_warning() {
    // The sniper is a Meridian role in unit_defs.ron.
    let r = check("veyran_sniper.glb", &infantry("veyran_sniper", 0.30));
    assert!(r.has("faction") && !r.failed("faction"));
    assert_eq!(r.status(), Status::Warn);
}

// ---------------------------------------------------------------- LODs

#[test]
fn a_missing_lod_is_a_failure_and_an_extra_one_a_warning() {
    let mut m = infantry("meridian_rifleman", 0.30);
    m.lods.pop();
    let r = check("meridian_rifleman.glb", &m);
    assert!(r.failed("lod-roots"));
    assert!(messages(&r, "lod-roots").contains("meridian_rifleman_LOD2 is missing"));

    let mut m = infantry("meridian_rifleman", 0.30);
    m.lods.push(m.lods[2].clone());
    let r = check("meridian_rifleman.glb", &m);
    assert!(
        r.has("lod-roots") && !r.failed("lod-roots"),
        "{:#?}",
        r.findings
    );
}

#[test]
fn a_scene_per_lod_is_judged_exactly_like_one_scene() {
    let checker = Checker::embedded();
    let cases = [
        ("meridian_rifleman", infantry("meridian_rifleman", 0.30)),
        ("veyran_light_tank", tank("veyran_light_tank", 0.30)),
        (
            "neutral_wreck_medium_vehicle",
            wreck("neutral_wreck_medium_vehicle"),
        ),
        ("neutral_tree_pine", decor("neutral_tree_pine")),
    ];
    for (stem, model) in cases {
        let file = format!("{stem}.glb");
        let one = model.bytes();
        let per_lod = scene_per_lod(&one);
        assert_ne!(one, per_lod, "{stem}: the fixture really has three scenes");
        assert!(
            String::from_utf8_lossy(&per_lod).contains("\"name\":\"LOD2\""),
            "{stem}"
        );
        let (a, b) = (
            checker.check_bytes(&file, &one),
            checker.check_bytes(&file, &per_lod),
        );
        assert_clean(&b);
        assert_eq!(a, b, "{stem}: same facts and findings in both layouts");
    }
}

#[test]
fn a_broken_scene_per_lod_file_is_a_failure_with_the_reason() {
    let checker = Checker::embedded();
    let bytes = infantry("meridian_rifleman", 0.30).bytes();
    // The root node numbers of scenes LOD1 and LOD2 in the fixture.
    let root = |json: &str, n: u32| {
        let head = format!("\"name\":\"LOD{n}\",\"nodes\":[");
        let from = json.find(&head).unwrap() + head.len();
        json[from..from + json[from..].find(']').unwrap()].to_owned()
    };
    let per_lod = scene_per_lod(&bytes);
    // A scene that holds another level's root, and one with two roots.
    for (swap, needle) in [
        (true, "scene LOD1 holds the root"),
        (false, "exactly one root"),
    ] {
        let broken = patch_json(&per_lod, |json| {
            let (x, y) = (root(&json, 1), root(&json, 2));
            let (lod1, lod2) = (
                format!("\"name\":\"LOD1\",\"nodes\":[{x}]"),
                format!("\"name\":\"LOD2\",\"nodes\":[{y}]"),
            );
            if swap {
                json.replace(&lod1, "LOD1_TMP")
                    .replace(&lod2, &format!("\"name\":\"LOD2\",\"nodes\":[{x}]"))
                    .replace("LOD1_TMP", &format!("\"name\":\"LOD1\",\"nodes\":[{y}]"))
            } else {
                json.replace(&lod1, &format!("\"name\":\"LOD1\",\"nodes\":[{x},{y}]"))
            }
        });
        let r = checker.check_bytes("meridian_rifleman.glb", &broken);
        assert!(r.failed("glb"), "{needle}: {:#?}", r.findings);
        assert!(messages(&r, "glb").contains(needle), "{needle}: {r:#?}");
    }
    // A file with two scenes named LOD0 and LOD1 lacks LOD2, as always.
    let mut m = infantry("meridian_rifleman", 0.30);
    m.lods.pop();
    let r = checker.check_bytes("meridian_rifleman.glb", &scene_per_lod(&m.bytes()));
    assert!(r.failed("lod-roots"), "{:#?}", r.findings);
    assert!(messages(&r, "lod-roots").contains("meridian_rifleman_LOD2 is missing"));
}

#[test]
fn every_lod_has_the_same_nodes_and_the_same_origin() {
    let mut m = infantry("meridian_rifleman", 0.30);
    m.remove_part(1, "arm_l");
    let r = check("meridian_rifleman.glb", &m);
    assert!(r.failed("lod-structure"));
    assert!(messages(&r, "lod-structure").contains("arm_l"));

    let mut m = infantry("meridian_rifleman", 0.30);
    m.lods[2].origin = [0.0, 0.0, 0.5];
    let r = check("meridian_rifleman.glb", &m);
    assert!(r.failed("lod-origin"), "{:#?}", r.findings);
}

#[test]
fn a_lod_of_another_size_is_a_warning_only() {
    let mut m = infantry("meridian_rifleman", 0.30);
    m.part_mut(2, "torso").boxes = vec![([-0.6, 0.0, -0.2], [0.6, 1.7, 0.2])];
    let r = check("meridian_rifleman.glb", &m);
    assert!(
        r.has("lod-size") && !r.failed("lod-size"),
        "{:#?}",
        r.findings
    );
    assert!(messages(&r, "lod-size").contains("LOD2"));
}

// ---------------------------------------------------------------- budgets

#[test]
fn infantry_over_the_budget_fails_on_the_level_that_is_over() {
    let mut m = infantry("meridian_rifleman", 0.30);
    m.part_mut(0, "torso").reps = 130; // 1,560 + 60 triangles
    let r = check("meridian_rifleman.glb", &m);
    assert!(r.failed("budget"));
    let said = messages(&r, "budget");
    assert!(
        said.contains("LOD0 has 1620") && !said.contains("LOD1"),
        "{said}"
    );
}

#[test]
fn the_decor_budget_is_150_for_lod2_and_the_infantry_budget_60() {
    // 12 triangles a box: 12 boxes = 144, 13 = 156.
    let mut d = decor("neutral_tree_pine");
    for lod in 0..3 {
        d.part_mut(lod, "trunk").reps = 12;
    }
    assert_clean(&check("neutral_tree_pine.glb", &d));
    for lod in 0..3 {
        d.part_mut(lod, "trunk").reps = 13;
    }
    let r = check("neutral_tree_pine.glb", &d);
    assert!(r.failed("budget"));
    assert!(messages(&r, "budget").contains("LOD2 has 156"));

    // The same 72 triangles in LOD2 of a figure are over 60.
    let mut m = infantry("meridian_rifleman", 0.30);
    m.lods[2] = m.lods[1].clone();
    let r = check("meridian_rifleman.glb", &m);
    assert!(
        messages(&r, "budget").contains("LOD2 has 72"),
        "{:#?}",
        r.findings
    );
    // ... but fine for a wreck, which has the decor budget.
    let mut w = wreck("neutral_wreck_light_vehicle");
    for lod in 0..3 {
        w.part_mut(lod, "hull").reps = 5; // 5 hull boxes + turret + barrel = 84
    }
    assert!(!check("neutral_wreck_light_vehicle.glb", &w).has("budget"));
}

#[test]
fn two_figures_get_twice_the_infantry_budget() {
    let mut m = infantry("veyran_demolition_team", 0.30);
    add_second_figure(&mut m);
    m.part_mut(0, "torso").reps = 130; // 1,692 triangles in LOD0
    let r = check("veyran_demolition_team.glb", &m);
    assert!(r.facts.pair);
    assert_eq!(r.facts.budget, Some([3000, 500, 120]));
    assert!(!r.has("budget"), "{:#?}", r.findings);

    // One figure with the same triangles is over.
    let mut single = infantry("meridian_rifleman", 0.30);
    single.part_mut(0, "torso").reps = 135;
    assert!(check("meridian_rifleman.glb", &single).failed("budget"));
}

#[test]
fn the_demolition_team_must_have_its_second_figure() {
    // Named a two-figure role, but only one figure in the file.
    let r = check(
        "veyran_demolition_team.glb",
        &infantry("veyran_demolition_team", 0.30),
    );
    assert!(r.facts.pair);
    assert!(r.failed("nodes"));
    assert!(messages(&r, "nodes").contains("torso_2"));
}

// ---------------------------------------------------------------- materials and team

#[test]
fn more_than_six_materials_or_any_texture_fails() {
    let mut m = decor("neutral_tree_pine");
    for i in 1..6 {
        m.materials.push((format!("extra{i}"), [0.5; 3]));
    }
    assert_clean(&check("neutral_tree_pine.glb", &m)); // exactly 6
    m.materials.push(("one_too_many".into(), [0.5; 3]));
    let r = check("neutral_tree_pine.glb", &m);
    assert!(r.failed("materials"));
    assert!(messages(&r, "materials").contains("7 materials"));

    let bytes = decor("neutral_tree_pine").bytes();
    let textured = patch_json(&bytes, |j| {
        j.replace(
            "\"buffers\"",
            "\"images\":[{\"uri\":\"a.png\"}],\"textures\":[{\"source\":0}],\"buffers\"",
        )
    });
    let r = Checker::embedded().check_bytes("neutral_tree_pine.glb", &textured);
    assert!(r.failed("textures"), "{:#?}", r.findings);
    assert!(
        r.failed("engine-load"),
        "the engine loader refuses textures too"
    );
}

#[test]
fn an_emissive_material_is_a_warning() {
    let bytes = decor("neutral_tree_pine").bytes();
    let lit = patch_json(&bytes, |j| {
        j.replacen(
            "\"pbrMetallicRoughness\"",
            "\"emissiveFactor\":[1.0,0.5,0.0],\"pbrMetallicRoughness\"",
            1,
        )
    });
    let r = Checker::embedded().check_bytes("neutral_tree_pine.glb", &lit);
    assert!(
        r.has("materials") && !r.failed("materials"),
        "{:#?}",
        r.findings
    );
    assert!(messages(&r, "materials").contains("bark"));
}

#[test]
fn neutral_objects_have_no_team_material_and_factions_must() {
    let r = check("neutral_civilian.glb", &infantry("neutral_civilian", 0.30));
    assert!(r.failed("team"));
    assert!(messages(&r, "team").contains("neutral"));

    let mut m = infantry("meridian_rifleman", 0.30);
    m.materials.truncate(1);
    for lod in 0..3 {
        m.part_mut(lod, "head").mat = 0;
    }
    let r = check("meridian_rifleman.glb", &m);
    assert!(r.failed("team"));
    assert!(messages(&r, "team").contains("needs a material named team"));
}

#[test]
fn the_team_material_must_show_in_lod0() {
    let mut m = infantry("meridian_rifleman", 0.30);
    m.part_mut(0, "head").mat = 0;
    let r = check("meridian_rifleman.glb", &m);
    assert!(r.failed("team"));
    assert!(messages(&r, "team").contains("no team triangles"));
}

#[test]
fn team_must_be_twenty_to_forty_percent_of_the_top_view_of_lod0() {
    for (share, ok) in [
        (0.05, false),
        (0.19, false),
        (0.25, true),
        (0.38, true),
        (0.60, false),
    ] {
        let r = check(
            "meridian_rifleman.glb",
            &infantry("meridian_rifleman", share),
        );
        assert_eq!(
            !r.failed("team-share"),
            ok,
            "share {share}: {:#?}",
            r.findings
        );
    }
    let r = check(
        "meridian_rifleman.glb",
        &infantry("meridian_rifleman", 0.05),
    );
    assert!(messages(&r, "team-share").contains("LOD0"));
}

#[test]
fn team_outside_the_range_in_a_smaller_lod_is_a_warning() {
    let mut m = infantry("meridian_rifleman", 0.30);
    m.part_mut(2, "head").boxes = vec![([-0.3, 1.7, -0.2], [0.18, 1.75, 0.2])]; // 80 %
    let r = check("meridian_rifleman.glb", &m);
    assert!(
        r.has("team-share") && !r.failed("team-share"),
        "{:#?}",
        r.findings
    );
    assert!(messages(&r, "team-share").contains("LOD2"));
}

// ---------------------------------------------------------------- bounds

#[test]
fn the_lowest_point_must_be_on_the_ground() {
    let at = |y: f32| {
        let mut m = infantry("meridian_rifleman", 0.30);
        for lod in &mut m.lods {
            lod.origin = [0.0, y, 0.0];
        }
        check("meridian_rifleman.glb", &m)
    };
    assert!(!at(0.0).has("ground"));
    assert!(!at(0.04).has("ground"), "within 5 cm");
    let sunk = at(-0.1);
    assert!(sunk.has("ground") && !sunk.failed("ground"), "a warning");
    assert!(at(0.5).failed("ground"), "floating");
    assert!(at(-0.5).failed("ground"), "buried");
}

#[test]
fn a_vessel_floats_on_y_zero() {
    let at = |stem: &str, y: f32| {
        let mut m = wreck(stem);
        for lod in &mut m.lods {
            lod.origin = [0.0, y, 0.0];
        }
        check(&format!("{stem}.glb"), &m)
    };
    // Waterline inside the hull (Y -0.5 to 0.5): fine for a vessel ...
    let v = at("neutral_wreck_vessel", -0.5);
    assert!(v.facts.vessel);
    assert!(!v.has("ground"), "{:#?}", v.findings);
    // ... a warning when the whole hull is above the water ...
    assert!(at("neutral_wreck_vessel", 0.0).has("ground"));
    // ... and a failure for a land wreck that sinks into the ground.
    assert!(at("neutral_wreck_light_vehicle", -0.5).failed("ground"));
}

#[test]
fn vehicles_are_longer_along_x_and_infantry_stand_up() {
    let mut w = wreck("neutral_wreck_medium_vehicle");
    for lod in 0..3 {
        w.part_mut(lod, "hull").boxes = vec![([-1.0, 0.0, -2.0], [1.0, 1.0, 2.0])];
    }
    let r = check("neutral_wreck_medium_vehicle.glb", &w);
    assert!(r.failed("orientation"));
    assert!(messages(&r, "orientation").contains("+X"));

    let mut m = infantry("meridian_rifleman", 0.30);
    // Lying on its back: 1.7 m long along X, 0.55 m high.
    m.part_mut(0, "torso").boxes = vec![([-0.85, 0.0, -0.2], [0.85, 0.5, 0.2])];
    m.part_mut(0, "head").boxes = vec![([-0.85, 0.5, -0.2], [-0.5, 0.55, 0.2])];
    let r = check("meridian_rifleman.glb", &m);
    assert!(r.failed("orientation"));
    assert!(messages(&r, "orientation").contains("stand along +Y"));
}

#[test]
fn an_ore_field_fits_one_cell_and_the_origin_is_under_the_middle() {
    let mut f = decor("neutral_ore_field_stage_04");
    for lod in 0..3 {
        f.part_mut(lod, "trunk").boxes = vec![([-2.25, 0.0, -2.0], [2.25, 0.5, 2.0])];
    }
    let r = check("neutral_ore_field_stage_04.glb", &f);
    assert!(
        r.has("footprint") && !r.failed("footprint"),
        "{:#?}",
        r.findings
    );

    let mut off = decor("neutral_tree_pine");
    for lod in 0..3 {
        off.part_mut(lod, "trunk").boxes = vec![([0.0, 0.0, -1.0], [2.0, 6.0, 1.0])];
    }
    let r = check("neutral_tree_pine.glb", &off);
    assert!(r.has("origin") && !r.failed("origin"), "{:#?}", r.findings);
}

// ---------------------------------------------------------------- node names

#[test]
fn infantry_need_their_six_nodes() {
    let mut m = infantry("meridian_rifleman", 0.30);
    for lod in 0..3 {
        m.remove_part(lod, "arm_r");
    }
    let r = check("meridian_rifleman.glb", &m);
    assert!(r.failed("nodes"));
    assert!(messages(&r, "nodes").contains("arm_r"));
}

#[test]
fn a_wreck_needs_a_hull_and_empty_fx_nodes() {
    let mut m = wreck("neutral_wreck_medium_vehicle");
    for lod in 0..3 {
        m.remove_part(lod, "fx_smoke_1");
    }
    let r = check("neutral_wreck_medium_vehicle.glb", &m);
    assert!(r.failed("fx-nodes"));
    assert!(messages(&r, "fx-nodes").contains("fx_smoke_"));

    let mut m = wreck("neutral_wreck_medium_vehicle");
    for lod in 0..3 {
        *m.part_mut(lod, "fx_fire_1") = Part::new("fx_fire_1").boxed([0.0; 3], [0.1; 3]);
    }
    let r = check("neutral_wreck_medium_vehicle.glb", &m);
    assert!(messages(&r, "fx-nodes").contains("must be an empty node"));

    let mut m = wreck("neutral_wreck_medium_vehicle");
    for lod in 0..3 {
        m.remove_part(lod, "hull");
        m.lods[lod]
            .parts
            .push(Part::new("body").boxed([-2.0, 0.0, -1.0], [2.0, 1.0, 1.0]));
    }
    let r = check("neutral_wreck_medium_vehicle.glb", &m);
    assert!(messages(&r, "nodes").contains("no node called hull"));
}

#[test]
fn turret_hangs_under_hull_and_barrel_under_turret() {
    // A wreck's turret may lie off the hull, directly under the root (the
    // delivered heavy wreck): allowed, like a missing turret.
    let mut m = wreck("neutral_wreck_heavy_vehicle");
    for lod in 0..3 {
        let turret = m.part_mut(lod, "turret").clone();
        m.remove_part(lod, "turret");
        m.lods[lod].parts.push(turret);
    }
    assert_clean(&check("neutral_wreck_heavy_vehicle.glb", &m));

    // A living vehicle's turret must hang under the hull.
    let mut t = tank("veyran_light_tank", 0.30);
    for lod in 0..3 {
        t.remove_part(lod, "turret");
        t.lods[lod]
            .parts
            .push(Part::new("turret").child(Part::new("barrel")));
    }
    let r = check("veyran_light_tank.glb", &t);
    assert!(r.failed("nodes"));
    assert!(messages(&r, "nodes").contains("turret hangs under the root"));

    // A barrel must hang under the turret, also on a wreck.
    let mut w = wreck("neutral_wreck_medium_vehicle");
    for lod in 0..3 {
        let barrel = w.part_mut(lod, "barrel").clone();
        w.remove_part(lod, "barrel");
        w.part_mut(lod, "hull").children.push(barrel);
    }
    let r = check("neutral_wreck_medium_vehicle.glb", &w);
    assert!(r.failed("nodes"));
    assert!(messages(&r, "nodes").contains("barrel hangs under hull"));
}

#[test]
fn a_wreck_may_lose_its_turret_but_a_vehicle_keeps_the_nodes() {
    let mut w = wreck("neutral_wreck_light_vehicle");
    for lod in 0..3 {
        w.remove_part(lod, "turret");
    }
    assert_clean(&check("neutral_wreck_light_vehicle.glb", &w));

    let mut t = tank("veyran_light_tank", 0.30);
    for lod in 0..3 {
        t.remove_part(lod, "turret");
    }
    let r = check("veyran_light_tank.glb", &t);
    assert!(r.has("nodes") && !r.failed("nodes"), "{:#?}", r.findings);
}

#[test]
fn a_turret_or_barrel_that_is_a_plain_mesh_node_is_fine() {
    // The engine loader makes a node named turret or barrel a rigged part
    // whatever it holds, so a childless mesh node needs no empty frame node
    // around it (the old `engine-parts` rule is gone).
    let mut t = tank("veyran_light_tank", 0.30);
    for lod in 0..3 {
        t.remove_part(lod, "turret");
        // A turret that is a mesh and carries a barrel that is a plain mesh.
        let mut turret = Part::new("turret")
            .at([0.0, 1.0, 0.0])
            .boxed([-0.5, 0.0, -0.5], [0.5, 0.3, 0.5]);
        turret.children.push(
            Part::new("barrel")
                .at([0.3, 0.2, 0.0])
                .boxed([0.0, -0.05, -0.05], [1.0, 0.05, 0.05]),
        );
        t.part_mut(lod, "hull").children.push(turret);
    }
    let r = check("veyran_light_tank.glb", &t);
    assert!(!r.has("engine-parts"), "{:#?}", r.findings);
    assert!(!r.failed("nodes"), "{:#?}", r.findings);
    assert_clean(&r);

    // The same for the barrel of a wreck.
    let mut w = wreck("neutral_wreck_medium_vehicle");
    for lod in 0..3 {
        *w.part_mut(lod, "barrel") = Part::new("barrel")
            .boxed([0.0, -0.05, -0.05], [1.0, 0.05, 0.05])
            .at([0.5, 0.15, 0.0]);
    }
    let r = check("neutral_wreck_medium_vehicle.glb", &w);
    assert!(!r.has("engine-parts"), "{:#?}", r.findings);
    assert_clean(&r);
}

#[test]
fn degenerate_triangles_and_zero_normals_are_warnings_with_a_count_per_lod() {
    let mut d = decor("neutral_tree_pine");
    // A flat slab: of its 12 triangles the 8 on the four faces along X have no area.
    d.part_mut(1, "trunk").boxes = vec![([0.0, 0.0, -1.0], [0.0, 6.0, 1.0])];
    // A part whose 24 vertices have zero normals.
    *d.part_mut(0, "trunk") = Part::new("trunk")
        .boxed([-1.0, 0.0, -1.0], [1.0, 6.0, 1.0])
        .zero_normals();
    let r = check("neutral_tree_pine.glb", &d);
    assert!(r.has("degenerate") && !r.failed("degenerate"));
    assert!(
        messages(&r, "degenerate").contains("LOD1 8"),
        "{}",
        messages(&r, "degenerate")
    );
    assert!(
        messages(&r, "normals").contains("LOD0 24"),
        "{}",
        messages(&r, "normals")
    );
    // A clean model has neither.
    let clean = check("neutral_tree_pine.glb", &decor("neutral_tree_pine"));
    assert!(!clean.has("degenerate") && !clean.has("normals"));
}

// ---------------------------------------------------------------- broken files

#[test]
fn files_that_are_not_models_fail_without_panicking() {
    let r = Checker::embedded().check_bytes("meridian_rifleman.glb", b"not a model");
    assert!(r.failed("glb"));
    assert_eq!(r.status(), Status::Fail);

    let bytes = infantry("meridian_rifleman", 0.30).bytes();
    let r = Checker::embedded().check_bytes("meridian_rifleman.glb", &bytes[..bytes.len() / 2]);
    assert_eq!(r.status(), Status::Fail);

    let r =
        Checker::embedded().check_path(std::path::Path::new("/nonexistent/meridian_rifleman.glb"));
    assert!(r.failed("io"));
}
