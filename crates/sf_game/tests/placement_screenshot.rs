//! Screenshot test for the placement ghost (WP6.2), through the real renderer on a software adapter
//! (Mesa lavapipe / DX12 WARP): the sandbox as `sf_game` starts it (with the building rules), a
//! structure chosen to build, and the ghost on a place the simulation allows, on terrain it
//! refuses, on a structure, too far from the base, for the wrong faction, and for both factions.
//!
//! The ghost is drawn from `World::preview_placement`, the function `BuildStructure` runs: the
//! places are *found* with it (nothing here knows a rule), and every check compares the same
//! frame with and without the ghost and looks at how the pixels of each cell moved: green where the
//! simulation allows the place, red on the cell that is the problem, amber on a free cell of a
//! refused place. No golden image: the two software drivers differ a little in blending. Set
//! `SF_SAVE_SHOTS=<dir>` to write the images for a look (`docs/screenshots/wp6.2/`).
//!
//! Adapter rules as in `selection_screenshot.rs`: a software adapter is preferred; on CI (`CI` set)
//! a missing one is a failure, locally any adapter is accepted.

use std::path::PathBuf;
use std::sync::Arc;

use sf_data::map::{Map, generate::generate_cells};
use sf_data::units::UnitDefs;
use sf_game::placement_view::{self, GhostFrame};
use sf_game::playfield::{make_world, with_buildings, with_movement};
use sf_game::roster::Roster;
use sf_game::sandbox::{self, MERIDIAN, VEYRAN};
use sf_game::sim_snapshot::{GhostAnswer, GhostQuery};
use sf_render::camera::DVec2;
use sf_render::image::{self, Rgba8};
use sf_render::model::EyeView;
use sf_render::pick::project_px;
use sf_render::{
    Camera, CameraConfig, FrameView, Frustum, GpuOptions, Ground, Instancer, Motion, Renderer,
    Style, TerrainGround, UnitId,
};
use sf_sim::{
    Cell, CellVerdict, Kind, PlacementError, PlacementPreview, PlayerId, World, WorldConfig,
};
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
    r.set_icon_atlas(&icon_set());
    r
}

fn icon_set() -> sf_render::IconAtlas {
    let atlas = sf_render::IconAtlas::load(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons"),
    );
    assert!(atlas.warnings().is_empty(), "{:?}", atlas.warnings());
    atlas
}

fn save(name: &str, img: &Rgba8) {
    if let Some(dir) = std::env::var_os("SF_SAVE_SHOTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("shot directory");
        image::write_png(&dir.join(name), img).expect("save shot");
    }
}

/// The sandbox as the game starts it, with the building rules.
struct Scene {
    map: Map,
    ground: Arc<TerrainGround>,
    world: World,
    defs: UnitDefs,
    instancer: Instancer,
    roster: Roster,
}

impl Scene {
    fn new() -> Self {
        let map = generate_cells(CELLS, CELLS, 1);
        let ground = Arc::new(TerrainGround::new(&map));
        let defs = UnitDefs::embedded();
        let config = WorldConfig::new(1).with_map_size(CELLS, CELLS);
        let world = make_world(config, &map)
            .unwrap()
            .with_units(sf_bridge::unit_table(&defs).unwrap())
            .unwrap();
        let mut world = with_buildings(with_movement(world, &defs), &defs);
        let plan = sandbox::schedule(&mut world, &defs);
        let report = world.step();
        assert!(plan.structures > 0 && report.commands_ignored == 0);
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let base = Style::embedded().scaled(3.0);
        let roster = Roster::build(&defs, &base, &icon_set(), &assets, "", false).unwrap();
        let mut instancer = Instancer::new(
            roster.style(&base),
            (map.width() as f32, map.height() as f32),
        );
        instancer.set_height_range(ground.min_height() as f32, ground.max_height() as f32);
        let mut scene = Scene {
            map,
            ground,
            world,
            defs,
            instancer,
            roster,
        };
        scene.sync();
        scene
    }

    fn sync(&mut self) {
        let roster = &self.roster;
        let views: Vec<_> = self.world.snapshot().iter().collect();
        self.instancer.update_tick(views.iter().map(|v| {
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

    fn kind(&self, id: &str) -> Kind {
        Kind(self.defs.kind_of(id).unwrap())
    }

    /// The anchor of the base of `owner`: where its construction_center stands.
    fn base_of(&self, owner: u8) -> Cell {
        let anchor = self.kind("construction_center");
        self.world
            .snapshot()
            .iter()
            .find(|v| v.kind == anchor && v.owner == PlayerId(owner))
            .and_then(|v| self.world.cell_of(v.id))
            .expect("the sandbox gives every side an anchor")
    }

    /// The simulation's answer to "may `player` build `role` on `cell`?", as the sim thread gives it.
    fn ask(&self, role: &str, cell: Cell, player: u8) -> GhostAnswer {
        let query = GhostQuery {
            kind: self.kind(role),
            cell,
            player: PlayerId(player),
        };
        GhostAnswer {
            query,
            preview: self
                .world
                .preview_placement(query.kind, query.cell, query.player),
        }
    }

    /// The cell nearest to `near` (the chessboard distance, then row, then column) within 14 cells
    /// where the simulation's answer for `role` fits `want`.
    fn find(
        &self,
        role: &str,
        player: u8,
        near: Cell,
        want: impl Fn(&PlacementPreview) -> bool,
    ) -> Option<Cell> {
        let mut cells: Vec<Cell> = (near.y.saturating_sub(14)..=(near.y + 14).min(CELLS - 1))
            .flat_map(|y| {
                (near.x.saturating_sub(14)..=(near.x + 14).min(CELLS - 1))
                    .map(move |x| Cell::new(x, y))
            })
            .collect();
        cells.sort_by_key(|c| (c.x.abs_diff(near.x).max(c.y.abs_diff(near.y)), c.y, c.x));
        cells.into_iter().find(|&cell| {
            let preview = self.ask(role, cell, player).preview;
            want(&preview)
        })
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

    /// The middle of `cell` on the screen.
    fn centre_px(&self, cam: &Camera, cell: Cell) -> [f32; 2] {
        let (x, z) = ((cell.x as f32 + 0.5) * 4.0, (cell.y as f32 + 0.5) * 4.0);
        let y = self.ground.height(f64::from(x), f64::from(z)) as f32;
        project_px(&cam.view_proj(), [x, y, z].into(), [W as f32, H as f32])
            .expect("in front of the camera")
            .0
    }

    /// One frame as `sf_game` draws it: the units, then the ghost of `answer` on the UI's world
    /// layer, then the render. Returns the picture and the quads the ghost put on the layer.
    fn shoot(
        &mut self,
        r: &mut Renderer,
        cam: &Camera,
        answer: Option<&GhostAnswer>,
    ) -> (Rgba8, usize) {
        let vp = cam.view_proj();
        r.set_view_proj(vp);
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
        let mut ui = Ui::new(UiConfig::default());
        ui.begin_frame(UiInput::default(), (W, H), 1.0);
        ui.end_frame();
        let mut quads = 0;
        if let Some(answer) = answer {
            let frame = GhostFrame {
                view_proj: vp,
                viewport: [W as f32, H as f32],
                ui_scale: 1.0,
            };
            let height = |x: f32, z: f32| ground.height(f64::from(x), f64::from(z)) as f32;
            let mut layer = ui.world_layer();
            placement_view::draw_ghost(&mut layer, &frame, &height, answer);
            quads = layer.quad_count();
        }
        r.upload_units(&self.instancer);
        let mut ur = UiRenderer::new(&r.gpu().device, r.color_format());
        let img = r
            .render_to_image_with(|t| ur.render(t, &mut ui))
            .expect("render");
        (img, quads)
    }
}

/// The tints of the ghost, as the layer draws them.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Seen {
    Green,
    Red,
    Amber,
    Blue,
}

fn colour_of(tint: Seen) -> sf_ui::Color {
    match tint {
        Seen::Green => placement_view::VALID,
        Seen::Red => placement_view::BLOCKED,
        Seen::Amber => placement_view::ELSEWHERE,
        Seen::Blue => placement_view::EXIT_OPEN,
    }
}

fn to_linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(c: f32) -> i32 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as i32
}

/// The pixel `base` becomes under a translucent `tint`. The UI pass blends straight alpha in the
/// linear colour space of the sRGB surface, so the prediction does too.
fn blended(base: [u8; 4], tint: Seen) -> [i32; 3] {
    let sf_ui::Color(r, g, b, a) = colour_of(tint);
    let a = f32::from(a) / 255.0;
    let mix = |under: u8, over: u8| to_srgb(to_linear(under) * (1.0 - a) + to_linear(over) * a);
    [mix(base[0], r), mix(base[1], g), mix(base[2], b)]
}

/// Which tint the pixel at `p` shows: the one whose blend over the picture without the ghost is
/// the closest to the picture with it, if it is close (a few steps of rounding and of the
/// driver), and none otherwise.
fn seen(base: &Rgba8, img: &Rgba8, p: [f32; 2]) -> Option<Seen> {
    let (x, y) = (p[0] as u32, p[1] as u32);
    let (under, shown) = (base.pixel(x, y), img.pixel(x, y));
    let off = |t: Seen| -> i32 {
        let want = blended(under, t);
        (0..3).map(|i| (want[i] - i32::from(shown[i])).abs()).sum()
    };
    [Seen::Green, Seen::Red, Seen::Amber, Seen::Blue]
        .into_iter()
        .map(|t| (off(t), t))
        .min_by_key(|(d, _)| *d)
        .filter(|(d, _)| *d <= 18)
        .map(|(_, t)| t)
}

/// Every cell of the answer on the screen, with what the ghost made of its pixel.
fn tints(
    scene: &Scene,
    cam: &Camera,
    base: &Rgba8,
    img: &Rgba8,
    answer: &GhostAnswer,
) -> Vec<(Cell, CellVerdict, Option<Seen>)> {
    answer
        .preview
        .cells
        .iter()
        .filter(|(_, v)| *v != CellVerdict::OffMap)
        .map(|&(cell, verdict)| (cell, verdict, seen(base, img, scene.centre_px(cam, cell))))
        .collect()
}

/// Looks at the place for `role`: returns the frames without and with the ghost.
fn look(
    scene: &mut Scene,
    r: &mut Renderer,
    answer: &GhostAnswer,
) -> (Camera, Rgba8, Rgba8, usize) {
    let fp = answer.preview.footprint.unwrap();
    let at = answer.query.cell;
    let focus = [
        (at.x as f32 + f32::from(fp.width) * 0.5) * 4.0,
        (at.y as f32 + f32::from(fp.height) * 0.5) * 4.0,
    ];
    let cam = scene.camera(0.12, focus);
    let (base, none) = scene.shoot(r, &cam, None);
    assert_eq!(none, 0, "no ghost, nothing drawn");
    let (img, quads) = scene.shoot(r, &cam, Some(answer));
    (cam, base, img, quads)
}

fn all_free(p: &PlacementPreview) -> bool {
    p.cells.iter().all(|c| c.1 == CellVerdict::Free)
}

fn mixed(p: &PlacementPreview, bad: CellVerdict) -> bool {
    p.cells.iter().any(|c| c.1 == CellVerdict::Free) && p.cells.iter().any(|c| c.1 == bad)
}

#[test]
fn a_valid_place_is_green_on_every_cell() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let tick = scene.world.tick().0;
    for (owner, role, name) in [
        (MERIDIAN, "barracks", "meridian_barracks"),
        (VEYRAN, "barracks", "veyran_barracks"),
        (MERIDIAN, "decoy_emplacement", "meridian_decoy"),
        (VEYRAN, "fortified_bunker", "veyran_bunker"),
    ] {
        let base = scene.base_of(owner);
        let cell = scene
            .find(role, owner, base, |p| p.verdict.is_ok())
            .unwrap_or_else(|| panic!("{role}: the simulation allows a place near the base"));
        let answer = scene.ask(role, cell, owner);
        assert_eq!(answer.preview.verdict, Ok(()));
        let (cam, base_img, img, quads) = look(&mut scene, &mut r, &answer);
        save(&format!("valid_{name}_tick{tick}.png"), &img);
        let fp = answer.preview.footprint.unwrap();
        // Every cell is tinted, then the outline of four lines, and a factory's exit cell
        // (a fill and four lines).
        let exit = if answer.preview.exit.is_some() { 5 } else { 0 };
        assert_eq!(
            quads,
            usize::from(fp.width) * usize::from(fp.height) + 4 + exit,
            "{role}"
        );
        for (cell, verdict, tint) in tints(&scene, &cam, &base_img, &img, &answer) {
            assert_eq!(verdict, CellVerdict::Free);
            assert_eq!(tint, Some(Seen::Green), "{role} owner {owner} cell {cell}");
        }
    }
}

#[test]
fn terrain_a_structure_and_distance_each_make_the_ghost_red_or_amber() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let tick = scene.world.tick().0;
    let owner = MERIDIAN;
    let base = scene.base_of(owner);
    let role = "power_plant";

    // Ground that cannot be built on: the cells that are the problem are red, the others amber.
    let on_water = scene
        .find(role, owner, base, |p| {
            p.verdict == Err(PlacementError::BadTerrain) && mixed(p, CellVerdict::BadTerrain)
        })
        .expect("the map has unbuildable ground within reach of the base");
    // A structure in the way.
    let on_structure = scene
        .find(role, owner, base, |p| {
            p.verdict == Err(PlacementError::Occupied) && mixed(p, CellVerdict::Structure)
        })
        .expect("the base has structures to overlap");
    // Too far from the base: every cell is fine, the place is not.
    let far = scene
        .find(role, owner, Cell::new(base.x + 14, base.y), |p| {
            p.verdict == Err(PlacementError::TooFarFromBase) && all_free(p)
        })
        .expect("a free place beyond the radius");

    for (what, cell, bad) in [
        ("terrain", on_water, Some(CellVerdict::BadTerrain)),
        ("structure", on_structure, Some(CellVerdict::Structure)),
        ("too_far", far, None),
    ] {
        let answer = scene.ask(role, cell, owner);
        let (cam, base_img, img, _) = look(&mut scene, &mut r, &answer);
        save(&format!("invalid_{what}_tick{tick}.png"), &img);
        let tinted = tints(&scene, &cam, &base_img, &img, &answer);
        assert!(!tinted.is_empty());
        for (c, verdict, tint) in &tinted {
            let expected = match (verdict, bad) {
                (CellVerdict::Free, _) => Seen::Amber,
                (v, Some(b)) if *v == b => Seen::Red,
                other => panic!("{what}: {other:?}"),
            };
            assert_eq!(*tint, Some(expected), "{what}: cell {c} is {verdict:?}");
        }
        // A refused place has both kinds of cell, or only amber ones.
        let reds = tinted.iter().filter(|t| t.2 == Some(Seen::Red)).count();
        if bad.is_some() {
            assert!(reds >= 1, "{what}: the cell that is the problem is red");
        } else {
            assert_eq!(reds, 0, "{what}: nothing is wrong with the cells");
        }
    }
}

#[test]
fn the_wrong_factions_structure_is_amber_for_each_side() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let tick = scene.world.tick().0;
    // Meridian asks for the Veyran bunker and Veyran for the Meridian decoy: free ground next to
    // each base, refused for the faction.
    for (owner, role, name) in [
        (MERIDIAN, "fortified_bunker", "meridian_asks_bunker"),
        (VEYRAN, "decoy_emplacement", "veyran_asks_decoy"),
    ] {
        let base = scene.base_of(owner);
        let cell = scene
            .find(role, owner, base, |p| {
                p.verdict == Err(PlacementError::WrongFaction) && all_free(p)
            })
            .expect("free ground next to the base");
        let answer = scene.ask(role, cell, owner);
        let (cam, base_img, img, _) = look(&mut scene, &mut r, &answer);
        save(&format!("wrong_faction_{name}_tick{tick}.png"), &img);
        for (c, _, tint) in tints(&scene, &cam, &base_img, &img, &answer) {
            assert_eq!(
                tint,
                Some(Seen::Amber),
                "{role} for owner {owner}: cell {c}"
            );
        }
    }
}

#[test]
fn a_factory_shows_its_exit_cell_in_blue_and_in_red_when_it_is_blocked() {
    let mut scene = Scene::new();
    let mut r = renderer(&scene.map);
    let tick = scene.world.tick().0;
    let owner = MERIDIAN;
    let base = scene.base_of(owner);
    let role = "vehicle_factory";
    let open = scene
        .find(role, owner, base, |p| {
            p.verdict.is_ok() && p.exit.is_some_and(|e| e.usable)
        })
        .expect("a place for the factory with its exit free");
    let answer = scene.ask(role, open, owner);
    let exit = answer.preview.exit.unwrap().cell;
    let (cam, base_img, img, quads) = look(&mut scene, &mut r, &answer);
    save(&format!("factory_exit_tick{tick}.png"), &img);
    // 12 cells, the outline (4), the exit cell (1) and its outline (4).
    assert_eq!(quads, 12 + 4 + 1 + 4);
    assert_eq!(
        seen(&base_img, &img, scene.centre_px(&cam, exit)),
        Some(Seen::Blue),
        "the exit cell {exit}"
    );

    // A place whose exit is blocked (by ground or a structure), if the map has one near the base.
    if let Some(blocked) = scene.find(role, owner, base, |p| {
        p.verdict == Err(PlacementError::ExitBlocked) && all_free(p)
    }) {
        let answer = scene.ask(role, blocked, owner);
        let exit = answer.preview.exit.map(|e| e.cell);
        let (cam, base_img, img, _) = look(&mut scene, &mut r, &answer);
        save(&format!("factory_exit_blocked_tick{tick}.png"), &img);
        if let Some(exit) = exit {
            assert_eq!(
                seen(&base_img, &img, scene.centre_px(&cam, exit)),
                Some(Seen::Red),
                "the blocked exit {exit}"
            );
        }
        for (c, _, tint) in tints(&scene, &cam, &base_img, &img, &answer) {
            assert_eq!(tint, Some(Seen::Amber), "cell {c}");
        }
    }
}
