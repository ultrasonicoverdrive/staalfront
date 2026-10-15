//! Minimap input without a GPU: a click or drag moves the camera focus to
//! the world point under the cursor, the click does not reach the game, and
//! the field-of-view frame follows the camera afterwards.

use sf_data::map::generate::generate;
use sf_render::camera::DVec2;
use sf_render::{Camera, CameraConfig};
use sf_ui::{Anchor, Minimap, MinimapGeometry, MouseButton, Rect, Ui, UiConfig, UiInput, fov_quad};

const SCREEN: (u32, u32) = (1280, 720);

struct Fixture {
    ui: Ui,
    minimap: Minimap,
    camera: Camera,
    frame: Rect,
}

impl Fixture {
    fn new() -> Self {
        let map = generate(128, 128, 3);
        let camera = Camera::new(
            CameraConfig {
                map_size: DVec2::new(128.0, 128.0),
                ..CameraConfig::default()
            },
            SCREEN.0,
            SCREEN.1,
        );
        let ui = Ui::new(UiConfig::default());
        let frame = ui.place(
            Rect::new(0.0, 0.0, SCREEN.0 as f32, SCREEN.1 as f32),
            Anchor::BottomRight,
            (10.0, 10.0),
            (200.0, 200.0),
        );
        Self {
            ui,
            minimap: Minimap::bake(&map, 128),
            camera,
            frame,
        }
    }

    fn geometry(&self) -> MinimapGeometry {
        MinimapGeometry::new(self.frame.inset(2.0), (128.0, 128.0))
    }

    /// One frame as the game does it; moves the camera on a minimap click.
    /// Returns the input the game would act on.
    fn frame(&mut self, raw: UiInput) -> UiInput {
        self.ui.begin_frame(raw, SCREEN, 1.0);
        let fov = fov_quad(
            &self.camera,
            (SCREEN.0 as f32, SCREEN.1 as f32),
            (128.0, 128.0),
        );
        let target = self
            .ui
            .minimap(&self.minimap, "minimap", self.frame, Some(fov), []);
        self.ui.end_frame();
        if let Some((x, z)) = target {
            self.camera
                .set_focus(DVec2::new(f64::from(x), f64::from(z)));
        }
        self.ui.filter_for_game(&raw)
    }
}

fn mouse(cursor: (f32, f32), pressed: bool, down: bool, released: bool) -> UiInput {
    let mut i = UiInput {
        cursor: Some(cursor),
        ..UiInput::default()
    };
    i.pressed[0] = pressed;
    i.down[0] = down;
    i.released[0] = released;
    i
}

fn close(a: DVec2, x: f64, z: f64) -> bool {
    (a.x - x).abs() < 0.05 && (a.y - z).abs() < 0.05
}

#[test]
fn clicking_the_minimap_moves_the_camera_there_and_the_click_is_consumed() {
    let mut f = Fixture::new();
    let g = f.geometry();
    let (sx, sy) = g.world_to_screen(30.0, 100.0);
    let game = f.frame(mouse((sx, sy), true, true, false));
    assert!(
        close(f.camera.focus(), 30.0, 100.0),
        "focus {:?}",
        f.camera.focus()
    );
    assert!(
        !game.pressed(MouseButton::Left),
        "the camera/selection never sees the click"
    );
    assert!(f.ui.press_consumed(MouseButton::Left));

    // The release passes, and ends the capture.
    let game = f.frame(mouse((sx, sy), false, false, true));
    assert!(game.released(MouseButton::Left));
    assert!(!f.ui.button_captured(MouseButton::Left));
    assert!(
        close(f.camera.focus(), 30.0, 100.0),
        "release does not move it"
    );
}

#[test]
fn dragging_across_the_minimap_follows_the_cursor_and_clamps_outside() {
    let mut f = Fixture::new();
    let g = f.geometry();
    let a = g.world_to_screen(20.0, 20.0);
    f.frame(mouse(a, true, true, false));
    assert!(close(f.camera.focus(), 20.0, 20.0));
    let b = g.world_to_screen(90.0, 40.0);
    let game = f.frame(mouse(b, false, true, false));
    assert!(close(f.camera.focus(), 90.0, 40.0), "drag follows");
    assert!(
        !game.down(MouseButton::Left),
        "the drag is the UI's, not the game's"
    );
    // The cursor leaves the minimap past the east edge: the focus clamps to the map.
    let out = (g.content().right() + 300.0, g.world_to_screen(0.0, 64.0).1);
    f.frame(mouse(out, false, true, false));
    assert!(
        close(f.camera.focus(), 128.0, 64.0),
        "clamped to the map: {:?}",
        f.camera.focus()
    );
    f.frame(mouse(out, false, false, true));
    // After the release the cursor over open ground does nothing to the camera.
    let before = f.camera.focus();
    f.frame(mouse((100.0, 100.0), false, false, false));
    assert_eq!(f.camera.focus(), before);
}

#[test]
fn a_press_outside_the_minimap_does_not_move_the_camera() {
    let mut f = Fixture::new();
    let before = f.camera.focus();
    let game = f.frame(mouse((300.0, 300.0), true, true, false));
    assert_eq!(f.camera.focus(), before);
    assert!(
        game.pressed(MouseButton::Left),
        "ordinary ground clicks reach the game"
    );
}

#[test]
fn the_wheel_over_the_minimap_is_not_a_zoom() {
    let mut f = Fixture::new();
    let g = f.geometry();
    let mut raw = mouse(g.world_to_screen(64.0, 64.0), false, false, false);
    raw.wheel = 3.0;
    let game = f.frame(raw);
    assert_eq!(game.wheel, 0.0);
    let mut raw = mouse((300.0, 300.0), false, false, false);
    raw.wheel = 3.0;
    assert_eq!(f.frame(raw).wheel, 3.0);
}

#[test]
fn the_frame_follows_the_camera_after_a_click() {
    let mut f = Fixture::new();
    f.camera.set_zoom(0.2);
    let g = f.geometry();
    let before = fov_quad(&f.camera, (1280.0, 720.0), (128.0, 128.0));
    let (sx, sy) = g.world_to_screen(100.0, 100.0);
    f.frame(mouse((sx, sy), true, true, false));
    let after = fov_quad(&f.camera, (1280.0, 720.0), (128.0, 128.0));
    // The frame's centre moved to the clicked point (the view is centred on the focus).
    let mid = |q: &[(f32, f32); 4]| {
        (
            q.iter().map(|p| p.0).sum::<f32>() / 4.0,
            q.iter().map(|p| p.1).sum::<f32>() / 4.0,
        )
    };
    let (bx, _) = mid(&before);
    let (ax, az) = mid(&after);
    assert!(
        (ax - 100.0).abs() < 6.0 && az > 80.0 && bx < 100.0,
        "frame centre ({ax}, {az})"
    );
}
