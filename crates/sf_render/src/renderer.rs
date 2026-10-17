//! The forward renderer: window surface or offscreen texture, depth buffer,
//! one directional light + ambient, the built-in test scene.

use std::cell::Cell;
use std::ops::Range;

use glam::Mat4;
use wgpu::util::DeviceExt;

use crate::frustum::Frustum;
use crate::gpu::{Gpu, GpuOptions, RenderError};
use crate::gpu_timer::GpuTimer;
use crate::icons::{IconInstance, IconParams};
use crate::image::Rgba8;
use crate::instancing::Instancer;
use crate::lod::{LodConfig, LodView, TerrainConfig, TerrainStats};
use crate::mesh::{MeshData, Vertex};
use crate::model::{BUILTIN_MESHES, MeshId};
use crate::overlay::{Overlay, OverlayFrame};
use crate::scene::{
    Globals, Instance, Lighting, MeshKind, PlaceholderCamera, SceneObject, test_scene,
};
use crate::terrain::LOD_LEVELS;

/// The shader embedded at build time. Release builds always use this; debug
/// builds start from it and may hot-reload from disk (see
/// [`Renderer::watch_shader_file`]).
pub const EMBEDDED_SHADER: &str = include_str!("../../../assets/shaders/scene.wgsl");

/// The strategic icon shader (WP1.9), embedded at build time. Not
/// hot-reloaded: it is small and stable, and the scene shader's reload
/// path rebuilds a different pipeline.
pub const ICON_SHADER: &str = include_str!("../../../assets/shaders/icon.wgsl");

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Format of the offscreen target (sRGB, so shader output is linear light).
const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// Linear-light clear color (a muted sky blue).
const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.08,
    g: 0.14,
    b: 0.24,
    a: 1.0,
};

enum Target {
    Window {
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
    },
    Offscreen {
        texture: wgpu::Texture,
    },
}

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

/// What an extra pass drawn on top of the scene needs: the device and queue,
/// the frame's command encoder (record a render pass with `LoadOp::Load` on
/// `view`), and the target's format and size in pixels.
pub struct FrameTarget<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub view: &'a wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    pub size: (u32, u32),
}

/// A per-frame instance buffer for one mesh kind (grows, never shrinks).
#[derive(Default)]
struct DynamicInstances {
    buffer: Option<wgpu::Buffer>,
    capacity: u64,
    count: u32,
}

/// Draw-call counters of the last rendered frame, for the debug panel.
///
/// Counted where the `draw_indexed` calls are recorded, so a call that is
/// skipped (or added) in [`Renderer::render`] shows up here by construction.
/// The egui overlay pass is not included.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawStats {
    /// Instances drawn from the per-frame lists (units).
    pub dynamic_instances: u32,
    /// Draw calls for the per-frame instance lists.
    pub dynamic_draw_calls: u32,
    /// Instances drawn from the static scene.
    pub scene_instances: u32,
    /// Draw calls for the static scene objects (one per mesh kind used).
    pub scene_draw_calls: u32,
    /// Draw calls for the terrain: chunks of the same level of detail that
    /// are neighbours in the chunk list share one call.
    pub terrain_draw_calls: u32,
    /// Triangles drawn from the per-frame lists (the unit models).
    pub dynamic_triangles: u32,
    /// Units drawn as strategic icons.
    pub icon_instances: u32,
    /// Draw calls for the icons: 1 when there are any, else 0.
    pub icon_draw_calls: u32,
    /// Terrain chunks in the map.
    pub terrain_chunks_total: u32,
    /// Terrain chunks that passed the frustum test and were drawn.
    pub terrain_chunks_visible: u32,
    /// Terrain triangles drawn (surface and skirts).
    pub terrain_triangles: u32,
    /// Visible terrain chunks per level of detail (index 0 = full detail).
    pub terrain_chunks_per_level: [u32; LOD_LEVELS],
}

impl DrawStats {
    pub fn total_draw_calls(&self) -> u32 {
        self.dynamic_draw_calls
            + self.scene_draw_calls
            + self.terrain_draw_calls
            + self.icon_draw_calls
    }
}

pub struct Renderer {
    gpu: Gpu,
    target: Target,
    format: wgpu::TextureFormat,
    size: (u32, u32),
    depth_view: wgpu::TextureView,
    pipeline_layout: wgpu::PipelineLayout,
    pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    globals_layout: wgpu::BindGroupLayout,
    terrain: Option<crate::terrain_gpu::TerrainGpu>,
    meshes: Vec<GpuMesh>,
    instances: wgpu::Buffer,
    /// `(mesh index, instance range)` per draw call.
    draws: Vec<(usize, Range<u32>)>,
    /// Per-frame instances (units), one buffer per mesh (a kind at a level of detail).
    dynamic: Vec<DynamicInstances>,
    /// `ModelSet::revision` of the extra meshes uploaded (0 = none yet).
    model_revision: u64,
    /// The strategic icons (WP1.9): their own pipeline, one instance buffer
    /// and one draw call. No depth test, drawn after the units.
    icon_pipeline: wgpu::RenderPipeline,
    icon_params: wgpu::Buffer,
    icon_bind_group: wgpu::BindGroup,
    icon_indices: wgpu::Buffer,
    icon_sizes: IconParams,
    icons: DynamicInstances,
    camera: PlaceholderCamera,
    /// When set, replaces the placeholder camera (see [`Renderer::set_view_proj`]).
    view_proj: Option<Mat4>,
    lighting: Lighting,
    /// Counters of the last rendered frame.
    last_stats: Cell<DrawStats>,
    /// Terrain debug visualisations (LOD tint, cell grid).
    debug_view: DebugView,
    /// The egui painter, created on the first [`Renderer::set_overlay`].
    overlay: Option<Overlay>,
    #[cfg(debug_assertions)]
    watch: Option<crate::shader_watch::ShaderWatch>,
    /// How the window presents and what was asked for (WP1.14).
    present: PresentInfo,
    /// GPU frame timing, created by [`Renderer::enable_gpu_timing`].
    gpu_timer: Option<GpuTimer>,
    /// Caller's number for the frames submitted now (GPU timing).
    gpu_frame_tag: u64,
    /// Cumulative GPU buffer allocations made while running.
    allocs: AllocStats,
    /// The next frame is also copied out of the window (see
    /// [`Renderer::capture_next_frame`]).
    capture_requested: bool,
    captured: Option<Rgba8>,
    /// Time the last [`Renderer::render_with`] spent waiting for the swap
    /// chain to hand out an image (the display pacing it, not CPU work).
    acquire_wait: std::time::Duration,
}

/// How the window presents frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresentInfo {
    /// The wgpu present mode in use (`Fifo`, `Immediate`, `Mailbox`, ...),
    /// or `offscreen`.
    pub mode: String,
    /// Set when the requested mode was not available.
    pub note: Option<String>,
}

/// GPU buffers created after start-up, for the hitch analysis: every
/// allocation is a possible stall on the frame that makes it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllocStats {
    /// Instance buffers created or grown (cumulative).
    pub instance_buffers: u64,
    /// Bytes of those buffers (cumulative).
    pub instance_bytes: u64,
}

/// Terrain debug visualisations, switched from the F1 panel (WP1.11).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DebugView {
    /// Tint terrain chunks by their level of detail (`lod::LOD_TINT`: red for
    /// full detail, then orange, yellow, green, blue, purple).
    pub lod_tint: bool,
    /// Opacity `0..=1` of the gameplay cell grid lines (one per 4 m cell,
    /// B12); `0` = off.
    pub grid_alpha: f32,
}

impl DebugView {
    /// Camera distance in cells up to which the grid is fully visible, and
    /// from which it is gone. They are cells, not metres, so that they follow
    /// the cell size (B12: 40 and 140 when a cell was a metre, now 160 m and
    /// 560 m).
    pub const GRID_FULL_UNTIL_CELLS: f64 = 40.0;
    pub const GRID_GONE_AT_CELLS: f64 = 140.0;

    /// Grid opacity for a camera distance in metres: fully visible when
    /// zoomed in, gone when zoomed far out (where the cell lines are
    /// sub-pixel).
    pub fn grid_alpha_for_distance(distance: f64) -> f32 {
        let cell = f64::from(sf_math::render_bridge::METRES_PER_CELL);
        let full_until = Self::GRID_FULL_UNTIL_CELLS * cell;
        let gone_at = Self::GRID_GONE_AT_CELLS * cell;
        (1.0 - (distance - full_until) / (gone_at - full_until)).clamp(0.0, 1.0) as f32
    }
}

impl Renderer {
    /// Renderer drawing into a window. Vsync (`Fifo`) by default.
    ///
    /// `target` is typically `Arc<winit::window::Window>`; sf_render itself
    /// does not depend on winit.
    pub fn for_window(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
        options: &GpuOptions,
    ) -> Result<Self, RenderError> {
        let instance = Gpu::instance(options);
        crate::gpu::trace("create surface");
        let surface = instance
            .create_surface(target)
            .map_err(|e| RenderError::Surface(e.to_string()))?;
        let gpu = Gpu::new(instance, options, Some(&surface))?;
        let (width, height) = (width.max(1), height.max(1));
        let mut config = surface
            .get_default_config(&gpu.adapter, width, height)
            .ok_or_else(|| RenderError::Surface("adapter cannot present to surface".into()))?;
        let caps = surface.get_capabilities(&gpu.adapter);
        if let Some(srgb) = caps.formats.iter().copied().find(|f| f.is_srgb()) {
            config.format = srgb;
        }
        let (present_mode, note) = options.present.choose(&caps.present_modes);
        config.present_mode = present_mode;
        // So that a frame can be copied out for screenshots (`capture_next_frame`),
        // only when asked for: the benchmark measures the normal surface.
        let can_capture = caps.usages.contains(wgpu::TextureUsages::COPY_SRC);
        if options.capture && can_capture {
            config.usage |= wgpu::TextureUsages::COPY_SRC;
        }
        let present = PresentInfo {
            mode: format!("{present_mode:?}"),
            note,
        };
        crate::gpu::trace("configure surface");
        surface.configure(&gpu.device, &config);
        crate::gpu::trace("surface ready");
        let format = config.format;
        Ok(Self::build(
            gpu,
            Target::Window { surface, config },
            format,
            (width, height),
            present,
        ))
    }

    /// Renderer drawing into an offscreen texture (screenshots, tests).
    pub fn headless(width: u32, height: u32, options: &GpuOptions) -> Result<Self, RenderError> {
        let gpu = Gpu::headless(options)?;
        let (width, height) = (width.max(1), height.max(1));
        let texture = offscreen_texture(&gpu.device, width, height);
        let present = PresentInfo {
            mode: "offscreen".into(),
            note: None,
        };
        Ok(Self::build(
            gpu,
            Target::Offscreen { texture },
            OFFSCREEN_FORMAT,
            (width, height),
            present,
        ))
    }

    fn build(
        gpu: Gpu,
        target: Target,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        present: PresentInfo,
    ) -> Self {
        let device = &gpu.device;
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = build_pipeline(device, &pipeline_layout, EMBEDDED_SHADER, format)
            .unwrap_or_else(|e| panic!("embedded shader scene.wgsl is invalid: {e}"));

        let camera = PlaceholderCamera::default();
        let lighting = Lighting::default();
        let globals = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("globals"),
            contents: bytemuck::bytes_of(&Globals::new(camera.view_proj(aspect(size)), &lighting)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals bind group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });

        // Strategic icons: a second small uniform (viewport and icon sizes)
        // next to the shared globals.
        let icon_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("icon params layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let icon_params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("icon params"),
            size: std::mem::size_of::<[f32; 4]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let icon_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("icon params bind group"),
            layout: &icon_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: icon_params.as_entire_binding(),
            }],
        });
        let icon_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("icon pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout), Some(&icon_layout)],
            immediate_size: 0,
        });
        let icon_pipeline = build_icon_pipeline(device, &icon_pipeline_layout, ICON_SHADER, format)
            .unwrap_or_else(|e| panic!("embedded shader icon.wgsl is invalid: {e}"));
        let icon_indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("icon quad indices"),
            contents: bytemuck::cast_slice(&[0_u16, 1, 2, 0, 2, 3]),
            usage: wgpu::BufferUsages::INDEX,
        });

        let meshes = MeshKind::ALL
            .iter()
            .map(|kind| {
                let data = match kind {
                    MeshKind::Plane => MeshData::plane(),
                    MeshKind::Cube => MeshData::cube(),
                    MeshKind::Pyramid => MeshData::pyramid(),
                    MeshKind::Cylinder => MeshData::cylinder(),
                    MeshKind::Cone => MeshData::cone(),
                };
                upload_mesh(device, &data)
            })
            .collect();
        let (instances, draws) = upload_scene(device, &test_scene());
        let depth_view = depth_texture(device, size.0, size.1);

        Self {
            gpu,
            target,
            format,
            size,
            depth_view,
            pipeline_layout,
            pipeline,
            globals,
            bind_group,
            globals_layout: bind_group_layout,
            terrain: None,
            meshes,
            instances,
            draws,
            dynamic: MeshKind::ALL
                .iter()
                .map(|_| DynamicInstances::default())
                .collect(),
            model_revision: 0,
            icon_pipeline,
            icon_params,
            icon_bind_group,
            icon_indices,
            icon_sizes: IconParams {
                size_px: 14.0,
                outline_px: 1.5,
            },
            icons: DynamicInstances::default(),
            camera,
            view_proj: None,
            lighting,
            last_stats: Cell::new(DrawStats::default()),
            debug_view: DebugView::default(),
            overlay: None,
            #[cfg(debug_assertions)]
            watch: None,
            present,
            gpu_timer: None,
            gpu_frame_tag: 0,
            allocs: AllocStats::default(),
            capture_requested: false,
            captured: None,
            acquire_wait: std::time::Duration::ZERO,
        }
    }

    /// How the window presents (the mode in use and why it differs from the
    /// request, if it does).
    pub fn present_info(&self) -> &PresentInfo {
        &self.present
    }

    /// Start timing the GPU side of every frame. Returns `false` when the
    /// device has no timestamp queries (the results then stay empty).
    pub fn enable_gpu_timing(&mut self) -> bool {
        if self.gpu_timer.is_none() {
            self.gpu_timer = GpuTimer::new(&self.gpu);
        }
        self.gpu_timer.is_some()
    }

    /// Number the frames submitted from now on (until the next call) with
    /// `tag`; [`Renderer::take_gpu_frame_times`] hands it back with each GPU
    /// time, so a result that arrives frames late can be credited to the
    /// frame it measured.
    pub fn set_gpu_frame_tag(&mut self, tag: u64) {
        self.gpu_frame_tag = tag;
    }

    /// GPU frame times that finished since the last call, as `(tag, ms)`
    /// with the tag from [`Renderer::set_gpu_frame_tag`] at the time the
    /// frame was submitted (a frame's result arrives a few frames late).
    pub fn take_gpu_frame_times(&mut self) -> Vec<(u64, f64)> {
        if let Some(timer) = &mut self.gpu_timer {
            let _ = self.gpu.device.poll(wgpu::PollType::Poll);
            timer.collect();
            return timer.take();
        }
        Vec::new()
    }

    /// How long the last frame waited for the window's swap chain to give
    /// it an image. With vsync this is the wait for the display; subtracted
    /// from a frame's wall time it leaves the CPU work.
    pub fn last_acquire_wait(&self) -> std::time::Duration {
        self.acquire_wait
    }

    /// GPU buffers created after start-up so far.
    pub fn alloc_stats(&self) -> AllocStats {
        self.allocs
    }

    /// Copy the next frame rendered to the window into an image, to be
    /// fetched with [`Renderer::take_capture`]. Works for the window only
    /// when the renderer was made with [`GpuOptions::capture`] and the
    /// surface allows copying out of it (otherwise nothing is captured). Offscreen renderers use [`Renderer::render_to_image`].
    pub fn capture_next_frame(&mut self) {
        self.capture_requested = true;
    }

    /// The image requested with [`Renderer::capture_next_frame`], once the
    /// frame has been rendered.
    pub fn take_capture(&mut self) -> Option<Rgba8> {
        self.captured.take()
    }

    /// The adapter in use, for logs.
    pub fn adapter_summary(&self) -> String {
        self.gpu.adapter_summary()
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    pub fn color_format(&self) -> wgpu::TextureFormat {
        self.format
    }

    pub fn is_offscreen(&self) -> bool {
        matches!(self.target, Target::Offscreen { .. })
    }

    /// Resize the target and depth buffer. A zero size (minimized window) is
    /// ignored.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 || (width, height) == self.size {
            return;
        }
        self.size = (width, height);
        match &mut self.target {
            Target::Window { surface, config } => {
                config.width = width;
                config.height = height;
                surface.configure(&self.gpu.device, config);
            }
            Target::Offscreen { texture } => {
                *texture = offscreen_texture(&self.gpu.device, width, height);
            }
        }
        self.depth_view = depth_texture(&self.gpu.device, width, height);
    }

    /// Replace the scene's shader. On a compile or pipeline error the old
    /// pipeline stays and the message is returned.
    pub fn reload_shader(&mut self, wgsl: &str) -> Result<(), String> {
        let pipeline = build_pipeline(&self.gpu.device, &self.pipeline_layout, wgsl, self.format)?;
        self.pipeline = pipeline;
        Ok(())
    }

    /// Debug builds: reload the shader whenever `path` changes on disk,
    /// checking at most every `interval`. Errors are logged and the previous
    /// pipeline keeps rendering. Does nothing in release builds.
    pub fn watch_shader_file(&mut self, path: std::path::PathBuf, interval: std::time::Duration) {
        #[cfg(debug_assertions)]
        {
            eprintln!("sf_render: hot-reloading shader {}", path.display());
            self.watch = Some(crate::shader_watch::ShaderWatch::new(path, interval));
        }
        #[cfg(not(debug_assertions))]
        let _ = (path, interval);
    }

    /// Check the watched shader file once and reload it if it changed.
    /// Returns `Some(result)` when a reload was attempted.
    pub fn poll_shader_reload(&mut self) -> Option<Result<(), String>> {
        #[cfg(debug_assertions)]
        {
            let text = self.watch.as_mut()?.poll()?;
            let name = self
                .watch
                .as_ref()
                .map(|w| w.path().display().to_string())
                .unwrap_or_default();
            let result = self.reload_shader(&text);
            match &result {
                Ok(()) => eprintln!("sf_render: reloaded shader {name}"),
                Err(e) => eprintln!("sf_render: shader {name} rejected, keeping old one:\n{e}"),
            }
            Some(result)
        }
        #[cfg(not(debug_assertions))]
        None
    }

    /// The device and queue, for crates that draw their own passes
    /// (`sf_ui` creates its pipeline and textures from these).
    pub fn gpu(&self) -> &Gpu {
        &self.gpu
    }

    /// Draw one frame: present to the window, or fill the offscreen texture.
    pub fn render(&mut self) -> Result<(), RenderError> {
        self.render_with(|_| {})
    }

    /// Like [`Renderer::render`], plus `overlay`: it is called after the scene
    /// pass with the frame's encoder and colour target, to record a pass on
    /// top (the player-facing UI of `sf_ui`). The egui overlay of
    /// [`Renderer::set_overlay`] is still painted after it. Draws recorded by
    /// the hook are not in [`DrawStats`] (the caller counts them).
    pub fn render_with(
        &mut self,
        overlay: impl FnOnce(&mut FrameTarget),
    ) -> Result<(), RenderError> {
        let _ = self.poll_shader_reload();
        self.prepare_frame();
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });

        let mut frame = None;
        let view = match &self.target {
            Target::Offscreen { texture } => {
                texture.create_view(&wgpu::TextureViewDescriptor::default())
            }
            Target::Window { surface, config } => {
                use wgpu::CurrentSurfaceTexture as C;
                let waiting = std::time::Instant::now();
                let acquired = surface.get_current_texture();
                self.acquire_wait = waiting.elapsed();
                match acquired {
                    C::Success(t) | C::Suboptimal(t) => {
                        let v = t
                            .texture
                            .create_view(&wgpu::TextureViewDescriptor::default());
                        frame = Some(t);
                        v
                    }
                    C::Outdated | C::Lost => {
                        surface.configure(&self.gpu.device, config);
                        return Ok(());
                    }
                    // Timeout, occluded or anything else: skip this frame.
                    _ => return Ok(()),
                }
            }
        };

        let slot = self
            .gpu_timer
            .as_mut()
            .and_then(|timer| timer.begin(&mut encoder));
        let extra = self.prepare_overlay(&mut encoder);
        self.draw(&mut encoder, &view);
        overlay(&mut FrameTarget {
            device: &self.gpu.device,
            queue: &self.gpu.queue,
            encoder: &mut encoder,
            view: &view,
            format: self.format,
            size: self.size,
        });
        self.paint_overlay(&mut encoder, &view);
        let readback = match (&frame, self.capture_requested) {
            (Some(t), true) => {
                self.capture_requested = false;
                let can = t.texture.usage().contains(wgpu::TextureUsages::COPY_SRC);
                can.then(|| queue_readback(&self.gpu.device, &mut encoder, &t.texture, self.size))
            }
            _ => None,
        };
        if let (Some(timer), Some(slot)) = (&mut self.gpu_timer, slot) {
            timer.end(&mut encoder, slot);
        }
        self.gpu
            .queue
            .submit(extra.into_iter().chain([encoder.finish()]));
        if let (Some(timer), Some(slot)) = (&mut self.gpu_timer, slot) {
            timer.submitted(slot, self.gpu_frame_tag);
        }
        if let Some(frame) = frame {
            self.gpu.queue.present(frame);
        }
        if let Some(readback) = readback {
            let bgra = matches!(
                self.format,
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
            );
            self.captured = finish_readback(&self.gpu.device, readback, self.size, bgra).ok();
        }
        if let Some(overlay) = &mut self.overlay {
            overlay.free_pending();
        }
        Ok(())
    }

    /// Render and read the offscreen texture back as sRGB RGBA8.
    pub fn render_to_image(&mut self) -> Result<Rgba8, RenderError> {
        self.render_to_image_with(|_| {})
    }

    /// Like [`Renderer::render_to_image`], with the same `overlay` hook as
    /// [`Renderer::render_with`] (the UI screenshot tests use it).
    pub fn render_to_image_with(
        &mut self,
        overlay: impl FnOnce(&mut FrameTarget),
    ) -> Result<Rgba8, RenderError> {
        let Target::Offscreen { .. } = self.target else {
            return Err(RenderError::Readback(
                "render_to_image needs a headless renderer".into(),
            ));
        };
        self.prepare_frame();
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("screenshot"),
            });
        let extra = self.prepare_overlay(&mut encoder);
        let Target::Offscreen { texture } = &self.target else {
            unreachable!("checked above");
        };
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.draw(&mut encoder, &view);
        overlay(&mut FrameTarget {
            device: &self.gpu.device,
            queue: &self.gpu.queue,
            encoder: &mut encoder,
            view: &view,
            format: self.format,
            size: self.size,
        });
        self.paint_overlay(&mut encoder, &view);

        let Target::Offscreen { texture } = &self.target else {
            unreachable!("checked above");
        };
        let readback = queue_readback(&self.gpu.device, &mut encoder, texture, self.size);
        self.gpu
            .queue
            .submit(extra.into_iter().chain([encoder.finish()]));
        if let Some(overlay) = &mut self.overlay {
            overlay.free_pending();
        }
        finish_readback(&self.gpu.device, readback, self.size, false)
    }

    /// Use this view-projection (wgpu clip space, depth `0..1`) instead of the
    /// fixed placeholder camera. The caller rebuilds it when the size changes.
    pub fn set_view_proj(&mut self, view_proj: Mat4) {
        self.view_proj = Some(view_proj);
    }

    /// Replace the scene (all objects are re-uploaded; call on scene changes,
    /// not every frame).
    pub fn set_scene(&mut self, objects: &[SceneObject]) {
        let (instances, draws) = upload_scene(&self.gpu.device, objects);
        self.instances = instances;
        self.draws = draws;
    }

    /// Replace the per-frame instances of one mesh kind (units): one instance
    /// buffer per mesh kind, one draw call. Cheap enough to call every frame;
    /// the buffer grows when needed and is reused otherwise. An empty slice
    /// hides the kind.
    pub fn set_instances(&mut self, mesh: MeshKind, instances: &[Instance]) {
        self.set_mesh_instances(MeshId::of(mesh), instances);
    }

    /// Like [`Renderer::set_instances`] for any entry of the mesh table: a
    /// mesh kind at a level of detail, or a mesh registered with
    /// [`Renderer::set_model_meshes`]. An id that is not in the table is
    /// ignored.
    pub fn set_mesh_instances(&mut self, mesh: MeshId, instances: &[Instance]) {
        if let Some(slot) = self.dynamic.get_mut(mesh.index()) {
            fill_instances(
                &self.gpu,
                &mut self.allocs,
                slot,
                "unit instances",
                instances,
            );
        }
    }

    /// Replace the meshes after the built-in kinds (ids `BUILTIN_MESHES..`):
    /// the lower levels of detail of the placeholder shapes, later the glTF
    /// models. Their instance lists start empty. Call when the model set
    /// changes, not every frame; [`Renderer::upload_units`] does it when
    /// needed.
    pub fn set_model_meshes(&mut self, meshes: &[MeshData]) {
        self.meshes.truncate(BUILTIN_MESHES);
        self.meshes
            .extend(meshes.iter().map(|m| upload_mesh(&self.gpu.device, m)));
        self.dynamic.truncate(BUILTIN_MESHES);
        self.dynamic
            .resize_with(self.meshes.len(), DynamicInstances::default);
    }

    /// Replace the strategic icons of the frame: one instance buffer and one
    /// draw call, drawn over the scene without a depth test. `params` are the
    /// icon sizes in physical pixels (`Instancer::icon_params`). An empty
    /// slice draws no icons.
    pub fn set_icons(&mut self, icons: &[IconInstance], params: IconParams) {
        self.icon_sizes = params;
        fill_instances(
            &self.gpu,
            &mut self.allocs,
            &mut self.icons,
            "icon instances",
            icons,
        );
    }

    /// Upload everything the instancer built for the frame: the extra model
    /// meshes if the model set changed, every instance list (one draw call
    /// each) and the icons.
    pub fn upload_units(&mut self, units: &Instancer) {
        self.sync_models(units);
        for id in 0..units.models().mesh_count() {
            let id = MeshId(id as u16);
            self.set_mesh_instances(id, units.mesh_instances(id));
        }
        self.set_icons(units.icon_instances(), units.icon_params());
    }

    /// Upload the model set's extra meshes if the set changed.
    fn sync_models(&mut self, units: &Instancer) {
        let models = units.models();
        if self.model_revision != models.revision() {
            self.set_model_meshes(models.extra_meshes());
            self.model_revision = models.revision();
        }
    }

    /// Allocate every unit instance buffer at the largest size the
    /// instancer's entities can ever need ([`Instancer::mesh_capacity`]),
    /// once, at load. Without it a buffer is created on the first frame its
    /// list is not empty and again whenever the list outgrows it, which is
    /// what happens when the camera first zooms out over the whole crowd
    /// (progress.md follow-up 14). Call again if the number of entities grows.
    pub fn reserve_units(&mut self, units: &Instancer) {
        self.sync_models(units);
        for (index, &count) in units.mesh_capacity().iter().enumerate() {
            if count > 0
                && let Some(slot) = self.dynamic.get_mut(index)
            {
                let bytes = u64::from(count) * std::mem::size_of::<Instance>() as u64;
                reserve_instances(&self.gpu, &mut self.allocs, slot, "unit instances", bytes);
            }
        }
        let bytes = units.len() as u64 * std::mem::size_of::<IconInstance>() as u64;
        reserve_instances(
            &self.gpu,
            &mut self.allocs,
            &mut self.icons,
            "icon instances",
            bytes,
        );
    }

    /// Draw-call counters of the last rendered frame (all zero before the
    /// first). They are counted at the `draw_indexed` calls themselves.
    pub fn draw_stats(&self) -> DrawStats {
        self.last_stats.get()
    }

    /// Choose the terrain debug visualisations (LOD tint, cell grid).
    /// Cheap enough to call every frame.
    pub fn set_debug_view(&mut self, view: DebugView) {
        // Unchanged: no uniform write (`set_terrain` re-applies the view).
        if view == self.debug_view {
            return;
        }
        self.debug_view = view;
        if let Some(t) = &self.terrain {
            t.write_debug_view(&self.gpu.queue, view);
        }
    }

    /// Hand over the egui output for the next frame(s): drawn on top of the
    /// scene by [`Renderer::render`] and [`Renderer::render_to_image`] until
    /// replaced or cleared. The painter is created on the first call.
    pub fn set_overlay(&mut self, frame: OverlayFrame) {
        let (device, format) = (&self.gpu.device, self.format);
        self.overlay
            .get_or_insert_with(|| Overlay::new(device, format))
            .set(device, &self.gpu.queue, frame);
    }

    /// Stop drawing the overlay (the panels are off): no extra render pass.
    pub fn clear_overlay(&mut self) {
        if let Some(o) = &mut self.overlay {
            o.clear();
        }
    }

    /// Show a terrain with the shipped settings (`assets/config/terrain.ron`):
    /// builds the chunk meshes of all levels of detail, the material texture
    /// array and the material index texture from `map` and draws it before
    /// the scene objects. Replaces a previous terrain. Call on map changes,
    /// not every frame.
    pub fn set_terrain(&mut self, map: &sf_data::map::Map) {
        self.set_terrain_with(map, &TerrainConfig::embedded());
    }

    /// Like [`Renderer::set_terrain`] with explicit settings.
    pub fn set_terrain_with(&mut self, map: &sf_data::map::Map, config: &TerrainConfig) {
        self.terrain = Some(crate::terrain_gpu::build(
            &self.gpu.device,
            &self.gpu.queue,
            &self.globals_layout,
            self.format,
            DEPTH_FORMAT,
            map,
            config,
        ));
        if let Some(t) = &self.terrain {
            t.write_debug_view(&self.gpu.queue, self.debug_view);
        }
    }

    /// The level of detail settings in use (`None` without terrain).
    pub fn terrain_lod(&self) -> Option<LodConfig> {
        self.terrain.as_ref().map(|t| t.config.lod)
    }

    /// Change the level of detail settings live: error budget, hysteresis,
    /// draw-call cost, forced level. The tint by level is
    /// [`DebugView::lod_tint`], the skirts (`seams`) are part of the mesh and
    /// change only with [`Renderer::set_terrain_with`].
    pub fn set_terrain_lod(&mut self, lod: LodConfig) {
        if let Some(t) = &mut self.terrain {
            t.config.lod = lod;
        }
    }

    /// Debug and tests: draw chunk `i` at level `levels[i]` (clamped to what
    /// the chunk has) instead of selecting by error; `None` goes back to the
    /// automatic selection. Chunks outside the view are still culled.
    pub fn set_terrain_levels(&mut self, levels: Option<Vec<u8>>) {
        if let Some(t) = &mut self.terrain {
            t.level_override = levels;
        }
    }

    /// Terrain counters of the last frame (all zero without terrain or before
    /// the first frame).
    pub fn terrain_stats(&self) -> TerrainStats {
        self.terrain
            .as_ref()
            .map(|t| t.planner.selection().stats)
            .unwrap_or_default()
    }

    /// Remove the terrain.
    pub fn clear_terrain(&mut self) {
        self.terrain = None;
    }

    /// Number of terrain chunks (0 without terrain); the visible ones are
    /// drawn each frame, see [`Renderer::terrain_stats`].
    pub fn terrain_chunk_count(&self) -> usize {
        self.terrain.as_ref().map_or(0, |t| t.planner.chunk_count())
    }

    /// Upload the overlay's buffers; returns command buffers its paint
    /// callbacks want submitted first (none for plain egui).
    fn prepare_overlay(&mut self, encoder: &mut wgpu::CommandEncoder) -> Vec<wgpu::CommandBuffer> {
        match &mut self.overlay {
            Some(o) if o.has_jobs() => {
                o.prepare(&self.gpu.device, &self.gpu.queue, encoder, self.size)
            }
            _ => Vec::new(),
        }
    }

    fn current_view_proj(&self) -> Mat4 {
        self.view_proj
            .unwrap_or_else(|| self.camera.view_proj(aspect(self.size)))
    }

    /// Everything that depends on the camera and happens before drawing: the
    /// globals and the terrain's level selection and culling.
    fn prepare_frame(&mut self) {
        let view_proj = self.current_view_proj();
        let globals = Globals::new(view_proj, &self.lighting);
        self.gpu
            .queue
            .write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
        // Icons: viewport, icon side and outline width, all in pixels.
        let icon = [
            self.size.0 as f32,
            self.size.1 as f32,
            self.icon_sizes.size_px,
            self.icon_sizes.outline_px,
        ];
        self.gpu
            .queue
            .write_buffer(&self.icon_params, 0, bytemuck::bytes_of(&icon));
        if let Some(t) = &mut self.terrain {
            let view = LodView::from_view_proj(view_proj, self.size.1 as f32);
            let frustum = Frustum::from_view_proj(view_proj);
            t.planner
                .select(&view, &frustum, &t.config.lod, t.level_override.as_deref());
        }
    }

    /// Record the scene pass and count the draw calls into `last_stats`.
    fn draw(&self, encoder: &mut wgpu::CommandEncoder, color: &wgpu::TextureView) {
        let mut pass = CountedPass::new(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(CLEAR),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        }));
        if let Some(t) = &self.terrain {
            pass.set_pipeline(&t.pipeline);
            pass.set_bind_group(0, &self.bind_group);
            pass.set_bind_group(1, &t.bind_group);
            pass.set_vertex_buffer(0, t.vertices.slice(..));
            pass.set_index_buffer(t.indices.slice(..), wgpu::IndexFormat::Uint32);
            // One call per run of chunks with the same level (see `lod`).
            for range in &t.planner.selection().draws {
                pass.draw(DrawGroup::Terrain, range.clone(), 0..1);
            }
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group);
        // An empty instance buffer cannot be bound, so skip when there is nothing.
        if !self.draws.is_empty() {
            pass.set_vertex_buffer(1, self.instances.slice(..));
            for (mesh, range) in &self.draws {
                let mesh = &self.meshes[*mesh];
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw(DrawGroup::Scene, 0..mesh.index_count, range.clone());
            }
        }
        // Units: one buffer and one draw call per mesh kind.
        for (index, slot) in self.dynamic.iter().enumerate() {
            let (Some(buffer), true) = (&slot.buffer, slot.count > 0) else {
                continue;
            };
            let mesh = &self.meshes[index];
            // An empty mesh (a glTF part that is only a frame) cannot be bound.
            if mesh.index_count == 0 {
                continue;
            }
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_vertex_buffer(1, buffer.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw(DrawGroup::Units, 0..mesh.index_count, 0..slot.count);
        }
        // Strategic icons last, over everything, one instanced quad each.
        if let (Some(buffer), true) = (&self.icons.buffer, self.icons.count > 0) {
            pass.set_pipeline(&self.icon_pipeline);
            pass.set_bind_group(0, &self.bind_group);
            pass.set_bind_group(1, &self.icon_bind_group);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.set_index_buffer(self.icon_indices.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw(DrawGroup::Icons, 0..6, 0..self.icons.count);
        }
        let mut stats = pass.finish();
        if let Some(t) = &self.terrain {
            // Which chunks were culled is the planner's business, not a draw call's.
            let chunks = t.planner.selection().stats;
            stats.terrain_chunks_total = chunks.chunks_total;
            stats.terrain_chunks_visible = chunks.chunks_visible;
            stats.terrain_chunks_per_level = chunks.per_level;
        }
        self.last_stats.set(stats);
    }

    /// The egui overlay pass, last: scene, then the `render_with` hook
    /// (`sf_ui`), then this, so the debug panels stay on top of the HUD.
    fn paint_overlay(&self, encoder: &mut wgpu::CommandEncoder, color: &wgpu::TextureView) {
        if let Some(overlay) = self.overlay.as_ref().filter(|o| o.has_jobs()) {
            overlay.paint(encoder, color, self.size);
        }
    }
}

/// Which counter a draw call belongs to.
#[derive(Clone, Copy)]
enum DrawGroup {
    Terrain,
    Scene,
    Units,
    Icons,
}

/// A render pass whose only way to draw is [`CountedPass::draw`], which
/// issues the `draw_indexed` and counts it in one place: a draw cannot be
/// added, removed or skipped without the counters following. The inner pass
/// is private and only the state-setting calls are forwarded.
struct CountedPass<'a> {
    pass: wgpu::RenderPass<'a>,
    stats: DrawStats,
}

impl<'a> CountedPass<'a> {
    fn new(pass: wgpu::RenderPass<'a>) -> Self {
        Self {
            pass,
            stats: DrawStats::default(),
        }
    }

    fn set_pipeline(&mut self, pipeline: &wgpu::RenderPipeline) {
        self.pass.set_pipeline(pipeline);
    }

    fn set_bind_group(&mut self, index: u32, group: &wgpu::BindGroup) {
        self.pass.set_bind_group(index, group, &[]);
    }

    fn set_vertex_buffer(&mut self, slot: u32, buffer: wgpu::BufferSlice<'_>) {
        self.pass.set_vertex_buffer(slot, buffer);
    }

    fn set_index_buffer(&mut self, buffer: wgpu::BufferSlice<'_>, format: wgpu::IndexFormat) {
        self.pass.set_index_buffer(buffer, format);
    }

    fn draw(&mut self, group: DrawGroup, indices: Range<u32>, instances: Range<u32>) {
        let n = instances.end - instances.start;
        let triangles = (indices.end - indices.start) / 3 * n;
        self.pass.draw_indexed(indices, 0, instances);
        let s = &mut self.stats;
        match group {
            DrawGroup::Terrain => {
                s.terrain_draw_calls += 1;
                s.terrain_triangles += triangles;
            }
            DrawGroup::Scene => {
                s.scene_draw_calls += 1;
                s.scene_instances += n;
            }
            DrawGroup::Units => {
                s.dynamic_draw_calls += 1;
                s.dynamic_instances += n;
                s.dynamic_triangles += triangles;
            }
            DrawGroup::Icons => {
                s.icon_draw_calls += 1;
                s.icon_instances += n;
            }
        }
    }

    fn finish(self) -> DrawStats {
        self.stats
    }
}

/// A texture copy waiting to be mapped (see [`queue_readback`]).
struct Readback {
    buffer: wgpu::Buffer,
    padded_row: u32,
}

/// Record a copy of `texture` (4 bytes per pixel) into a buffer that can be
/// mapped; call [`finish_readback`] after the encoder was submitted.
fn queue_readback(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    (width, height): (u32, u32),
) -> Readback {
    let unpadded = width * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_row = unpadded.div_ceil(align) * align;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(padded_row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    Readback { buffer, padded_row }
}

/// Wait for a submitted [`Readback`] and return its pixels as RGBA
/// (`bgra`: the texture stores blue first, as a window surface often does).
fn finish_readback(
    device: &wgpu::Device,
    readback: Readback,
    (width, height): (u32, u32),
    bgra: bool,
) -> Result<Rgba8, RenderError> {
    let (tx, rx) = std::sync::mpsc::channel();
    readback
        .buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| RenderError::Readback(e.to_string()))?;
    rx.recv()
        .map_err(|e| RenderError::Readback(e.to_string()))?
        .map_err(|e| RenderError::Readback(e.to_string()))?;
    let mapped = readback
        .buffer
        .slice(..)
        .get_mapped_range()
        .map_err(|e| RenderError::Readback(e.to_string()))?;
    let unpadded = width as usize * 4;
    let mut pixels = Vec::with_capacity(unpadded * height as usize);
    for row in mapped.chunks_exact(readback.padded_row as usize) {
        pixels.extend_from_slice(&row[..unpadded]);
    }
    drop(mapped);
    readback.buffer.unmap();
    if bgra {
        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
    }
    Ok(Rgba8::new(width, height, pixels))
}

fn aspect((w, h): (u32, u32)) -> f32 {
    w as f32 / h.max(1) as f32
}

fn offscreen_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen color"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OFFSCREEN_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn depth_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

fn upload_mesh(device: &wgpu::Device, data: &MeshData) -> GpuMesh {
    GpuMesh {
        vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh vertices"),
            contents: bytemuck::cast_slice(&data.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh indices"),
            contents: bytemuck::cast_slice(&data.indices),
            usage: wgpu::BufferUsages::INDEX,
        }),
        index_count: data.indices.len() as u32,
    }
}

/// One instance buffer for the whole scene, grouped by mesh kind.
fn upload_scene(
    device: &wgpu::Device,
    objects: &[SceneObject],
) -> (wgpu::Buffer, Vec<(usize, Range<u32>)>) {
    let mut instances: Vec<Instance> = Vec::new();
    let mut draws = Vec::new();
    for kind in MeshKind::ALL {
        let start = instances.len() as u32;
        instances.extend(
            objects
                .iter()
                .filter(|o| o.mesh == kind)
                .map(SceneObject::instance),
        );
        let end = instances.len() as u32;
        if end > start {
            draws.push((kind.index(), start..end));
        }
    }
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("instances"),
        contents: bytemuck::cast_slice(&instances),
        usage: wgpu::BufferUsages::VERTEX,
    });
    (buffer, draws)
}

/// Make sure the buffer of `slot` holds at least `bytes` (see
/// [`Renderer::reserve_units`]); the count is left alone.
fn reserve_instances(
    gpu: &Gpu,
    allocs: &mut AllocStats,
    slot: &mut DynamicInstances,
    label: &'static str,
    bytes: u64,
) {
    if bytes == 0 || (slot.buffer.is_some() && slot.capacity >= bytes) {
        return;
    }
    let capacity = bytes.next_multiple_of(4096);
    slot.buffer = Some(gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: capacity,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    }));
    slot.capacity = capacity;
    allocs.instance_buffers += 1;
    allocs.instance_bytes += capacity;
}

/// Write `items` into a growing per-frame instance buffer and set its count.
fn fill_instances<T: bytemuck::Pod>(
    gpu: &Gpu,
    allocs: &mut AllocStats,
    slot: &mut DynamicInstances,
    label: &'static str,
    items: &[T],
) {
    slot.count = items.len() as u32;
    if items.is_empty() {
        return;
    }
    let bytes: &[u8] = bytemuck::cast_slice(items);
    let needed = bytes.len() as u64;
    if slot.buffer.is_none() || slot.capacity < needed {
        // Grow with headroom so a crowd changing size does not reallocate every frame.
        let capacity = (needed + needed / 4).next_multiple_of(4096);
        slot.buffer = Some(gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        slot.capacity = capacity;
        allocs.instance_buffers += 1;
        allocs.instance_bytes += capacity;
    }
    if let Some(buffer) = &slot.buffer {
        gpu.queue.write_buffer(buffer, 0, bytes);
    }
}

/// Mesh vertex: position, normal, material colour with the team weight
/// (location 7, after the instance attributes).
const VERTEX_ATTRS: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 7 => Float32x4];
/// An icon instance: world anchor, shape and flags, color with the model weight.
const ICON_ATTRS: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Uint32, 2 => Float32x4];
const INSTANCE_ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
    2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4
];
/// The model instances: one step per instance, so every vertex of an instance
/// reads the same matrix and color, and with it the same cross-fade weight
/// (`color.a`, location 6).
const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: std::mem::size_of::<Instance>() as u64,
    step_mode: wgpu::VertexStepMode::Instance,
    attributes: &INSTANCE_ATTRS,
};

/// Compile `wgsl` and build the scene pipeline. Validation errors (WGSL
/// syntax, types, binding mismatches) come back as `Err` instead of
/// aborting, so a hot-reload typo cannot crash the game.
fn build_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    wgsl: &str,
    format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, String> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("scene.wgsl"),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("scene pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[
                Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &VERTEX_ATTRS,
                }),
                Some(INSTANCE_LAYOUT),
            ],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    match pollster::block_on(scope.pop()) {
        Some(e) => Err(e.to_string()),
        None => Ok(pipeline),
    }
}

/// The strategic icon pipeline: instance data only (the quad's corners come
/// from the vertex index), alpha blending for the antialiased edge, no depth
/// test and no depth write, no culling.
fn build_icon_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    wgsl: &str,
    format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, String> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("icon.wgsl"),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("icon pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<IconInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &ICON_ATTRS,
            })],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    match pollster::block_on(scope.pop()) {
        Some(e) => Err(e.to_string()),
        None => Ok(pipeline),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The model's cross-fade weight is the alpha of the instance color: it
    /// must step per instance (every vertex of an instance carries the same
    /// weight) and reach the fragment shader flat, as in icon.wgsl, so the
    /// two dither tests see the identical number.
    #[test]
    fn the_model_weight_is_per_instance_and_flat() {
        assert_eq!(INSTANCE_LAYOUT.step_mode, wgpu::VertexStepMode::Instance);
        let color = INSTANCE_LAYOUT
            .attributes
            .iter()
            .find(|a| a.shader_location == 6)
            .expect("the color attribute");
        assert_eq!(color.offset, std::mem::offset_of!(Instance, color) as u64);
        assert_eq!(color.format, wgpu::VertexFormat::Float32x4);
        let flat = |src: &str| {
            src.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .contains("@interpolate(flat) weight: f32")
        };
        let scene = include_str!("../../../assets/shaders/scene.wgsl");
        assert!(scene.contains("out.weight = in.color.a;"));
        assert!(scene.contains("@location(6) color: vec4<f32>"));
        assert!(flat(scene), "scene.wgsl weight is flat");
        assert!(flat(include_str!("../../../assets/shaders/icon.wgsl")));
    }

    /// Every `draw_indexed` goes through `CountedPass::draw`, which counts it
    /// next to the call. If someone records a draw anywhere else (or splits
    /// the counting from the call again) this fails.
    #[test]
    fn there_is_exactly_one_draw_indexed_call_site() {
        let source = include_str!("renderer.rs");
        let production = source.split("#[cfg(test)]").next().unwrap();
        let needle = concat!(".draw", "_indexed(");
        assert_eq!(production.matches(needle).count(), 1);
        let helper = production.split("fn draw(&mut self, group").nth(1).unwrap();
        let body = helper.split("fn finish").next().unwrap();
        assert!(
            body.contains(needle),
            "the call site is the counting helper"
        );
        assert!(
            body.contains("draw_calls += 1"),
            "and it counts right there"
        );
    }
}
