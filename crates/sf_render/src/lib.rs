//! wgpu rendering: terrain, models, camera, zoom, effects and LOD.
//!
//! WP1.4 basics: device/surface setup ([`gpu`]), a forward renderer with depth
//! buffer and one directional light + ambient ([`renderer`]), placeholder
//! meshes and scene ([`mesh`], [`scene`]), shader hot-reload in debug builds
//! and offscreen rendering to PNG for screenshot tests ([`image`]).
//!
//! WP1.5 adds the game camera ([`camera`]): log zoom toward the cursor, pitch
//! that follows the zoom, damped panning, plus a 256x256 m world scene.
//!
//! WP1.7 adds the terrain ([`terrain`]): 32x32-cell chunk meshes from an
//! `sf_data` map and a procedural material texture array.
//!
//! This crate does not depend on `sf_sim` or on a windowing library; the
//! window is passed in as a `wgpu::SurfaceTarget` (e.g. `Arc<winit::Window>`).

pub mod camera;
pub mod gpu;
pub mod image;
pub mod mesh;
pub mod renderer;
pub mod scene;
#[cfg(debug_assertions)]
mod shader_watch;
pub mod terrain;
mod terrain_gpu;

pub use camera::{Camera, CameraConfig, CameraInput};
pub use gpu::{Gpu, GpuOptions, RenderError};
pub use image::{Rgba8, Tolerance};
pub use renderer::{EMBEDDED_SHADER, Renderer};
/// Re-exported so callers can name wgpu types (e.g. `SurfaceTarget`) without a second dependency.
pub use wgpu;
