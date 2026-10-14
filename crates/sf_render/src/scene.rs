//! The built-in test scene, the placeholder camera and the lighting setup.

use bytemuck::{Pod, Zeroable};
use glam::camera::rh;
use glam::{Mat4, Vec3};

/// Which placeholder mesh an object uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshKind {
    Plane,
    Cube,
    Pyramid,
}

impl MeshKind {
    pub const ALL: [MeshKind; 3] = [MeshKind::Plane, MeshKind::Cube, MeshKind::Pyramid];

    pub fn index(self) -> usize {
        self as usize
    }
}

/// Per-instance GPU data: model matrix (column major) and linear RGB color.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Instance {
    pub model: [[f32; 4]; 4],
    pub color: [f32; 4],
}

/// One object of the scene.
#[derive(Clone, Copy, Debug)]
pub struct SceneObject {
    pub mesh: MeshKind,
    pub transform: Mat4,
    /// Linear RGB.
    pub color: [f32; 3],
}

impl SceneObject {
    pub fn instance(&self) -> Instance {
        Instance {
            model: self.transform.to_cols_array_2d(),
            color: [self.color[0], self.color[1], self.color[2], 1.0],
        }
    }
}

/// Directional light plus ambient light (colors are linear RGB).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lighting {
    /// Direction the light travels in (from the light into the scene).
    pub direction: Vec3,
    pub color: Vec3,
    pub ambient: Vec3,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            direction: Vec3::new(0.5, -0.8, 0.4),
            color: Vec3::new(0.85, 0.82, 0.75),
            ambient: Vec3::new(0.22, 0.25, 0.30),
        }
    }
}

/// Fixed placeholder view and projection. WP1.5 replaces this with the real
/// camera; until then the view looks north (-Z) at the scene from the south (+Z, +X is east/right),
/// from above at about 45 degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaceholderCamera {
    pub eye: Vec3,
    pub target: Vec3,
    pub fov_y_radians: f32,
}

impl Default for PlaceholderCamera {
    fn default() -> Self {
        Self {
            eye: Vec3::new(0.0, 9.0, 9.5),
            target: Vec3::new(0.0, 0.3, -0.5),
            fov_y_radians: 45_f32.to_radians(),
        }
    }
}

impl PlaceholderCamera {
    /// View-projection matrix with wgpu clip space (depth `0..1`).
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let view = rh::view::look_at_mat4(self.eye, self.target, Vec3::Y);
        // wgpu/WebGPU clip space: depth 0..1, Y up.
        let proj = rh::proj::directx::perspective(self.fov_y_radians, aspect.max(1e-3), 0.1, 200.0);
        proj * view
    }
}

/// Ground plane, two cubes and a pyramid, each in its own hue so a wrong
/// color is easy to see in the screenshot test.
pub fn test_scene() -> Vec<SceneObject> {
    let at = |x: f32, z: f32, yaw_deg: f32, scale: Vec3| {
        Mat4::from_translation(Vec3::new(x, 0.0, z))
            * Mat4::from_rotation_y(yaw_deg.to_radians())
            * Mat4::from_scale(scale)
    };
    vec![
        SceneObject {
            mesh: MeshKind::Plane,
            transform: Mat4::from_scale(Vec3::new(14.0, 1.0, 14.0)),
            color: [0.10, 0.28, 0.08],
        },
        SceneObject {
            mesh: MeshKind::Cube,
            transform: at(-2.4, 0.0, 20.0, Vec3::splat(1.6)),
            color: [0.80, 0.08, 0.06],
        },
        SceneObject {
            mesh: MeshKind::Cube,
            transform: at(0.8, -2.2, -15.0, Vec3::splat(1.1)),
            color: [0.06, 0.15, 0.80],
        },
        SceneObject {
            mesh: MeshKind::Pyramid,
            transform: at(2.8, 0.6, 0.0, Vec3::new(1.8, 2.0, 1.8)),
            color: [0.85, 0.65, 0.05],
        },
    ]
}

/// GPU uniform block, mirrors `Globals` in `scene.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Globals {
    pub view_proj: [[f32; 4]; 4],
    /// Unit vector from the surface towards the light.
    pub light_dir: [f32; 4],
    pub light_color: [f32; 4],
    pub ambient: [f32; 4],
}

impl Globals {
    pub fn new(view_proj: Mat4, lighting: &Lighting) -> Self {
        let to_light = (-lighting.direction).normalize_or_zero();
        Self {
            view_proj: view_proj.to_cols_array_2d(),
            light_dir: to_light.extend(0.0).to_array(),
            light_color: lighting.color.extend(0.0).to_array(),
            ambient: lighting.ambient.extend(0.0).to_array(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_center_projects_inside_clip_volume() {
        let cam = PlaceholderCamera::default();
        let clip = cam.view_proj(1.0) * Vec3::new(0.0, 0.5, 0.0).extend(1.0);
        let ndc = clip.truncate() / clip.w;
        assert!(ndc.x.abs() < 1.0 && ndc.y.abs() < 1.0);
        assert!((0.0..1.0).contains(&ndc.z));
    }

    #[test]
    fn globals_light_points_towards_the_light() {
        let g = Globals::new(Mat4::IDENTITY, &Lighting::default());
        // The light travels downwards, so towards-light has positive y.
        assert!(g.light_dir[1] > 0.0);
        let len = (g.light_dir[0].powi(2) + g.light_dir[1].powi(2) + g.light_dir[2].powi(2)).sqrt();
        assert!((len - 1.0).abs() < 1e-5);
    }

    #[test]
    fn globals_size_matches_shader_layout() {
        // mat4x4 (64) + 3 x vec4 (48)
        assert_eq!(std::mem::size_of::<Globals>(), 112);
        assert_eq!(std::mem::size_of::<Instance>(), 80);
    }
}
