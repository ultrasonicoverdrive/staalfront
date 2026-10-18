//! Screenshot test for the selection feedback (WP3.3), through the real
//! renderer on a software adapter (Mesa lavapipe / DX12 WARP): the sandbox as
//! `sf_game` starts it, with units selected, close up (rings and health bars
//! around the models) and zoomed out (highlighted icons with their bars), and
//! the drag rectangle.
//!
//! No golden image: the two software drivers differ a little in blending and
//! antialiasing. Every check compares the same frame with and without the
//! selection and counts the pixels that changed in front of each unit, so it
//! states what has to be true (selected units change, unselected ones do not,
//! and the whole selection is one UI draw call) without depending on the
//! driver. Set `SF_SAVE_SHOTS=<dir>` to write the images for a look.
//!
//! Adapter rules as in `sf_render`'s screenshot tests: a software adapter is
//! preferred; on CI (`CI` set) a missing one is a failure, locally any
//! adapter is accepted. No adapter at all fails the test.

use std::path::PathBuf;
use std::sync::Arc;

use sf_data::map::{Map, generate::generate_cells};
use sf_data::units::UnitDefs;
use sf_game::commands::LOCAL_PLAYER;
use sf_game::roster::Roster;
use sf_game::sandbox::{self, MERIDIAN, VEYRAN};
use sf_game::selection::Selection;
use sf_game::selection_view::{FrameGeometry, SelectionView, draw_drag_rect};
use sf_math::render_bridge::vec2_cells_to_metres;
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8};
use sf_render::model::EyeView;
use sf_render::pick::project_px;
use sf_render::{
    Camera, CameraConfig, FrameView, Frustum, GpuOptions, Ground, Instancer, Motion, Renderer,
    ScreenRect, Style, TerrainGround, UnitId,
};
use sf_sim::{EntityId, World, WorldConfig};
use sf_ui::{Ui, UiConfig, UiInput, UiRenderer};

const W: u32 = 960;
const H: u32 = 540;
const CELLS: u32 = 64;

fn renderer(map: &Map) -> Renderer {
    let mut r = Renderer::headless(W, H, &GpuOptions::software())
        .or_else(|software_err| {
            if std::env::var_os("CI").is_some() {
                return Err(software_err);
            }
            Renderer::headless(
                W,
                H,
                &GpuOptions {
                    backends: sf_render::wgpu::Backends::PRIMARY,
                    ..GpuOptions::default()
                },
            )
        })
        .unwrap_or_else(|e| panic!("NO GPU ADAPTER AVAILABLE: {e}"));
    println!("adapter: {}", r.adapter_summary());
    r.set_scene(&[]);
    r.set_terrain(map);
    r
}

fn save(name: &str, img: &Rgba8) {
    if let Some(dir) = std::env::var_os("SF_SAVE_SHOTS") {
        image::write_png(&PathBuf::from(dir).join(name), img).expect("save shot");
    }
}

/// The sandbox as the game starts it: the units and structures of the three
/// sides on a generated map, drawn as shapes (no model files read).
struct Scene {
    map: Map,
    ground: Arc<TerrainGround>,
    world: World,
    instancer: Instancer,
    roster: Roster,
    /// Entities by owner: (id, is a unit that moves).
    own: Vec<(EntityId, bool)>,
    enemy: Vec<EntityId>,
    /// The cursor, in pixels, if the next frame is drawn with the mouse over
    /// the picture: the unit under it is highlighted as the game does.
    hover: Option<[f32; 2]>,
}

impl Scene {
    fn new() -> Self {
        let map = generate_cells(CELLS, CELLS, 1);
        let ground = Arc::new(TerrainGround::new(&map));
        let defs = UnitDefs::embedded();
        let table = sf_bridge::unit_table(&defs).unwrap();
        let mut world = World::new(WorldConfig::new(1).with_map_size(CELLS, CELLS))
            .unwrap()
            .with_units(table)
            .unwrap();
        let plan = sandbox::schedule(&mut world, &defs);
        world.step();
        assert!(plan.units > 0 && plan.structures > 0);
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let base = Style::embedded().scaled(3.0);
        let roster = Roster::build(&defs, &base, &assets, "", false).unwrap();
        let mut instancer = Instancer::new(
            roster.style(&base),
            (map.width() as f32, map.height() as f32),
        );
        instancer.set_height_range(ground.min_height() as f32, ground.max_height() as f32);
        sync(&mut instancer, &roster, &world);
        let views: Vec<_> = world.snapshot().iter().collect();
        let mobile = |id| {
            world
                .stats_of(id)
                .is_some_and(|s| s.movement_class.is_some())
        };
        let own = views
            .iter()
            .filter(|v| v.owner.0 == MERIDIAN)
            .map(|v| (v.id, mobile(v.id)))
            .collect();
        let enemy = views
            .iter()
            .filter(|v| v.owner.0 == VEYRAN)
            .map(|v| v.id)
            .collect();
        Scene {
            map,
            ground,
            world,
            instancer,
            roster,
            own,
            enemy,
            hover: None,
        }
    }

    /// A new unit of `role` for `owner` at a cell, as the game's `SpawnUnit`
    /// makes it; the instancer takes the new tick.
    fn spawn(&mut self, role: &str, owner: u8, cell: (i32, i32)) -> EntityId {
        let defs = UnitDefs::embedded();
        let before: Vec<EntityId> = self.world.snapshot().iter().map(|v| v.id).collect();
        let command = sf_sim::Command::SpawnUnit {
            kind: sf_sim::Kind(defs.kind_of(role).unwrap()),
            pos: sf_math::Vec2::from_ints(cell.0, cell.1),
            owner: sf_sim::PlayerId(owner),
        };
        let tick = self.world.tick();
        self.world
            .schedule(tick, sf_sim::PlayerId(owner), command)
            .unwrap();
        self.world.step();
        sync(&mut self.instancer, &self.roster, &self.world);
        self.world
            .snapshot()
            .iter()
            .map(|v| v.id)
            .find(|id| !before.contains(id))
            .expect("the unit was made")
    }

    /// Takes a unit out of the world (a `Despawn` run on the next tick); the
    /// instancer takes the new tick.
    fn remove(&mut self, id: EntityId) {
        let tick = self.world.tick();
        self.world
            .schedule(tick, LOCAL_PLAYER, sf_sim::Command::Despawn { id })
            .unwrap();
        self.world.step();
        sync(&mut self.instancer, &self.roster, &self.world);
    }

    /// Builds the frame of `cam` (no picture): the culling, the levels of
    /// detail and the models or icons, as the game does before it picks.
    /// Returns the blend of the frame.
    fn build(&mut self, cam: &Camera) -> sf_render::Blend {
        let vp = cam.view_proj();
        let frustum = Frustum::from_view_proj(vp);
        let eye = EyeView::from_view_proj(vp, H as f32);
        let blend = self.instancer.icon_config().blend_for_view(
            eye.focal_px,
            cam.distance() as f32,
            cam.max_distance() as f32,
            1.0,
        );
        let ground = &self.ground;
        self.instancer
            .build_frame(&FrameView::new(&frustum, 0.0, eye, blend, 1.0), |x, z| {
                ground.height(f64::from(x), f64::from(z)) as f32
            });
        blend
    }

    fn camera(&self, zoom: f64, focus: [f32; 2]) -> Camera {
        let cfg = CameraConfig {
            map_size: DVec2::new(f64::from(self.map.width()), f64::from(self.map.height())),
            ..CameraConfig::default()
        };
        let mut c = Camera::new(cfg, W, H);
        c.set_ground(self.ground.clone());
        c.set_zoom(zoom);
        c.set_focus(DVec2::new(f64::from(focus[0]), f64::from(focus[1])));
        c
    }

    /// Where an entity is, in metres.
    fn metres(&self, id: EntityId) -> [f32; 2] {
        vec2_cells_to_metres(self.world.get(id).unwrap().pos)
    }

    /// The screen position of an entity's ground point.
    fn screen(&self, cam: &Camera, id: EntityId) -> [f32; 2] {
        let [x, z] = self.metres(id);
        let y = self.ground.height(f64::from(x), f64::from(z)) as f32;
        project_px(&cam.view_proj(), [x, y, z].into(), [W as f32, H as f32])
            .expect("in front of the camera")
            .0
    }

    /// One frame as `sf_game` draws it: build the units, mark the selection
    /// (rings and bars on the UI's world layer), upload, render. Returns the
    /// picture and the UI draw calls of the frame.
    fn shoot(
        &mut self,
        r: &mut Renderer,
        cam: &Camera,
        selection: &Selection,
        drag: Option<ScreenRect>,
    ) -> (Rgba8, u32) {
        let vp = cam.view_proj();
        r.set_view_proj(vp);
        let blend = self.build(cam);
        if let Some(cursor) = self.hover {
            let query = sf_render::PickQuery::new(cursor, vp, [W as f32, H as f32])
                .with_ground(&*self.ground);
            if let Some(pick) = self.instancer.pick(&query) {
                self.instancer.highlight(&pick);
            }
        }
        let mut ui = Ui::new(UiConfig::default());
        ui.begin_frame(UiInput::default(), (W, H), 1.0);
        ui.end_frame();
        let frame = FrameGeometry {
            view_proj: vp,
            viewport: [W as f32, H as f32],
            ui_scale: 1.0,
            blend,
            icon_px: self.instancer.icon_params().size_px,
        };
        let mut layer = ui.world_layer();
        let mut view = SelectionView::default();
        view.mark(
            &mut self.instancer,
            selection,
            &self.world,
            &frame,
            &mut layer,
        );
        if let Some(rect) = drag {
            draw_drag_rect(&mut layer, rect, 1.0);
        }
        r.upload_units(&self.instancer);
        let mut ur = UiRenderer::new(&r.gpu().device, r.color_format());
        let img = r
            .render_to_image_with(|t| ur.render(t, &mut ui))
            .expect("render");
        (img, ur.draw_calls())
    }
}

/// Hands the world's entities to the instancer as the game does each tick.
fn sync(instancer: &mut Instancer, roster: &Roster, world: &World) {
    let views: Vec<_> = world.snapshot().iter().collect();
    instancer.update_tick(views.iter().map(|v| {
        Motion::from_fixed(
            roster.shape_of(v.kind).unwrap_or(0),
            v.owner.0,
            v.prev_pos,
            v.pos,
            v.prev_facing,
            v.facing,
        )
        .with_id(UnitId::from_parts(v.id.index(), v.id.generation()))
    }));
}

/// Pixels that differ visibly between the two pictures inside `rect`.
fn changed(a: &Rgba8, b: &Rgba8, rect: ScreenRect) -> usize {
    let (x0, y0) = (rect.x0.max(0.0) as u32, rect.y0.max(0.0) as u32);
    let (x1, y1) = ((rect.x1 as u32).min(W - 1), (rect.y1 as u32).min(H - 1));
    let mut n = 0;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (p, q) = (a.pixel(x, y), b.pixel(x, y));
            let d: i32 = (0..3)
                .map(|i| (i32::from(p[i]) - i32::from(q[i])).abs())
                .sum();
            n += usize::from(d > 24);
        }
    }
    n
}

/// The box around a unit's screen position: `half` pixels each way, and
/// `above` more pixels up where its health bar floats.
fn box_at(p: [f32; 2], half: f32, above: f32) -> ScreenRect {
    ScreenRect::from_corners(
        [p[0] - half, p[1] - half - above],
        [p[0] + half, p[1] + half],
    )
}

fn selection_of(scene: &Scene, ids: &[EntityId]) -> Selection {
    let mut selection = Selection::new(LOCAL_PLAYER);
    selection.drag(&scene.world, ids, false);
    selection
}

/// Close up, where the models are drawn: a ring on the ground around each
/// selected unit and a health bar over it; nothing around the others.
#[test]
fn selected_models_get_rings_and_health_bars_and_the_others_do_not() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let units: Vec<EntityId> = scene
        .own
        .iter()
        .filter(|(_, mobile)| *mobile)
        .map(|(id, _)| *id)
        .collect();
    assert!(units.len() >= 6, "the sandbox has own units");
    // Look at the first few own units; select the first and the third.
    let focus = scene.metres(units[2]);
    let cam = scene.camera(0.08, focus);
    let none = Selection::new(LOCAL_PLAYER);
    let (base, base_calls) = scene.shoot(&mut r, &cam, &none, None);
    assert_eq!(base_calls, 0, "no selection, nothing for the UI to draw");
    let chosen = [units[0], units[2]];
    let selection = selection_of(&scene, &chosen);
    assert_eq!(selection.ids().len(), 2);
    let (img, calls) = scene.shoot(&mut r, &cam, &selection, None);
    save("selection_close.png", &img);
    assert_eq!(calls, 1, "all rings and bars are one draw call");

    let mut checked = 0;
    for id in &units {
        let p = scene.screen(&cam, *id);
        if !(80.0..(W as f32 - 80.0)).contains(&p[0]) || !(120.0..(H as f32 - 60.0)).contains(&p[1])
        {
            continue;
        }
        let near = box_at(p, 70.0, 60.0);
        let n = changed(&base, &img, near);
        if chosen.contains(id) {
            // A ring is a line of a few thousand pixels' length in all, a bar a few hundred.
            assert!(
                n > 250,
                "{id}: only {n} pixels changed around a selected unit"
            );
            checked += 1;
        } else {
            // Not inside the rectangle of a selected neighbour either.
            let by_neighbour = chosen
                .iter()
                .any(|c| near.overlaps(&box_at(scene.screen(&cam, *c), 70.0, 60.0)));
            if !by_neighbour {
                assert_eq!(n, 0, "{id}: an unselected unit changed");
                checked += 1;
            }
        }
    }
    assert!(checked >= 3, "the camera sees the units: {checked}");
}

/// The ring is a closed loop around the unit and its colour is the own tone.
#[test]
fn the_ring_is_green_and_surrounds_the_unit() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let id = scene.own.iter().find(|(_, m)| *m).unwrap().0;
    let cam = scene.camera(0.08, scene.metres(id));
    let none = Selection::new(LOCAL_PLAYER);
    let (base, _) = scene.shoot(&mut r, &cam, &none, None);
    let selection = selection_of(&scene, &[id]);
    let (img, _) = scene.shoot(&mut r, &cam, &selection, None);
    let c = scene.screen(&cam, id);
    // Pixels in the ring's colour that were not there before, near the unit
    // (the health bar is another green and is not counted).
    let own = sf_game::selection_view::OWN;
    let near = |a: u8, b: u8| (i32::from(a) - i32::from(b)).abs() < 12;
    let (mut lo, mut hi, mut n) = ([f32::MAX; 2], [f32::MIN; 2], 0);
    for y in 0..H {
        for x in 0..W {
            let (p, q) = (img.pixel(x, y), base.pixel(x, y));
            let (dx, dy) = (x as f32 - c[0], y as f32 - c[1]);
            if dx.abs() < 150.0
                && dy.abs() < 150.0
                && p != q
                && near(p[0], own.0)
                && near(p[1], own.1)
                && near(p[2], own.2)
            {
                lo = [lo[0].min(x as f32), lo[1].min(y as f32)];
                hi = [hi[0].max(x as f32), hi[1].max(y as f32)];
                n += 1;
            }
        }
    }
    assert!(n > 100, "{n} ring pixels near {c:?}");
    // The loop goes round the unit's ground point and is wider than it is
    // deep (the ground is seen at a slant).
    assert!(
        lo[0] < c[0] - 10.0 && hi[0] > c[0] + 10.0,
        "{lo:?} {hi:?} around {c:?}"
    );
    assert!(
        lo[1] < c[1] - 5.0 && hi[1] > c[1] + 5.0,
        "{lo:?} {hi:?} around {c:?}"
    );
    assert!(hi[0] - lo[0] > 1.2 * (hi[1] - lo[1]), "{lo:?} {hi:?}");
}

/// Zoomed out the units are icons: a selected icon is drawn with the thick
/// outline and a health bar, an unselected one is as it was, and the whole
/// selection is still one draw call however many units are in it.
#[test]
fn selected_icons_are_highlighted_and_the_others_are_not() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let cam = scene.camera(1.0, [128.0, 128.0]);
    let none = Selection::new(LOCAL_PLAYER);
    let (base, _) = scene.shoot(&mut r, &cam, &none, None);
    assert!(scene.instancer.blend().dominant() == sf_render::icons::Dominant::Icon);
    let own: Vec<EntityId> = scene.own.iter().map(|(id, _)| *id).collect();
    let mobile: Vec<EntityId> = scene
        .own
        .iter()
        .filter(|(_, m)| *m)
        .map(|(id, _)| *id)
        .collect();
    let selection = selection_of(&scene, &mobile);
    assert!(selection.len() >= 8);
    let (img, calls) = scene.shoot(&mut r, &cam, &selection, None);
    save("selection_strategic.png", &img);
    assert_eq!(calls, 1, "{} units, one draw call", selection.len());

    let icon = |id: EntityId| box_at(scene.screen(&cam, id), 9.0, 0.0);
    // The icon of every selected unit changed (thicker outline), unless
    // another selected unit's icon or bar sits on it.
    let mut changed_icons = 0;
    for id in &mobile {
        if changed(&base, &img, icon(*id)) > 8 {
            changed_icons += 1;
        }
    }
    assert!(
        changed_icons * 10 >= mobile.len() * 9,
        "{changed_icons} of {} selected icons changed",
        mobile.len()
    );
    // A structure that was not selected is untouched (its icon is not near a selected unit's bar).
    let far: Vec<_> = own
        .iter()
        .filter(|id| !mobile.contains(id))
        .filter(|id| {
            let p = scene.screen(&cam, **id);
            mobile.iter().all(|m| {
                let q = scene.screen(&cam, *m);
                (p[0] - q[0]).abs() > 30.0 || (p[1] - q[1]).abs() > 40.0
            })
        })
        .collect();
    assert!(
        !far.is_empty(),
        "the sandbox has structures away from the units"
    );
    for id in far {
        assert_eq!(changed(&base, &img, icon(*id)), 0, "{id}");
    }
    // The other side is untouched.
    for id in &scene.enemy {
        assert_eq!(changed(&base, &img, icon(*id)), 0, "{id}");
    }
}

/// A unit of the other side that is inspected gets a ring in another colour;
/// the own selection stays empty.
#[test]
fn an_inspected_enemy_gets_a_red_ring() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let enemy = scene
        .enemy
        .iter()
        .copied()
        .find(|id| {
            scene
                .world
                .stats_of(*id)
                .is_some_and(|s| s.movement_class.is_some())
        })
        .unwrap();
    let cam = scene.camera(0.08, scene.metres(enemy));
    let none = Selection::new(LOCAL_PLAYER);
    let (base, _) = scene.shoot(&mut r, &cam, &none, None);
    let mut selection = Selection::new(LOCAL_PLAYER);
    selection.click(&scene.world, Some(enemy), false);
    assert!(selection.is_empty() && selection.inspected() == Some(enemy));
    let (img, calls) = scene.shoot(&mut r, &cam, &selection, None);
    save("selection_inspect.png", &img);
    assert_eq!(calls, 1);
    let p = scene.screen(&cam, enemy);
    assert!(changed(&base, &img, box_at(p, 70.0, 60.0)) > 250);
    // Red dominates the new pixels, not green.
    let (mut red, mut green) = (0, 0);
    for y in 0..H {
        for x in 0..W {
            let (a, b) = (base.pixel(x, y), img.pixel(x, y));
            if a != b {
                red += usize::from(b[0] > 200 && b[1] < 140);
                green += usize::from(b[1] > 200 && b[0] < 140);
            }
        }
    }
    assert!(red > 100 && red > 4 * green, "red {red}, green {green}");
}

/// The drag rectangle: a translucent tint with a bright outline, and nothing
/// outside it.
#[test]
fn the_drag_rectangle_is_drawn_as_a_tinted_box_with_an_outline() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let cam = scene.camera(0.6, [128.0, 128.0]);
    let none = Selection::new(LOCAL_PLAYER);
    let (base, _) = scene.shoot(&mut r, &cam, &none, None);
    let rect = ScreenRect::from_corners([300.0, 150.0], [620.0, 380.0]);
    let (img, calls) = scene.shoot(&mut r, &cam, &none, Some(rect));
    save("selection_drag.png", &img);
    assert_eq!(calls, 1);
    let inside = changed(
        &base,
        &img,
        ScreenRect::from_corners([310.0, 160.0], [610.0, 370.0]),
    );
    let area = 300 * 210;
    assert!(
        inside > area * 8 / 10,
        "the tint covers the inside: {inside} of {area}"
    );
    // The outline is brighter than the tint.
    let edge = img.pixel(300, 260);
    let middle = img.pixel(450, 260);
    let lum = |p: [u8; 4]| i32::from(p[0]) + i32::from(p[1]) + i32::from(p[2]);
    assert!(lum(edge) > lum(middle) + 60, "{edge:?} vs {middle:?}");
    // Nothing happens outside.
    assert_eq!(
        changed(
            &base,
            &img,
            ScreenRect::from_corners([0.0, 0.0], [290.0, 140.0])
        ),
        0
    );
    assert_eq!(
        changed(
            &base,
            &img,
            ScreenRect::from_corners([640.0, 400.0], [950.0, 530.0])
        ),
        0
    );
}

fn entity(id: UnitId) -> EntityId {
    EntityId::new(id.index(), id.generation())
}

/// The click, the drag and the double-click through the real picking: what
/// the picture says is under the cursor or the rectangle goes into the
/// selection logic, as the runner does it. Models close up and icons zoomed
/// out. No GPU is needed for this one.
#[test]
fn clicks_drags_and_double_clicks_pick_through_models_and_icons() {
    let mut scene = Scene::new();
    // The sandbox has one of each role: add two more light tanks of the local
    // player beside it, one at the far corner of the map, and one of the
    // enemy's right by them.
    let first = scene.spawn("light_tank", MERIDIAN, (32, 40));
    let same_a = scene.spawn("light_tank", MERIDIAN, (34, 40));
    let same_b = scene.spawn("light_tank", MERIDIAN, (34, 42));
    let far = scene.spawn("light_tank", MERIDIAN, (3, 3));
    let enemy = scene.spawn("light_tank", VEYRAN, (36, 42));
    let wanted = [first, same_a, same_b];
    let local = Selection::new(LOCAL_PLAYER);
    let size = [W as f32, H as f32];

    for (zoom, name) in [(0.1, "models"), (1.0, "icons")] {
        let cam = scene.camera(zoom, scene.metres(same_a));
        scene.build(&cam);
        let query = |p: [f32; 2]| {
            sf_render::PickQuery::new(p, cam.view_proj(), size).with_ground(&*scene.ground)
        };
        let rect_ids = |rect: ScreenRect| {
            let mut ids = Vec::new();
            let q = sf_render::RectQuery {
                rect,
                view_proj: cam.view_proj(),
                viewport: size,
            };
            scene.instancer.pick_rect(&q, &mut ids);
            ids.into_iter().map(entity).collect::<Vec<_>>()
        };
        // Where to aim at a unit: the middle of what is drawn (a model stands
        // on its ground point, an icon is centred on it).
        let aim = |id: EntityId| {
            let p = scene.screen(&cam, id);
            [p[0], p[1] - if zoom < 0.5 { 10.0 } else { 0.0 }]
        };

        // A click on a unit picks it; on bare ground, nothing.
        let hit = scene
            .instancer
            .pick(&query(aim(same_a)))
            .map(|p| entity(p.id));
        assert_eq!(
            hit,
            Some(same_a),
            "{name}: the click picks the unit under it"
        );
        let mut sel = local.clone();
        sel.click(&scene.world, hit, false);
        assert_eq!(sel.ids(), [same_a], "{name}");
        // Somewhere on the screen with nothing under it (a corner, say).
        let bare = [
            [4.0, 4.0],
            [W as f32 - 4.0, 4.0],
            [4.0, H as f32 - 4.0],
            [W as f32 - 4.0, H as f32 - 4.0],
        ]
        .into_iter()
        .find(|&p| scene.instancer.pick(&query(p)).is_none())
        .expect("a corner of the screen is bare ground");
        let miss = scene.instancer.pick(&query(bare));
        sel.click(&scene.world, miss.map(|p| entity(p.id)), false);
        assert!(sel.is_empty(), "{name}: bare ground deselects");

        // A double-click: the same kind, own, on the screen.
        let in_view = rect_ids(ScreenRect::viewport(size));
        assert!(
            in_view.contains(&enemy),
            "{name}: the screen holds the enemy too"
        );
        let mut sel = local.clone();
        sel.select_same_kind(&scene.world, hit, &in_view, false);
        let mut expect = wanted.to_vec();
        if zoom > 0.5 {
            // The whole map is on the screen, so the far one counts too.
            assert!(in_view.contains(&far), "{name}");
            expect.push(far);
        } else {
            assert!(
                !in_view.contains(&far),
                "{name}: the far one is off the screen"
            );
        }
        // The sandbox's own light tank is of that kind as well, wherever it stands.
        let sandbox_tank = scene
            .own
            .iter()
            .map(|(id, _)| *id)
            .find(|&id| {
                scene.world.stats_of(id).map(|s| s.kind)
                    == scene.world.stats_of(same_a).map(|s| s.kind)
            })
            .expect("the sandbox has a light tank");
        if in_view.contains(&sandbox_tank) {
            expect.push(sandbox_tank);
        }
        expect.sort();
        assert_eq!(
            sel.ids(),
            expect,
            "{name}: own units of that kind in view, not the enemy's"
        );
        assert!(!sel.ids().contains(&enemy));

        // A drag around two of them takes those two, whichever way it goes.
        let (a, b) = (aim(same_a), aim(same_b));
        let margin = if zoom < 0.5 { 50.0 } else { 5.0 };
        let rect = ScreenRect::from_corners(
            [a[0].max(b[0]) + margin, a[1].max(b[1]) + margin],
            [a[0].min(b[0]) - margin, a[1].min(b[1]) - margin],
        );
        let inside = rect_ids(rect);
        let mut sel = local.clone();
        sel.drag(&scene.world, &inside, false);
        assert!(
            sel.ids().contains(&same_a) && sel.ids().contains(&same_b),
            "{name}: both are in the rectangle: {:?} in {inside:?}",
            sel.ids()
        );
        assert!(
            !sel.ids().contains(&far) && !sel.ids().contains(&enemy),
            "{name}"
        );
    }
}

fn near(p: [u8; 4], c: [u8; 4], tol: i32) -> bool {
    (0..3).all(|i| (i32::from(p[i]) - i32::from(c[i])).abs() <= tol)
}

fn differs(a: [u8; 4], b: [u8; 4]) -> bool {
    !near(a, b, 24)
}

fn white(p: [u8; 4]) -> bool {
    p[0] > 225 && p[1] > 225 && p[2] > 225
}

fn yellow(p: [u8; 4]) -> bool {
    p[0] > 225 && p[1] > 190 && p[2] < 160
}

/// The four states of an icon: no highlight, hovered (a thin white outline
/// outside the shape), selected (a clearly thicker white one), selected and
/// hovered (the selected width in yellow). The outline lies **outside** the
/// plain icon, so the player colour keeps the same area in every state and
/// ownership stays readable at the real 14 px size.
#[test]
fn an_icon_looks_different_plain_hovered_selected_and_both() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let cam = scene.camera(1.0, [128.0, 128.0]);
    // A unit of ours alone in a corner of the map, so no other icon is near it.
    let id = scene.spawn("rifleman", MERIDIAN, (8, 56));
    // The icon is lifted off the ground point; the cursor aims at its middle.
    let lift = scene.instancer.icon_config().lift;
    let (centre, _) = {
        let [x, z] = scene.metres(id);
        let y = scene.ground.height(f64::from(x), f64::from(z)) as f32 + lift;
        sf_render::pick::project_px(&cam.view_proj(), [x, y, z].into(), [W as f32, H as f32])
            .unwrap()
    };
    let region = box_at(centre, 12.0, 0.0);
    let none = Selection::new(LOCAL_PLAYER);
    let one = selection_of(&scene, &[id]);

    scene.hover = None;
    let (plain, _) = scene.shoot(&mut r, &cam, &none, None);
    scene.hover = Some(centre);
    let (hover, _) = scene.shoot(&mut r, &cam, &none, None);
    scene.hover = None;
    let (selected, _) = scene.shoot(&mut r, &cam, &one, None);
    scene.hover = Some(centre);
    let (both, _) = scene.shoot(&mut r, &cam, &one, None);
    // The same frame without the unit: what is behind the icon.
    scene.hover = None;
    scene.remove(id);
    let (bare, _) = scene.shoot(&mut r, &cam, &none, None);
    save("icon_bare.png", &bare);
    save("icon_plain.png", &plain);
    save("icon_hover.png", &hover);
    save("icon_selected.png", &selected);
    save("icon_both.png", &both);

    let (x0, y0) = (region.x0 as u32, region.y0 as u32);
    let (x1, y1) = (region.x1 as u32, region.y1 as u32);
    let at = |x: u32, y: u32| (x, y);
    // The plain icon's footprint: where it changed the picture, and its core,
    // the footprint without the pixels on its edge (those are antialiased).
    let in_foot = |x: u32, y: u32| differs(plain.pixel(x, y), bare.pixel(x, y));
    let in_core = |x: u32, y: u32| {
        in_foot(x, y)
            && [(1, 0), (0, 1), (-1, 0), (0, -1)]
                .iter()
                .all(|&(dx, dy)| in_foot((x as i32 + dx) as u32, (y as i32 + dy) as u32))
    };
    let mut core = Vec::new();
    for y in y0..=y1 {
        for x in x0..=x1 {
            if in_core(x, y) {
                core.push(at(x, y));
            }
        }
    }
    assert!(
        core.len() >= 40,
        "the plain icon is on the screen: {} core pixels",
        core.len()
    );
    // The fill colour: the middle of the plain icon.
    let fill = plain.pixel(centre[0] as u32, centre[1] as u32 + 2);
    assert!(
        fill[0] > 150 && fill[1] < 110,
        "a red (Meridian) fill: {fill:?}"
    );
    let count_fill = |img: &Rgba8| {
        let mut n = 0;
        for y in y0..=y1 {
            for x in x0..=x1 {
                n += usize::from(near(img.pixel(x, y), fill, 30));
            }
        }
        n
    };
    let n_plain = count_fill(&plain);
    assert!(n_plain >= 40, "{n_plain} fill pixels");

    let (mut white_n, mut yellow_n) = ([0usize; 3], [0usize; 3]);
    for (k, img) in [&hover, &selected, &both].into_iter().enumerate() {
        // The player colour keeps its area: the same number of fill pixels.
        let n = count_fill(img);
        assert!(
            n.abs_diff(n_plain) * 20 <= n_plain,
            "state {k}: {n} fill pixels against {n_plain} in the plain icon"
        );
        // Nothing inside the plain icon changed (but for its antialiased edge).
        let moved = core
            .iter()
            .filter(|&&(x, y)| differs(img.pixel(x, y), plain.pixel(x, y)))
            .count();
        assert!(
            moved * 50 <= core.len(),
            "state {k}: {moved} of {} core pixels changed",
            core.len()
        );
        // The outline pixels: new, white or yellow, and outside the core.
        for y in y0..=y1 {
            for x in x0..=x1 {
                let p = img.pixel(x, y);
                if !differs(p, plain.pixel(x, y)) || in_core(x, y) {
                    continue;
                }
                white_n[k] += usize::from(white(p));
                yellow_n[k] += usize::from(yellow(p));
            }
        }
    }
    println!(
        "fill: plain {n_plain}, hover {}, selected {}, both {}; outline white {white_n:?}, yellow {yellow_n:?}",
        count_fill(&hover),
        count_fill(&selected),
        count_fill(&both)
    );
    let (w_hover, w_sel) = (white_n[0], white_n[1]);
    // Hover: a thin white outline, no yellow.
    assert!(w_hover >= 8, "the hover outline shows: {w_hover}");
    assert_eq!(yellow_n[0], 0);
    // Selected: clearly more white than the hover, still no yellow.
    assert!(
        w_sel * 10 >= w_hover * 20,
        "selected {w_sel} is clearly thicker than hover {w_hover}"
    );
    assert_eq!(yellow_n[1], 0);
    // Selected and hovered: the selected width, in yellow, not white.
    assert!(
        yellow_n[2] * 10 >= w_sel * 8,
        "yellow {} vs selected white {w_sel}",
        yellow_n[2]
    );
    assert!(
        white_n[2] < w_hover,
        "the outline is yellow, not white: {}",
        white_n[2]
    );
    // All four pictures differ from each other in the icon's region.
    let pics = [&plain, &hover, &selected, &both];
    for (i, a) in pics.iter().enumerate() {
        for b in &pics[i + 1..] {
            assert!(changed(a, b, region) > 0);
        }
    }
}
