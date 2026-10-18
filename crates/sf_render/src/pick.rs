//! Picking per zoom (WP1.9, PLAN.md section 2.3): which unit is under the
//! cursor. This module holds the geometry and the types; the search over the
//! units is [`crate::instancing::Instancer::pick`], which looks at exactly
//! what the last [`build`](crate::instancing::Instancer::build_frame) drew.
//!
//! # What is clickable
//! * **Zoomed in (models):** the ray from the cursor against the model's
//!   oriented bounding box ([`ray_obb`]), at the position the model is drawn
//!   at: the interpolated position and facing of the frame and the terrain
//!   height it stands on. A model smaller than `icons.click_min_px` on the
//!   screen gets a bigger box, so a distant unit is still easy to hit (and
//!   the click target does not shrink below the icon that replaces the
//!   model). The nearest hit along the ray wins. With the terrain known
//!   ([`PickQuery::with_ground`]: the exact ray-terrain hit of WP1.8, stored
//!   in [`PickQuery::terrain_t`]) a model entered behind the point where the
//!   ray meets the ground is hidden by the hill and does not count.
//! * **Strategic zoom (icons):** the icon's screen quad ([`icon_hit`]), the
//!   same snapped square the shader draws, `size_px` wide. The terrain does
//!   not hide icons (they are not depth tested), so it does not hide them
//!   from the cursor either. When quads overlap, the icon whose centre is
//!   nearest to the cursor wins; on a tie the one nearer to the camera.
//! * **In the cross-fade band:** the representation with the larger weight
//!   ([`Blend::dominant`](crate::icons::Blend::dominant)): the model while
//!   it is at least half of the picture, the icon beyond that. Never both,
//!   so a unit is not picked through a layer that is mostly gone.
//!
//! Ties between equal hits go to the lower [`UnitId`], so the answer does not
//! depend on the order the units were stored in.

use std::f32::consts::TAU;

use glam::{DMat4, DVec4, Mat4, Vec3};

use crate::model::EyeView;

/// A handle for a unit that the picking returns: the simulation's entity id
/// packed into a number (`sf_render` does not depend on `sf_sim`; the game
/// converts with [`UnitId::from_parts`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnitId(pub u64);

impl UnitId {
    /// "No id given": the instancer numbers such units by their place in the
    /// tick's list.
    pub const NONE: UnitId = UnitId(u64::MAX);

    /// From a slot index and its generation (an `EntityId`'s two parts).
    pub const fn from_parts(index: u32, generation: u32) -> Self {
        UnitId(((generation as u64) << 32) | index as u64)
    }

    pub const fn index(self) -> u32 {
        self.0 as u32
    }

    pub const fn generation(self) -> u32 {
        (self.0 >> 32) as u32
    }
}

impl std::fmt::Display for UnitId {
    /// Like `EntityId`: `index`v`generation`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}v{}", self.index(), self.generation())
    }
}

/// A ray through the cursor: `origin + t * dir` with a unit `dir`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickRay {
    pub origin: Vec3,
    pub dir: Vec3,
}

/// What to pick at: the cursor and the view of the frame that was drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickQuery {
    /// Cursor in physical pixels, origin top left.
    pub cursor: [f32; 2],
    /// The view-projection the frame was drawn with.
    pub view_proj: Mat4,
    /// Viewport size in physical pixels.
    pub viewport: [f32; 2],
    /// Distance along [`PickQuery::ray`] to the terrain, if the caller has
    /// it (`Ground::raycast`): models behind it are hidden by the ground.
    pub terrain_t: Option<f32>,
}

impl PickQuery {
    pub fn new(cursor: [f32; 2], view_proj: Mat4, viewport: [f32; 2]) -> Self {
        Self {
            cursor,
            view_proj,
            viewport,
            terrain_t: None,
        }
    }

    pub fn with_terrain(mut self, terrain_t: Option<f32>) -> Self {
        self.terrain_t = terrain_t;
        self
    }

    /// Where the cursor ray meets `ground` (the exact ray-terrain hit of
    /// WP1.8: [`Ground::raycast`](crate::ground::Ground::raycast)), so a model
    /// behind a hill is hidden by it. A ray that never meets the ground
    /// hides nothing.
    pub fn with_ground(self, ground: &dyn crate::ground::Ground) -> Self {
        let hit = self
            .ray()
            .and_then(|r| ground.raycast(r.origin.as_dvec3(), r.dir.as_dvec3()))
            .map(|t| t as f32);
        self.with_terrain(hit)
    }

    /// The ray from the near plane through the cursor, or `None` for a
    /// degenerate view.
    pub fn ray(&self) -> Option<PickRay> {
        let [w, h] = self.viewport;
        if !(w > 0.0 && h > 0.0) {
            return None;
        }
        let inverse =
            DMat4::from_cols_array(&self.view_proj.to_cols_array().map(f64::from)).inverse();
        let ndc = (
            2.0 * f64::from(self.cursor[0]) / f64::from(w) - 1.0,
            1.0 - 2.0 * f64::from(self.cursor[1]) / f64::from(h),
        );
        let unproject = |z: f64| {
            let p = inverse * DVec4::new(ndc.0, ndc.1, z, 1.0);
            p.truncate() / p.w
        };
        let (near, far) = (unproject(0.0), unproject(1.0));
        let dir = (far - near).normalize();
        (dir.is_finite() && near.is_finite()).then(|| PickRay {
            origin: near.as_vec3(),
            dir: dir.as_vec3(),
        })
    }
}

/// Through which representation a unit was picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickVia {
    /// The ray hit the model's bounding box.
    Model,
    /// The cursor is inside the icon's screen quad.
    Icon,
}

/// A unit under the cursor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pick {
    pub id: UnitId,
    pub via: PickVia,
    /// Where it was hit: the distance along the ray for a model, the
    /// distance from the eye for an icon.
    pub distance: f32,
    /// Pixels from the cursor to the unit's icon centre (icons) or to the
    /// projection of the model's box centre (models).
    pub cursor_distance: f32,
    /// Index of the unit's record in the instancer's last frame, for the
    /// highlight.
    pub(crate) record: u32,
}

/// Where the ray enters the box of a unit standing at `pos` (the point on
/// the ground under its centre) facing `steps` (1/256 turns, as in
/// `model_matrix`), with the footprint and height of `size`
/// `(along the facing, height, across)`, grown by `margin` metres on every
/// side. Returns the distance along the ray (`0` when the ray starts inside),
/// `None` for a miss. The box is the mesh's bounding box; the placeholder
/// meshes fit it.
pub fn ray_obb(ray: &PickRay, pos: Vec3, steps: f32, size: [f32; 3], margin: f32) -> Option<f32> {
    let (s, c) = (steps * (TAU / 256.0)).sin_cos();
    // The unit's own axes: along the facing, up, across.
    let to_local = |v: Vec3| Vec3::new(v.x * c + v.z * s, v.y, -v.x * s + v.z * c);
    let o = to_local(ray.origin - pos);
    let d = to_local(ray.dir);
    let lo = Vec3::new(-0.5 * size[0] - margin, -margin, -0.5 * size[2] - margin);
    let hi = Vec3::new(
        0.5 * size[0] + margin,
        size[1] + margin,
        0.5 * size[2] + margin,
    );
    let (mut t0, mut t1) = (0.0_f32, f32::INFINITY);
    for axis in 0..3 {
        let (oa, da) = (o[axis], d[axis]);
        if da.abs() < 1e-12 {
            if oa < lo[axis] || oa > hi[axis] {
                return None;
            }
            continue;
        }
        let (a, b) = ((lo[axis] - oa) / da, (hi[axis] - oa) / da);
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
        if t0 > t1 {
            return None;
        }
    }
    Some(t0)
}

/// Whether the ray meets the axis-aligned box `min..max` at a distance of
/// zero or more (the tile pruning).
pub fn ray_hits_aabb(ray: &PickRay, min: Vec3, max: Vec3) -> bool {
    let (mut t0, mut t1) = (0.0_f32, f32::INFINITY);
    for axis in 0..3 {
        let (oa, da) = (ray.origin[axis], ray.dir[axis]);
        if da.abs() < 1e-12 {
            if oa < min[axis] || oa > max[axis] {
                return false;
            }
            continue;
        }
        let (a, b) = ((min[axis] - oa) / da, (max[axis] - oa) / da);
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
        if t0 > t1 {
            return false;
        }
    }
    true
}

/// A world point in physical pixels (origin top left) and its clip `w`, or
/// `None` behind the camera.
pub fn project_px(view_proj: &Mat4, p: Vec3, viewport: [f32; 2]) -> Option<([f32; 2], f32)> {
    let clip = *view_proj * p.extend(1.0);
    if clip.w <= 1e-6 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some((
        [
            (ndc.x * 0.5 + 0.5) * viewport[0],
            (0.5 - ndc.y * 0.5) * viewport[1],
        ],
        clip.w,
    ))
}

/// Whether the cursor is inside the icon quad centred at `center` (already
/// snapped, as the shader draws it) of side `size_px`; the boundary counts as
/// inside. Returns the cursor's distance to the centre.
pub fn icon_hit(cursor: [f32; 2], center: [f32; 2], size_px: f32) -> Option<f32> {
    let half = 0.5 * size_px;
    let (dx, dy) = (cursor[0] - center[0], cursor[1] - center[1]);
    (dx.abs() <= half && dy.abs() <= half).then(|| dx.hypot(dy))
}

/// How much bigger than its box a model's click volume has to be so that its
/// `width` (the smaller side of the footprint, in metres) is at least
/// `min_px` pixels across on the screen at `distance`: the margin in metres
/// on each side. `0` for a model that is wide enough already.
pub fn click_margin(width: f32, distance: f32, focal_px: f32, min_px: f32) -> f32 {
    if focal_px <= 0.0 {
        return 0.0;
    }
    let wanted = min_px * distance / focal_px;
    ((wanted - width) * 0.5).max(0.0)
}

/// Conservative bound for the tile pruning: the largest [`click_margin`] any
/// unit at `distance` or nearer can need (a model without any width).
pub fn click_reach(distance: f32, eye: &EyeView, min_px: f32) -> f32 {
    click_margin(0.0, distance, eye.focal_px, min_px)
}

/// A rectangle on the screen in physical pixels, origin top left: the drag
/// selection of WP3.3. `x0 <= x1` and `y0 <= y1` always hold.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScreenRect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl ScreenRect {
    /// The rectangle with these two opposite corners, whichever way the drag
    /// went.
    pub fn from_corners(a: [f32; 2], b: [f32; 2]) -> Self {
        Self {
            x0: a[0].min(b[0]),
            y0: a[1].min(b[1]),
            x1: a[0].max(b[0]),
            y1: a[1].max(b[1]),
        }
    }

    /// The whole viewport.
    pub fn viewport(size: [f32; 2]) -> Self {
        Self::from_corners([0.0, 0.0], size)
    }

    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }

    /// Whether the point is inside; the boundary counts as inside.
    pub fn contains(&self, p: [f32; 2]) -> bool {
        p[0] >= self.x0 && p[0] <= self.x1 && p[1] >= self.y0 && p[1] <= self.y1
    }

    /// Whether the two rectangles share a point; touching edges count.
    pub fn overlaps(&self, other: &ScreenRect) -> bool {
        self.x0 <= other.x1 && other.x0 <= self.x1 && self.y0 <= other.y1 && other.y0 <= self.y1
    }

    /// The rectangle grown to at least `min_px` wide and tall around its
    /// centre.
    pub fn at_least(&self, min_px: f32) -> Self {
        let (cx, cy) = (0.5 * (self.x0 + self.x1), 0.5 * (self.y0 + self.y1));
        let (hw, hh) = (
            (0.5 * self.width()).max(0.5 * min_px),
            (0.5 * self.height()).max(0.5 * min_px),
        );
        Self {
            x0: cx - hw,
            y0: cy - hh,
            x1: cx + hw,
            y1: cy + hh,
        }
    }
}

/// What to pick in a drag: the rectangle and the view of the frame that was
/// drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectQuery {
    pub rect: ScreenRect,
    /// The view-projection the frame was drawn with.
    pub view_proj: Mat4,
    /// Viewport size in physical pixels.
    pub viewport: [f32; 2],
}

/// Whether the icon quad centred at `center` (already snapped, as the shader
/// draws it) of side `size_px` shares a point with `rect`; touching counts.
pub fn rect_hits_icon(rect: &ScreenRect, center: [f32; 2], size_px: f32) -> bool {
    let half = 0.5 * size_px;
    rect.overlaps(&ScreenRect {
        x0: center[0] - half,
        y0: center[1] - half,
        x1: center[0] + half,
        y1: center[1] + half,
    })
}

/// The screen rectangle that covers a model's box: the eight corners of the
/// box of a unit standing at `pos` facing `steps` (as in [`ray_obb`]), with
/// the footprint and height of `size`, projected. A corner behind the camera
/// is left out; `None` if every corner is.
pub fn model_screen_rect(
    view_proj: &Mat4,
    viewport: [f32; 2],
    pos: Vec3,
    steps: f32,
    size: [f32; 3],
) -> Option<ScreenRect> {
    let (s, c) = (steps * (TAU / 256.0)).sin_cos();
    // The inverse of `to_local` in `ray_obb`.
    let to_world = |l: Vec3| pos + Vec3::new(l.x * c - l.z * s, l.y, l.x * s + l.z * c);
    let (hx, hz) = (0.5 * size[0], 0.5 * size[2]);
    let mut rect: Option<ScreenRect> = None;
    for corner in 0..8_u8 {
        let local = Vec3::new(
            if corner & 1 == 0 { -hx } else { hx },
            if corner & 2 == 0 { 0.0 } else { size[1] },
            if corner & 4 == 0 { -hz } else { hz },
        );
        let Some((p, _)) = project_px(view_proj, to_world(local), viewport) else {
            continue;
        };
        rect = Some(match rect {
            None => ScreenRect::from_corners(p, p),
            Some(r) => ScreenRect {
                x0: r.x0.min(p[0]),
                y0: r.y0.min(p[1]),
                x1: r.x1.max(p[0]),
                y1: r.y1.max(p[1]),
            },
        });
    }
    rect
}

/// Whether a model's box, on the screen, shares a point with `rect`. The
/// box counts at least `min_px` wide and tall (the click size of a small
/// model, `icons.click_min_px`), so a unit too far away to see well can still
/// be dragged over.
pub fn rect_hits_model(
    rect: &ScreenRect,
    view_proj: &Mat4,
    viewport: [f32; 2],
    pos: Vec3,
    steps: f32,
    size: [f32; 3],
    min_px: f32,
) -> bool {
    model_screen_rect(view_proj, viewport, pos, steps, size)
        .is_some_and(|r| rect.overlaps(&r.at_least(min_px)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{Camera, CameraConfig, DVec2};

    fn ray(origin: [f32; 3], dir: [f32; 3]) -> PickRay {
        PickRay {
            origin: Vec3::from(origin),
            dir: Vec3::from(dir).normalize(),
        }
    }

    #[test]
    fn unit_id_round_trips_and_prints_like_an_entity_id() {
        let id = UnitId::from_parts(7, 3);
        assert_eq!((id.index(), id.generation()), (7, 3));
        assert_eq!(id.to_string(), "7v3");
        assert_ne!(id, UnitId::from_parts(7, 4));
        assert_ne!(UnitId::NONE, UnitId::from_parts(0, 0));
        let big = UnitId::from_parts(u32::MAX - 1, u32::MAX - 1);
        assert_eq!(
            (big.index(), big.generation()),
            (u32::MAX - 1, u32::MAX - 1)
        );
    }

    #[test]
    fn a_ray_through_the_box_hits_at_the_near_face() {
        // A 2 x 1 x 1 box (along the facing x, height, across) standing at the origin.
        let size = [2.0, 1.0, 1.0];
        let down = ray([0.3, 10.0, 0.2], [0.0, -1.0, 0.0]);
        let t = ray_obb(&down, Vec3::ZERO, 0.0, size, 0.0).unwrap();
        assert!((t - 9.0).abs() < 1e-5, "top face at height 1: {t}");
        // From the side, along x: enters at x = -1.
        let side = ray([-10.0, 0.5, 0.0], [1.0, 0.0, 0.0]);
        assert!((ray_obb(&side, Vec3::ZERO, 0.0, size, 0.0).unwrap() - 9.0).abs() < 1e-5);
        // Starting inside: distance 0.
        let inside = ray([0.0, 0.5, 0.0], [0.0, 1.0, 0.0]);
        assert_eq!(ray_obb(&inside, Vec3::ZERO, 0.0, size, 0.0), Some(0.0));
    }

    #[test]
    fn a_ray_past_the_box_misses() {
        let size = [2.0, 1.0, 1.0];
        for r in [
            ray([1.2, 10.0, 0.0], [0.0, -1.0, 0.0]),  // past the end
            ray([0.0, 10.0, 0.6], [0.0, -1.0, 0.0]),  // past the side
            ray([-10.0, 1.2, 0.0], [1.0, 0.0, 0.0]),  // over the top
            ray([-10.0, -0.1, 0.0], [1.0, 0.0, 0.0]), // under the floor
            ray([10.0, 0.5, 0.0], [1.0, 0.0, 0.0]),   // pointing away
            ray([0.0, 10.0, 0.0], [0.0, 1.0, 0.0]),   // pointing up
        ] {
            assert_eq!(ray_obb(&r, Vec3::ZERO, 0.0, size, 0.0), None, "{r:?}");
        }
    }

    #[test]
    fn the_box_turns_with_the_facing() {
        // 4 m long along x, 1 m across: a ray down at (1.5, _, 0) hits when
        // facing +x and misses after a quarter turn (then it is 4 m along z).
        let size = [4.0, 1.0, 1.0];
        let r = ray([1.5, 5.0, 0.0], [0.0, -1.0, 0.0]);
        assert!(ray_obb(&r, Vec3::ZERO, 0.0, size, 0.0).is_some());
        assert!(ray_obb(&r, Vec3::ZERO, 64.0, size, 0.0).is_none());
        let r = ray([0.0, 5.0, 1.5], [0.0, -1.0, 0.0]);
        assert!(ray_obb(&r, Vec3::ZERO, 0.0, size, 0.0).is_none());
        assert!(ray_obb(&r, Vec3::ZERO, 64.0, size, 0.0).is_some());
        // An eighth turn: the diagonal (1.2, 1.2) is inside, (1.2, -1.2) is not (it is a 4 x 1 box on the diagonal).
        let diag = ray([1.2, 5.0, 1.2], [0.0, -1.0, 0.0]);
        let anti = ray([1.2, 5.0, -1.2], [0.0, -1.0, 0.0]);
        assert!(ray_obb(&diag, Vec3::ZERO, 32.0, size, 0.0).is_some());
        assert!(ray_obb(&anti, Vec3::ZERO, 32.0, size, 0.0).is_none());
    }

    #[test]
    fn the_box_stands_on_its_position() {
        let size = [1.0, 1.0, 1.0];
        let pos = Vec3::new(20.0, 7.0, -3.0);
        let r = ray([20.0, 20.0, -3.0], [0.0, -1.0, 0.0]);
        let t = ray_obb(&r, pos, 0.0, size, 0.0).unwrap();
        assert!(
            (t - 12.0).abs() < 1e-5,
            "the top is 1 m over the ground at 7: {t}"
        );
        // Under the ground point: nothing.
        let under = ray([20.0, 5.0, -3.0], [0.0, -1.0, 0.0]);
        assert_eq!(ray_obb(&under, pos, 0.0, size, 0.0), None);
    }

    #[test]
    fn a_margin_grows_the_box_on_every_side() {
        let size = [1.0, 1.0, 1.0];
        let past = ray([0.7, 10.0, 0.0], [0.0, -1.0, 0.0]);
        assert_eq!(ray_obb(&past, Vec3::ZERO, 0.0, size, 0.0), None);
        assert!(ray_obb(&past, Vec3::ZERO, 0.0, size, 0.25).is_some());
        let above = ray([-10.0, 1.2, 0.0], [1.0, 0.0, 0.0]);
        assert_eq!(ray_obb(&above, Vec3::ZERO, 0.0, size, 0.0), None);
        assert!(ray_obb(&above, Vec3::ZERO, 0.0, size, 0.25).is_some());
    }

    #[test]
    fn click_margin_makes_small_models_big_enough_and_leaves_big_ones() {
        // 1000 px focal length, 14 px wanted: at 100 m that is 1.4 m.
        let m = click_margin(0.5, 100.0, 1000.0, 14.0);
        assert!((m - 0.45).abs() < 1e-5, "{m}");
        // Already big enough: no margin.
        assert_eq!(click_margin(3.0, 100.0, 1000.0, 14.0), 0.0);
        assert_eq!(click_margin(1.0, 100.0, 0.0, 14.0), 0.0);
        // The grown box is exactly `min_px` across on the screen.
        let d = 0.5 + 2.0 * m;
        assert!((d * 1000.0 / 100.0 - 14.0).abs() < 1e-3);
    }

    #[test]
    fn the_icon_quad_boundary_is_inclusive() {
        let center = [100.5, 50.5];
        assert!(icon_hit([100.5, 50.5], center, 14.0).is_some());
        assert!(
            icon_hit([107.5, 50.5], center, 14.0).is_some(),
            "right edge"
        );
        assert!(icon_hit([93.5, 57.5], center, 14.0).is_some(), "a corner");
        assert!(icon_hit([107.51, 50.5], center, 14.0).is_none());
        assert!(icon_hit([100.5, 43.49], center, 14.0).is_none());
        assert!(icon_hit([93.49, 57.5], center, 14.0).is_none());
        let d = icon_hit([103.5, 54.5], center, 14.0).unwrap();
        assert!((d - 5.0).abs() < 1e-5);
    }

    fn camera(zoom: f64) -> Camera {
        let cfg = CameraConfig {
            map_size: DVec2::new(256.0, 256.0),
            ..CameraConfig::default()
        };
        let mut c = Camera::new(cfg, 1280, 720);
        c.set_zoom(zoom);
        c.set_focus(DVec2::new(100.0, 120.0));
        c
    }

    #[test]
    fn the_cursor_ray_runs_through_the_eye_and_the_screen_centre_hits_the_focus() {
        for zoom in [0.0, 0.4, 1.0] {
            let cam = camera(zoom);
            let eye = cam.eye();
            let q = PickQuery::new([640.0, 360.0], cam.view_proj(), [1280.0, 720.0]);
            let r = q.ray().unwrap();
            // The centre ray goes through the eye ...
            let to_eye = Vec3::new(eye.x as f32, eye.y as f32, eye.z as f32) - r.origin;
            let off = to_eye - r.dir * to_eye.dot(r.dir);
            assert!(
                off.length() < 0.05 * cam.distance() as f32 * 1e-2 + 0.02,
                "zoom {zoom}: {off}"
            );
            // ... and lands on the ground at the focus.
            let t = -r.origin.y / r.dir.y;
            let hit = r.origin + r.dir * t;
            assert!(
                (hit.x - 100.0).abs() < 0.05 && (hit.z - 120.0).abs() < 0.05,
                "{hit}"
            );
            // A ray through another pixel is another direction.
            let other = PickQuery::new([100.0, 100.0], cam.view_proj(), [1280.0, 720.0])
                .ray()
                .unwrap();
            assert!(other.dir.dot(r.dir) < 0.9999);
        }
        assert!(
            PickQuery::new([0.0, 0.0], Mat4::ZERO, [10.0, 10.0])
                .ray()
                .is_none()
        );
        assert!(
            PickQuery::new([0.0, 0.0], Mat4::IDENTITY, [0.0, 10.0])
                .ray()
                .is_none()
        );
    }

    #[test]
    fn projection_agrees_with_the_ray() {
        let cam = camera(0.3);
        let vp = cam.view_proj();
        // A ground point is projected to a pixel, and the ray through that
        // pixel passes through the point.
        let p = Vec3::new(104.0, 0.0, 117.0);
        let (px, w) = project_px(&vp, p, [1280.0, 720.0]).unwrap();
        assert!(w > 0.0);
        let r = PickQuery::new(px, vp, [1280.0, 720.0]).ray().unwrap();
        let t = (p - r.origin).dot(r.dir);
        assert!((r.origin + r.dir * t - p).length() < 0.02);
        // Behind the camera there is no pixel.
        let eye = cam.eye();
        let behind = Vec3::new(eye.x as f32, eye.y as f32 + 100.0, eye.z as f32 + 400.0);
        assert!(project_px(&vp, behind, [1280.0, 720.0]).is_none());
    }

    #[test]
    fn a_screen_rect_is_normalised_and_overlaps_inclusively() {
        let r = ScreenRect::from_corners([30.0, 40.0], [10.0, 5.0]);
        assert_eq!((r.x0, r.y0, r.x1, r.y1), (10.0, 5.0, 30.0, 40.0));
        assert_eq!((r.width(), r.height()), (20.0, 35.0));
        assert!(r.contains([10.0, 40.0]) && !r.contains([9.9, 20.0]));
        let touching = ScreenRect::from_corners([30.0, 0.0], [50.0, 100.0]);
        assert!(
            r.overlaps(&touching) && touching.overlaps(&r),
            "edges touch"
        );
        let apart = ScreenRect::from_corners([30.01, 0.0], [50.0, 100.0]);
        assert!(!r.overlaps(&apart));
        // A degenerate rectangle (a point) still overlaps what it is inside.
        let point = ScreenRect::from_corners([20.0, 20.0], [20.0, 20.0]);
        assert!(r.overlaps(&point));
        assert_eq!(
            ScreenRect::viewport([640.0, 360.0]),
            ScreenRect::from_corners([0.0, 0.0], [640.0, 360.0])
        );
    }

    #[test]
    fn at_least_grows_a_small_rectangle_around_its_centre_only() {
        let small = ScreenRect::from_corners([10.0, 10.0], [14.0, 12.0]);
        let g = small.at_least(20.0);
        assert_eq!((g.width(), g.height()), (20.0, 20.0));
        assert_eq!((g.x0 + g.x1) * 0.5, 12.0);
        assert_eq!((g.y0 + g.y1) * 0.5, 11.0);
        let big = ScreenRect::from_corners([0.0, 0.0], [50.0, 40.0]);
        assert_eq!(big.at_least(20.0), big);
    }

    #[test]
    fn an_icon_quad_is_hit_by_a_rectangle_that_touches_it() {
        let rect = ScreenRect::from_corners([100.0, 100.0], [150.0, 150.0]);
        // Quad of 14 px: its edge is 7 px from the centre.
        assert!(rect_hits_icon(&rect, [125.0, 125.0], 14.0), "inside");
        assert!(rect_hits_icon(&rect, [157.0, 125.0], 14.0), "touching");
        assert!(!rect_hits_icon(&rect, [157.1, 125.0], 14.0), "a hair off");
        assert!(
            rect_hits_icon(&rect, [93.0, 93.0], 14.0),
            "corner to corner"
        );
        assert!(!rect_hits_icon(&rect, [92.9, 93.0], 14.0));
        assert!(
            rect_hits_icon(&rect, [125.0, 125.0], 400.0),
            "quad covers the rectangle"
        );
    }

    #[test]
    fn a_model_box_projects_to_a_rectangle_that_covers_its_corners() {
        let cam = camera(0.3);
        let vp = cam.view_proj();
        let viewport = [1280.0, 720.0];
        let pos = Vec3::new(104.0, 0.0, 117.0);
        let size = [4.0, 2.0, 2.0];
        let r = model_screen_rect(&vp, viewport, pos, 0.0, size).unwrap();
        for (x, y, z) in [
            (-2.0, 0.0, -1.0),
            (2.0, 2.0, 1.0),
            (2.0, 0.0, -1.0),
            (-2.0, 2.0, 1.0),
        ] {
            let (p, _) = project_px(&vp, pos + Vec3::new(x, y, z), viewport).unwrap();
            assert!(r.contains(p), "{p:?} in {r:?}");
        }
        // A quarter turn swaps the footprint's sides: the box is a different shape.
        let turned = model_screen_rect(&vp, viewport, pos, 64.0, size).unwrap();
        assert_ne!((r.width(), r.height()), (turned.width(), turned.height()));
        // Turning it around gives the same rectangle (the box is symmetric).
        let half = model_screen_rect(&vp, viewport, pos, 128.0, size).unwrap();
        assert!((half.x0 - r.x0).abs() < 1e-3 && (half.y1 - r.y1).abs() < 1e-3);
        // Behind the camera there is nothing to cover.
        let eye = cam.eye();
        let behind = Vec3::new(eye.x as f32, eye.y as f32 + 100.0, eye.z as f32 + 400.0);
        assert!(model_screen_rect(&vp, viewport, behind, 0.0, size).is_none());
    }

    #[test]
    fn a_model_is_hit_by_a_rectangle_over_its_box_not_beside_it() {
        let cam = camera(0.3);
        let vp = cam.view_proj();
        let viewport = [1280.0, 720.0];
        let pos = Vec3::new(104.0, 0.0, 117.0);
        let size = [4.0, 2.0, 2.0];
        let box_rect = model_screen_rect(&vp, viewport, pos, 0.0, size).unwrap();
        let inside = ScreenRect::from_corners(
            [box_rect.x0 + 1.0, box_rect.y0 + 1.0],
            [box_rect.x0 + 3.0, box_rect.y0 + 3.0],
        );
        assert!(rect_hits_model(&inside, &vp, viewport, pos, 0.0, size, 0.0));
        let beside = ScreenRect::from_corners(
            [box_rect.x1 + 40.0, box_rect.y0],
            [box_rect.x1 + 80.0, box_rect.y1],
        );
        assert!(!rect_hits_model(
            &beside, &vp, viewport, pos, 0.0, size, 0.0
        ));
        // The minimum size reaches the rectangle beside it.
        assert!(rect_hits_model(
            &beside, &vp, viewport, pos, 0.0, size, 400.0
        ));
    }
}
