//! A loaded level of detail as world-space triangles.
//!
//! The engine loader gives parts with a mesh each and the transform to the
//! parent part; this puts the parts in the unit's frame (rest pose) so that
//! bounds and the top-down view are what the engine draws.

use glam::{Mat4, Vec3};
use sf_render::glb::LoadedLevel;

/// Twice the area (m^2) below which a triangle counts as degenerate: half a
/// square millimetre.
const DEGENERATE_AREA_X2: f32 = 1e-6;

/// One triangle in the unit's frame (metres, `+Y` up, front `+X`).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Tri {
    pub v: [Vec3; 3],
    /// All three corners have the team weight of the loader (`team` material).
    pub team: bool,
}

/// A level's triangles and their bounding box.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelGeom {
    pub tris: Vec<Tri>,
    pub min: Vec3,
    pub max: Vec3,
    /// Triangles with (next to) no area: they count against the budget and
    /// give their corners no usable normal.
    pub degenerate: usize,
    /// Vertices whose normal is not a unit vector after loading (zero
    /// length): the lighting is undefined there.
    pub zero_normals: usize,
}

impl LevelGeom {
    /// The rest pose of `level`. Parts come parents first.
    pub fn from_level(level: &LoadedLevel) -> Self {
        let mut world: Vec<Mat4> = Vec::with_capacity(level.parts.len());
        let mut tris = Vec::new();
        let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        let (mut degenerate, mut zero_normals) = (0, 0);
        for part in &level.parts {
            zero_normals += part
                .mesh
                .vertices
                .iter()
                .filter(|v| Vec3::from(v.normal).length_squared() < 0.25)
                .count();
            let m = part
                .parent
                .and_then(|p| world.get(p))
                .map_or(part.rest, |parent| *parent * part.rest);
            world.push(m);
            let corner = |i: u32| {
                let v = &part.mesh.vertices[i as usize];
                (
                    m.transform_point3(Vec3::from(v.position)),
                    v.color[3] >= 0.5,
                )
            };
            for tri in part.mesh.indices.chunks_exact(3) {
                let ((a, ta), (b, tb), (c, tc)) = (corner(tri[0]), corner(tri[1]), corner(tri[2]));
                for p in [a, b, c] {
                    min = min.min(p);
                    max = max.max(p);
                }
                if (b - a).cross(c - a).length() < DEGENERATE_AREA_X2 {
                    degenerate += 1;
                }
                tris.push(Tri {
                    v: [a, b, c],
                    team: ta && tb && tc,
                });
            }
        }
        if tris.is_empty() {
            (min, max) = (Vec3::ZERO, Vec3::ZERO);
        }
        Self {
            tris,
            min,
            max,
            degenerate,
            zero_normals,
        }
    }

    /// Size along X, Y and Z.
    pub fn extent(&self) -> Vec3 {
        self.max - self.min
    }

    /// Number of triangles that use the `team` material.
    pub fn team_triangles(&self) -> usize {
        self.tris.iter().filter(|t| t.team).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_render::glb::writer::{self, Node, Prim, Scene};
    use sf_render::glb::{AxisConvention, load_glb};

    /// A unit square on the ground plane, normal up.
    fn quad(material: usize) -> Prim {
        Prim {
            positions: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 0.0, 1.0],
                [0.0, 0.0, 1.0],
            ],
            normals: vec![[0.0, 1.0, 0.0]; 4],
            indices: vec![0, 2, 1, 0, 3, 2],
            material,
            mode: None,
        }
    }

    #[test]
    fn a_child_part_is_placed_by_its_parents() {
        let scene = Scene {
            materials: vec![("steel".into(), [0.5; 3]), ("team".into(), [0.9; 3])],
            nodes: vec![
                Node {
                    name: "m_LOD0".into(),
                    children: vec![1],
                    ..Node::default()
                },
                Node {
                    name: "hull".into(),
                    translation: [10.0, 0.0, 0.0],
                    prim: Some(quad(0)),
                    children: vec![2],
                    ..Node::default()
                },
                Node {
                    name: "turret".into(),
                    translation: [0.0, 5.0, 0.0],
                    prim: Some(quad(1)),
                    ..Node::default()
                },
            ],
            roots: vec![0],
            version: "2.0".into(),
            extra_json: String::new(),
        };
        let model = load_glb(&writer::write(&scene), &AxisConvention::ENGINE).unwrap();
        let g = LevelGeom::from_level(&model.levels[0]);
        assert_eq!(g.tris.len(), 4);
        assert_eq!(g.min, Vec3::new(10.0, 0.0, 0.0));
        assert_eq!(g.max, Vec3::new(11.0, 5.0, 1.0));
        assert_eq!(g.team_triangles(), 2);
        assert_eq!(g.extent(), Vec3::new(1.0, 5.0, 1.0));
    }
}
