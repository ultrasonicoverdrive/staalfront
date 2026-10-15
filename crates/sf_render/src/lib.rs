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
//! WP1.6 adds instancing ([`instancing`], [`frustum`], [`style`]): interpolated
//! unit instances from two sim ticks, per-tile frustum culling, one instance
//! buffer and one draw call per placeholder mesh (cube, cylinder, cone).
//!
//! WP1.11 adds the debug overlay layer ([`overlay`]): the egui painter
//! (`egui-wgpu`) on this crate's wgpu device, a terrain LOD tint and a 1 m cell
//! grid ([`DebugView`]), and draw-call counters taken at the actual draw calls.
//!
//! This crate does not depend on `sf_sim` or on a windowing library; the
//! window is passed in as a `wgpu::SurfaceTarget` (e.g. `Arc<winit::Window>`).

pub mod camera;
pub mod frustum;
pub mod gpu;
pub mod image;
pub mod instancing;
pub mod mesh;
pub mod overlay;
pub mod renderer;
pub mod scene;
#[cfg(debug_assertions)]
mod shader_watch;
pub mod style;
pub mod terrain;
mod terrain_gpu;

pub use camera::{Camera, CameraConfig, CameraInput};
pub use frustum::{Aabb, Frustum, Visibility};
pub use gpu::{Gpu, GpuOptions, RenderError};
pub use image::{Rgba8, Tolerance};
pub use instancing::{InstanceStats, Instancer, Motion};
pub use overlay::OverlayFrame;
pub use renderer::{DebugView, DrawStats, EMBEDDED_SHADER, Renderer};
pub use style::Style;
/// Re-exported so callers can name wgpu types (e.g. `SurfaceTarget`) without a second dependency.
pub use wgpu;
