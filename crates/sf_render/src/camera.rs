//! The game camera (PLAN.md §2.1): perspective, north up, logarithmic zoom
//! toward the cursor, pitch that follows the zoom, damped panning.
//!
//! This is presentation code: it uses floats (`f64` internally, so the
//! cursor invariant can be checked to a millimetre; the GPU gets an `f32`
//! matrix). It reads no window, clock or input device: [`Camera::update`]
//! takes the frame time and a plain [`CameraInput`], so everything is unit
//! testable with synthetic input.
//!
//! # World frame
//! `x` east, `y` up, `z` south (the camera looks towards `-z`); one cell is
//! one metre. Ground plane is `y = 0`. [`Camera::focus`] is the ground point
//! at the centre of the screen.
//!
//! # Model
//! * `zoom` in `[0, 1]` is a log-scale parameter:
//!   `distance = min * (max / min) ^ zoom` (eye to focus point).
//! * `pitch = lerp(pitch_near, pitch_far, smoothstep(zoom))`, about 45° close
//!   up, about 80° when the whole map is in view.
//! * The camera hangs south of the focus: `eye = focus + distance * (0, sin
//!   pitch, cos pitch)`. Because the view orientation depends on `zoom` only,
//!   the ground point under a screen position is `focus + offset(zoom,
//!   ndc)`, which makes "keep this ground point under the cursor" a closed
//!   form instead of an iteration.
//!
//! # Cursor guarantees
//! * **Settled:** after a wheel step the ground point that was under the
//!   cursor is under the cursor again once the zoom has settled, within
//!   floating-point error (~1e-9 m; the tests demand 1 cm), for every zoom
//!   level and every cursor position that hits the ground.
//! * **During damping:** the same holds for every frame of the transition:
//!   the focus is recomputed each frame from the anchor (the ground point
//!   and screen position at the time of the wheel step), so the point does
//!   not drift or swim while the zoom eases. If the cursor moves while the
//!   zoom is still easing, the anchor stays at the screen position of the
//!   wheel event.
//! * **Middle-mouse drag** uses the same anchor: the grabbed ground point
//!   follows the cursor exactly (no damping, that would feel like lag).
//! * **Exception:** when the focus would leave the map bounds plus margin
//!   (see [`CameraConfig::margin`]) it is clamped, and the point under the
//!   cursor moves instead. This only happens near the map edge.
//! * **Frame rate:** damping uses exact exponentials, so the same input
//!   sequence at 30 or 144 frames per second ends in the same state (events
//!   are applied at frame boundaries, so mid-transition states of different
//!   frame rates differ by the event quantisation only).

pub use glam::DVec2;
use glam::{DMat3, DMat4, DVec3, DVec4, Mat4};

/// Tunable camera parameters. [`Default`] is the shipped feel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraConfig {
    /// Map size in cells (= metres): `x` east, `y` south.
    pub map_size: DVec2,
    /// How far the focus may leave the map, in metres.
    pub margin: f64,
    /// Closest eye-to-focus distance.
    pub min_distance: f64,
    /// Farthest distance; `None` = just enough to show the whole map.
    pub max_distance: Option<f64>,
    /// Pitch (angle below the horizon) fully zoomed in, radians.
    pub pitch_near: f64,
    /// Pitch fully zoomed out, radians.
    pub pitch_far: f64,
    /// Vertical field of view, radians.
    pub fov_y: f64,
    /// One wheel notch multiplies the distance by this (or its inverse).
    pub distance_per_notch: f64,
    /// Time constant of the zoom easing, seconds.
    pub zoom_smooth_secs: f64,
    /// Time constant of the pan velocity easing, seconds.
    pub pan_smooth_secs: f64,
    /// Pan speed in distances per second (so it scales with the zoom).
    pub pan_speed: f64,
    /// Zoom (`0..=1`) a new camera starts at.
    pub start_zoom: f64,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            map_size: DVec2::new(256.0, 256.0),
            margin: 16.0,
            min_distance: 12.0,
            max_distance: None,
            pitch_near: 45_f64.to_radians(),
            pitch_far: 80_f64.to_radians(),
            fov_y: 45_f64.to_radians(),
            distance_per_notch: 0.85,
            zoom_smooth_secs: 0.10,
            pan_smooth_secs: 0.12,
            pan_speed: 0.7,
            start_zoom: 0.35,
        }
    }
}

/// What the player did this frame, independent of devices.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraInput {
    /// Pan direction, each axis in `-1..=1`: `x` right (east), `y` up (north).
    pub pan: DVec2,
    /// Wheel notches; positive zooms in.
    pub zoom_notches: f64,
    /// Cursor position in physical pixels, `None` if outside the window.
    pub cursor: Option<DVec2>,
    /// Middle mouse button (grab the ground) is held.
    pub drag: bool,
}

/// A ground point that must stay under a screen position.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Anchor {
    ground: DVec2,
    ndc: DVec2,
}

/// The camera. See the module docs for the model and the guarantees.
#[derive(Clone, Debug)]
pub struct Camera {
    cfg: CameraConfig,
    max_distance: f64,
    viewport: DVec2,
    focus: DVec2,
    zoom: f64,
    zoom_target: f64,
    pan_velocity: DVec2,
    anchor: Option<Anchor>,
    dragging: bool,
}

impl Camera {
    /// New camera over the map centre at `cfg.start_zoom`, for a viewport in pixels.
    pub fn new(cfg: CameraConfig, width: u32, height: u32) -> Self {
        let max_distance = max_distance_for(&cfg, width, height);
        let zoom = cfg.start_zoom.clamp(0.0, 1.0);
        Self {
            focus: cfg.map_size * 0.5,
            cfg,
            max_distance,
            viewport: DVec2::new(f64::from(width.max(1)), f64::from(height.max(1))),
            zoom,
            zoom_target: zoom,
            pan_velocity: DVec2::ZERO,
            anchor: None,
            dragging: false,
        }
    }

    pub fn config(&self) -> &CameraConfig {
        &self.cfg
    }

    /// Resize the viewport. Unless `max_distance` is configured, the farthest
    /// distance is recomputed so the whole map still just fits at full
    /// zoom-out for the new aspect ratio (a portrait window needs more
    /// distance than a landscape one). The zoom value stays, so the distance
    /// (and the view) changes with it.
    pub fn set_viewport(&mut self, width: u32, height: u32) {
        self.viewport = DVec2::new(f64::from(width.max(1)), f64::from(height.max(1)));
        self.max_distance = max_distance_for(&self.cfg, width, height);
    }

    pub fn aspect(&self) -> f64 {
        self.viewport.x / self.viewport.y
    }

    /// Ground point at the screen centre.
    pub fn focus(&self) -> DVec2 {
        self.focus
    }

    /// Jump to a focus (clamped to the bounds), cancelling any motion.
    pub fn set_focus(&mut self, focus: DVec2) {
        self.focus = self.clamp_focus(focus);
        self.pan_velocity = DVec2::ZERO;
        self.anchor = None;
    }

    /// Current (eased) zoom, `0` = closest, `1` = whole map.
    pub fn zoom(&self) -> f64 {
        self.zoom
    }

    /// Zoom the easing is heading for.
    pub fn zoom_target(&self) -> f64 {
        self.zoom_target
    }

    /// Jump to a zoom without easing (tests, scripted shots).
    pub fn set_zoom(&mut self, zoom: f64) {
        self.zoom = zoom.clamp(0.0, 1.0);
        self.zoom_target = self.zoom;
        self.anchor = None;
    }

    pub fn max_distance(&self) -> f64 {
        self.max_distance
    }

    /// Eye-to-focus distance at a zoom level.
    pub fn distance_at(&self, zoom: f64) -> f64 {
        self.cfg.min_distance * (self.max_distance / self.cfg.min_distance).powf(zoom)
    }

    /// Pitch (radians below the horizon) at a zoom level.
    pub fn pitch_at(&self, zoom: f64) -> f64 {
        let s = zoom * zoom * (3.0 - 2.0 * zoom); // smoothstep
        self.cfg.pitch_near + (self.cfg.pitch_far - self.cfg.pitch_near) * s
    }

    pub fn distance(&self) -> f64 {
        self.distance_at(self.zoom)
    }

    pub fn pitch(&self) -> f64 {
        self.pitch_at(self.zoom)
    }

    /// Eye position in world space.
    pub fn eye(&self) -> DVec3 {
        let (d, p) = (self.distance(), self.pitch());
        DVec3::new(self.focus.x, d * p.sin(), self.focus.y + d * p.cos())
    }

    /// Near and far plane, scaled with the distance to keep depth precision.
    pub fn near_far(&self) -> (f64, f64) {
        let d = self.distance();
        (0.1 * d, 4.0 * d)
    }

    /// Where on the ground (`y = 0`) a screen position in `-1..=1` (y up)
    /// lands, relative to the focus, for a camera at `zoom`.
    fn ground_offset(&self, zoom: f64, ndc: DVec2) -> Option<DVec2> {
        let (d, p) = (self.distance_at(zoom), self.pitch_at(zoom));
        let (sp, cp) = p.sin_cos();
        let half = (self.cfg.fov_y * 0.5).tan();
        // Camera basis: right = +x, forward = (0, -sp, -cp), up = (0, cp, -sp).
        let dir = DVec3::new(
            ndc.x * half * self.aspect(),
            -sp + ndc.y * half * cp,
            -cp - ndc.y * half * sp,
        );
        if dir.y >= -1e-9 {
            return None; // at or above the horizon
        }
        let eye = DVec3::new(0.0, d * sp, d * cp);
        let s = -eye.y / dir.y;
        Some(DVec2::new(eye.x + s * dir.x, eye.z + s * dir.z))
    }

    fn to_ndc(&self, px: DVec2) -> DVec2 {
        DVec2::new(
            2.0 * px.x / self.viewport.x - 1.0,
            1.0 - 2.0 * px.y / self.viewport.y,
        )
    }

    /// Ground point under a cursor position in pixels (current state).
    pub fn ground_point(&self, cursor_px: DVec2) -> Option<DVec2> {
        let off = self.ground_offset(self.zoom, self.to_ndc(cursor_px))?;
        Some(self.focus + off)
    }

    /// Focus that puts `ground` under `ndc` at `zoom`.
    fn focus_for(&self, anchor: Anchor, zoom: f64) -> Option<DVec2> {
        Some(anchor.ground - self.ground_offset(zoom, anchor.ndc)?)
    }

    fn clamp_focus(&self, f: DVec2) -> DVec2 {
        let m = self.cfg.margin;
        DVec2::new(
            f.x.clamp(-m, self.cfg.map_size.x + m),
            f.y.clamp(-m, self.cfg.map_size.y + m),
        )
    }

    /// Advance by `dt` seconds with this frame's input.
    pub fn update(&mut self, dt: f64, input: &CameraInput) {
        let dt = dt.max(0.0);

        // 1. Drag: grab the ground point under the cursor.
        match (input.drag, input.cursor) {
            (true, Some(cursor)) => {
                let ndc = self.to_ndc(cursor);
                if !self.dragging {
                    // Start the grab at the point under the cursor right now.
                    if let Some(off) = self.ground_offset(self.zoom, ndc) {
                        self.dragging = true;
                        self.anchor = Some(Anchor {
                            ground: self.focus + off,
                            ndc,
                        });
                    }
                } else if let Some(a) = &mut self.anchor {
                    a.ndc = ndc;
                }
                self.pan_velocity = DVec2::ZERO;
            }
            _ => {
                if self.dragging {
                    self.dragging = false;
                    self.anchor = None;
                }
            }
        }

        // 2. Wheel: new zoom target, anchored at the cursor (or screen centre).
        if input.zoom_notches != 0.0 && !self.dragging {
            let ndc = input.cursor.map_or(DVec2::ZERO, |c| self.to_ndc(c));
            // Cursor over the sky (no ground hit): zoom about the centre.
            let (ndc, off) = match self.ground_offset(self.zoom, ndc) {
                Some(off) => (ndc, off),
                None => (DVec2::ZERO, DVec2::ZERO),
            };
            self.anchor = Some(Anchor {
                ground: self.focus + off,
                ndc,
            });
            let per_notch = (1.0 / self.cfg.distance_per_notch).ln()
                / (self.max_distance / self.cfg.min_distance).ln();
            self.zoom_target = (self.zoom_target - input.zoom_notches * per_notch).clamp(0.0, 1.0);
        }

        // 3. Zoom easing (before the pan, so the pan speed can use the mean distance).
        let distance_before = self.distance();
        let diff = self.zoom_target - self.zoom;
        if diff.abs() < 1e-7 {
            self.zoom = self.zoom_target;
        } else {
            self.zoom += diff * (1.0 - (-dt / self.cfg.zoom_smooth_secs).exp());
        }

        // 4. Pan: exact integration of a velocity easing towards the wish.
        if !self.dragging {
            let wish = input.pan.clamp_length_max(1.0)
                * self.cfg.pan_speed
                * 0.5
                * (distance_before + self.distance());
            // north is -z
            let wish = DVec2::new(wish.x, -wish.y);
            let k = 1.0 / self.cfg.pan_smooth_secs;
            let decay = (-k * dt).exp();
            let moved = wish * dt + (self.pan_velocity - wish) * ((1.0 - decay) / k);
            self.pan_velocity = wish + (self.pan_velocity - wish) * decay;
            self.focus += moved;
            if let Some(a) = &mut self.anchor {
                a.ground += moved;
            }
        }

        // 5. Keep the anchored ground point under its screen position.
        if let Some(a) = self.anchor {
            if let Some(f) = self.focus_for(a, self.zoom) {
                self.focus = f;
            }
            if self.zoom == self.zoom_target && !self.dragging {
                self.anchor = None;
            }
        }

        // 6. Bounds. The anchor follows the clamp so it cannot fight it.
        let clamped = self.clamp_focus(self.focus);
        if clamped != self.focus {
            if let Some(a) = &mut self.anchor {
                a.ground += clamped - self.focus;
            }
            self.focus = clamped;
        }
    }

    /// View matrix (world to eye) in `f64`.
    fn view(&self) -> DMat4 {
        let (sp, cp) = self.pitch().sin_cos();
        let right = DVec3::X;
        let up = DVec3::new(0.0, cp, -sp);
        let back = DVec3::new(0.0, sp, cp);
        let rot = DMat3::from_cols(right, up, back).transpose();
        DMat4::from_mat3(rot) * DMat4::from_translation(-self.eye())
    }

    /// Perspective projection with wgpu clip space (depth `0..1`), `f64`.
    fn projection(&self) -> DMat4 {
        let (near, far) = self.near_far();
        let f = 1.0 / (self.cfg.fov_y * 0.5).tan();
        DMat4::from_cols(
            DVec4::new(f / self.aspect(), 0.0, 0.0, 0.0),
            DVec4::new(0.0, f, 0.0, 0.0),
            DVec4::new(0.0, 0.0, far / (near - far), -1.0),
            DVec4::new(0.0, 0.0, near * far / (near - far), 0.0),
        )
    }

    /// View-projection in `f64` (tests unproject through this).
    pub fn view_proj_f64(&self) -> DMat4 {
        self.projection() * self.view()
    }

    /// View-projection for the GPU.
    pub fn view_proj(&self) -> Mat4 {
        Mat4::from_cols_array(&self.view_proj_f64().to_cols_array().map(|v| v as f32))
    }
}

/// Farthest eye distance for a viewport: the configured one, or just enough
/// to show the whole map.
fn max_distance_for(cfg: &CameraConfig, width: u32, height: u32) -> f64 {
    cfg.max_distance
        .unwrap_or_else(|| fit_distance(cfg, width.max(1), height.max(1)))
        .max(cfg.min_distance * 1.001)
}

/// Smallest distance at which the whole map (centred, fully zoomed-out pitch)
/// is inside a `width` x `height` viewport, plus 3 % slack.
fn fit_distance(cfg: &CameraConfig, width: u32, height: u32) -> f64 {
    let probe = |d: f64| -> bool {
        let mut cam = Camera::new(
            CameraConfig {
                max_distance: Some(d.max(cfg.min_distance * 1.001)),
                ..*cfg
            },
            width,
            height,
        );
        cam.set_zoom(1.0);
        let vp = cam.view_proj_f64();
        let c = cfg.map_size;
        [
            DVec2::new(0.0, 0.0),
            DVec2::new(c.x, 0.0),
            DVec2::new(0.0, c.y),
            DVec2::new(c.x, c.y),
        ]
        .iter()
        .all(|p| {
            let clip = vp * DVec4::new(p.x, 0.0, p.y, 1.0);
            clip.w > 0.0 && (clip.x / clip.w).abs() <= 1.0 && (clip.y / clip.w).abs() <= 1.0
        })
    };
    let (mut lo, mut hi) = (cfg.min_distance, 1.0e5);
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        if probe(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi * 1.03
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZES: [(u32, u32); 4] = [(800, 800), (1280, 720), (1920, 1080), (640, 1000)];

    fn cam(w: u32, h: u32) -> Camera {
        Camera::new(CameraConfig::default(), w, h)
    }

    /// Ground point under `px` found independently of the camera's closed
    /// form: unproject through the inverse of the `f32` matrix the GPU gets.
    fn unproject(c: &Camera, px: DVec2) -> DVec2 {
        let vp: DMat4 = DMat4::from_cols_array(&c.view_proj().to_cols_array().map(f64::from));
        let inv = vp.inverse();
        let ndc = c.to_ndc(px);
        let near = inv * DVec4::new(ndc.x, ndc.y, 0.0, 1.0);
        let far = inv * DVec4::new(ndc.x, ndc.y, 1.0, 1.0);
        let (a, b) = (near.truncate() / near.w, far.truncate() / far.w);
        let s = -a.y / (b.y - a.y);
        let p = a + (b - a) * s;
        DVec2::new(p.x, p.z)
    }

    fn cursors(w: u32, h: u32) -> Vec<DVec2> {
        let (w, h) = (f64::from(w), f64::from(h));
        let mut v = vec![DVec2::new(w * 0.5, h * 0.5)];
        for (fx, fy) in [
            (0.5, 0.5),
            (0.25, 0.4),
            (0.75, 0.4),
            (0.3, 0.7),
            (0.8, 0.8),
            (0.1, 0.5),
            (0.9, 0.3),
            (0.5, 0.1),
            (0.5, 0.95),
        ] {
            v.push(DVec2::new(w * fx, h * fy));
        }
        v
    }

    #[test]
    fn range_is_12_m_to_whole_map_and_pitch_follows() {
        let c = cam(1280, 720);
        assert!((c.distance_at(0.0) - 12.0).abs() < 1e-9);
        let max = c.max_distance();
        assert!(max > 256.0 && max < 2000.0, "{max}");
        assert!((c.pitch_at(0.0).to_degrees() - 45.0).abs() < 1e-9);
        assert!((c.pitch_at(1.0).to_degrees() - 80.0).abs() < 1e-9);
        let mut last = 0.0;
        for i in 0..=100 {
            let p = c.pitch_at(f64::from(i) / 100.0);
            assert!(p >= last);
            last = p;
        }
        // Whole map in view at full zoom-out, for any window at least as wide as tall.
        let mut far = cam(1280, 720);
        far.set_zoom(1.0);
        far.set_focus(DVec2::new(128.0, 128.0));
        let vp = far.view_proj_f64();
        for (x, z) in [(0.0, 0.0), (256.0, 0.0), (0.0, 256.0), (256.0, 256.0)] {
            let clip = vp * DVec4::new(x, 0.0, z, 1.0);
            let (nx, ny, nz) = (clip.x / clip.w, clip.y / clip.w, clip.z / clip.w);
            assert!(
                nx.abs() < 1.0 && ny.abs() < 1.0,
                "corner {x},{z}: {nx} {ny}"
            );
            assert!((0.0..1.0).contains(&nz));
        }
    }

    #[test]
    fn closed_form_matches_matrix_unprojection() {
        for (w, h) in SIZES {
            for zoom in [0.0, 0.2, 0.5, 0.8, 1.0] {
                let mut c = cam(w, h);
                c.set_zoom(zoom);
                for px in cursors(w, h) {
                    let a = c.ground_point(px).unwrap();
                    let b = unproject(&c, px);
                    assert!((a - b).length() < 5e-3, "{w}x{h} z={zoom} {px}: {a} vs {b}");
                }
            }
        }
    }

    #[test]
    fn near_far_cover_the_visible_ground() {
        for zoom in [0.0, 0.5, 1.0] {
            let mut c = cam(1920, 1080);
            c.set_zoom(zoom);
            let (near, far) = c.near_far();
            let eye = c.eye();
            for px in cursors(1920, 1080) {
                let g = c.ground_point(px).unwrap();
                let dist = (DVec3::new(g.x, 0.0, g.y) - eye).length();
                assert!(
                    dist > near && dist < far,
                    "zoom {zoom}: {dist} not in {near}..{far}"
                );
            }
        }
    }

    fn wheel(notches: f64, cursor: DVec2) -> CameraInput {
        CameraInput {
            zoom_notches: notches,
            cursor: Some(cursor),
            ..CameraInput::default()
        }
    }

    fn idle(cursor: DVec2) -> CameraInput {
        CameraInput {
            cursor: Some(cursor),
            ..CameraInput::default()
        }
    }

    /// Core of the acceptance test: for every cursor accepted by `keep`, at
    /// every zoom step across the full range in both directions, the ground
    /// point under the cursor stays within 1 cm: on every damped frame, when
    /// settled, and (no drift) after 40 steps. Returns the number of checks.
    fn check_cursor_invariant(cfg: CameraConfig, keep: impl Fn(DVec2) -> bool) -> u32 {
        let mut checked = 0_u32;
        for (w, h) in SIZES {
            for px in cursors(w, h) {
                if !keep(DVec2::new(
                    2.0 * px.x / f64::from(w) - 1.0,
                    1.0 - 2.0 * px.y / f64::from(h),
                )) {
                    continue;
                }
                for direction in [1.0_f64, -1.0] {
                    let mut c = Camera::new(cfg, w, h);
                    // Start at the opposite end so the whole range gets covered.
                    c.set_zoom(if direction > 0.0 { 1.0 } else { 0.0 });
                    c.set_focus(DVec2::new(128.0, 128.0));
                    let anchor_point = c.ground_point(px).unwrap();
                    for _step in 0..40 {
                        // The point under the cursor *now* must survive this step.
                        let before = c.ground_point(px).unwrap();
                        c.update(1.0 / 60.0, &wheel(direction, px));
                        for _frame in 0..90 {
                            c.update(1.0 / 60.0, &idle(px));
                            let now = c.ground_point(px).unwrap();
                            assert!(
                                (now - before).length() < 0.01,
                                "{w}x{h} px={px} dir={direction} focus={} zoom={}: moved {} m",
                                c.focus(),
                                c.zoom(),
                                (now - before).length()
                            );
                            // Same through the matrix that reaches the GPU.
                            assert!((unproject(&c, px) - before).length() < 0.01);
                            checked += 1;
                        }
                        assert_eq!(c.zoom(), c.zoom_target(), "settled");
                    }
                    // The whole range was traversed, and nothing drifted.
                    assert!(c.zoom() == 0.0 || c.zoom() == 1.0);
                    let end = c.ground_point(px).unwrap();
                    assert!(
                        (end - anchor_point).length() < 0.01,
                        "{w}x{h} px={px}: drifted {} m over the whole range",
                        (end - anchor_point).length()
                    );
                }
            }
        }
        checked
    }

    /// THE acceptance test of WP1.5: centred and off-centre cursors (down to
    /// 10 % from the window edge), square, wide and tall windows, no focus
    /// bounds in the way.
    #[test]
    fn cursor_point_stays_within_1_cm_at_every_zoom_step() {
        let cfg = CameraConfig {
            margin: 1.0e9,
            ..CameraConfig::default()
        };
        let checked = check_cursor_invariant(cfg, |_| true);
        assert!(checked > 100_000, "{checked}");
    }

    /// Same with the real map bounds, for cursors in the central 60 % of the
    /// window. Further out, zooming out on a 256 m map can ask for a focus
    /// beyond the bounds (the screen shows ~540 m at full zoom-out); then
    /// the bounds win, see the module docs.
    #[test]
    fn cursor_point_holds_with_default_bounds_near_the_centre() {
        let checked = check_cursor_invariant(CameraConfig::default(), |ndc| {
            ndc.x.abs() <= 0.3 && ndc.y.abs() <= 0.3
        });
        assert!(checked > 10_000, "{checked}");
    }

    /// The exception is real and limited: the point under an edge cursor
    /// gives way only while the focus sits on the bounds.
    #[test]
    fn bounds_win_over_the_cursor_only_at_the_bounds() {
        let (w, h) = (1280, 720);
        let px = DVec2::new(128.0, 360.0);
        let mut c = cam(w, h);
        c.set_zoom(0.0);
        c.set_focus(DVec2::new(128.0, 128.0));
        let before = c.ground_point(px).unwrap();
        c.update(1.0 / 60.0, &wheel(-30.0, px)); // all the way out
        let mut pinned = false;
        for _ in 0..200 {
            c.update(1.0 / 60.0, &idle(px));
            let at_bound = (c.focus().x - 272.0).abs() < 1e-9;
            pinned |= at_bound;
            let moved = (c.ground_point(px).unwrap() - before).length();
            assert!(at_bound || moved < 0.01, "moved {moved} m without clamp");
        }
        assert!(pinned, "expected the bounds to engage in this setup");
    }

    /// Without the damped frames in between: a burst of wheel steps, one per frame.
    #[test]
    fn rapid_wheel_steps_keep_the_point() {
        let (w, h) = (1280, 720);
        let px = DVec2::new(900.0, 200.0);
        let mut c = cam(w, h);
        let before = c.ground_point(px).unwrap();
        for _ in 0..8 {
            c.update(1.0 / 60.0, &wheel(1.0, px));
            assert!((c.ground_point(px).unwrap() - before).length() < 0.01);
        }
        for _ in 0..200 {
            c.update(1.0 / 60.0, &idle(px));
            assert!((c.ground_point(px).unwrap() - before).length() < 0.01);
        }
        assert!(c.zoom() < 0.35);
    }

    #[test]
    fn zoom_is_clamped_and_log_scaled() {
        let mut c = cam(1280, 720);
        let px = DVec2::new(640.0, 360.0);
        for _ in 0..200 {
            c.update(1.0 / 60.0, &wheel(5.0, px));
        }
        for _ in 0..300 {
            c.update(1.0 / 60.0, &idle(px));
        }
        assert_eq!(c.zoom(), 0.0);
        assert!((c.distance() - 12.0).abs() < 1e-9);
        // One notch = constant distance factor anywhere on the scale.
        c.set_zoom(0.5);
        let d0 = c.distance();
        c.update(1.0 / 60.0, &wheel(-1.0, px));
        for _ in 0..300 {
            c.update(1.0 / 60.0, &idle(px));
        }
        assert!((c.distance() / d0 - 1.0 / 0.85).abs() < 1e-6);
        for _ in 0..200 {
            c.update(1.0 / 60.0, &wheel(-5.0, px));
        }
        for _ in 0..300 {
            c.update(1.0 / 60.0, &idle(px));
        }
        assert_eq!(c.zoom(), 1.0);
    }

    #[test]
    fn frame_rate_independent() {
        let run = |hz: f64| {
            let mut c = cam(1280, 720);
            let px = DVec2::new(900.0, 250.0);
            let dt = 1.0 / hz;
            let frames = hz as usize; // 1 s
            for i in 0..frames {
                let input = CameraInput {
                    // One wheel step in the first frame, pan right + up all along.
                    zoom_notches: if i == 0 { 3.0 } else { 0.0 },
                    pan: DVec2::new(1.0, 1.0),
                    cursor: Some(px),
                    drag: false,
                };
                c.update(dt, &input);
            }
            c
        };
        let a = run(30.0);
        let b = run(144.0);
        let c = run(60.0);
        for other in [&b, &c] {
            assert!(
                (a.zoom() - other.zoom()).abs() < 1e-6,
                "{} {}",
                a.zoom(),
                other.zoom()
            );
            assert!(
                (a.focus() - other.focus()).length() < 0.05,
                "{} {}",
                a.focus(),
                other.focus()
            );
        }
    }

    #[test]
    fn pan_speed_scales_with_distance_and_is_damped() {
        let mut close = cam(1280, 720);
        close.set_zoom(0.0);
        let mut far = cam(1280, 720);
        far.set_zoom(0.9);
        far.set_focus(DVec2::new(128.0, 128.0));
        close.set_focus(DVec2::new(128.0, 128.0));
        let east = CameraInput {
            pan: DVec2::new(1.0, 0.0),
            ..CameraInput::default()
        };
        let (c0, f0) = (close.focus(), far.focus());
        // First frame moves less than full speed (velocity eases in).
        close.update(1.0 / 60.0, &east);
        let first = (close.focus() - c0).x;
        assert!(first > 0.0 && first < 0.7 * 12.0 / 60.0, "{first}");
        for _ in 0..59 {
            close.update(1.0 / 60.0, &east);
            far.update(1.0 / 60.0, &east);
        }
        far.update(1.0 / 60.0, &east);
        let (dc, df) = ((close.focus() - c0).x, (far.focus() - f0).x);
        let ratio = df / dc;
        let expect = far.distance() / close.distance();
        assert!(
            (ratio / expect - 1.0).abs() < 0.05,
            "ratio {ratio} vs {expect}"
        );
        // Coasts to a stop after release.
        for _ in 0..120 {
            close.update(1.0 / 60.0, &CameraInput::default());
        }
        let rest = close.focus();
        close.update(1.0 / 60.0, &CameraInput::default());
        assert!((close.focus() - rest).length() < 1e-3);
        // North is -z, diagonal input is not faster.
        let mut n = cam(1280, 720);
        let n0 = n.focus();
        let diag = CameraInput {
            pan: DVec2::new(1.0, 1.0),
            ..CameraInput::default()
        };
        for _ in 0..30 {
            n.update(1.0 / 60.0, &diag);
        }
        let d = n.focus() - n0;
        assert!(d.x > 0.0 && d.y < 0.0);
        assert!((d.x + d.y).abs() < 1e-9);
    }

    #[test]
    fn focus_is_clamped_to_map_plus_margin() {
        let mut c = cam(1280, 720);
        let west = CameraInput {
            pan: DVec2::new(-1.0, 0.0),
            ..CameraInput::default()
        };
        for _ in 0..600 {
            c.update(1.0 / 60.0, &west);
        }
        assert!((c.focus().x + 16.0).abs() < 1e-9, "{}", c.focus());
        let north = CameraInput {
            pan: DVec2::new(0.0, 1.0),
            ..CameraInput::default()
        };
        for _ in 0..600 {
            c.update(1.0 / 60.0, &north);
        }
        assert!((c.focus().y + 16.0).abs() < 1e-9);
    }

    #[test]
    fn drag_keeps_the_grabbed_point_under_the_cursor() {
        let mut c = cam(1280, 720);
        for zoom in [0.0, 0.4, 1.0] {
            c.set_zoom(zoom);
            c.set_focus(DVec2::new(128.0, 128.0));
            let start = DVec2::new(700.0, 300.0);
            let grabbed = c.ground_point(start).unwrap();
            let mut cursor = start;
            // Frame 0 presses the button at `start`.
            c.update(
                1.0 / 60.0,
                &CameraInput {
                    cursor: Some(start),
                    drag: true,
                    ..CameraInput::default()
                },
            );
            for i in 0..30 {
                cursor += DVec2::new(-9.0, 4.0);
                let input = CameraInput {
                    cursor: Some(cursor),
                    drag: true,
                    ..CameraInput::default()
                };
                c.update(1.0 / 60.0, &input);
                let under = c.ground_point(cursor).unwrap();
                assert!(
                    (under - grabbed).length() < 0.01,
                    "zoom {zoom} frame {i}: {} m off",
                    (under - grabbed).length()
                );
            }
            // The camera really moved, and releasing the button stops it dead.
            assert!((c.focus() - DVec2::new(128.0, 128.0)).length() > 0.5);
            let at_release = c.focus();
            c.update(1.0 / 60.0, &idle(cursor));
            c.update(1.0 / 60.0, &idle(cursor));
            assert!((c.focus() - at_release).length() < 1e-9);
        }
    }

    #[test]
    fn zooming_while_dragging_keeps_the_grab() {
        let mut c = cam(1280, 720);
        let px = DVec2::new(400.0, 500.0);
        let grabbed = c.ground_point(px).unwrap();
        for i in 0..90 {
            let input = CameraInput {
                cursor: Some(px),
                drag: true,
                zoom_notches: if i == 0 { 2.0 } else { 0.0 },
                ..CameraInput::default()
            };
            c.update(1.0 / 60.0, &input);
            assert!((c.ground_point(px).unwrap() - grabbed).length() < 0.01);
        }
    }

    #[test]
    fn wheel_without_cursor_zooms_about_the_screen_centre() {
        let mut c = cam(1280, 720);
        let f = c.focus();
        c.update(
            1.0 / 60.0,
            &CameraInput {
                zoom_notches: 2.0,
                ..CameraInput::default()
            },
        );
        for _ in 0..200 {
            c.update(1.0 / 60.0, &CameraInput::default());
        }
        assert!((c.focus() - f).length() < 1e-9);
        assert!(c.zoom() < 0.35);
    }

    #[test]
    fn panning_during_a_zoom_does_not_fight_the_anchor() {
        let mut c = cam(1280, 720);
        let px = DVec2::new(900.0, 200.0);
        c.update(1.0 / 60.0, &wheel(4.0, px));
        let pan = CameraInput {
            pan: DVec2::new(1.0, 0.0),
            cursor: Some(px),
            ..CameraInput::default()
        };
        let f0 = c.focus();
        for _ in 0..30 {
            c.update(1.0 / 60.0, &pan);
        }
        assert!(
            c.focus().x > f0.x + 1.0,
            "pan was swallowed by the zoom anchor"
        );
    }
    /// All four map corners are inside the view at full zoom-out.
    fn whole_map_visible(c: &Camera) -> bool {
        let vp = c.view_proj_f64();
        let m = c.config().map_size;
        [(0.0, 0.0), (m.x, 0.0), (0.0, m.y), (m.x, m.y)]
            .iter()
            .all(|&(x, z)| {
                let clip = vp * DVec4::new(x, 0.0, z, 1.0);
                clip.w > 0.0 && (clip.x / clip.w).abs() <= 1.0 && (clip.y / clip.w).abs() <= 1.0
            })
    }

    #[test]
    fn whole_map_fits_at_max_zoom_for_any_aspect_ratio() {
        for (w, h) in [
            (1920, 1080),
            (1000, 1000),
            (720, 1280),
            (400, 2000),
            (3440, 1440),
        ] {
            let mut c = Camera::new(CameraConfig::default(), w, h);
            c.set_zoom(1.0);
            assert!(whole_map_visible(&c), "{w}x{h}");
        }
    }

    #[test]
    fn resizing_to_portrait_refits_the_map() {
        let mut c = Camera::new(CameraConfig::default(), 1920, 1080);
        c.set_zoom(1.0);
        let landscape = c.distance();
        c.set_viewport(600, 1200);
        assert!(
            c.distance() > landscape * 1.2,
            "{} vs {landscape}",
            c.distance()
        );
        assert!(whole_map_visible(&c));
        // Back to landscape: same distance again.
        c.set_viewport(1920, 1080);
        assert!((c.distance() - landscape).abs() < 1e-9);
    }
}
