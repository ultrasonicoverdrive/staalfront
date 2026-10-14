//! Placeholder meshes generated in code (position + normal per vertex).
//!
//! All meshes sit on the `y = 0` plane with `+Y` up, footprint within
//! `x, z` in `[-0.5, 0.5]`.

use bytemuck::{Pod, Zeroable};

/// One mesh vertex: position and unit normal.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
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
            self.vertices.push(Vertex { position, normal });
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn triangle(&mut self, corners: [[f32; 3]; 3], normal: [f32; 3]) {
        let base = self.vertices.len() as u32;
        for position in corners {
            self.vertices.push(Vertex { position, normal });
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
        assert_eq!(MeshData::cube().indices.len(), 36);
    }
}
