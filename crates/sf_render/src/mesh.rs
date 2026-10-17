//! Placeholder meshes generated in code (position, normal and material
//! colour per vertex).
//!
//! All meshes sit on the `y = 0` plane with `+Y` up, footprint within
//! `x, z` in `[-0.5, 0.5]`.

use bytemuck::{Pod, Zeroable};

/// One mesh vertex: position, unit normal and material colour.
///
/// `color.rgb` is the material's base colour (linear light) and `color.a`
/// the **team weight**: 1 takes the instance colour (the player colour), 0
/// keeps the material colour. The placeholder meshes are all team colour
/// ([`Vertex::TEAM`]); glTF models (WP1.10) mark the `team` material with 1
/// and every other material with 0.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
}

impl Vertex {
    /// The vertex colour that takes the instance's colour entirely.
    pub const TEAM: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

    /// A team-coloured vertex (the placeholder meshes).
    pub const fn team(position: [f32; 3], normal: [f32; 3]) -> Self {
        Self {
            position,
            normal,
            color: Self::TEAM,
        }
    }
}

/// CPU-side indexed triangle mesh (counter-clockwise front faces).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl MeshData {
    /// Append a flat-shaded quad (4 corners, counter-clockwise seen from the
    /// side the normal points to).
    fn quad(&mut self, corners: [[f32; 3]; 4], normal: [f32; 3]) {
        let base = self.vertices.len() as u32;
        for position in corners {
            self.vertices.push(Vertex::team(position, normal));
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn triangle(&mut self, corners: [[f32; 3]; 3], normal: [f32; 3]) {
        let base = self.vertices.len() as u32;
        for position in corners {
            self.vertices.push(Vertex::team(position, normal));
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    /// Horizontal square, `y = 0`, normal `+Y`, side 1.
    pub fn plane() -> Self {
        let mut m = Self::default();
        m.quad(
            [
                [-0.5, 0.0, -0.5],
                [-0.5, 0.0, 0.5],
                [0.5, 0.0, 0.5],
                [0.5, 0.0, -0.5],
            ],
            [0.0, 1.0, 0.0],
        );
        m
    }

    /// Unit cube standing on the ground: `y` in `[0, 1]`.
    pub fn cube() -> Self {
        let mut m = Self::default();
        let (lo, hi) = (-0.5, 0.5);
        // +Y (top)
        m.quad(
            [[lo, 1.0, lo], [lo, 1.0, hi], [hi, 1.0, hi], [hi, 1.0, lo]],
            [0.0, 1.0, 0.0],
        );
        // -Y (bottom)
        m.quad(
            [[lo, 0.0, lo], [hi, 0.0, lo], [hi, 0.0, hi], [lo, 0.0, hi]],
            [0.0, -1.0, 0.0],
        );
        // +X
        m.quad(
            [[hi, 0.0, lo], [hi, 1.0, lo], [hi, 1.0, hi], [hi, 0.0, hi]],
            [1.0, 0.0, 0.0],
        );
        // -X
        m.quad(
            [[lo, 0.0, lo], [lo, 0.0, hi], [lo, 1.0, hi], [lo, 1.0, lo]],
            [-1.0, 0.0, 0.0],
        );
        // +Z
        m.quad(
            [[lo, 0.0, hi], [hi, 0.0, hi], [hi, 1.0, hi], [lo, 1.0, hi]],
            [0.0, 0.0, 1.0],
        );
        // -Z
        m.quad(
            [[lo, 0.0, lo], [lo, 1.0, lo], [hi, 1.0, lo], [hi, 0.0, lo]],
            [0.0, 0.0, -1.0],
        );
        m
    }

    /// Four-sided pyramid: base on the ground, apex at `y = 1`.
    pub fn pyramid() -> Self {
        let mut m = Self::default();
        let (lo, hi) = (-0.5_f32, 0.5_f32);
        let apex = [0.0, 1.0, 0.0];
        // Face normal of a side with slope 1 over a half-width of 0.5.
        let k = 0.5_f32.hypot(1.0);
        let (s, c) = (1.0 / k, 0.5 / k);
        m.quad(
            [[lo, 0.0, lo], [hi, 0.0, lo], [hi, 0.0, hi], [lo, 0.0, hi]],
            [0.0, -1.0, 0.0],
        );
        m.triangle([[lo, 0.0, hi], [hi, 0.0, hi], apex], [0.0, c, s]); // +Z
        m.triangle([[hi, 0.0, lo], [lo, 0.0, lo], apex], [0.0, c, -s]); // -Z
        m.triangle([[hi, 0.0, hi], [hi, 0.0, lo], apex], [s, c, 0.0]); // +X
        m.triangle([[lo, 0.0, lo], [lo, 0.0, hi], apex], [-s, c, 0.0]); // -X
        m
    }

    /// Cylinder standing on the ground: radius 0.5, `y` in `[0, 1]`,
    /// [`SEGMENTS`] flat-shaded side facets plus a bottom and a top cap.
    pub fn cylinder() -> Self {
        Self::cylinder_with(SEGMENTS)
    }

    /// A cylinder like [`MeshData::cylinder`] with `segments` side facets
    /// (the lower levels of detail use fewer). At least 3.
    pub fn cylinder_with(segments: usize) -> Self {
        let segments = segments.max(MIN_SEGMENTS);
        let mut m = Self::default();
        for i in 0..segments {
            let (a, b) = ring_pair(i, segments);
            let (a_top, b_top) = ([a[0], 1.0, a[2]], [b[0], 1.0, b[2]]);
            m.quad([b, a, a_top, b_top], ring_mid_normal(i, segments));
            m.triangle([[0.0, 1.0, 0.0], b_top, a_top], [0.0, 1.0, 0.0]);
            m.triangle([[0.0, 0.0, 0.0], a, b], [0.0, -1.0, 0.0]);
        }
        m
    }

    /// Cone standing on the ground: base radius 0.5, apex at `y = 1`,
    /// [`SEGMENTS`] flat-shaded side facets plus a base cap.
    pub fn cone() -> Self {
        Self::cone_with(SEGMENTS)
    }

    /// A cone like [`MeshData::cone`] with `segments` side facets. At least 3.
    pub fn cone_with(segments: usize) -> Self {
        let segments = segments.max(MIN_SEGMENTS);
        let mut m = Self::default();
        // A flat facet rises 1 over the distance of its chord from the axis,
        // 0.5 * cos(pi / segments) (0.5 for a smooth cone, noticeably less
        // for the few facets of a low level of detail), so its normal tilts
        // up by rho / |(rho, 1)|.
        let rho = 0.5 * (std::f32::consts::PI / segments as f32).cos();
        let k = rho.hypot(1.0);
        let (horizontal, up) = (1.0 / k, rho / k);
        for i in 0..segments {
            let (a, b) = ring_pair(i, segments);
            let n = ring_mid_normal(i, segments);
            m.triangle(
                [b, a, [0.0, 1.0, 0.0]],
                [n[0] * horizontal, up, n[2] * horizontal],
            );
            m.triangle([[0.0, 0.0, 0.0], a, b], [0.0, -1.0, 0.0]);
        }
        m
    }

    /// Number of triangles.
    pub fn triangle_count(&self) -> u32 {
        (self.indices.len() / 3) as u32
    }
}

/// Facets around a round placeholder mesh (the full-detail level).
pub const SEGMENTS: usize = 16;

/// Fewest facets a round mesh can have.
pub const MIN_SEGMENTS: usize = 3;

/// Ring points (radius 0.5, `y = 0`) at the start and end of facet `i` of
/// `segments`.
fn ring_pair(i: usize, segments: usize) -> ([f32; 3], [f32; 3]) {
    let point = |j: usize| {
        let angle = j as f32 / segments as f32 * std::f32::consts::TAU;
        [0.5 * angle.cos(), 0.0, 0.5 * angle.sin()]
    };
    (point(i), point(i + 1))
}

/// Unit outward normal (horizontal) of facet `i` of `segments`.
fn ring_mid_normal(i: usize, segments: usize) -> [f32; 3] {
    let angle = (i as f32 + 0.5) / segments as f32 * std::f32::consts::TAU;
    [angle.cos(), 0.0, angle.sin()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(m: &MeshData) {
        assert_eq!(m.indices.len() % 3, 0);
        assert!(m.indices.iter().all(|&i| (i as usize) < m.vertices.len()));
        for tri in m.indices.chunks(3) {
            let p = |i: u32| glam::Vec3::from(m.vertices[i as usize].position);
            let n = glam::Vec3::from(m.vertices[tri[0] as usize].normal);
            let face = (p(tri[1]) - p(tri[0])).cross(p(tri[2]) - p(tri[0]));
            assert!(face.length() > 1e-6, "degenerate triangle");
            // Winding agrees with the stored normal (counter-clockwise front).
            assert!(face.normalize().dot(n) > 0.99, "winding vs normal");
            assert!((n.length() - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn meshes_are_well_formed() {
        check(&MeshData::plane());
        check(&MeshData::cube());
        check(&MeshData::pyramid());
        check(&MeshData::cylinder());
        check(&MeshData::cone());
        assert_eq!(MeshData::cube().indices.len(), 36);
        // 16 sides x 2 triangles + 2 caps x 16 triangles.
        assert_eq!(MeshData::cylinder().indices.len(), 16 * 4 * 3);
        // 16 side triangles + 16 base triangles.
        assert_eq!(MeshData::cone().indices.len(), 16 * 2 * 3);
    }

    #[test]
    fn lower_detail_round_meshes_are_well_formed_and_smaller() {
        for segments in [3, 4, 5, 6, 8, 12, 16, 32] {
            let (cyl, cone) = (
                MeshData::cylinder_with(segments),
                MeshData::cone_with(segments),
            );
            check(&cyl);
            check(&cone);
            assert_eq!(cyl.triangle_count() as usize, segments * 4);
            assert_eq!(cone.triangle_count() as usize, segments * 2);
            for v in cyl.vertices.iter().chain(&cone.vertices) {
                assert!(v.position[0].hypot(v.position[2]) <= 0.5 + 1e-6);
                assert!((0.0..=1.0).contains(&v.position[1]));
            }
        }
        // The default meshes are the 16-facet ones, bit for bit.
        assert_eq!(MeshData::cylinder(), MeshData::cylinder_with(SEGMENTS));
        assert_eq!(MeshData::cone(), MeshData::cone_with(SEGMENTS));
        // Fewer than three facets is not a solid: clamped.
        assert_eq!(MeshData::cylinder_with(0), MeshData::cylinder_with(3));
    }

    #[test]
    fn round_meshes_fit_the_unit_box_and_face_outwards() {
        for m in [MeshData::cylinder(), MeshData::cone()] {
            for v in &m.vertices {
                let [x, y, z] = v.position;
                assert!((0.0..=1.0).contains(&y));
                assert!(x.hypot(z) <= 0.5 + 1e-6);
                // A side facet's normal points away from the axis.
                if v.normal[1].abs() < 0.999 && (x != 0.0 || z != 0.0) {
                    assert!(v.normal[0] * x + v.normal[2] * z > 0.0, "{v:?}");
                }
            }
        }
        let cyl = MeshData::cylinder();
        assert!(cyl.vertices.iter().any(|v| v.normal == [0.0, 1.0, 0.0]));
        assert!(cyl.vertices.iter().any(|v| v.normal == [0.0, -1.0, 0.0]));
        let cone = MeshData::cone();
        assert!(cone.vertices.iter().any(|v| v.position == [0.0, 1.0, 0.0]));
    }
}
