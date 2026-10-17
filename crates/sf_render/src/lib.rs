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
//! WP1.8 adds terrain level of detail and culling ([`lod`]), skirts that close the
//! seams between chunks of different levels, and ray picking against the
//! terrain ([`ground`]) for the camera.
//!
//! WP1.9 adds model levels of detail ([`model`]), strategic icons with a
//! dithered cross-fade ([`icons`]) and picking per zoom ([`pick`]): a chain of
//! meshes per model chosen by projected size with hysteresis, screen-fixed
//! icons in player colour past a zoom threshold, and a click volume that is the
//! model's box when zoomed in and the icon's quad when zoomed out.
//!
//! WP1.11 adds the debug overlay layer ([`overlay`]): the egui painter
//! (`egui-wgpu`) on this crate's wgpu device, the terrain LOD tint and a 1 m cell
//! grid ([`DebugView`]), and draw-call counters taken at the actual draw calls.
//!
//! WP1.10 adds glTF models ([`glb`]): `.glb` loading with the axis/unit
//! conversion to the engine convention done once in the loader, the
//! per-material nodes of a part merged into one mesh (per-vertex material
//! colour, `team` = player colour), and parts (hull, turret, barrel) posed per
//! instance ([`model::pose_parts`]), one draw call per part per level.
//!
//! This crate does not depend on `sf_sim` or on a windowing library; the
//! window is passed in as a `wgpu::SurfaceTarget` (e.g. `Arc<winit::Window>`).

pub mod camera;
pub mod frustum;
pub mod glb;
pub mod gpu;
pub mod ground;
pub mod icons;
pub mod image;
pub mod instancing;
pub mod lod;
pub mod mesh;
pub mod model;
pub mod overlay;
pub mod pick;
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
pub use ground::{FlatGround, Ground, TerrainGround};
pub use icons::{Blend, IconConfig, IconShape, ViewBand};
pub use image::{Rgba8, Tolerance};
pub use instancing::{FrameView, InstanceStats, Instancer, Motion};
pub use lod::{LodConfig, TerrainConfig, TerrainStats};
pub use model::{EyeView, MeshId, ModelLods, ModelSet};
pub use overlay::OverlayFrame;
pub use pick::{Pick, PickQuery, PickRay, PickVia, UnitId};
pub use renderer::{DebugView, DrawStats, EMBEDDED_SHADER, FrameTarget, Renderer};
pub use style::Style;
/// Re-exported so callers can name wgpu types (e.g. `SurfaceTarget`) without a second dependency.
pub use wgpu;
