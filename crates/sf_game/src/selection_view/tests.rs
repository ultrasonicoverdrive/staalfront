//! The selection feedback without a GPU: the ring geometry, and what gets
//! drawn on the world layer for what is selected. The picture itself is
//! checked by `tests/selection_screenshot.rs`.

use super::*;
use crate::sim_snapshot::SimSnapshot;
use sf_data::units::UnitDefs;
use sf_math::Vec2;
use sf_render::camera::{Camera, CameraConfig, DVec2};
use sf_render::{FrameView, Frustum, Motion, Style};
use sf_sim::{Cell, Command, Kind, PlayerId, World, WorldConfig};
use sf_ui::{Ui, UiConfig, UiInput};

const W: f32 = 960.0;
const H: f32 = 540.0;

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

fn bounds(points: &[[f32; 2]]) -> ([f32; 2], [f32; 2]) {
    points
        .iter()
        .fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), p| {
            (
                [lo[0].min(p[0]), lo[1].min(p[1])],
                [hi[0].max(p[0]), hi[1].max(p[1])],
            )
        })
}

#[test]
fn a_circle_on_the_ground_projects_to_a_loop_round_its_centre() {
    let cam = camera(0.2, (128.0, 128.0));
    let vp = cam.view_proj();
    let centre = Vec3::new(128.0, 0.0, 128.0);
    let mut out = Vec::new();
    assert!(ground_loop(
        &vp,
        [W, H],
        centre,
        0.0,
        GroundShape::Circle(2.0),
        &mut out
    ));
    assert_eq!(out.len(), RING_SEGMENTS);
    let (lo, hi) = bounds(&out);
    let (c, _) = project_px(&vp, centre, [W, H]).unwrap();
    assert!(lo[0] < c[0] && c[0] < hi[0] && lo[1] < c[1] && c[1] < hi[1]);
    // The ground is seen at a slant: a circle is wider than it is high.
    assert!(hi[0] - lo[0] > hi[1] - lo[1]);
    // A bigger ring is bigger on the screen; the buffer is reused, not grown.
    let capacity = out.capacity();
    assert!(ground_loop(
        &vp,
        [W, H],
        centre,
        0.0,
        GroundShape::Circle(4.0),
        &mut out
    ));
    let (lo2, hi2) = bounds(&out);
    assert!(hi2[0] - lo2[0] > 1.8 * (hi[0] - lo[0]));
    assert_eq!(out.len(), RING_SEGMENTS, "cleared before each loop");
    assert_eq!(out.capacity(), capacity);
}

#[test]
fn a_box_outline_has_four_corners_that_turn_with_the_facing() {
    let cam = camera(0.2, (128.0, 128.0));
    let vp = cam.view_proj();
    let centre = Vec3::new(128.0, 0.0, 128.0);
    let shape = GroundShape::Rect {
        along: 8.0,
        across: 2.0,
    };
    let (mut flat, mut turned) = (Vec::new(), Vec::new());
    assert!(ground_loop(&vp, [W, H], centre, 0.0, shape, &mut flat));
    assert!(ground_loop(&vp, [W, H], centre, 64.0, shape, &mut turned));
    assert_eq!((flat.len(), turned.len()), (4, 4));
    let (lo, hi) = bounds(&flat);
    let (tlo, thi) = bounds(&turned);
    // Along the facing is x at facing 0, so it is the wide way; a quarter turn
    // puts the long side along z, which is the deep way.
    assert!(hi[0] - lo[0] > thi[0] - tlo[0]);
    assert!(thi[1] - tlo[1] > hi[1] - lo[1]);
}

#[test]
fn a_ring_behind_the_camera_is_not_drawn() {
    let cam = camera(0.2, (128.0, 128.0));
    let eye = cam.eye();
    let behind = Vec3::new(eye.x as f32, eye.y as f32 + 50.0, eye.z as f32 + 300.0);
    let mut out = Vec::new();
    assert!(!ground_loop(
        &cam.view_proj(),
        [W, H],
        behind,
        0.0,
        GroundShape::Circle(2.0),
        &mut out
    ));
}

#[test]
fn rings_are_sized_from_the_model_and_a_structure_gets_a_box() {
    let size = [3.0, 2.0, 5.0];
    let GroundShape::Circle(r) = ring_shape(Some(UnitClass::Mobile), size) else {
        panic!("a unit gets a circle");
    };
    assert!(
        (r - 0.5 * 5.0 * RING_GROW).abs() < 1e-6,
        "from the longer side"
    );
    assert!(matches!(ring_shape(None, size), GroundShape::Circle(_)));
    assert!(matches!(
        ring_shape(Some(UnitClass::Object), size),
        GroundShape::Circle(_)
    ));
    let GroundShape::Rect { along, across } = ring_shape(Some(UnitClass::Structure), size) else {
        panic!("a structure gets a box");
    };
    assert!(along > size[0] && across > size[2]);
}

#[test]
fn the_health_bar_goes_green_amber_red() {
    let (g, a, r) = (health_color(1.0), health_color(0.5), health_color(0.1));
    assert!(g.1 > g.0 && a.0 > 200 && a.1 > 150 && r.0 > 200 && r.1 < 100);
    assert_eq!(health_color(0.61), g);
    assert_eq!(health_color(0.6), a);
    assert_eq!(health_color(0.3), r);
}

/// A world with a tank and a barracks of the local player, an enemy tank and
/// a neutral ore field, and an instancer that has drawn a frame of them.
struct Frame {
    world: World,
    instancer: Instancer,
    geometry: FrameGeometry,
    ids: Vec<EntityId>,
}

impl Frame {
    /// The world as the presentation reads it.
    fn snap(&self) -> SimSnapshot {
        SimSnapshot::capture(&self.world)
    }
}

fn frame(zoom: f64, blend: Blend) -> Frame {
    let defs = UnitDefs::embedded();
    let table = sf_bridge::unit_table(&defs).unwrap();
    let mut world = World::new(WorldConfig::new(2).with_map_size(64, 64))
        .unwrap()
        .with_units(table)
        .unwrap();
    let kind = |role: &str| Kind(defs.kind_of(role).unwrap());
    let schedule = |world: &mut World, owner: u8, command| {
        world
            .schedule(world.tick(), PlayerId(owner), command)
            .unwrap();
    };
    schedule(
        &mut world,
        0,
        Command::SpawnUnit {
            kind: kind("light_tank"),
            pos: Vec2::from_ints(32, 32),
            owner: PlayerId(0),
        },
    );
    schedule(
        &mut world,
        0,
        Command::PlaceStructure {
            kind: kind("barracks"),
            cell: Cell::new(36, 30),
            owner: PlayerId(0),
        },
    );
    schedule(
        &mut world,
        1,
        Command::SpawnUnit {
            kind: kind("light_tank"),
            pos: Vec2::from_ints(28, 32),
            owner: PlayerId(1),
        },
    );
    schedule(
        &mut world,
        2,
        Command::SpawnUnit {
            kind: kind("ore_field"),
            pos: Vec2::from_ints(32, 28),
            owner: PlayerId(2),
        },
    );
    world.step();
    let views: Vec<_> = world.snapshot().iter().collect();
    let ids = views.iter().map(|v| v.id).collect();
    let mut instancer = Instancer::new(Style::embedded().scaled(3.0), (256.0, 256.0));
    instancer.set_height_range(0.0, 1.0);
    instancer.update_tick(views.iter().map(|v| {
        Motion::from_fixed(
            (v.kind.0 % 3) as u8,
            v.owner.0,
            v.prev_pos,
            v.pos,
            v.prev_facing,
            v.facing,
        )
        .with_id(UnitId::from_parts(v.id.index(), v.id.generation()))
    }));
    let cam = camera(zoom, (128.0, 128.0));
    let vp = cam.view_proj();
    let frustum = Frustum::from_view_proj(vp);
    let eye = sf_render::EyeView::from_view_proj(vp, H);
    instancer.build_frame(&FrameView::new(&frustum, 0.0, eye, blend, 1.0), |_, _| 0.0);
    let geometry = FrameGeometry {
        view_proj: vp,
        viewport: [W, H],
        ui_scale: 1.0,
        blend,
        icon_px: instancer.icon_params().size_px,
    };
    Frame {
        world,
        instancer,
        geometry,
        ids,
    }
}

fn quads_for(frame: &mut Frame, selection: &Selection) -> usize {
    let mut ui = Ui::new(UiConfig::default());
    ui.begin_frame(UiInput::default(), (W as u32, H as u32), 1.0);
    ui.end_frame();
    let mut layer = ui.world_layer();
    let mut view = SelectionView::default();
    let snap = frame.snap();
    view.mark(
        &mut frame.instancer,
        selection,
        &snap,
        &frame.geometry,
        &mut layer,
    );
    layer.quad_count()
}

fn local() -> Selection {
    Selection::new(crate::commands::LOCAL_PLAYER)
}

#[test]
fn with_nothing_selected_nothing_is_drawn_and_no_icon_is_marked() {
    let mut f = frame(0.2, Blend::from_model_weight(0.5));
    assert_eq!(quads_for(&mut f, &local()), 0);
    assert!(f.instancer.icon_instances().iter().all(|i| !i.selected()));
}

#[test]
fn a_selected_unit_gets_a_ring_a_bar_and_a_selected_icon() {
    let mut f = frame(0.2, Blend::from_model_weight(0.5));
    let mut selection = local();
    selection.drag(&f.snap(), &f.ids, false);
    assert_eq!(selection.len(), 1, "the tank; the barracks is not dragged");
    let quads = quads_for(&mut f, &selection);
    // 28 segments of a ring, and a bar of three quads.
    assert_eq!(quads, RING_SEGMENTS + 3);
    assert_eq!(
        f.instancer
            .icon_instances()
            .iter()
            .filter(|i| i.selected())
            .count(),
        1
    );
}

#[test]
fn a_structure_gets_a_box_of_four_lines_and_a_bar() {
    let mut f = frame(0.2, Blend::MODELS);
    let barracks = f
        .ids
        .iter()
        .copied()
        .find(|&id| facts(&f.snap(), id).is_some_and(|x| x.class == UnitClass::Structure))
        .unwrap();
    let mut selection = local();
    selection.click(&f.snap(), Some(barracks), false);
    assert_eq!(quads_for(&mut f, &selection), 4 + 3);
}

#[test]
fn an_inspected_enemy_is_drawn_and_never_joins_the_selection() {
    let mut f = frame(0.2, Blend::MODELS);
    let enemy = f
        .ids
        .iter()
        .copied()
        .find(|&id| f.world.get(id).unwrap().owner.0 == 1)
        .unwrap();
    let mut selection = local();
    selection.click(&f.snap(), Some(enemy), false);
    assert!(selection.is_empty());
    assert_eq!(quads_for(&mut f, &selection), RING_SEGMENTS + 3);
}

#[test]
fn an_ore_field_that_cannot_be_damaged_has_a_ring_but_no_bar() {
    let mut f = frame(0.2, Blend::MODELS);
    let crate_id = f
        .ids
        .iter()
        .copied()
        .find(|&id| f.world.get(id).unwrap().owner.0 == 2)
        .unwrap();
    assert_eq!(f.world.stats_of(crate_id).map(|s| s.hp), Some(0));
    let mut selection = local();
    selection.click(&f.snap(), Some(crate_id), false);
    assert_eq!(selection.inspected(), Some(crate_id));
    assert_eq!(quads_for(&mut f, &selection), RING_SEGMENTS);
}

#[test]
fn zoomed_out_the_ring_is_left_out_and_the_bar_sits_over_the_icon() {
    let mut f = frame(1.0, Blend::ICONS);
    let mut selection = local();
    selection.drag(&f.snap(), &f.ids, false);
    assert_eq!(quads_for(&mut f, &selection), 3, "a bar, no ring");
}

#[test]
fn the_dead_are_not_drawn() {
    let mut f = frame(0.2, Blend::MODELS);
    let mut selection = local();
    selection.drag(&f.snap(), &f.ids, false);
    let tank = selection.ids()[0];
    f.world
        .schedule(
            f.world.tick(),
            crate::commands::LOCAL_PLAYER,
            Command::Despawn { id: tank },
        )
        .unwrap();
    f.world.step();
    // The frame the instancer drew still has the tank, but the world does not:
    // nothing is drawn for it, and the selection forgets it.
    assert_eq!(quads_for(&mut f, &selection), 0);
    selection.forget_the_dead(&f.snap());
    assert!(selection.is_empty());
}

#[test]
fn the_drag_rectangle_is_a_fill_and_an_outline() {
    let mut ui = Ui::new(UiConfig::default());
    ui.begin_frame(UiInput::default(), (W as u32, H as u32), 1.0);
    ui.end_frame();
    let mut layer = ui.world_layer();
    draw_drag_rect(
        &mut layer,
        ScreenRect::from_corners([100.0, 100.0], [300.0, 200.0]),
        1.0,
    );
    assert_eq!(layer.quad_count(), 1 + 4);
    // Nothing for a rectangle with no area.
    let mut empty = Ui::new(UiConfig::default());
    empty.begin_frame(UiInput::default(), (W as u32, H as u32), 1.0);
    let mut layer = empty.world_layer();
    draw_drag_rect(
        &mut layer,
        ScreenRect::from_corners([5.0, 5.0], [5.0, 5.0]),
        1.0,
    );
    assert_eq!(layer.quad_count(), 0);
}

#[test]
fn the_world_layer_is_cleared_every_frame_and_is_under_the_panels() {
    let mut ui = Ui::new(UiConfig::default());
    ui.begin_frame(UiInput::default(), (W as u32, H as u32), 1.0);
    ui.end_frame();
    draw_drag_rect(
        &mut ui.world_layer(),
        ScreenRect::from_corners([10.0, 10.0], [90.0, 90.0]),
        1.0,
    );
    assert_eq!(ui.underlay().quad_count(), 5);
    assert_eq!(ui.draw_list().quad_count(), 0, "not in the panels' list");
    ui.begin_frame(UiInput::default(), (W as u32, H as u32), 1.0);
    assert_eq!(ui.underlay().quad_count(), 0);
}
