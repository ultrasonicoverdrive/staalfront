//! Art set v5 (`assets/set_v5`, WP11.6a, made by `tools/sf_model_gen`) through the engine: every
//! file loads with the engine axes, has three levels with the parts and anchors its role needs,
//! the `set_v5` pack of `assets/config/models.ron` lists exactly the files of the folder, and the
//! renderer draws each model with its player colour.
//!
//! The render test also writes the contact sheets for the style check when `SF_SAVE_SHOTS=<dir>`
//! is set: every model x two player colours x two views (45 and 80 degrees down), two models per
//! row, one sheet per group (finished structures, build stages, damage stages, objects and
//! rubble, units). Adapter rules as in the other screenshot tests (software preferred, required on
//! CI).

use std::collections::BTreeSet;
use std::path::PathBuf;

use glam::camera::rh;
use glam::{Mat4, Vec3};
use sf_math::render_bridge::metres_to_cells;
use sf_math::{Facing, Vec2};
use sf_render::glb::{self, AxisConvention, LoadedModel, PackFile};
use sf_render::image::{self, Rgba8};
use sf_render::model::Joint;
use sf_render::{FrameView, Frustum, GpuOptions, Instancer, Motion, Renderer, Style, UnitId};

fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

#[test]
fn the_baked_shade_reaches_the_vertices_and_the_team_colour_stays_whole() {
    // COLOR_0 multiplies the material colour (the glb loader, WP11.6a): a model of five plain
    // materials gets far more than five colours; the team vertices keep weight 1 untouched.
    for file in &files() {
        let file = file.as_str();
        let m = load(file);
        let verts: Vec<[f32; 4]> = m.levels[0]
            .parts
            .iter()
            .flat_map(|p| p.mesh.vertices.iter().map(|v| v.color))
            .collect();
        let team: Vec<[f32; 4]> = verts.iter().copied().filter(|c| c[3] > 0.0).collect();
        // A neutral object has no player colour (WP11.6c: its third slot is `paint3`).
        assert_eq!(team.is_empty(), is_neutral(file), "{file}");
        assert!(
            team.iter().all(|c| *c == sf_render::mesh::Vertex::TEAM),
            "{file}: a team vertex was shaded"
        );
        let mut shades: Vec<[u32; 3]> = verts
            .iter()
            .filter(|c| c[3] == 0.0)
            .map(|c| [c[0].to_bits(), c[1].to_bits(), c[2].to_bits()])
            .collect();
        shades.sort_unstable();
        shades.dedup();
        assert!(shades.len() > 50, "{file}: {} colours", shades.len());
    }
}

/// Every model of the folder, sorted by file name (WP11.6c: 143 of them; the pack must list the
/// same, `the_folder_holds_the_models_and_the_pack_lists_them`).
fn files() -> Vec<String> {
    let mut on_disk: Vec<String> = std::fs::read_dir(assets().join("set_v5"))
        .expect("assets/set_v5 exists")
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".glb"))
        .collect();
    on_disk.sort();
    on_disk
}

/// The roles of the structures and mission objects (WP11.6c): smoke and fire anchors.
const STRUCTURES: [&str; 22] = [
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
    "evidence_crate",
    "automated_gun",
    "unmanned_artillery",
    "neutral_rubble_",
];

/// A structure, an object or a heap: smoke and fire anchors.
fn is_building(file: &str) -> bool {
    STRUCTURES.iter().any(|b| file.contains(b))
}

/// A wreck of a vehicle or a boat (WP11.6d): hull, turret and barrel parts and the smoke and
/// fire anchors, but no muzzle.
fn is_wreck(file: &str) -> bool {
    file.starts_with("neutral_wreck_")
}

/// Rocks, fences, cliff edges, ore field stages and trees (WP11.6d): one body, no anchors.
fn is_scenery(file: &str) -> bool {
    [
        "neutral_rock_",
        "neutral_fence",
        "neutral_cliff_edge",
        "neutral_ore_field_",
        "neutral_tree_",
    ]
    .iter()
    .any(|p| file.starts_with(p))
}

/// The state of a structure that has no gun: a building stage or a heap.
fn is_unarmed_state(file: &str) -> bool {
    ["_foundation", "_scaffolding", "_rubble"]
        .iter()
        .any(|s| file.ends_with(&format!("{s}.glb")))
}

/// Has a gun: a muzzle anchor and a turning turret and barrel.
fn is_armed(file: &str) -> bool {
    let armed = [
        "light_tank",
        "heavy_tank",
        "siege_artillery",
        "gunboat",
        "defence_tower",
        "advanced_defences",
        "decoy_emplacement",
        "automated_gun",
        "unmanned_artillery",
    ];
    armed.iter().any(|b| file.contains(b)) && !is_unarmed_state(file)
}

/// A neutral object has no player colour.
fn is_neutral(file: &str) -> bool {
    file.starts_with("neutral_")
}

/// The contact sheet a model goes on.
fn sheet_of(file: &str) -> &'static str {
    if is_wreck(file) || is_scenery(file) {
        "decor"
    } else if file.starts_with("neutral_") || file.starts_with("network_") {
        "objects"
    } else if ["_foundation.glb", "_scaffolding.glb"]
        .iter()
        .any(|s| file.ends_with(s))
    {
        "build_stages"
    } else if ["_damaged.glb", "_heavily_damaged.glb"]
        .iter()
        .any(|s| file.ends_with(s))
    {
        "damage_stages"
    } else if is_building(file) {
        "structures"
    } else {
        "units"
    }
}

fn load(file: &str) -> LoadedModel {
    glb::load_glb_file(&assets().join("set_v5").join(file), &AxisConvention::ENGINE)
        .unwrap_or_else(|e| panic!("{file}: {e}"))
}

#[test]
fn the_folder_holds_the_models_and_the_pack_lists_them() {
    let on_disk = files();
    // WP11.6a: 6; WP11.6b: 14 units; WP11.6c: 22 structures x 5 states less the 3 finished ones
    // of WP11.6a, 24 mission objects and states, 6 heaps.
    // WP11.6d: 14 decor and wreck files (4 wrecks, 2 rocks, fence, cliff edge, 4 ore field
    // stages, 2 broadleaf trees).
    // WP11.6i: the two trial riflemen.
    assert_eq!(on_disk.len(), 6 + 14 + 22 * 5 - 3 + 24 + 6 + 14 + 2);
    let config = std::fs::read_to_string(assets().join(glb::MODELS_CONFIG)).unwrap();
    let file: PackFile = ron::from_str(&config).unwrap();
    let spec = file
        .packs
        .get("set_v5")
        .expect("models.ron has the pack set_v5");
    assert_eq!(spec.dir, "set_v5");
    let listed: BTreeSet<&str> = spec.models.iter().map(|m| m.file.as_str()).collect();
    assert_eq!(listed, on_disk.iter().map(String::as_str).collect());
    let pack = glb::load_pack(&config, "set_v5", &assets()).expect("pack loads");
    assert_eq!(pack.set.models().len(), on_disk.len());
}

#[test]
fn every_model_loads_with_three_levels_parts_and_anchors() {
    for file in &files() {
        let file = file.as_str();
        let m = load(file);
        assert_eq!(m.levels.len(), 3, "{file}");
        let tris: Vec<usize> = m
            .levels
            .iter()
            .map(|l| l.parts.iter().map(|p| p.mesh.indices.len() / 3).sum())
            .collect();
        assert!(tris[0] > tris[1] && tris[1] > tris[2], "{file}: {tris:?}");
        for (n, level) in m.levels.iter().enumerate() {
            let names: Vec<&str> = level.anchors.iter().map(|a| a.name.as_str()).collect();
            let building = is_building(file) || is_wreck(file);
            assert_eq!(
                names.contains(&"fx_smoke_0") && names.contains(&"fx_fire_0"),
                building,
                "{file} LOD{n}: {names:?} (structures have smoke and fire anchors)"
            );
            if is_armed(file) {
                // The heavy tank's twin cannon has a muzzle anchor per barrel (WP11.6b).
                let muzzles: &[&str] = if file.contains("heavy_tank") {
                    &["muzzle_l", "muzzle_r"]
                } else {
                    &["muzzle"]
                };
                for m in muzzles {
                    assert!(names.contains(m), "{file} LOD{n}: {names:?}");
                }
                let joints: Vec<Joint> = level.parts.iter().map(|p| p.joint).collect();
                assert!(
                    joints.contains(&Joint::Yaw) && joints.contains(&Joint::Pitch),
                    "{file} LOD{n}: turret and barrel parts: {joints:?}"
                );
            }
        }
    }
}

/// WP11.6i: the trial riflemen load as figures that walk (six parts with a figure joint each, in
/// every level) and the walk moves their legs in the engine's pose; every other model of the set
/// does not walk.
#[test]
fn the_trial_riflemen_load_as_figures_that_walk() {
    use sf_render::model::{MAX_PARTS, PartAngles, RigPart, pose_parts};
    use sf_render::walk::{WalkConfig, walk_pose};
    for file in &files() {
        let m = load(file);
        let walks = m.levels[0]
            .parts
            .iter()
            .any(|p| matches!(p.joint, Joint::Figure(_)));
        assert_eq!(walks, file.ends_with("_rifleman.glb"), "{file}");
        if !walks {
            continue;
        }
        for level in &m.levels {
            assert_eq!(level.parts.len(), 6, "{file}");
            assert!(
                level
                    .parts
                    .iter()
                    .all(|p| matches!(p.joint, Joint::Figure(_))),
                "{file}"
            );
        }
        let rig: Vec<RigPart> = m.levels[0]
            .parts
            .iter()
            .map(|p| RigPart {
                mesh: sf_render::MeshId(0),
                parent: None,
                rest: p.rest,
                joint: p.joint,
            })
            .collect();
        let mut world = [Mat4::IDENTITY; MAX_PARTS];
        let angles = PartAngles::default().with_walk(walk_pose(0.25, 1.0, &WalkConfig::embedded()));
        pose_parts(Mat4::IDENTITY, &rig, angles, &mut world);
        // The left foot (0.9 m below the hip) is forward, the right one back, both on the ground.
        let foot_l = world[0].transform_point3(Vec3::new(0.0, -0.9, 0.0));
        let foot_r = world[1].transform_point3(Vec3::new(0.0, -0.9, 0.0));
        assert!(
            foot_l.x > 0.3 && foot_r.x < -0.3,
            "{file}: {foot_l} {foot_r}"
        );
        assert!(foot_l.y.abs() < 1e-4 && foot_r.y.abs() < 1e-4, "{file}");
    }
}

const TW: u32 = 320;
const TH: u32 = 240;

fn renderer() -> Renderer {
    let mut r = Renderer::headless(TW, TH, &GpuOptions::software())
        .or_else(|software_err| {
            if std::env::var_os("CI").is_some() {
                return Err(software_err);
            }
            Renderer::headless(
                TW,
                TH,
                &GpuOptions {
                    backends: sf_render::wgpu::Backends::PRIMARY,
                    ..GpuOptions::default()
                },
            )
        })
        .unwrap_or_else(|e| panic!("NO GPU ADAPTER AVAILABLE: {e}"));
    r.set_scene(&[]);
    r
}

/// A view from `elevation_deg` down, from the front-right, framing a model of radius `radius`: the
/// whole bounding sphere fits the 40° vertical field (WP11.6r: at 2.3 radii long models were cut
/// off), and seen from above the model's length runs across the wide side of the tile.
fn view(centre: Vec3, radius: f32, elevation: f32) -> Mat4 {
    // cos/sin of the azimuth (35 degrees from +X towards +Z) and of the elevation.
    let (ca, sa) = (0.819_f32, 0.574_f32);
    let (ce, se) = if elevation > 60.0 {
        (0.174_f32, 0.985_f32)
    } else {
        (0.707_f32, 0.707_f32)
    };
    // sin(20°) = 0.342: the distance at which the sphere just fits, plus a margin.
    let d = radius / 0.342 * 1.04;
    let eye = centre + Vec3::new(ca * ce, se, sa * ce) * d;
    let up = if elevation > 60.0 { -Vec3::Z } else { Vec3::Y };
    let proj =
        rh::proj::directx::perspective(40_f32.to_radians(), TW as f32 / TH as f32, 0.1, d * 4.0);
    proj * rh::view::look_at_mat4(eye, centre, up)
}

#[test]
fn every_model_is_drawn_in_its_player_colour_and_the_contact_sheet_is_written_on_request() {
    let names = files();
    let models: Vec<LoadedModel> = names.iter().map(|f| load(f)).collect();
    let set = glb::model_set(&models, &[60.0, 30.0]).unwrap();
    // Two known player colours: blue and red (sRGB).
    let style = Style::embedded()
        .with_palette(&[
            ("blue".into(), [40, 90, 230]),
            ("red".into(), [220, 40, 40]),
        ])
        .expect("palette");
    let mut inst = Instancer::new(style.clone(), (64.0, 64.0));
    inst.set_models(set);
    let mut r = renderer();
    let at = (128.0_f32, 128.0_f32);
    let mut tiles: Vec<Rgba8> = Vec::new();
    for (shape, model) in models.iter().enumerate() {
        let b = glb::bounds_of(model);
        let ext = b.max - b.min;
        let radius = (ext.length() * 0.5).max(1.0);
        let mid = (b.min + b.max) * 0.5;
        let centre = Vec3::new(at.0 + mid.x, mid.y, at.1 + mid.z);
        let mut counts = [[0usize; 2]; 2];
        for player in [0u8, 1] {
            for elevation in [45.0, 80.0] {
                let p = Vec2::new(metres_to_cells(at.0), metres_to_cells(at.1));
                let unit =
                    Motion::from_fixed(shape as u8, player, p, p, Facing::new(0), Facing::new(0))
                        .with_id(UnitId::from_parts(1, 0));
                inst.update_tick([unit]);
                let vp = view(centre, radius, elevation);
                let frustum = Frustum::from_view_proj(vp);
                inst.build_frame(&FrameView::models_only(&frustum, 1.0), |_, _| 0.0);
                r.set_view_proj(vp);
                r.upload_units(&inst);
                let img = r.render_to_image().expect("render");
                // Pixels where blue, resp. red, clearly dominates.
                for (hue, count) in counts[player as usize].iter_mut().enumerate() {
                    *count += (0..TH)
                        .flat_map(|y| (0..TW).map(move |x| (x, y)))
                        .filter(|&(x, y)| {
                            let [r, g, b, _] = img.pixel(x, y).map(i32::from);
                            if hue == 0 {
                                b > r + 50 && b > g + 30
                            } else {
                                r > g + 70 && r > b + 70
                            }
                        })
                        .count();
                }
                tiles.push(img);
            }
        }
        // The team panels take the player colour: blue shows for the blue player and red for
        // the red one, far more than the other way round (the Veyran crimson trim is small).
        let [blue_p0, red_p0] = counts[0];
        let [blue_p1, red_p1] = counts[1];
        println!(
            "{}: blue {blue_p0}/{blue_p1}, red {red_p0}/{red_p1}",
            names[shape]
        );
        if is_neutral(&names[shape]) {
            // Neither player colour shows on a neutral object: the same pixels for both players.
            assert!(
                blue_p0.abs_diff(blue_p1) < 200 && red_p0.abs_diff(red_p1) < 200,
                "{}: the player colour changed a neutral model",
                names[shape]
            );
            continue;
        }
        assert!(
            blue_p0 > blue_p1 + 400,
            "{}: blue {blue_p0} vs {blue_p1}",
            names[shape]
        );
        assert!(
            red_p1 > red_p0 + 400,
            "{}: red {red_p1} vs {red_p0}",
            names[shape]
        );
    }
    if let Some(dir) = std::env::var_os("SF_SAVE_SHOTS") {
        // One sheet per group; two models per row, each as four full-size tiles (blue 45, blue
        // 80, red 45, red 80; WP11.6r: full size, the half-size tiles hid the facets).
        let (w, h) = (TW, TH);
        for group in [
            "structures",
            "build_stages",
            "damage_stages",
            "objects",
            "decor",
            "units",
        ] {
            let picked: Vec<usize> = (0..names.len())
                .filter(|&i| sheet_of(&names[i]) == group)
                .collect();
            if picked.is_empty() {
                continue;
            }
            let cols = 8u32;
            let rows = (picked.len() as u32).div_ceil(2);
            let mut px = vec![0u8; (w * cols * h * rows * 4) as usize];
            for (slot, &i) in picked.iter().enumerate() {
                for k in 0..4 {
                    let tile = &tiles[i * 4 + k];
                    let (cx, cy) = ((slot as u32 % 2) * 4 + k as u32, slot as u32 / 2);
                    for y in 0..h {
                        for x in 0..w {
                            let p = tile.pixel(x, y);
                            let o = (((cy * h + y) * w * cols + cx * w + x) * 4) as usize;
                            px[o..o + 4].copy_from_slice(&p);
                        }
                    }
                }
            }
            let sheet = Rgba8::new(w * cols, h * rows, px);
            let path = PathBuf::from(&dir).join(format!("set_v5_{group}.png"));
            image::write_png(&path, &sheet).expect("write the contact sheet");
        }
    }
}
