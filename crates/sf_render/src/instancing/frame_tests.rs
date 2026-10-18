//! Levels of detail per instance, models and icons per frame, picking per
//! zoom and the highlight (WP1.9). CPU only: no GPU involved.

use super::*;
use crate::camera::{Camera, CameraConfig, DVec2};
use crate::pick::{PickQuery, PickRay};
use sf_math::render_bridge::metres_to_cells;

const W: f32 = 1280.0;
const H: f32 = 720.0;

fn fx2(x: f32, z: f32) -> Vec2 {
    Vec2::new(metres_to_cells(x), metres_to_cells(z))
}

/// A unit standing still at `at`.
fn still(shape: u8, player: u8, id: u32, at: (f32, f32)) -> Motion {
    moving(shape, player, id, at, at)
}

fn moving(shape: u8, player: u8, id: u32, from: (f32, f32), to: (f32, f32)) -> Motion {
    Motion::from_fixed(
        shape,
        player,
        fx2(from.0, from.1),
        fx2(to.0, to.1),
        Facing::new(0),
        Facing::new(0),
    )
    .with_id(UnitId::from_parts(id, 0))
}

fn camera(zoom: f64, focus: (f64, f64)) -> Camera {
    let cfg = CameraConfig {
        map_size: DVec2::new(256.0, 256.0),
        ..CameraConfig::default()
    };
    let mut c = Camera::new(cfg, W as u32, H as u32);
    c.set_zoom(zoom);
    c.set_focus(DVec2::new(focus.0, focus.1));
    c
}

fn instancer_with(style: Style, motions: &[Motion]) -> Instancer {
    let mut inst = Instancer::new(style, (256.0, 256.0));
    inst.set_height_range(0.0, 30.0);
    inst.update_tick(motions.iter().copied());
    inst
}

fn instancer(motions: &[Motion]) -> Instancer {
    instancer_with(Style::embedded(), motions)
}

fn flat(_x: f32, _z: f32) -> f32 {
    0.0
}

/// Build a frame as the game does: the camera's frustum and eye, `blend` and `ui_scale` as given.
fn build(
    inst: &mut Instancer,
    cam: &Camera,
    alpha: f32,
    blend: Blend,
    ui_scale: f32,
    ground: impl Fn(f32, f32) -> f32,
) -> InstanceStats {
    let vp = cam.view_proj();
    let frustum = Frustum::from_view_proj(vp);
    let eye = EyeView::from_view_proj(vp, H);
    inst.build_frame(
        &FrameView::new(&frustum, alpha, eye, blend, ui_scale),
        ground,
    )
}

/// A query with the cursor over the pixel where the world point `p` is drawn.
fn query_over(cam: &Camera, p: Vec3, dx: f32, dy: f32) -> PickQuery {
    let vp = cam.view_proj();
    let (px, _) = project_px(&vp, p, [W, H]).expect("in front of the camera");
    PickQuery::new([px[0] + dx, px[1] + dy], vp, [W, H])
}

/// The level a record's model was drawn at.
fn level_of(inst: &Instancer, record: usize) -> usize {
    let rec = &inst.records[record];
    inst.models
        .model(rec.shape)
        .levels()
        .iter()
        .position(|l| l.mesh.0 == rec.model_mesh)
        .expect("the record's mesh is in its chain")
}

fn style_ron(shapes: &str, rest: &str) -> Style {
    Style::from_ron(&format!(
        "(palette: [(name: \"a\", srgb: (200, 30, 30)), (name: \"b\", srgb: (30, 30, 200))],
          shapes: [{shapes}], {rest})"
    ))
    .unwrap()
}

// ---------------------------------------------------------------------------
// Levels of detail
// ---------------------------------------------------------------------------

/// Cylinders in a row away from the eye: the farther, the coarser or equal;
/// all three levels occur; each unit has the level the plain threshold gives
/// its projected size; the counters add up.
#[test]
fn levels_follow_the_distance_per_instance() {
    let motions: Vec<_> = (0..40)
        .map(|i| still(1, 0, i, (5.0 + i as f32 * 6.0, 128.0)))
        .collect();
    let mut inst = instancer(&motions);
    let wide = camera(1.0, (128.0, 128.0)); // sees the whole row
    let frustum = Frustum::from_view_proj(wide.view_proj());
    let eye = EyeView::new(Vec3::new(0.0, 10.0, 128.0), H, 45_f32.to_radians());
    let stats = inst.build_frame(
        &FrameView::new(&frustum, 0.0, eye, Blend::MODELS, 1.0),
        flat,
    );
    assert_eq!(stats.visible, 40);
    let shape = inst.style().shape(1);
    let chain = inst.models().model(1).clone();
    let mut by_distance: Vec<(f32, usize)> = Vec::new();
    for (i, rec) in inst.records.iter().enumerate() {
        let middle = Vec3::new(rec.pos[0], 0.5 * shape.size[1], rec.pos[2]);
        let distance = (middle - eye.eye).length();
        let level = level_of(&inst, i);
        let px = eye.projected_px(shape.bounding_diameter(), distance);
        assert_eq!(level, usize::from(chain.ideal(px)), "{px} px");
        by_distance.push((distance, level));
    }
    by_distance.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(
        by_distance.windows(2).all(|w| w[0].1 <= w[1].1),
        "levels must not get finer with distance: {by_distance:?}"
    );
    assert_eq!(by_distance.first().unwrap().1, 0);
    assert_eq!(by_distance.last().unwrap().1, 2);
    // The counters add up: every drawn model is counted at exactly one level.
    assert_eq!(stats.per_lod.iter().sum::<u32>(), 40);
    assert_eq!(stats.models, 40);
    assert!(
        stats.per_lod[..3].iter().all(|&n| n > 0),
        "{:?}",
        stats.per_lod
    );
    assert_eq!(stats.per_lod[3], 0);
}

/// One list, one buffer and one draw call per (mesh kind, level): three
/// shapes with 1 + 3 + 3 levels make at most seven draws, however many units.
#[test]
fn the_draws_are_one_per_mesh_kind_and_level() {
    let motions: Vec<_> = (0..600)
        .map(|i| {
            still(
                (i % 3) as u8,
                (i % 4) as u8,
                i,
                (5.0 + (i / 3) as f32 * 1.2, 128.0),
            )
        })
        .collect();
    let mut inst = instancer(&motions);
    let wide = camera(1.0, (128.0, 128.0));
    let frustum = Frustum::from_view_proj(wide.view_proj());
    let eye = EyeView::new(Vec3::new(0.0, 10.0, 128.0), H, 45_f32.to_radians());
    let stats = inst.build_frame(
        &FrameView::new(&frustum, 0.0, eye, Blend::MODELS, 1.0),
        flat,
    );
    let non_empty = (0..inst.models().mesh_count())
        .filter(|&id| !inst.mesh_instances(MeshId(id as u16)).is_empty())
        .count() as u32;
    assert_eq!(stats.model_draw_calls, non_empty);
    assert_eq!(stats.draw_calls(), non_empty);
    assert!((5..=7).contains(&non_empty), "{non_empty} draw calls");
    assert_eq!(stats.icon_draw_calls, 0);
    // Lower levels submit fewer triangles than the same crowd at level 0.
    let full: u64 = (0..600)
        .map(|i| {
            let kind = inst.style().shape((i % 3) as u8).mesh;
            u64::from(inst.models().mesh_triangles(MeshId::of(kind)))
        })
        .sum();
    println!("triangles {} vs {full} at level 0", stats.model_triangles);
    assert!(stats.model_triangles < full);
    // The instances are where the counters say.
    let listed: usize = (0..inst.models().mesh_count())
        .map(|id| inst.mesh_instances(MeshId(id as u16)).len())
        .sum();
    assert_eq!(listed as u32, stats.models);
}

/// A unit at a switch distance with a camera that wobbles by a few percent:
/// with the style's hysteresis it keeps one level, without it the level
/// flickers (so the test can fail).
#[test]
fn a_unit_at_a_switch_distance_does_not_flicker_with_hysteresis() {
    let shape_size = "(1.2, 1.1, 1.2)";
    let shapes = format!(
        "(mesh: Cylinder, size: {shape_size},
          lods: [(segments: Some(16), min_px: 60.0), (segments: Some(8), min_px: 30.0), (segments: Some(5), min_px: 0.0)])"
    );
    let levels_changed = |hysteresis: f32| {
        let style = style_ron(&shapes, &format!("lod: (hysteresis: {hysteresis})"));
        let diameter = style.shape(0).bounding_diameter();
        let mut inst = instancer_with(style, &[still(0, 0, 0, (100.0, 100.0))]);
        let wide = camera(1.0, (128.0, 128.0));
        let frustum = Frustum::from_view_proj(wide.view_proj());
        let focal = 0.5 * H / (0.5 * 45_f32.to_radians()).tan();
        // The distance at which the unit is exactly 60 px big.
        let switch = diameter * focal / 60.0;
        let mut changes = 0;
        let mut last = None;
        for frame in 0..400 {
            let wobble = 1.0 + 0.05 * (frame as f32 * 0.9).sin();
            let eye = EyeView {
                eye: Vec3::new(100.0 + switch * wobble, 0.55, 100.0),
                focal_px: focal,
            };
            inst.build_frame(
                &FrameView::new(&frustum, 0.0, eye, Blend::MODELS, 1.0),
                flat,
            );
            let level = level_of(&inst, 0);
            if last.is_some_and(|l| l != level) {
                changes += 1;
            }
            last = Some(level);
        }
        changes
    };
    let (without, with) = (levels_changed(0.0), levels_changed(0.15));
    println!("level changes in 400 wobbling frames: {without} without hysteresis, {with} with");
    assert!(without > 100, "the control flickers: {without}");
    assert_eq!(with, 0);
}

/// The level a unit had is remembered by its entity slot, not by its place in
/// the list: when an earlier unit disappears, the others keep their level.
#[test]
fn the_hysteresis_state_follows_the_entity_slot() {
    let shapes = "(mesh: Cylinder, size: (1.2, 1.1, 1.2),
        lods: [(segments: Some(16), min_px: 60.0), (segments: Some(8), min_px: 30.0), (segments: Some(5), min_px: 0.0)])";
    let style = style_ron(shapes, "lod: (hysteresis: 0.2)");
    let diameter = style.shape(0).bounding_diameter();
    let focal = 0.5 * H / (0.5 * 45_f32.to_radians()).tan();
    // Slot 3 is very near (level 0), slot 7 is 52 px big: it holds level 1
    // only through the band (48..72 px around the 60 px switch).
    let at_px = |px: f32| diameter * focal / px;
    let near = still(0, 0, 3, (100.0, 100.0));
    let held = still(0, 0, 7, (110.0, 110.0));
    let mut inst = instancer_with(style, &[near, held]);
    let wide = camera(1.0, (128.0, 128.0));
    let frustum = Frustum::from_view_proj(wide.view_proj());
    let eye_for = |px: f32| EyeView {
        eye: Vec3::new(110.0 + at_px(px), 0.55, 110.0),
        focal_px: focal,
    };
    let frame = |inst: &mut Instancer, eye: EyeView| {
        inst.build_frame(
            &FrameView::new(&frustum, 0.0, eye, Blend::MODELS, 1.0),
            flat,
        );
    };
    // Come from far (level 2), then to 80 px (level 0 for the held unit).
    frame(&mut inst, eye_for(10.0));
    frame(&mut inst, eye_for(80.0));
    let slot = |inst: &Instancer, id: u32| {
        inst.records
            .iter()
            .position(|r| r.id.index() == id)
            .map(|i| level_of(inst, i))
    };
    assert_eq!(slot(&inst, 7), Some(0));
    // Back off to 52 px: inside the band, the held unit keeps level 0.
    frame(&mut inst, eye_for(52.0));
    assert_eq!(slot(&inst, 7), Some(0));
    // Now the near unit disappears; the held one moves to the front of the
    // list but keeps its remembered level.
    inst.update_tick([held]);
    frame(&mut inst, eye_for(52.0));
    assert_eq!(inst.records.len(), 1);
    assert_eq!(
        level_of(&inst, 0),
        0,
        "state must follow slot 7, not position 0"
    );
}

// ---------------------------------------------------------------------------
// Models and icons per frame
// ---------------------------------------------------------------------------

#[test]
fn the_blend_decides_what_is_built() {
    let motions: Vec<_> = (0..30)
        .map(|i| still((i % 3) as u8, (i % 4) as u8, i, (100.0 + i as f32, 100.0)))
        .collect();
    let mut inst = instancer(&motions);
    let cam = camera(0.3, (110.0, 100.0));
    // Models only: no icons at all.
    let s = build(&mut inst, &cam, 0.0, Blend::MODELS, 1.0, flat);
    assert_eq!((s.models, s.icons, s.icon_draw_calls), (30, 0, 0));
    assert!(inst.icon_instances().is_empty());
    // Icons only: no model instance, no model draw.
    let s = build(&mut inst, &cam, 0.0, Blend::ICONS, 1.0, flat);
    assert_eq!((s.models, s.icons), (0, 30));
    assert_eq!(
        (s.model_draw_calls, s.model_triangles, s.icon_draw_calls),
        (0, 0, 1)
    );
    assert_eq!(s.draw_calls(), 1, "strategic zoom is a single draw call");
    assert_eq!(s.per_lod, [0; MAX_LODS]);
    for id in 0..inst.models().mesh_count() {
        assert!(inst.mesh_instances(MeshId(id as u16)).is_empty());
    }
    assert_eq!(s.visible, 30, "the units are still counted as visible");
    // In between: both, for the same units, each carrying the model weight.
    let half = Blend::from_model_weight(0.4);
    let s = build(&mut inst, &cam, 0.0, half, 1.0, flat);
    assert_eq!((s.models, s.icons), (30, 30));
    assert!(s.model_draw_calls > 0 && s.icon_draw_calls == 1);
    let all_models =
        (0..inst.models().mesh_count()).flat_map(|id| inst.mesh_instances(MeshId(id as u16)));
    assert!(all_models.clone().count() == 30);
    assert!(all_models.into_iter().all(|i| i.color[3] == 0.4));
    assert!(inst.icon_instances().iter().all(|i| i.color[3] == 0.4));
    // Full models: the weight is exactly 1, the value that skips the dissolve.
    build(&mut inst, &cam, 0.0, Blend::MODELS, 1.0, flat);
    assert!(
        inst.instances(MeshKind::Cube)
            .iter()
            .all(|i| i.color[3] == 1.0)
    );
}

#[test]
fn icons_sit_over_the_ground_with_the_shape_and_color_of_their_unit() {
    let motions = [
        still(0, 0, 0, (100.0, 100.0)),
        still(1, 1, 1, (120.0, 100.0)),
        still(2, 3, 2, (140.0, 100.0)),
    ];
    let mut inst = instancer(&motions);
    let cam = camera(1.0, (128.0, 100.0));
    build(&mut inst, &cam, 0.0, Blend::ICONS, 1.0, |x, _| 0.1 * x);
    let lift = inst.icon_config().lift;
    let style = Style::embedded();
    let mut icons = inst.icon_instances().to_vec();
    icons.sort_by(|a, b| a.position[0].total_cmp(&b.position[0]));
    assert_eq!(icons.len(), 3);
    for (i, (icon, x)) in icons.iter().zip([100.0, 120.0, 140.0]).enumerate() {
        assert_eq!(icon.position[0], x);
        assert!(
            (icon.position[1] - (0.1 * x + lift)).abs() < 1e-4,
            "on the hill"
        );
        assert_eq!(icon.shape_index(), style.shape(i as u8).icon.index());
        assert_eq!(icon.color[..3], style.linear([0, 1, 3][i]));
        assert!(!icon.highlighted());
    }
    let params = inst.icon_params();
    assert_eq!((params.size_px, params.outline_px), (14.0, 1.5));
    build(&mut inst, &cam, 0.0, Blend::ICONS, 2.0, flat);
    let params = inst.icon_params();
    assert_eq!((params.size_px, params.outline_px), (28.0, 3.0));
}

// ---------------------------------------------------------------------------
// Picking: models
// ---------------------------------------------------------------------------

fn one_cube_at(at: (f32, f32)) -> Instancer {
    // A cube of 1.2 x 1.0 x 1.2 m whatever the shipped placeholder sizes are
    // (they follow the cell size, B12): the pixel offsets below are for it.
    let style = style_ron("(mesh: Cube, size: (1.2, 1.0, 1.2), icon: Square)", "");
    instancer_with(style, &[still(0, 0, 11, at)])
}

#[test]
fn a_ray_through_a_model_hits_and_a_ray_past_it_misses() {
    let mut inst = one_cube_at((100.0, 100.0));
    let cam = camera(0.1, (100.0, 100.0));
    build(&mut inst, &cam, 0.0, Blend::MODELS, 1.0, flat);
    let middle = Vec3::new(100.0, 0.5, 100.0);
    let hit = inst.pick(&query_over(&cam, middle, 0.0, 0.0)).unwrap();
    assert_eq!(hit.id, UnitId::from_parts(11, 0));
    assert_eq!(hit.via, PickVia::Model);
    assert!(hit.distance > 0.0 && hit.cursor_distance < 1.0);
    // The same units are hit off centre, inside the cube's outline ...
    assert!(inst.pick(&query_over(&cam, middle, 20.0, -10.0)).is_some());
    // ... and missed outside it, on every side.
    for (dx, dy) in [
        (90.0, 0.0),
        (-90.0, 0.0),
        (0.0, 90.0),
        (0.0, -90.0),
        (300.0, 200.0),
    ] {
        assert_eq!(
            inst.pick(&query_over(&cam, middle, dx, dy)),
            None,
            "({dx}, {dy}) px away"
        );
    }
}

#[test]
fn nothing_is_picked_before_a_build_or_from_an_empty_frame() {
    let mut inst = one_cube_at((100.0, 100.0));
    let cam = camera(0.1, (100.0, 100.0));
    let q = query_over(&cam, Vec3::new(100.0, 0.5, 100.0), 0.0, 0.0);
    assert_eq!(inst.pick(&q), None, "no frame yet");
    inst.update_tick([]);
    build(&mut inst, &cam, 0.0, Blend::MODELS, 1.0, flat);
    assert_eq!(inst.pick(&q), None, "no units");
}

/// Two tall boxes, one behind the other along the cursor's ray: the nearer wins,
/// whichever order the units are stored in.
#[test]
fn of_two_overlapping_models_the_nearer_one_wins() {
    let style = || style_ron("(mesh: Cube, size: (2.0, 6.0, 2.0))", "");
    let near = (100.0, 104.0); // south: nearer to the camera
    let far = (100.0, 99.0);
    let cam = camera(0.15, (100.0, 100.0));
    for order in [[0u32, 1], [1, 0]] {
        let units = [
            still(0, 0, 1, near), // id 1 is the near one
            still(0, 0, 2, far),
        ];
        let mut inst = instancer_with(
            style(),
            &[units[order[0] as usize], units[order[1] as usize]],
        );
        build(&mut inst, &cam, 0.0, Blend::MODELS, 1.0, flat);
        // The cursor over the near box's top: the ray runs on, down and north,
        // through the far box standing behind it.
        let q = query_over(&cam, Vec3::new(near.0, 5.5, near.1), 0.0, 0.0);
        let ray = q.ray().unwrap();
        let size = [2.0, 6.0, 2.0];
        let t_near = ray_obb(&ray, Vec3::new(near.0, 0.0, near.1), 0.0, size, 0.0);
        let t_far = ray_obb(&ray, Vec3::new(far.0, 0.0, far.1), 0.0, size, 0.0);
        let (t_near, t_far) = (
            t_near.expect("ray crosses the near box"),
            t_far.expect("ray crosses the far box"),
        );
        assert!(
            t_near < t_far,
            "the precondition: near {t_near}, far {t_far}"
        );
        let hit = inst.pick(&q).unwrap();
        assert_eq!(hit.id, UnitId::from_parts(1, 0), "order {order:?}");
        assert!((hit.distance - t_near).abs() < 1e-3);
    }
}

/// With nothing between them, equal hits go to the lower id, whatever the storage order.
#[test]
fn equal_hits_go_to_the_lower_id() {
    let cam = camera(0.15, (100.0, 100.0));
    for ids in [[5u32, 3], [3, 5]] {
        let units = [
            still(0, 0, ids[0], (100.0, 100.0)),
            still(0, 1, ids[1], (100.0, 100.0)),
        ];
        let mut inst = instancer(&units);
        build(&mut inst, &cam, 0.0, Blend::MODELS, 1.0, flat);
        let hit = inst
            .pick(&query_over(&cam, Vec3::new(100.0, 0.5, 100.0), 0.0, 0.0))
            .unwrap();
        assert_eq!(hit.id, UnitId::from_parts(3, 0));
    }
}

/// The pick answers for the frame that was drawn: at `alpha` the unit is
/// drawn between its two ticks, and that is where it can be clicked, not at
/// either tick position.
#[test]
fn the_pick_follows_the_interpolated_position_that_is_drawn() {
    let walker = moving(0, 2, 9, (90.0, 100.0), (110.0, 100.0));
    let mut inst = instancer(&[walker]);
    let cam = camera(0.1, (100.0, 100.0));
    let at = |x: f32| Vec3::new(x, 0.5, 100.0);
    for (alpha, drawn_x) in [
        (0.0, 90.0),
        (0.25, 95.0),
        (0.5, 100.0),
        (0.75, 105.0),
        (1.0, 110.0),
    ] {
        build(&mut inst, &cam, alpha, Blend::MODELS, 1.0, flat);
        // The instance really is at the interpolated place ...
        let m = inst.instances(MeshKind::Cube)[0].model;
        assert!(
            (m[3][0] - drawn_x).abs() < 1e-4,
            "alpha {alpha}: drawn at {}",
            m[3][0]
        );
        // ... and that is where the click lands.
        assert!(
            inst.pick(&query_over(&cam, at(drawn_x), 0.0, 0.0))
                .is_some(),
            "alpha {alpha}: miss at the drawn position {drawn_x}"
        );
        // Not where the unit was or will be, when that is further away than its size.
        for other in [90.0, 100.0, 110.0] {
            if (other - drawn_x).abs() >= 5.0 {
                assert_eq!(
                    inst.pick(&query_over(&cam, at(other), 0.0, 0.0)),
                    None,
                    "alpha {alpha}: hit at {other}, drawn at {drawn_x}"
                );
            }
        }
    }
}

#[test]
fn the_click_volume_stands_on_the_terrain_the_unit_stands_on() {
    let mut inst = one_cube_at((100.0, 100.0));
    let cam = camera(0.1, (100.0, 100.0));
    build(&mut inst, &cam, 0.0, Blend::MODELS, 1.0, |_, _| 3.0);
    // A cube 1 m tall on a 3 m hill: hit at height 3.5, miss at 0.5.
    assert!(
        inst.pick(&query_over(&cam, Vec3::new(100.0, 3.5, 100.0), 0.0, 0.0))
            .is_some()
    );
    assert_eq!(
        inst.pick(&query_over(&cam, Vec3::new(100.0, 0.5, 100.0), 0.0, 0.0)),
        None
    );
}

/// A hill between the camera and the unit hides it; a hill behind it does not.
#[test]
fn terrain_in_front_of_a_model_hides_it_from_the_cursor() {
    let mut inst = one_cube_at((100.0, 100.0));
    let cam = camera(0.1, (100.0, 100.0));
    build(&mut inst, &cam, 0.0, Blend::MODELS, 1.0, flat);
    let q = query_over(&cam, Vec3::new(100.0, 0.5, 100.0), 0.0, 0.0);
    let t = inst.pick(&q).unwrap().distance;
    assert!(
        inst.pick(&q.with_terrain(Some(t + 5.0))).is_some(),
        "ground behind the unit"
    );
    assert!(
        inst.pick(&q.with_terrain(Some(t + 0.01))).is_some(),
        "the unit's own ground"
    );
    assert_eq!(
        inst.pick(&q.with_terrain(Some(t - 5.0))),
        None,
        "ground in front of it"
    );
    assert!(inst.pick(&q.with_terrain(None)).is_some());
}

/// A model smaller than `click_min_px` on the screen still has a click
/// volume of that size; without the setting the same click misses.
#[test]
fn a_tiny_model_keeps_a_minimum_click_size() {
    let shapes = "(mesh: Cube, size: (1.0, 1.0, 1.0))";
    let cam = camera(0.6, (100.0, 100.0));
    let miss_at = |click_min_px: f32| {
        let style = style_ron(shapes, &format!("icons: (click_min_px: {click_min_px})"));
        let mut inst = instancer_with(style, &[still(0, 0, 4, (100.0, 100.0))]);
        build(&mut inst, &cam, 0.0, Blend::MODELS, 1.0, flat);
        let middle = Vec3::new(100.0, 0.5, 100.0);
        // How big the cube is on the screen.
        let (a, _) = project_px(&cam.view_proj(), Vec3::new(99.5, 0.5, 100.0), [W, H]).unwrap();
        let (b, _) = project_px(&cam.view_proj(), Vec3::new(100.5, 0.5, 100.0), [W, H]).unwrap();
        let width = b[0] - a[0];
        // Just outside the cube's own outline, inside the minimum click size.
        let dx = 0.5 * width + 2.0;
        (
            width,
            inst.pick(&query_over(&cam, middle, dx, 0.0)).is_none(),
        )
    };
    let (width, without) = miss_at(0.0);
    let (_, with) = miss_at(2.0 * 0.5 * 40.0 / 1.0); // generous minimum
    println!("cube is {width:.1} px wide on the screen");
    assert!(
        width < 14.0,
        "the cube must be small for this test: {width}"
    );
    assert!(without, "no minimum: a click 2 px outside the cube misses");
    assert!(!with, "a 80 px minimum: the same click lands");
    // The default minimum (14 px) lets a click 5 px beside the centre land.
    let style = style_ron(shapes, "");
    let mut inst = instancer_with(style, &[still(0, 0, 4, (100.0, 100.0))]);
    build(&mut inst, &cam, 0.0, Blend::MODELS, 1.0, flat);
    let middle = Vec3::new(100.0, 0.5, 100.0);
    assert!(inst.pick(&query_over(&cam, middle, 5.0, 0.0)).is_some());
    assert!(inst.pick(&query_over(&cam, middle, 30.0, 0.0)).is_none());
}

// ---------------------------------------------------------------------------
// Picking: icons
// ---------------------------------------------------------------------------

/// The pixel centre of an icon as the shader snaps it.
fn icon_centre(cam: &Camera, inst: &Instancer, unit: (f32, f32), ui_scale: f32) -> [f32; 2] {
    let lift = inst.icon_config().lift;
    let size = inst.icon_config().icon_px(ui_scale);
    let (px, _) = project_px(&cam.view_proj(), Vec3::new(unit.0, lift, unit.1), [W, H]).unwrap();
    [snap_icon_center(px[0], size), snap_icon_center(px[1], size)]
}

fn query_at_px(cam: &Camera, px: [f32; 2]) -> PickQuery {
    PickQuery::new(px, cam.view_proj(), [W, H])
}

/// The icon quad is `size_px` square around its snapped centre, and the edge counts as inside.
#[test]
fn an_icon_is_hit_inside_its_screen_quad_and_missed_outside_it() {
    let mut inst = instancer(&[still(0, 0, 6, (128.0, 128.0))]);
    let cam = camera(1.0, (128.0, 128.0));
    build(&mut inst, &cam, 0.0, Blend::ICONS, 1.0, flat);
    let c = icon_centre(&cam, &inst, (128.0, 128.0), 1.0);
    // Even sides: the centre is on a pixel corner.
    assert_eq!(c[0].fract(), 0.0);
    let hit = |dx: f32, dy: f32| inst.pick(&query_at_px(&cam, [c[0] + dx, c[1] + dy]));
    let h = hit(0.0, 0.0).unwrap();
    assert_eq!((h.id, h.via), (UnitId::from_parts(6, 0), PickVia::Icon));
    assert_eq!(h.cursor_distance, 0.0);
    // The boundary: 7 px (half of 14) is in, a hair more is out; all four sides.
    for (dx, dy) in [
        (7.0, 0.0),
        (-7.0, 0.0),
        (0.0, 7.0),
        (0.0, -7.0),
        (7.0, 7.0),
        (-7.0, -7.0),
    ] {
        assert!(hit(dx, dy).is_some(), "({dx}, {dy}) is on the edge");
    }
    for (dx, dy) in [
        (7.01, 0.0),
        (-7.01, 0.0),
        (0.0, 7.01),
        (0.0, -7.01),
        (7.01, 7.01),
        (20.0, 0.0),
    ] {
        assert!(hit(dx, dy).is_none(), "({dx}, {dy}) is outside");
    }
}

/// The same size at every zoom: that is the point of a screen-fixed icon.
#[test]
fn the_icon_click_quad_has_the_same_size_at_every_zoom() {
    for zoom in [0.7, 0.85, 1.0] {
        let mut inst = instancer(&[still(0, 0, 6, (128.0, 128.0))]);
        let cam = camera(zoom, (128.0, 128.0));
        build(&mut inst, &cam, 0.0, Blend::ICONS, 1.0, flat);
        let c = icon_centre(&cam, &inst, (128.0, 128.0), 1.0);
        let at = |dx: f32| inst.pick(&query_at_px(&cam, [c[0] + dx, c[1]]));
        assert!(at(7.0).is_some(), "zoom {zoom}");
        assert!(at(7.5).is_none(), "zoom {zoom}");
        assert!(at(-7.0).is_some(), "zoom {zoom}");
        assert!(at(-7.5).is_none(), "zoom {zoom}");
    }
}

#[test]
fn the_ui_scale_scales_the_icon_quad() {
    let mut inst = instancer(&[still(0, 0, 6, (128.0, 128.0))]);
    let cam = camera(1.0, (128.0, 128.0));
    build(&mut inst, &cam, 0.0, Blend::ICONS, 2.0, flat);
    let c = icon_centre(&cam, &inst, (128.0, 128.0), 2.0);
    let at = |dx: f32| inst.pick(&query_at_px(&cam, [c[0] + dx, c[1]]));
    assert!(at(14.0).is_some());
    assert!(at(14.5).is_none());
    // An odd side is centred on a pixel centre (1.5 x 14 = 21).
    build(&mut inst, &cam, 0.0, Blend::ICONS, 1.5, flat);
    let c = icon_centre(&cam, &inst, (128.0, 128.0), 1.5);
    assert_eq!(c[0].fract(), 0.5);
    let at = |dx: f32| inst.pick(&query_at_px(&cam, [c[0] + dx, c[1]]));
    assert!(at(10.5).is_some());
    assert!(at(11.0).is_none());
}

/// Terrain in front of a unit does not hide its icon, drawn or picked.
#[test]
fn terrain_does_not_hide_an_icon_from_the_cursor() {
    let mut inst = instancer(&[still(0, 0, 6, (128.0, 128.0))]);
    let cam = camera(1.0, (128.0, 128.0));
    build(&mut inst, &cam, 0.0, Blend::ICONS, 1.0, flat);
    let c = icon_centre(&cam, &inst, (128.0, 128.0), 1.0);
    let q = query_at_px(&cam, c).with_terrain(Some(0.5));
    assert!(inst.pick(&q).is_some());
}

/// Quads overlap when units are close: the icon whose centre is nearest the
/// cursor wins, from either side.
#[test]
fn of_two_overlapping_icons_the_one_whose_centre_is_nearer_the_cursor_wins() {
    // Two units a few pixels apart on the screen at whole-map zoom.
    let motions = [
        still(0, 0, 1, (128.0, 128.0)),
        still(0, 1, 2, (129.5, 128.0)),
    ];
    let mut inst = instancer(&motions);
    let cam = camera(1.0, (128.0, 128.0));
    build(&mut inst, &cam, 0.0, Blend::ICONS, 1.0, flat);
    let a = icon_centre(&cam, &inst, (128.0, 128.0), 1.0);
    let b = icon_centre(&cam, &inst, (129.5, 128.0), 1.0);
    let apart = b[0] - a[0];
    println!("icon centres {apart} px apart");
    assert!(apart > 2.0 && apart < 13.0, "the quads overlap: {apart}");
    let near_a = inst.pick(&query_at_px(&cam, [a[0] + 0.5, a[1]])).unwrap();
    let near_b = inst.pick(&query_at_px(&cam, [b[0] - 0.5, b[1]])).unwrap();
    assert_eq!(near_a.id, UnitId::from_parts(1, 0));
    assert_eq!(near_b.id, UnitId::from_parts(2, 0));
    // Exactly in the middle: a tie in the pixels, then the lower id.
    let middle = inst
        .pick(&query_at_px(&cam, [(a[0] + b[0]) / 2.0, a[1]]))
        .unwrap();
    assert_eq!(middle.id, UnitId::from_parts(1, 0));
}

// ---------------------------------------------------------------------------
// Picking in the cross-fade band
// ---------------------------------------------------------------------------

/// A big model (6 m) so its box reaches far outside its 14 px icon.
#[test]
fn in_the_band_the_more_visible_representation_is_picked() {
    let style = || style_ron("(mesh: Cube, size: (6.0, 6.0, 6.0))", "");
    let cam = camera(0.3, (100.0, 100.0));
    let pick_with = |model_weight: f32, dx: f32| {
        let mut inst = instancer_with(style(), &[still(0, 0, 8, (100.0, 100.0))]);
        build(
            &mut inst,
            &cam,
            0.0,
            Blend::from_model_weight(model_weight),
            1.0,
            flat,
        );
        // The icon's quad is centred on the lifted anchor, the box on its middle.
        inst.pick(&query_over(&cam, Vec3::new(100.0, 0.5, 100.0), dx, 0.0))
            .map(|p| p.via)
    };
    // 60 px beside the centre: inside the big box, far outside a 14 px icon.
    assert_eq!(pick_with(0.9, 60.0), Some(PickVia::Model));
    assert_eq!(
        pick_with(0.5, 60.0),
        Some(PickVia::Model),
        "a tie goes to the model"
    );
    assert_eq!(
        pick_with(0.49, 60.0),
        None,
        "mostly icon: only the icon counts"
    );
    assert_eq!(pick_with(0.1, 60.0), None);
    // 2 px beside the centre: inside both, the answer names the dominant one.
    assert_eq!(pick_with(0.9, 2.0), Some(PickVia::Model));
    assert_eq!(pick_with(0.2, 2.0), Some(PickVia::Icon));
    assert_eq!(pick_with(0.0, 2.0), Some(PickVia::Icon));
    assert_eq!(pick_with(1.0, 2.0), Some(PickVia::Model));
}

// ---------------------------------------------------------------------------
// Highlight, ids
// ---------------------------------------------------------------------------

#[test]
fn the_highlight_tints_the_model_and_flags_the_icon_of_the_picked_unit_only() {
    let motions = [
        still(0, 0, 1, (100.0, 100.0)),
        still(0, 1, 2, (110.0, 100.0)),
    ];
    let mut inst = instancer(&motions);
    let cam = camera(0.2, (105.0, 100.0));
    build(
        &mut inst,
        &cam,
        0.0,
        Blend::from_model_weight(0.5),
        1.0,
        flat,
    );
    let before_models: Vec<_> = inst.instances(MeshKind::Cube).to_vec();
    let before_icons = inst.icon_instances().to_vec();
    // The model dominates at weight 0.5: pick through it.
    let pick = inst
        .pick(&query_over(&cam, Vec3::new(110.0, 0.5, 100.0), 0.0, 0.0))
        .unwrap();
    assert_eq!(pick.id, UnitId::from_parts(2, 0));
    inst.highlight(&pick);
    let after_models = inst.instances(MeshKind::Cube);
    let after_icons = inst.icon_instances();
    let at_x = |list: &[Instance], x: f32| {
        *list
            .iter()
            .find(|i| (i.model[3][0] - x).abs() < 1e-3)
            .expect("the unit is in the list")
    };
    let (old, new) = (at_x(&before_models, 110.0), at_x(after_models, 110.0));
    assert!(
        new.color[..3]
            .iter()
            .zip(&old.color[..3])
            .all(|(n, o)| n > o),
        "brighter"
    );
    assert!(new.color[..3].iter().all(|&c| c <= 1.0));
    assert_eq!(new.color[3], old.color[3], "the weight is not touched");
    assert_eq!(new.model, old.model);
    assert_eq!(
        at_x(after_models, 100.0),
        at_x(&before_models, 100.0),
        "the other unit is untouched"
    );
    let flagged: Vec<_> = after_icons.iter().filter(|i| i.highlighted()).collect();
    assert_eq!(flagged.len(), 1, "one icon is highlighted");
    assert_eq!(flagged[0].position[0], 110.0);
    assert_eq!(after_icons.iter().filter(|i| !i.highlighted()).count(), 1);
    for (a, b) in before_icons.iter().zip(after_icons) {
        assert_eq!(a.position, b.position);
        assert_eq!(a.color, b.color);
    }
    // A pick from another frame does nothing.
    let stale = Pick {
        record: 40_000,
        ..pick
    };
    let snapshot = inst.icon_instances().to_vec();
    inst.highlight(&stale);
    assert_eq!(inst.icon_instances(), &snapshot[..]);
    let wrong_id = Pick {
        id: UnitId::from_parts(77, 0),
        ..pick
    };
    inst.highlight(&wrong_id);
    assert_eq!(inst.icon_instances(), &snapshot[..]);
}

#[test]
fn units_without_an_id_are_numbered_by_their_place_in_the_list() {
    let plain = |x: f32| {
        Motion::from_fixed(
            0,
            0,
            fx2(x, 100.0),
            fx2(x, 100.0),
            Facing::new(0),
            Facing::new(0),
        )
    };
    let mut inst = instancer(&[plain(100.0), plain(110.0)]);
    let cam = camera(0.2, (105.0, 100.0));
    build(&mut inst, &cam, 0.0, Blend::MODELS, 1.0, flat);
    let second = inst
        .pick(&query_over(&cam, Vec3::new(110.0, 0.5, 100.0), 0.0, 0.0))
        .unwrap();
    assert_eq!(second.id, UnitId::from_parts(1, 0));
}

/// The pick ray is the one the caller uses for the terrain: it has a unit direction.
#[test]
fn the_query_ray_is_a_unit_ray() {
    let cam = camera(0.4, (100.0, 100.0));
    let r: PickRay = query_at_px(&cam, [300.0, 200.0]).ray().unwrap();
    assert!((r.dir.length() - 1.0).abs() < 1e-5);
}

// ---------------------------------------------------------------------------
// Occlusion by the real terrain (WP1.8's ray-terrain hit)
// ---------------------------------------------------------------------------

/// A 256 m map whose vertex heights come from `height(x, z)` (metres, up to 60).
fn terrain(height: impl Fn(usize, usize) -> f32) -> std::sync::Arc<crate::ground::TerrainGround> {
    let n = 257;
    let heights = (0..n * n)
        .map(|i| (height(i % n, i / n) / 60.0 * 65535.0) as u16)
        .collect();
    let map = sf_data::map::Map::from_parts(
        256,
        256,
        60.0,
        vec!["a".into()],
        heights,
        vec![0; 256 * 256],
    )
    .expect("valid terrain");
    std::sync::Arc::new(crate::ground::TerrainGround::new(&map))
}

fn on(ground: &crate::ground::TerrainGround) -> impl Fn(f32, f32) -> f32 + '_ {
    use crate::ground::Ground;
    move |x, z| ground.height(f64::from(x), f64::from(z)) as f32
}

/// A ridge 12 m high between the camera and a unit hides it; a unit on the
/// camera's side of the ridge is not hidden; without the terrain ray the
/// hidden unit would be picked (so the test can fail).
#[test]
fn a_ridge_in_front_of_a_unit_hides_it_from_the_cursor() {
    let ground = terrain(|_, z| if (105..=108).contains(&z) { 12.0 } else { 0.0 });
    let cam = camera(0.15, (100.0, 100.0)); // the eye is south of the focus
    let motions = [
        still(0, 0, 1, (100.0, 100.0)),
        still(0, 1, 2, (100.0, 111.0)),
    ];
    let mut inst = instancer(&motions);
    build(&mut inst, &cam, 0.0, Blend::MODELS, 1.0, on(&ground));
    let behind = query_over(&cam, Vec3::new(100.0, 0.5, 100.0), 0.0, 0.0);
    let before = query_over(&cam, Vec3::new(100.0, 0.5, 111.0), 0.0, 0.0);
    // Without the ground both are under the cursor ray ...
    assert_eq!(
        inst.pick(&behind).map(|p| p.id),
        Some(UnitId::from_parts(1, 0))
    );
    assert_eq!(
        inst.pick(&before).map(|p| p.id),
        Some(UnitId::from_parts(2, 0))
    );
    // ... with it, the ridge hides the one behind it and not the one before it.
    assert_eq!(
        inst.pick(&behind.with_ground(&*ground)),
        None,
        "hidden by the ridge"
    );
    assert_eq!(
        inst.pick(&before.with_ground(&*ground)).map(|p| p.id),
        Some(UnitId::from_parts(2, 0))
    );
}

/// A unit standing on a slope is not hidden by the ground it stands on.
#[test]
fn a_unit_on_a_slope_is_not_hidden_by_its_own_ground() {
    let ground = terrain(|x, _| 0.06 * x as f32);
    let cam = camera(0.15, (100.0, 100.0));
    let mut inst = instancer(&[still(0, 0, 3, (100.0, 100.0))]);
    build(&mut inst, &cam, 0.0, Blend::MODELS, 1.0, on(&ground));
    let top = 0.06 * 100.0 + 1.0;
    let mut hits = 0;
    for (dy, h) in [(0.0, 0.5), (-8.0, 0.8), (6.0, 0.3)] {
        let q = query_over(
            &cam,
            Vec3::new(100.0, ground_h(&ground, 100.0) + h, 100.0),
            0.0,
            dy,
        );
        hits += usize::from(inst.pick(&q.with_ground(&*ground)).is_some());
    }
    println!("unit top at {top} m: {hits}/3 clicks on it landed");
    assert_eq!(hits, 3);
}

fn ground_h(ground: &crate::ground::TerrainGround, x: f32) -> f32 {
    use crate::ground::Ground;
    ground.height(f64::from(x), 100.0) as f32
}

// ---------------------------------------------------------------------------
// Rectangle picking and the selection marks (WP3.3)
// ---------------------------------------------------------------------------

use crate::pick::{RectQuery, ScreenRect};

fn rect_query(cam: &Camera, rect: ScreenRect) -> RectQuery {
    RectQuery {
        rect,
        view_proj: cam.view_proj(),
        viewport: [W, H],
    }
}

fn found(inst: &Instancer, cam: &Camera, rect: ScreenRect) -> Vec<UnitId> {
    let mut out = Vec::new();
    inst.pick_rect(&rect_query(cam, rect), &mut out);
    out
}

fn around_px(c: [f32; 2], half: f32) -> ScreenRect {
    ScreenRect::from_corners([c[0] - half, c[1] - half], [c[0] + half, c[1] + half])
}

/// Three cubes in a row, 4 m apart, seen close up: the rectangle takes the
/// cubes whose picture it touches.
#[test]
fn a_rectangle_takes_the_models_it_touches() {
    let style = style_ron("(mesh: Cube, size: (1.2, 1.0, 1.2), icon: Square)", "");
    let motions: Vec<_> = (0..3)
        .map(|i| still(0, 0, i, (96.0 + 4.0 * i as f32, 100.0)))
        .collect();
    let mut inst = instancer_with(style, &motions);
    let cam = camera(0.1, (100.0, 100.0));
    build(&mut inst, &cam, 0.0, Blend::MODELS, 1.0, flat);
    let at = |x: f32| {
        project_px(&cam.view_proj(), Vec3::new(x, 0.5, 100.0), [W, H])
            .unwrap()
            .0
    };
    let id = |i: u32| UnitId::from_parts(i, 0);
    // A box around the middle one only.
    assert_eq!(found(&inst, &cam, around_px(at(100.0), 10.0)), [id(1)]);
    // A wide one over the first two, whichever way it was dragged.
    let (a, b) = (at(96.0), at(100.0));
    let two = ScreenRect::from_corners([b[0] + 10.0, b[1] + 10.0], [a[0] - 10.0, a[1] - 10.0]);
    assert_eq!(found(&inst, &cam, two), [id(0), id(1)]);
    // The whole screen takes all of them, in ascending id order.
    assert_eq!(
        found(&inst, &cam, ScreenRect::viewport([W, H])),
        [id(0), id(1), id(2)]
    );
    // A rectangle in the gap, or over the sky, takes none.
    assert!(found(&inst, &cam, around_px(at(98.0), 2.0)).is_empty());
    assert!(
        found(
            &inst,
            &cam,
            ScreenRect::from_corners([0.0, 0.0], [30.0, 30.0])
        )
        .is_empty()
    );
}

/// A rectangle that only overlaps the edge of the model's outline still takes
/// it: units are taken by what they cover, not by their centre point.
#[test]
fn a_rectangle_over_the_edge_of_a_model_takes_it_and_one_beside_it_does_not() {
    let mut inst = one_cube_at((100.0, 100.0));
    let cam = camera(0.1, (100.0, 100.0));
    build(&mut inst, &cam, 0.0, Blend::MODELS, 1.0, flat);
    let c = project_px(&cam.view_proj(), Vec3::new(100.0, 0.5, 100.0), [W, H])
        .unwrap()
        .0;
    // The cube is wider than 20 px here (a click 20 px off centre was inside).
    let edge = ScreenRect::from_corners([c[0] + 20.0, c[1] - 5.0], [c[0] + 200.0, c[1] + 5.0]);
    assert_eq!(found(&inst, &cam, edge).len(), 1);
    let beside = ScreenRect::from_corners([c[0] + 95.0, c[1] - 5.0], [c[0] + 300.0, c[1] + 5.0]);
    assert!(found(&inst, &cam, beside).is_empty());
}

/// A model too small to see keeps the minimum click size: a rectangle that
/// touches the box the picking would grow it to takes it.
#[test]
fn a_tiny_model_is_taken_by_a_rectangle_near_it() {
    let style = style_ron("(mesh: Cube, size: (0.2, 0.2, 0.2), icon: Square)", "");
    let mut inst = instancer_with(style, &[still(0, 0, 4, (100.0, 100.0))]);
    let cam = camera(0.6, (100.0, 100.0));
    build(&mut inst, &cam, 0.0, Blend::MODELS, 1.0, flat);
    let c = project_px(&cam.view_proj(), Vec3::new(100.0, 0.1, 100.0), [W, H])
        .unwrap()
        .0;
    let min = inst.icon_config().click_min_px;
    let near =
        |d: f32| ScreenRect::from_corners([c[0] + d, c[1] - 2.0], [c[0] + d + 4.0, c[1] + 2.0]);
    assert_eq!(found(&inst, &cam, near(0.4 * min)).len(), 1);
    assert!(found(&inst, &cam, near(2.0 * min)).is_empty());
}

/// Zoomed out the icon quads count, 14 px square, the edge inside.
#[test]
fn a_rectangle_takes_the_icons_whose_quad_it_touches() {
    let motions = [
        still(0, 0, 1, (120.0, 128.0)),
        still(0, 0, 2, (128.0, 128.0)),
        still(0, 1, 3, (136.0, 128.0)),
    ];
    let mut inst = instancer(&motions);
    let cam = camera(1.0, (128.0, 128.0));
    build(&mut inst, &cam, 0.0, Blend::ICONS, 1.0, flat);
    let id = |i: u32| UnitId::from_parts(i, 0);
    let c = icon_centre(&cam, &inst, (128.0, 128.0), 1.0);
    assert_eq!(found(&inst, &cam, around_px(c, 2.0)), [id(2)]);
    // Touching the edge of the quad counts; a hair short of it does not.
    let right = |x: f32| ScreenRect::from_corners([x, c[1] - 1.0], [x + 2000.0, c[1] + 1.0]);
    let touching = found(&inst, &cam, right(c[0] + 7.0));
    assert!(touching.contains(&id(2)), "{touching:?}");
    let short = found(&inst, &cam, right(c[0] + 7.01));
    assert!(!short.contains(&id(2)), "{short:?}");
    // The whole screen takes every icon, the corner of it none.
    assert_eq!(
        found(&inst, &cam, ScreenRect::viewport([W, H])),
        [id(1), id(2), id(3)]
    );
    assert!(
        found(
            &inst,
            &cam,
            ScreenRect::from_corners([0.0, 0.0], [5.0, 5.0])
        )
        .is_empty()
    );
}

/// What is taken is the representation that is drawn: the model while it is
/// at least half the picture, the icon beyond.
#[test]
fn in_the_band_the_rectangle_follows_the_larger_representation() {
    let mut inst = instancer(&[still(0, 0, 1, (128.0, 128.0))]);
    let cam = camera(0.6, (128.0, 128.0));
    for weight in [0.9, 0.5, 0.49, 0.1] {
        build(
            &mut inst,
            &cam,
            0.0,
            Blend::from_model_weight(weight),
            1.0,
            flat,
        );
        let c = project_px(&cam.view_proj(), Vec3::new(128.0, 0.0, 128.0), [W, H])
            .unwrap()
            .0;
        let here = ScreenRect::from_corners([c[0] - 3.0, c[1] - 12.0], [c[0] + 3.0, c[1] + 3.0]);
        assert_eq!(found(&inst, &cam, here).len(), 1, "weight {weight}");
        assert!(
            found(
                &inst,
                &cam,
                ScreenRect::from_corners([0.0, 0.0], [20.0, 20.0])
            )
            .is_empty()
        );
    }
}

#[test]
fn nothing_is_found_before_a_build_and_the_buffer_is_reused() {
    let mut inst = instancer(&[still(0, 0, 1, (128.0, 128.0))]);
    let cam = camera(1.0, (128.0, 128.0));
    let mut out = vec![UnitId::from_parts(9, 9)];
    inst.pick_rect(&rect_query(&cam, ScreenRect::viewport([W, H])), &mut out);
    assert!(out.is_empty(), "cleared, and nothing built yet");
    build(&mut inst, &cam, 0.0, Blend::ICONS, 1.0, flat);
    inst.pick_rect(&rect_query(&cam, ScreenRect::viewport([W, H])), &mut out);
    assert_eq!(out, [UnitId::from_parts(1, 0)]);
    let capacity = out.capacity();
    inst.pick_rect(&rect_query(&cam, ScreenRect::viewport([W, H])), &mut out);
    assert_eq!(out.len(), 1, "cleared before it is filled again");
    assert_eq!(out.capacity(), capacity);
}

/// The selected icons get the selected flag, and the visit reports where the
/// selected units are drawn.
#[test]
fn the_selected_units_are_flagged_and_reported() {
    let style = style_ron("(mesh: Cube, size: (1.2, 1.0, 1.2), icon: Square)", "");
    let motions = [
        still(0, 0, 1, (100.0, 100.0)),
        still(0, 0, 2, (104.0, 100.0)),
        still(0, 1, 3, (108.0, 100.0)),
    ];
    let mut inst = instancer_with(style, &motions);
    let cam = camera(0.5, (104.0, 100.0));
    let blend = Blend::from_model_weight(0.5);
    build(&mut inst, &cam, 0.0, blend, 1.0, flat);
    let mut seen = Vec::new();
    inst.mark_selected(
        |id| id.index() != 2,
        |u| seen.push((u.id, u.centre, u.size, u.icon_anchor)),
    );
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].0, UnitId::from_parts(1, 0));
    assert_eq!(seen[0].1, [100.0, 0.0, 100.0]);
    assert_eq!(seen[0].2, [1.2, 1.0, 1.2]);
    assert_eq!(seen[0].3[0], 100.0);
    assert!(
        seen[0].3[1] > 0.0,
        "the icon anchor is lifted off the ground"
    );
    let flagged: Vec<f32> = inst
        .icon_instances()
        .iter()
        .filter(|i| i.selected())
        .map(|i| i.position[0])
        .collect();
    assert_eq!(flagged, [100.0, 108.0]);
    assert!(inst.icon_instances().iter().all(|i| !i.highlighted()));
    // Nothing selected: no visit, no flag.
    build(&mut inst, &cam, 0.0, blend, 1.0, flat);
    inst.mark_selected(|_| false, |_| panic!("nobody is selected"));
    assert!(inst.icon_instances().iter().all(|i| !i.selected()));
}

/// The icon quad the rectangle is tested against is the drawn one, so it grows
/// with the UI scale: at scale 2 it is 28 px square.
#[test]
fn the_rectangle_test_of_an_icon_follows_the_ui_scale() {
    let mut inst = instancer(&[still(0, 0, 3, (128.0, 128.0))]);
    let cam = camera(1.0, (128.0, 128.0));
    let touching = |scale: f32, inst: &mut Instancer, dx: f32| {
        build(inst, &cam, 0.0, Blend::ICONS, scale, flat);
        let c = icon_centre(&cam, inst, (128.0, 128.0), scale);
        let rect =
            ScreenRect::from_corners([c[0] + dx, c[1] - 1.0], [c[0] + dx + 500.0, c[1] + 1.0]);
        found(inst, &cam, rect).len() == 1
    };
    let half = inst.icon_config().icon_px(1.0) * 0.5;
    // At scale 1 the edge is `half` from the centre; a rectangle starting a
    // hair beyond it misses, and starting at it hits.
    assert!(touching(1.0, &mut inst, half));
    assert!(!touching(1.0, &mut inst, half + 0.01));
    // At scale 2 the same rectangles are inside the bigger quad; the edge moved out to twice that.
    assert!(touching(2.0, &mut inst, half + 0.01));
    assert!(touching(2.0, &mut inst, 2.0 * half));
    assert!(!touching(2.0, &mut inst, 2.0 * half + 0.01));
}

/// A model's minimum click size (`icons.click_min_px`) is in logical pixels
/// too: a rectangle beside a tiny model that misses at scale 1 hits at 2.
#[test]
fn the_rectangle_test_of_a_tiny_model_follows_the_ui_scale() {
    let style = style_ron("(mesh: Cube, size: (0.2, 0.2, 0.2), icon: Square)", "");
    let mut inst = instancer_with(style, &[still(0, 0, 4, (100.0, 100.0))]);
    let cam = camera(0.6, (100.0, 100.0));
    let c = project_px(&cam.view_proj(), Vec3::new(100.0, 0.1, 100.0), [W, H])
        .unwrap()
        .0;
    let min = inst.icon_config().click_min_px;
    // A rectangle whose near edge is 1.5 minimum-sizes from the centre.
    let rect = ScreenRect::from_corners(
        [c[0] + 0.75 * min, c[1] - 2.0],
        [c[0] + 0.75 * min + 4.0, c[1] + 2.0],
    );
    build(&mut inst, &cam, 0.0, Blend::MODELS, 1.0, flat);
    assert!(
        found(&inst, &cam, rect).is_empty(),
        "scale 1: the box is {min} px"
    );
    build(&mut inst, &cam, 0.0, Blend::MODELS, 2.0, flat);
    assert_eq!(
        found(&inst, &cam, rect).len(),
        1,
        "scale 2: the box is {} px",
        2.0 * min
    );
}
