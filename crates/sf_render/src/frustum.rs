//! View frustum extracted from a view-projection matrix, for CPU culling.

use glam::{Mat4, Vec3, Vec4};

/// An axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }
}

/// The six planes of a view frustum; a point is inside when it is on the
/// positive side of all of them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frustum {
    /// `(normal.xyz, d)` with a unit normal; left, right, bottom, top, near, far.
    planes: [Vec4; 6],
}

/// Where a box lies relative to the frustum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    Outside,
    /// Crosses at least one plane (or is classified conservatively as such).
    Straddling,
    Inside,
}

impl Frustum {
    /// Planes of a wgpu view-projection (clip depth `0..1`, as produced by
    /// the camera and the placeholder camera).
    pub fn from_view_proj(view_proj: Mat4) -> Self {
        let m = view_proj;
        let row = |i: usize| Vec4::new(m.x_axis[i], m.y_axis[i], m.z_axis[i], m.w_axis[i]);
        let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));
        let raw = [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2, r3 - r2];
        Self {
            planes: raw.map(|p| {
                let len = p.truncate().length();
                if len > 0.0 { p / len } else { p }
            }),
        }
    }

    /// Classify `aabb`. `Outside` means fully behind one plane. `Straddling`
    /// may also be reported for a box outside near a frustum corner (the
    /// usual plane-only test), which only costs a few extra draws.
    pub fn classify(&self, aabb: &Aabb) -> Visibility {
        let mut inside = true;
        for p in &self.planes {
            let n = p.truncate();
            // Corner furthest along the normal / furthest against it.
            let positive = n.cmpge(Vec3::ZERO);
            let far = Vec3::select(positive, aabb.max, aabb.min);
            let near = Vec3::select(positive, aabb.min, aabb.max);
            if n.dot(far) + p.w < 0.0 {
                return Visibility::Outside;
            }
            if n.dot(near) + p.w < 0.0 {
                inside = false;
            }
        }
        if inside {
            Visibility::Inside
        } else {
            Visibility::Straddling
        }
    }

    /// True unless `aabb` is entirely outside.
    pub fn intersects(&self, aabb: &Aabb) -> bool {
        self.classify(aabb) != Visibility::Outside
    }

    pub fn contains_point(&self, p: Vec3) -> bool {
        self.planes
            .iter()
            .all(|pl| pl.truncate().dot(p) + pl.w >= 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::PlaceholderCamera;

    fn frustum() -> Frustum {
        Frustum::from_view_proj(PlaceholderCamera::default().view_proj(16.0 / 9.0))
    }

    fn cube(center: Vec3, half: f32) -> Aabb {
        Aabb::new(center - Vec3::splat(half), center + Vec3::splat(half))
    }

    #[test]
    fn box_in_front_of_the_camera_is_inside() {
        let f = frustum();
        assert_eq!(
            f.classify(&cube(Vec3::new(0.0, 0.3, -0.5), 0.2)),
            Visibility::Inside
        );
    }

    #[test]
    fn boxes_far_to_the_side_behind_or_beyond_far_are_outside() {
        let f = frustum();
        for c in [
            Vec3::new(500.0, 0.0, 0.0),
            Vec3::new(-500.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 400.0), // behind the eye (eye is at z = 9.5)
            Vec3::new(0.0, 0.0, -900.0), // beyond the far plane
            Vec3::new(0.0, 500.0, 0.0),
            Vec3::new(0.0, -500.0, 0.0),
        ] {
            assert_eq!(f.classify(&cube(c, 1.0)), Visibility::Outside, "{c}");
        }
    }

    #[test]
    fn box_crossing_an_edge_straddles() {
        let f = frustum();
        // A huge box around the eye crosses every plane.
        assert_eq!(
            f.classify(&cube(Vec3::new(0.0, 5.0, 5.0), 50.0)),
            Visibility::Straddling
        );
        // A box centred on the left boundary at the target depth.
        let edge = (-30..0)
            .map(|i| i as f32)
            .find(|&x| f.contains_point(Vec3::new(x, 0.3, -0.5)))
            .expect("boundary exists");
        let b = cube(Vec3::new(edge, 0.3, -0.5), 1.5);
        assert_eq!(f.classify(&b), Visibility::Straddling);
    }

    #[test]
    fn contains_point_agrees_with_clip_space() {
        let vp = PlaceholderCamera::default().view_proj(16.0 / 9.0);
        let f = Frustum::from_view_proj(vp);
        for x in -12..=12 {
            for z in -12..=12 {
                let p = Vec3::new(x as f32, 0.0, z as f32);
                let c = vp * p.extend(1.0);
                let ndc = c.truncate() / c.w;
                let in_clip = c.w > 0.0
                    && ndc.x.abs() <= 1.0
                    && ndc.y.abs() <= 1.0
                    && (0.0..=1.0).contains(&ndc.z);
                // Points within a hair of a plane may disagree by rounding.
                let margin = ndc.x.abs().max(ndc.y.abs()) > 0.999;
                if !margin {
                    assert_eq!(f.contains_point(p), in_clip, "{p}");
                }
            }
        }
    }
}
