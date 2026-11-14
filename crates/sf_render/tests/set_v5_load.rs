//! Art set v5 (`assets/set_v5`, WP11.6a, made by `tools/sf_model_gen`) through the engine: every
//! file loads with the engine axes, has three levels with the parts and anchors its role needs,
//! the `set_v5` pack of `assets/config/models.ron` lists exactly the files of the folder, and the
//! renderer draws each model with its player colour.
//!
//! The render test also writes the contact sheet for the style check when `SF_SAVE_SHOTS=<dir>`
//! is set: six models x two player colours x two views (45 and 80 degrees down), one row per
//! model. Adapter rules as in the other screenshot tests (software preferred, required on CI).

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
    for file in FILES {
        let m = load(file);
        let verts: Vec<[f32; 4]> = m.levels[0]
            .parts
            .iter()
            .flat_map(|p| p.mesh.vertices.iter().map(|v| v.color))
            .collect();
        let team: Vec<[f32; 4]> = verts.iter().copied().filter(|c| c[3] > 0.0).collect();
        assert!(!team.is_empty(), "{file}");
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

/// The six models, sorted by file name.
const FILES: [&str; 6] = [
    "meridian_construction_center.glb",
    "meridian_harvester.glb",
    "meridian_light_tank.glb",
    "veyran_defence_tower.glb",
    "veyran_light_tank.glb",
    "veyran_power_plant.glb",
];

fn load(file: &str) -> LoadedModel {
    glb::load_glb_file(&assets().join("set_v5").join(file), &AxisConvention::ENGINE)
        .unwrap_or_else(|e| panic!("{file}: {e}"))
}

#[test]
fn the_folder_holds_the_six_models_and_the_pack_lists_them() {
    let mut on_disk: Vec<String> = std::fs::read_dir(assets().join("set_v5"))
        .expect("assets/set_v5 exists")
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".glb"))
        .collect();
    on_disk.sort();
    assert_eq!(on_disk, FILES);
    let config = std::fs::read_to_string(assets().join(glb::MODELS_CONFIG)).unwrap();
    let file: PackFile = ron::from_str(&config).unwrap();
    let spec = file
        .packs
        .get("set_v5")
        .expect("models.ron has the pack set_v5");
    assert_eq!(spec.dir, "set_v5");
    let listed: BTreeSet<&str> = spec.models.iter().map(|m| m.file.as_str()).collect();
    assert_eq!(listed, FILES.into_iter().collect());
    let pack = glb::load_pack(&config, "set_v5", &assets()).expect("pack loads");
    assert_eq!(pack.set.models().len(), 6);
}

#[test]
fn every_model_loads_with_three_levels_parts_and_anchors() {
    for file in FILES {
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
            assert!(
                names.contains(&"fx_smoke_0") && names.contains(&"fx_fire_0"),
                "{file} LOD{n}: {names:?}"
            );
            let armed = file.contains("light_tank") || file.contains("defence_tower");
            if armed {
                assert!(names.contains(&"muzzle"), "{file} LOD{n}: {names:?}");
                let joints: Vec<Joint> = level.parts.iter().map(|p| p.joint).collect();
                assert!(
                    joints.contains(&Joint::Yaw) && joints.contains(&Joint::Pitch),
                    "{file} LOD{n}: turret and barrel parts: {joints:?}"
                );
            }
        }
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

/// A view from `elevation_deg` down, from the front-right, framing a model of radius `radius`.
fn view(centre: Vec3, radius: f32, elevation: f32) -> Mat4 {
    // cos/sin of the azimuth (35 degrees from +X towards +Z) and of the elevation.
    let (ca, sa) = (0.819_f32, 0.574_f32);
    let (ce, se) = if elevation > 60.0 {
        (0.174_f32, 0.985_f32)
    } else {
        (0.707_f32, 0.707_f32)
    };
    let d = radius * 2.3;
    let eye = centre + Vec3::new(ca * ce, se, sa * ce) * d;
    let up = if elevation > 60.0 { Vec3::X } else { Vec3::Y };
    let proj =
        rh::proj::directx::perspective(40_f32.to_radians(), TW as f32 / TH as f32, 0.1, d * 4.0);
    proj * rh::view::look_at_mat4(eye, centre, up)
}

#[test]
fn every_model_is_drawn_in_its_player_colour_and_the_contact_sheet_is_written_on_request() {
    let models: Vec<LoadedModel> = FILES.iter().map(|f| load(f)).collect();
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
        let centre = Vec3::new(at.0, b.max.y * 0.4, at.1);
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
            FILES[shape]
        );
        assert!(
            blue_p0 > blue_p1 + 400,
            "{}: blue {blue_p0} vs {blue_p1}",
            FILES[shape]
        );
        assert!(
            red_p1 > red_p0 + 400,
            "{}: red {red_p1} vs {red_p0}",
            FILES[shape]
        );
    }
    if let Some(dir) = std::env::var_os("SF_SAVE_SHOTS") {
        let cols = 4u32;
        let rows = models.len() as u32;
        let mut px = vec![0u8; (TW * cols * TH * rows * 4) as usize];
        for (i, tile) in tiles.iter().enumerate() {
            let (cx, cy) = (i as u32 % cols, i as u32 / cols);
            for y in 0..TH {
                for x in 0..TW {
                    let o = (((cy * TH + y) * TW * cols + cx * TW + x) * 4) as usize;
                    px[o..o + 4].copy_from_slice(&tile.pixel(x, y));
                }
            }
        }
        let sheet = Rgba8::new(TW * cols, TH * rows, px);
        let path = PathBuf::from(dir).join("set_v5_contact_sheet_raw.png");
        image::write_png(&path, &sheet).expect("write the contact sheet");
    }
}
